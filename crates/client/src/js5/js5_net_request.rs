// @ObfuscatedName("fl")
//
// jagex3.js5.Js5NetRequest — a single in-flight download. Sits in both a
// HashTable (via Linkable.key) and a request queue (via Linkable2), which is
// why it extends Linkable2.

#![allow(dead_code)]

use crate::datastruct::linkable2::Linkable2;

// repr(C) — Js5Net casts `*mut Linkable` (hashtable bucket entries) back to
// `*mut Js5NetRequest`. The cast is only sound if the embedded Linkable
// lives at offset 0, which Rust's default layout doesn't guarantee.
#[repr(C)]
pub struct Js5NetRequest {
    // Java `extends Linkable2`; we embed.
    pub base: Linkable2,

    // @ObfuscatedName("fl.n") — Js5Loader id (we resolve via Client static slot)
    pub provider: i32,

    // @ObfuscatedName("fl.j")
    pub expected_crc: i32,

    // @ObfuscatedName("fl.z")
    pub padding: i8,
}

impl Js5NetRequest {
    pub fn new() -> Self {
        Self { base: Linkable2::new(), provider: -1, expected_crc: 0, padding: 0 }
    }
}

impl Default for Js5NetRequest {
    fn default() -> Self {
        Self::new()
    }
}
