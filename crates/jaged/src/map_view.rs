//! Map editor. Loads a region through the byte-exact .jm2 codec pair
//! (RawRegion for editing/save fidelity, decoded Region for display)
//! and offers two synced views:
//!   • 2D — top-down tile map coloured by the CLIENT flo/flu configs,
//!     the editing surface (paint underlay/overlay, raise/lower height).
//!   • 3D — the REAL client scene pipeline (load_ground → finish_build
//!     → world.render_all) with a free orbit camera, so the region is
//!     previewed exactly as in-game.
//! Both read the same edited RawRegion; the 3D world rebuilds on edit.

use cache::maps::text::RawRegion;
use cache::maps::{Region, XteaKeys};
use cache::Cache;
use client::dash3d::world::World;
use client::dash3d::pix3d;
use client::graphics::pix2d;
use eframe::egui;

const LEVELS: usize = 4;
const SIZE: usize = 64;

/// Resolve the XTEA `keys.json` used to decrypt map groups. The keys are an
/// input that lives with the read-only vanilla cache, NOT inside Content (they
/// aren't packed cache content). So when jaged opens a raw cache dir the file
/// sits beside it; when it opens a Content tree (packed to a temp cache) it
/// doesn't — fall back to the conventional `./cache/keys.json`. Empty keys on
/// failure (regions then read as not-decodable rather than crashing).
fn load_xtea_keys(cache_path: &str) -> XteaKeys {
    let candidates = [
        std::path::Path::new(cache_path).join("keys.json"),
        std::path::Path::new("cache").join("keys.json"),
    ];
    for path in candidates {
        if path.exists()
            && let Ok(keys) = XteaKeys::load(&path)
        {
            return keys;
        }
    }
    XteaKeys { by_mapsquare: Default::default() }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Inspect,
    PaintUnderlay,
    PaintOverlay,
    RaiseHeight,
    LowerHeight,
    ClearOverlay,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Map2D,
    Scene3D,
}

pub struct MapView {
    pub region_x: u32,
    pub region_y: u32,
    /// Editing model — byte-exact (.jm2) representation.
    pub raw: Option<RawRegion>,
    /// Display model — decoded with perlin-filled heights.
    pub decoded: Option<Region>,
    pub loaded_coords: Option<(u32, u32)>,
    pub level: usize,
    pub mode: Mode,
    pub tool: Tool,
    /// Floor id used by the paint tools (FluType for underlay, FloType
    /// for overlay) — the +1-encoded id, matching the map format.
    pub brush: u8,
    pub dirty: bool,
    pub status: Option<String>,
    pub show_locs: bool,
    pub show_flags: bool,
    // ── 3D scene state ──────────────────────────────────────────────
    /// The built client World for the current region (None until the
    /// 3D view is first shown / after an edit invalidates it).
    world: Option<World>,
    world_pivot_y: i32,
    /// Region the cached `world` was built for (rebuild on mismatch).
    world_for: Option<(u32, u32)>,
    cam_yaw: i32,
    cam_pitch: i32,
    cam_dist: i32,
}

impl Default for MapView {
    fn default() -> Self {
        Self {
            region_x: 50,
            region_y: 50,
            raw: None,
            decoded: None,
            loaded_coords: None,
            level: 0,
            mode: Mode::Map2D,
            tool: Tool::Inspect,
            brush: 1,
            dirty: false,
            status: None,
            show_locs: true,
            show_flags: false,
            world: None,
            world_pivot_y: 0,
            world_for: None,
            cam_yaw: 0,
            cam_pitch: 280,
            cam_dist: 2400,
        }
    }
}

impl MapView {
    /// Sync to the region the browser selected. Loads it (and drops the
    /// stale 3D world) only when the coords actually change, so the
    /// per-frame viewport call is cheap.
    pub fn set_region(&mut self, rx: u32, ry: u32, cache: &mut Cache, cache_path: &str) {
        if self.loaded_coords == Some((rx, ry)) {
            return;
        }
        self.region_x = rx;
        self.region_y = ry;
        self.load(cache, cache_path);
    }

