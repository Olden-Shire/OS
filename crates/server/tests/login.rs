//! Headless login smoke test: boot the real game server in a background thread
//! (serving the vanilla cache, no Content repack) and perform the rev1 login
//! handshake at the protocol level, asserting the server accepts it (login
//! response code 2) and starts streaming the game session.
//!
//! It's `#[ignore]` because it boots the full server (cache open + 926-region
//! collision build), which is too heavy for the fast unit-test pass. Run it via:
//!   cargo test -p server --test login -- --ignored --nocapture
//! Only `cache/` is required (groups + keys.json) — script compilation is skipped
//! (empty content dir -> engine fallback), so no JDK/Content is needed here.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use io::packet::Packet;

const PORT: u16 = 47_321;
const REVISION: i32 = 1;

fn repo_root() -> PathBuf {
    // Integration tests run with CWD = the crate dir, so resolve paths relative
    // to the workspace root (crates/server/../..).
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

#[test]
#[ignore = "boots the full server; run with --ignored (see client-login workflow)"]
fn headless_login_returns_success() {
    let root = repo_root();
    // Serve the vanilla cache directly (no Content->cache repack / CRC verify).
    // SAFETY: set before the server thread is spawned; no other thread reads env yet.
    unsafe { std::env::set_var("OS_SKIP_CACHE_VERIFY", "1"); }

    // Empty content dir => no scripts/ => script compilation is skipped (engine
    // fallback), so the test needs neither JDK nor the Content tree.
    let empty_content = std::env::temp_dir().join("os_login_test_content");
    std::fs::create_dir_all(&empty_content).unwrap();

    let mut cfg = server::ServerConfig::default();
    cfg.addr = format!("127.0.0.1:{PORT}");
    cfg.cache_dir = root.join("cache").to_string_lossy().into_owned();
    cfg.content_dir = Some(empty_content.to_string_lossy().into_owned());
    cfg.script_dir = None;

    std::thread::spawn(move || {
        if let Err(e) = server::run(cfg) {
            eprintln!("[login-test] server::run failed: {e}");
        }
    });

    // Boot is heavy (cache open + collision build); wait for the listener.
    let mut stream = connect_retry(PORT, Duration::from_secs(120));
    stream.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    stream.set_write_timeout(Some(Duration::from_secs(15))).unwrap();

    // 1. Session begin -> the server replies a single 0.
    stream.write_all(&[14]).unwrap();
    let mut ack = [0u8; 1];
    stream.read_exact(&mut ack).unwrap();
    assert_eq!(ack[0], 0, "session-begin ack should be 0");

    // 2. Login block. rev1 stubs RSA/XTEA/ISAAC, so it's plaintext; the layout
    //    mirrors Connection::process_handshake (opcode 16 = new login).
    let mut block = Packet::new(128);
    block.p4(REVISION); // revision (must match the server's)
    block.p1(0); // isaac10 (ignored)
    for _ in 0..4 {
        block.p4(0); // 4 client seeds
    }
    block.p8(0); // isaac0 (ignored)
    block.pjstr("password"); // password
    block.p2(0); // RSA block length (0 = none in dev)
    block.pjstr("citest"); // username
    block.p1(0); // low-memory flag (0 = high detail)
    for _ in 0..24 {
        block.p1(0); // uid dat (skipped server-side)
    }
    let body = &block.data[..block.pos];

    let mut framed = Vec::with_capacity(3 + body.len());
    framed.push(16); // opcode 16 = new login
    framed.push((body.len() >> 8) as u8); // u16 block length, big-endian
    framed.push((body.len() & 0xff) as u8);
    framed.extend_from_slice(body);
    stream.write_all(&framed).unwrap();

    // 3. First response byte is the login result code; 2 = success.
    let mut resp = [0u8; 1];
    stream.read_exact(&mut resp).unwrap();
    assert_eq!(resp[0], 2, "expected login response 2 (OK), got {}", resp[0]);
}

fn connect_retry(port: u16, timeout: Duration) -> TcpStream {
    let deadline = Instant::now() + timeout;
    loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(s) => return s,
            Err(e) => {
                if Instant::now() >= deadline {
                    panic!("server never accepted on 127.0.0.1:{port}: {e}");
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
}
