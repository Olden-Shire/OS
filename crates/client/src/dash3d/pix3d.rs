// @ObfuscatedName("fx") — jag::oldscape::dash3d::Pix3D extends Pix2D
//
// Software triangle rasterizer. Maintains its own origin / clip on top
// of Pix2D's pixel buffer. The big lookup tables (sin, cos, divTable,
// divTable2) are computed once at first use.
//
// Coverage so far: tables + setOrigin/resetOrigin/setClipping/
// setRenderClipping + uniform triangle fill + gouraud triangle. Texture
// mapping + lit-model rendering land in follow-up turns.

#![allow(dead_code)]

use std::sync::{Mutex, OnceLock};

use crate::graphics::pix2d;

// ── Static lookup tables ─────────────────────────────────────────────

// @ObfuscatedName("fx.an") — Pix3D.sinTable
pub fn sin_table() -> &'static [i32; 2048] {
    static T: OnceLock<[i32; 2048]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0i32; 2048];
        for i in 0..2048 {
            t[i] = ((i as f64 * 0.003_067_961_5).sin() * 65536.0) as i32;
        }
        t
    })
}

// @ObfuscatedName("fx.ah") — Pix3D.cosTable
pub fn cos_table() -> &'static [i32; 2048] {
    static T: OnceLock<[i32; 2048]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0i32; 2048];
        for i in 0..2048 {
            t[i] = ((i as f64 * 0.003_067_961_5).cos() * 65536.0) as i32;
        }
        t
    })
}

// @ObfuscatedName("fx.ak") — Pix3D.divTable (32768 / i, i in 1..512)
pub fn div_table() -> &'static [i32; 512] {
    static T: OnceLock<[i32; 512]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0i32; 512];
        for i in 1..512 {
            t[i] = 32768 / i as i32;
        }
        t
    })
}

// @ObfuscatedName("fx.ap") — Pix3D.colourTable. 65536-entry HSL→RGB
// palette built by initColourTable with HSL packing:
//   index = (hue << 10) | (saturation << 7) | lightness
// Hue is 6 bits, saturation 3 bits, lightness 7 bits. Java applies a
// gamma curve via gammaCorrect() — default brightness 0.8.
pub fn colour_table() -> &'static [i32; 65536] {
    static T: OnceLock<[i32; 65536]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0i32; 65536];
        let brightness = 0.8;
        for hl in 0..512 {
            let hue_part = (hl >> 3) as f64 / 64.0 + 0.0078125;
            let sat_part = (hl & 0x7) as f64 / 8.0 + 0.0625;
            for li in 0..128 {
                let light = li as f64 / 128.0;
                let (mut r, mut g, mut b) = (light, light, light);
                if sat_part != 0.0 {
                    let q = if light < 0.5 {
                        (sat_part + 1.0) * light
                    } else {
                        sat_part + light - sat_part * light
                    };
                    let p = light * 2.0 - q;
                    let mut tr = hue_part + 1.0 / 3.0;
                    if tr > 1.0 { tr -= 1.0; }
                    let mut tb = hue_part - 1.0 / 3.0;
                    if tb < 0.0 { tb += 1.0; }
                    let hue_to_rgb = |t: f64| -> f64 {
                        if t * 6.0 < 1.0 { (q - p) * 6.0 * t + p }
                        else if t * 2.0 < 1.0 { q }
                        else if t * 3.0 < 2.0 { (q - p) * (2.0 / 3.0 - t) * 6.0 + p }
                        else { p }
                    };
                    r = hue_to_rgb(tr);
                    g = hue_to_rgb(hue_part);
                    b = hue_to_rgb(tb);
                }
                let ri = (r * 256.0) as i32;
                let gi = (g * 256.0) as i32;
                let bi = (b * 256.0) as i32;
                let rgb = (ri << 16) | (gi << 8) | bi;
                let mut corrected = gamma_correct(rgb, brightness);
                if corrected == 0 { corrected = 1; }
                t[hl * 128 + li] = corrected;
            }
        }
        t
    })
}

// @ObfuscatedName("fx.bm(ID)I") — Pix3D.gammaCorrect. Verbatim port
// of Pix3D.java:239-250. Applies pow(channel, gamma) to each 8-bit
// RGB band then repacks. Used by initColourTable + texture loading.
#[inline]
pub fn gamma_correct(rgb: i32, gamma: f64) -> i32 {
    let r = ((rgb >> 16) & 0xFF) as f64 / 256.0;
    let g = ((rgb >> 8) & 0xFF) as f64 / 256.0;
    let b = (rgb & 0xFF) as f64 / 256.0;
    let ri = (r.powf(gamma) * 256.0) as i32;
    let gi = (g.powf(gamma) * 256.0) as i32;
    let bi = (b.powf(gamma) * 256.0) as i32;
    (ri << 16) | (gi << 8) | bi
}

// @ObfuscatedName("fx.az") — Pix3D.divTable2 (65536 / i, i in 1..2048)
pub fn div_table2() -> &'static [i32; 2048] {
    static T: OnceLock<[i32; 2048]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0i32; 2048];
        for i in 1..2048 {
            t[i] = 65536 / i as i32;
        }
        t
    })
}

// ── Mutable state ────────────────────────────────────────────────────

pub struct State {
    // @ObfuscatedName("fx.u")
    pub hclip: bool,
    // @ObfuscatedName("fx.v")
    pub opaque: bool,
    // @ObfuscatedName("fx.w")
    pub low_mem: bool,
    // @ObfuscatedName("fx.e")
    pub low_detail: bool,
    // @ObfuscatedName("fx.b")
    pub trans: i32,
    // @ObfuscatedName("fx.a")
    pub origin_x: i32,
    // @ObfuscatedName("fx.h")
    pub origin_y: i32,
    // @ObfuscatedName("fx.x")
    pub size_x: i32,
    // @ObfuscatedName("fx.p")
    pub size_y: i32,
    // @ObfuscatedName("fx.ad")
    pub min_x: i32,
    // @ObfuscatedName("fx.ac")
    pub max_x: i32,
    // @ObfuscatedName("fx.aa")
    pub min_y: i32,
    // @ObfuscatedName("fx.as")
    pub max_y: i32,
    // @ObfuscatedName("fx.am") — re-grown to next power of two when
    // needed by setClipping.
    pub scanline: Vec<i32>,
    // @ObfuscatedName("fx.aw") — active TextureManager handle. Java
    // stores an object reference; we store an i32 slot id since the
    // TextureManager lives in the JS5 loader registry.
    pub texture_manager_slot: i32,
    // custom — model far-cull plane. Java hardcodes 3500 in
    // ModelLit.worldRender (line 927), tuned for its fixed-zoom orbit
    // camera. Our camera adds a mouse-wheel zoom on top of Java's
    // `pitch * 3 + 600` distance, so the scene renderer pushes this out
    // by the extra pull-back each frame — models on a rendered tile
    // stay rendered at any zoom. Default matches Java exactly.
    pub model_far_clip: i32,
}

impl State {
    fn new() -> Self {
        Self {
            hclip: false,
            opaque: false,
            low_mem: false,
            low_detail: true,
            trans: 0,
            origin_x: 0, origin_y: 0,
            size_x: 0, size_y: 0,
            min_x: 0, max_x: 0,
            min_y: 0, max_y: 0,
            scanline: vec![0i32; 1024],
            texture_manager_slot: -1,
            model_far_clip: 3500,
        }
    }
}

