//! Left archive rail — a slim vertical list of the 16 archives, the
//! primary navigation. Replaces the old bottom card grid: scannable,
//! always visible, and frees the whole bottom + the cramped feel.
//!
//! Each row shows the archive name and its group count; the selected row
//! gets an accent bar + tint. Clicking selects the archive and auto-picks
//! its first group.

use cache::{ARCHIVE_COUNT, Cache};
use eframe::egui;

use crate::{Selection, archive_label, theme};

const ROW_H: f32 = 30.0;

pub fn draw(ui: &mut egui::Ui, cache: &Cache, sel: &mut Selection) {
    ui.add_space(8.0);
    ui.label(egui::RichText::new("ARCHIVES").size(11.0).weak().strong());
    ui.add_space(4.0);

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        for archive in 0..ARCHIVE_COUNT {
            row(ui, cache, archive, sel);
        }
    });
}

fn row(ui: &mut egui::Ui, cache: &Cache, archive: u8, sel: &mut Selection) {
    let index = cache.index(archive);
    let selected = sel.archive == Some(archive);

    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, ROW_H), egui::Sense::click());

    let bg = if selected {
        theme::ACCENT.gamma_multiply(0.22)
    } else if resp.hovered() {
        ui.visuals().widgets.hovered.bg_fill
    } else {
        egui::Color32::TRANSPARENT
    };
    let painter = ui.painter_at(rect);
    if bg != egui::Color32::TRANSPARENT {
        painter.rect_filled(rect, 5.0, bg);
    }
    if selected {
        // Accent bar on the leading edge.
        let bar = egui::Rect::from_min_size(rect.left_top(), egui::vec2(3.0, rect.height()));
        painter.rect_filled(bar, 2.0, theme::ACCENT);
    }

    let name_color = if selected { theme::ACCENT_TEXT } else { ui.visuals().text_color() };
    painter.text(
        rect.left_center() + egui::vec2(12.0, 0.0),
        egui::Align2::LEFT_CENTER,
        archive_label(archive),
        egui::FontId::proportional(13.5),
        name_color,
    );
    painter.text(
        rect.right_center() - egui::vec2(10.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        index.size.to_string(),
        egui::FontId::monospace(11.0),
        ui.visuals().weak_text_color(),
    );

    if resp.clicked() {
        sel.archive = Some(archive);
        let first_group = index.group_ids.first().copied().map(|g| g as u32);
        sel.group = first_group;
        sel.file_id = match first_group {
            Some(gid) => index
                .file_ids
                .get(gid as usize)
                .and_then(|files| (files.len() > 1).then(|| files[0])),
            None => None,
        };
    }
}
