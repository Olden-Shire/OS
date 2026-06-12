// @ObfuscatedName("ci")
//
// jagex3.datastruct.LinkList2 — same as LinkList but operates on the second
// pair of pointers (next2/prev2) so a node can be in two lists at once.

#![allow(dead_code)]

use std::ptr;

use super::linkable2::Linkable2;

pub struct LinkList2 {
    // @ObfuscatedName("ci.r")
    pub sentinel: Box<Linkable2>,
}

impl LinkList2 {
    pub fn new() -> Self {
        let mut sentinel = Box::new(Linkable2::new());
        let s: *mut Linkable2 = &mut *sentinel;
        sentinel.next2 = s;
        sentinel.prev2 = s;
        Self { sentinel }
    }

    fn sentinel_ptr(&mut self) -> *mut Linkable2 {
        &mut *self.sentinel as *mut Linkable2
    }

    // @ObfuscatedName("ci.r(Len;)V")
    pub unsafe fn push(&mut self, node: *mut Linkable2) {
        unsafe {
            if !(*node).prev2.is_null() {
                (*node).unlink2();
            }
            let s = self.sentinel_ptr();
            (*node).prev2 = (*s).prev2;
            (*node).next2 = s;
            (*(*node).prev2).next2 = node;
            (*(*node).next2).prev2 = node;
        }
    }

    // @ObfuscatedName("ci.d(Len;)V")
    pub unsafe fn push_front(&mut self, node: *mut Linkable2) {
        unsafe {
            if !(*node).prev2.is_null() {
                (*node).unlink2();
            }
            let s = self.sentinel_ptr();
            (*node).prev2 = s;
            (*node).next2 = (*s).next2;
            (*(*node).prev2).next2 = node;
            (*(*node).next2).prev2 = node;
        }
    }

    // @ObfuscatedName("ci.l()Len;")
    pub fn pop_front(&mut self) -> *mut Linkable2 {
        let s = self.sentinel_ptr();
        unsafe {
            let node = (*s).next2;
            if node == s {
                return ptr::null_mut();
            }
            (*node).unlink2();
            node
        }
    }

    // @ObfuscatedName("ci.m()Len;")
    pub fn next(&mut self) -> *mut Linkable2 {
        let s = self.sentinel_ptr();
        unsafe {
            let node = (*s).next2;
            if node == s { ptr::null_mut() } else { node }
        }
    }

    // @ObfuscatedName("ci.c()V")
    pub fn clear(&mut self) {
        let s = self.sentinel_ptr();
        unsafe {
            loop {
                let node = (*s).next2;
                if node == s {
                    return;
                }
                (*node).unlink2();
            }
        }
    }
}

impl Default for LinkList2 {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl Send for LinkList2 {}
