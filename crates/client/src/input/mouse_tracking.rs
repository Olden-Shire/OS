// @ObfuscatedName("j") — jag::oldscape::client::MouseTracking.
//
// Background sampler that captures the local mouse position into
// parallel x/y buffers. Client.gameLoop drains it into the
// EVENT_MOUSE_MOVE (72) anti-bot packet whenever a click happens or
// 40+ samples are pending (Client.java:2290-2352).
//
// Java spawns a daemon thread that polls MouseInfo.getPointerInfo()
// every 50ms; we sample from winit's mouse-move events instead, which
// gives at least the same coverage.

#![allow(dead_code)]

use std::sync::Mutex;

const BUFFER_SIZE: usize = 500;

pub struct MouseTracking {
    // @ObfuscatedName("j.r") / "j.d" — sample buffers.
    pub x: [i32; BUFFER_SIZE],
    pub y: [i32; BUFFER_SIZE],
    // @ObfuscatedName("j.l") — pending sample count.
    pub length: usize,
    // @ObfuscatedName("client.ak") — Client.mouseTracked: whether the
    // sampler is active (Java enables it post-login).
    pub tracked: bool,
}

pub static TRACKING: Mutex<MouseTracking> = Mutex::new(MouseTracking {
    x: [0; BUFFER_SIZE],
    y: [0; BUFFER_SIZE],
    length: 0,
    tracked: true,
});

// MouseTracking.sample — push the current (x, y). (-1, -1) is the
// "mouse left the window" sentinel Java encodes as position 524287.
pub fn sample(x: i32, y: i32) {
    let mut t = TRACKING.lock().unwrap();
    if !t.tracked {
        return;
    }
    if t.length < BUFFER_SIZE {
        let i = t.length;
        t.x[i] = x;
        t.y[i] = y;
        t.length += 1;
    }
}
