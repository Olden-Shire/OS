// @ObfuscatedName("cc")
// jag::oldscape::jagex3::Js5NetThread
//
// jagex3.js5.Js5NetThread — worker thread that services the disk cache on
// behalf of Js5Loader (read/write groups + index files). With the disk
// cache stubbed (`DataFile` is a no-op), this worker has nothing to do but
// we keep the queue/lock surface so call sites stay verbatim.

#![allow(dead_code)]

use std::sync::{LazyLock, Mutex};

use crate::datastruct::link_list::LinkList;

pub struct Js5NetThreadState {
    // @ObfuscatedName("cc.r")
    pub request_queue: LinkList,

    // @ObfuscatedName("cc.d")
    pub completed: LinkList,

    // @ObfuscatedName("cc.l")
    pub keep_alive: i32,
}

impl Js5NetThreadState {
    fn new() -> Self {
        Self { request_queue: LinkList::new(), completed: LinkList::new(), keep_alive: 0 }
    }
}

// @ObfuscatedName("cc.m") — the synchronisation monitor in Java. Folded
// into the outer Mutex<Js5NetThreadState> because we don't need a
// separate notify channel.
pub static STATE: LazyLock<Mutex<Js5NetThreadState>> =
    LazyLock::new(|| Mutex::new(Js5NetThreadState::new()));

// @ObfuscatedName("cu.m(ILap;Ldq;I)V")
//
// Java: queue a read against DataFile; if a write for the same key is
// still in the request queue, deliver that buffer back directly. With the
// disk-cache stubbed there are never queued writes, so we just call
// loadIndex with `None` (no data) — Js5Loader treats that as "fall back
// to a network request."
pub fn queue_request(_key: i32, _fs: i32, _loader: i32) {
    // disk cache stubbed; nothing to do.
}

// @ObfuscatedName("bv.c(B)V")
pub fn shutdown() {
    let mut s = STATE.lock().unwrap();
    s.keep_alive = 0;
    s.request_queue.clear();
    s.completed.clear();
}
