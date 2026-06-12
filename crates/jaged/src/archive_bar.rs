//! Bottom archive selector — fixed grid of cards, one per archive. Each card shows the
//! archive's name + group count and is the entry point for navigating the cache.
//!
//! 16 archives in OSRS rev1, laid out as 2 rows × 8 columns (tunable). Cards highlight
//! the currently-selected archive. Clicking a card resets the group / file selection.

use cache::{ARCHIVE_COUNT, Cache};
use eframe::egui;

use crate::{Selection, archive_label};

const ROWS: usize = 2;
const COLS: usize = (ARCHIVE_COUNT as usize).div_ceil(ROWS);
const CARD_HEIGHT: f32 = 54.0;
const CARD_GAP: f32 = 6.0;

/// Total height the panel wants — outer padding + 2 card rows + row gap.
pub fn panel_height() -> f32 {
    CARD_HEIGHT * ROWS as f32 + CARD_GAP * (ROWS as f32 - 1.0) + 18.0
}

pub fn draw(ui: &mut egui::Ui, cache: &Cache, sel: &mut Selection) {
    let avail_w = ui.available_width();
    let card_w = ((avail_w - CARD_GAP * (COLS as f32 - 1.0)) / COLS as f32).max(80.0);

    ui.add_space(2.0);
    for row in 0..ROWS {
        ui.horizontal(|ui| {
            for col in 0..COLS {
                let archive = (row * COLS + col) as u8;
                if archive >= ARCHIVE_COUNT {
                    break;
                }
                draw_card(ui, cache, archive, sel, card_w);
                if col + 1 < COLS {
                    ui.add_space(CARD_GAP);
                }
            }
        });
        if row + 1 < ROWS {
            ui.add_space(CARD_GAP);
        }
    }
}

fn draw_card(ui: &mut egui::Ui, cache: &Cache, archive: u8, sel: &mut Selection, w: f32) {
    let index = cache.index(archive);
    let selected = sel.archive == Some(archive);

    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(w, CARD_HEIGHT), egui::Sense::click());

    let visuals = ui.visuals();
    let bg = if selected {
        visuals.selection.bg_fill
    } else if resp.hovered() {
        visuals.widgets.hovered.bg_fill
    } else {
        visuals.widgets.inactive.bg_fill
    };
    let stroke = if selected {
        egui::Stroke::new(1.5, visuals.selection.stroke.color)
    } else {
        egui::Stroke::new(1.0, visuals.widgets.inactive.bg_stroke.color)
    };

    let painter = ui.painter_at(rect);
    painter.rect(rect, 4.0, bg, stroke, egui::StrokeKind::Inside);

    let inner = rect.shrink2(egui::vec2(8.0, 6.0));
    let name = archive_label(archive);
    let name_color = if selected {
        visuals.selection.stroke.color
    } else {
        visuals.text_color()
    };
    painter.text(
        inner.left_top(),
        egui::Align2::LEFT_TOP,
        name,
        egui::FontId::proportional(13.0),
        name_color,
    );
    painter.text(
        inner.left_bottom() - egui::vec2(0.0, 2.0),
        egui::Align2::LEFT_BOTTOM,
        format!("{} groups", index.size),
        egui::FontId::monospace(10.0),
        visuals.weak_text_color(),
    );

    if resp.clicked() {
        sel.archive = Some(archive);
        // Auto-select the first group in this archive so the viewport + details have
        // something to render immediately. For multi-file groups, also pick the first
        // file. Falls back to None on empty archives (rare — most rev1 archives have
        // at least one group).
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
