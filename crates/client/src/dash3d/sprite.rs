// @ObfuscatedName("au") — jag::oldscape::dash3d::Sprite.
//
// A free-standing scene element occupying a rectangle of tiles —
// scenery locs (kind 9-11), and at runtime players / NPCs /
// projectiles / spotanims, all placed via World.setSprite. The fill
// renderer draws them back-to-front by tile distance.
//
// Verbatim port of Sprite.java. Note: this is the dash3d Sprite, NOT
// the graphics::Sprite which is a 2D pixel buffer.

#![allow(dead_code)]

use std::sync::Arc;

use crate::dash3d::model_source::ModelSource;

#[derive(Clone, Default)]
pub struct Sprite {
    // @ObfuscatedName("au.r")
    pub level: i32,
    // @ObfuscatedName("au.d") — world-space Y (height).
    pub y: i32,
    // @ObfuscatedName("au.l") — world-space X.
    pub x: i32,
    // @ObfuscatedName("au.m") — facing yaw 0..2047.
    pub yaw: i32,
    // @ObfuscatedName("au.c") — world-space Z.
    pub z: i32,
    // @ObfuscatedName("au.n") — ModelSource reference.
    pub model: Option<Arc<ModelSource>>,
    // @ObfuscatedName("au.j")
    pub min_tile_x: i32,
    // @ObfuscatedName("au.z")
    pub max_tile_x: i32,
    // @ObfuscatedName("au.g")
    pub min_tile_z: i32,
    // @ObfuscatedName("au.q")
    pub max_tile_z: i32,
    // @ObfuscatedName("au.i") — render distance — sorted descending
    // during the back-to-front pass.
    pub distance: i32,
    // @ObfuscatedName("au.s") — cycle stamp; equal to World.cycleNo
    // once the sprite has rendered this frame.
    pub cycle: i32,
    // @ObfuscatedName("au.u")
    pub typecode: i32,
    // @ObfuscatedName("au.v")
    pub typecode2: i32,
}

impl Sprite {
    pub fn new() -> Self {
        Self::default()
    }
}
