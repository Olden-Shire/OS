//! Live 3D "admin spectator" scene — renders the selected player's map region
//! through the REAL client scene pipeline (load_ground → finish_build →
//! world.render_all), the same path jaged's 3D map view uses, with a free orbit
//! camera. Toggleable so it costs nothing when off.
//!
//! Terrain + locs come from the region's decoded `.jm2` (the same map the server
//! serves); live entities (players/npcs) are overlaid from the tick snapshot.
//!
//! (This deliberately mirrors jaged's `client_bridge`/`pix_bridge`; once both
//! settle, the shared glue should move into the client crate to dedupe.)

use cache::maps::text::RawRegion;
use cache::Cache;
use client::dash3d::{pix3d, world::World};
use client::graphics::pix2d;
use eframe::egui;

/// Border centring the 64² region inside the 104² client world.
pub const REGION_BORDER: i32 = (104 - 64) / 2; // 20
/// Pixels per tile in the baked top-down map (matches the client minimap).
pub const MAP_PX_PER_TILE: usize = 4;
/// Build-area edge in tiles (the loaded square around a region).
pub const MAP_TILES: usize = 104;

/// World-tile coordinate of the baked map's south-west corner (image origin)
/// for region `(rx, ry)`.
pub fn map_origin(rx: u32, ry: u32) -> (i32, i32) {
    (rx as i32 * 64 - REGION_BORDER, ry as i32 * 64 - REGION_BORDER)
}

/// Client-loader archive slots (same scheme as the client's openJs5).
const ANIMS: i32 = 0;
const BASES: i32 = 1;
const CONFIG: i32 = 2;
const MODELS: i32 = 7;
const SPRITES: i32 = 8;
const TEXTURES: i32 = 9;

/// What kind of entity a marker is — drives which real model to render.
#[derive(Clone)]
pub enum MarkerKind {
    /// A player avatar: 12-slot worn appearance, 5 recolours, female flag,
    /// idle + walk stance seqs, and whether it's moving this tick.
    Player { worn: [i32; 12], colours: [i32; 5], female: bool, ready_anim: i32, walk_anim: i32, moving: bool },
    /// An NPC by type id (idle/walk anims come from its config), + moving flag.
    Npc { type_id: i32, moving: bool },
}

/// An entity rendered as a real model in the 3D scene + labelled in the overlay.
pub struct Marker {
    /// Stable id across ticks (player pid / namespaced npc nid) so the scene can
    /// keep a persistent [`Actor`] per entity and interpolate it between ticks.
    pub id: i32,
    pub x: i32,
    pub z: i32,
    pub color: egui::Color32,
    pub label: Option<String>,
    pub kind: MarkerKind,
}

/// Persistent client-entity for one tracked actor — the real `ClientEntity` the
/// game client uses, so `move_entity` (route interpolation + walk/turn anim
/// selection) drives smooth movement and animation between server ticks. Fed a
/// route step per tick via `teleport(tile, jump=false)`; advanced per 50Hz cycle.
struct Actor {
    entity: client::dash3d::ClientEntity,
    kind: MarkerKind,
    label: Option<String>,
    /// NPC `walksmoothing` (players: true) — passed to `move_entity`.
    smoothing: bool,
    /// Local build-area tile (0..103) it was last told to walk to, so we only
    /// feed a new route step when the target tile actually changes.
    target: (i32, i32),
}

/// 3D scene state held by the panel.
pub struct Scene {
    pub enabled: bool,
    installed: bool,
    content_dir: String,
    world: Option<World>,
    world_for: Option<(u32, u32)>,
    /// Per-tile ground heights [level][x][z] for the built region — so the
    /// camera pivot and entity markers sit on the actual terrain.
    heights: Vec<Vec<Vec<i32>>>,
    cam_yaw: i32,
    cam_pitch: i32,
    cam_dist: i32,
    status: Option<String>,
    /// Animated-loc/texture clock, advanced at ~50Hz wall-clock so torches,
    /// fires and water animate.
    anim_cycle: i32,
    /// Fractional carry of accumulated wall-clock cycles (a frame at 16ms is
    /// 0.8 of a 20ms cycle — integer truncation per frame dropped ~40% of the
    /// rate, which read as "animating too slow"). We accumulate and only step
    /// whole cycles, so the long-run rate is a true 50Hz regardless of fps.
    anim_accum: f64,
    last_anim: Option<std::time::Instant>,
    /// Last-frame diagnostic line (clock + focused entity anim state), painted
    /// over the scene so divergences are reportable without a debugger.
    dbg: String,
    /// Cached top-down minimap bake: (region_x, region_y, level) it was baked
    /// for, the RGB pixels, and the square edge in px.
    map_baked: Option<(u32, u32, i32)>,
    map_image: Vec<i32>,
    map_w: usize,
    /// Mapscene sprites (trees, stairs, …) for the map detail pass, loaded once.
    mapscene: Option<Vec<client::graphics::pix8::Pix8>>,
    /// Map-function icons (quest, bank, shop, altar, …) keyed by loc mapfunction.
    mapfunction: Option<Vec<client::graphics::pix32::Pix32>>,
    /// Persistent per-entity client-entities, keyed by marker id. Synced from the
    /// snapshot once per tick; advanced by `move_entity` every cycle for smooth
    /// movement + walk anims (real scene composition, not per-tick snapping).
    actors: std::collections::HashMap<i32, Actor>,
    /// The snapshot tick the actors were last synced from (feed routes once/tick).
    actors_tick: u32,
    /// The region the actors' local coords are anchored to; on change we reset
    /// them (local origin shifts when the camera follows into a new region).
    actors_region: Option<(u32, u32)>,
}

