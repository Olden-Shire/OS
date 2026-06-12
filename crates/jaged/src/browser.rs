//! Left-column browser panels: groups (top) + files (bottom). Archives live in the
//! bottom card bar — see `archive_bar`.
//!
//! Both panels honour the current [`Selection`] and update it on click. The files panel
//! is only shown by the caller when the selected group actually has multiple files;
//! single-file groups collapse it away so the groups list takes the full left column.

use cache::Cache;
use eframe::egui;

use crate::Selection;

/// Top-left: list of groups in the currently-selected archive.
pub fn draw_groups(ui: &mut egui::Ui, cache: &Cache, sel: &mut Selection) {
    let Some(archive) = sel.archive else {
        ui.label(egui::RichText::new("groups").weak().small());
        ui.add_space(6.0);
        ui.label(egui::RichText::new("(select an archive below)").weak().italics());
        return;
    };
    let index = cache.index(archive);
    ui.label(egui::RichText::new(format!("groups · {}", index.size)).weak().small());
    let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
    egui::ScrollArea::vertical()
        .id_salt("col_groups")
        .auto_shrink([false, false])
        .show_rows(ui, row_h, index.group_ids.len(), |ui, range| {
            for i in range {
                let gid = index.group_ids[i] as u32;
                let file_count = index.file_ids.get(gid as usize).map_or(1, Vec::len);
                let selected = sel.group == Some(gid);
                if group_row(ui, selected, gid, file_count).clicked() {
                    sel.group = Some(gid);
                    sel.file_id = if file_count > 1 {
                        index.file_ids[gid as usize].first().copied()
                    } else {
                        None
                    };
                }
            }
        });
}

/// Bottom-left: list of sub-files in the currently-selected group. The caller only
/// shows this panel when the group has >1 file — see `has_multi_file_selection`.
pub fn draw_files(ui: &mut egui::Ui, cache: &Cache, sel: &mut Selection) {
    let (Some(archive), Some(gid)) = (sel.archive, sel.group) else { return };
    let index = cache.index(archive);
    let Some(file_ids) = index.file_ids.get(gid as usize) else { return };
    ui.label(
        egui::RichText::new(format!("files in {gid} · {}", file_ids.len()))
            .weak()
            .small(),
    );
    let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
    egui::ScrollArea::vertical()
        .id_salt("col_files")
        .auto_shrink([false, false])
        .show_rows(ui, row_h, file_ids.len(), |ui, range| {
            for i in range {
                let fid = file_ids[i];
                let selected = sel.file_id == Some(fid);
                if file_row(ui, selected, fid).clicked() {
                    sel.file_id = Some(fid);
                }
            }
        });
}

/// True when the selected group has >1 file (so the bottom-left files panel is useful).
pub fn has_multi_file_selection(cache: &Cache, sel: &Selection) -> bool {
    let (Some(archive), Some(gid)) = (sel.archive, sel.group) else { return false };
    cache
        .index(archive)
        .file_ids
        .get(gid as usize)
        .is_some_and(|f| f.len() > 1)
}

/// Full-width row: `{id}   · N files`.
fn group_row(ui: &mut egui::Ui, selected: bool, gid: u32, file_count: usize) -> egui::Response {
    let label = if file_count > 1 {
        format!("{gid}    · {file_count} files")
    } else {
        format!("{gid}")
    };
    ui.add_sized(
        egui::vec2(ui.available_width(), 0.0),
        egui::SelectableLabel::new(selected, egui::RichText::new(label).monospace()),
    )
}

fn file_row(ui: &mut egui::Ui, selected: bool, fid: i32) -> egui::Response {
    ui.add_sized(
        egui::vec2(ui.available_width(), 0.0),
        egui::SelectableLabel::new(selected, egui::RichText::new(format!("{fid}")).monospace()),
    )
}

/// Coloured type chip — used by the details panel header.
pub fn chip(ui: &mut egui::Ui, label: &str, color: egui::Color32) {
    let text = egui::RichText::new(format!("[{label}]"))
        .monospace()
        .color(color);
    ui.label(text);
}
