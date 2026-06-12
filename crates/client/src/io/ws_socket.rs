// custom — browser WebSocket standing in for TcpStream on wasm (browsers
// have no raw TCP). The server speaks the same JS5/game byte protocols over
// binary WS frames (it sniffs the HTTP upgrade on its normal port), so this
// just maps a frame stream onto the byte-queue interface ClientStream
// polls: onmessage appends to an Rc'd queue, available() is its length,
// and writes go straight out (the browser buffers sends internally).

#![cfg(target_arch = "wasm32")]
#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{BinaryType, MessageEvent, WebSocket};

#[derive(Default)]
pub struct WsShared {
    pub queue: VecDeque<u8>,
    pub open: bool,
    pub error: bool,
    pub closed: bool,
}

pub struct WsSocket {
    ws: WebSocket,
    pub shared: Rc<RefCell<WsShared>>,
    // Keep the JS callbacks alive for the socket's lifetime.
    _onmessage: Closure<dyn FnMut(MessageEvent)>,
    _onopen: Closure<dyn FnMut()>,
    _onerror: Closure<dyn FnMut()>,
    _onclose: Closure<dyn FnMut()>,
}

fn clog(msg: String) {
    web_sys::console::log_1(&msg.into());
}

impl WsSocket {
    pub fn connect(host: &str, port: i32) -> Result<Self, String> {
        let url = format!("ws://{host}:{port}/");
        clog(format!("[ws] connect {url}"));
        let ws = WebSocket::new(&url).map_err(|e| format!("{e:?}"))?;
        ws.set_binary_type(BinaryType::Arraybuffer);

        let shared = Rc::new(RefCell::new(WsShared::default()));

        let s = shared.clone();
        let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |ev: MessageEvent| {
            if let Ok(buf) = ev.data().dyn_into::<js_sys::ArrayBuffer>() {
                let bytes = js_sys::Uint8Array::new(&buf).to_vec();
                let mut sh = s.borrow_mut();
                if sh.queue.is_empty() {
                    clog(format!("[ws] <- {} bytes (first of batch)", bytes.len()));
                }
                sh.queue.extend(bytes);
            } else {
                clog("[ws] <- non-arraybuffer message dropped".to_string());
            }
        });
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

        let s = shared.clone();
        let onopen = Closure::<dyn FnMut()>::new(move || {
            clog("[ws] open".to_string());
            s.borrow_mut().open = true;
        });
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));

        let s = shared.clone();
        let onerror = Closure::<dyn FnMut()>::new(move || {
            clog("[ws] error".to_string());
            s.borrow_mut().error = true;
        });
        ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));

        let s = shared.clone();
        let onclose = Closure::<dyn FnMut()>::new(move || {
            clog("[ws] close".to_string());
            s.borrow_mut().closed = true;
        });
        ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));

        Ok(Self {
            ws,
            shared,
            _onmessage: onmessage,
            _onopen: onopen,
            _onerror: onerror,
            _onclose: onclose,
        })
    }

    pub fn is_open(&self) -> bool {
        self.shared.borrow().open
    }

    pub fn is_dead(&self) -> bool {
        let s = self.shared.borrow();
        s.error || s.closed
    }

    pub fn available(&self) -> usize {
        self.shared.borrow().queue.len()
    }

    /// Pop up to `buf.len()` queued bytes; returns the count copied.
    pub fn read_into(&self, buf: &mut [u8]) -> usize {
        let mut s = self.shared.borrow_mut();
        let n = buf.len().min(s.queue.len());
        for slot in buf.iter_mut().take(n) {
            *slot = s.queue.pop_front().unwrap();
        }
        n
    }

    pub fn send(&self, data: &[u8]) -> Result<(), ()> {
        self.ws.send_with_u8_array(data).map_err(|e| {
            clog(format!("[ws] send failed: {e:?}"));
        })
    }

    pub fn close(&self) {
        let _ = self.ws.close();
    }
}

impl Drop for WsSocket {
    fn drop(&mut self) {
        // Detach callbacks before the closures are freed.
        self.ws.set_onmessage(None);
        self.ws.set_onopen(None);
        self.ws.set_onerror(None);
        self.ws.set_onclose(None);
        let _ = self.ws.close();
    }
}
