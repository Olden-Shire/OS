// @ObfuscatedName("cg")
//
// jagex3.datastruct.LinkList — doubly-linked list of Linkable nodes with a
// stable sentinel + cursor for iteration. The list does NOT own the nodes
// it holds (the caller does, via Box); it only wires up next/prev pointers.

#![allow(dead_code)]

use std::ptr;

use super::linkable::Linkable;

pub struct LinkList {
    // @ObfuscatedName("cg.r")
    pub sentinel: Box<Linkable>,

    // @ObfuscatedName("cg.d")
    pub cursor: *mut Linkable,
}

impl LinkList {
    pub fn new() -> Self {
        let mut sentinel = Box::new(Linkable::new());
        let s: *mut Linkable = &mut *sentinel;
        sentinel.next = s;
        sentinel.prev = s;
        Self { sentinel, cursor: ptr::null_mut() }
    }

    fn sentinel_ptr(&mut self) -> *mut Linkable {
        &mut *self.sentinel as *mut Linkable
    }

    // @ObfuscatedName("cg.r()V")
    pub fn clear(&mut self) {
        let s = self.sentinel_ptr();
        unsafe {
            loop {
                let node = (*s).next;
                if node == s {
                    self.cursor = ptr::null_mut();
                    return;
                }
                (*node).unlink();
            }
        }
    }

    // @ObfuscatedName("cg.d(Ldg;)V")
    //
    // SAFETY: `node` must point to a live Linkable that outlives the list.
    pub unsafe fn push(&mut self, node: *mut Linkable) {
        unsafe {
            if !(*node).prev.is_null() {
                (*node).unlink();
            }
            let s = self.sentinel_ptr();
            (*node).prev = (*s).prev;
            (*node).next = s;
            (*(*node).prev).next = node;
            (*(*node).next).prev = node;
        }
    }

    // @ObfuscatedName("cg.l(Ldg;)V")
    pub unsafe fn push_front(&mut self, node: *mut Linkable) {
        unsafe {
            if !(*node).prev.is_null() {
                (*node).unlink();
            }
            let s = self.sentinel_ptr();
            (*node).prev = s;
            (*node).next = (*s).next;
            (*(*node).prev).next = node;
            (*(*node).next).prev = node;
        }
    }

    // @ObfuscatedName("cg.m(Ldg;Ldg;)V")
    pub unsafe fn insert_before(node1: *mut Linkable, node2: *mut Linkable) {
        unsafe {
            if !(*node1).prev.is_null() {
                (*node1).unlink();
            }
            (*node1).prev = (*node2).prev;
            (*node1).next = node2;
            (*(*node1).prev).next = node1;
            (*(*node1).next).prev = node1;
        }
    }

    // @ObfuscatedName("cg.c()Ldg;")
    pub fn pop_front(&mut self) -> *mut Linkable {
        let s = self.sentinel_ptr();
        unsafe {
            let node = (*s).next;
            if node == s {
                return ptr::null_mut();
            }
            (*node).unlink();
            node
        }
    }

    // @ObfuscatedName("cg.n()Ldg;")
    pub fn pop(&mut self) -> *mut Linkable {
        let s = self.sentinel_ptr();
        unsafe {
            let node = (*s).prev;
            if node == s {
                return ptr::null_mut();
            }
            (*node).unlink();
            node
        }
    }

    // @ObfuscatedName("cg.j()Ldg;")
    pub fn head(&mut self) -> *mut Linkable {
        let s = self.sentinel_ptr();
        unsafe {
            let node = (*s).next;
            if node == s {
                self.cursor = ptr::null_mut();
                ptr::null_mut()
            } else {
                self.cursor = (*node).next;
                node
            }
        }
    }

    // @ObfuscatedName("cg.z()Ldg;")
    pub fn tail(&mut self) -> *mut Linkable {
        let s = self.sentinel_ptr();
        unsafe {
            let node = (*s).prev;
            if node == s {
                self.cursor = ptr::null_mut();
                ptr::null_mut()
            } else {
                self.cursor = (*node).prev;
                node
            }
        }
    }

    // @ObfuscatedName("cg.g()Ldg;")
    pub fn next(&mut self) -> *mut Linkable {
        let s = self.sentinel_ptr();
        let node = self.cursor;
        unsafe {
            if node == s {
                self.cursor = ptr::null_mut();
                ptr::null_mut()
            } else {
                self.cursor = (*node).next;
                node
            }
        }
    }

    // @ObfuscatedName("cg.q()Ldg;")
    pub fn prev(&mut self) -> *mut Linkable {
        let s = self.sentinel_ptr();
        let node = self.cursor;
        unsafe {
            if node == s {
                self.cursor = ptr::null_mut();
                ptr::null_mut()
            } else {
                self.cursor = (*node).prev;
                node
            }
        }
    }
}

impl Default for LinkList {
    fn default() -> Self {
        Self::new()
    }
}

// Safety: LinkList holds raw pointers into nodes owned by callers. The
// caller is responsible for synchronising access; the type itself is no
// less Send/Sync than an &mut to its sentinel.
unsafe impl Send for LinkList {}
