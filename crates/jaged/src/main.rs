//! JagEd — OSRS rev1 cache/map editor.
//!
//! First-cut deliverable: open a window, load the cache, browse archives and groups,
//! show typed metadata for whichever group is selected. 3D scene rendering arrives in a
//! later session (will sit on top of a Pix3D port).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

use cache::{ARCHIVE_NAMES, Cache};
use eframe::egui;

mod archive_bar;
mod client_bridge;
mod browser;
mod cs2_view;
mod details;
mod interface_view;
mod jagfx_view;
mod map_view;
mod model_view;
mod music;
mod pix_bridge;
mod sprite_view;
mod theme;
mod typeinfo;
mod viewport;
mod vorbis_view;

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
            theme::install(&cc.egui_ctx);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(JagEd::new(cc)))
        }),
    )
}

/// Which layout the central content uses for a given archive. `Inspector`
/// fills the centre with the typed field tables (configs / anim data /
/// raw); `Viewport` fills it with the self-contained visual or media view
/// (models, sprites, interfaces, maps, audio, scripts). No archive uses a
/// cramped right panel anymore — each view owns the full centre.
#[derive(Clone, Copy, PartialEq)]
enum Layout {
    Inspector,
    Viewport,
}

fn layout_for(archive: u8) -> Layout {
    match archive {
        // interfaces, jagfx, maps, songs, models, sprites, binary,
        // jingles, clientscripts, vorbis — self-contained centre views.
        3 | 4 | 5 | 6 | 7 | 8 | 10 | 11 | 12 | 14 => Layout::Viewport,
        // config, anims, bases, textures, fonts, patches — data tables.
        _ => Layout::Inspector,
    }
}

pub struct JagEd {
    /// Path the user typed into the cache-dir field. Defaults to `./cache` (relative to
    /// the editor's working dir).
    cache_path: String,
    cache: Option<Cache>,
    cache_status: Option<String>,
    selection: Selection,
    /// Client subsystems, booted in full at cache-open like the game's
    /// startup (loaders → config installs → audio). Views assume ready.
    pub client_sys: Option<client_bridge::ClientSystems>,
    pub player_error: Option<String>,
    /// Game-loop cycle counter (drives texture anims like loopCycle).
    pub cycle: i32,
    /// Wall-clock origin for the 50Hz game clock.
    pub anim_start: std::time::Instant,
    /// Persistent orbit camera for the model viewer.
    pub model_view: model_view::ModelView,
    /// Map editor state (region, tool, edits).
    pub map_view: map_view::MapView,
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
        // Content is the source of truth — jaged is the hub for renaming pack
        // members, so it reads from Content, not the read-only vanilla cache.
        // An argv override (`jaged <path>`) opens any cache/Content dir.
        let default_path = std::env::args()
            .nth(1)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("Content"));
        let mut me = Self {
            cache_path: default_path.display().to_string(),
            cache: None,
            cache_status: None,
            selection: Selection::default(),
            client_sys: None,
            player_error: None,
            cycle: 0,
            anim_start: std::time::Instant::now(),
            model_view: model_view::ModelView::default(),
            map_view: map_view::MapView::default(),
            cs2_view: cs2_view::Cs2View::default(),
        };
        me.open_cache();
        me
    }

    fn open_cache(&mut self) {
        let path = PathBuf::from(&self.cache_path);
        match Self::resolve_cache(&path) {
            Ok((source, mut c)) => {
                // Boot the client subsystems in full — loaders, config
                // installs, textures, anims, audio — exactly once, the
                // way the game's startup does. Every view can assume
                // the client crate is ready after this.
                let sys = client_bridge::init(&mut c);
                self.player_error = sys.audio_error.clone();
                self.cache_status = Some(format!(
                    "opened {source} ({} master entries, {} client loaders)",
                    cache::ARCHIVE_COUNT,
                    sys.loaders_installed,
                ));
                self.client_sys = Some(sys);
                self.cache = Some(c);
            }
            Err(e) => {
                self.cache_status = Some(format!("failed to open {}: {e}", path.display()));
                self.cache = None;
                self.client_sys = None;
            }
        }
    }

    /// Resolve a source path to an openable binary cache, returning a human
    /// label for the status line plus the opened cache. A directory that
    /// already holds `main_file_cache.dat2` is opened directly; otherwise it's
    /// treated as a Content tree (the editable source of truth) and packed into
    /// a temp cache — so jaged always reflects Content, not the vanilla cache.
    fn resolve_cache(src: &Path) -> std::io::Result<(String, Cache)> {
        if src.join("main_file_cache.dat2").exists() {
            let c = Cache::open(src)?;
            return Ok((src.display().to_string(), c));
        }
        let gen_dir = std::env::temp_dir().join("os1_jaged_cache");
        cache::content::pack::pack(src, &gen_dir)?;
        let c = Cache::open(&gen_dir)?;
        Ok((format!("{} (packed → {})", src.display(), gen_dir.display()), c))
    }
}

