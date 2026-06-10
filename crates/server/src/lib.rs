//! rev1 game server: TCP listener + per-connection state machines,
//! JS5 cache service, login, and the 600ms world tick driving
//! crates/engine. Mirrors the Engine2007 reference flow (Login.ts,
//! Js5.ts, World.ts) with the engine/script layers it stubs out.

pub mod connection;
pub mod js5;

use std::collections::HashMap;
use std::net::TcpListener;
use std::path::Path;
use std::time::{Duration, Instant};

use cache::Cache;
use connection::{ConnState, Connection};
use engine::World;
use io::packet::Packet;
use protocol::client as cproto;
use protocol::server as sproto;

/// Archives served over JS5 (idx0..idx15 + the master idx255).
const ARCHIVE_COUNT: u8 = 16;
const TICK: Duration = Duration::from_millis(600);

pub struct ServerConfig {
    pub addr: String,
    pub cache_dir: String,
    pub script_dir: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            addr: "0.0.0.0:43594".to_string(),
            cache_dir: "cache".to_string(),
            script_dir: None,
        }
    }
}

pub fn run(config: ServerConfig) -> std::io::Result<()> {
    let mut cache = Cache::open(Path::new(&config.cache_dir))?;
    let checksum_table = js5::build_checksum_table(&mut cache, ARCHIVE_COUNT);

    // XTEA keys for map encryption — keys.json next to the cache.
    let xtea = cache::maps::XteaKeys::load(Path::new(&config.cache_dir).join("keys.json").as_path())
        .unwrap_or_else(|e| {
            eprintln!("[server] keys.json not loaded ({e}); maps unencrypted");
            cache::maps::XteaKeys { by_mapsquare: HashMap::new() }
        });

    let mut world = World::new();
    if let Some(dir) = &config.script_dir {
        world.load_scripts(dir);
    } else {
        eprintln!("[server] no script dir configured; engine fallbacks active");
    }

    let listener = TcpListener::bind(&config.addr)?;
    listener.set_nonblocking(true)?;
    eprintln!("[server] listening on {}", config.addr);

    let mut connections: Vec<Connection> = Vec::new();
    let mut next_tick = Instant::now() + TICK;

    loop {
        // Accept.
        loop {
            match listener.accept() {
                Ok((stream, addr)) => {
                    eprintln!("[server] connection from {addr}");
                    connections.push(Connection::new(stream));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    eprintln!("[server] accept error: {e}");
                    break;
                }
            }
        }

        // Service sockets.
        for conn in connections.iter_mut() {
            conn.fill();
            if conn.state == ConnState::Closed {
                continue;
            }

            match conn.state {
                ConnState::Handshake => {
                    if let Some(login) = conn.process_handshake() {
                        handle_login(conn, login, &mut world, &xtea);
                    }
                }
                ConnState::Js5 => {
                    service_js5(conn, &mut cache, &checksum_table);
                }
                ConnState::Game { pid } => {
                    pump_game(conn, pid, &mut world);
                }
                ConnState::Closed => {}
            }

            conn.flush();
        }

        // Reap closed connections (logging their players out).
        connections.retain(|c| {
            if c.state == ConnState::Closed {
                false
            } else if let ConnState::Game { pid } = c.state {
                if world.players.get(pid).map_or(true, |p| p.is_none()) {
                    // Player slot vanished out from under the socket.
                    return false;
                }
                true
            } else {
                true
            }
        });

        // World tick.
        if Instant::now() >= next_tick {
            world.cycle();
            next_tick += TICK;

            // Drain per-player outgoing queues onto their sockets.
            let mut to_remove: Vec<usize> = Vec::new();
            for conn in connections.iter_mut() {
                let ConnState::Game { pid } = conn.state else { continue; };
                let Some(player) = world.players[pid].as_mut() else {
                    conn.state = ConnState::Closed;
                    continue;
                };
                if player.logging_out {
                    to_remove.push(pid);
                }
                let mut out = Packet::new(4096);
                for packet in player.out.drain(..) {
                    packet.frame(&mut out);
                }
                let mut data = out.data;
                data.truncate(out.pos);
                conn.write(&data);
                conn.flush();
                if player.logging_out {
                    conn.state = ConnState::Closed;
                }
            }
            for pid in to_remove {
                world.remove_player(pid);
            }
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}

fn handle_login(conn: &mut Connection, login: connection::LoginRequest,
                world: &mut World, xtea: &cache::maps::XteaKeys) {
    if login.reconnect {
        conn.write(&[15]);
        return;
    }

    let Some(pid) = world.add_player(login.username.clone(), 3222, 3222, 0) else {
        conn.write(&[7]); // world full
        conn.state = ConnState::Closed;
        return;
    };

    // Success reply: 2, staffmod 0, 0, pid16, members 0.
    let mut reply = Packet::new(6);
    reply.p1(2);
    reply.p1(0);
    reply.p1(0);
    reply.p2(0);
    reply.p1(0);
    let mut data = reply.data;
    data.truncate(reply.pos);
    conn.write(&data);

    // REBUILD_NORMAL must reach the client before the queued
    // welcome/interface packets so the scene exists.
    let player = world.players[pid].as_mut().expect("fresh player");
    let (x, z) = (player.entity.x, player.entity.z);
    let rebuild = sproto::rebuild_normal(x, z, |mx, mz| {
        xtea.get(mx as u32, mz as u32).copied().unwrap_or([0; 4])
    });
    player.out.insert(0, rebuild);

    let mut out = Packet::new(4096);
    for packet in player.out.drain(..) {
        packet.frame(&mut out);
    }
    let mut data = out.data;
    data.truncate(out.pos);
    conn.write(&data);

    conn.state = ConnState::Game { pid };
    eprintln!("[server] {} logged in as pid {pid}", login.username);
}

fn service_js5(conn: &mut Connection, cache: &mut Cache, checksum_table: &[u8]) {
    while conn.inbuf.len() >= 4 {
        let req: Vec<u8> = conn.inbuf.drain(..4).collect();
        let opcode = req[0];
        let archive = req[1];
        let group = ((req[2] as u16) << 8) | req[3] as u16;

        match opcode {
            js5::JS5_PREFETCH | js5::JS5_URGENT => {
                let data = if archive == 255 && group == 255 {
                    Some(checksum_table.to_vec())
                } else if archive == 255 {
                    cache.read_master_raw(group as u8).ok().flatten()
                } else {
                    cache.read_raw(archive, group as u32).ok().flatten()
                };
                match data {
                    Some(data) => {
                        let frame = js5::group_response(archive, group, &data);
                        conn.write(&frame);
                    }
                    None => {
                        eprintln!("[js5] missing group {archive}/{group}");
                    }
                }
            }
            js5::JS5_PRIORITY_HIGH | js5::JS5_PRIORITY_LOW | js5::JS5_XOR => {
                // Consumed; no state to keep yet.
            }
            _ => {
                conn.state = ConnState::Closed;
                return;
            }
        }
    }
}

fn pump_game(conn: &mut Connection, pid: usize, world: &mut World) {
    // Java-style read pump: opcode, optional var-size byte(s), body.
    loop {
        if conn.packet_type == -1 {
            if conn.inbuf.is_empty() {
                return;
            }
            conn.packet_type = conn.inbuf[0] as i32;
            let Some(size) = cproto::packet_size(conn.packet_type as u8) else {
                eprintln!("[game] unhandled packet {} — dropping connection", conn.packet_type);
                conn.state = ConnState::Closed;
                return;
            };
            conn.packet_size = size;
            conn.inbuf.drain(..1);
        }

        if conn.packet_size == -1 {
            if conn.inbuf.is_empty() {
                return;
            }
            conn.packet_size = conn.inbuf[0] as i32;
            conn.inbuf.drain(..1);
        } else if conn.packet_size == -2 {
            if conn.inbuf.len() < 2 {
                return;
            }
            conn.packet_size = ((conn.inbuf[0] as i32) << 8) | conn.inbuf[1] as i32;
            conn.inbuf.drain(..2);
        }

        if conn.inbuf.len() < conn.packet_size as usize {
            return;
        }

        let body: Vec<u8> = conn.inbuf.drain(..conn.packet_size as usize).collect();
        let opcode = conn.packet_type as u8;
        let size = conn.packet_size as usize;
        conn.packet_type = -1;

        let mut p = Packet::from_vec(body);
        let message = cproto::decode(opcode, &mut p, size);
        world.handle_message(pid, message);
    }
}
