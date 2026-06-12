//! `jagex3.graphics.PixLoader::depack` — sprite-sheet decoder.
//!
//! Cache archive 8 stores groups of related sprites (one group = one "sheet"). Each sheet
//! has a shared outer size (`owi × ohi`) and palette; individual sprites have their own
//! offset, inner size, and palette-indexed pixel data.
//!
//! On-disk layout (read from the END of the buffer):
//!
//! ```text
//! [u8 layout][wi*hi palette indices]   per sprite, in order at the START of the buffer
//! …
//! [palette: (count-1) × 3 bytes RGB]
//! [array: xof[count], yof[count], wi[count], hi[count] as u16 BE]
//! [u16 owi][u16 ohi][u8 palette_count - 1]
//! [u16 count]
//! ```
//!
//! Layout flag:
//! - `0` — row-major
//! - `1` — column-major
//!
//! Palette index `0` is transparent; other indices map into `palette[]`.

use io::packet::Packet;

#[derive(Debug, Clone)]
pub struct SpriteSheet {
    pub outer_width: u16,
    pub outer_height: u16,
    pub palette: Vec<u32>, // index 0 = transparent (sentinel), rest are 0x00RRGGBB
    pub sprites: Vec<Sprite>,
}

#[derive(Debug, Clone)]
pub struct Sprite {
    pub x_offset: u16,
    pub y_offset: u16,
    pub width: u16,
    pub height: u16,
    /// Palette indices, `wi * hi` bytes (row-major). `0` = transparent.
    pub indices: Vec<u8>,
}

impl SpriteSheet {
    pub fn decode(src: &[u8]) -> Self {
        if src.len() < 7 {
            return Self { outer_width: 0, outer_height: 0, palette: vec![0], sprites: Vec::new() };
        }
        let mut p = Packet::from_vec(src.to_vec());

        // Trailer: last 2 bytes = count.
        p.pos = src.len() - 2;
        let count = p.g2() as usize;

        // Header block at: end - 7 - count*8. Bail if the math says we'd seek before 0.
        let header_size = 7 + count * 8;
        if header_size > src.len() {
            return Self { outer_width: 0, outer_height: 0, palette: vec![0], sprites: Vec::new() };
        }
        let header_start = src.len() - header_size;
        p.pos = header_start;
        let outer_width = p.g2() as u16;
        let outer_height = p.g2() as u16;
        let palette_count = (p.g1() & 0xFF) as usize + 1;

        // Per-sprite arrays.
        let mut x_offsets = vec![0u16; count];
        let mut y_offsets = vec![0u16; count];
        let mut widths = vec![0u16; count];
        let mut heights = vec![0u16; count];
        for x in &mut x_offsets { *x = p.g2() as u16; }
        for y in &mut y_offsets { *y = p.g2() as u16; }
        for w in &mut widths { *w = p.g2() as u16; }
        for h in &mut heights { *h = p.g2() as u16; }

        // Palette: header_start - (palette_count - 1) * 3. Same bounds guard as above.
        let palette_bytes = (palette_count - 1) * 3;
        if palette_bytes > header_start {
            return Self { outer_width, outer_height, palette: vec![0], sprites: Vec::new() };
        }
        p.pos = header_start - palette_bytes;
        let mut palette = vec![0u32; palette_count];
        for entry in palette.iter_mut().skip(1) {
            let rgb = p.g3() as u32;
            // Jagex remaps pure-black (0) to (0,0,1) so it isn't mistaken for transparent.
            *entry = if rgb == 0 { 1 } else { rgb };
        }

        // Sprite pixel data at start of buffer.
        p.pos = 0;
        let mut sprites = Vec::with_capacity(count);
        for i in 0..count {
            let w = widths[i] as usize;
            let h = heights[i] as usize;
            let n = w * h;
            let mut indices = vec![0u8; n];
            let layout = p.g1();
            if layout == 0 {
                for v in indices.iter_mut() {
                    *v = p.g1b() as u8;
                }
            } else {
                // Column-major: stored col-by-col, but we want row-major output.
                for col in 0..w {
                    for row in 0..h {
                        indices[row * w + col] = p.g1b() as u8;
                    }
                }
            }
            sprites.push(Sprite {
                x_offset: x_offsets[i],
                y_offset: y_offsets[i],
                width: widths[i],
                height: heights[i],
                indices,
            });
        }
        Self { outer_width, outer_height, palette, sprites }
    }
}

impl Sprite {
    /// Render to 32-bit RGBA8 (premultiplied per egui's `ColorImage`). Transparent pixels
    /// get `0x00000000`. Palette indices that exceed the palette are also rendered as
    /// transparent — some sheets in the cache reference out-of-range indices (probably
    /// originally relying on the renderer's clip behaviour).
    #[must_use]
    pub fn to_rgba(&self, palette: &[u32]) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.indices.len() * 4);
        for &idx in &self.indices {
            let i = idx as usize;
            if idx == 0 || i >= palette.len() {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                let rgb = palette[i];
                let r = ((rgb >> 16) & 0xFF) as u8;
                let g = ((rgb >> 8) & 0xFF) as u8;
                let b = (rgb & 0xFF) as u8;
                out.extend_from_slice(&[r, g, b, 0xFF]);
            }
        }
        out
    }
}
