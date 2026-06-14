// @ObfuscatedName("am")
//
// jagex3.io.ClientStream — wraps a TCP Socket. In the gamepack reads are
// blocking on the calling thread and writes are buffered in a 5000-byte
// ring fed by a dedicated writer thread (started via SignLink). For the
// port we keep the same field layout but split writes off into a Rust
// thread that drains the ring straight onto its own try_clone()'d socket
// handle so the read path can block without holding the write path off.
//
// On wasm the same API rides a browser WebSocket (io::ws_socket): incoming
// frames land in a byte queue, available() is its length, and writes go
// straight out (the browser buffers sends) — no threads. Every caller
// gates reads on available(), so the queue never under-runs in practice.

#![allow(dead_code)]

#[cfg(not(target_arch = "wasm32"))]
use std::io::{Read, Write};
#[cfg(not(target_arch = "wasm32"))]
use std::net::{Shutdown, TcpStream};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Arc, Condvar, Mutex};
#[cfg(not(target_arch = "wasm32"))]
use std::thread::{self, JoinHandle};

// java: java.io.IOException
#[derive(Debug)]
pub struct IoError;

impl std::fmt::Display for IoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "IOException")
    }
}

impl std::error::Error for IoError {}

impl From<std::io::Error> for IoError {
    fn from(_: std::io::Error) -> Self {
        Self
    }
}

// ── wasm: WebSocket-backed ClientStream ─────────────────────────────────

#[cfg(target_arch = "wasm32")]
pub struct ClientStream {
    pub sock: crate::io::ws_socket::WsSocket,
    // @ObfuscatedName("am.m") — closed flag, same semantics as native.
    pub dummy: bool,
}

#[cfg(target_arch = "wasm32")]
impl ClientStream {
    pub fn new(sock: crate::io::ws_socket::WsSocket) -> Result<Self, IoError> {
        Ok(Self { sock, dummy: false })
    }

    pub fn close(&mut self) {
        if self.dummy {
            return;
        }
        self.dummy = true;
        self.sock.close();
    }

    pub fn read_byte(&mut self) -> Result<i32, IoError> {
        if self.dummy {
            return Ok(0);
        }
        let mut b = [0u8; 1];
        if self.sock.read_into(&mut b) == 1 {
            Ok(b[0] as i32)
        } else if self.sock.is_dead() {
            Ok(-1) // EOF, like the native read of a closed socket
        } else {
            Err(IoError) // ungated read — callers poll available() first
        }
    }

    pub fn available(&mut self) -> Result<i32, IoError> {
        if self.dummy {
            return Ok(0);
        }
        if self.sock.available() == 0 && self.sock.is_dead() {
            return Err(IoError);
        }
        Ok(self.sock.available() as i32)
    }

    pub fn read(&mut self, arg0: &mut [u8], arg1: i32, arg2: i32) -> Result<(), IoError> {
        if self.dummy {
            return Ok(());
        }
        let (off, len) = (arg1 as usize, arg2 as usize);
        if self.sock.read_into(&mut arg0[off..off + len]) == len {
            Ok(())
        } else {
            Err(IoError)
        }
    }