// custom — per-frame far-cull update from the scene camera. `extra` is
// the camera pull-back beyond Java's fixed-zoom baseline (0 at default
// zoom → Java-identical 3500).
pub fn set_model_far_clip(extra: i32) {
    STATE.lock().unwrap().model_far_clip = 3500 + extra.max(0);
}

pub static STATE: std::sync::LazyLock<Mutex<State>> =
    std::sync::LazyLock::new(|| Mutex::new(State::new()));

// @ObfuscatedName("fx.u") write — Pix3D.hclip mutator. ModelLit sets
// this per-face from faceClippedX[f] right before each rasterizer
// call so the inner raster knows whether to do the per-scanline X
// clamp. Callers that paint scene triangles (loc models, ground tiles)
// must do the same: compute hclip from the triangle's vertex Xs vs
// the clip rect, then call set_hclip(...) before the rasterizer.
pub fn set_hclip(v: bool) {
    STATE.lock().unwrap().hclip = v;
}

// @ObfuscatedName("fx.r(Lfu;I)V") — Pix3D.setTextures. Verbatim port
// of Pix3D.java:157-159. Stores the active TextureManager that the
// textured-triangle rasterizers look up textures from. Java uses a
// static field; we mirror with a Mutex-protected slot.
pub fn set_textures(slot: i32) {
    STATE.lock().unwrap().texture_manager_slot = slot;
}

pub fn get_textures() -> i32 {
    STATE.lock().unwrap().texture_manager_slot
}

// @ObfuscatedName("fx.bd(III)V") — Pix3D.setHClip (Java 3-arg form).
// Verbatim port of Pix3D.java:254-256. Java's signature takes three
// vertex X coords (the screen-space X of every triangle vertex) and
// sets hclip true iff any of them is outside the [0, sizeX] band.
pub fn set_hclip_xyz(x1: i32, x2: i32, x3: i32) {
    let mut s = STATE.lock().unwrap();
    let size_x = s.size_x;
    s.hclip = x1 < 0 || x1 > size_x
        || x2 < 0 || x2 > size_x
        || x3 < 0 || x3 > size_x;
}

// @ObfuscatedName("fx.b") write — Pix3D.trans mutator.
// Per-face alpha set from `faceAlpha[face] & 0xFF` before each
// rasterizer call: 0 = opaque, 1..253 = blend (higher = more
// transparent), 254 = "shift-right" special case, 255 = fully
// invisible. Java's flatTriangle / gouraudTriangle / textureTriangle
// row writers branch on this.
pub fn set_trans(v: i32) {
    STATE.lock().unwrap().trans = v;
}

// Per-face X-clip predicate, ported from ModelLit lines 1116-1120:
// `xA >= 0 && ... && xC <= sizeX`. In our absolute coord system the
// thresholds are min_x / max_x instead of 0 / sizeX. Returns true if
// any vertex is outside the viewport on the X axis (i.e., hclip
// should be set true for this face).
pub fn face_x_clipped(xa: i32, xb: i32, xc: i32) -> bool {
    let (mn, mx) = {
        let s = STATE.lock().unwrap();
        (s.min_x, s.max_x)
    };
    !(xa >= mn && xb >= mn && xc >= mn && xa <= mx && xb <= mx && xc <= mx)
}

// @ObfuscatedName("fx.bm()V") — Pix3D.setRenderClipping
pub fn set_render_clipping() {
    let p = pix2d::STATE.lock().unwrap();
    let (mn_x, mn_y, mx_x, mx_y) = (p.clip_min_x, p.clip_min_y, p.clip_max_x, p.clip_max_y);
    drop(p);
    set_clipping(mn_x, mn_y, mx_x, mx_y);
}

// @ObfuscatedName("fx.bn(IIII)V") — Pix3D.setClipping
//
// (x, y) is the clip's top-left, (w, h) is the bottom-right — same as
// Pix2D.setClipping. min_x/max_x/min_y/max_y are stored in absolute
// screen coords so fill_row can clip directly without offset math.
pub fn set_clipping(x: i32, y: i32, w: i32, h: i32) {
    let mut s = STATE.lock().unwrap();
    s.size_x = w - x;
    s.size_y = h - y;
    s.min_x = x;
    s.max_x = w;
    s.min_y = y;
    s.max_y = h;
    s.origin_x = x + s.size_x / 2;
    s.origin_y = y + s.size_y / 2;
    if (s.scanline.len() as i32) < s.size_y {
        let mut n = (s.size_y - 1) as u32;
        n |= n >> 1; n |= n >> 2; n |= n >> 4; n |= n >> 8; n |= n >> 16;
        let len = (n + 1) as usize;
        s.scanline = vec![0i32; len];
    }
}

fn reset_origin_locked(s: &mut State) {
    s.origin_x = s.min_x + s.size_x / 2;
    s.origin_y = s.min_y + s.size_y / 2;
}

// @ObfuscatedName("fx.gj(IIIIIIIII)V") — Pix3D-style perspective project.
//
// Given a world-space (x, y, z) point relative to the camera and a
// camera (yaw, pitch) plus zoom, returns (screen_x, screen_y, view_z).
// view_z < 1 means the point is behind the near plane.
pub fn project(
    wx: i32, wy: i32, wz: i32,
    pitch: i32, yaw: i32,
    zoom: i32,
) -> (i32, i32, i32) {
    let (ox, oy) = origin();
    project_with_origin(wx, wy, wz, pitch, yaw, zoom, ox, oy)
}

// Cached-origin variant for tight inner loops — call origin() once and
// reuse the (ox, oy) pair across many vertices.
pub fn origin() -> (i32, i32) {
    let s = STATE.lock().unwrap();
    (s.origin_x, s.origin_y)
}

pub fn project_with_origin(
    wx: i32, wy: i32, wz: i32,
    pitch: i32, yaw: i32,
    zoom: i32, ox: i32, oy: i32,
) -> (i32, i32, i32) {
    let (sx, sy, _, _, vz) = project_with_view_space(wx, wy, wz, pitch, yaw, zoom, ox, oy);
    (sx, sy, vz)
}

// Mirrors Java's ModelLit.objRender vertex transform — returns the
// screen-space (sx, sy) AND the view-space (vx, vy, vz) that
// textureTriangleAffine needs as its P/M/N anchor coords. View-space
// here means post-yaw, post-pitch, pre-scale-by-zoom — the same
// values Java stashes in vertexViewSpaceX/Y/Z.
pub fn project_with_view_space(
    mut wx: i32, mut wy: i32, mut wz: i32,
    pitch: i32, yaw: i32,
    zoom: i32, ox: i32, oy: i32,
) -> (i32, i32, i32, i32, i32) {
    let sin_y = sin_table()[(yaw & 0x7FF) as usize];
    let cos_y = cos_table()[(yaw & 0x7FF) as usize];
    let sin_p = sin_table()[(pitch & 0x7FF) as usize];
    let cos_p = cos_table()[(pitch & 0x7FF) as usize];
    let tmp = (wz * sin_y + wx * cos_y) >> 16;
    wz = (wz * cos_y - wx * sin_y) >> 16;
    wx = tmp;
    let view_x = wx;
    let tmp = (wy * cos_p - wz * sin_p) >> 16;
    wz = (wy * sin_p + wz * cos_p) >> 16;
    wy = tmp;
    if wz < 1 { return (i32::MIN, i32::MIN, view_x, wy, wz); }
    let sx = (wx * zoom) / wz + ox;
    let sy = (wy * zoom) / wz + oy;
    (sx, sy, view_x, wy, wz)
}

