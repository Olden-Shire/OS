//! Port of `jagex3.dash3d.Pix3D` — software triangle rasterizer.
//!
//! Java keeps everything static; we model the per-frame mutable state as a struct that
//! borrows a `Pix2D` to draw into. The static lookup tables (sin, cos, div, HSL colour
//! palette) are built once via `OnceLock` and shared.
//!
//! **Rasterizer**: matches Java's pipeline — Y-sorted triangle with two scanline passes
//! (top→middle, middle→bottom). Java's source has 12 unrolled paths for each ordering of
//! the three vertices; we use the canonical sort-first form which produces equivalent
//! output for in-bounds triangles. `gouraudRaster` / `flatRaster` are literal ports of
//! the inner span fillers (with the lowDetail / opaque / trans variants flattened to
//! the common `lowDetail=true, trans=0` case for now).
//!
//! **HSL palette**: 65536-entry table indexed by 16-bit HSL packing
//! (top 6 bits hue, next 3 saturation, low 7 luminance). Initialised exactly as
//! `Pix3D.initColourTable` does — pre-applied gamma correction with `Math.pow(c, brightness)`.

use std::sync::OnceLock;

use crate::pix2d::Pix2D;

const TAU_OVER_2048: f64 = 0.003_067_961_5;

struct Tables {
    sin: Vec<i32>,    // 2048 entries, fixed-point .16
    cos: Vec<i32>,
    div: Vec<i32>,    // 512 entries; div[i] = 32768 / i
    div2: Vec<i32>,   // 2048 entries; div2[i] = 65536 / i
    palette: Vec<u32>, // 65536-entry HSL → RGB table at default brightness 0.8
}

fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| {
        let mut sin = vec![0i32; 2048];
        let mut cos = vec![0i32; 2048];
        for i in 0..2048usize {
            sin[i] = ((i as f64 * TAU_OVER_2048).sin() * 65536.0) as i32;
            cos[i] = ((i as f64 * TAU_OVER_2048).cos() * 65536.0) as i32;
        }
        let mut div = vec![0i32; 512];
        for i in 1..512usize {
            div[i] = 32768 / i as i32;
        }
        let mut div2 = vec![0i32; 2048];
        for i in 1..2048usize {
            div2[i] = 65536 / i as i32;
        }
        Tables { sin, cos, div, div2, palette: init_colour_table(0.8) }
    })
}

/// Sin lookup, Jagex angle units (0..2047 = full turn). Returns fixed-point .16
/// (`int(sin(a) * 65536)`).
#[must_use]
pub fn sin_table(idx: i32) -> i32 {
    tables().sin[((idx as u32) & 0x7FF) as usize]
}

#[must_use]
pub fn cos_table(idx: i32) -> i32 {
    tables().cos[((idx as u32) & 0x7FF) as usize]
}

/// Per-frame state for the rasterizer. Held alongside a `Pix2D`; methods take both.
pub struct Pix3D {
    pub size_x: i32,
    pub size_y: i32,
    pub origin_x: i32,
    pub origin_y: i32,
    pub min_x: i32,
    pub max_x: i32,
    pub min_y: i32,
    pub max_y: i32,
    /// Per-scanline starting byte offset within `pix2d.pixels`. `scanline[y]` is the index
    /// of the leftmost pixel of row (clip_min_y + y).
    pub scanline: Vec<i32>,
    pub hclip: bool,
    pub opaque: bool,
    pub trans: i32, // 0 = opaque, 256 = fully transparent
}

impl Pix3D {
    /// Wrap a `Pix2D`'s clip rect for 3D rendering. Mirrors `Pix3D.setRenderClipping` +
    /// `setClipping` — `(x0, y0, x1, y1)` are the clip BOUNDS (not size).
    pub fn new(pix2d: &Pix2D) -> Self {
        let mut me = Self {
            size_x: 0,
            size_y: 0,
            origin_x: 0,
            origin_y: 0,
            min_x: 0,
            max_x: 0,
            min_y: 0,
            max_y: 0,
            scanline: Vec::new(),
            hclip: false,
            opaque: false,
            trans: 0,
        };
        me.set_clipping(pix2d, pix2d.clip_min_x, pix2d.clip_min_y, pix2d.clip_max_x, pix2d.clip_max_y);
        me
    }