impl eframe::App for JagEd {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // The jaged game loop — always running, like the client
        // mainloop: texture anims + the midi2 fade/load state machine.
        // Drive the clock off WALL-CLOCK time (20ms = one game tick) so
        // animation runs at the true 50Hz regardless of jaged's repaint
        // rate — incrementing once per frame ran slow whenever egui
        // repainted below 50fps. `delta` is the ticks elapsed this frame
        // (Java's worldUpdateNum); `cycle` is the monotonic game clock.
        if let Some(sys) = self.client_sys.as_ref() {
            let elapsed_ms = self.anim_start.elapsed().as_millis() as i64;
            let cycle = (elapsed_ms / 20) as i32;
            let delta = (cycle - self.cycle).max(0);
            self.cycle = cycle;
            client_bridge::tick(sys, cycle, delta);
            ctx.request_repaint_after(std::time::Duration::from_millis(20));
        }

        // Layout: a slim title bar, a fixed archive rail (navigation), a
        // resizable group/file browser, then a centre that ADAPTS to the
        // selected archive — data tables for configs, a full-bleed canvas
        // for models/maps/interfaces, a centred card for audio. No bottom
        // grid, no cramped right panel; each view owns the whole centre.
        title_bar(ctx, self);

        egui::SidePanel::left("rail")
            .exact_width(168.0)
            .resizable(false)
            .frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin {
                left: 8, right: 8, top: 0, bottom: 8,
            }))
            .show(ctx, |ui| {
                if let Some(cache) = &self.cache {
                    archive_bar::draw(ui, cache, &mut self.selection);
                } else {
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new("no cache").weak().italics());
                }
            });

        // Group / file browser — only meaningful with an archive selected.
        if self.selection.archive.is_some() {
            let show_files = self
                .cache
                .as_ref()
                .is_some_and(|c| browser::has_multi_file_selection(c, &self.selection));
            egui::SidePanel::left("browser")
                .default_width(232.0)
                .min_width(180.0)
                .resizable(true)
                .show(ctx, |ui| {
                    let Some(cache) = self.cache.as_ref() else { return };
                    if show_files {
                        let groups_h = (ui.available_height() * 0.6).max(120.0);
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
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(cache) = self.cache.as_mut() else {
                ui.vertical_centered(|ui| {
                    ui.add_space(80.0);
                    ui.label(egui::RichText::new("open a cache to begin").weak().size(15.0));
                });
                return;
            };
            let Some(archive) = self.selection.archive else {
                ui.vertical_centered(|ui| {
                    ui.add_space(80.0);
                    ui.label(egui::RichText::new("select an archive").weak().size(15.0));
                });
                return;
            };

            match layout_for(archive) {
                Layout::Inspector => {
                    details::draw(ui, cache, &mut self.selection);
                }
                Layout::Viewport => {
                    viewport::draw(
                        ui,
                        cache,
                        &mut self.selection,
                        self.client_sys.as_mut(),
                        &mut self.player_error,
                        &mut self.model_view,
                        &mut self.cs2_view,
                        &mut self.map_view,
                        &self.cache_path,
                    );
                }
            }
        });
    }
}

/// Slim top bar: app mark + cache path/open + status. Replaces the old
/// bare "cache:" row with something that reads as a title bar.
fn title_bar(ctx: &egui::Context, app: &mut JagEd) {
    egui::TopBottomPanel::top("title")
        .frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(10, 6)))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("JagEd").strong().size(15.0).color(theme::ACCENT_TEXT));
                ui.add_space(2.0);
                ui.label(egui::RichText::new("cache editor").weak().small());
                ui.separator();
                ui.add(egui::TextEdit::singleline(&mut app.cache_path).desired_width(240.0));
                if ui.button("open").clicked() {
                    app.open_cache();
                    app.selection = Selection::default();
                }
                if let Some(s) = &app.cache_status {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(s).weak().small());
                }
            });
        });
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
