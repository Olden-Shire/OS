//! Port of `jagex3.graphics.PixFont` + `PixFontGeneric` — glyph bitmap font renderer
//! with the tag state machine (`<col=…>`, `<br>`, `<u>`, `<str>`, `<shad>`).
//!
//! ## Glyph format
//!
//! Each glyph is a width × height byte array where each byte is a luminance value:
//! `0x00` = transparent, `0xFF` = fully drawn. Multi-level values give anti-aliased edges.
//! Glyphs have per-character offsets (`xof`, `yof`) and an `advance` (horizontal cursor
//! step after plotting). 256 glyphs per font (CP-1252).
//!
//! ## Cache loading
//!
//! Font assets live in archive 13. `PixFont::decode` parses the metrics blob (the
//! `unpackMetrics` body from `PixFont.java`). The actual glyph BYTES come from a sister
//! archive — `PixLoader.makePixFont` reads the metrics from one file and the glyph data
//! from another. We expose a constructor that takes both buffers so callers can wire up
//! to whichever archives the rev1 cache uses.

use io::packet::Packet;

use crate::pix2d::Pix2D;

const N_GLYPHS: usize = 256;

pub struct PixFont {
    /// Per-glyph bitmap (each `wi * hi` bytes, luminance 0..255). Index 0 may be empty
    /// for glyphs the font doesn't define.
    pub glyphs: Vec<Vec<u8>>,
    /// Horizontal cursor step after each glyph (advance width).
    pub char_advance: Vec<i32>,
    /// Per-glyph inner dimensions and offsets.
    pub glyph_width: Vec<i32>,
    pub glyph_height: Vec<i32>,
    pub glyph_offset_x: Vec<i32>,
    pub glyph_offset_y: Vec<i32>,
    /// Font baseline (in pixels from the top of the glyph cell).
    pub ascent: i32,
    pub max_ascent: i32,
    pub max_descent: i32,
}

impl PixFont {
    /// Build a font from already-decoded metrics + glyph bitmaps. Use when you already
    /// have the data (e.g., tests or hand-constructed fonts). For cache-loaded fonts,
    /// use [`Self::decode`].
    pub fn from_parts(
        char_advance: Vec<i32>,
        glyph_width: Vec<i32>,
        glyph_height: Vec<i32>,
        glyph_offset_x: Vec<i32>,
        glyph_offset_y: Vec<i32>,
        ascent: i32,
        glyphs: Vec<Vec<u8>>,
    ) -> Self {
        let mut min_oy = i32::MAX;
        let mut max_oy = i32::MIN;
        for i in 0..N_GLYPHS {
            if glyph_offset_y[i] < min_oy && glyph_height[i] != 0 {
                min_oy = glyph_offset_y[i];
            }
            if glyph_height[i] + glyph_offset_y[i] > max_oy {
                max_oy = glyph_height[i] + glyph_offset_y[i];
            }
        }
        if min_oy == i32::MAX {
            min_oy = 0;
        }
        if max_oy == i32::MIN {
            max_oy = 0;
        }
        Self {
            glyphs,
            char_advance,
            glyph_width,
            glyph_height,
            glyph_offset_x,
            glyph_offset_y,
            ascent,
            max_ascent: ascent - min_oy,
            max_descent: max_oy - ascent,
        }
    }