    /// Match `Pix3D.setClipping(x0, y0, x1, y1)`. The args are clip BOUNDS.
    pub fn set_clipping(&mut self, pix2d: &Pix2D, x0: i32, y0: i32, x1: i32, y1: i32) {
        self.size_x = x1 - x0;
        self.size_y = y1 - y0;
        self.reset_origin();
        if self.scanline.len() < self.size_y as usize {
            let n = (self.size_y as usize).next_power_of_two().max(64);
            self.scanline = vec![0; n];
        }
        let mut base = pix2d.width * y0 + x0;
        for s in &mut self.scanline[..self.size_y as usize] {
            *s = base;
            base += pix2d.width;
        }
    }

    /// `Pix3D.resetOrigin` — pivot at the centre of the 3D viewport.
    pub fn reset_origin(&mut self) {
        self.origin_x = self.size_x / 2;
        self.origin_y = self.size_y / 2;
        self.min_x = -self.origin_x;
        self.max_x = self.size_x - self.origin_x;
        self.min_y = -self.origin_y;
        self.max_y = self.size_y - self.origin_y;
    }

    /// `Pix3D.setOrigin(x, y)` — set pivot to specific pixel coords within the 3D viewport.
    pub fn set_origin(&mut self, pix2d: &Pix2D, x: i32, y: i32) {
        if self.scanline.is_empty() {
            return;
        }
        let row0 = self.scanline[0];
        let row_y = row0 / pix2d.width;
        let row_x = row0 - pix2d.width * row_y;
        self.origin_x = x - row_x;
        self.origin_y = y - row_y;
        self.min_x = -self.origin_x;
        self.max_x = self.size_x - self.origin_x;
        self.min_y = -self.origin_y;
        self.max_y = self.size_y - self.origin_y;
    }

    /// Look up a precomputed HSL16 colour from the static palette table.
    #[must_use]
    pub fn palette_lookup(hsl: i32) -> u32 {
        tables().palette[(hsl & 0xFFFF) as usize]
    }

    /// Filled triangle, single colour per face. Y coords are screen Y (post-projection),
    /// X coords are screen X. `rgb` is the post-palette RGB.
    ///
    /// Per-scanline edge intersection (left = min(x_ac, x_other), right = max). Handles
    /// flat-top, flat-bottom, and middle-on-either-side cases uniformly. Equivalent to
    /// Java's 12-path dispatch but without the case-by-case unroll.
    pub fn flat_triangle(
        &self,
        pix2d: &mut Pix2D,
        y0: i32, y1: i32, y2: i32,
        x0: i32, x1: i32, x2: i32,
        rgb: u32,
    ) {
        let mut tri = [(y0, x0), (y1, x1), (y2, x2)];
        tri.sort_by_key(|v| v.0);
        let (a, b, c) = (tri[0], tri[1], tri[2]);
        if c.0 < 0 || a.0 >= self.size_y || a.0 == c.0 {
            return;
        }
        let s_ab = if b.0 != a.0 { ((b.1 - a.1) << 16) / (b.0 - a.0) } else { 0 };
        let s_ac = if c.0 != a.0 { ((c.1 - a.1) << 16) / (c.0 - a.0) } else { 0 };
        let s_bc = if c.0 != b.0 { ((c.1 - b.1) << 16) / (c.0 - b.0) } else { 0 };

        // Top half: y in [a.y, b.y). Edges are AB and AC, both starting at a.x.
        // Java init: `var23 = var22 = arg3 << 16` — NO +32768 half-pixel bias. The
        // bias I had here shifts vertex projections off the pixel grid relative to
        // Java, opening 1-px gaps at apexes where adjacent triangles converge.
        let mut x_ab = a.1 << 16;
        let mut x_ac = a.1 << 16;
        let y_top_start = a.0.max(0);
        let y_top_end = b.0.min(self.size_y);
        if a.0 < 0 {
            x_ab += s_ab * (-a.0);
            x_ac += s_ac * (-a.0);
        }
        for y in y_top_start..y_top_end {
            let (lx, rx) = if x_ab < x_ac { (x_ab >> 16, x_ac >> 16) } else { (x_ac >> 16, x_ab >> 16) };
            let lx = lx.max(0);
            let rx = rx.min(self.size_x);
            if lx < rx {
                flat_raster(pix2d, self.scanline[y as usize], rgb, lx, rx, self.trans);
            }
            x_ab += s_ab;
            x_ac += s_ac;
        }

        // Bottom half: y in [b.y, c.y). Edges are BC and AC. BC starts at b.x.
        let mut x_bc = b.1 << 16;
        let y_bot_start = b.0.max(0);
        let y_bot_end = c.0.min(self.size_y);
        if b.0 < y_bot_start {
            x_bc += s_bc * (y_bot_start - b.0);
        }
        // x_ac may have advanced beyond b.y already, or may need to catch up. Recompute
        // freshly so the bottom half always starts from a clean baseline.
        let mut x_ac2 = a.1 << 16;
        if y_bot_start > a.0 {
            x_ac2 += s_ac * (y_bot_start - a.0);
        }
        for y in y_bot_start..y_bot_end {
            let (lx, rx) = if x_bc < x_ac2 { (x_bc >> 16, x_ac2 >> 16) } else { (x_ac2 >> 16, x_bc >> 16) };
            let lx = lx.max(0);
            let rx = rx.min(self.size_x);
            if lx < rx {
                flat_raster(pix2d, self.scanline[y as usize], rgb, lx, rx, self.trans);
            }
            x_bc += s_bc;
            x_ac2 += s_ac;
        }
    }