// @ObfuscatedName("fx.aj(II)V") — Pix3D.setOrigin
pub fn set_origin(x: i32, y: i32) {
    let mut s = STATE.lock().unwrap();
    s.origin_x = x;
    s.origin_y = y;
}

// @ObfuscatedName("fx.au()V") — Pix3D.resetOrigin
pub fn reset_origin() {
    let mut s = STATE.lock().unwrap();
    reset_origin_locked(&mut s);
}

// ── Triangle rasterizers ─────────────────────────────────────────────
//
// Java's Pix3D uses divTable / divTable2 to avoid floating-point
// divides in the inner loops. We port the integer formulations
// directly so output is bit-identical (modulo the int/u32 distinction).

// @ObfuscatedName("fx.fillTriangle") — flat-shaded triangle. Sorts
// vertices by Y, walks scanlines, calls fill_row.
pub fn fill_triangle(
    mut x0: i32, mut y0: i32,
    mut x1: i32, mut y1: i32,
    mut x2: i32, mut y2: i32,
    colour: i32,
) {
    // Reorder vertices by Y so that y0 <= y1 <= y2.
    if y0 > y1 { std::mem::swap(&mut x0, &mut x1); std::mem::swap(&mut y0, &mut y1); }
    if y0 > y2 { std::mem::swap(&mut x0, &mut x2); std::mem::swap(&mut y0, &mut y2); }
    if y1 > y2 { std::mem::swap(&mut x1, &mut x2); std::mem::swap(&mut y1, &mut y2); }
    let (min_x, max_x, min_y, max_y, trans) = {
        let s = STATE.lock().unwrap();
        (s.min_x, s.max_x, s.min_y, s.max_y, s.trans)
    };

    let dy01 = (y1 - y0).max(1);
    let dy02 = (y2 - y0).max(1);
    let dy12 = (y2 - y1).max(1);
    let step_01 = ((x1 - x0) << 16) / dy01;
    let step_02 = ((x2 - x0) << 16) / dy02;
    let step_12 = ((x2 - x1) << 16) / dy12;
    let mut left = (x0 << 16) + (1 << 15);
    let mut right = left;

    // Hold the pixel buffer lock across the whole triangle so we
    // amortise the mutex cost over every scanline.
    let mut pix = pix2d::STATE.lock().unwrap();
    let w = pix.width;
    for y in y0..y1 {
        if y >= min_y && y < max_y {
            fill_row_locked(&mut pix.pixels, w, left >> 16, right >> 16, y, min_x, max_x, colour, trans);
        }
        left = left.wrapping_add(step_01);
        right = right.wrapping_add(step_02);
    }
    let mut left = (x1 << 16) + (1 << 15);
    let mut right_at_y1 = (x0 << 16) + step_02.wrapping_mul(y1 - y0) + (1 << 15);
    for y in y1..y2 {
        if y >= min_y && y < max_y {
            fill_row_locked(&mut pix.pixels, w, left >> 16, right_at_y1 >> 16, y, min_x, max_x, colour, trans);
        }
        left = left.wrapping_add(step_12);
        right_at_y1 = right_at_y1.wrapping_add(step_02);
    }
}

// Java's flat row writer with trans branching (Pix3D.java:1226-1287).
// trans == 0: direct write. trans == 255: skip entirely. Otherwise
// alpha-blend dest×trans + source×(256-trans).
fn fill_row_locked(
    pixels: &mut [i32], w: i32,
    mut lx: i32, mut rx: i32, y: i32,
    min_x: i32, max_x: i32, colour: i32,
    trans: i32,
) {
    if lx > rx { std::mem::swap(&mut lx, &mut rx); }
    if lx < min_x { lx = min_x; }
    if rx > max_x { rx = max_x; }
    if rx <= lx { return; }
    // Java's flatTriangle has no trans-255 short-circuit; trans = 255
    // is a legitimate 1/256 blend (dest * 255/256 + src * 1/256), not
    // an "invisible" marker.
    let row_base = (y * w) as usize;
    let end = (row_base + rx as usize).min(pixels.len());
    let start = (row_base + lx as usize).min(end);
    if trans == 0 {
        for p in &mut pixels[start..end] {
            *p = colour;
        }
    } else if trans == 254 {
        // Java's "frosted glass" shift (Pix3D.java:1244-1261) —
        // `arg0[var6++] = arg0[var6]` copies each pixel from its right
        // neighbour, producing a one-pixel left-shift "blur". Used by
        // face_alpha == -2 markers to render translucent transparent
        // edges (e.g. on partially-fading objects).
        let row_len = pixels.len();
        for x in start..end {
            let next = if x + 1 < row_len { pixels[x + 1] } else { pixels[x] };
            pixels[x] = next;
        }
    } else {
        // Premultiplied source contribution: (colour * (256-trans)) >> 8
        // packed into RB and G channels separately so a single multiply
        // works without channel bleed.
        let dest_w = trans;
        let src_w = 256 - trans;
        let src_pre = (((colour & 0xFF00FF) * src_w >> 8) & 0xFF00FF)
                    + (((colour & 0xFF00) * src_w >> 8) & 0xFF00);
        for p in &mut pixels[start..end] {
            let d = *p;
            *p = (((d & 0xFF00) * dest_w >> 8) & 0xFF00)
               + (((d & 0xFF00FF) * dest_w >> 8) & 0xFF00FF)
               + src_pre;
        }
    }
}

// @ObfuscatedName("fx.gouraudTriangle") — gradient-shaded triangle.
// Each vertex carries its own colour; the rasterizer linearly
// interpolates RGB across edges and along scanlines.
pub fn gouraud_triangle(
    mut x0: i32, mut y0: i32, mut c0: i32,
    mut x1: i32, mut y1: i32, mut c1: i32,
    mut x2: i32, mut y2: i32, mut c2: i32,
) {
    // Java Pix3D.gouraudTriangle (line 259+) interpolates HSL palette
    // INDICES along edges and across pixels in <<8 fixed-point, then
    // looks up colourTable[idx >> 8] per pixel. Java does NOT short-
    // circuit on trans == 255 — that's a legitimate 1/256 blend, not
    // a "fully invisible" marker.
    //
    // Our previous port interpolated in plain integer per pixel
    // (lc + (rc-lc)*t/span) which is much coarser than Java's <<8
    // accumulator stepping by a fractional per-pixel delta — visible
    // as chunky bands on smooth gradients.
    if y0 > y1 { std::mem::swap(&mut x0, &mut x1); std::mem::swap(&mut y0, &mut y1); std::mem::swap(&mut c0, &mut c1); }
    if y0 > y2 { std::mem::swap(&mut x0, &mut x2); std::mem::swap(&mut y0, &mut y2); std::mem::swap(&mut c0, &mut c2); }
    if y1 > y2 { std::mem::swap(&mut x1, &mut x2); std::mem::swap(&mut y1, &mut y2); std::mem::swap(&mut c1, &mut c2); }
    let (min_x, max_x, min_y, max_y, trans) = {
        let s = STATE.lock().unwrap();
        (s.min_x, s.max_x, s.min_y, s.max_y, s.trans)
    };
    let mut pix = pix2d::STATE.lock().unwrap();
    let w = pix.width;
    let palette = colour_table();
    // Edge interpolation. Coordinate lerp stays in i32 (max delta is
    // screen height squared, well within range). Colour lerp must use
    // i64 — `(b - a) * (y - ya)` with HSL palette indices pre-shifted
    // `<<8` (delta up to ±16.7M) and tall triangles (y-delta up to a
    // few hundred) overflows i32 and wraps the colour band.
    let lerp_x = |a: i32, b: i32, ya: i32, yb: i32, y: i32| -> i32 {
        if yb == ya { a } else { a + (b - a) * (y - ya) / (yb - ya) }
    };
    let lerp_c = |a: i32, b: i32, ya: i32, yb: i32, y: i32| -> i32 {
        if yb == ya { a } else {
            (a as i64 + (b as i64 - a as i64) * (y - ya) as i64 / (yb - ya) as i64) as i32
        }
    };
    for y in y0..=y2 {
        if y < min_y || y >= max_y { continue; }
        let (lx, rx, lc, rc) = if y < y1 {
            (lerp_x(x0, x2, y0, y2, y),
             lerp_x(x0, x1, y0, y1, y),
             lerp_c(c0 << 8, c2 << 8, y0, y2, y),
             lerp_c(c0 << 8, c1 << 8, y0, y1, y))
        } else {
            (lerp_x(x0, x2, y0, y2, y),
             lerp_x(x1, x2, y1, y2, y),
             lerp_c(c0 << 8, c2 << 8, y0, y2, y),
             lerp_c(c1 << 8, c2 << 8, y1, y2, y))
        };
        gouraud_row_hsl(&mut pix.pixels, w, lx, rx, lc, rc, y, min_x, max_x, trans, palette);
    }
}