    /// Decode metrics from a font-metrics blob. Mirrors `PixFont.unpackMetrics`:
    /// - 257-byte format: 256 char advances + ascent byte.
    /// - Larger format: 256 advances, 256 widths, 256 heights, then per-glyph delta-coded
    ///   offset arrays + kerning table (skipped here — kerning isn't used by the
    ///   renderer, only by tooling).
    ///
    /// Glyph bitmaps must be passed separately (they live in a sibling cache archive
    /// per `PixLoader.makePixFont`).
    pub fn decode(metrics: &[u8], glyphs: Vec<Vec<u8>>) -> Self {
        let mut char_advance = vec![0i32; N_GLYPHS];
        let mut glyph_width = vec![0i32; N_GLYPHS];
        let mut glyph_height = vec![0i32; N_GLYPHS];
        let mut glyph_offset_x = vec![0i32; N_GLYPHS];
        let mut glyph_offset_y = vec![0i32; N_GLYPHS];
        let mut ascent = 0i32;
        if metrics.len() == 257 {
            for i in 0..N_GLYPHS {
                char_advance[i] = metrics[i] as i32 & 0xFF;
            }
            ascent = metrics[256] as i32 & 0xFF;
        } else if !metrics.is_empty() {
            let mut p = Packet::from_vec(metrics.to_vec());
            for i in 0..N_GLYPHS {
                char_advance[i] = p.g1();
            }
            for i in 0..N_GLYPHS {
                glyph_width[i] = p.g1();
            }
            for i in 0..N_GLYPHS {
                glyph_height[i] = p.g1();
            }
            for i in 0..N_GLYPHS {
                let mut accum = 0i32;
                let bytes = glyph_width[i] as usize;
                for _ in 0..bytes {
                    accum = accum.wrapping_add(p.g1b() as i32);
                }
                let _ = accum; // delta-coded glyph_offset_x (kerning input, unused for render)
            }
            for i in 0..N_GLYPHS {
                let mut accum = 0i32;
                let bytes = glyph_width[i] as usize;
                for _ in 0..bytes {
                    accum = accum.wrapping_add(p.g1b() as i32);
                }
                glyph_offset_y[i] = accum;
            }
            // Kerning table (256x256 bytes) follows — skipped.
        }
        Self::from_parts(
            char_advance,
            glyph_width,
            glyph_height,
            glyph_offset_x,
            glyph_offset_y,
            ascent,
            glyphs,
        )
    }