    /// Gouraud-shaded triangle. Y/X are screen-space; `hsl_a/b/c` are HSL16 indices into
    /// the colour palette. Per-scanline left/right computed from actual intersected X
    /// values so all triangle orientations (flat-top, flat-bottom, middle-left, middle-
    /// right) work without case-by-case dispatch.
    #[allow(clippy::too_many_arguments)]
    /// Per-triangle Gouraud rasterizer — 1:1 with Java's `Pix3D.gouraudTriangle`.
    ///
    /// Colour interpolation is **triangle-wide**: compute a single `dC/dX` and `dC/dY`
    /// from the screen-space determinant, then walk scanlines accumulating `dC/dY` per
    /// row and pass `dC/dX` per pixel to the raster fill. Per-edge linear interpolation
    /// (what my earlier port did) yields stripy patterns because the scanline gradient
    /// is inconsistent between top/bottom halves.
    pub fn gouraud_triangle(
        &self,
        pix2d: &mut Pix2D,
        y0: i32, y1: i32, y2: i32,
        x0: i32, x1: i32, x2: i32,
        hsl_a: i32, hsl_b: i32, hsl_c: i32,
    ) {
        let mut tri = [(y0, x0, hsl_a), (y1, x1, hsl_b), (y2, x2, hsl_c)];
        tri.sort_by_key(|v| v.0);
        let (a, b, c) = (tri[0], tri[1], tri[2]);
        if c.0 < 0 || a.0 >= self.size_y || a.0 == c.0 {
            return;
        }

        // Screen-space determinant — proportional to 2× signed triangle area. Returning
        // on zero matches Java's `if (var18 == 0) return;` for degenerate (colinear)
        // triangles.
        let area = (b.1 - a.1) * (c.0 - a.0) - (b.0 - a.0) * (c.1 - a.1);
        if area == 0 {
            return;
        }
        // `dc_dx` / `dc_dy` use Java's exact formula. The `<< 8` is performed BEFORE the
        // divide so the result lands in << 8 fixed-point — be careful with Rust's
        // precedence (`<<` is lower than `/`), hence the explicit parens here.
        let dc_dx = (((c.0 - a.0) * (b.2 - a.2) - (b.0 - a.0) * (c.2 - a.2)) << 8) / area;
        let dc_dy = (((b.1 - a.1) * (c.2 - a.2) - (c.1 - a.1) * (b.2 - a.2)) << 8) / area;

        // Edge slopes (dX/dY in << 16 fixed-point) for the three edges, same as flat.
        let s_ab_x = if b.0 != a.0 { ((b.1 - a.1) << 16) / (b.0 - a.0) } else { 0 };
        let s_ac_x = if c.0 != a.0 { ((c.1 - a.1) << 16) / (c.0 - a.0) } else { 0 };
        let s_bc_x = if c.0 != b.0 { ((c.1 - b.1) << 16) / (c.0 - b.0) } else { 0 };

        // Per-row colour accumulator — matches Java's `var21 = (arg6 << 8) - arg3 * var19
        // + var19`. The `+ dc_dx` is a +1-pixel bias (Java increments by `dc_dx` once
        // before the loop), preserved here for byte-equivalence.
        let mut row_c = (a.2 << 8) - a.1 * dc_dx + dc_dx;
        if a.0 < 0 {
            row_c -= a.0 * dc_dy;
        }

        // Top half: edges AB and AC, both starting at vertex a.
        // Java init: `var23 = var22 = arg3 << 16` — NO +32768 half-pixel bias. The
        // bias I had here shifts vertex projections off the pixel grid relative to
        // Java, opening 1-px gaps at apexes where adjacent triangles converge.
        let mut x_ab = a.1 << 16;
        let mut x_ac = a.1 << 16;
        let y_top_start = a.0.max(0);
        let y_top_end = b.0.min(self.size_y);
        if a.0 < y_top_start {
            let n = y_top_start - a.0;
            x_ab += s_ab_x * n;
            x_ac += s_ac_x * n;
        }
        for y in y_top_start..y_top_end {
            let (lx_fx, rx_fx) = if x_ab < x_ac { (x_ab, x_ac) } else { (x_ac, x_ab) };
            scan_gouraud(
                pix2d,
                self.scanline[y as usize],
                lx_fx >> 16,
                rx_fx >> 16,
                row_c,
                dc_dx,
                self.size_x,
                self.trans,
            );
            x_ab += s_ab_x;
            x_ac += s_ac_x;
            row_c += dc_dy;
        }

        // Bottom half: edges BC (starting at b) and AC (continuing from a). Recompute
        // AC from scratch so it's clean regardless of where the top half ended.
        let mut x_bc = b.1 << 16;
        let mut x_ac2 = a.1 << 16;
        let y_bot_start = b.0.max(0);
        let y_bot_end = c.0.min(self.size_y);
        if b.0 < y_bot_start {
            x_bc += s_bc_x * (y_bot_start - b.0);
        }
        if a.0 < y_bot_start {
            x_ac2 += s_ac_x * (y_bot_start - a.0);
        }
        for y in y_bot_start..y_bot_end {
            let (lx_fx, rx_fx) = if x_bc < x_ac2 { (x_bc, x_ac2) } else { (x_ac2, x_bc) };
            scan_gouraud(
                pix2d,
                self.scanline[y as usize],
                lx_fx >> 16,
                rx_fx >> 16,
                row_c,
                dc_dx,
                self.size_x,
                self.trans,
            );
            x_bc += s_bc_x;
            x_ac2 += s_ac_x;
            row_c += dc_dy;
        }
    }
}

