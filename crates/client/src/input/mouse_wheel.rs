// @ObfuscatedName("ac") — jag::oldscape::client::input::MouseWheelInterface
// @ObfuscatedName("dh") — jag::oldscape::client::input::ClientMouseWheelListener
//
// Java separates the abstract MouseWheelInterface (getRotation) from
// the concrete ClientMouseWheelListener (the AWT MouseWheelListener
// impl that accumulates rotation between game ticks). Rust handles
// scroll inline in main.rs's winit handler today; we expose the type
// shape here so future ports can subscribe.

#![allow(dead_code)]

use std::sync::Mutex;

pub trait MouseWheelInterface {
    /// @ObfuscatedName("ac.r(I)I") — MouseWheelInterface.getRotation.
    /// Returns the accumulated rotation since the last call (signed:
    /// negative = scroll up).
    fn get_rotation(&mut self) -> i32;
}

pub struct ClientMouseWheelListener {
    pub rotation: i32,
}

impl Default for ClientMouseWheelListener {
    fn default() -> Self { Self { rotation: 0 } }
}

impl MouseWheelInterface for ClientMouseWheelListener {
    fn get_rotation(&mut self) -> i32 {
        let r = self.rotation;
        self.rotation = 0;
        r
    }
}

pub static LISTENER: Mutex<ClientMouseWheelListener> = Mutex::new(
    ClientMouseWheelListener { rotation: 0 }
);

// @ObfuscatedName("dh.r(Ljava/awt/event/MouseWheelEvent;I)V") —
// ClientMouseWheelListener.mouseWheelMoved. Called from winit's
// scroll handler.
pub fn on_wheel(delta: i32) {
    LISTENER.lock().unwrap().rotation += delta;
}
