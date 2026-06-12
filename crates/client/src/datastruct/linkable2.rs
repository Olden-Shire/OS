// @ObfuscatedName("en")
//
// jagex3.datastruct.Linkable2 — Linkable plus a second pair of next/prev
// pointers, so a single node can be a member of both a primary list/hashtable
// (Linkable) and a secondary queue (Linkable2) at the same time. Js5NetRequest
// relies on this (it sits in a HashTable via Linkable AND in a request queue
// via Linkable2).

#![allow(dead_code)]

use std::ptr;

use super::linkable::Linkable;

// repr(C) for the same reason as Linkable — the JS5 code relies on the
// embedded Linkable sitting at offset 0 of every Linkable2.
#[repr(C)]
pub struct Linkable2 {
    // Java extends Linkable; we embed.
    pub base: Linkable,

    // @ObfuscatedName("en.m")
    pub next2: *mut Linkable2,

    // @ObfuscatedName("en.c")
    pub prev2: *mut Linkable2,
}

impl Linkable2 {
    pub fn new() -> Self {
        Self { base: Linkable::new(), next2: ptr::null_mut(), prev2: ptr::null_mut() }
    }

    // @ObfuscatedName("en.c()V")
    //
    // SAFETY: caller guarantees `self.next2`/`self.prev2` are live.
    pub unsafe fn unlink2(&mut self) {
        if !self.prev2.is_null() {
            unsafe {
                (*self.prev2).next2 = self.next2;
                (*self.next2).prev2 = self.prev2;
            }
            self.next2 = ptr::null_mut();
            self.prev2 = ptr::null_mut();
        }
    }
}

impl Default for Linkable2 {
    fn default() -> Self {
        Self::new()
    }
}