/// `Pix3D.flatRaster` — solid horizontal span. Trans 0 = opaque, otherwise blend.
fn flat_raster(pix2d: &mut Pix2D, row_off: i32, rgb: u32, x_left: i32, x_right: i32, trans: i32) {
    let mut idx = (row_off + x_left) as usize;
    let count = x_right - x_left;
    if trans == 0 {
        for _ in 0..count {
            pix2d.pixels[idx] = rgb;
            idx += 1;
        }
    } else {
        let t = trans as u32;
        let inv = (256 - trans) as u32;
        let pre =
            (((rgb & 0xFF00FF) * inv >> 8) & 0xFF00FF) + (((rgb & 0xFF00) * inv >> 8) & 0xFF00);
        for _ in 0..count {
            let dst = pix2d.pixels[idx];
            let blend =
                (((dst & 0xFF00FF) * t >> 8) & 0xFF00FF) + (((dst & 0xFF00) * t >> 8) & 0xFF00);
            pix2d.pixels[idx] = pre + blend;
            idx += 1;
        }
    }
}

/// Bridge wrapper that clips left/right X to the viewport, then forwards to
/// `gouraud_raster`. Matches Java `gouraudRaster`'s `arg4 < 0 → arg4 = 0` /
/// `arg5 > sizeX → arg5 = sizeX` clamps. `row_c` is the per-row colour accumulator
/// (already `<< 8` with the +1 bias from the caller), `dc_dx` the per-pixel HSL step.
fn scan_gouraud(
    pix2d: &mut Pix2D,
    row_off: i32,
    mut lx: i32,
    mut rx: i32,
    row_c: i32,
    dc_dx: i32,
    size_x: i32,
    trans: i32,
) {
    if rx > size_x {
        rx = size_x;
    }
    if lx < 0 {
        lx = 0;
    }
    if lx >= rx {
        return;
    }
    gouraud_raster(pix2d, row_off, lx, rx, row_c, dc_dx, trans);
}