impl Default for Scene {
    fn default() -> Self {
        Scene {
            enabled: true,
            installed: false,
            content_dir: "Content".to_string(),
            world: None,
            world_for: None,
            heights: Vec::new(),
            cam_yaw: 0,
            cam_pitch: 280,
            cam_dist: 2400,
            status: None,
            anim_cycle: 0,
            anim_accum: 0.0,
            last_anim: None,
            dbg: String::new(),
            map_baked: None,
            map_image: Vec::new(),
            map_w: 0,
            mapscene: None,
            mapfunction: None,
            actors: std::collections::HashMap::new(),
            actors_tick: u32::MAX,
            actors_region: None,
        }
    }
}

impl Scene {
    /// Install the client loaders + scene config tables once (lazily, on first
    /// enable). Reads the vanilla cache for models/textures/configs.
    fn ensure_installed(&mut self) {
        if self.installed {
            return;
        }
        self.installed = true; // don't retry every frame on failure
        if let Err(e) = install_client() {
            self.status = Some(e);
        }
    }
}

/// Install the client loaders + scene config tables once, process-wide (used by
/// both the live scene and the world-map baker). Idempotent + thread-safe.
pub fn install_client() -> Result<(), String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    static DONE: AtomicBool = AtomicBool::new(false);
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = LOCK.lock().unwrap();
    if DONE.load(Ordering::Relaxed) {
        return Ok(());
    }
    let mut cache = Cache::open(std::path::Path::new("cache"))
        .map_err(|e| format!("open cache failed: {e}"))?;
    {
        install_local_loaders(&mut cache);
        // Scene-relevant config installs (terrain + locs + textures + anims).
        client::config::loc_type::install_archives(CONFIG, MODELS);
        client::config::flo_type::install_archives(CONFIG);
        client::config::flu_type::install_archives(CONFIG);
        client::config::npc_type::install_archives(CONFIG, MODELS);
        client::config::idk_type::install_archives(CONFIG, MODELS);
        client::dash3d::texture_manager::install_archives(TEXTURES, SPRITES);
        client::config::seq_type::install_archives(CONFIG, ANIMS, BASES);
        client::dash3d::anim_frame_set::install_archives(ANIMS, BASES);
        // Chat Huffman table (the "binary" archive's "huffman" group) so the
        // panel can decode public-chat WordPack bytes. Archive index varies, so
        // scan all installed archives for the named group.
        if !client::wordpack::huffman_loaded() {
            let mut reg = client::js5::js5_net::LOADERS.lock().unwrap();
            let mut found = false;
            for a in 0..reg.len() {
                if let Some(l) = reg.get_mut(a).and_then(|o| o.as_mut()) {
                    if let Some(table) = l.get_file_by_name("huffman", "") {
                        eprintln!("[panel] chat huffman table loaded from archive {a} ({} bytes)", table.len());
                        client::wordpack::set_huffman(client::wordpack::Huffman::new(&table));
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                eprintln!("[panel] chat huffman table NOT found in any archive");
            }
        }
    }
    DONE.store(true, Ordering::Relaxed);
    Ok(())
}

impl Scene {
    /// Build (or reuse) the client World for `(rx, ry)` from its `.jm2`.
    fn ensure_world(&mut self, rx: u32, ry: u32) {
        if self.world.is_some() && self.world_for == Some((rx, ry)) {
            return;
        }
        self.world = None;
        self.world_for = Some((rx, ry)); // mark attempted so we don't spin
        let path = std::path::Path::new(&self.content_dir)
            .join("maps")
            .join(format!("{rx}_{ry}.jm2"));
        let raw = match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| RawRegion::from_text(&t)) {
            Ok(r) => r,
            Err(_) => {
                self.status = Some(format!("no map for region {rx},{ry}"));
                return;
            }
        };
        let land = raw.encode_land();
        let locs = raw.encode_locs();
        let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            build_region_world(rx, ry, &land, Some(&locs))
        }));
        match built {
            Ok((world, heights)) => {
                self.world = Some(world);
                self.heights = heights;
                self.status = None;
            }
            Err(_) => self.status = Some("3D build panicked (incomplete cache?)".into()),
        }
    }

    /// Reset the orbit camera to the default angle/zoom.
    pub fn reset_camera(&mut self) {
        self.cam_yaw = 0;
        self.cam_pitch = 280;
        self.cam_dist = 2400;
    }

    /// Ground height at a region-local tile (bounds-checked).
    fn height_at(&self, level: i32, x: i32, z: i32) -> i32 {
        let l = level.clamp(0, 3) as usize;
        let (x, z) = (x.clamp(0, 103) as usize, z.clamp(0, 103) as usize);
        self.heights.get(l).and_then(|p| p.get(x)).and_then(|c| c.get(z)).copied().unwrap_or(0)
    }

    /// Load the mapscene sprites + mapfunction icons once (for the detail pass).
    fn load_map_sprites(&mut self) {
        if self.mapscene.is_some() && self.mapfunction.is_some() {
            return;
        }
        let (ms, mf) = load_map_sprites();
        self.mapscene = ms;
        self.mapfunction = mf;
    }

    /// Bake (or reuse) the focused region's detailed top-down image (the per-
    /// region map view). Covers world tiles `[rx*64-B .. rx*64-B+104)`.
    pub fn bake_map(&mut self, rx: u32, ry: u32, level: i32) -> Option<(&[i32], usize)> {
        self.ensure_installed();
        self.load_map_sprites();
        let level = level.clamp(0, 3);
        if self.map_baked != Some((rx, ry, level)) {
            let (img, w) = bake_region_detail(&self.content_dir, rx, ry, level,
                self.mapscene.as_deref(), self.mapfunction.as_deref())?;
            self.map_image = img;
            self.map_w = w;
            self.map_baked = Some((rx, ry, level));
        }
        Some((&self.map_image, self.map_w))
    }

    /// Advance the turntable clock when the 3D scene isn't already driving it
    /// (so inspector portraits rotate even with the scene off).
    fn tick_portrait_clock(&mut self) {
        if self.world.is_some() {
            return;
        }
        let now = std::time::Instant::now();
        let elapsed = self.last_anim.map_or(0.0, |t| now.saturating_duration_since(t).as_millis().min(200) as f64);
        self.last_anim = Some(now);
        self.anim_accum += elapsed / 20.0;
        let ticks = self.anim_accum as i32;
        if ticks > 0 {
            self.anim_accum -= ticks as f64;
            self.anim_cycle = self.anim_cycle.wrapping_add(ticks);
        }
    }

    /// Paint one lit model centred to an RGB portrait buffer (slow turntable),
    /// mirroring the client's interface model-preview path (`obj_render_icon`).
    fn paint_portrait(&self, model: &client::dash3d::model_lit::ModelLit, zoom: i32, w: i32, h: i32) -> Vec<i32> {
        let (prev, pw, ph) = pix2d::swap_pixels(vec![0x0010_1218; (w * h) as usize], w, h);
        pix2d::set_clipping(0, 0, w, h);
        pix3d::set_clipping(0, 0, w, h);
        // Centre the model vertically. Its points span py in [-min_y, max_y], so
        // the mid-height projects ~(max_y - min_y)*256/zoom px from the origin
        // (perspective: screen_y ≈ origin_y + py*512/vz2, with vz2 ≈ zoom). Using
        // the model's own bounds keeps feet-pivoted npc models from riding high.
        let mid = ((model.max_y - model.min_y) * 256) / zoom.max(1);
        pix3d::set_origin(w / 2, h / 2 - mid);
        let sin = pix3d::sin_table();
        let cos = pix3d::cos_table();
        let x_an = 150i32; // slight downward tilt (matches the client)
        let y_an = (self.anim_cycle * 6) & 0x7FF; // slow turntable
        let eye_y = (zoom * sin[(x_an & 0x7FF) as usize]) >> 16;
        let eye_z = (zoom * cos[(x_an & 0x7FF) as usize]) >> 16;
        model.obj_render_icon(0, y_an, 0, x_an, 0, eye_y, eye_z);
        pix3d::reset_origin();
        let (buf, _, _) = pix2d::swap_pixels(prev, pw, ph);
        buf
    }

    /// Render the selected player's actual character model to a portrait card.
    pub fn portrait(&mut self, worn: [i32; 12], colours: [i32; 5], female: bool,
                    anim: i32, w: i32, h: i32) -> Option<Vec<i32>> {
        self.ensure_installed();
        self.tick_portrait_clock();
        let clock = self.anim_cycle;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut pm = client::dash3d::player_model::PlayerModel::new();
            pm.apply_appearance(worn, colours, female, -1);
            let (seq, frame) = anim_frame(anim, clock);
            let mut model = pm.get_temp_model(seq.as_ref(), frame, None, -1)?;
            model.calc_bounding_cylinder(); // populate min_y/max_y for vertical centring
            // Camera further back (bigger zoom) so the full body fits the card.
            Some(self.paint_portrait(&model, 920, w, h))
        }))
        .ok()
        .flatten()
    }

    /// Render the selected NPC's model to a portrait card (zoom scaled by size).
    pub fn npc_portrait(&mut self, type_id: i32, w: i32, h: i32) -> Option<Vec<i32>> {
        self.ensure_installed();
        self.tick_portrait_clock();
        let clock = self.anim_cycle;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let nt = client::config::npc_type::list(type_id);
            let (seq, frame) = anim_frame(nt.readyanim, clock);
            let mut model = nt.get_temp_model(seq.as_ref(), frame, None, -1)?;
            model.calc_bounding_cylinder(); // populate min_y/max_y for vertical centring
            // Bigger NPCs sit further from the camera so they still fit the card.
            let zoom = 720 + (nt.size.max(1) - 1) * 420;
            Some(self.paint_portrait(&model, zoom, w, h))
        }))
        .ok()
        .flatten()
    }

    /// Render the region containing `(px, pz, level)` (a player's coord). Orbit
    /// with drag, zoom with scroll. Returns the index of a marker the user
    /// clicked (for select-in-3D), if any.
    /// Reconcile the persistent [`Actor`] set with this tick's markers: create
    /// new actors (snapped to their tile), feed a route step to movers
    /// (`teleport(tile, jump=false)` queues a walk, or snaps if >8 tiles), and
    /// drop actors that left view. Only runs when the snapshot tick changes, so
    /// each move is fed exactly once and `move_entity` interpolates it per cycle.
    fn sync_actors(&mut self, markers: &[Marker], rx: u32, ry: u32, tick: u32) {
        // Local build-area coords are anchored to the region; reset on a change.
        if self.actors_region != Some((rx, ry)) {
            self.actors.clear();
            self.actors_region = Some((rx, ry));
            self.actors_tick = u32::MAX;
        }
        if self.actors_tick == tick {
            return;
        }
        self.actors_tick = tick;

        let mut seen = std::collections::HashSet::with_capacity(markers.len());
        for m in markers {
            seen.insert(m.id);
            let lx = (REGION_BORDER + (m.x - rx as i32 * 64)).clamp(0, 103);
            let lz = (REGION_BORDER + (m.z - ry as i32 * 64)).clamp(0, 103);
            match self.actors.get_mut(&m.id) {
                Some(a) => {
                    a.kind = m.kind.clone();
                    a.label = m.label.clone();
                    if (lx, lz) != a.target {
                        a.target = (lx, lz);
                        a.entity.teleport(lx, lz, false);
                    }
                }
                None => {
                    let mut a = make_actor(&m.kind);
                    a.label = m.label.clone();
                    a.entity.teleport(lx, lz, true);
                    a.target = (lx, lz);
                    self.actors.insert(m.id, a);
                }
            }
        }
        self.actors.retain(|id, _| seen.contains(id));
    }

    pub fn show(&mut self, ui: &mut egui::Ui, px: i32, pz: i32, level: i32, tick: u32, markers: &[Marker]) -> Option<usize> {
        self.ensure_installed();
        let (rx, ry) = ((px >> 6) as u32, (pz >> 6) as u32);
        self.ensure_world(rx, ry);
        self.sync_actors(markers, rx, ry, tick);

        if self.world.is_none() {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                ui.label(self.status.clone().unwrap_or_else(|| "building scene…".into()));
            });
            return None;
        }

        let avail = ui.available_size_before_wrap();
        if avail.x < 16.0 || avail.y < 16.0 {
            return None;
        }
        let (rect, response) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());
        if response.dragged() {
            let d = response.drag_delta();
            self.cam_yaw = (self.cam_yaw - (d.x * 4.0) as i32).rem_euclid(2048);
            self.cam_pitch = (self.cam_pitch + (d.y * 3.0) as i32).clamp(128, 383);
        }
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 && response.hovered() {
            self.cam_dist = (self.cam_dist - (scroll * 4.0) as i32).clamp(600, 8000);
        }

        let ppp = ui.ctx().pixels_per_point();
        let w = (rect.width() * ppp) as i32;
        let h = (rect.height() * ppp) as i32;
        if w < 4 || h < 4 {
            return None;
        }

        // Camera orbits the followed entity. Pivot on the selected actor's
        // INTERPOLATED position (the YELLOW marker) so the camera tracks the
        // smoothly-moving model instead of snapping to its tile each tick; fall
        // back to the focus tile if that actor isn't tracked yet.
        let focus_id = markers.iter().find(|m| m.color == egui::Color32::YELLOW).map(|m| m.id);
        let (pivot_x, pivot_z) = focus_id
            .and_then(|id| self.actors.get(&id))
            .map(|a| (a.entity.x, a.entity.z))
            .unwrap_or_else(|| {
                let lx = (REGION_BORDER + (px - rx as i32 * 64)).clamp(0, 103);
                let lz = (REGION_BORDER + (pz - ry as i32 * 64)).clamp(0, 103);
                (lx * 128 + 64, lz * 128 + 64)
            });
        let pivot_y = self.height_at(level, (pivot_x >> 7).clamp(0, 103), (pivot_z >> 7).clamp(0, 103));
        let yaw = self.cam_yaw & 0x7FF;
        let pitch = self.cam_pitch.clamp(128, 383);
        let sin_t = pix3d::sin_table();
        let cos_t = pix3d::cos_table();
        let h_radius = (self.cam_dist * cos_t[pitch as usize]) >> 16;
        let cam_x = pivot_x + ((h_radius * sin_t[yaw as usize]) >> 16);
        let cam_y = pivot_y - ((self.cam_dist * sin_t[pitch as usize]) >> 16);
        let cam_z = pivot_z - ((h_radius * cos_t[yaw as usize]) >> 16);

        // Advance the animated-loc/texture clock at ~50Hz wall-clock so the
        // scene actually animates (torches, fires, water).
        let now = std::time::Instant::now();
        // Clamp gaps (window hidden/paused) so we don't fast-forward on resume.
        let elapsed = self.last_anim.map_or(0.0, |t| now.saturating_duration_since(t).as_millis().min(200) as f64);
        self.last_anim = Some(now);
        // 1 cycle = 20ms (50Hz), matching the client's per-frame seq stepping.
        self.anim_accum += elapsed / 20.0;
        let raw = self.anim_accum as i32;
        if raw > 0 {
            self.anim_accum -= raw as f64;
            let ticks = raw.min(10); // cap catch-up after a long stall
            client::dash3d::texture_manager::run_anims(ticks);
            // Advance each tracked actor one client cycle at a time: move_entity
            // interpolates its route (smooth movement) and route_move selects the
            // walk/turn seq, so animation + motion are the real client pipeline.
            let empty: std::collections::HashMap<i32, (i32, i32)> = std::collections::HashMap::new();
            let mut sounds: Vec<(i32, i32)> = Vec::new();
            for _ in 0..ticks {
                self.anim_cycle = self.anim_cycle.wrapping_add(1);
                let cyc = self.anim_cycle;
                for a in self.actors.values_mut() {
                    let size = a.entity.size.max(1);
                    client::client::move_entity(&mut a.entity, size, cyc, false, a.smoothing,
                        &empty, &empty, -1, 0, 0, false, &mut sounds);
                }
            }
            client::scene::LOOP_CYCLE.store(self.anim_cycle, std::sync::atomic::Ordering::Relaxed);
        }

        // Build each entity's animated model up front (global config state, no
        // world borrow). We then push them into the scene's sprite grid so
        // render_all depth-sorts them against walls/locs — exactly how the
        // client composites entities (push_entities → renderAll → removeSprites).
        // Drawing them on top afterward (obj_render) ignored scene depth, so
        // players punched through walls and floors.
        let clock = self.anim_cycle;
        let level_c = level.clamp(0, 3);
        // Build each actor's animated model from its (interpolated) ClientEntity:
        // position is the entity's fine coord (smoothly tweened by move_entity) and
        // the seq is whatever route_move/entity_anim set this cycle.
        let mut entity_models: Vec<(std::sync::Arc<client::dash3d::model_lit::ModelLit>, i32, i32, i32, i32)> = Vec::new();
        for a in self.actors.values() {
            let (ex, ez) = (a.entity.x, a.entity.z);
            let (lx, lz) = ((ex >> 7).clamp(0, 103), (ez >> 7).clamp(0, 103));
            let ey = self.height_at(level_c, lx, lz);
            let yaw = a.entity.yaw;
            let kind = a.kind.clone();
            let (p_id, p_f) = (a.entity.primary_seq_id, a.entity.primary_seq_frame);
            let (s_id, s_f) = (a.entity.secondary_seq_id, a.entity.secondary_seq_frame);
            let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                build_actor_model(&kind, p_id, p_f, s_id, s_f)
            }));
            if let Ok(Some(model)) = built {
                entity_models.push((std::sync::Arc::new(model), ex, ey, ez, yaw));
            }
        }
        let drawn = entity_models.len();

        let world = self.world.as_mut().unwrap();
        client::debug_opts::set_extended_draw(true);
        let (prev, pw, ph) = pix2d::swap_pixels(vec![0x0040_4048; (w * h) as usize], w, h);
        pix3d::set_clipping(0, 0, w, h);
        pix3d::set_trans(0);
        pix3d::set_model_far_clip(16000);
        let rendered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            use client::dash3d::model_source::ModelSource;
            for (model, ex, ey, ez, eyaw) in &entity_models {
                let source = ModelSource::lit(std::sync::Arc::clone(model));
                // (level, x, z, y, radius, model, yaw, typecode, extend_by_yaw)
                world.add_dynamic(level_c, *ex, *ez, *ey, 60, Some(source), *eyaw, 0, false);
            }
            world.render_all(cam_x, cam_y, cam_z, pitch, yaw, level_c);
            world.remove_sprites();
        }));
        // Diagnostic: clock + actor count + a sample actor's live seq state, so a
        // "not animating" report carries the actual secondary (walk/stand) seq.
        {
            let mut d = format!("clk {clock} · actors {} · drawn {}", self.actors.len(), drawn);
            if let Some(a) = self.actors.values().next() {
                d.push_str(&format!(" · seq2={} f={}", a.entity.secondary_seq_id, a.entity.secondary_seq_frame));
            }
            self.dbg = d;
        }
        let (frame, _, _) = pix2d::swap_pixels(prev, pw, ph);
        if rendered.is_err() {
            ui.colored_label(egui::Color32::LIGHT_RED, "scene render panicked");
            return None;
        }
        let tex = crate::pix_bridge::upload_rgb(ui.ctx(), "admin_scene", &frame, w as usize, h as usize);
        let painter = ui.painter_at(rect);
        painter.image(
            tex.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );

        // Diagnostic HUD (top-left): clock advancing + draw count + focus anim.
        painter.text(
            rect.left_top() + egui::vec2(6.0, 4.0),
            egui::Align2::LEFT_TOP,
            &self.dbg,
            egui::FontId::monospace(11.0),
            egui::Color32::from_rgb(120, 230, 160),
        );

        // Overlay live entities, projected with the SAME render camera (focal
        // 512), sitting on their tile. Radius scales with depth (closer = big).
        // Collect screen positions for click-to-select.
        let mut screen: Vec<(usize, egui::Pos2)> = Vec::new();
        for (i, m) in markers.iter().enumerate() {
            // Project the actor's interpolated position (falls back to the tile)
            // so rings/labels track the smoothly-moving model, not the snapped tile.
            let (ex, ez) = self.actors.get(&m.id)
                .map(|a| (a.entity.x, a.entity.z))
                .unwrap_or_else(|| {
                    let lx = (REGION_BORDER + (m.x - rx as i32 * 64)).clamp(0, 103);
                    let lz = (REGION_BORDER + (m.z - ry as i32 * 64)).clamp(0, 103);
                    (lx * 128 + 64, lz * 128 + 64)
                });
            let my = self.height_at(level, (ex >> 7).clamp(0, 103), (ez >> 7).clamp(0, 103));
            let (sx, sy, _, _, vz) = pix3d::project_with_view_space(
                ex - cam_x, my - cam_y, ez - cam_z, pitch, yaw, 512, w / 2, h / 2,
            );
            if vz < 1 || sx < 0 || sx >= w || sy < 0 || sy >= h {
                continue;
            }
            let fx = rect.left() + (sx as f32 / w as f32) * rect.width();
            let fy = rect.top() + (sy as f32 / h as f32) * rect.height();
            let pos = egui::pos2(fx, fy);
            // Entities are real models in-frame; the overlay just rings the
            // selected one and labels players (names float at the feet point).
            if m.color == egui::Color32::YELLOW {
                painter.circle_stroke(pos, 9.0, egui::Stroke::new(2.0, egui::Color32::YELLOW));
            }
            if let Some(label) = &m.label {
                painter.text(
                    egui::pos2(fx, fy - 4.0),
                    egui::Align2::CENTER_BOTTOM,
                    label,
                    egui::FontId::proportional(11.0),
                    m.color,
                );
            }
            screen.push((i, pos));
        }

        // Click an entity in the 3D view to select it (nearest within 16px).
        if response.clicked() {
            if let Some(p) = response.interact_pointer_pos() {
                let mut best: Option<(usize, f32)> = None;
                for (i, sp) in &screen {
                    let d = sp.distance(p);
                    if best.is_none_or(|(_, bd)| d < bd) {
                        best = Some((*i, d));
                    }
                }
                if let Some((i, d)) = best {
                    if d <= 16.0 {
                        return Some(i);
                    }
                }
            }
        }
        None
    }
}

