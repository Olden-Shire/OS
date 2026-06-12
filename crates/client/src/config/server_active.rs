// @ObfuscatedName("el") — jag::oldscape::rs2lib::ServerActive
//
// Pure bit-field extractors over the 32-bit eventCode that IfType
// instances carry. Java's ServerActive class collects all the masks
// in one place; we mirror that as standalone `pub fn` helpers (the
// "Linkable" wrapper class itself isn't needed — Rust just operates
// on the `i32` directly).
//
// All functions are pure: primitive in, primitive out, no globals.
// Used by the menu / drag / target / inv-swap dispatchers in
// client.rs and the cs2 cc_gettargetmask opcode.

#![allow(dead_code)]

// @ObfuscatedName("bh.r(II)Z") — ServerActive.pauseButton.
// Bit 0: "this component pauses script on click."
pub fn pause_button(event_code: i32) -> bool {
    (event_code & 0x1) != 0
}

// ServerActive.hasOp — bit (opindex + 1) is the per-op enable bit.
pub fn has_op(event_code: i32, op_index: i32) -> bool {
    ((event_code >> (op_index + 1)) & 0x1) != 0
}

// @ObfuscatedName("da.d(II)I") — ServerActive.targetMask.
// 6-bit mask at bits 11..16 selects which entity classes can be the
// target of a cs2 "use" prompt (player, npc, obj, loc, etc.).
pub fn target_mask(event_code: i32) -> i32 {
    (event_code >> 11) & 0x3F
}

// @ObfuscatedName("az.l(II)I") — ServerActive.serverDraggable.
// 3-bit mask at bits 17..19 — picks the drag-target component layer.
pub fn server_draggable(event_code: i32) -> i32 {
    (event_code >> 17) & 0x7
}

// ServerActive.isDragTarget — bit 20.
pub fn is_drag_target(event_code: i32) -> bool {
    ((event_code >> 20) & 0x1) != 0
}

// ServerActive.isUseTarget — bit 21.
pub fn is_use_target(event_code: i32) -> bool {
    ((event_code >> 21) & 0x1) != 0
}

// @ObfuscatedName("bn.m(II)Z") — ServerActive.isObjSwapEnabled.
pub fn is_obj_swap_enabled(event_code: i32) -> bool {
    ((event_code >> 28) & 0x1) != 0
}

pub fn is_obj_replace_enabled(event_code: i32) -> bool {
    ((event_code >> 29) & 0x1) != 0
}

pub fn is_obj_ops_enabled(event_code: i32) -> bool {
    ((event_code >> 30) & 0x1) != 0
}

pub fn is_obj_use_enabled(event_code: i32) -> bool {
    ((event_code >> 31) & 0x1) != 0
}
