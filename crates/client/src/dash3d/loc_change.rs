// @ObfuscatedName("dn") — jag::oldscape::LocChange.
//
// Pending location change record — the LOC_ADD_CHANGE / LOC_DEL
// packets enqueue these onto a per-level LinkList so the world
// renderer can run the change at a precise tick. After endTime the
// loc is replaced by oldType (rollback for timed changes like
// herblore traps reverting).
//
// Verbatim port of LocChange.java.

#![allow(dead_code)]

#[derive(Debug, Clone)]
pub struct LocChange {
    // @ObfuscatedName("dn.m")
    pub level: i32,
    // @ObfuscatedName("dn.c") — 0 wall, 1 wallDecor, 2 normal loc, 3
    // ground decor. Matches the Java `layer` enum used by addLoc.
    pub layer: i32,
    // @ObfuscatedName("dn.n")
    pub x: i32,
    // @ObfuscatedName("dn.j")
    pub z: i32,
    // @ObfuscatedName("dn.z")
    pub old_type: i32,
    // @ObfuscatedName("dn.g")
    pub old_angle: i32,
    // @ObfuscatedName("dn.q")
    pub old_shape: i32,
    // @ObfuscatedName("dn.i")
    pub new_type: i32,
    // @ObfuscatedName("dn.s")
    pub new_angle: i32,
    // @ObfuscatedName("dn.u")
    pub new_shape: i32,
    // @ObfuscatedName("dn.v")
    pub start_time: i32,
    // @ObfuscatedName("dn.w") — -1 means "permanent" (no rollback).
    pub end_time: i32,
}

impl Default for LocChange {
    fn default() -> Self {
        Self {
            level: 0, layer: 0, x: 0, z: 0,
            old_type: -1, old_angle: 0, old_shape: 0,
            new_type: -1, new_angle: 0, new_shape: 0,
            start_time: 0, end_time: -1,
        }
    }
}

impl LocChange {
    pub fn new() -> Self { Self::default() }
}
