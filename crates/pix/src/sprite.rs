//! Port of `jagex3.graphics.Pix8` + `Pix32` — palette-indexed and 32-bit sprite types
//! with `plot` / `scale_plot` / `trans_plot` variants that blit into a [`Pix2D`].
//!
//! Bridges to [`cache::sprite::SpriteSheet`] / `Sprite` so a cache-loaded sprite group
//! can be turned into one or more `Pix8`s (palette-indexed) or `Pix32`s (palette resolved).
//!
//! Naming + semantics mirror Java's:
//! - `wi` / `hi`: inner sprite dimensions
//! - `owi` / `ohi`: outer canvas dimensions (the bounds the sprite was drawn in)
//! - `xof` / `yof`: offset of the inner sprite within the outer canvas
//!
//! Pix8 palette index 0 is the transparent slot.

use cache::sprite::SpriteSheet;

use crate::pix2d::Pix2D;

/// 32-bit ARGB sprite (`0x00000000` = transparent, anything else = opaque RGB).
pub struct Pix32 {
    pub data: Vec<u32>,
    pub wi: i32,
    pub hi: i32,
    pub xof: i32,
    pub yof: i32,
    pub owi: i32,
    pub ohi: i32,
}

impl Pix32 {
    /// Direct blit at `(x, y) + (xof, yof)`, clipping to the destination's clip rect.
    /// Skips pixels with value 0 (Jagex's transparent sentinel).
    pub fn plot(&self, dst: &mut Pix2D, x: i32, y: i32) {
        let dst_w = dst.width;
        let mut sx = self.xof + x;
        let mut sy = self.yof + y;
        let mut dst_idx = dst_w * sy + sx;
        let mut src_idx = 0i32;
        let mut h = self.hi;
        let mut w = self.wi;
        let mut row_skip = dst_w - w;
        let mut src_pad = 0i32;
        if sy < dst.clip_min_y {
            let n = dst.clip_min_y - sy;
            h -= n;
            sy = dst.clip_min_y;
            src_idx += w * n;
            dst_idx += dst_w * n;
        }
        if sy + h > dst.clip_max_y {
            h -= sy + h - dst.clip_max_y;
        }
        if sx < dst.clip_min_x {
            let n = dst.clip_min_x - sx;
            w -= n;
            sx = dst.clip_min_x;
            src_idx += n;
            dst_idx += n;
            src_pad += n;
            row_skip += n;
        }
        if sx + w > dst.clip_max_x {
            let n = sx + w - dst.clip_max_x;
            w -= n;
            src_pad += n;
            row_skip += n;
        }
        if w <= 0 || h <= 0 {
            return;
        }
        // Java's plotSprite unrolls 4x; an unrolled-or-tail loop ends up the same speed
        // in Rust release with autovectorization. Use a plain loop.
        for _ in 0..h {
            for _ in 0..w {
                let s = self.data[src_idx as usize];
                if s != 0 {
                    dst.pixels[dst_idx as usize] = s;
                }
                src_idx += 1;
                dst_idx += 1;
            }
            dst_idx += row_skip;
            src_idx += src_pad;
        }
    }

    /// `Pix32.scalePlotSprite` — stretch from the OUTER bounds (`owi × ohi`) into a
    /// `(w, h)` region at `(x, y)`. Inner pixels (at `xof, yof, wi, hi`) occupy only the
    /// `wi/owi × hi/ohi` fraction of the destination.
    #[allow(clippy::too_many_arguments)]
    pub fn scale_plot(&self, dst: &mut Pix2D, mut x: i32, mut y: i32, mut w: i32, mut h: i32) {
        if w <= 0 || h <= 0 {
            return;
        }
        let owi = self.owi;
        let ohi = self.ohi;
        let step_x = (owi << 16) / w;
        let step_y = (ohi << 16) / h;
        let mut src_x_off = 0i32;
        let mut src_y_off = 0i32;
        if self.xof > 0 {
            let n = ((self.xof << 16) + step_x - 1) / step_x;
            x += n;
            src_x_off += step_x * n - (self.xof << 16);
        }
        if self.yof > 0 {
            let n = ((self.yof << 16) + step_y - 1) / step_y;
            y += n;
            src_y_off += step_y * n - (self.yof << 16);
        }
        if self.wi < owi {
            w = ((self.wi << 16) - src_x_off + step_x - 1) / step_x;
        }
        if self.hi < ohi {
            h = ((self.hi << 16) - src_y_off + step_y - 1) / step_y;
        }
        if w <= 0 || h <= 0 {
            return;
        }
        // Clip against destination clip rect.
        if y + h > dst.clip_max_y {
            h -= y + h - dst.clip_max_y;
        }
        if y < dst.clip_min_y {
            let n = dst.clip_min_y - y;
            h -= n;
            y = dst.clip_min_y;
            src_y_off += step_y * n;
        }
        if x + w > dst.clip_max_x {
            let n = x + w - dst.clip_max_x;
            w -= n;
        }
        if x < dst.clip_min_x {
            let n = dst.clip_min_x - x;
            w -= n;
            x = dst.clip_min_x;
            src_x_off += step_x * n;
        }
        if w <= 0 || h <= 0 {
            return;
        }
        let dst_w = dst.width;
        let inner_w = self.wi;
        let mut dst_idx = dst_w * y + x;
        let mut sy = src_y_off;
        for _ in 0..h {
            let mut sx = src_x_off;
            let src_row = (sy >> 16) * inner_w;
            for _ in 0..w {
                let s = self.data[(src_row + (sx >> 16)) as usize];
                if s != 0 {
                    dst.pixels[dst_idx as usize] = s;
                }
                dst_idx += 1;
                sx += step_x;
            }
            dst_idx += dst_w - w;
            sy += step_y;
        }
    }