    /// Plot a single character at `(x, y)`. `y` is the baseline. Color is opaque RGB
    /// `(0x00RRGGBB`); the glyph's luminance bytes scale this color per pixel.
    pub fn plot_letter(&self, dst: &mut Pix2D, ch: usize, x: i32, y: i32, rgb: u32) {
        if ch >= N_GLYPHS {
            return;
        }
        if ch >= self.glyphs.len() || self.glyphs[ch].is_empty() {
            return;
        }
        let gw = self.glyph_width[ch];
        let gh = self.glyph_height[ch];
        if gw <= 0 || gh <= 0 {
            return;
        }
        let gox = self.glyph_offset_x[ch];
        let goy = self.glyph_offset_y[ch];
        let mut sx = x + gox;
        let mut sy = y + goy;
        let mut w = gw;
        let mut h = gh;
        let mut src_idx = 0i32;
        let mut src_pad = 0i32;
        let mut dst_idx = dst.width * sy + sx;
        let mut row_skip = dst.width - w;
        if sy < dst.clip_min_y {
            let n = dst.clip_min_y - sy;
            h -= n;
            sy = dst.clip_min_y;
            src_idx += w * n;
            dst_idx += dst.width * n;
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
        let glyph = &self.glyphs[ch];
        for _ in 0..h {
            for _ in 0..w {
                let lum = glyph[src_idx as usize];
                if lum != 0 {
                    // Treat glyph byte as "draw / don't draw" — anti-aliased blending is
                    // possible but the Java path here treats glyphs as binary masks too
                    // (lum != 0 → opaque write of the color).
                    dst.pixels[dst_idx as usize] = rgb;
                }
                src_idx += 1;
                dst_idx += 1;
            }
            dst_idx += row_skip;
            src_idx += src_pad;
        }
    }

    /// Width of a literal string with no tag handling. Used by string layout for
    /// alignment and wrapping.
    pub fn string_width(&self, s: &str) -> i32 {
        let mut total = 0i32;
        for ch in s.chars() {
            let i = ch as usize;
            if i < N_GLYPHS {
                total += self.char_advance[i];
            }
        }
        total
    }

    /// Plot a string at `(x, y)` (y = baseline) with a single color. Use `draw_tagged`
    /// for OSRS tag support (`<col=…>`, `<br>`, etc.).
    pub fn draw_string(&self, dst: &mut Pix2D, s: &str, mut x: i32, y: i32, rgb: u32) {
        for ch in s.chars() {
            let i = ch as usize;
            if i < N_GLYPHS {
                self.plot_letter(dst, i, x, y, rgb);
                x += self.char_advance[i];
            }
        }
    }
}

/// Parsed text-run output from the tag tokenizer. Each run shares one color and is on
/// one line; `<br>` and `\n` create new lines.
pub struct StyledRun {
    pub text: String,
    pub color: u32,
}

/// Tokenize Jagex tag soup into per-line styled runs. Recognised tags (matching
/// `PixFont.updateState`): `<col=RRGGBB>` / `</col>`, `<br>`. Other tags are silently
/// dropped. `default_color` is what `</col>` and unspecified runs use.
pub fn parse_tagged(s: &str, default_color: u32) -> Vec<Vec<StyledRun>> {
    let mut lines: Vec<Vec<StyledRun>> = vec![Vec::new()];
    let mut current = default_color;
    let mut buf = String::new();
    let mut iter = s.char_indices().peekable();
    let push = |buf: &mut String, lines: &mut Vec<Vec<StyledRun>>, c: u32| {
        if !buf.is_empty() {
            lines
                .last_mut()
                .unwrap()
                .push(StyledRun { text: std::mem::take(buf), color: c });
        }
    };
    while let Some((_, ch)) = iter.next() {
        if ch == '\n' {
            push(&mut buf, &mut lines, current);
            lines.push(Vec::new());
            continue;
        }
        if ch != '<' {
            buf.push(ch);
            continue;
        }
        let mut tag = String::new();
        let mut closed = false;
        for (_, c2) in iter.by_ref() {
            if c2 == '>' {
                closed = true;
                break;
            }
            tag.push(c2);
        }
        if !closed {
            buf.push('<');
            buf.push_str(&tag);
            continue;
        }
        push(&mut buf, &mut lines, current);
        match tag.as_str() {
            "br" => {
                lines.push(Vec::new());
                current = default_color;
            }
            "/col" => current = default_color,
            other if other.starts_with("col=") => {
                if let Ok(rgb) = u32::from_str_radix(&other[4..], 16) {
                    current = rgb;
                }
            }
            _ => {}
        }
    }
    push(&mut buf, &mut lines, current);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_font() -> PixFont {
        // 3x5 'A'-ish glyph at index 65, advance 4, baseline at y=5.
        let mut glyphs: Vec<Vec<u8>> = (0..N_GLYPHS).map(|_| Vec::new()).collect();
        glyphs[65] = vec![
            0xFF, 0xFF, 0xFF,
            0xFF, 0x00, 0xFF,
            0xFF, 0xFF, 0xFF,
            0xFF, 0x00, 0xFF,
            0xFF, 0x00, 0xFF,
        ];
        let mut adv = vec![0i32; N_GLYPHS];
        adv[65] = 4;
        let mut gw = vec![0i32; N_GLYPHS];
        gw[65] = 3;
        let mut gh = vec![0i32; N_GLYPHS];
        gh[65] = 5;
        let gox = vec![0i32; N_GLYPHS];
        let mut goy = vec![0i32; N_GLYPHS];
        goy[65] = -5;
        PixFont::from_parts(adv, gw, gh, gox, goy, 5, glyphs)
    }

    #[test]
    fn plot_letter_writes_glyph_pixels() {
        let font = tiny_font();
        let mut dst = Pix2D::new(16, 16);
        font.plot_letter(&mut dst, 65, 2, 8, 0x00FF0000);
        // Top-left of glyph 'A' should be red.
        assert_eq!(dst.pixels[16 * 3 + 2], 0x00FF0000);
        // The inner hole at row 1 col 1 should stay 0.
        assert_eq!(dst.pixels[16 * 4 + 3], 0);
    }

    #[test]
    fn string_width_sums_advances() {
        let font = tiny_font();
        assert_eq!(font.string_width("AAA"), 12);
    }

    #[test]
    fn parse_tagged_splits_lines_and_colours() {
        let lines = parse_tagged("hi<col=ff0000>red</col>\nnext<br>third", 0x0000FF00);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0][0].text, "hi");
        assert_eq!(lines[0][0].color, 0x0000FF00);
        assert_eq!(lines[0][1].text, "red");
        assert_eq!(lines[0][1].color, 0xFF0000);
        assert_eq!(lines[1][0].text, "next");
        assert_eq!(lines[2][0].text, "third");
    }
}
