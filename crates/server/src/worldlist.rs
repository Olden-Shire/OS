//! Worldlist HTTP service — the dev-mode stand-in for the live
//! `worldlistUrl` endpoint (javconfig param 9) the client's
//! TitleScreen.listFetch downloads.
//!
//! Response body format (consumed by HTTPRequest.getData +
//! TitleScreen.listFetch, Client TitleScreen.java:1090-1109):
//!
//! ```text
//! g4  length                    // HTTPRequest's body frame
//! g2  num                       // world count
//! num × {
//!   g2    id | (members << 15)
//!   gjstr host
//!   g1    country               // sl_flags sprite index (0-7)
//!   g2    players               // signed; -1 = offline, >1980 = FULL
//! }
//! ```
//!
//! Single-world for now: this server IS the world, so it advertises
//! itself with a live player count refreshed each world tick.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use io::packet::Packet;

pub struct WorldlistInfo {
    pub worldid: i32,
    pub members: bool,
    /// Host the client should connect to for game/js5 after switching.
    pub host: String,
    /// sl_flags sprite index.
    pub country: i32,
    /// Live player count, updated by the world tick.
    pub players: Arc<AtomicI32>,
}

/// Spawn the worldlist listener on `port`. Connections are served on
/// the spawned thread (requests are rare and tiny); errors only log.
pub fn spawn(port: u16, info: WorldlistInfo) {
    std::thread::spawn(move || {
        let listener = match TcpListener::bind(("0.0.0.0", port)) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[worldlist] bind 0.0.0.0:{port} failed: {e}");
                return;
            }
        };
        eprintln!("[worldlist] serving http://0.0.0.0:{port}/worldlist.ws");
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            stream.set_read_timeout(Some(Duration::from_millis(5000))).ok();
            stream.set_write_timeout(Some(Duration::from_millis(5000))).ok();
            // Drain the request line + headers; the response is the same
            // for any path, so we only need to wait for the blank line.
            let mut req = Vec::new();
            let mut chunk = [0u8; 512];
            while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => req.extend_from_slice(&chunk[..n]),
                }
            }

            let body = encode(&info);
            // Access-Control-Allow-Origin: the wasm client fetches this from
            // the page's origin (a static file server), which the browser
            // treats as cross-origin. The worldlist is public data.
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
}

fn encode(info: &WorldlistInfo) -> Vec<u8> {
    let mut inner = Packet::new(0);
    inner.p2(1); // num
    let flags = (info.worldid & 0x7FFF) | if info.members { 0x8000 } else { 0 };
    inner.p2(flags);
    inner.pjstr(&info.host);
    inner.p1(info.country);
    inner.p2(info.players.load(Ordering::Relaxed));
    let inner = inner.data;

    let mut out = Packet::new(0);
    out.p4(inner.len() as i32);
    out.data.extend_from_slice(&inner);
    out.data
}
