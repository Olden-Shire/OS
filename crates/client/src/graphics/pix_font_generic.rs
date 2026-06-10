// @ObfuscatedName("fm")
// jagex3.graphics.PixFontGeneric
//
// Concrete font subclass providing scanline plot routines. The glyph
// bytes are 1 byte per pixel, packed row-major (width * height). A zero
// byte is "transparent" (advance the dest pointer); any non-zero byte
// is "plot the colour" (opaque) or "alpha-blend the colour" (trans).
//
// Verbatim port of PixFontGeneric.java including the manual 4-wide
// loop unroll in plot — the unroll is part of the Java emitted
// behaviour we diff against future-revision gamepacks.

#![allow(dead_code)]

use super::pix_font::PixFont;
use super::pix2d::STATE;

#[derive(Clone)]
pub struct PixFontGeneric {
    pub base: PixFont,
}

impl PixFontGeneric {
    pub fn from_metrics(src: &[u8]) -> Self {
        let mut base = PixFont::new();
        base.unpack_metrics(src);
        Self { base }
    }

    // @ObfuscatedName("fm.cz([BIIIII)V") — PixFontGeneric.plotLetterScanline.
    // Verbatim port of PixFontGeneric.java:17-50.
    //
    // glyph: raw byte buffer for this glyph (one byte per pixel)
    // x, y: top-left destination in Pix2D
    // w, h: glyph width / height
    // rgb : opaque ARGB to plot for any non-zero glyph byte
    pub fn plot_letter_scanline(
        &self, glyph: &[u8], mut x: i32, mut y: i32, mut w: i32, mut h: i32, rgb: i32,
    ) {
        let mut s = STATE.lock().unwrap();
        let surface_w = s.width;
        let clip_min_x = s.clip_min_x;
        let clip_max_x = s.clip_max_x;
        let clip_min_y = s.clip_min_y;
        let clip_max_y = s.clip_max_y;

        let mut dst = surface_w * y + x;       // var7
        let mut dst_step = surface_w - w;      // var8 — extra dst stride per row
        let mut src_step = 0i32;               // var9 — extra src stride per row
        let mut src = 0i32;                    // var10 — current glyph offset

        if y < clip_min_y {
            let dy = clip_min_y - y;
            h -= dy;
            y = clip_min_y;
            src += w * dy;
            dst += surface_w * dy;
        }
        if y + h > clip_max_y {
            h -= y + h - clip_max_y;
        }
        if x < clip_min_x {
            let dx = clip_min_x - x;
            w -= dx;
            x = clip_min_x;
            src += dx;
            dst += dx;
            src_step += dx;
            dst_step += dx;
        }
        if x + w > clip_max_x {
            let dx = x + w - clip_max_x;
            w -= dx;
            src_step += dx;
            dst_step += dx;
        }
        if w > 0 && h > 0 {
            plot(&mut s.pixels, glyph, rgb, src, dst, w, h, dst_step, src_step);
        }
    }

    // @ObfuscatedName("fm.cv([BIIIIII)V") — PixFontGeneric.plotLetterTransScanline.
    // Verbatim port of PixFontGeneric.java:53-86. `alpha` is 0..256:
    // 256 = fully opaque, 0 = fully transparent.
    pub fn plot_letter_trans_scanline(
        &self, glyph: &[u8], mut x: i32, mut y: i32, mut w: i32, mut h: i32, rgb: i32, alpha: i32,
    ) {
        let mut s = STATE.lock().unwrap();
        let surface_w = s.width;
        let clip_min_x = s.clip_min_x;
        let clip_max_x = s.clip_max_x;
        let clip_min_y = s.clip_min_y;
        let clip_max_y = s.clip_max_y;

        let mut dst = surface_w * y + x;
        let mut dst_step = surface_w - w;
        let mut src_step = 0i32;
        let mut src = 0i32;

        if y < clip_min_y {
            let dy = clip_min_y - y;
            h -= dy;
            y = clip_min_y;
            src += w * dy;
            dst += surface_w * dy;
        }
        if y + h > clip_max_y {
            h -= y + h - clip_max_y;
        }
        if x < clip_min_x {
            let dx = clip_min_x - x;
            w -= dx;
            x = clip_min_x;
            src += dx;
            dst += dx;
            src_step += dx;
            dst_step += dx;
        }
        if x + w > clip_max_x {
            let dx = x + w - clip_max_x;
            w -= dx;
            src_step += dx;
            dst_step += dx;
        }
        if w > 0 && h > 0 {
            plot_trans(&mut s.pixels, glyph, rgb, src, dst, w, h, dst_step, src_step, alpha);
        }
    }
}

// @ObfuscatedName("fs.cx([I[BIIIIIIII)V") — PixFont.plot. Verbatim port
// of PixFont.java:861-897 with its 4-wide loop unroll preserved.
fn plot(
    dst: &mut [i32], glyph: &[u8], rgb: i32,
    mut src: i32, mut dst_off: i32, w: i32, h: i32,
    dst_row_step: i32, src_row_step: i32,
) {
    let blocks = -(w >> 2);
    let tail = -(w & 0x3);
    let mut row = -h;
    while row < 0 {
        let mut b = blocks;
        while b < 0 {
            for _ in 0..4 {
                let s = glyph[src as usize] as i8;
                src += 1;
                if s == 0 {
                    dst_off += 1;
                } else {
                    dst[dst_off as usize] = rgb;
                    dst_off += 1;
                }
            }
            b += 1;
        }
        let mut t = tail;
        while t < 0 {
            let s = glyph[src as usize] as i8;
            src += 1;
            if s == 0 {
                dst_off += 1;
            } else {
                dst[dst_off as usize] = rgb;
                dst_off += 1;
            }
            t += 1;
        }
        dst_off += dst_row_step;
        src += src_row_step;
        row += 1;
    }
}

// @ObfuscatedName("fs.cq([I[BIIIIIIII)V") — PixFont.plotTrans. Verbatim
// port of PixFont.java:936-951. Alpha-blends the colour over the dest
// pixel using the standard 8-bit RB/G split.
fn plot_trans(
    dst: &mut [i32], glyph: &[u8], rgb: i32,
    mut src: i32, mut dst_off: i32, w: i32, h: i32,
    dst_row_step: i32, src_row_step: i32, alpha: i32,
) {
    // Pre-multiply the colour by alpha (in 0..256).
    let rb = ((rgb & 0x00FF00FF) as i64) * alpha as i64;
    let g = ((rgb & 0x0000FF00) as i64) * alpha as i64;
    let pre = (((rb & 0xFF00FF00u32 as i64) + (g & 0x00FF0000)) >> 8) as i32;
    let inv = 256 - alpha;

    let mut row = -h;
    while row < 0 {
        let mut col = -w;
        while col < 0 {
            let s = glyph[src as usize] as i8;
            src += 1;
            if s == 0 {
                dst_off += 1;
            } else {
                let bg = dst[dst_off as usize];
                let bg_rb = ((bg & 0x00FF00FF) as i64) * inv as i64;
                let bg_g = ((bg & 0x0000FF00) as i64) * inv as i64;
                let blended = (((bg_rb & 0xFF00FF00u32 as i64) + (bg_g & 0x00FF0000)) >> 8) as i32;
                dst[dst_off as usize] = blended + pre;
                dst_off += 1;
            }
            col += 1;
        }
        dst_off += dst_row_step;
        src += src_row_step;
        row += 1;
    }
}
