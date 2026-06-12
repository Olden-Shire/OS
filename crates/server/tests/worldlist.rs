//! Round-trip the worldlist HTTP service: spawn it on a test port, GET
//! it like HTTPRequest does, and decode the body exactly like the
//! client's TitleScreen.listFetch (g4 frame; g2 num; per world: g2
//! id|members, gjstr host, g1 country, g2b players).

use std::io::{Read, Write};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

use io::packet::Packet;
use server::worldlist::{self, WorldlistInfo};

#[test]
fn worldlist_http_round_trip() {
    let players = Arc::new(AtomicI32::new(0));
    worldlist::spawn(
        57231,
        WorldlistInfo {
            worldid: 7,
            members: false,
            host: "127.0.0.1".to_string(),
            country: 0,
            players: Arc::clone(&players),
        },
    );
    players.store(42, Ordering::Relaxed);

    // The listener thread binds asynchronously; retry the connect briefly.
    let mut stream = None;
    for _ in 0..50 {
        match std::net::TcpStream::connect(("127.0.0.1", 57231)) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
        }
    }
    let mut stream = stream.expect("worldlist listener did not come up");

    stream
        .write_all(b"GET /worldlist.ws HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();

    let header_end = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("http header terminator");
    let body = &response[header_end + 4..];

    let mut buf = Packet::from_vec(body.to_vec());
    let length = buf.g4();
    assert_eq!(length as usize, body.len() - 4, "g4 frame covers the payload");

    let num = buf.g2();
    assert_eq!(num, 1);
    let info = buf.g2();
    assert_eq!(info & 0x7FFF, 7, "world id");
    assert_eq!(info & 0x8000, 0, "free world");
    assert_eq!(buf.gjstr(), "127.0.0.1");
    assert_eq!(buf.g1(), 0, "country");
    assert_eq!(buf.g2b(), 42, "live player count");
    assert_eq!(buf.pos, body.len(), "no trailing bytes");
}
