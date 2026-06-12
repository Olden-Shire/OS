// @ObfuscatedName("cy") — jag::oldscape::client::ClientInvCache.
//
// Per-inventory item arrays keyed by inventory id. Java extends
// Linkable and maintains a static HashTable + LRU eviction; UPD_INVS
// opcodes look up an inv id and replace / merge slots.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct ClientInvCache {
    // @ObfuscatedName("cy.r") — inventory id (matches InvType.id).
    pub id: i32,
    // @ObfuscatedName("cy.d") — per-slot object ids (-1 = empty).
    pub obj_ids: Vec<i32>,
    // @ObfuscatedName("cy.l") — per-slot stack counts.
    pub obj_counts: Vec<i32>,
    // @ObfuscatedName("cy.m") — transmit counter; bumped by UPD_INVS.
    pub transmit_num: i32,
}

impl ClientInvCache {
    pub fn new(id: i32, capacity: usize) -> Self {
        Self {
            id,
            obj_ids: vec![-1; capacity],
            obj_counts: vec![0; capacity],
            transmit_num: 0,
        }
    }

    pub fn set_slot(&mut self, slot: usize, obj_id: i32, count: i32) {
        if slot < self.obj_ids.len() {
            self.obj_ids[slot] = obj_id;
            self.obj_counts[slot] = count;
        }
    }

    pub fn clear(&mut self) {
        for v in self.obj_ids.iter_mut() { *v = -1; }
        for v in self.obj_counts.iter_mut() { *v = 0; }
    }
}

// @ObfuscatedName("cy.s") — INV_LIST static cache (Java's HashTable).
pub static INV_LIST: std::sync::LazyLock<Mutex<HashMap<i32, ClientInvCache>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn get_or_create(inv_id: i32, capacity: usize) -> ClientInvCache {
    let mut list = INV_LIST.lock().unwrap();
    if let Some(c) = list.get(&inv_id) { return c.clone(); }
    let fresh = ClientInvCache::new(inv_id, capacity);
    list.insert(inv_id, fresh.clone());
    fresh
}

pub fn upsert(cache: ClientInvCache) {
    INV_LIST.lock().unwrap().insert(cache.id, cache);
}

pub fn clear_all() {
    INV_LIST.lock().unwrap().clear();
}

// @ObfuscatedName(— ClientInvCache.deleteAll). Java alias for the
// reconnect path — drops every cached inv map so the server can
// re-push the full state.
pub fn delete_all() {
    clear_all();
}

// @ObfuscatedName("r.c(III)I") — ClientInvCache.getCount. Verbatim
// port of ClientInvCache.java:23-32.
pub fn get_count(inv_id: i32, slot: i32) -> i32 {
    let list = INV_LIST.lock().unwrap();
    let Some(c) = list.get(&inv_id) else { return 0; };
    if slot < 0 || (slot as usize) >= c.obj_counts.len() { return 0; }
    c.obj_counts[slot as usize]
}

// @ObfuscatedName("ClientInvCache.getType") — Verbatim port of
// ClientInvCache.java:89-98.
pub fn get_type(inv_id: i32, slot: i32) -> i32 {
    let list = INV_LIST.lock().unwrap();
    let Some(c) = list.get(&inv_id) else { return -1; };
    if slot < 0 || (slot as usize) >= c.obj_ids.len() { return -1; }
    c.obj_ids[slot as usize]
}

// @ObfuscatedName("dj.n(IIB)I") — ClientInvCache.invTotal. Verbatim
// port of ClientInvCache.java:36-51. Sums every slot whose obj id
// matches `obj_id`. obj_id == -1 → 0 (Java's "no item" sentinel).
pub fn inv_total(inv_id: i32, obj_id: i32) -> i32 {
    if obj_id == -1 { return 0; }
    let list = INV_LIST.lock().unwrap();
    let Some(c) = list.get(&inv_id) else { return 0; };
    let mut total = 0i32;
    for (i, &slot_id) in c.obj_ids.iter().enumerate() {
        if slot_id == obj_id {
            total = total.saturating_add(*c.obj_counts.get(i).unwrap_or(&0));
        }
    }
    total
}

// @ObfuscatedName("n.z(IB)V") — ClientInvCache.delete.
pub fn delete(inv_id: i32) {
    INV_LIST.lock().unwrap().remove(&inv_id);
}

// @ObfuscatedName("fh.j(IIIII)V") — ClientInvCache.set. Verbatim
// port of ClientInvCache.java:55-77 — upserts a single slot, growing
// the per-cache obj_ids / obj_counts vectors with -1 / 0 fill if the
// slot index exceeds current capacity.
pub fn set(inv_id: i32, slot: i32, obj_id: i32, count: i32) {
    if slot < 0 { return; }
    let mut list = INV_LIST.lock().unwrap();
    let cache = list.entry(inv_id).or_insert_with(|| ClientInvCache::new(inv_id, 1));
    let needed = slot as usize + 1;
    if cache.obj_ids.len() < needed {
        cache.obj_ids.resize(needed, -1);
        cache.obj_counts.resize(needed, 0);
    }
    cache.obj_ids[slot as usize] = obj_id;
    cache.obj_counts[slot as usize] = count;
}
