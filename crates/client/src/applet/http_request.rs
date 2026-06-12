// @ObfuscatedName("i") — jag::oldscape::client::HTTPRequest.
//
// One-shot HTTP fetcher. Java's flow:
//   1. spawn a worker thread
//   2. open a URLConnection, read until EOF
//   3. store the body bytes on `data` and set `done = true`
//
// TitleScreen uses this for the world-list download. We implement
// HTTP/1.0 GET using std::net::TcpStream (no external deps), which
// matches Java's minimalist URLConnection.openConnection() flow:
// just the Status-Line + Host header + empty CRLF; body returned
// raw on success.

#![allow(dead_code)]

#[cfg(not(target_arch = "wasm32"))]
use std::io::{Read, Write};
#[cfg(not(target_arch = "wasm32"))]
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
#[cfg(not(target_arch = "wasm32"))]
use std::thread;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct HTTPRequest {
    pub url: String,
    pub done: bool,
    pub data: Vec<u8>,
    pub status: i32,
}

impl HTTPRequest {
    pub fn new(url: String) -> Self {
        Self { url, done: false, data: Vec::new(), status: 0 }
    }

    // @ObfuscatedName("i.r(B)V") — HTTPRequest.start. Spawns a worker
    // thread that runs an HTTP/1.0 GET against `req.url`. The fetched
    // body lands on `req.data`; `req.status` mirrors the HTTP status
    // code, or 0 on connection error.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start(req: Arc<Mutex<HTTPRequest>>) {
        let url = req.lock().unwrap().url.clone();
        thread::spawn(move || {
            let (status, body) = match fetch(&url) {
                Ok((s, b)) => (s, b),
                Err(_) => (0, Vec::new()),
            };
            let mut r = req.lock().unwrap();
            r.data = body;
            r.status = status;
            r.done = true;
        });
    }

    // wasm: same polling contract over the browser's fetch(). The Promise
    // callbacks are one-shot per request, so leaking them via forget() is
    // bounded by the handful of worldlist fetches per session.
    #[cfg(target_arch = "wasm32")]
    pub fn start(req: Arc<Mutex<HTTPRequest>>) {
        use wasm_bindgen::closure::Closure;
        use wasm_bindgen::{JsCast, JsValue};

        let url = req.lock().unwrap().url.clone();
        let Some(window) = web_sys::window() else {
            let mut r = req.lock().unwrap();
            r.status = 0;
            r.done = true;
            return;
        };

        let fail = {
            let req = req.clone();
            Closure::<dyn FnMut(JsValue)>::new(move |_| {
                let mut r = req.lock().unwrap();
                r.status = 0;
                r.done = true;
            })
        };

        let on_response = {
            let req = req.clone();
            let fail2 = {
                let req = req.clone();
                Closure::<dyn FnMut(JsValue)>::new(move |_| {
                    let mut r = req.lock().unwrap();
                    r.status = 0;
                    r.done = true;
                })
            };
            Closure::<dyn FnMut(JsValue)>::new(move |resp: JsValue| {
                let Ok(resp) = resp.dyn_into::<web_sys::Response>() else {
                    let mut r = req.lock().unwrap();
                    r.status = 0;
                    r.done = true;
                    return;
                };
                let status = resp.status() as i32;
                let Ok(buf_promise) = resp.array_buffer() else {
                    let mut r = req.lock().unwrap();
                    r.status = status;
                    r.done = true;
                    return;
                };
                let req2 = req.clone();
                let on_body = Closure::<dyn FnMut(JsValue)>::new(move |buf: JsValue| {
                    let data = buf
                        .dyn_into::<js_sys::ArrayBuffer>()
                        .map(|b| js_sys::Uint8Array::new(&b).to_vec())
                        .unwrap_or_default();
                    let mut r = req2.lock().unwrap();
                    r.data = data;
                    r.status = status;
                    r.done = true;
                });
                let _ = buf_promise.then2(&on_body, &fail2);
                on_body.forget();
            })
        };

        let _ = window.fetch_with_str(&url).then2(&on_response, &fail);
        on_response.forget();
        fail.forget();
    }
}

// Minimal HTTP/1.0 GET. Java's URLConnection covers redirects + HTTPS,
// but TitleScreen only fetches the world-list which is plain HTTP.
#[cfg(not(target_arch = "wasm32"))]
fn fetch(url: &str) -> Result<(i32, Vec<u8>), Box<dyn std::error::Error + Send + Sync>> {
    let (host, port, path) = parse_url(url)?;
    let addr = format!("{host}:{port}");
    let mut sock = TcpStream::connect_timeout(
        &addr.parse()?,
        Duration::from_secs(8),
    )?;
    sock.set_read_timeout(Some(Duration::from_secs(8)))?;
    sock.set_write_timeout(Some(Duration::from_secs(8)))?;
    let req = format!(
        "GET {path} HTTP/1.0\r\nHost: {host}\r\nUser-Agent: jagex/rev1\r\n\r\n"
    );
    sock.write_all(req.as_bytes())?;
    let mut all = Vec::with_capacity(4096);
    sock.read_to_end(&mut all)?;

    // Split status line + headers from body. HTTP delimits with
    // \r\n\r\n; tolerate \n\n as fallback.
    let split_at = all.windows(4).position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
        .or_else(|| all.windows(2).position(|w| w == b"\n\n").map(|i| i + 2))
        .unwrap_or(all.len());

    let head_bytes = &all[..split_at.min(all.len())];
    let head = std::str::from_utf8(head_bytes).unwrap_or("");
    let status = head.lines().next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);
    let body = if split_at >= all.len() { Vec::new() } else { all[split_at..].to_vec() };
    Ok((status, body))
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_url(url: &str) -> Result<(String, u16, String), &'static str> {
    let rest = url.strip_prefix("http://").ok_or("only http:// supported")?;
    let slash = rest.find('/').unwrap_or(rest.len());
    let host_part = &rest[..slash];
    let path = if slash == rest.len() { "/" } else { &rest[slash..] };
    let (host, port) = match host_part.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse::<u16>().map_err(|_| "bad port")?),
        None => (host_part.to_string(), 80),
    };
    Ok((host, port, path.to_string()))
}
