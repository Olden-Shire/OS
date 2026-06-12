//! Per-socket connection state machine — handshake → login → js5/game.
//! Mirrors the Engine2007 reference Login.ts/NetworkPlayer.ts flow
//! (the rev1 client currently runs NO_ISAAC, like the reference).

use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;

use io::packet::Packet;

pub const REVISION: i32 = 1;

#[derive(Debug, PartialEq, Eq)]
pub enum ConnState {
    Handshake,
    Js5,
    Game { pid: usize },
    Closed,
}

pub struct Connection {
    pub stream: TcpStream,
    pub state: ConnState,
    pub inbuf: Vec<u8>,
    pub outbuf: Vec<u8>,
    // Game packet pump state.
    pub packet_type: i32,
    pub packet_size: i32,
    /// Some(..) once the connection is identified as WebSocket (the wasm
    /// client — browsers can't open raw TCP). None = plain TCP. Unset
    /// until the first bytes arrive.
    pub ws: Option<crate::websocket::WsState>,
    /// First-bytes sniff done (TCP vs WS decided).
    sniffed: bool,
}

impl Connection {
    pub fn new(stream: TcpStream) -> Connection {
        stream.set_nonblocking(true).ok();
        stream.set_nodelay(true).ok();
        Connection {
            stream,
            state: ConnState::Handshake,
            inbuf: Vec::new(),
            outbuf: Vec::new(),
            packet_type: -1,
            packet_size: 0,
            ws: None,
            sniffed: false,
        }
    }

    /// Pull whatever's available off the socket into `inbuf`. WebSocket
    /// connections de-frame through WsState; plain TCP appends directly.
    pub fn fill(&mut self) {
        let mut tmp = [0u8; 8192];
        loop {
            match self.stream.read(&mut tmp) {
                Ok(0) => {
                    self.state = ConnState::Closed;
                    return;
                }
                Ok(n) => {
                    let bytes = &tmp[..n];
                    if !self.sniffed {
                        self.sniffed = true;
                        // No game/js5 handshake opcode is 'G' (0x47), so a
                        // leading "GET " can only be an HTTP/WS upgrade.
                        if bytes.starts_with(b"G") {
                            self.ws = Some(crate::websocket::WsState::default());
                        }
                    }
                    match self.ws.as_mut() {
                        Some(ws) => {
                            let payload = ws.ingest(bytes, &mut self.outbuf);
                            self.inbuf.extend_from_slice(&payload);
                            if ws.closed {
                                self.flush();
                                self.state = ConnState::Closed;
                                return;
                            }
                        }
                        None => self.inbuf.extend_from_slice(bytes),
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => return,
                Err(_) => {
                    self.state = ConnState::Closed;
                    return;
                }
            }
        }
    }

    /// Flush as much of `outbuf` as the socket accepts.
    pub fn flush(&mut self) {
        while !self.outbuf.is_empty() {
            match self.stream.write(&self.outbuf) {
                Ok(0) => {
                    self.state = ConnState::Closed;
                    return;
                }
                Ok(n) => {
                    self.outbuf.drain(..n);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => return,
                Err(_) => {
                    self.state = ConnState::Closed;
                    return;
                }
            }
        }
    }

    pub fn write(&mut self, data: &[u8]) {
        // outbuf holds final wire bytes (ingest queues the HTTP 101 reply
        // there directly), so payload writes frame here. The server never
        // writes before the client's first protocol bytes arrive, so the
        // handshake is always done by the time this runs in WS mode.
        match self.ws.as_ref() {
            Some(ws) if ws.active => {
                let framed = ws.frame_out(data);
                self.outbuf.extend_from_slice(&framed);
            }
            _ => self.outbuf.extend_from_slice(data),
        }
    }

    fn consume(&mut self, n: usize) -> Vec<u8> {
        self.inbuf.drain(..n).collect()
    }

    /// Handle the pre-login handshake opcodes. Returns Some(login)
    /// when a full login block was parsed.
    pub fn process_handshake(&mut self) -> Option<LoginRequest> {
        loop {
            if self.state != ConnState::Handshake || self.inbuf.is_empty() {
                return None;
            }
            let opcode = self.inbuf[0];
            match opcode {
                14 => {
                    // Session begin — reply 0, await login block.
                    self.consume(1);
                    self.write(&[0]);
                }
                15 => {
                    // JS5 handshake: u32 revision.
                    if self.inbuf.len() < 5 {
                        return None;
                    }
                    let body = self.consume(5);
                    let mut p = Packet::from_vec(body);
                    p.pos = 1;
                    let revision = p.g4();
                    if revision != REVISION {
                        self.write(&[6]);
                        self.flush();
                        self.state = ConnState::Closed;
                        return None;
                    }
                    self.write(&[0]);
                    self.state = ConnState::Js5;
                }
                16 | 18 => {
                    // Login: u16 block length then the block.
                    if self.inbuf.len() < 3 {
                        return None;
                    }
                    let len = ((self.inbuf[1] as usize) << 8) | self.inbuf[2] as usize;
                    if self.inbuf.len() < 3 + len {
                        return None;
                    }
                    let reconnect = opcode == 18;
                    let body = self.consume(3 + len);
                    let mut p = Packet::from_vec(body);
                    p.pos = 3;

                    let revision = p.g4();
                    if revision != REVISION {
                        self.write(&[6]);
                        self.flush();
                        self.state = ConnState::Closed;
                        return None;
                    }

                    let _isaac10 = p.g1();
                    let mut seeds = [0i32; 4];
                    for s in &mut seeds {
                        *s = p.g4();
                    }
                    let _isaac0 = p.g8();
                    let password = p.gjstr();
                    let rsa_len = p.g2() as usize;
                    p.pos += rsa_len;
                    let username = p.gjstr();
                    let low_memory = p.g1() == 1; // 1 = low-detail client
                    p.pos += 24; // uid dat
                    // 16 per-archive client CRCs follow — unchecked
                    // like the reference.

                    return Some(LoginRequest { username, password, reconnect, seeds, low_memory });
                }
                _ => {
                    self.state = ConnState::Closed;
                    return None;
                }
            }
        }
    }
}

pub struct LoginRequest {
    pub username: String,
    pub password: String,
    pub reconnect: bool,
    pub seeds: [i32; 4],
    pub low_memory: bool,
}
