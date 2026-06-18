// custom — renderer/input toggles, surfaced as checkboxes in the imgui
// perf overlay. Plain atomics so render-path reads stay lock-free.
//
// DEFAULTS ARE VANILLA: every custom extension ships disabled so the
// out-of-the-box client behaves 1:1 with the Java reference. (One
// presentation-only exception: INTEGER_SCALE defaults on — it changes
// no game-frame bytes, just replicates each pixel k×k at present time.)

use std::sync::atomic::{AtomicBool, Ordering};

/// Middle-mouse drag camera rotation. No Java analogue — disabled by
/// default for 1:1.
static MIDDLE_DRAG_CAMERA: AtomicBool = AtomicBool::new(false);

/// Scroll-wheel camera zoom (orbit_cam_zoom instead of Java's fixed
/// distance = pitch*3+600). No Java analogue — disabled by default
/// for 1:1.
static WHEEL_ZOOM: AtomicBool = AtomicBool::new(false);

/// Blue sky gradient backdrop in the 3D viewport. Java fills black —
/// disabled by default for 1:1.
static SKYBOX: AtomicBool = AtomicBool::new(false);

/// Stretch the 765x503 game frame to fill the window. Vanilla layout
/// (default) presents it top-centred, like the Java applet page.
static STRETCHED: AtomicBool = AtomicBool::new(false);

/// Scale the frame by the largest whole factor that fits the window
/// (lossless: each game pixel becomes an exact k×k block). ON by
/// default — turning it off pins the frame to strict 1:1. Ignored
/// while "stretched" is on.
static INTEGER_SCALE: AtomicBool = AtomicBool::new(true);

/// Drop the top render level to `minusedlevel` everywhere, not just when
/// the camera→player walk crosses an "inside" tile.
static ALWAYS_HIDE_ROOFS: AtomicBool = AtomicBool::new(false);

/// Render every built tile (no 25-tile radius, no visibility-matrix
/// gating) and push the model far-cull out to match.
static EXTENDED_DRAW: AtomicBool = AtomicBool::new(false);

/// Render the 3D viewport on the GPU (offscreen FBO → readback → the same
/// SCENE_IMAGE composite) instead of the CPU software rasterizer. Optional,
/// experimental, visually close but not byte-identical. Falls back to the CPU
/// path automatically when unsupported (soft present backend / FBO failure).
/// Disabled by default for 1:1.
static GPU_SCENE: AtomicBool = AtomicBool::new(false);

pub fn middle_drag_camera() -> bool { MIDDLE_DRAG_CAMERA.load(Ordering::Relaxed) }
pub fn set_middle_drag_camera(v: bool) { MIDDLE_DRAG_CAMERA.store(v, Ordering::Relaxed); }

pub fn wheel_zoom() -> bool { WHEEL_ZOOM.load(Ordering::Relaxed) }
pub fn set_wheel_zoom(v: bool) { WHEEL_ZOOM.store(v, Ordering::Relaxed); }

pub fn skybox() -> bool { SKYBOX.load(Ordering::Relaxed) }
pub fn set_skybox(v: bool) { SKYBOX.store(v, Ordering::Relaxed); }

pub fn stretched() -> bool { STRETCHED.load(Ordering::Relaxed) }
pub fn set_stretched(v: bool) { STRETCHED.store(v, Ordering::Relaxed); }

pub fn integer_scale() -> bool { INTEGER_SCALE.load(Ordering::Relaxed) }
pub fn set_integer_scale(v: bool) { INTEGER_SCALE.store(v, Ordering::Relaxed); }

pub fn always_hide_roofs() -> bool { ALWAYS_HIDE_ROOFS.load(Ordering::Relaxed) }
pub fn set_always_hide_roofs(v: bool) { ALWAYS_HIDE_ROOFS.store(v, Ordering::Relaxed); }

pub fn extended_draw() -> bool { EXTENDED_DRAW.load(Ordering::Relaxed) }
pub fn set_extended_draw(v: bool) { EXTENDED_DRAW.store(v, Ordering::Relaxed); }

pub fn gpu_scene() -> bool { GPU_SCENE.load(Ordering::Relaxed) }
pub fn set_gpu_scene(v: bool) { GPU_SCENE.store(v, Ordering::Relaxed); }