    /// `Pix32.transPlotSprite` — direct blit with per-pixel alpha multiplied by `alpha`
    /// (0..256). 256 is opaque, 0 is fully transparent (skips draw).
    pub fn trans_plot(&self, dst: &mut Pix2D, x: i32, y: i32, alpha: i32) {
        if alpha == 0 {
            return;
        }
        if alpha >= 256 {
            self.plot(dst, x, y);
            return;
        }
        let dst_w = dst.width;
        let mut sx = self.xof + x;
        let mut sy = self.yof + y;
        let mut dst_idx = dst_w * sy + sx;
        let mut src_idx = 0i32;
        let mut h = self.hi;
        let mut w = self.wi;
        let mut row_skip = dst_w - w;
        let mut src_pad = 0i32;
        if sy < dst.clip_min_y {
            let n = dst.clip_min_y - sy;
            h -= n;
            sy = dst.clip_min_y;
            src_idx += w * n;
            dst_idx += dst_w * n;
        }
        if sy + h > dst.clip_max_y {
            h -= sy + h - dst.clip_max_y;
        }
        if sx < dst.clip_min_x {
            let n = dst.clip_min_x - sx;
            w -= n;
            sx = dst.clip_min_x;
            src_idx += n;
            dst_idx += n;
            src_pad += n;
            row_skip += n;
        }
        if sx + w > dst.clip_max_x {
            let n = sx + w - dst.clip_max_x;
            w -= n;
            src_pad += n;
            row_skip += n;
        }
        if w <= 0 || h <= 0 {
            return;
        }
        let inv = (256 - alpha) as u32;
        let a = alpha as u32;
        for _ in 0..h {
            for _ in 0..w {
                let s = self.data[src_idx as usize];
                if s != 0 {
                    let d = dst.pixels[dst_idx as usize];
                    let mix = (((s & 0xFF00FF) * a >> 8) & 0xFF00FF)
                        + (((s & 0xFF00) * a >> 8) & 0xFF00);
                    let keep = (((d & 0xFF00FF) * inv >> 8) & 0xFF00FF)
                        + (((d & 0xFF00) * inv >> 8) & 0xFF00);
                    dst.pixels[dst_idx as usize] = mix + keep;
                }
                src_idx += 1;
                dst_idx += 1;
            }
            dst_idx += row_skip;
            src_idx += src_pad;
        }
    }
}

/// 8-bit palette-indexed sprite. Palette index 0 = transparent.
pub struct Pix8 {
    pub data: Vec<u8>,
    pub bpal: Vec<u32>, // entry 0 is the transparent sentinel
    pub wi: i32,
    pub hi: i32,
    pub xof: i32,
    pub yof: i32,
    pub owi: i32,
    pub ohi: i32,
}