    fn load(&mut self, cache: &mut Cache, cache_path: &str) {
        let (rx, ry) = (self.region_x, self.region_y);

        // Content stores maps as decoded, decrypted `.jm2` text — read that
        // DIRECTLY. No cache round-trip, no XTEA keys (keys only matter for the
        // raw encrypted cache, or when re-packing/verifying CRCs). The `.jm2`
        // round-trip is byte-exact, so the display Region built from the
        // re-encoded halves is identical to one decoded straight from binary.
        let jm2 = std::path::Path::new(cache_path)
            .join("maps")
            .join(format!("{rx}_{ry}.jm2"));
        if jm2.exists() {
            match std::fs::read_to_string(&jm2)
                .map_err(|e| e.to_string())
                .and_then(|t| RawRegion::from_text(&t))
            {
                Ok(raw) => {
                    let land = raw.encode_land();
                    let locs = raw.encode_locs();
                    self.decoded = Some(Region::decode(rx, ry, &land, Some(&locs)));
                    self.raw = Some(raw);
                    self.loaded_coords = Some((rx, ry));
                    self.dirty = false;
                    self.world = None;
                    self.world_for = None;
                    self.status = Some(format!(
                        "loaded m{rx}_{ry} from {} ({} locs)",
                        jm2.display(),
                        self.decoded.as_ref().map_or(0, |r| r.locs.len())
                    ));
                }
                Err(e) => self.status = Some(format!("parse {} failed: {e}", jm2.display())),
            }
            return;
        }

        // Raw cache source (no `.jm2`): decrypt + decode the encrypted map
        // group. With the wrong/empty key the decrypted bytes aren't a valid
        // compression container and region_raw panics deep in gzip — guard it
        // so a bad region surfaces as a status, not a crash.
        let keys = load_xtea_keys(cache_path);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cache.region_raw(rx, ry, &keys)
        }));
        let result = match result {
            Ok(r) => r,
            Err(_) => {
                self.status = Some(format!(
                    "failed to decode m{rx}_{ry} — missing/incorrect XTEA keys (keys.json)"
                ));
                return;
            }
        };
        match result {
            Ok(Some((land, locs))) => {
                self.raw = Some(RawRegion::decode(&land, locs.as_deref()));
                self.decoded = Some(Region::decode(
                    self.region_x,
                    self.region_y,
                    &land,
                    locs.as_deref(),
                ));
                self.loaded_coords = Some((self.region_x, self.region_y));
                self.dirty = false;
                self.world = None;
                self.world_for = None;
                self.status = Some(format!(
                    "loaded m{}_{} ({} locs)",
                    self.region_x,
                    self.region_y,
                    self.decoded.as_ref().map_or(0, |r| r.locs.len())
                ));
            }
            Ok(None) => {
                self.status = Some(format!(
                    "region m{}_{} not in cache",
                    self.region_x, self.region_y
                ));
            }
            Err(e) => self.status = Some(format!("read failed: {e}")),
        }
    }

    /// Re-derive the display Region from the edited RawRegion via the
    /// byte-exact encoders — guarantees the editor view always shows
    /// exactly what a save would produce. Invalidates the 3D world.
    fn refresh_decoded(&mut self) {
        if let (Some(raw), Some((x, y))) = (self.raw.as_ref(), self.loaded_coords) {
            let land = raw.encode_land();
            let locs = raw.encode_locs();
            self.decoded = Some(Region::decode(x, y, &land, Some(&locs)));
            self.dirty = true;
            self.world = None;
            self.world_for = None;
        }
    }

    /// Build (or reuse) the client scene World for the current edited
    /// region. Drives the same load_ground/finish_build path as the game.
    fn ensure_world(&mut self) {
        let Some((x, y)) = self.loaded_coords else { return };
        if self.world.is_some() && self.world_for == Some((x, y)) {
            return;
        }
        let Some(raw) = self.raw.as_ref() else { return };
        let land = raw.encode_land();
        let locs = raw.encode_locs();
        // load_locations resolves LocType models from the bridged
        // loaders; an incomplete cache could panic mid-build, so isolate.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::client_bridge::build_region_world(x, y, &land, Some(&locs))
        })) {
            Ok((world, pivot_y)) => {
                self.world = Some(world);
                self.world_pivot_y = pivot_y;
                self.world_for = Some((x, y));
            }
            Err(_) => {
                self.status = Some("3D build panicked (incomplete cache?)".into());
                self.world_for = Some((x, y)); // don't retry every frame
            }
        }
    }

    fn export_jm2(&mut self) {
        let (Some(raw), Some((x, y))) = (self.raw.as_ref(), self.loaded_coords) else {
            return;
        };
        let dir = std::path::Path::new("jm2_out");
        let _ = std::fs::create_dir_all(dir);
        let path = dir.join(format!("{x}_{y}.jm2"));
        match std::fs::write(&path, raw.to_text()) {
            Ok(()) => self.status = Some(format!("exported {}", path.display())),
            Err(e) => self.status = Some(format!("export failed: {e}")),
        }
    }
}

