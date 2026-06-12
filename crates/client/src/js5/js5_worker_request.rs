// @ObfuscatedName("eh")
//
// jagex3.js5.Js5WorkerRequest — a disk-cache read/write job. Held by the
// Js5NetThread worker queue.

#![allow(dead_code)]

use crate::datastruct::linkable::Linkable;

// repr(C) — same rationale as Js5NetRequest; Linkable must be at offset 0.
#[repr(C)]
pub struct Js5WorkerRequest {
    // Java `extends Linkable`; we embed.
    pub base: Linkable,

    // @ObfuscatedName("eh.m") — 0 = write, 1 = read
    pub req_type: i32,

    // @ObfuscatedName("eh.c")
    pub data: Option<Vec<u8>>,

    // @ObfuscatedName("eh.n") — DataFile slot (archive id)
    pub fs: i32,

    // @ObfuscatedName("eh.j") — Js5Loader slot id
    pub field1773: i32,
}

impl Js5WorkerRequest {
    pub fn new() -> Self {
        Self { base: Linkable::new(), req_type: 0, data: None, fs: -1, field1773: -1 }
    }
}

impl Default for Js5WorkerRequest {
    fn default() -> Self {
        Self::new()
    }
}
