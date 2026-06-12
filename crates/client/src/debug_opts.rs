// custom — renderer debug toggles, surfaced as checkboxes in the imgui
// perf overlay. Plain atomics so render-path reads stay lock-free.

use std::sync::atomic::{AtomicBool, Ordering};

/// Lock the orbit camera to Java's fixed zoom (distance = pitch*3 + 600,
/// wheel ignored) for byte-accuracy comparisons against the reference
/// client.
static VANILLA_CAMERA: AtomicBool = AtomicBool::new(false);

/// Drop the top render level to `minusedlevel` everywhere, not just when
/// the camera→player walk crosses an "inside" tile.
static ALWAYS_HIDE_ROOFS: AtomicBool = AtomicBool::new(false);

/// Render every built tile (no 25-tile radius, no visibility-matrix
/// gating) and push the model far-cull out to match.
static EXTENDED_DRAW: AtomicBool = AtomicBool::new(false);

pub fn vanilla_camera() -> bool { VANILLA_CAMERA.load(Ordering::Relaxed) }
pub fn set_vanilla_camera(v: bool) { VANILLA_CAMERA.store(v, Ordering::Relaxed); }

pub fn always_hide_roofs() -> bool { ALWAYS_HIDE_ROOFS.load(Ordering::Relaxed) }
pub fn set_always_hide_roofs(v: bool) { ALWAYS_HIDE_ROOFS.store(v, Ordering::Relaxed); }

pub fn extended_draw() -> bool { EXTENDED_DRAW.load(Ordering::Relaxed) }
pub fn set_extended_draw(v: bool) { EXTENDED_DRAW.store(v, Ordering::Relaxed); }
