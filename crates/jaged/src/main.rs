//! JagEd — OSRS rev1 cache/map editor.
//!
//! First-cut deliverable: open a window, load the cache, browse archives and groups,
//! show typed metadata for whichever group is selected. 3D scene rendering arrives in a
//! later session (will sit on top of a Pix3D port).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use cache::{ARCHIVE_NAMES, Cache};
use eframe::egui;

mod browser;
mod inspector;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_title("JagEd"),
        ..Default::default()
    };
    eframe::run_native("JagEd", options, Box::new(|cc| Ok(Box::new(JagEd::new(cc)))))
}

pub struct JagEd {
    /// Path the user typed into the cache-dir field. Defaults to `./cache` (relative to
    /// the editor's working dir).
    cache_path: String,
    cache: Option<Cache>,
    cache_status: Option<String>,
    selection: Selection,
}

/// Identifies what's currently selected in the browser. Two-level: archive + (group or
/// (group, file_id)). For multi-file groups (config types, interfaces, anims) the
/// inspector wants the file id too.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub archive: Option<u8>,
    pub group: Option<u32>,
    pub file_id: Option<i32>,
}

impl JagEd {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let default_path = PathBuf::from("cache");
        let mut me = Self {
            cache_path: default_path.display().to_string(),
            cache: None,
            cache_status: None,
            selection: Selection::default(),
        };
        me.open_cache();
        me
    }

    fn open_cache(&mut self) {
        let path = PathBuf::from(&self.cache_path);
        match Cache::open(&path) {
            Ok(c) => {
                self.cache_status = Some(format!(
                    "opened {} ({} master entries)",
                    path.display(),
                    cache::ARCHIVE_COUNT,
                ));
                self.cache = Some(c);
            }
            Err(e) => {
                self.cache_status = Some(format!("failed to open {}: {e}", path.display()));
                self.cache = None;
            }
        }
    }
}

impl eframe::App for JagEd {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("cache:");
                ui.add_sized([300.0, 0.0], egui::TextEdit::singleline(&mut self.cache_path));
                if ui.button("open").clicked() {
                    self.open_cache();
                    self.selection = Selection::default();
                }
                if let Some(s) = &self.cache_status {
                    ui.separator();
                    ui.label(s);
                }
            });
        });

        egui::SidePanel::left("browser").default_width(280.0).show(ctx, |ui| {
            ui.heading("browser");
            ui.separator();
            if let Some(cache) = &self.cache {
                browser::draw(ui, cache, &mut self.selection);
            } else {
                ui.label("no cache loaded.");
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(cache) = self.cache.as_mut() {
                inspector::draw(ui, cache, self.selection);
            } else {
                ui.label("open a cache to begin.");
            }
        });
    }
}

/// Pretty name for an archive — semantic name from `ARCHIVE_NAMES`, with master rendered
/// as "_master".
pub fn archive_label(archive: u8) -> &'static str {
    if archive == cache::MASTER_ARCHIVE {
        "_master"
    } else {
        ARCHIVE_NAMES[archive as usize]
    }
}
