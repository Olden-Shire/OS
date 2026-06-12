// @ObfuscatedName("ah")
//
// jagex3.client.applet.PrivilegedRequest — futures-on-a-thread. The
// gamepack uses it to ferry socket open / DNS / thread spawn requests off
// the worker thread; we mirror the same status field so callers can
// compare against the gamepack's status codes 0/1/2 directly.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

pub const STATUS_PENDING: i32 = 0;
pub const STATUS_DONE: i32 = 1;
pub const STATUS_ERROR: i32 = 2;

pub enum Result {
    None,
    Socket(crate::io::NetSocket),
    #[cfg(not(target_arch = "wasm32"))]
    Thread(std::thread::JoinHandle<()>),
    Hostname(String),
}

pub struct PrivilegedRequest {
    // @ObfuscatedName("ah.r")
    pub req_type: i32,

    // @ObfuscatedName("ah.d")
    pub int_arg: i32,

    // @ObfuscatedName("ah.l")
    pub obj_arg: Option<String>,

    // @ObfuscatedName("ah.m")
    pub status: Arc<Mutex<i32>>,

    // @ObfuscatedName("ah.c")
    pub result: Arc<Mutex<Result>>,

    // @ObfuscatedName("ah.n")
    pub next: Option<Box<PrivilegedRequest>>,

    // wasm: socket-open requests resolve from the WebSocket's async
    // callbacks rather than a worker thread; status() consults this.
    #[cfg(target_arch = "wasm32")]
    pub ws_shared: Option<std::rc::Rc<std::cell::RefCell<crate::io::ws_socket::WsShared>>>,
}

impl PrivilegedRequest {
    pub fn new(req_type: i32, int_arg: i32, obj_arg: Option<String>) -> Self {
        Self {
            req_type,
            int_arg,
            obj_arg,
            status: Arc::new(Mutex::new(STATUS_PENDING)),
            result: Arc::new(Mutex::new(Result::None)),
            next: None,
            #[cfg(target_arch = "wasm32")]
            ws_shared: None,
        }
    }

    pub fn status(&self) -> i32 {
        #[cfg(target_arch = "wasm32")]
        if let Some(shared) = &self.ws_shared {
            let s = shared.borrow();
            return if s.error || s.closed {
                STATUS_ERROR
            } else if s.open {
                STATUS_DONE
            } else {
                STATUS_PENDING
            };
        }
        *self.status.lock().unwrap()
    }
}
