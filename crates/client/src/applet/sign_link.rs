// @ObfuscatedName("ak")
//
// jagex3.client.applet.SignLink — privileged worker thread that the
// gamepack hands socket-open / thread-spawn / DNS jobs to. The port spawns
// each request onto its own Rust std::thread so we keep the same async
// status semantics without porting Java's `notify()` pump.

#![allow(dead_code)]

#[cfg(not(target_arch = "wasm32"))]
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::Ordering;
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::thread;

use super::privileged_request::{PrivilegedRequest, Result, STATUS_DONE, STATUS_ERROR};

pub struct SignLink {
    // @ObfuscatedName("ak.r")
    pub java_vendor: String,

    // @ObfuscatedName("ak.d")
    pub java_version: String,

    // @ObfuscatedName("ak.l") — AudioSource, deferred.
    pub audio: (),

    // @ObfuscatedName("ak.j")
    pub is_closed: bool,
}

impl SignLink {
    pub fn new() -> Self {
        Self {
            java_vendor: "Rust".to_string(),
            java_version: "1.0".to_string(),
            audio: (),
            is_closed: false,
        }
    }

    // @ObfuscatedName("ak.m(B)V")
    pub fn close(&mut self) {
        self.is_closed = true;
    }

    // @ObfuscatedName("ak.c(IIILjava/lang/Object;S)Lah;")
    //
    // wasm: no worker threads in the browser — socket opens map onto the
    // WebSocket's own async connect (status() polls its open/error flags),
    // and the other request types complete inline.
    #[cfg(target_arch = "wasm32")]
    pub fn new_request(&self, req_type: i32, int_arg: i32, _arg2: i32, obj_arg: Option<String>) -> PrivilegedRequest {
        let mut req = PrivilegedRequest::new(req_type, int_arg, obj_arg.clone());
        match req_type {
            1 => {
                let host = obj_arg.as_deref().unwrap_or("127.0.0.1");
                match crate::io::ws_socket::WsSocket::connect(host, int_arg) {
                    Ok(sock) => {
                        req.ws_shared = Some(sock.shared.clone());
                        *req.result.lock().unwrap() = Result::Socket(sock);
                    }
                    Err(_) => *req.status.lock().unwrap() = STATUS_ERROR,
                }
            }
            _ => *req.status.lock().unwrap() = STATUS_DONE,
        }
        req
    }

    // @ObfuscatedName("ak.c(IIILjava/lang/Object;S)Lah;")
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_request(&self, req_type: i32, int_arg: i32, _arg2: i32, obj_arg: Option<String>) -> PrivilegedRequest {
        let req = PrivilegedRequest::new(req_type, int_arg, obj_arg.clone());
        let status = req.status.clone();
        let result = req.result.clone();
        let int_arg_clone = int_arg;
        let obj_arg_clone = obj_arg;
        thread::spawn(move || {
            match req_type {
                1 => {
                    // Socket open: host=obj_arg, port=int_arg
                    if let Some(host) = obj_arg_clone.as_deref() {
                        let addr = format!("{host}:{int_arg_clone}");
                        match addr.to_socket_addrs() {
                            Ok(mut addrs) => match addrs.next() {
                                Some(a) => match TcpStream::connect(a) {
                                    Ok(s) => {
                                        *result.lock().unwrap() = Result::Socket(s);
                                        *status.lock().unwrap() = STATUS_DONE;
                                    }
                                    Err(_) => {
                                        *status.lock().unwrap() = STATUS_ERROR;
                                    }
                                },
                                None => *status.lock().unwrap() = STATUS_ERROR,
                            },
                            Err(_) => *status.lock().unwrap() = STATUS_ERROR,
                        }
                    } else {
                        *status.lock().unwrap() = STATUS_ERROR;
                    }
                }
                2 => {
                    // Thread spawn — caller passes Runnable as obj_arg; in
                    // the port we instead push named worker spawns in
                    // js5::net_thread::spawn_if_needed, so this branch is
                    // unused. Mark complete so callers don't spin.
                    *status.lock().unwrap() = STATUS_DONE;
                }
                _ => {
                    *status.lock().unwrap() = STATUS_DONE;
                }
            }
        });
        req
    }

    // @ObfuscatedName("ak.n(Ljava/lang/String;IB)Lah;")
    pub fn socketreq(&self, host: &str, port: i32) -> PrivilegedRequest {
        self.new_request(1, port, 0, Some(host.to_string()))
    }

    // @ObfuscatedName("ak.j(Ljava/lang/Runnable;II)Lah;")
    pub fn threadreq(&self, _runnable: (), priority: i32) -> PrivilegedRequest {
        self.new_request(2, priority, 0, None)
    }

    // @ObfuscatedName("ak.z(II)Lah;")
    pub fn dnsreq(&self, arg0: i32) -> PrivilegedRequest {
        self.new_request(3, arg0, 0, None)
    }

    // @ObfuscatedName("ak.q(I)Lw;")
    pub fn get_audio(&self) -> () {
        self.audio
    }
}

impl Default for SignLink {
    fn default() -> Self {
        Self::new()
    }
}

// @ObfuscatedName(— SignLink.flushEvents). Verbatim semantic port of
// SignLink.java:162-174. Java's flushEvents pumps AWT's EventQueue to
// give any pending UI events a chance to dispatch before the run loop
// proceeds; on the Rust side winit owns the event loop so there's no
// queue to drain. The hook stays as a no-op so that callers preserve
// the Java ordering — when an explicit pump is ever needed we'd plug
// the winit run handler here.
pub fn flush_events() {
    // Java sleeps up to 50ms total in 1ms bursts waiting for peekEvent;
    // since winit pumps events automatically per-frame, we just yield
    // the current thread once to mirror Java's "give other threads a
    // tick" intent. (No-op on wasm — there is only one thread.)
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::yield_now();
}

// Suppress unused-import warning under #[allow(dead_code)] when Ordering is
// only conditionally referenced from the helper above.
#[allow(dead_code)]
fn _force_use(_: Ordering, _: Arc<()>) {}
