// @ObfuscatedName("cf")
//
// jagex3.datastruct.HashTable — power-of-two-bucket intrusive hashtable.
// Each bucket is a circular linked list anchored on a sentinel Linkable.
// Iteration uses a search cursor that survives `find` so the gamepack's
// `searchCursor != null` checks port directly.

#![allow(dead_code)]

use std::ptr;

use super::linkable::Linkable;

pub struct HashTable {
    // @ObfuscatedName("cf.r")
    pub bucket_count: i32,

    // @ObfuscatedName("cf.d")
    pub buckets: Vec<Box<Linkable>>,

    // @ObfuscatedName("cf.l")
    pub search_cursor: *mut Linkable,

    // @ObfuscatedName("cf.m")
    pub iterator_cursor: *mut Linkable,

    // @ObfuscatedName("cf.c")
    pub iterator_bucket: i32,
}

impl HashTable {
    pub fn new(bucket_count: i32) -> Self {
        let mut buckets: Vec<Box<Linkable>> = Vec::with_capacity(bucket_count as usize);
        for _ in 0..bucket_count {
            let mut sentinel = Box::new(Linkable::new());
            let s: *mut Linkable = &mut *sentinel;
            sentinel.next = s;
            sentinel.prev = s;
            buckets.push(sentinel);
        }
        Self {
            bucket_count,
            buckets,
            search_cursor: ptr::null_mut(),
            iterator_cursor: ptr::null_mut(),
            iterator_bucket: 0,
        }
    }

    fn bucket_sentinel(&mut self, idx: usize) -> *mut Linkable {
        &mut *self.buckets[idx] as *mut Linkable
    }

    // @ObfuscatedName("cf.r(J)Ldg;")
    pub fn find(&mut self, key: i64) -> *mut Linkable {
        let mask = (self.bucket_count - 1) as i64;
        let idx = (key & mask) as usize;
        let sentinel = self.bucket_sentinel(idx);
        unsafe {
            self.search_cursor = (*sentinel).next;
            while self.search_cursor != sentinel {
                if (*self.search_cursor).key == key {
                    let value = self.search_cursor;
                    self.search_cursor = (*self.search_cursor).next;
                    return value;
                }
                self.search_cursor = (*self.search_cursor).next;
            }
        }
        self.search_cursor = ptr::null_mut();
        ptr::null_mut()
    }

    // @ObfuscatedName("cf.d(Ldg;J)V")
    pub unsafe fn put(&mut self, node: *mut Linkable, key: i64) {
        unsafe {
            if !(*node).prev.is_null() {
                (*node).unlink();
            }
        }
        let mask = (self.bucket_count - 1) as i64;
        let idx = (key & mask) as usize;
        let sentinel = self.bucket_sentinel(idx);
        unsafe {
            (*node).prev = (*sentinel).prev;
            (*node).next = sentinel;
            (*(*node).prev).next = node;
            (*(*node).next).prev = node;
            (*node).key = key;
        }
    }

    // @ObfuscatedName("cf.l()V")
    pub fn clear(&mut self) {
        for i in 0..self.bucket_count {
            let sentinel = self.bucket_sentinel(i as usize);
            unsafe {
                loop {
                    let node = (*sentinel).next;
                    if node == sentinel {
                        break;
                    }
                    (*node).unlink();
                }
            }
        }
        self.search_cursor = ptr::null_mut();
        self.iterator_cursor = ptr::null_mut();
    }

    // @ObfuscatedName("cf.m()Ldg;")
    pub fn search(&mut self) -> *mut Linkable {
        self.iterator_bucket = 0;
        self.findnext()
    }

    // @ObfuscatedName("cf.c()Ldg;")
    pub fn findnext(&mut self) -> *mut Linkable {
        unsafe {
            if self.iterator_bucket > 0
                && self.bucket_sentinel((self.iterator_bucket - 1) as usize) != self.iterator_cursor
            {
                let node = self.iterator_cursor;
                self.iterator_cursor = (*node).next;
                return node;
            }
            loop {
                if self.iterator_bucket >= self.bucket_count {
                    return ptr::null_mut();
                }
                let sentinel = self.bucket_sentinel(self.iterator_bucket as usize);
                self.iterator_bucket += 1;
                let node = (*sentinel).next;
                if self.bucket_sentinel((self.iterator_bucket - 1) as usize) != node {
                    self.iterator_cursor = (*node).next;
                    return node;
                }
            }
        }
    }
}

unsafe impl Send for HashTable {}