/// Pick the current frame of a looping animation `anim_id` at scene-clock
/// `ticks` (walks the per-frame durations). Returns the seq + frame, or
/// `(None, -1)` for no animation (static base pose).
fn anim_frame(anim_id: i32, ticks: i32) -> (Option<client::config::seq_type::SeqType>, i32) {
    if anim_id < 0 {
        return (None, -1);
    }
    let seq = client::config::seq_type::list(anim_id);
    let n = seq.frames.as_ref().map_or(0, |f| f.len());
    if n == 0 {
        return (None, -1);
    }
    let total: i32 = (0..n as i32).map(|i| seq.frame_duration(i).max(1)).sum();
    if total <= 0 {
        return (Some(seq), 0);
    }
    let mut t = ticks.rem_euclid(total);
    let mut frame = 0i32;
    for i in 0..n as i32 {
        let d = seq.frame_duration(i).max(1);
        if t < d {
            frame = i;
            break;
        }
        t -= d;
    }
    (Some(seq), frame)
}

/// Create a persistent [`Actor`] for a marker kind, seeding the ClientEntity's
/// anim fields (walk/stand/turn seqs, size, turn speed) so `route_move` can pick
/// the right stance as it moves — mirrors the client's NPC_INFO/appearance setup.
fn make_actor(kind: &MarkerKind) -> Actor {
    let mut e = client::dash3d::ClientEntity::default();
    let smoothing;
    match kind {
        MarkerKind::Npc { type_id, .. } => {
            let t = client::config::npc_type::list(*type_id);
            e.size = t.size.max(1);
            e.turnspeed = t.turnspeed;
            e.walkanim = t.walkanim;
            e.walkanim_b = t.walkanim_b;
            e.walkanim_l = t.walkanim_r; // Java swaps l/r when copying from the type
            e.walkanim_r = t.walkanim_l;
            e.readyanim = t.readyanim;
            e.turnleftanim = t.turnleftanim;
            e.turnrightanim = t.turnrightanim;
            smoothing = t.walksmoothing;
        }
        MarkerKind::Player { ready_anim, walk_anim, .. } => {
            e.size = 1;
            e.readyanim = *ready_anim;
            e.walkanim = *walk_anim;
            e.walkanim_b = *walk_anim;
            e.walkanim_l = *walk_anim;
            e.walkanim_r = *walk_anim;
            smoothing = true;
        }
    }
    Actor { entity: e, kind: kind.clone(), label: None, smoothing, target: (0, 0) }
}

