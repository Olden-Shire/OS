//! Upload a client-crate software frame (`Vec<i32>` of `0x00RRGGBB`) as an
//! opaque egui texture each frame. (Mirrors jaged's pix_bridge.)

use eframe::egui;

pub fn upload_rgb(
    ctx: &egui::Context,
    name: impl Into<String>,
    pixels: &[i32],
    w: usize,
    h: usize,
) -> egui::TextureHandle {
    upload_rgb_opts(ctx, name, pixels, w, h, egui::TextureOptions::NEAREST)
}

/// Like [`upload_rgb`] but with linear filtering — for the world map, whose
/// 4px/tile tiles get upscaled when zoomed in; nearest-neighbour turns the thin
/// baked wall lines into ugly blocky bricks, linear keeps them smooth.
pub fn upload_rgb_linear(
    ctx: &egui::Context,
    name: impl Into<String>,
    pixels: &[i32],
    w: usize,
    h: usize,
) -> egui::TextureHandle {
    upload_rgb_opts(ctx, name, pixels, w, h, egui::TextureOptions::LINEAR)
}

fn upload_rgb_opts(
    ctx: &egui::Context,
    name: impl Into<String>,
    pixels: &[i32],
    w: usize,
    h: usize,
    opts: egui::TextureOptions,
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
    ctx.load_texture(name, img, opts)
}
