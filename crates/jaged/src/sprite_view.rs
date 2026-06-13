//! Sprite-sheet viewer — decodes through the CLIENT sprite loader
//! (`pix_loader::decode_pix32_array`, the exact in-game sprite depack)
//! and shows every sprite tiled with its size + offset, palette already
//! resolved to RGB. Click sizing matches the native sprite dimensions.

use client::graphics::pix32::Pix32;
use client::graphics::pix_loader;
use eframe::egui;

pub fn draw(ui: &mut egui::Ui, group_id: u32, bytes: &[u8]) {
    if bytes.is_empty() {
        ui.label("(empty)");
        return;
    }
    let sprites = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pix_loader::decode_pix32_array(bytes)
    })) {
        Ok(s) => s,
        Err(_) => {
            ui.colored_label(egui::Color32::LIGHT_RED, "sprite decode failed");
            return;
        }
    };

    // Sanity-cap each sprite's pixel count so a malformed wi/hi can't OOM us.
    const MAX_PIXELS: i32 = 4 * 1024 * 1024;
    if sprites.iter().any(|s| s.wi as i64 * s.hi as i64 > MAX_PIXELS as i64) {
        ui.colored_label(egui::Color32::LIGHT_RED, "sprite has implausible size — likely malformed");
        return;
    }

    let (outer_w, outer_h) = sprites.first().map_or((0, 0), |s| (s.owi, s.ohi));

    section(ui, "sheet", |ui| {
        egui::Grid::new("sheet_meta").num_columns(2).striped(true).show(ui, |ui| {
            kv(ui, "group", &group_id.to_string());
            kv(ui, "outer size", &format!("{outer_w} × {outer_h}"));
            kv(ui, "sprites", &sprites.len().to_string());
        });
    });

    section(ui, "sprites", |ui| {
        let visible: Vec<(usize, &Pix32)> = sprites
            .iter()
            .enumerate()
            .filter(|(_, s)| s.wi > 0 && s.hi > 0)
            .collect();
        let empty = sprites.len() - visible.len();
        if empty > 0 {
            ui.label(
                egui::RichText::new(format!("({empty} empty sprite slots hidden)")).weak().small(),
            );
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            for (i, sprite) in visible {
                // Pix32.data is palette-resolved 0x00RRGGBB; index-0 pixels
                // resolve to 0 (the sheet's transparent slot) → alpha 0.
                let mut rgba = Vec::with_capacity(sprite.data.len() * 4);
                for &p in &sprite.data {
                    let a = if p == 0 { 0 } else { 0xFF };
                    rgba.extend_from_slice(&[
                        ((p >> 16) & 0xFF) as u8,
                        ((p >> 8) & 0xFF) as u8,
                        (p & 0xFF) as u8,
                        a,
                    ]);
                }
                let img = egui::ColorImage::from_rgba_unmultiplied(
                    [sprite.wi as usize, sprite.hi as usize],
                    &rgba,
                );
                let texture = ui.ctx().load_texture(
                    format!("sprite_{group_id}_{i}"),
                    img,
                    egui::TextureOptions::NEAREST,
                );
                let display_size = egui::vec2(sprite.wi as f32, sprite.hi as f32);
                ui.add(
                    egui::Image::from_texture(&texture)
                        .fit_to_exact_size(display_size)
                        .max_size(egui::vec2(160.0, 160.0))
                        .bg_fill(egui::Color32::from_rgb(40, 40, 48)),
                )
                .on_hover_text(format!(
                    "#{i}  {}×{}  offset ({}, {})",
                    sprite.wi, sprite.hi, sprite.xof, sprite.yof
                ));
            }
        });
    });
}

fn section(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(title.to_uppercase()).small().weak());
    egui::Frame::group(ui.style())
        .corner_radius(4.0)
        .inner_margin(egui::Margin::same(10))
        .show(ui, body);
    ui.add_space(6.0);
}

fn kv(ui: &mut egui::Ui, k: &str, v: &str) {
    ui.label(egui::RichText::new(k).weak());
    ui.label(egui::RichText::new(v).monospace());
    ui.end_row();
}