fn gouraud_row_hsl(
    pixels: &mut [i32], w: i32,
    mut lx: i32, mut rx: i32, mut lc: i32, mut rc: i32,
    y: i32, min_x: i32, max_x: i32,
    trans: i32,
    palette: &[i32; 65536],
) {
    if lx > rx { std::mem::swap(&mut lx, &mut rx); std::mem::swap(&mut lc, &mut rc); }
    let orig_lx = lx;
    let orig_rx = rx;
    if lx < min_x { lx = min_x; }
    if rx > max_x { rx = max_x; }
    if rx <= lx { return; }
    let row_base = (y * w) as usize;
    // Span MUST be the unclipped width — using `rx` after the max_x clip
    // gives a steeper colour gradient for right-clipped triangles (the
    // span shrinks but the colour delta doesn't), causing colour banding
    // at the right viewport edge.
    let unclipped_span = (orig_rx - orig_lx).max(1) as i64;
    // Per-pixel <<8 colour step, computed in i64 — lc/rc are in <<8
    // space (range ±16.7M), and step * (lx - orig_lx) on a tall + wide
    // clipped triangle overflows i32 (`step * 768` with step ≈ 16M
    // wraps).
    let lc64 = lc as i64;
    let step: i64 = (rc as i64 - lc64) / unclipped_span;
    let mut acc: i64 = lc64 + step * (lx - orig_lx) as i64;
    let dest_w = trans;
    let src_w = 256 - trans;
    for x in lx..rx {
        let idx = ((acc >> 8) & 0xFFFF) as usize;
        let src = palette[idx];
        let pix_idx = row_base + x as usize;
        if let Some(p) = pixels.get_mut(pix_idx) {
            if trans == 0 {
                *p = src;
            } else {
                let d = *p;
                let src_pre = (((src & 0xFF00FF) * src_w >> 8) & 0xFF00FF)
                            + (((src & 0xFF00) * src_w >> 8) & 0xFF00);
                *p = (((d & 0xFF00) * dest_w >> 8) & 0xFF00)
                   + (((d & 0xFF00FF) * dest_w >> 8) & 0xFF00FF)
                   + src_pre;
            }
        }
        acc += step;
    }
}

fn interp_edge(
    ax: i32, ay: i32, ac: i32, bx: i32, by: i32, bc: i32, y: i32,
    cx: i32, cy: i32, cc: i32, dx: i32, dy: i32, dc: i32, y2: i32,
) -> (i32, i32, i32, i32) {
    fn lerp(ax: i32, ay: i32, bx: i32, by: i32, y: i32) -> i32 {
        if by == ay { ax } else { ax + (bx - ax) * (y - ay) / (by - ay) }
    }
    fn lerp_col(ac: i32, ay: i32, bc: i32, by: i32, y: i32) -> i32 {
        if by == ay { ac } else {
            let ar = (ac >> 16) & 0xFF; let ag = (ac >> 8) & 0xFF; let ab = ac & 0xFF;
            let br = (bc >> 16) & 0xFF; let bg = (bc >> 8) & 0xFF; let bb = bc & 0xFF;
            let r = ar + (br - ar) * (y - ay) / (by - ay);
            let g = ag + (bg - ag) * (y - ay) / (by - ay);
            let b = ab + (bb - ab) * (y - ay) / (by - ay);
            (r << 16) | (g << 8) | b
        }
    }
    let lx = lerp(ax, ay, bx, by, y);
    let rx = lerp(cx, cy, dx, dy, y2);
    let lc = lerp_col(ac, ay, bc, by, y);
    let rc = lerp_col(cc, cy, dc, dy, y2);
    (lx, rx, lc, rc)
}

// Old per-row helpers replaced by *_locked variants that take a pixel
// slice + width directly. Triangle rasterizers now hold the pix2d
// mutex across all of their scanlines for ~10× lower lock pressure.

// @ObfuscatedName("fx.cd(II)I") — Pix3D.textureLightColour.
// Java's helper applied to averageRgb in the texels==null fallback.
// Pure: HSL16-packed colour in arg0 + intensity in arg1, returns
// recombined HSL16 with the lightness scaled and clamped to [2, 126].
pub fn texture_light_colour(arg0: i32, arg1: i32) -> i32 {
    let mut var2 = ((arg0 & 0x7F) * arg1) >> 7;
    if var2 < 2 { var2 = 2; } else if var2 > 126 { var2 = 126; }
    (arg0 & 0xFF80) + var2
}

// @ObfuscatedName(— Pix3D.setClipping scanline sizer). Pure bit-twiddle
// that rounds (n-1) up to the next power of two. Extracted from
// Pix3D.java:111-120 where it sizes the scanline lookup table.
// Returns 1 for n <= 1.
pub fn next_pow2_ceil(n: i32) -> i32 {
    if n <= 1 { return 1; }
    let mut v = (n - 1) as u32;
    v |= v >> 1;
    v |= v >> 2;
    v |= v >> 4;
    v |= v >> 8;
    v |= v >> 16;
    (v + 1) as i32
}