/// `Pix3D.gouraudRaster` — horizontal span fill with HSL interpolation.
///
/// Java: `var9 = arg4 * arg7 + arg6` → starting colour at pixel `x_left` derives from
/// `row_c + x_left * dc_dx`. Each pixel adds `dc_dx` and indexes the palette with
/// `var9 >> 8`. We follow the same per-pixel pattern; the "low detail" 4× unroll branch
/// in Java is purely a perf optimisation (same output) so we keep the simple loop.
fn gouraud_raster(
    pix2d: &mut Pix2D,
    row_off: i32,
    x_left: i32,
    x_right: i32,
    row_c: i32,
    dc_dx: i32,
    trans: i32,
) {
    let palette = &tables().palette;
    let mut idx = (row_off + x_left) as usize;
    let count = x_right - x_left;
    let mut h = x_left * dc_dx + row_c;
    if trans == 0 {
        for _ in 0..count {
            pix2d.pixels[idx] = palette[(h >> 8) as usize & 0xFFFF];
            h += dc_dx;
            idx += 1;
        }
    } else {
        let t = trans as u32;
        let inv = (256 - trans) as u32;
        for _ in 0..count {
            let src = palette[(h >> 8) as usize & 0xFFFF];
            let dst = pix2d.pixels[idx];
            let pre = (((src & 0xFF00FF) * inv >> 8) & 0xFF00FF)
                + (((src & 0xFF00) * inv >> 8) & 0xFF00);
            let blend =
                (((dst & 0xFF00FF) * t >> 8) & 0xFF00FF) + (((dst & 0xFF00) * t >> 8) & 0xFF00);
            pix2d.pixels[idx] = pre + blend;
            h += dc_dx;
            idx += 1;
        }
    }
}

/// Build the 65536-entry HSL → RGB lookup table. Literal port of
/// `Pix3D.initColourTable(brightness)`. Indexed by 16-bit packed HSL
/// (top 6 bits hue, next 3 sat, low 7 luminance). Includes gamma correction.
fn init_colour_table(brightness: f64) -> Vec<u32> {
    let mut out = vec![0u32; 65536];
    let mut i = 0usize;
    let randomness = 0.0; // We skip the `Math.random() * 0.03 - 0.015` jitter for determinism.
    let brt = brightness + randomness;
    for v7 in 0..512usize {
        let v8 = (v7 >> 3) as f64 / 64.0 + 1.0 / 128.0;
        let v10 = (v7 & 7) as f64 / 8.0 + 1.0 / 16.0;
        for v12 in 0..128usize {
            let v13 = v12 as f64 / 128.0;
            let (mut r, mut g, mut b) = (v13, v13, v13);
            if v10 != 0.0 {
                let q = if v13 < 0.5 { (v10 + 1.0) * v13 } else { v10 + v13 - v10 * v13 };
                let p = v13 * 2.0 - q;
                let mut h_off = v8 + 1.0 / 3.0;
                if h_off > 1.0 { h_off -= 1.0; }
                let mut s_off = v8;
                let mut l_off = v8 - 1.0 / 3.0;
                if l_off < 0.0 { l_off += 1.0; }
                r = hue_to_rgb(p, q, h_off);
                g = hue_to_rgb(p, q, s_off);
                b = hue_to_rgb(p, q, l_off);
            }
            let ri = (r * 256.0) as i32;
            let gi = (g * 256.0) as i32;
            let bi = (b * 256.0) as i32;
            let packed = ((ri & 0xFF) << 16) + ((gi & 0xFF) << 8) + (bi & 0xFF);
            let gamma = gamma_correct(packed as u32, brt);
            out[i] = if gamma == 0 { 1 } else { gamma };
            i += 1;
        }
    }
    out
}