impl Pix8 {
    /// Direct blit. Index 0 pixels skip the destination write.
    pub fn plot(&self, dst: &mut Pix2D, x: i32, y: i32) {
        let dst_w = dst.width;
        let mut sx = self.xof + x;
        let mut sy = self.yof + y;
        let mut dst_idx = dst_w * sy + sx;
        let mut src_idx = 0i32;
        let mut h = self.hi;
        let mut w = self.wi;
        let mut row_skip = dst_w - w;
        let mut src_pad = 0i32;
        if sy < dst.clip_min_y {
            let n = dst.clip_min_y - sy;
            h -= n;
            sy = dst.clip_min_y;
            src_idx += w * n;
            dst_idx += dst_w * n;
        }
        if sy + h > dst.clip_max_y {
            h -= sy + h - dst.clip_max_y;
        }
        if sx < dst.clip_min_x {
            let n = dst.clip_min_x - sx;
            w -= n;
            sx = dst.clip_min_x;
            src_idx += n;
            dst_idx += n;
            src_pad += n;
            row_skip += n;
        }
        if sx + w > dst.clip_max_x {
            let n = sx + w - dst.clip_max_x;
            w -= n;
            src_pad += n;
            row_skip += n;
        }
        if w <= 0 || h <= 0 {
            return;
        }
        for _ in 0..h {
            for _ in 0..w {
                let idx = self.data[src_idx as usize] as usize;
                if idx != 0 && idx < self.bpal.len() {
                    dst.pixels[dst_idx as usize] = self.bpal[idx];
                }
                src_idx += 1;
                dst_idx += 1;
            }
            dst_idx += row_skip;
            src_idx += src_pad;
        }
    }

    /// Expand to a 32-bit Pix32 with palette resolved per pixel. Matches Java's
    /// `PixLoader.makePix32` conversion from Pix8 data.
    #[must_use]
    pub fn to_pix32(&self) -> Pix32 {
        let mut data = vec![0u32; (self.wi * self.hi) as usize];
        for (out, &idx) in data.iter_mut().zip(self.data.iter()) {
            let i = idx as usize;
            if i != 0 && i < self.bpal.len() {
                *out = self.bpal[i];
            }
        }
        Pix32 {
            data,
            wi: self.wi,
            hi: self.hi,
            xof: self.xof,
            yof: self.yof,
            owi: self.owi,
            ohi: self.ohi,
        }
    }
}

/// Build a `Pix8` per sprite from a cache-decoded `SpriteSheet`. The sheet's outer
/// dimensions become each sprite's `owi/ohi`; each sprite's individual `xof/yof/wi/hi`
/// come from the sheet's per-sprite metadata.
#[must_use]
pub fn sheet_to_pix8(sheet: &SpriteSheet) -> Vec<Pix8> {
    sheet
        .sprites
        .iter()
        .map(|s| Pix8 {
            data: s.indices.clone(),
            bpal: sheet.palette.clone(),
            wi: s.width as i32,
            hi: s.height as i32,
            xof: s.x_offset as i32,
            yof: s.y_offset as i32,
            owi: sheet.outer_width as i32,
            ohi: sheet.outer_height as i32,
        })
        .collect()
}

/// Build a `Pix32` per sprite (palette pre-resolved). Convenience for callers that want
/// the full 32-bit representation upfront.
#[must_use]
pub fn sheet_to_pix32(sheet: &SpriteSheet) -> Vec<Pix32> {
    sheet_to_pix8(sheet).into_iter().map(|p| p.to_pix32()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pix32_plot_writes_pixels() {
        let mut dst = Pix2D::new(16, 16);
        let sprite = Pix32 {
            data: vec![0xFFAABBCC; 4 * 4],
            wi: 4,
            hi: 4,
            xof: 0,
            yof: 0,
            owi: 4,
            ohi: 4,
        };
        sprite.plot(&mut dst, 2, 2);
        // Some interior pixel should be the sprite color.
        assert_eq!(dst.pixels[16 * 2 + 2], 0xFFAABBCC);
        assert_eq!(dst.pixels[16 * 5 + 5], 0xFFAABBCC);
    }

    #[test]
    fn pix8_plot_resolves_palette() {
        let mut dst = Pix2D::new(8, 8);
        let sprite = Pix8 {
            data: vec![0, 1, 1, 0, 1, 0, 1, 1, 0],
            bpal: vec![0, 0x00FF0000],
            wi: 3,
            hi: 3,
            xof: 0,
            yof: 0,
            owi: 3,
            ohi: 3,
        };
        sprite.plot(&mut dst, 1, 1);
        // Pixels where index == 1 should be the red palette entry. Index 0 leaves dst at 0.
        assert_eq!(dst.pixels[8 * 1 + 0], 0); // outside sprite
        assert_eq!(dst.pixels[8 * 1 + 1], 0); // sprite[0,0] = 0 (transparent)
        assert_eq!(dst.pixels[8 * 1 + 2], 0x00FF0000); // sprite[1,0] = 1 → red
    }
}