// @ObfuscatedName("fx.co(IIIIIIIIIIIIIIIIIII)V") — Pix3D.textureTriangleAffine.
//
// Verbatim Rust port of the Java triangle walker. Args use the same
// names as the Java code (arg0..arg18). The texture mapping is
// parameterised by three model-space points (P, M, N) — `face_texture_p/m/n`
// vertex indices — projected to view space; per-pixel U/V is derived
// from the cross products of the (P-M, P-N) basis with the X axis.
// Note: this is the affine variant — only the non-affine
// `textureTriangle` does the per-8-pixel perspective divide. Java's
// ModelLit.draw + render3ZClip both call this affine version for
// every textured face, so we don't currently port the non-affine one.
pub fn texture_triangle_affine(
    mut arg0: i32, mut arg1: i32, mut arg2: i32,                       // y0, y1, y2
    arg3: i32, arg4: i32, arg5: i32,                                   // x0, x1, x2
    arg6: i32, arg7: i32, arg8: i32,                                   // light0, light1, light2
    arg9: i32, arg10: i32, arg11: i32,                                 // Px, Mx, Nx
    arg12: i32, arg13: i32, arg14: i32,                                // Py, My, Ny
    arg15: i32, arg16: i32, arg17: i32,                                // Pz, Mz, Nz
    arg18: i32,                                                        // textureId
) {
    let texels_opt = crate::dash3d::texture_manager::get_texels(arg18);
    // Java sets Pix3D.opaque/low_mem from the texture metadata before
    // rastering — mirror that here.
    let opaque_flag = crate::dash3d::texture_manager::is_opaque(arg18);
    let low_mem_flag = false; // We don't yet honour the textures-low-mem flag.
    // Java's Pix3D uses viewport-relative coords (0..sizeY for Y, X
    // centred around 0). Our project() output is absolute screen
    // coords — the rasterizer's clip checks below substitute min_y for
    // Java's literal `0` and min_x for the X clamp in the raster.
    let (state_max_y, state_min_y, state_min_x, origin_x, origin_y, hclip_flag, size_x) = {
        let s = STATE.lock().unwrap();
        (s.max_y, s.min_y, s.min_x, s.origin_x, s.origin_y, s.hclip, s.max_x)
    };
    let state_size_y = state_max_y;
    let zero_y = state_min_y; // Java's literal `0` becomes our viewport-top.
    let texels = match texels_opt {
        Some(t) => t,
        None => {
            // Java's fallback: gouraud-shade with the averageRgb tint
            // and per-vertex light intensities via textureLightColour.
            let avg = crate::dash3d::texture_manager::get_average_rgb(arg18);
            let palette = colour_table();
            let ca = palette[(texture_light_colour(avg, arg6) as usize) & 0xFFFF];
            let cb = palette[(texture_light_colour(avg, arg7) as usize) & 0xFFFF];
            let cc = palette[(texture_light_colour(avg, arg8) as usize) & 0xFFFF];
            gouraud_triangle(
                arg3, arg0, ca,
                arg4, arg1, cb,
                arg5, arg2, cc,
            );
            return;
        }
    };
    let opaque = opaque_flag;
    let low_mem = low_mem_flag;
    let hclip = hclip_flag;

    let var21 = arg4 - arg3;
    let var22 = arg1 - arg0;
    let var23 = arg5 - arg3;
    let var24 = arg2 - arg0;
    let var25 = arg7 - arg6;
    let var26 = arg8 - arg6;
    let mut var27 = 0;
    if arg0 != arg1 { var27 = ((arg4 - arg3) << 16) / (arg1 - arg0); }
    let mut var28 = 0;
    if arg1 != arg2 { var28 = ((arg5 - arg4) << 16) / (arg2 - arg1); }
    let mut var29 = 0;
    if arg0 != arg2 { var29 = ((arg3 - arg5) << 16) / (arg0 - arg2); }
    let var30 = var21 * var24 - var22 * var23;
    if var30 == 0 { return; }
    let var31 = ((var24 * var25 - var22 * var26) << 9) / var30;
    let var32 = ((var21 * var26 - var23 * var25) << 9) / var30;
    let var33 = arg9 - arg10;
    let var34 = arg12 - arg13;
    let var35 = arg15 - arg16;
    let var36 = arg11 - arg9;
    let var37 = arg14 - arg12;
    let var38 = arg17 - arg15;
    let var39 = (arg12 * var36 - arg9 * var37) << 14;
    let var40 = (arg15 * var37 - arg12 * var38) << 5;
    let var41 = (arg9 * var38 - arg15 * var36) << 5;
    let var42 = (arg12 * var33 - arg9 * var34) << 14;
    let var43 = (arg15 * var34 - arg12 * var35) << 5;
    let var44 = (arg9 * var35 - arg15 * var33) << 5;
    let var45 = (var34 * var36 - var33 * var37) << 14;
    let var46 = (var35 * var37 - var34 * var38) << 5;
    let var47 = (var33 * var38 - var35 * var36) << 5;
    let mut pix = pix2d::STATE.lock().unwrap();
    let width = pix.width;
    let pixels = &mut pix.pixels;
    let scanline = |y: i32| -> i32 { y * width };

    let ctx = RasterCtx {
        opaque, low_mem, hclip,
        size_x, min_x: state_min_x, origin_x, origin_y, width,
    };

    if arg0 <= arg1 && arg0 <= arg2 {
        if arg0 >= state_size_y { return; }
        if arg1 > state_size_y { arg1 = state_size_y; }
        if arg2 > state_size_y { arg2 = state_size_y; }
        let mut var48 = (arg6 << 9) - arg3 * var31 + var31;
        if arg1 < arg2 {
            let mut var49;
            let mut var50;
            var50 = arg3 << 16; var49 = arg3 << 16;
            if arg0 < zero_y {
                var50 -= (arg0 - zero_y) * var29;
                var49 -= (arg0 - zero_y) * var27;
                var48 -= (arg0 - zero_y) * var32;
                arg0 = zero_y;
            }
            let mut var51 = arg4 << 16;
            if arg1 < zero_y {
                var51 -= (arg1 - zero_y) * var28;
                arg1 = zero_y;
            }
            let var52 = arg0 - origin_y;
            let mut var53 = var41 * var52 + var39;
            let mut var54 = var44 * var52 + var42;
            let mut var55 = var47 * var52 + var45;
            if (arg0 != arg1 && var29 < var27) || (arg0 == arg1 && var29 > var28) {
                let mut var56 = arg2 - arg1;
                let mut var57 = arg1 - arg0;
                let mut var58 = scanline(arg0);
                loop {
                    var57 -= 1;
                    if var57 < 0 {
                        loop {
                            var56 -= 1;
                            if var56 < 0 { return; }
                            texture_raster_affine(pixels, &texels, var58, var50 >> 16, var51 >> 16, var48, var31, var53, var54, var55, var40, var43, var46, &ctx);
                            var50 += var29; var51 += var28; var48 += var32;
                            var58 += width;
                            var53 += var41; var54 += var44; var55 += var47;
                        }
                    }
                    texture_raster_affine(pixels, &texels, var58, var50 >> 16, var49 >> 16, var48, var31, var53, var54, var55, var40, var43, var46, &ctx);
                    var50 += var29; var49 += var27; var48 += var32;
                    var58 += width;
                    var53 += var41; var54 += var44; var55 += var47;
                }
            } else {
                let mut var59 = arg2 - arg1;
                let mut var60 = arg1 - arg0;
                let mut var61 = scanline(arg0);
                loop {
                    var60 -= 1;
                    if var60 < 0 {
                        loop {
                            var59 -= 1;
                            if var59 < 0 { return; }
                            texture_raster_affine(pixels, &texels, var61, var51 >> 16, var50 >> 16, var48, var31, var53, var54, var55, var40, var43, var46, &ctx);
                            var50 += var29; var51 += var28; var48 += var32;
                            var61 += width;
                            var53 += var41; var54 += var44; var55 += var47;
                        }
                    }
                    texture_raster_affine(pixels, &texels, var61, var49 >> 16, var50 >> 16, var48, var31, var53, var54, var55, var40, var43, var46, &ctx);
                    var50 += var29; var49 += var27; var48 += var32;
                    var61 += width;
                    var53 += var41; var54 += var44; var55 += var47;
                }
            }
        } else {
            let mut var62;
            let mut var63;
            var63 = arg3 << 16; var62 = arg3 << 16;
            if arg0 < zero_y {
                var63 -= (arg0 - zero_y) * var29;
                var62 -= (arg0 - zero_y) * var27;
                var48 -= (arg0 - zero_y) * var32;
                arg0 = zero_y;
            }
            let mut var64 = arg5 << 16;
            if arg2 < zero_y {
                var64 -= (arg2 - zero_y) * var28;
                arg2 = zero_y;
            }
            let var65 = arg0 - origin_y;
            let mut var66 = var41 * var65 + var39;
            let mut var67 = var44 * var65 + var42;
            let mut var68 = var47 * var65 + var45;
            if (arg0 == arg2 || var29 >= var27) && (arg0 != arg2 || var28 <= var27) {
                let mut var72 = arg1 - arg2;
                let mut var73 = arg2 - arg0;
                let mut var74 = scanline(arg0);
                loop {
                    var73 -= 1;
                    if var73 < 0 {
                        loop {
                            var72 -= 1;
                            if var72 < 0 { return; }
                            texture_raster_affine(pixels, &texels, var74, var62 >> 16, var64 >> 16, var48, var31, var66, var67, var68, var40, var43, var46, &ctx);
                            var64 += var28; var62 += var27; var48 += var32;
                            var74 += width;
                            var66 += var41; var67 += var44; var68 += var47;
                        }
                    }
                    texture_raster_affine(pixels, &texels, var74, var62 >> 16, var63 >> 16, var48, var31, var66, var67, var68, var40, var43, var46, &ctx);
                    var63 += var29; var62 += var27; var48 += var32;
                    var74 += width;
                    var66 += var41; var67 += var44; var68 += var47;
                }
            } else {
                let mut var69 = arg1 - arg2;
                let mut var70 = arg2 - arg0;
                let mut var71 = scanline(arg0);
                loop {
                    var70 -= 1;
                    if var70 < 0 {
                        loop {
                            var69 -= 1;
                            if var69 < 0 { return; }
                            texture_raster_affine(pixels, &texels, var71, var64 >> 16, var62 >> 16, var48, var31, var66, var67, var68, var40, var43, var46, &ctx);
                            var64 += var28; var62 += var27; var48 += var32;
                            var71 += width;
                            var66 += var41; var67 += var44; var68 += var47;
                        }
                    }
                    texture_raster_affine(pixels, &texels, var71, var63 >> 16, var62 >> 16, var48, var31, var66, var67, var68, var40, var43, var46, &ctx);
                    var63 += var29; var62 += var27; var48 += var32;
                    var71 += width;
                    var66 += var41; var67 += var44; var68 += var47;
                }
            }
        }
    } else if arg1 <= arg2 {
        if arg1 >= state_size_y { return; }
        if arg2 > state_size_y { arg2 = state_size_y; }
        if arg0 > state_size_y { arg0 = state_size_y; }
        let mut var75 = (arg7 << 9) - arg4 * var31 + var31;
        if arg2 < arg0 {
            let mut var76;
            let mut var77;
            var77 = arg4 << 16; var76 = arg4 << 16;
            if arg1 < zero_y {
                var77 -= (arg1 - zero_y) * var27;
                var76 -= (arg1 - zero_y) * var28;
                var75 -= (arg1 - zero_y) * var32;
                arg1 = zero_y;
            }
            let mut var78 = arg5 << 16;
            if arg2 < zero_y {
                var78 -= (arg2 - zero_y) * var29;
                arg2 = zero_y;
            }
            let var79 = arg1 - origin_y;
            let mut var80 = var41 * var79 + var39;
            let mut var81 = var44 * var79 + var42;
            let mut var82 = var47 * var79 + var45;
            if (arg1 != arg2 && var27 < var28) || (arg1 == arg2 && var27 > var29) {
                let mut var83 = arg0 - arg2;
                let mut var84 = arg2 - arg1;
                let mut var85 = scanline(arg1);
                loop {
                    var84 -= 1;
                    if var84 < 0 {
                        loop {
                            var83 -= 1;
                            if var83 < 0 { return; }
                            texture_raster_affine(pixels, &texels, var85, var77 >> 16, var78 >> 16, var75, var31, var80, var81, var82, var40, var43, var46, &ctx);
                            var77 += var27; var78 += var29; var75 += var32;
                            var85 += width;
                            var80 += var41; var81 += var44; var82 += var47;
                        }
                    }
                    texture_raster_affine(pixels, &texels, var85, var77 >> 16, var76 >> 16, var75, var31, var80, var81, var82, var40, var43, var46, &ctx);
                    var77 += var27; var76 += var28; var75 += var32;
                    var85 += width;
                    var80 += var41; var81 += var44; var82 += var47;
                }
            } else {
                let mut var86 = arg0 - arg2;
                let mut var87 = arg2 - arg1;
                let mut var88 = scanline(arg1);
                loop {
                    var87 -= 1;
                    if var87 < 0 {
                        loop {
                            var86 -= 1;
                            if var86 < 0 { return; }
                            texture_raster_affine(pixels, &texels, var88, var78 >> 16, var77 >> 16, var75, var31, var80, var81, var82, var40, var43, var46, &ctx);
                            var77 += var27; var78 += var29; var75 += var32;
                            var88 += width;
                            var80 += var41; var81 += var44; var82 += var47;
                        }
                    }
                    texture_raster_affine(pixels, &texels, var88, var76 >> 16, var77 >> 16, var75, var31, var80, var81, var82, var40, var43, var46, &ctx);
                    var77 += var27; var76 += var28; var75 += var32;
                    var88 += width;
                    var80 += var41; var81 += var44; var82 += var47;
                }
            }
        } else {
            let mut var89;
            let mut var90;
            var90 = arg4 << 16; var89 = arg4 << 16;
            if arg1 < zero_y {
                var90 -= (arg1 - zero_y) * var27;
                var89 -= (arg1 - zero_y) * var28;
                var75 -= (arg1 - zero_y) * var32;
                arg1 = zero_y;
            }
            let mut var91 = arg3 << 16;
            if arg0 < zero_y {
                var91 -= (arg0 - zero_y) * var29;
                arg0 = zero_y;
            }
            let var92 = arg1 - origin_y;
            let mut var93 = var41 * var92 + var39;
            let mut var94 = var44 * var92 + var42;
            let mut var95 = var47 * var92 + var45;
            if var27 < var28 {
                let mut var96 = arg2 - arg0;
                let mut var97 = arg0 - arg1;
                let mut var98 = scanline(arg1);
                loop {
                    var97 -= 1;
                    if var97 < 0 {
                        loop {
                            var96 -= 1;
                            if var96 < 0 { return; }
                            texture_raster_affine(pixels, &texels, var98, var91 >> 16, var89 >> 16, var75, var31, var93, var94, var95, var40, var43, var46, &ctx);
                            var91 += var29; var89 += var28; var75 += var32;
                            var98 += width;
                            var93 += var41; var94 += var44; var95 += var47;
                        }
                    }
                    texture_raster_affine(pixels, &texels, var98, var90 >> 16, var89 >> 16, var75, var31, var93, var94, var95, var40, var43, var46, &ctx);
                    var90 += var27; var89 += var28; var75 += var32;
                    var98 += width;
                    var93 += var41; var94 += var44; var95 += var47;
                }
            } else {
                let mut var99 = arg2 - arg0;
                let mut var100 = arg0 - arg1;
                let mut var101 = scanline(arg1);
                loop {
                    var100 -= 1;
                    if var100 < 0 {
                        loop {
                            var99 -= 1;
                            if var99 < 0 { return; }
                            texture_raster_affine(pixels, &texels, var101, var89 >> 16, var91 >> 16, var75, var31, var93, var94, var95, var40, var43, var46, &ctx);
                            var91 += var29; var89 += var28; var75 += var32;
                            var101 += width;
                            var93 += var41; var94 += var44; var95 += var47;
                        }
                    }
                    texture_raster_affine(pixels, &texels, var101, var89 >> 16, var90 >> 16, var75, var31, var93, var94, var95, var40, var43, var46, &ctx);
                    var90 += var27; var89 += var28; var75 += var32;
                    var101 += width;
                    var93 += var41; var94 += var44; var95 += var47;
                }
            }
        }
    } else if arg2 < state_size_y {
        if arg0 > state_size_y { arg0 = state_size_y; }
        if arg1 > state_size_y { arg1 = state_size_y; }
        let mut var102 = (arg8 << 9) - arg5 * var31 + var31;
        if arg0 < arg1 {
            let mut var103;
            let mut var104;
            var104 = arg5 << 16; var103 = arg5 << 16;
            if arg2 < zero_y {
                var104 -= (arg2 - zero_y) * var28;
                var103 -= (arg2 - zero_y) * var29;
                var102 -= (arg2 - zero_y) * var32;
                arg2 = zero_y;
            }
            let mut var105 = arg3 << 16;
            if arg0 < zero_y {
                var105 -= (arg0 - zero_y) * var27;
                arg0 = zero_y;
            }
            let var106 = arg2 - origin_y;
            let mut var107 = var41 * var106 + var39;
            let mut var108 = var44 * var106 + var42;
            let mut var109 = var47 * var106 + var45;
            if var28 < var29 {
                let mut var110 = arg1 - arg0;
                let mut var111 = arg0 - arg2;
                let mut var112 = scanline(arg2);
                loop {
                    var111 -= 1;
                    if var111 < 0 {
                        loop {
                            var110 -= 1;
                            if var110 < 0 { return; }
                            texture_raster_affine(pixels, &texels, var112, var104 >> 16, var105 >> 16, var102, var31, var107, var108, var109, var40, var43, var46, &ctx);
                            var104 += var28; var105 += var27; var102 += var32;
                            var112 += width;
                            var107 += var41; var108 += var44; var109 += var47;
                        }
                    }
                    texture_raster_affine(pixels, &texels, var112, var104 >> 16, var103 >> 16, var102, var31, var107, var108, var109, var40, var43, var46, &ctx);
                    var104 += var28; var103 += var29; var102 += var32;
                    var112 += width;
                    var107 += var41; var108 += var44; var109 += var47;
                }
            } else {
                let mut var113 = arg1 - arg0;
                let mut var114 = arg0 - arg2;
                let mut var115 = scanline(arg2);
                loop {
                    var114 -= 1;
                    if var114 < 0 {
                        loop {
                            var113 -= 1;
                            if var113 < 0 { return; }
                            texture_raster_affine(pixels, &texels, var115, var105 >> 16, var104 >> 16, var102, var31, var107, var108, var109, var40, var43, var46, &ctx);
                            var104 += var28; var105 += var27; var102 += var32;
                            var115 += width;
                            var107 += var41; var108 += var44; var109 += var47;
                        }
                    }
                    texture_raster_affine(pixels, &texels, var115, var103 >> 16, var104 >> 16, var102, var31, var107, var108, var109, var40, var43, var46, &ctx);
                    var104 += var28; var103 += var29; var102 += var32;
                    var115 += width;
                    var107 += var41; var108 += var44; var109 += var47;
                }
            }
        } else {
            let mut var116;
            let mut var117;
            var117 = arg5 << 16; var116 = arg5 << 16;
            if arg2 < zero_y {
                var117 -= (arg2 - zero_y) * var28;
                var116 -= (arg2 - zero_y) * var29;
                var102 -= (arg2 - zero_y) * var32;
                arg2 = zero_y;
            }
            let mut var118 = arg4 << 16;
            if arg1 < zero_y {
                var118 -= (arg1 - zero_y) * var27;
                arg1 = zero_y;
            }
            let var119 = arg2 - origin_y;
            let mut var120 = var41 * var119 + var39;
            let mut var121 = var44 * var119 + var42;
            let mut var122 = var47 * var119 + var45;
            if var28 < var29 {
                let mut var123 = arg0 - arg1;
                let mut var124 = arg1 - arg2;
                let mut var125 = scanline(arg2);
                loop {
                    var124 -= 1;
                    if var124 < 0 {
                        loop {
                            var123 -= 1;
                            if var123 < 0 { return; }
                            texture_raster_affine(pixels, &texels, var125, var118 >> 16, var116 >> 16, var102, var31, var120, var121, var122, var40, var43, var46, &ctx);
                            var118 += var27; var116 += var29; var102 += var32;
                            var125 += width;
                            var120 += var41; var121 += var44; var122 += var47;
                        }
                    }
                    texture_raster_affine(pixels, &texels, var125, var117 >> 16, var116 >> 16, var102, var31, var120, var121, var122, var40, var43, var46, &ctx);
                    var117 += var28; var116 += var29; var102 += var32;
                    var125 += width;
                    var120 += var41; var121 += var44; var122 += var47;
                }
            } else {
                let mut var126 = arg0 - arg1;
                let mut var127 = arg1 - arg2;
                let mut var128 = scanline(arg2);
                loop {
                    var127 -= 1;
                    if var127 < 0 {
                        loop {
                            var126 -= 1;
                            if var126 < 0 { return; }
                            texture_raster_affine(pixels, &texels, var128, var116 >> 16, var118 >> 16, var102, var31, var120, var121, var122, var40, var43, var46, &ctx);
                            var118 += var27; var116 += var29; var102 += var32;
                            var128 += width;
                            var120 += var41; var121 += var44; var122 += var47;
                        }
                    }
                    texture_raster_affine(pixels, &texels, var128, var116 >> 16, var117 >> 16, var102, var31, var120, var121, var122, var40, var43, var46, &ctx);
                    var117 += var28; var116 += var29; var102 += var32;
                    var128 += width;
                    var120 += var41; var121 += var44; var122 += var47;
                }
            }
        }
    }
}

