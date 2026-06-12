// @ObfuscatedName("dg")
//
// jag::Linkable — base of every node that participates in a LinkList /
// HashTable. Holds the doubly-linked-list pointers + the 64-bit key the
// HashTable buckets index by.

#![allow(dead_code)]

use std::ptr;

// repr(C) is required because the JS5 code casts `*mut Linkable` →
// `*mut Js5NetRequest` (and similar) assuming Linkable lives at offset 0
// of the embedding struct. The default Rust layout is allowed to reorder
// fields and would silently break those casts.
#[repr(C)]
pub struct Linkable {
    // @ObfuscatedName("dg.r")
    pub key: i64,

    // @ObfuscatedName("dg.d")
    pub next: *mut Linkable,

    // @ObfuscatedName("dg.l")
    pub prev: *mut Linkable,
}

impl Linkable {
    pub fn new() -> Self {
        Self { key: 0, next: ptr::null_mut(), prev: ptr::null_mut() }
    }

    // @ObfuscatedName("dg.r()V")
    //
    // SAFETY: caller guarantees that `self.next`/`self.prev`, if non-null,
    // point at live Linkables reachable from a list/table sentinel.
    pub unsafe fn unlink(&mut self) {
        if !self.prev.is_null() {
            unsafe {
                (*self.prev).next = self.next;
                (*self.next).prev = self.prev;
            }
            self.next = ptr::null_mut();
            self.prev = ptr::null_mut();
        }
    }

    // @ObfuscatedName("dg.d()Z")
    pub fn is_linked(&self) -> bool {
        !self.prev.is_null()
    }
}

impl Default for Linkable {
    fn default() -> Self {
        Self::new()
    }
}