/// Build an actor's animated model from its live ClientEntity seq state (the
/// primary one-shot + secondary walk/stand seq route_move set this cycle).
fn build_actor_model(
    kind: &MarkerKind, p_id: i32, p_f: i32, s_id: i32, s_f: i32,
) -> Option<client::dash3d::model_lit::ModelLit> {
    use client::config::seq_type;
    let primary = (p_id != -1).then(|| seq_type::list(p_id));
    let secondary = (s_id != -1).then(|| seq_type::list(s_id));
    match kind {
        MarkerKind::Npc { type_id, .. } => client::config::npc_type::list(*type_id)
            .get_temp_model(primary.as_ref(), p_f, secondary.as_ref(), s_f),
        MarkerKind::Player { worn, colours, female, .. } => {
            let mut pm = client::dash3d::player_model::PlayerModel::new();
            pm.apply_appearance(*worn, *colours, *female, -1);
            pm.get_temp_model(primary.as_ref(), p_f, secondary.as_ref(), s_f)
        }
    }
}

/// Build the client scene World for a region's raw map bytes, centred in the
/// 104² world. Mirrors jaged's `build_region_world`.
/// Load the mapscene sprites (trees/stairs/…) + mapfunction icons (quest/bank/
/// shop/…) from the sprites archive — shared by the live scene and the boot
/// world-map baker. Requires [`install_client`] to have run.
pub fn load_map_sprites() -> (
    Option<Vec<client::graphics::pix8::Pix8>>,
    Option<Vec<client::graphics::pix32::Pix32>>,
) {
    let mut reg = client::js5::js5_net::LOADERS.lock().unwrap();
    if let Some(loader) = reg.get_mut(SPRITES as usize).and_then(|o| o.as_mut()) {
        let ms = client::graphics::pix_loader::make_pix8_array(loader, "mapscene", "");
        let mf = client::graphics::pix_loader::make_pix32_array(loader, "mapfunction", "");
        (ms, mf)
    } else {
        (None, None)
    }
}

