//! rev1 game server: TCP listener + per-connection state machines,
//! JS5 cache service, login, and the 600ms world tick driving
//! crates/engine. Mirrors the Engine2007 reference flow (Login.ts,
//! Js5.ts, World.ts) with the engine/script layers it stubs out.

pub mod connection;
pub mod js5;
pub mod websocket;
pub mod worldlist;

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cache::Cache;
use connection::{ConnState, Connection};
use engine::World;
use io::packet::Packet;
use protocol::client as cproto;
use protocol::server as sproto;

/// `OS_DEBUG` gate for verbose diagnostic logging — checked once, cached.
/// Set the env var `OS_DEBUG` (to any value) to surface gated `dbg_log!` output.
pub fn debug_enabled() -> bool {
    use std::sync::atomic::{AtomicU8, Ordering};
    static STATE: AtomicU8 = AtomicU8::new(0);
    match STATE.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => {
            let on = std::env::var_os("OS_DEBUG").is_some();
            STATE.store(if on { 2 } else { 1 }, Ordering::Relaxed);
            on
        }
    }
}

/// `eprintln!` that only fires when `OS_DEBUG` is set (gated diagnostics).
#[macro_export]
macro_rules! dbg_log {
    ($($arg:tt)*) => {
        if $crate::debug_enabled() { eprintln!($($arg)*); }
    };
}

/// Archives served over JS5 (idx0..idx15 + the master idx255).
const ARCHIVE_COUNT: u8 = 16;
const TICK: Duration = Duration::from_millis(600);

/// Rolling sink of runtime warnings (missing JS5 groups, dropped packets, …) so
/// the control panel can surface server-health issues in its UI instead of them
/// hiding in stderr. Capped to the last 200 lines; drained by observers.
pub static WARN_LOG: std::sync::Mutex<std::collections::VecDeque<String>> =
    std::sync::Mutex::new(std::collections::VecDeque::new());

/// Log a runtime warning to stderr and the panel-visible [`WARN_LOG`].
pub fn warn(msg: impl Into<String>) {
    let msg = msg.into();
    eprintln!("[server] ⚠ {msg}");
    if let Ok(mut w) = WARN_LOG.lock() {
        if w.len() >= 200 {
            w.pop_front();
        }
        w.push_back(msg);
    }
}

/// Per-tick time accounting handed to the control panel for its stacked
/// performance graph: network IO (accept/read/parse), the engine cycle, and
/// output (queue drain + socket writes).
#[derive(Clone, Copy)]
pub struct TickStats {
    /// Network input: accept, socket reads, packet parse.
    pub io: Duration,
    /// Whole engine cycle (sum of `cycle`'s phases).
    pub engine: Duration,
    /// Output: per-player queue drain + socket writes/flush.
    pub output: Duration,
    /// Per-phase engine breakdown (world/npcs/players/info + scripts subset).
    pub cycle: engine::CycleStats,
    /// Open sockets this tick (all states except Closed).
    pub connections: usize,
    /// Subset of `connections` currently streaming JS5 cache.
    pub js5: usize,
}

/// A command from the control panel, applied on the world-tick thread (which
/// owns `&mut World`) so the GUI never mutates the world directly.
pub enum PanelCommand {
    /// Broadcast a chatbox message to every online player.
    Broadcast(String),
    /// Force-log-out the player with this pid.
    Kick(usize),
    /// Teleport the player to a coordinate (snaps + triggers a scene rebuild).
    Teleport { pid: usize, x: i32, z: i32, level: i32 },
    /// Send a private chatbox message to a single player.
    Message { pid: usize, text: String },
}

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
    /// Optional startup-progress reporter (control panel splash): called with a
    /// human stage label as the server boots (`verifying cache`, …, `listening`).
    pub progress: Option<Box<dyn FnMut(&str) + Send>>,
    /// Optional per-tick observer (control panel): called after each world cycle
    /// with the live world + that tick's time accounting, to build a UI snapshot.
    pub tick_hook: Option<Box<dyn FnMut(&engine::World, TickStats) + Send>>,
    /// Optional control-panel command stream, drained each tick and applied to
    /// the world (broadcast, …).
    pub command_rx: Option<std::sync::mpsc::Receiver<PanelCommand>>,
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
            progress: None,
            tick_hook: None,
            command_rx: None,
        }
    }
}