pub fn draw(ui: &mut egui::Ui, _cache: &mut Cache, _cache_path: &str, state: &mut MapView) {
    // ── Toolbar ──────────────────────────────────────────────────────
    ui.horizontal(|ui| {
        let (rx, ry) = state.loaded_coords.unwrap_or((state.region_x, state.region_y));
        ui.label(egui::RichText::new(format!("region {rx}_{ry}")).strong());
        ui.separator();
        ui.selectable_value(&mut state.mode, Mode::Map2D, "2D map");
        ui.selectable_value(&mut state.mode, Mode::Scene3D, "3D scene");
        ui.separator();
        ui.label("level");
        for l in 0..LEVELS {
            ui.selectable_value(&mut state.level, l, format!("{l}"));
        }
        ui.separator();
        ui.checkbox(&mut state.show_locs, "locs");
        if state.mode == Mode::Map2D {
            ui.checkbox(&mut state.show_flags, "flags");
        }
        ui.separator();
        if ui
            .add_enabled(state.dirty, egui::Button::new("export .jm2"))
            .clicked()
        {
            state.export_jm2();
        }
        if !state.dirty && ui.button("export .jm2 (clean)").clicked() {
            state.export_jm2();
        }
    });
    if state.mode == Mode::Map2D {
        ui.horizontal(|ui| {
            ui.label("tool");
            ui.selectable_value(&mut state.tool, Tool::Inspect, "inspect");
            ui.selectable_value(&mut state.tool, Tool::PaintUnderlay, "underlay");
            ui.selectable_value(&mut state.tool, Tool::PaintOverlay, "overlay");
            ui.selectable_value(&mut state.tool, Tool::ClearOverlay, "clear overlay");
            ui.selectable_value(&mut state.tool, Tool::RaiseHeight, "raise");
            ui.selectable_value(&mut state.tool, Tool::LowerHeight, "lower");
            if matches!(state.tool, Tool::PaintUnderlay | Tool::PaintOverlay) {
                ui.label("floor id");
                let mut b = state.brush as i32;
                ui.add(egui::DragValue::new(&mut b).range(1..=255));
                state.brush = b as u8;
                // Swatch from the client config the paint will resolve to.
                let colour = if state.tool == Tool::PaintUnderlay {
                    client::config::flu_type::list(b - 1).colour
                } else {
                    client::config::flo_type::list(b - 1).colour
                };
                let (r, g, bl) = (
                    ((colour >> 16) & 0xFF) as u8,
                    ((colour >> 8) & 0xFF) as u8,
                    (colour & 0xFF) as u8,
                );
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(18.0, 14.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, 2.0, egui::Color32::from_rgb(r, g, bl));
            }
        });
    } else {
        ui.label(
            egui::RichText::new("drag = orbit · scroll = zoom · client scene render")
                .weak()
                .small(),
        );
    }
    if let Some(s) = &state.status {
        ui.label(egui::RichText::new(s).weak().small());
    }

    if state.loaded_coords.is_none() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label("select a region from the maps browser on the left");
        });
        return;
    }

    if state.mode == Mode::Scene3D {
        draw_scene_3d(ui, state);
        return;
    }

    // ── Tile canvas ──────────────────────────────────────────────────
    let avail = ui.available_size_before_wrap();
    let side = avail.x.min(avail.y).max(64.0);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(side, side),
        egui::Sense::click_and_drag(),
    );
    let tile_px = side / SIZE as f32;

    // Build the 64x64 image from the decoded region.
    let img = build_tile_image(state);
    let tex = ui.ctx().load_texture(
        "map_view_tiles",
        img,
        egui::TextureOptions::NEAREST,
    );
    ui.painter_at(rect).image(
        tex.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );

    // Loc markers.
    if state.show_locs {
        if let Some(dec) = state.decoded.as_ref() {
            let painter = ui.painter_at(rect);
            for loc in &dec.locs {
                if loc.level as usize != state.level {
                    continue;
                }
                let cx = rect.min.x + (loc.x as f32 + 0.5) * tile_px;
                // North up: z=63 at top.
                let cy = rect.min.y + ((SIZE - 1 - loc.z as usize) as f32 + 0.5) * tile_px;
                painter.circle_filled(
                    egui::pos2(cx, cy),
                    (tile_px * 0.22).max(1.5),
                    egui::Color32::from_rgb(255, 80, 60),
                );
            }
        }
    }

    // ── Interaction ──────────────────────────────────────────────────
    let hover_tile = response.hover_pos().map(|p| {
        let tx = (((p.x - rect.min.x) / tile_px) as usize).min(SIZE - 1);
        let tz = SIZE - 1 - (((p.y - rect.min.y) / tile_px) as usize).min(SIZE - 1);
        (tx, tz)
    });

    if let Some((tx, tz)) = hover_tile {
        let apply = response.clicked() || response.dragged();
        if apply && state.tool != Tool::Inspect {
            if let Some(raw) = state.raw.as_mut() {
                let t = &mut raw.tiles[state.level][tx][tz];
                match state.tool {
                    Tool::PaintUnderlay => t.underlay = Some(state.brush),
                    Tool::PaintOverlay => match t.overlay.as_mut() {
                        Some(ov) => ov.id = state.brush,
                        None => {
                            t.overlay = Some(cache::maps::text::RawOverlay {
                                id: state.brush,
                                shape: 0,
                                rotation: 0,
                            })
                        }
                    },
                    Tool::ClearOverlay => t.overlay = None,
                    Tool::RaiseHeight | Tool::LowerHeight => {
                        // Heights are negative-up in Jagex units; raise
                        // = more negative. Start from the decoded
                        // (perlin-filled) value when unset.
                        let base = t.height.unwrap_or_else(|| {
                            state
                                .decoded
                                .as_ref()
                                .map_or(0, |d| d.tiles[state.level][tx][tz].height)
                        });
                        let delta = if state.tool == Tool::RaiseHeight { -8 } else { 8 };
                        t.height = Some(base + delta);
                    }
                    Tool::Inspect => {}
                }
                state.refresh_decoded();
            }
        }

        // Inspector line under the canvas.
        if let (Some(dec), Some(raw)) = (state.decoded.as_ref(), state.raw.as_ref()) {
            let t = &dec.tiles[state.level][tx][tz];
            let rt = &raw.tiles[state.level][tx][tz];
            let locs_here = dec
                .locs
                .iter()
                .filter(|l| {
                    l.level as usize == state.level && l.x as usize == tx && l.z as usize == tz
                })
                .count();
            ui.label(
                egui::RichText::new(format!(
                    "tile ({tx}, {tz})  h={}{}  underlay={}  overlay={} (shape {} rot {})  flags={:#04x}  locs={locs_here}",
                    t.height,
                    if rt.height.is_some() { "" } else { " (perlin)" },
                    t.underlay,
                    t.overlay,
                    t.overlay_shape,
                    t.overlay_rotation,
                    t.mapflags,
                ))
                .monospace()
                .small(),
            );
        }
    }
}

