//! Left-pane browser tree. archive → group (→ file id, for multi-file groups).

use cache::{ARCHIVE_COUNT, Cache};
use eframe::egui;

use crate::{Selection, archive_label};

pub fn draw(ui: &mut egui::Ui, cache: &Cache, selection: &mut Selection) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        for archive in 0..ARCHIVE_COUNT {
            draw_archive(ui, cache, archive, selection);
        }
    });
}

fn draw_archive(ui: &mut egui::Ui, cache: &Cache, archive: u8, selection: &mut Selection) {
    let index = cache.index(archive);
    let header = egui::CollapsingHeader::new(format!(
        "{} ({} groups)",
        archive_label(archive),
        index.size,
    ))
    .id_salt(("arc", archive));

    let response = header.show_unindented(ui, |ui| {
        let group_ids = &index.group_ids;
        // Virtual scroll for archives with many groups (models has 27k).
        let row_h = ui.text_style_height(&egui::TextStyle::Body);
        egui::ScrollArea::vertical()
            .max_height(300.0)
            .show_rows(ui, row_h, group_ids.len(), |ui, range| {
                for i in range {
                    let gid = group_ids[i] as u32;
                    let file_count = index.file_ids.get(gid as usize).map_or(1, Vec::len);
                    let label = if file_count > 1 {
                        format!("{gid} ({file_count} files)")
                    } else {
                        format!("{gid}")
                    };
                    let selected = selection.archive == Some(archive)
                        && selection.group == Some(gid);
                    if ui.selectable_label(selected, label).clicked() {
                        selection.archive = Some(archive);
                        selection.group = Some(gid);
                        selection.file_id = if file_count > 1
                            && index.file_ids[gid as usize].len() > 0
                        {
                            Some(index.file_ids[gid as usize][0])
                        } else {
                            None
                        };
                    }
                }
            });
    });

    // If this archive is the currently-selected one and the group has > 1 file, also draw
    // a flat file-id picker right under the header so the inspector can target one record.
    if response.openness > 0.0
        && selection.archive == Some(archive)
        && let Some(gid) = selection.group
    {
        let file_ids = index.file_ids.get(gid as usize);
        if let Some(file_ids) = file_ids
            && file_ids.len() > 1
        {
            ui.label(format!("files in group {gid}:"));
            let row_h = ui.text_style_height(&egui::TextStyle::Body);
            egui::ScrollArea::vertical()
                .id_salt(("files", archive, gid))
                .max_height(200.0)
                .show_rows(ui, row_h, file_ids.len(), |ui, range| {
                    for i in range {
                        let fid = file_ids[i];
                        let selected = selection.file_id == Some(fid);
                        if ui.selectable_label(selected, format!("{fid}")).clicked() {
                            selection.file_id = Some(fid);
                        }
                    }
                });
        }
    }
}
