//! Convert a software-rendered `Pix2D` buffer into an egui texture each frame.
//!
//! `Pix2D` stores ARGB packed `u32`s (`0xAARRGGBB`). egui's `ColorImage` wants RGBA
//! bytes. We shuffle bytes inline. Texture handle is stored on the caller (so it lives
//! for at least one frame after upload — required by egui's deferred composite).

use eframe::egui;

/// Upload a CLIENT-crate frame (`Vec<i32>` of `0x00RRGGBB`, no alpha —
/// the client's Pix2D pixel format) as an opaque egui texture.
pub fn upload_rgb(
    ctx: &egui::Context,
    name: impl Into<String>,
    pixels: &[i32],
    w: usize,
    h: usize,
) -> egui::TextureHandle {
    let mut rgba = Vec::with_capacity(w * h * 4);
    for &p in pixels.iter().take(w * h) {
        rgba.extend_from_slice(&[
            ((p >> 16) & 0xFF) as u8,
            ((p >> 8) & 0xFF) as u8,
            (p & 0xFF) as u8,
            0xFF,
        ]);
    }
    let img = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
    ctx.load_texture(name, img, egui::TextureOptions::NEAREST)
}
