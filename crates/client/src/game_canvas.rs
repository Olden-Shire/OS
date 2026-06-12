// @ObfuscatedName("fk")
//
// Java extends java.awt.Canvas; winit + softbuffer own the real window
// surface in this port, so the canvas is a size-bearing placeholder that
// mirrors the gamepack field layout.

#![allow(dead_code)]

pub struct GameCanvas {
    // @ObfuscatedName("fk.r")
    pub component_id: u32,

    // custom — backing dimensions for the placeholder canvas
    pub width: u32,

    // custom
    pub height: u32,
}

impl GameCanvas {
    // custom — Java ctor `GameCanvas(Component c)` has no @ObfuscatedName
    pub fn new(component_id: u32, width: u32, height: u32) -> Self {
        Self { component_id, width, height }
    }
}