/// Bake a region's detailed top-down image (416², 4px/tile) with floors + walls
/// + scenery + map-function icons, building its own World (with locs). Free
/// function so the boot world-map baker can pre-bake every region. The 64×64
/// region itself is the centre of the 104² build-area image.
pub fn bake_region_detail(
    content_dir: &str, rx: u32, ry: u32, level: i32,
    mapscene: Option<&[client::graphics::pix8::Pix8]>,
    mapfunction: Option<&[client::graphics::pix32::Pix32]>,
) -> Option<(Vec<i32>, usize)> {
    use cache::maps::text::RawRegion;
    let path = std::path::Path::new(content_dir).join("maps").join(format!("{rx}_{ry}.jm2"));
    let raw = std::fs::read_to_string(&path).ok().and_then(|t| RawRegion::from_text(&t).ok())?;
    let land = raw.encode_land();
    let locs = raw.encode_locs();
    let (world, _) = build_region_world(rx, ry, &land, Some(&locs));

    // Bake at the client minimap's native 512² layout (build area inset 48px,
    // 4px/tile) so we can reuse its exact wall/mapscene detail pass, then crop.
    const S: usize = 512;
    const INSET: usize = (S - MAP_TILES * MAP_PX_PER_TILE) / 2; // 48
    let mut img = vec![0i32; S * S];
    for tz in 1..(MAP_TILES as i32 - 1) {
        let mut off = ((MAP_TILES as i32 - 1 - tz) as usize * MAP_PX_PER_TILE + INSET) * S + INSET + MAP_PX_PER_TILE;
        for tx in 1..(MAP_TILES as i32 - 1) {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                world.render_2d_ground(&mut img, off, S, level, tx, tz);
            }));
            off += MAP_PX_PER_TILE;
        }
    }
    let wall_rgb = (238 << 16) + (238 << 8) + 238;
    let door_rgb = 238 << 16;
    let (prev, pw, ph) = pix2d::swap_pixels(img, S as i32, S as i32);
    pix2d::set_clipping(0, 0, S as i32, S as i32);
    for tz in 1..(MAP_TILES as i32 - 1) {
        for tx in 1..(MAP_TILES as i32 - 1) {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                client::minimap::draw_detail(&world, mapscene, level, tx, tz, wall_rgb, door_rgb);
            }));
        }
    }
    if let Some(funcs) = mapfunction {
        for tz in 0..MAP_TILES as i32 {
            for tx in 0..MAP_TILES as i32 {
                let gd = world.gd_type(level, tx, tz);
                if gd == 0 {
                    continue;
                }
                let loc_id = (gd >> 14) & 0x7FFF;
                let Some(lt) = client::config::loc_type::list(loc_id) else { continue };
                let f = lt.mapfunction;
                if f < 0 {
                    continue;
                }
                let Some(sprite) = funcs.get(f as usize) else { continue };
                let cx = tx * 4 + INSET as i32 + 2 - sprite.wi / 2;
                let cy = (MAP_TILES as i32 - 1 - tz) * 4 + INSET as i32 + 2 - sprite.hi / 2;
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    sprite.plot_sprite(cx, cy);
                }));
            }
        }
    }
    let (baked, _, _) = pix2d::swap_pixels(prev, pw, ph);
    let w = MAP_TILES * MAP_PX_PER_TILE;
    let mut cropped = vec![0i32; w * w];
    for y in 0..w {
        let src = (y + INSET) * S + INSET;
        cropped[y * w..y * w + w].copy_from_slice(&baked[src..src + w]);
    }
    Some((cropped, w))
}

