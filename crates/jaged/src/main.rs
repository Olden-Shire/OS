//! JagEd — OSRS rev1 cache/map editor.
//!
//! First-cut deliverable: open a window, load the cache, browse archives and groups,
//! show typed metadata for whichever group is selected. 3D scene rendering arrives in a
//! later session (will sit on top of a Pix3D port).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use cache::{ARCHIVE_NAMES, Cache};
use eframe::egui;

mod archive_bar;
mod browser;
mod cs2_view;
mod details;
mod interface_view;
mod model_view;
mod music;
mod pix_bridge;
mod sprite_view;
mod typeinfo;
mod viewport;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 880.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("JagEd"),
        ..Default::default()
    };
    eframe::run_native(
        "JagEd",
        options,
        Box::new(|cc| {
            install_visuals(&cc.egui_ctx);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(JagEd::new(cc)))
        }),
    )
}

/// Lightly customise egui's dark theme — tighter spacing, rounder selectables,
/// monospace number alignment.
fn install_visuals(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals.widgets.hovered.corner_radius = 3.0.into();
    style.visuals.widgets.active.corner_radius = 3.0.into();
    style.visuals.widgets.inactive.corner_radius = 3.0.into();
    style.visuals.selection.bg_fill = egui::Color32::from_rgb(60, 105, 160);
    style.spacing.item_spacing = egui::vec2(8.0, 4.0);
    style.spacing.interact_size.y = 22.0;
    ctx.set_style(style);
}

pub struct JagEd {
    /// Path the user typed into the cache-dir field. Defaults to `./cache` (relative to
    /// the editor's working dir).
    cache_path: String,
    cache: Option<Cache>,
    cache_status: Option<String>,
    selection: Selection,
    /// Lazy-initialised on first Play click. Owns the cpal stream.
    pub player: Option<synth::Player>,
    pub player_error: Option<String>,
    /// Persistent orbit camera for the model viewer.
    pub model_view: model_view::ModelView,
    /// CS2 disassembler view-mode preference (raw / labeled).
    pub cs2_view: cs2_view::Cs2View,
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
            player: None,
            player_error: None,
            model_view: model_view::ModelView::default(),
            cs2_view: cs2_view::Cs2View::default(),
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
        // Top: cache path + status. Bottom: archive card grid. Then left (groups, then
        // optional files panel). Then right (details). Center fills the remainder with
        // the viewport. Panel order matters in egui — top/bottom are claimed first,
        // then left/right, then central.
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

        egui::TopBottomPanel::bottom("archives")
            .exact_height(archive_bar::panel_height())
            .show(ctx, |ui| {
                if let Some(cache) = &self.cache {
                    archive_bar::draw(ui, cache, &mut self.selection);
                } else {
                    ui.label(egui::RichText::new("(no cache loaded)").weak().italics());
                }
            });

        // Left column: groups on top, files on bottom — files panel collapses entirely
        // when the selected group is single-file (cleaner for models/sprites).
        let show_files = self
            .cache
            .as_ref()
            .is_some_and(|c| browser::has_multi_file_selection(c, &self.selection));

        egui::SidePanel::left("left_column")
            .default_width(260.0)
            .min_width(200.0)
            .resizable(true)
            .show(ctx, |ui| {
                let Some(cache) = self.cache.as_ref() else {
                    ui.label("no cache loaded.");
                    return;
                };
                if show_files {
                    // Split the left column vertically: top half groups, bottom half files.
                    let total = ui.available_height();
                    let groups_h = (total * 0.6).max(120.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), groups_h),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| browser::draw_groups(ui, cache, &mut self.selection),
                    );
                    ui.separator();
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), ui.available_height()),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| browser::draw_files(ui, cache, &mut self.selection),
                    );
                } else {
                    browser::draw_groups(ui, cache, &mut self.selection);
                }
            });

        egui::SidePanel::right("details")
            .default_width(340.0)
            .min_width(240.0)
            .resizable(true)
            .show(ctx, |ui| {
                if let Some(cache) = self.cache.as_mut() {
                    details::draw(ui, cache, &mut self.selection);
                } else {
                    ui.label(egui::RichText::new("(no cache loaded)").weak().italics());
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(cache) = self.cache.as_mut() {
                viewport::draw(
                    ui,
                    cache,
                    &mut self.selection,
                    &mut self.player,
                    &mut self.player_error,
                    &mut self.model_view,
                    &mut self.cs2_view,
                );
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space(60.0);
                    ui.label("open a cache to begin.");
                });
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