fn hue_to_rgb(p: f64, q: f64, mut t: f64) -> f64 {
    if t < 0.0 { t += 1.0; }
    if t > 1.0 { t -= 1.0; }
    if t * 6.0 < 1.0 { p + (q - p) * 6.0 * t }
    else if t * 2.0 < 1.0 { q }
    else if t * 3.0 < 2.0 { p + (q - p) * (2.0 / 3.0 - t) * 6.0 }
    else { p }
}

fn gamma_correct(rgb: u32, gamma: f64) -> u32 {
    let r = (rgb >> 16) as f64 / 256.0;
    let g = ((rgb >> 8) & 0xFF) as f64 / 256.0;
    let b = (rgb & 0xFF) as f64 / 256.0;
    let r2 = (r.powf(gamma) * 256.0) as u32;
    let g2 = (g.powf(gamma) * 256.0) as u32;
    let b2 = (b.powf(gamma) * 256.0) as u32;
    ((r2 & 0xFF) << 16) + ((g2 & 0xFF) << 8) + (b2 & 0xFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_triangle_fills_pixels() {
        let mut p = Pix2D::new(32, 32);
        let r = Pix3D::new(&p);
        r.flat_triangle(&mut p, 4, 28, 28, 16, 4, 28, 0xFF_AA_55_00);
        // The interior of the triangle should have lots of non-zero pixels.
        let nonzero = p.pixels.iter().filter(|&&px| px != 0).count();
        assert!(nonzero > 100, "expected filled tri, got {nonzero} nonzero");
    }

    #[test]
    fn gouraud_triangle_fills_pixels() {
        let mut p = Pix2D::new(32, 32);
        let r = Pix3D::new(&p);
        r.gouraud_triangle(&mut p, 4, 28, 28, 16, 4, 28, 0x1000, 0x4000, 0x7FFF);
        let nonzero = p.pixels.iter().filter(|&&px| px != 0).count();
        assert!(nonzero > 100, "expected filled tri, got {nonzero} nonzero");
    }

    /// Flat-top: a and b share the same Y, c is below. Previously broken because slope-
    /// based "which edge is left" picked the wrong edge when AB had slope 0.
    #[test]
    fn flat_top_triangle_fills_pixels() {
        let mut p = Pix2D::new(32, 32);
        let r = Pix3D::new(&p);
        r.flat_triangle(&mut p, 4, 4, 28, 4, 24, 14, 0xFF_AA_55_00);
        let nonzero = p.pixels.iter().filter(|&&px| px != 0).count();
        assert!(nonzero > 100, "flat-top tri should fill many pixels, got {nonzero}");
    }

    /// Flat-bottom: b and c share the same Y. Similar slope-degeneracy case.
    #[test]
    fn flat_bottom_triangle_fills_pixels() {
        let mut p = Pix2D::new(32, 32);
        let r = Pix3D::new(&p);
        r.flat_triangle(&mut p, 4, 28, 28, 14, 4, 24, 0xFF_AA_55_00);
        let nonzero = p.pixels.iter().filter(|&&px| px != 0).count();
        assert!(nonzero > 100, "flat-bottom tri should fill many pixels, got {nonzero}");
    }

    /// Middle vertex on the LEFT side (b.x < a.x and b.x < c.x). Edge tracking has to
    /// detect this per-scanline rather than via initial slope sign.
    #[test]
    fn middle_left_triangle_fills_pixels() {
        let mut p = Pix2D::new(32, 32);
        let r = Pix3D::new(&p);
        // a at top-right (28, 4), b at middle-left (4, 16), c at bottom-right (28, 28)
        r.flat_triangle(&mut p, 4, 16, 28, 28, 4, 28, 0xFF_55_AA_00);
        let nonzero = p.pixels.iter().filter(|&&px| px != 0).count();
        assert!(nonzero > 100, "middle-left tri should fill many pixels, got {nonzero}");
    }
}