pub fn build_region_world(rx: u32, ry: u32, land: &[u8], locs: Option<&[u8]>) -> (World, Vec<Vec<Vec<i32>>>) {
    use client::client_build;

    let b = REGION_BORDER;
    client_build::init();
    client_build::load_ground(land, b, b, rx as i32 * 64 - b, ry as i32 * 64 - b, None);
    if let Some(locs) = locs {
        client_build::load_locations(locs, b, b);
    }

    let ground_h = client_build::STATE.lock().unwrap().ground_h.clone();
    let mut world = World::new(4, 104, 104, ground_h.clone());
    world.fill_base_level(0);

    let sin = pix3d::sin_table();
    let mut heights = [0i32; 9];
    for (i, slot) in heights.iter_mut().enumerate() {
        let pitch = (i as i32) * 32 + 128 + 15;
        let dist = pitch * 3 + 600;
        *slot = (dist * sin[pitch as usize]) >> 16;
    }
    world.reset_vis_calc(&heights, 500, 800, 512, 334);
    client_build::finish_build(&mut world, None, false, 0);
    (world, ground_h)
}

/// Install the 16 client loaders from the cache (idempotent).
fn install_local_loaders(cache: &mut Cache) {
    use std::sync::atomic::Ordering;
    let mut reg = client::js5::js5_net::LOADERS.lock().unwrap();
    if reg.len() < 16 {
        reg.resize_with(16, || None);
    }
    for archive in 0u8..16 {
        if reg[archive as usize].is_some() {
            continue;
        }
        let Ok(Some(index_raw)) = cache.read_master_raw(archive) else {
            continue;
        };
        let mut loader = client::js5::js5_loader::Js5Loader::new(archive as i32, false, false, false);
        loader.base.decode_index(&index_raw);
        for gid in 0..loader.base.packed.len() {
            if let Ok(Some(raw)) = cache.read_raw(archive, gid as u32) {
                let end = raw.len().saturating_sub(2);
                loader.base.packed[gid] = Some(raw[..end].to_vec());
            }
        }
        loader.load_status.store(true, Ordering::SeqCst);
        reg[archive as usize] = Some(Box::new(loader));
    }
}
