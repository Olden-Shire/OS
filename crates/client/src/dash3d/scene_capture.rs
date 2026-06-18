// custom — optional GPU scene path: per-frame geometry capture.
//
// When the `gpu_scene` debug toggle is on AND a GL Present backend has lent its
// scene renderer for the frame, the software rasterizer's three geometry
// sources (ModelLit::render_faces, World::render_quick_ground,
// World::render_ground) divert each face they would have drawn into the
// CAPTURE buffer instead of writing pixels. The faces arrive in the EXACT order
// the CPU would have painted them (octant/fill order, then per-model
// priority+depth sort), so replaying them on the GPU with depth-test OFF and
// blending ON reproduces the painter's algorithm 1:1.
//
// Vertices are captured in VIEW space (post yaw+pitch rotation, pre
// perspective-divide) — the same `view_x/view_y/view_z` the CPU transform
// already produced. The GPU vertex shader does only the divide + NDC mapping,
// so screen positions match the CPU to the pixel, and the clip-space near plane
// (mapped to view_z = 50) clips partially-behind faces for free.
//
// CPU-path fidelity: when the toggle is off, `CAPTURING` is false and none of
// the `emit_*` calls below are reached — the existing rasterizer runs verbatim.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// Floats per captured vertex:
///   0..3  pos.xyz       — view space
///   3     c             — gouraud: HSL palette index; textured: light/brightness [2,126]
///   4..6  uv            — texture coords (s along M-P, t along N-P); 0 if untextured
///   6     tex           — texture id, or -1 for a plain gouraud/flat face
///   7     alpha         — straight-alpha factor (0=transparent .. 1=opaque)
pub const VERTEX_FLOATS: usize = 8;

/// Armed only while the world render runs under the GPU toggle. Read on the hot
/// rasterizer path, so a plain relaxed atomic.
pub static CAPTURING: AtomicBool = AtomicBool::new(false);

/// One frame's flattened scene geometry, consumed by the GL backend.
#[derive(Default)]
pub struct SceneFrame {
    /// Interleaved vertices, `VERTEX_FLOATS` each.
    pub verts: Vec<f32>,
    /// Triangle list, in painter's draw order.
    pub indices: Vec<u32>,
    /// Projection params captured from the scene (focal length + screen origin).
    pub zoom: i32,
    pub origin_x: i32,
    pub origin_y: i32,
    /// Viewport size in pixels.
    pub w: i32,
    pub h: i32,
}

pub static CAPTURE: Mutex<SceneFrame> = Mutex::new(SceneFrame {
    verts: Vec::new(),
    indices: Vec::new(),
    zoom: 512,
    origin_x: 0,
    origin_y: 0,
    w: 0,
    h: 0,
});

/// Start capturing for a frame: clear the buffer and record the projection.
pub fn begin(zoom: i32, origin_x: i32, origin_y: i32, w: i32, h: i32) {
    let mut f = CAPTURE.lock().unwrap();
    f.verts.clear();
    f.indices.clear();
    f.zoom = zoom;
    f.origin_x = origin_x;
    f.origin_y = origin_y;
    f.w = w;
    f.h = h;
    CAPTURING.store(true, Ordering::Relaxed);
}

/// Stop capturing. The CAPTURE buffer holds this frame's geometry until the GL
/// backend reads it.
pub fn end() {
    CAPTURING.store(false, Ordering::Relaxed);
}

#[inline]
pub fn capturing() -> bool {
    CAPTURING.load(Ordering::Relaxed)
}

/// Convert a Pix3D `trans` (0 = opaque .. 255 = nearly invisible) to a GL
/// straight-alpha factor for SRC_ALPHA/ONE_MINUS_SRC_ALPHA blending.
#[inline]
pub fn trans_to_alpha(trans: i32) -> f32 {
    ((256 - trans.clamp(0, 255)) as f32) / 256.0
}

/// Emit one gouraud/flat triangle in view space. `hsl_*` are raw HSL palette
/// indices (0..65535), one per vertex (flat faces pass the same index thrice).
/// `alpha` is the straight-alpha factor (see `trans_to_alpha`).
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn emit_tri(
    ax: i32, ay: i32, az: i32, ahsl: i32,
    bx: i32, by: i32, bz: i32, bhsl: i32,
    cx: i32, cy: i32, cz: i32, chsl: i32,
    alpha: f32,
) {
    let mut f = CAPTURE.lock().unwrap();
    let base = (f.verts.len() / VERTEX_FLOATS) as u32;
    push_vertex(&mut f.verts, ax, ay, az, (ahsl & 0xFFFF) as f32, 0.0, 0.0, -1.0, alpha);
    push_vertex(&mut f.verts, bx, by, bz, (bhsl & 0xFFFF) as f32, 0.0, 0.0, -1.0, alpha);
    push_vertex(&mut f.verts, cx, cy, cz, (chsl & 0xFFFF) as f32, 0.0, 0.0, -1.0, alpha);
    f.indices.push(base);
    f.indices.push(base + 1);
    f.indices.push(base + 2);
}

