// @ObfuscatedName("bk") — jag::oldscape::datastruct::LruCache.
//
// LRU cache used throughout the client (sprite cache 200, model cache
// 50, font cache 20, etc). Java's impl composes a HashTable for O(1)
// lookup with a LinkList2 to track eviction order; insertion at the
// head, eviction from the tail.
//
// We mirror with a Rust HashMap + VecDeque for the order list. The
// API exposes find / put / size / clear matching Java's signatures.

#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};

pub struct LruCache<V> {
    capacity: usize,
    map: HashMap<i64, V>,
    order: VecDeque<i64>,
}

impl<V: Clone> LruCache<V> {
    // @ObfuscatedName("bk.<init>(I)V") — LruCache.<init>.
    pub fn new(capacity: i32) -> Self {
        Self {
            capacity: capacity.max(1) as usize,
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    // @ObfuscatedName("bk.r(J)Lez;") — LruCache.find. Returns the
    // cached value and moves it to the head of the eviction list.
    pub fn find(&mut self, key: i64) -> Option<V> {
        if let Some(v) = self.map.get(&key).cloned() {
            // Move-to-front.
            if let Some(pos) = self.order.iter().position(|&k| k == key) {
                self.order.remove(pos);
            }
            self.order.push_front(key);
            return Some(v);
        }
        None
    }

    // @ObfuscatedName("bk.d(Lez;JI)V") — LruCache.put. Evicts the
    // tail entry if at capacity.
    pub fn put(&mut self, value: V, key: i64) {
        if self.map.contains_key(&key) {
            self.map.insert(key, value);
            if let Some(pos) = self.order.iter().position(|&k| k == key) {
                self.order.remove(pos);
            }
            self.order.push_front(key);
            return;
        }
        if self.map.len() >= self.capacity {
            if let Some(evicted) = self.order.pop_back() {
                self.map.remove(&evicted);
            }
        }
        self.map.insert(key, value);
        self.order.push_front(key);
    }

    // @ObfuscatedName("bk.l(I)V") — LruCache.clear.
    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }

    // @ObfuscatedName("ce.d(J)V") — LruCache.remove. Verbatim port of
    // LruCache.java:43-51. Drops a single entry by key and frees its
    // capacity slot. Java tracks `available` as a free-slot counter; we
    // emulate that implicitly via `map.len() vs capacity`, so the
    // observable effect of remove() is exactly "the next put() won't
    // evict if the cache was full".
    pub fn remove(&mut self, key: i64) -> Option<V> {
        let value = self.map.remove(&key)?;
        if let Some(pos) = self.order.iter().position(|&k| k == key) {
            self.order.remove(pos);
        }
        Some(value)
    }

    pub fn size(&self) -> i32 { self.map.len() as i32 }
}
