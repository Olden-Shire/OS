//! rev1 game server: TCP listener + per-connection state machines,
//! JS5 cache service, login, and the 600ms world tick driving
//! crates/engine. Mirrors the Engine2007 reference flow (Login.ts,
//! Js5.ts, World.ts) with the engine/script layers it stubs out.

pub mod connection;
pub mod js5;
pub mod websocket;
pub mod worldlist;

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
    /// Content-shaped source tree to repack-verify before startup. Default `Content`.
    pub content_dir: Option<String>,
    /// Vanilla CRC baseline (`crc_baseline.json`). Default `{cache_dir}/crc_baseline.json`.
    pub baseline_path: Option<String>,
    /// World id advertised on the worldlist (dev convention: game port =
    /// worldid+40000, js5 = worldid+50000, website/worldlist = worldid+7000).
    pub worldid: i32,
    /// Host advertised on the worldlist entry — what the client connects
    /// to for game/js5 after switching to this world.
    pub adv_host: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            addr: "0.0.0.0:43594".to_string(),
            cache_dir: "cache".to_string(),
            script_dir: None,
            content_dir: None,
            baseline_path: None,
            worldid: 1,
            adv_host: "127.0.0.1".to_string(),
        }
    }
}

pub fn run(config: ServerConfig) -> std::io::Result<()> {
    // Content is the editable source of truth. Pack it into a real cache and
    // — when a CRC baseline exists — prove that generated cache is byte-for-byte
    // CRC-identical to the vanilla cache before serving a single group. The
    // vanilla `cache_dir` stays a read-only CRC reference (and holds keys.json).
    // We then serve from the GENERATED cache, so edits/renames in Content flow
    // straight through. Refuses to start on any CRC mismatch.
    let serve_dir = prepare_served_cache(&config)?;
    eprintln!("[server] serving cache from {}", serve_dir.display());

    let mut cache = Cache::open(&serve_dir)?;
    let checksum_table = js5::build_checksum_table(&cache, ARCHIVE_COUNT);

    // XTEA keys for map encryption — keys.json lives with the vanilla cache
    // (it isn't a packed group, so it never lands in the generated cache).
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
    // Build the server-side collision map: loc/seq config first, then every
    // region's terrain + loc footprints. This powers the map ops (map_blocked,
    // lineofsight/walk, map_findsquare, …) and script-driven pathfinding.
    world.load_configs(&mut cache);
    world.load_map(&mut cache, &xtea);

    let listener = TcpListener::bind(&config.addr)?;
    listener.set_nonblocking(true)?;
    eprintln!("[server] listening on {}", config.addr);

    // Worldlist endpoint with a live player count, refreshed each tick.
    let live_players = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0));
    worldlist::spawn(
        (7000 + config.worldid) as u16,
        worldlist::WorldlistInfo {
            worldid: config.worldid,
            members: false,
            host: config.adv_host.clone(),
            country: 0,
            players: std::sync::Arc::clone(&live_players),
        },
    );

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
            live_players.store(
                world.players.iter().filter(|p| p.is_some()).count() as i32,
                std::sync::atomic::Ordering::Relaxed,
            );

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

/// Build the cache the server serves from. Packs the Content tree into a real
/// cache directory (`main_file_cache.*`) and, when a CRC baseline exists,
/// verifies it is CRC-identical to the vanilla cache — refusing to start (`Err`)
/// on any mismatch. Returns the directory to open and serve.
///
/// Bootstrapping/fallback cases serve the vanilla `cache_dir` directly: when
/// `OS1_SKIP_CACHE_VERIFY=1` is set, or no Content tree is present. A missing
/// baseline is non-fatal — the generated cache is served unverified (with a
/// warning to run `os1 gen-baseline`).
fn prepare_served_cache(config: &ServerConfig) -> std::io::Result<std::path::PathBuf> {
    let cache_dir = std::path::PathBuf::from(&config.cache_dir);

    if std::env::var_os("OS1_SKIP_CACHE_VERIFY").is_some() {
        eprintln!("[server] OS1_SKIP_CACHE_VERIFY set — serving vanilla cache (no Content repack)");
        return Ok(cache_dir);
    }

    let content_dir = config.content_dir.clone().unwrap_or_else(|| "Content".to_string());
    let content_dir = Path::new(&content_dir);
    if !content_dir.exists() {
        eprintln!(
            "[server] no Content tree at {} — serving vanilla cache",
            content_dir.display()
        );
        return Ok(cache_dir);
    }

    // Pack Content → a real cache: these are the bytes we actually serve.
    let gen_dir = std::env::temp_dir().join("os1_content_cache");
    eprintln!("[server] packing Content ({}) → {} …", content_dir.display(), gen_dir.display());
    cache::content::pack::pack(content_dir, &gen_dir)?;

    // Prove the generated cache is CRC-identical to the vanilla baseline.
    let baseline_path = config
        .baseline_path
        .clone()
        .unwrap_or_else(|| format!("{}/crc_baseline.json", config.cache_dir));
    let baseline_path = Path::new(&baseline_path);
    if !baseline_path.exists() {
        eprintln!(
            "[server] no CRC baseline at {} — serving generated cache UNVERIFIED \
             (run `os1 gen-baseline` to enable the startup gate)",
            baseline_path.display()
        );
        return Ok(gen_dir);
    }

    let baseline = cache::verify::Baseline::load(baseline_path)?;
    eprintln!("[server] verifying generated cache → vs {} …", baseline_path.display());
    let report = cache::verify::verify_cache(&gen_dir, &baseline)?;
    if report.is_ok() {
        eprintln!("[server] {report}");
        Ok(gen_dir)
    } else {
        eprint!("{report}");
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "cache verification failed; refusing to start (set OS1_SKIP_CACHE_VERIFY=1 to override)",
        ))
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
    // Carry the client's low-detail flag so the music/sound ops can skip playback.
    world.players[pid].as_mut().expect("fresh player").low_memory = login.low_memory;

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

        eprintln!("[game] recv op={opcode} sz={size} body={:02x?}", &body[..body.len().min(16)]);
        let mut p = Packet::from_vec(body);
        let message = cproto::decode(opcode, &mut p, size);
        world.handle_message(pid, message);
    }
}
