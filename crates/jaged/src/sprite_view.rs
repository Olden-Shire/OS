//! Sprite-sheet viewer. Decodes a sprites-archive group (sheet) and shows every sprite
//! tiled with its size + offset readout. Click a sprite to expand it at native scale.

use cache::sprite::SpriteSheet;
use eframe::egui;

/// One ColorImage per sprite, uploaded once and reused while the same group is selected.
pub fn draw(ui: &mut egui::Ui, group_id: u32, bytes: &[u8]) {
    if bytes.is_empty() {
        ui.label("(empty)");
        return;
    }
    let sheet = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        SpriteSheet::decode(bytes)
    })) {
        Ok(s) => s,
        Err(_) => {
            ui.colored_label(egui::Color32::LIGHT_RED, "sprite decode failed");
            return;
        }
    };

    // Sanity-cap each sprite's pixel count so a malformed wi/hi can't OOM us.
    const MAX_PIXELS: usize = 4 * 1024 * 1024;
    for s in &sheet.sprites {
        if s.indices.len() > MAX_PIXELS {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                format!("sprite has implausible size {}×{} — likely malformed", s.width, s.height),
            );
            return;
        }
    }

    section(ui, "sheet", |ui| {
        egui::Grid::new("sheet_meta").num_columns(2).striped(true).show(ui, |ui| {
            kv(ui, "group", &group_id.to_string());
            kv(ui, "outer size", &format!("{} × {}", sheet.outer_width, sheet.outer_height));
            kv(ui, "palette", &format!("{} colors (incl. transparent)", sheet.palette.len()));
            kv(ui, "sprites", &sheet.sprites.len().to_string());
        });
    });

    section(ui, "sprites", |ui| {
        let visible: Vec<(usize, &cache::sprite::Sprite)> = sheet
            .sprites
            .iter()
            .enumerate()
            .filter(|(_, s)| s.width > 0 && s.height > 0)
            .collect();
        let empty = sheet.sprites.len() - visible.len();
        if empty > 0 {
            ui.label(
                egui::RichText::new(format!("({empty} empty sprite slots hidden)"))
                    .weak()
                    .small(),
            );
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            for (i, sprite) in visible {
                let rgba = sprite.to_rgba(&sheet.palette);
                let img = egui::ColorImage::from_rgba_unmultiplied(
                    [sprite.width as usize, sprite.height as usize],
                    &rgba,
                );
                let texture = ui.ctx().load_texture(
                    format!("sprite_{group_id}_{i}"),
                    img,
                    egui::TextureOptions::NEAREST,
                );
                let display_size = egui::vec2(sprite.width as f32, sprite.height as f32);
                ui.add(
                    egui::Image::from_texture(&texture)
                        .fit_to_exact_size(display_size)
                        .max_size(egui::vec2(160.0, 160.0))
                        .bg_fill(egui::Color32::from_rgb(40, 40, 48)),
                )
                .on_hover_text(format!(
                    "#{i}  {}×{}  offset ({}, {})",
                    sprite.width, sprite.height, sprite.x_offset, sprite.y_offset
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
