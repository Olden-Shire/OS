//! Convert a software-rendered `Pix2D` buffer into an egui texture each frame.
//!
//! `Pix2D` stores ARGB packed `u32`s (`0xAARRGGBB`). egui's `ColorImage` wants RGBA
//! bytes. We shuffle bytes inline. Texture handle is stored on the caller (so it lives
//! for at least one frame after upload — required by egui's deferred composite).

use eframe::egui;
use pix::Pix2D;

/// Upload `pix.pixels` as a fresh egui texture. Treats input alpha as opaque if it's 0
/// (Pix3D doesn't bother writing alpha, so pixels appear as `0x00RRGGBB`).
pub fn upload(ctx: &egui::Context, name: impl Into<String>, pix: &Pix2D) -> egui::TextureHandle {
    let w = pix.width.max(0) as usize;
    let h = pix.height.max(0) as usize;
    let mut rgba = Vec::with_capacity(w * h * 4);
    for &p in &pix.pixels {
        let a = ((p >> 24) & 0xFF) as u8;
        let r = ((p >> 16) & 0xFF) as u8;
        let g = ((p >> 8) & 0xFF) as u8;
        let b = (p & 0xFF) as u8;
        // If Pix2D never wrote alpha (clear color was 0), treat zero pixels as fully
        // transparent and any non-zero pixel as opaque. Otherwise honor the alpha.
        let alpha = if a == 0 {
            if (r | g | b) == 0 { 0 } else { 0xFF }
        } else {
            a
        };
        rgba.extend_from_slice(&[r, g, b, alpha]);
    }
    let img = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
    ctx.load_texture(name, img, egui::TextureOptions::NEAREST)
}