/// 3D scene preview — drives the real client world render with a free
/// orbit camera around the region centre (tile 32, 32). Builds the
/// World lazily (and after any edit) via the same load_ground →
/// finish_build path the game uses.
fn draw_scene_3d(ui: &mut egui::Ui, state: &mut MapView) {
    state.ensure_world();
    if state.world.is_none() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| ui.label("building scene…"));
        return;
    }

    let avail = ui.available_size_before_wrap();
    if avail.x < 16.0 || avail.y < 16.0 {
        return;
    }
    let (rect, response) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());

    if response.dragged() {
        let d = response.drag_delta();
        state.cam_yaw = (state.cam_yaw - (d.x * 4.0) as i32).rem_euclid(2048);
        state.cam_pitch = (state.cam_pitch + (d.y * 3.0) as i32).clamp(128, 383);
    }
    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
    if scroll != 0.0 && response.hovered() {
        state.cam_dist = (state.cam_dist - (scroll * 4.0) as i32).clamp(600, 8000);
    }

    let ppp = ui.ctx().pixels_per_point();
    let w = (rect.width() * ppp) as i32;
    let h = (rect.height() * ppp) as i32;
    if w < 4 || h < 4 {
        return;
    }

    let yaw = state.cam_yaw & 0x7FF;
    let pitch = state.cam_pitch.clamp(128, 383);
    let dist = state.cam_dist;
    let pivot_y = state.world_pivot_y;
    let top_level = state.level as i32;
    let world = state.world.as_mut().unwrap();

    // Orbit the region centre (loaded centred in the 104² world),
    // mirroring scene::draw_viewport's camera math.
    let sin_t = pix3d::sin_table();
    let cos_t = pix3d::cos_table();
    let pivot_tile = crate::client_bridge::REGION_PIVOT_TILE;
    let pivot_x = pivot_tile * 128 + 64;
    let pivot_z = pivot_tile * 128 + 64;
    let h_radius = (dist * cos_t[pitch as usize]) >> 16;
    let cam_x = pivot_x + ((h_radius * sin_t[yaw as usize]) >> 16);
    let cam_y = pivot_y - ((dist * sin_t[pitch as usize]) >> 16);
    let cam_z = pivot_z - ((h_radius * cos_t[yaw as usize]) >> 16);

    // Render into a scratch frame bound on the client's global Pix2D
    // (single-threaded here), then swap the real buffer back. Extended
    // draw disables the in-game 25-tile elision + visibility gate so the
    // whole region renders regardless of camera distance.
    client::debug_opts::set_extended_draw(true);
    let (prev, pw, ph) = pix2d::swap_pixels(vec![0x00404048; (w * h) as usize], w, h);
    pix3d::set_clipping(0, 0, w, h);
    pix3d::set_trans(0);
    pix3d::set_model_far_clip(16000);
    let rendered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        world.render_all(cam_x, cam_y, cam_z, pitch, yaw, top_level);
        world.remove_sprites();
    }));
    let (frame, _, _) = pix2d::swap_pixels(prev, pw, ph);
    if rendered.is_err() {
        ui.colored_label(egui::Color32::LIGHT_RED, "scene render panicked");
        return;
    }

    let tex = crate::pix_bridge::upload_rgb(
        ui.ctx(),
        "map_scene_3d",
        &frame,
        w as usize,
        h as usize,
    );
    ui.painter_at(rect).image(
        tex.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

/// 64x64 top-down tile colours: overlay > underlay > black, shaded by
/// height slope, with optional flag tinting. North (z=63) renders at
/// the top row, matching the in-game map orientation.
fn build_tile_image(state: &MapView) -> egui::ColorImage {
    let mut px = vec![egui::Color32::BLACK; SIZE * SIZE];
    let Some(dec) = state.decoded.as_ref() else {
        return egui::ColorImage { size: [SIZE, SIZE], pixels: px };
    };
    let level = state.level;
    for z in 0..SIZE {
        for x in 0..SIZE {
            let t = &dec.tiles[level][x][z];
            let mut colour = if t.overlay > 0 {
                client::config::flo_type::list(t.overlay as i32 - 1).colour
            } else if t.underlay > 0 {
                client::config::flu_type::list(t.underlay as i32 - 1).colour
            } else {
                0x000000
            };
            // Slope shading from the east/north height deltas (cheap
            // light-from-northeast like the in-game minimap feel).
            if colour != 0 && x + 1 < SIZE && z + 1 < SIZE {
                let h = t.height;
                let he = dec.tiles[level][x + 1][z].height;
                let hn = dec.tiles[level][x][z + 1].height;
                let slope = ((h - he) + (h - hn)).clamp(-96, 96);
                let scale = 256 + slope;
                let r = (((colour >> 16) & 0xFF) * scale / 256).min(255);
                let g = (((colour >> 8) & 0xFF) * scale / 256).min(255);
                let b = ((colour & 0xFF) * scale / 256).min(255);
                colour = (r << 16) | (g << 8) | b;
            }
            if state.show_flags && t.mapflags != 0 {
                // Tint flagged tiles: blocked = red-ish, bridge = blue-ish.
                if t.mapflags & 0x1 != 0 {
                    colour = (colour & 0x00FFFF) | 0xC00000;
                }
                if t.mapflags & 0x2 != 0 {
                    colour = (colour & 0xFFFF00) | 0x0000C0;
                }
            }
            // z up = row 0 at top.
            px[(SIZE - 1 - z) * SIZE + x] = egui::Color32::from_rgb(
                ((colour >> 16) & 0xFF) as u8,
                ((colour >> 8) & 0xFF) as u8,
                (colour & 0xFF) as u8,
            );
        }
    }
    egui::ColorImage { size: [SIZE, SIZE], pixels: px }
}