/// Emit one TEXTURED triangle. `bright_*` are the per-vertex light scalars
/// (`tex_light`, 2..126 — the GPU multiplies the texel by bright/128, matching
/// the CPU's `texel * (light*2) >> 8`). `(u,v)` are planar texture coords in
/// [0,1] (P→0,0; M→1,0; N→0,1). `tex` is the texture id.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn emit_tri_tex(
    ax: i32, ay: i32, az: i32, abright: i32, au: f32, av: f32,
    bx: i32, by: i32, bz: i32, bbright: i32, bu: f32, bv: f32,
    cx: i32, cy: i32, cz: i32, cbright: i32, cu: f32, cv: f32,
    tex: i32, alpha: f32,
) {
    let t = tex as f32;
    let mut f = CAPTURE.lock().unwrap();
    let base = (f.verts.len() / VERTEX_FLOATS) as u32;
    push_vertex(&mut f.verts, ax, ay, az, abright as f32, au, av, t, alpha);
    push_vertex(&mut f.verts, bx, by, bz, bbright as f32, bu, bv, t, alpha);
    push_vertex(&mut f.verts, cx, cy, cz, cbright as f32, cu, cv, t, alpha);
    f.indices.push(base);
    f.indices.push(base + 1);
    f.indices.push(base + 2);
}

#[allow(clippy::too_many_arguments)]
#[inline]
fn push_vertex(verts: &mut Vec<f32>, x: i32, y: i32, z: i32, c: f32, u: f32, v: f32, tex: f32, alpha: f32) {
    verts.push(x as f32);
    verts.push(y as f32);
    verts.push(z as f32);
    verts.push(c);
    verts.push(u);
    verts.push(v);
    verts.push(tex);
    verts.push(alpha);
}

/// Texture coordinates of a face vertex `Q` against the (P, M, N) anchor, all in
/// VIEW space. This is an exact replication of Jagex's per-pixel texture-plane
/// math (`Pix3D.textureTriangle`): it forms the same plane coefficients, then
/// evaluates `u = Unum/Wden`, `v = Vnum/Wden` at the vertex's screen position
/// (`xrel = Qx*zoom/Qz`, `yrel = Qy*zoom/Qz`). Returns GL coords in texture
/// units (`u/16384`, `v/16384`) — texel = coord*128. Because the texture is a
/// true planar projection, perspective-correct GPU interpolation (w=view_z) of
/// these reproduces the CPU per-pixel result. i64 throughout (the `<<14` plane
/// terms overflow i32). Degenerate/behind-eye cases return (0,0).
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn jagex_uv(
    px: i32, py: i32, pz: i32,
    mx: i32, my: i32, mz: i32,
    nx: i32, ny: i32, nz: i32,
    qx: i32, qy: i32, qz: i32,
    zoom: i32,
) -> (f32, f32) {
    if qz < 1 {
        return (0.0, 0.0);
    }
    let (px, py, pz) = (px as i64, py as i64, pz as i64);
    let (mx, my, mz) = (mx as i64, my as i64, mz as i64);
    let (nx, ny, nz) = (nx as i64, ny as i64, nz as i64);
    let xrel = (qx as i64 * zoom as i64) / qz as i64;
    let yrel = (qy as i64 * zoom as i64) / qz as i64;
    // P-M and N-P edges (Pix3D var33..var38).
    let (e1x, e1y, e1z) = (px - mx, py - my, pz - mz);
    let (e2x, e2y, e2z) = (nx - px, ny - py, nz - pz);
    let unum0 = (py * e2x - px * e2y) << 14;
    let unum_x = (pz * e2y - py * e2z) << 8;
    let unum_y = (px * e2z - pz * e2x) << 5;
    let vnum0 = (py * e1x - px * e1y) << 14;
    let vnum_x = (pz * e1y - py * e1z) << 8;
    let vnum_y = (px * e1z - pz * e1x) << 5;
    let wden0 = (e1y * e2x - e1x * e2y) << 14;
    let wden_x = (e1z * e2y - e1y * e2z) << 8;
    let wden_y = (e1x * e2z - e1z * e2x) << 5;
    let unum = (unum_x >> 3) * xrel + unum_y * yrel + unum0;
    let vnum = (vnum_x >> 3) * xrel + vnum_y * yrel + vnum0;
    let wden = (wden_x >> 3) * xrel + wden_y * yrel + wden0;
    let den = wden >> 14;
    if den == 0 {
        return (0.0, 0.0);
    }
    let u = unum as f64 / den as f64;
    let v = vnum as f64 / den as f64;
    ((u / 16384.0) as f32, (v / 16384.0) as f32)
}