struct RasterCtx {
    opaque: bool,
    low_mem: bool,
    hclip: bool,
    size_x: i32,
    min_x: i32,
    origin_x: i32,
    origin_y: i32,
    width: i32,
}

// @ObfuscatedName("fx.ch") — Pix3D.textureRasterAffine.
//
// Inner per-scanline texel sampler. arg5 = clipped left X, arg6 =
// right X. arg7 = light value scaled (>> 8 to get 0..255 multiplier).
// arg8 = light step per pixel. arg9/10/11 = U/V/Z numerators at row
// start. arg12/13/14 = U/V/Z step in X per pixel.
fn texture_raster_affine(
    pixels: &mut [i32],
    texels: &[i32],
    arg4: i32, mut arg5: i32, mut arg6: i32,
    arg7: i32, arg8: i32,
    arg9: i32, arg10: i32, arg11: i32,
    arg12: i32, arg13: i32, arg14: i32,
    ctx: &RasterCtx,
) {
    let _ = ctx.origin_y;
    // Java line 2840-2846: clamp X to viewport only when the per-face
    // hclip flag is set (i.e., at least one vertex's X is outside the
    // viewport bounds). Callers set Pix3D.hclip = faceClippedX[face]
    // via `set_hclip()` before each rasterizer call.
    if ctx.hclip {
        if arg6 > ctx.size_x { arg6 = ctx.size_x; }
        if arg5 < ctx.min_x { arg5 = ctx.min_x; }
    }
    if arg5 >= arg6 { return; }
    let mut var15 = arg4 + arg5;
    let mut var16 = arg5 * arg8 + arg7;
    let var17 = arg6 - arg5;

    if !ctx.low_mem {
        let var70 = arg5 - ctx.origin_x;
        let var71 = arg12 * var70 + arg9;
        let var72 = arg13 * var70 + arg10;
        let var73 = arg14 * var70 + arg11;
        let var74 = var73 >> 14;
        let (var75, var76) = if var74 == 0 { (0, 0) } else { (var71 / var74, var72 / var74) };
        let var77 = arg12 * var17 + var71;
        let var78 = arg13 * var17 + var72;
        let var79 = arg14 * var17 + var73;
        let var80 = var79 >> 14;
        let (var81, var82) = if var80 == 0 { (0, 0) } else { (var77 / var80, var78 / var80) };
        let mut var83 = (var75 << 18) + var76;
        let var84 = if var17 == 0 { 0 } else { (((var81 - var75) / var17) << 18) + (var82 - var76) / var17 };
        let mut var85 = var17 >> 3;
        let var86 = arg8 << 3;
        let mut var87 = var16 >> 8;
        if ctx.opaque {
            if var85 > 0 {
                loop {
                    for _ in 0..8 {
                        let idx = ((var83 as u32 >> 25) as i32 + (var83 & 0x3F80)) as usize;
                        if let (Some(&tex), Some(p)) = (texels.get(idx), pixels.get_mut(var15 as usize)) {
                            *p = (((tex & 0xFF00FF) * var87) & 0xFF00FF00u32 as i32) + (((tex & 0xFF00) * var87) & 0xFF0000) >> 8;
                        }
                        var15 += 1;
                        var83 = var83.wrapping_add(var84);
                    }
                    var16 += var86;
                    var87 = var16 >> 8;
                    var85 -= 1;
                    if var85 <= 0 { break; }
                }
            }
            let mut var103 = (arg6 - arg5) & 0x7;
            while var103 > 0 {
                let idx = ((var83 as u32 >> 25) as i32 + (var83 & 0x3F80)) as usize;
                if let (Some(&tex), Some(p)) = (texels.get(idx), pixels.get_mut(var15 as usize)) {
                    *p = (((tex & 0xFF00FF) * var87) & 0xFF00FF00u32 as i32) + (((tex & 0xFF00) * var87) & 0xFF0000) >> 8;
                }
                var15 += 1;
                var83 = var83.wrapping_add(var84);
                var103 -= 1;
            }
        } else {
            if var85 > 0 {
                loop {
                    for _ in 0..8 {
                        let idx = ((var83 as u32 >> 25) as i32 + (var83 & 0x3F80)) as usize;
                        if let Some(&tex) = texels.get(idx) {
                            if tex != 0 {
                                if let Some(p) = pixels.get_mut(var15 as usize) {
                                    *p = (((tex & 0xFF00FF) * var87) & 0xFF00FF00u32 as i32) + (((tex & 0xFF00) * var87) & 0xFF0000) >> 8;
                                }
                            }
                        }
                        var15 += 1;
                        var83 = var83.wrapping_add(var84);
                    }
                    var16 += var86;
                    var87 = var16 >> 8;
                    var85 -= 1;
                    if var85 <= 0 { break; }
                }
            }
            let mut var120 = (arg6 - arg5) & 0x7;
            while var120 > 0 {
                let idx = ((var83 as u32 >> 25) as i32 + (var83 & 0x3F80)) as usize;
                if let Some(&tex) = texels.get(idx) {
                    if tex != 0 {
                        if let Some(p) = pixels.get_mut(var15 as usize) {
                            *p = (((tex & 0xFF00FF) * var87) & 0xFF00FF00u32 as i32) + (((tex & 0xFF00) * var87) & 0xFF0000) >> 8;
                        }
                    }
                }
                var15 += 1;
                var83 = var83.wrapping_add(var84);
                var120 -= 1;
            }
        }
        return;
    }

    let var18 = arg5 - ctx.origin_x;
    let var19 = arg12 * var18 + arg9;
    let var20 = arg13 * var18 + arg10;
    let var21_ = arg14 * var18 + arg11;
    let var22 = var21_ >> 12;
    let (var23, var24) = if var22 == 0 { (0, 0) } else { (var19 / var22, var20 / var22) };
    let var25 = arg12 * var17 + var19;
    let var26 = arg13 * var17 + var20;
    let var27 = arg14 * var17 + var21_;
    let var28 = var27 >> 12;
    let (var29, var30) = if var28 == 0 { (0, 0) } else { (var25 / var28, var26 / var28) };
    let mut var31 = (var23 << 20) + var24;
    let var32 = if var17 == 0 { 0 } else { (((var29 - var23) / var17) << 20) + (var30 - var24) / var17 };
    let mut var33 = var17 >> 3;
    let var34 = arg8 << 3;
    let mut var35 = var16 >> 8;

    if ctx.opaque {
        if var33 > 0 {
            loop {
                for _ in 0..8 {
                    let idx = ((var31 as u32 >> 26) as i32 + (var31 & 0xFC0)) as usize;
                    if let (Some(&tex), Some(p)) = (texels.get(idx), pixels.get_mut(var15 as usize)) {
                        *p = (((tex & 0xFF00FF) * var35) & 0xFF00FF00u32 as i32) + (((tex & 0xFF00) * var35) & 0xFF0000) >> 8;
                    }
                    var15 += 1;
                    var31 = var31.wrapping_add(var32);
                }
                var16 += var34;
                var35 = var16 >> 8;
                var33 -= 1;
                if var33 <= 0 { break; }
            }
        }
        let mut var51 = (arg6 - arg5) & 0x7;
        while var51 > 0 {
            let idx = ((var31 as u32 >> 26) as i32 + (var31 & 0xFC0)) as usize;
            if let (Some(&tex), Some(p)) = (texels.get(idx), pixels.get_mut(var15 as usize)) {
                *p = (((tex & 0xFF00FF) * var35) & 0xFF00FF00u32 as i32) + (((tex & 0xFF00) * var35) & 0xFF0000) >> 8;
            }
            var15 += 1;
            var31 = var31.wrapping_add(var32);
            var51 -= 1;
        }
        return;
    }
    if var33 > 0 {
        loop {
            for _ in 0..8 {
                let idx = ((var31 as u32 >> 26) as i32 + (var31 & 0xFC0)) as usize;
                if let Some(&tex) = texels.get(idx) {
                    if tex != 0 {
                        if let Some(p) = pixels.get_mut(var15 as usize) {
                            *p = (((tex & 0xFF00FF) * var35) & 0xFF00FF00u32 as i32) + (((tex & 0xFF00) * var35) & 0xFF0000) >> 8;
                        }
                    }
                }
                var15 += 1;
                var31 = var31.wrapping_add(var32);
            }
            var16 += var34;
            var35 = var16 >> 8;
            var33 -= 1;
            if var33 <= 0 { break; }
        }
    }
    let mut var68 = (arg6 - arg5) & 0x7;
    while var68 > 0 {
        let idx = ((var31 as u32 >> 26) as i32 + (var31 & 0xFC0)) as usize;
        if let Some(&tex) = texels.get(idx) {
            if tex != 0 {
                if let Some(p) = pixels.get_mut(var15 as usize) {
                    *p = (((tex & 0xFF00FF) * var35) & 0xFF00FF00u32 as i32) + (((tex & 0xFF00) * var35) & 0xFF0000) >> 8;
                }
            }
        }
        var15 += 1;
        var31 = var31.wrapping_add(var32);
        var68 -= 1;
    }
}