    pub fn write(&mut self, arg0: &[u8], arg1: i32, arg2: i32) -> Result<(), IoError> {
        if self.dummy {
            return Ok(());
        }
        if self.sock.is_dead() {
            return Err(IoError);
        }
        let (off, len) = (arg1 as usize, arg2 as usize);
        self.sock.send(&arg0[off..off + len]).map_err(|_| IoError)
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for ClientStream {
    fn drop(&mut self) {
        self.close();
    }
}

// ── native: TCP-backed ClientStream ─────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
pub struct WriterRing {
    pub buf: Mutex<Vec<u8>>,
    pub head: AtomicUsize,
    pub tail: AtomicUsize,
    pub closed: AtomicBool,
    pub cv: Condvar,
}

#[cfg(not(target_arch = "wasm32"))]
pub struct ClientStream {
    // @ObfuscatedName("am.r") — InputStream side (blocking reads). Owned
    // exclusively by the calling thread; no mutex so a blocking read
    // never holds the write side off.
    pub read_stream: TcpStream,

    // @ObfuscatedName("am.d") — OutputStream side, owned by the writer
    // thread via a try_clone()'d handle.
    _out: (),

    // @ObfuscatedName("am.l") — the underlying Socket peer address.
    pub socket_addr: Option<std::net::SocketAddr>,

    // @ObfuscatedName("am.m")
    pub dummy: bool,

    // @ObfuscatedName("am.c") — SignLink; elided.
    _signlink: (),

    // @ObfuscatedName("am.n") — PrivilegedRequest for the writer thread.
    pub writer: Option<JoinHandle<()>>,

    // @ObfuscatedName("am.j")
    pub buf: Option<Arc<WriterRing>>,

    // @ObfuscatedName("am.z")
    pub tcyl: i32,

    // @ObfuscatedName("am.g")
    pub tnum: i32,

    // @ObfuscatedName("am.q")
    pub ioerror: Arc<AtomicBool>,
}

#[cfg(not(target_arch = "wasm32"))]
impl ClientStream {
    pub fn new(socket: TcpStream) -> Result<Self, IoError> {
        socket.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
        socket.set_nodelay(true)?;
        let addr = socket.peer_addr().ok();
        Ok(Self {
            read_stream: socket,
            _out: (),
            socket_addr: addr,
            dummy: false,
            _signlink: (),
            writer: None,
            buf: None,
            tcyl: 0,
            tnum: 0,
            ioerror: Arc::new(AtomicBool::new(false)),
        })
    }

    // @ObfuscatedName("am.m(I)V")
    pub fn close(&mut self) {
        if self.dummy {
            return;
        }
        self.dummy = true;
        if let Some(ring) = &self.buf {
            ring.closed.store(true, Ordering::SeqCst);
            ring.cv.notify_all();
        }
        if let Some(handle) = self.writer.take() {
            let _ = handle.join();
        }
        let _ = self.read_stream.shutdown(Shutdown::Both);
        self.buf = None;
    }

    // @ObfuscatedName("am.c(I)I")
    pub fn read_byte(&mut self) -> Result<i32, IoError> {
        if self.dummy {
            return Ok(0);
        }
        let mut byte = [0u8; 1];
        match self.read_stream.read(&mut byte) {
            Ok(0) => Ok(-1),
            Ok(_) => Ok(byte[0] as i32),
            Err(e) => Err(IoError::from(e)),
        }
    }

    // @ObfuscatedName("am.n(I)I")
    //
    // Java's `InputStream.available()` returns "bytes that can be read
    // without blocking." For TcpStream we toggle nonblocking, peek with
    // a large probe buffer so the returned count reflects what's actually
    // queued in the kernel (not just "any vs none"), then restore
    // blocking.
    pub fn available(&mut self) -> Result<i32, IoError> {
        if self.dummy {
            return Ok(0);
        }
        self.read_stream.set_nonblocking(true)?;
        let mut probe = [0u8; 8192];
        let n = match self.read_stream.peek(&mut probe) {
            // peek == 0 on a non-blocking socket means the peer closed (orderly
            // shutdown / EOF) — distinct from WouldBlock ("no data yet"). Surface
            // it as an error so the read loop drops to lost_con instead of
            // treating a dead connection as merely idle.
            Ok(0) => {
                let _ = self.read_stream.set_nonblocking(false);
                return Err(IoError);
            }
            Ok(n) => n as i32,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => 0,
            Err(e) => {
                let _ = self.read_stream.set_nonblocking(false);
                return Err(IoError::from(e));
            }
        };
        self.read_stream.set_nonblocking(false)?;
        Ok(n)
    }

    // @ObfuscatedName("am.j([BIII)V")
    pub fn read(&mut self, arg0: &mut [u8], arg1: i32, arg2: i32) -> Result<(), IoError> {
        if self.dummy {
            return Ok(());
        }
        let mut off = arg1 as usize;
        let mut remaining = arg2 as usize;
        while remaining > 0 {
            let n = self.read_stream.read(&mut arg0[off..off + remaining])?;
            if n == 0 {
                return Err(IoError);
            }
            off += n;
            remaining -= n;
        }
        Ok(())
    }

    // @ObfuscatedName("am.z([BIII)V")
    pub fn write(&mut self, arg0: &[u8], arg1: i32, arg2: i32) -> Result<(), IoError> {
        if self.dummy {
            return Ok(());
        }
        if self.ioerror.swap(false, Ordering::SeqCst) {
            return Err(IoError);
        }
        if self.buf.is_none() {
            self.buf = Some(Arc::new(WriterRing {
                buf: Mutex::new(vec![0u8; 5000]),
                head: AtomicUsize::new(0),
                tail: AtomicUsize::new(0),
                closed: AtomicBool::new(false),
                cv: Condvar::new(),
            }));
        }
        let ring = self.buf.as_ref().unwrap().clone();

        {
            let mut buf = ring.buf.lock().unwrap();
            for k in 0..arg2 as usize {
                buf[self.tnum as usize] = arg0[arg1 as usize + k];
                self.tnum = (self.tnum + 1) % 5000;
                // Java's full-check reads `tcyl` — the DRAIN cursor the
                // writer thread advances as it pushes bytes onto the
                // socket (ClientStream.java:126/169). Our writer thread
                // tracks that cursor in `ring.tail`; refresh per byte
                // like Java's volatile-ish field read, otherwise the
                // check compares against a frozen 0 and trips once 4900
                // CUMULATIVE bytes have ever been written (~20s of
                // gameplay), force-dropping a healthy connection.
                self.tcyl = ring.tail.load(Ordering::SeqCst) as i32;
                if (self.tcyl + 4900) % 5000 == self.tnum {
                    return Err(IoError);
                }
            }
            ring.head.store(self.tnum as usize, Ordering::SeqCst);
        }

        if self.writer.is_none() {
            // Hand the writer thread its own clone of the socket fd so
            // simultaneous read/write don't have to serialize through a
            // mutex. POSIX/Winsock are both fine with concurrent reads and
            // writes on the same fd.
            let write_clone = self
                .read_stream
                .try_clone()
                .map_err(|_| IoError)?;
            let ring_w = ring.clone();
            let ioerror = self.ioerror.clone();
            self.writer = Some(thread::spawn(move || writer_run(write_clone, ring_w, ioerror)));
        }
        ring.cv.notify_all();
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for ClientStream {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn writer_run(mut stream: TcpStream, ring: Arc<WriterRing>, ioerror: Arc<AtomicBool>) {
    loop {
        let (start, len) = {
            let mut buf = ring.buf.lock().unwrap();
            loop {
                let head = ring.head.load(Ordering::SeqCst);
                let tail = ring.tail.load(Ordering::SeqCst);
                if head == tail {
                    if ring.closed.load(Ordering::SeqCst) {
                        return;
                    }
                    buf = ring.cv.wait(buf).unwrap();
                    continue;
                }
                let start = tail;
                let len = if head >= tail { head - tail } else { 5000 - tail };
                break (start, len);
            }
        };

        let bytes: Vec<u8> = {
            let buf = ring.buf.lock().unwrap();
            buf[start..start + len].to_vec()
        };

        if let Err(_) = stream.write_all(&bytes) {
            ioerror.store(true, Ordering::SeqCst);
            return;
        }
        let new_tail = (start + len) % 5000;
        ring.tail.store(new_tail, Ordering::SeqCst);
        let head = ring.head.load(Ordering::SeqCst);
        if head == new_tail {
            let _ = stream.flush();
        }
    }
}