pub fn run(mut config: ServerConfig) -> std::io::Result<()> {
    let mut progress = config.progress.take();
    let mut tick_hook = config.tick_hook.take();
    let command_rx = config.command_rx.take();
    macro_rules! stage { ($s:expr) => {{ eprintln!("[server] {}", $s); if let Some(p) = progress.as_mut() { p($s); } }}; }

    // Content is the editable source of truth. Pack it into a real cache and
    // — when a CRC baseline exists — prove that generated cache is byte-for-byte
    // CRC-identical to the vanilla cache before serving a single group. The
    // vanilla `cache_dir` stays a read-only CRC reference (and holds keys.json).
    // We then serve from the GENERATED cache, so edits/renames in Content flow
    // straight through. Refuses to start on any CRC mismatch. The closure emits
    // the same packing/verifying sub-stages to the splash + log.
    let serve_dir = {
        let mut emit = |s: &str| {
            // Per-group pack/verify counts go to the panel only (don't spam log).
            if !s.starts_with("packing cache ") && !s.starts_with("verifying cache ") {
                eprintln!("[server] {s}");
            }
            if let Some(p) = progress.as_mut() {
                p(s);
            }
        };
        prepare_served_cache(&config, &mut emit)?
    };
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

    // Build the server script bundle from Content/scripts (best-effort; falls
    // back to the existing data/pack bundle if the RuneScript toolchain/JDK
    // isn't available). Gated by a source hash so it only recompiles on change.
    stage!("compiling scripts");
    if let Err(msg) = compile_server_scripts(&config) {
        // A broken script bundle halts startup — surface it to the control panel
        // splash first, then refuse to start (don't run on a stale bundle).
        if let Some(p) = progress.as_mut() {
            p(&format!("scripts error: {msg}"));
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("server script compile failed; refusing to start:\n{msg}"),
        ));
    }

    stage!("loading scripts");
    let mut world = World::new();
    if let Some(dir) = &config.script_dir {
        world.load_scripts(dir);
    } else {
        eprintln!("[server] no script dir configured; engine fallbacks active");
    }
    stage!("loading map");
    // Build the server-side collision map: loc/seq config first, then every
    // region's terrain + loc footprints. This powers the map ops (map_blocked,
    // lineofsight/walk, map_findsquare, …) and script-driven pathfinding.
    world.load_configs(&mut cache);
    world.load_map(&mut cache, &xtea);
    // Server-side npc AI config (wanderrange/maxrange/…) lives only in the .npc
    // source text, not the client cache — overlay it before spawning so spawns
    // pick up per-type default modes.
    let spawn_dir = config.content_dir.clone().unwrap_or_else(|| "Content".to_string());
    world.load_server_npc_props(Path::new(&spawn_dir));
    // World npc spawns: the `==== NPC ====` section of each `.jm2` map, plus the
    // legacy OS text list `npcs.txt` if present (the 2007 cache has no Jagex
    // spawn file).
    world.load_npc_spawns_from_maps(Path::new(&spawn_dir).join("maps").as_path());
    world.load_npc_spawns(Path::new(&spawn_dir).join("npcs.txt").as_path());

    let listener = TcpListener::bind(&config.addr)?;
    listener.set_nonblocking(true)?;
    stage!("listening");
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
    let mut io_accum = Duration::ZERO;

    loop {
        let io_start = Instant::now();
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

        io_accum += io_start.elapsed();

        // World tick.
        if Instant::now() >= next_tick {
            // Apply queued control-panel commands before the cycle.
            if let Some(rx) = command_rx.as_ref() {
                while let Ok(cmd) = rx.try_recv() {
                    match cmd {
                        PanelCommand::Broadcast(text) => {
                            for p in world.players.iter_mut().flatten() {
                                p.out.push(sproto::message_game(&text));
                            }
                        }
                        PanelCommand::Kick(pid) => {
                            if let Some(Some(p)) = world.players.get_mut(pid) {
                                p.logging_out = true;
                            }
                        }
                        PanelCommand::Teleport { pid, x, z, level } => {
                            if let Some(Some(p)) = world.players.get_mut(pid) {
                                p.entity.teleport(x, z, level, true);
                            }
                        }
                        PanelCommand::Message { pid, text } => {
                            if let Some(Some(p)) = world.players.get_mut(pid) {
                                p.out.push(sproto::message_game(&text));
                            }
                        }
                    }
                }
            }
            let cycle_start = Instant::now();
            let cycle_stats = world.cycle();
            let engine_dur = cycle_start.elapsed();
            next_tick += TICK;
            live_players.store(
                world.players.iter().filter(|p| p.is_some()).count() as i32,
                std::sync::atomic::Ordering::Relaxed,
            );

            // Drain per-player outgoing queues onto their sockets.
            let output_start = Instant::now();
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
            let output_dur = output_start.elapsed();

            if let Some(h) = tick_hook.as_mut() {
                // Connection census: total open sockets + the JS5 (cache-stream)
                // subset, so the panel can show raw connection load vs players.
                let mut conns = 0usize;
                let mut js5 = 0usize;
                for c in &connections {
                    match c.state {
                        ConnState::Closed => {}
                        ConnState::Js5 => { conns += 1; js5 += 1; }
                        _ => conns += 1,
                    }
                }
                h(&world, TickStats {
                    io: io_accum, engine: engine_dur, output: output_dur, cycle: cycle_stats,
                    connections: conns, js5,
                });
            }
            io_accum = Duration::ZERO;
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
/// `OS_SKIP_CACHE_VERIFY=1` is set, or no Content tree is present. A missing
/// baseline is non-fatal — the generated cache is served unverified (with a
/// warning to run `os gen-baseline`).
fn prepare_served_cache(
    config: &ServerConfig,
    stage: &mut dyn FnMut(&str),
) -> std::io::Result<std::path::PathBuf> {
    let cache_dir = std::path::PathBuf::from(&config.cache_dir);

    if std::env::var_os("OS_SKIP_CACHE_VERIFY").is_some() {
        eprintln!("[server] OS_SKIP_CACHE_VERIFY set — serving vanilla cache (no Content repack)");
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
    let gen_dir = std::env::temp_dir().join("os_content_cache");
    stage("packing cache");
    eprintln!("[server] packing Content ({}) -> {} …", content_dir.display(), gen_dir.display());
    cache::content::pack::pack_with_progress(content_dir, &gen_dir, &mut |done, total| {
        stage(&format!("packing cache {done}/{total}"));
    })?;

    // Prove the generated cache is CRC-identical to the vanilla baseline.
    let baseline_path = config
        .baseline_path
        .clone()
        .unwrap_or_else(|| format!("{}/crc_baseline.json", config.cache_dir));
    let baseline_path = Path::new(&baseline_path);
    if !baseline_path.exists() {
        eprintln!(
            "[server] no CRC baseline at {} — serving generated cache UNVERIFIED \
             (run `os gen-baseline` to enable the startup gate)",
            baseline_path.display()
        );
        return Ok(gen_dir);
    }

    let baseline = cache::verify::Baseline::load(baseline_path)?;
    stage("verifying cache");
    eprintln!("[server] verifying generated cache vs {} …", baseline_path.display());
    let report = cache::verify::verify_cache_with_progress(&gen_dir, &baseline, &mut |done, total| {
        stage(&format!("verifying cache {done}/{total}"));
    })?;
    if report.is_ok() {
        eprintln!("[server] {report}");
        Ok(gen_dir)
    } else {
        eprint!("{report}");
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "cache verification failed; refusing to start (set OS_SKIP_CACHE_VERIFY=1 to override)",
        ))
    }
}

/// Compile the server RuneScript sources (`Content/scripts`) into the engine's
/// `data/pack/server/script.{dat,idx}` bundle via the Kotlin compiler in
/// `runescript/`. Skipped when the sources are unchanged since the last
/// successful compile.
///
/// A real compile failure (or a toolchain that's present but won't run) is
/// FATAL — returns `Err` so the server refuses to start on a broken bundle. The
/// only non-fatal cases are "nothing to compile" (no source dir) and "compiler
/// not installed" (no gradlew — a deploy may ship the prebuilt bundle).
fn compile_server_scripts(config: &ServerConfig) -> Result<(), String> {
    let content = config.content_dir.clone().unwrap_or_else(|| "Content".to_string());
    let scripts_dir = Path::new(&content).join("scripts");
    if !scripts_dir.exists() {
        eprintln!("[server] no {} — keeping existing script bundle", scripts_dir.display());
        return Ok(());
    }
    let gradlew = if cfg!(windows) { "runescript/gradlew.bat" } else { "runescript/gradlew" };
    if !Path::new(gradlew).exists() {
        eprintln!("[server] RuneScript compiler not present (no {gradlew}); keeping existing bundle");
        return Ok(());
    }

    // Skip recompiling when the sources are unchanged and a bundle exists.
    let hash = scripts_source_hash(&scripts_dir);
    let hash_path = Path::new("data/pack/.scripts_hash");
    let bundle = Path::new("data/pack/server/script.dat");
    if bundle.exists() {
        if let Ok(prev) = std::fs::read_to_string(hash_path) {
            if prev.trim() == hash.to_string() {
                eprintln!("[server] server scripts unchanged; using cached bundle");
                return Ok(());
            }
        }
    }

    eprintln!("[server] compiling server scripts ({} -> data/pack) …", scripts_dir.display());
    // The compiler's working dir is runescript/compiler/, so pass ABSOLUTE
    // paths (forward slashes — accepted on both platforms, avoids cwd surprises).
    let root = std::env::current_dir().unwrap_or_default();
    let abs = |rel: &str| root.join(rel).display().to_string().replace('\\', "/");
    let args = format!(
        "--args=--src {} --out {} --commands {} --packs {}",
        abs("Content/scripts"),
        abs("data/pack"),
        abs("runescript/data/symbols/command.pack"),
        abs("Content/pack"),
    );
    let mut cmd = std::process::Command::new(gradlew);
    cmd.current_dir("runescript")
        .arg("--console=plain")
        .arg(":compiler:run")
        .arg(args);
    // Gradle needs JDK 21 (a newer default JDK on PATH breaks Gradle).
    if let Some(jdk) = find_jdk21() {
        cmd.env("JAVA_HOME", jdk);
    }
    match cmd.output() {
        Ok(o) if o.status.success() => {
            let _ = std::fs::write(hash_path, hash.to_string());
            eprintln!("[server] server scripts compiled");
            Ok(())
        }
        Ok(o) => {
            // Pull out the compiler's own error lines (RuneScript errors are
            // "file.rs2:line:col: error: …"), not the Gradle noise.
            let out = String::from_utf8_lossy(&o.stdout);
            let err = String::from_utf8_lossy(&o.stderr);
            let errors: Vec<String> = out.lines().chain(err.lines())
                .map(|l| l.trim())
                .filter(|l| l.contains(".rs2") || l.contains("error:") || l.contains("compilation failed"))
                .map(|l| l.to_string())
                .collect();
            eprintln!("[server] script compile FAILED (exit {:?}):", o.status.code());
            for l in &errors {
                eprintln!("[server]   {l}");
            }
            let summary = if errors.is_empty() {
                format!("compiler exited {:?}", o.status.code())
            } else {
                errors.join("\n")
            };
            Err(summary)
        }
        Err(e) => Err(format!("could not run RuneScript compiler: {e}")),
    }
}

/// Fingerprint every `.rs2` source (path + size + mtime) so we only recompile on
/// change.
fn scripts_source_hash(dir: &Path) -> u64 {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if matches!(p.extension().and_then(|x| x.to_str()), Some("rs2" | "constant")) {
                    files.push(p);
                }
            }
        }
    }
    files.sort();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for f in &files {
        f.to_string_lossy().hash(&mut h);
        if let Ok(md) = std::fs::metadata(f) {
            md.len().hash(&mut h);
            if let Ok(t) = md.modified() {
                if let Ok(d) = t.duration_since(std::time::UNIX_EPOCH) {
                    d.as_secs().hash(&mut h);
                }
            }
        }
    }
    h.finish()
}

/// Locate a JDK 21 for Gradle: honour `JAVA_HOME` if it points at one, else scan
/// the standard Windows install dir. Returns `None` elsewhere (use Gradle's default).
#[cfg(windows)]
fn find_jdk21() -> Option<PathBuf> {
    if let Some(jh) = std::env::var_os("JAVA_HOME") {
        let p = PathBuf::from(&jh);
        if p.file_name().is_some_and(|n| n.to_string_lossy().contains("jdk-21")) {
            return Some(p);
        }
    }
    let mut cands: Vec<PathBuf> = std::fs::read_dir("C:\\Program Files\\Java")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("jdk-21")))
        .collect();
    cands.sort();
    cands.pop()
}

#[cfg(not(windows))]
fn find_jdk21() -> Option<PathBuf> {
    None
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
                        warn(format!("missing JS5 group {archive}/{group}"));
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
                warn(format!("unhandled game packet {} — dropping connection", conn.packet_type));
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

        dbg_log!("[game] recv op={opcode} sz={size} body={:02x?}", &body[..body.len().min(16)]);
        let mut p = Packet::from_vec(body);
        let message = cproto::decode(opcode, &mut p, size);
        world.handle_message(pid, message);
    }
}
