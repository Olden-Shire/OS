//! OS server control panel — an eframe/egui app that runs the game server on
//! a background thread and renders a live, in-depth admin GUI: a startup splash
//! with boot progress, then a control panel with a tick-time performance graph
//! and live player/npc tables. (3D admin scene view, chat, equipment and deeper
//! analytics build on this foundation.)
//!
//! The server tick loop owns the `World` exclusively; each cycle it hands the
//! panel a read-only snapshot via a `tick_hook`, and reports boot stages via a
//! `progress` callback — so the GUI never touches the hot loop's `&mut World`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};

mod pix_bridge;
mod scene;
mod worldmap;

/// `OS_DEBUG` gate for verbose diagnostic logging — checked once, cached.
/// Set the env var `OS_DEBUG` (to any value) to surface gated `dbg_log!` output.
pub fn debug_enabled() -> bool {
    use std::sync::atomic::{AtomicU8, Ordering};
    static STATE: AtomicU8 = AtomicU8::new(0);
    match STATE.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => {
            let on = std::env::var_os("OS_DEBUG").is_some();
            STATE.store(if on { 2 } else { 1 }, Ordering::Relaxed);
            on
        }
    }
}

/// `eprintln!` that only fires when `OS_DEBUG` is set (gated diagnostics).
#[macro_export]
macro_rules! dbg_log {
    ($($arg:tt)*) => {
        if $crate::debug_enabled() { eprintln!($($arg)*); }
    };
}

/// Tick-time samples kept for the graph (~2.4 min at the 600ms tick).
const TICK_HISTORY: usize = 240;

#[derive(Clone, Default)]
struct PlayerRow {
    pid: usize,
    name: String,
    x: i32,
    z: i32,
    level: i32,
    combat: i32,
    run_energy: i32,
    gender: i32,
    colours: [i32; 5],
    body: [i32; 7],
    ready_anim: i32,
    walk_anim: i32,
    moving: bool,
}

#[derive(Clone, Default)]
struct NpcRow {
    nid: usize,
    type_id: i32,
    x: i32,
    z: i32,
    level: i32,
    moving: bool,
}

/// One tick's stacked time-accounting sample. `stack` are the six
/// non-overlapping phases (bottom->top: io, world, npcs, players, info, output);
/// `scripts` is the cross-cutting RuneScript subset, drawn as an overlay line.
#[derive(Clone, Copy, Default)]
struct PerfSample {
    stack: [f32; 6],
    scripts: f32,
}

impl PerfSample {
    fn total(&self) -> f32 {
        self.stack.iter().sum()
    }
}

#[derive(Clone, Default)]
struct Snapshot {
    tick: u32,
    io_ms: f32,
    world_ms: f32,
    npcs_ms: f32,
    players_ms: f32,
    info_ms: f32,
    output_ms: f32,
    /// RuneScript total (subset of the world/npcs/players phases).
    scripts_ms: f32,
    /// Whole engine cycle, as measured by the server (≈ sum of phases).
    engine_ms: f32,
    /// Open sockets + JS5 (cache-stream) subset.
    connections: usize,
    js5: usize,
    players: Vec<PlayerRow>,
    npcs: Vec<NpcRow>,
}

impl Snapshot {
    /// True wall-clock per server tick: network in + engine + network out.
    fn total_ms(&self) -> f32 {
        self.io_ms + self.engine_ms + self.output_ms
    }
}

/// Shared state between the server thread (writer) and the GUI (reader).
struct PanelState {
    stage: String,
    started: bool,
    started_at: Option<Instant>,
    snapshot: Snapshot,
    /// Per-tick [io, engine, output] ms, for the stacked frame-time graph.
    perf_history: VecDeque<PerfSample>,
    player_history: VecDeque<f32>,
    peak_players: usize,
    /// Rolling activity feed (joins/leaves + admin actions).
    events: VecDeque<String>,
    /// Last tick's pid->name, to detect joins/leaves.
    prev_players: std::collections::HashMap<usize, String>,
    /// Next world chat_seq we haven't yet pulled into the chat log.
    last_chat_seq: u64,
    /// Decoded public-chat lines (separate from the activity feed).
    chat: VecDeque<String>,
    /// Count of server warnings seen this session (for the header indicator).
    warning_count: usize,
    /// When set, the tick_hook stops sampling so the panel freezes on a moment
    /// (the server keeps ticking — this only pauses the panel's view).
    paused: bool,
    /// While paused, sample exactly one more tick then re-freeze (frame-step).
    step_once: bool,
    /// Whole-world map: baked composite (None until ready) + bake progress.
    worldmap: Option<Arc<worldmap::WorldMap>>,
    /// Full-detail per-region tile store (walls/icons), read on demand.
    tiles: Option<Arc<worldmap::TileStore>>,
    map_bake_done: usize,
    map_bake_total: usize,
    map_bake_ready: bool,
    /// Cache pack/verify progress (groups done / total) for the splash bars.
    pack_done: usize,
    pack_total: usize,
    verify_done: usize,
    verify_total: usize,
    /// Set if the server script bundle failed to compile — startup halts and the
    /// splash shows this instead of hanging.
    script_error: Option<String>,
    /// Set if the server thread exited before "listening" for any other reason
    /// (cache pack/verify failure, bind error, …) — splash shows it instead of
    /// hanging forever.
    boot_error: Option<String>,
}

impl PanelState {
    fn new() -> Self {
        PanelState {
            stage: "starting…".to_string(),
            started: false,
            started_at: None,
            snapshot: Snapshot::default(),
            perf_history: VecDeque::with_capacity(TICK_HISTORY),
            player_history: VecDeque::with_capacity(TICK_HISTORY),
            peak_players: 0,
            events: VecDeque::with_capacity(256),
            prev_players: std::collections::HashMap::new(),
            last_chat_seq: 0,
            chat: VecDeque::with_capacity(256),
            warning_count: 0,
            paused: false,
            step_once: false,
            worldmap: None,
            tiles: None,
            map_bake_done: 0,
            map_bake_total: 0,
            map_bake_ready: false,
            pack_done: 0,
            pack_total: 0,
            verify_done: 0,
            verify_total: 0,
            script_error: None,
            boot_error: None,
        }
    }
}

fn main() -> eframe::Result<()> {
    let state = Arc::new(Mutex::new(PanelState::new()));
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<server::PanelCommand>();

    // Launch the server on a background thread, reporting boot progress and a
    // per-tick snapshot into the shared state.
    {
        let progress_state = Arc::clone(&state);
        let tick_state = Arc::clone(&state);
        let config = server::ServerConfig {
            command_rx: Some(cmd_rx),
            addr: "0.0.0.0:40001".to_string(),
            cache_dir: "cache".to_string(),
            content_dir: Some("Content".to_string()),
            script_dir: Some("data/pack".to_string()),
            progress: Some(Box::new(move |stage: &str| {
                let mut s = progress_state.lock().unwrap();
                // "packing/verifying cache D/T" carry counts -> drive the bars.
                if let Some(rest) = stage.strip_prefix("packing cache ") {
                    if let Some((d, t)) = rest.split_once('/') {
                        if let (Ok(d), Ok(t)) = (d.parse(), t.parse()) {
                            s.pack_done = d;
                            s.pack_total = t;
                        }
                    }
                    s.stage = "packing cache".to_string();
                    return;
                }
                if let Some(rest) = stage.strip_prefix("verifying cache ") {
                    if let Some((d, t)) = rest.split_once('/') {
                        if let (Ok(d), Ok(t)) = (d.parse(), t.parse()) {
                            s.verify_done = d;
                            s.verify_total = t;
                        }
                    }
                    s.stage = "verifying cache".to_string();
                    return;
                }
                if let Some(msg) = stage.strip_prefix("scripts error: ") {
                    s.script_error = Some(msg.to_string());
                    return;
                }
                s.stage = stage.to_string();
                if stage == "listening" {
                    s.started = true;
                    s.started_at = Some(Instant::now());
                }
            })),
            tick_hook: Some(Box::new(move |world: &engine::World, stats: server::TickStats| {
                let mut s = tick_state.lock().unwrap();
                // Frozen view: keep the last sampled moment (server keeps
                // running). A queued frame-step samples exactly one more tick.
                if s.paused && !s.step_once {
                    return;
                }
                s.step_once = false;
                s.snapshot = build_snapshot(world, stats);
                let players = s.snapshot.players.len();
                s.peak_players = s.peak_players.max(players);

                // Diff the player set against last tick -> join/leave events.
                let tick = s.snapshot.tick;
                let current: std::collections::HashMap<usize, String> =
                    s.snapshot.players.iter().map(|p| (p.pid, p.name.clone())).collect();
                for (pid, name) in &current {
                    if !s.prev_players.contains_key(pid) {
                        push_event(&mut s.events, tick, format!("▶ {name} (pid {pid}) logged in"));
                    }
                }
                let left: Vec<String> = s.prev_players.iter()
                    .filter(|(pid, _)| !current.contains_key(pid))
                    .map(|(pid, name)| format!("◀ {name} (pid {pid}) logged out"))
                    .collect();
                for e in left {
                    push_event(&mut s.events, tick, e);
                }
                s.prev_players = current;

                // Pull any new public-chat lines into the activity feed,
                // decoding the WordPack bytes with the chat Huffman table.
                let new_chat: Vec<(u32, String, Vec<u8>)> = world.chat_log.iter()
                    .filter(|l| l.seq >= s.last_chat_seq)
                    .map(|l| (l.tick, l.name.clone(), l.message.clone()))
                    .collect();
                if let Some(last) = world.chat_log.back() {
                    s.last_chat_seq = last.seq + 1;
                }
                for (_ctick, name, bytes) in new_chat {
                    let text = client::wordpack::unpack(&mut client::io::packet::Packet::from_vec(bytes));
                    if s.chat.len() >= 256 {
                        s.chat.pop_front();
                    }
                    s.chat.push_back(format!("{name}: {text}"));
                }

                // Surface server-side warnings (missing JS5 groups, dropped
                // packets) into the activity feed instead of only stderr.
                let warns: Vec<String> = server::WARN_LOG.lock()
                    .map(|mut w| w.drain(..).collect())
                    .unwrap_or_default();
                s.warning_count += warns.len();
                for w in warns {
                    push_event(&mut s.events, tick, format!("⚠ {w}"));
                }

                let sample = PerfSample {
                    stack: [
                        s.snapshot.io_ms, s.snapshot.world_ms, s.snapshot.npcs_ms,
                        s.snapshot.players_ms, s.snapshot.info_ms, s.snapshot.output_ms,
                    ],
                    scripts: s.snapshot.scripts_ms,
                };
                if s.perf_history.len() == TICK_HISTORY {
                    s.perf_history.pop_front();
                }
                s.perf_history.push_back(sample);
                if s.player_history.len() == TICK_HISTORY {
                    s.player_history.pop_front();
                }
                s.player_history.push_back(players as f32);
            })),
            ..Default::default()
        };
        let boot_state = Arc::clone(&state);
        std::thread::spawn(move || {
            if let Err(e) = server::run(config) {
                eprintln!("[panel] server exited: {e}");
                // Record the exit so the splash shows it (don't hang forever).
                // Only meaningful if it died before reaching "listening".
                let mut s = boot_state.lock().unwrap();
                if !s.started {
                    s.boot_error = Some(e.to_string());
                }
            }
        });
    }

    // Bake the whole-world overview map on a background thread (cached to disk;
    // re-baked only when the .jm2 maps change). Progress drives the splash.
    {
        let map_state = Arc::clone(&state);
        std::thread::spawn(move || {
            eprintln!("[worldmap] baking/loading whole-world map (with walls/icons)…");
            let prog_state = Arc::clone(&map_state);
            let baked = worldmap::bake_or_load("Content", move |done, total| {
                if total > 0 && (done % 50 == 0 || done == total) {
                    crate::dbg_log!("[worldmap] {done}/{total} regions");
                }
                let mut s = prog_state.lock().unwrap();
                s.map_bake_done = done;
                s.map_bake_total = total;
            });
            match &baked {
                Some((m, store)) => eprintln!("[worldmap] ready: {}×{} overview, {} detail tiles", m.w, m.h, store.len()),
                None => eprintln!("[worldmap] bake failed (no regions?)"),
            }
            // Ensure the client config + chat Huffman table are installed even on
            // a world-map cache hit (so chat decodes + the scene is instant).
            let _ = scene::install_client();
            let mut s = map_state.lock().unwrap();
            if let Some((m, store)) = baked {
                s.worldmap = Some(Arc::new(m));
                s.tiles = Some(Arc::new(store));
            }
            s.map_bake_ready = true;
        });
    }

    let app_state = Arc::clone(&state);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([800.0, 560.0])
            .with_title("OS Control Panel"),
        ..Default::default()
    };
    eframe::run_native(
        "OS Control Panel",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(PanelApp {
                state: app_state,
                cmd_tx,
                broadcast_text: String::new(),
                selected: Selection::None,
                last_inspected: None,
                tp_x: 0,
                tp_z: 0,
                tp_level: 0,
                scene: scene::Scene::default(),
                scene_view_level: None,
                filter: String::new(),
                map_whole_world: true,
                world_tex: None,
                world_tile_tex: std::collections::HashMap::new(),
                world_zoom: 0.0,
                world_pan: egui::Vec2::ZERO,
                world_centered_on: None,
                console_tab: ConsoleTab::Activity,
                last_title: String::new(),
                msg_text: String::new(),
                scroll_sel: Selection::None,
            }))
        }),
    )
}

fn push_event(events: &mut VecDeque<String>, tick: u32, msg: String) {
    if events.len() >= 256 {
        events.pop_front();
    }
    events.push_back(format!("[t{tick}] {msg}"));
}

fn build_snapshot(world: &engine::World, stats: server::TickStats) -> Snapshot {
    let players = world
        .players
        .iter()
        .flatten()
        .map(|p| PlayerRow {
            pid: p.pid,
            name: p.username.clone(),
            x: p.entity.x,
            z: p.entity.z,
            level: p.entity.level,
            combat: p.combat_level,
            run_energy: p.run_energy,
            gender: p.gender,
            colours: p.colours,
            body: p.body,
            ready_anim: p.ready_anim,
            walk_anim: p.walk_anim,
            moving: p.entity.walk_dir >= 0,
        })
        .collect();
    let npcs = world
        .npcs
        .iter()
        .flatten()
        .map(|n| NpcRow {
            nid: n.nid,
            type_id: n.type_id,
            x: n.entity.x,
            z: n.entity.z,
            level: n.entity.level,
            moving: n.entity.walk_dir >= 0,
        })
        .collect();
    let ms = |d: Duration| d.as_secs_f32() * 1000.0;
    let c = stats.cycle;
    Snapshot {
        tick: world.tick,
        io_ms: ms(stats.io),
        world_ms: ms(c.world),
        npcs_ms: ms(c.npcs),
        players_ms: ms(c.players),
        info_ms: ms(c.info),
        output_ms: ms(stats.output),
        scripts_ms: ms(c.scripts),
        engine_ms: ms(stats.engine),
        connections: stats.connections,
        js5: stats.js5,
        players,
        npcs,
    }
}

/// What entity (if any) the admin has selected for inspection.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Selection {
    None,
    Player(usize),
    Npc(usize),
}

/// Which log the bottom console shows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ConsoleTab {
    Activity,
    Chat,
}

struct PanelApp {
    state: Arc<Mutex<PanelState>>,
    cmd_tx: std::sync::mpsc::Sender<server::PanelCommand>,
    broadcast_text: String,
    selected: Selection,
    /// Teleport editor, seeded from the selected player's coord on selection.
    last_inspected: Option<usize>,
    tp_x: i32,
    tp_z: i32,
    tp_level: i32,
    scene: scene::Scene,
    /// Scene plane override (None = follow the selected player's level).
    scene_view_level: Option<i32>,
    /// Substring/id filter for the player + npc lists.
    filter: String,
    /// World map: false = follow the focused entity's region (real baked map),
    /// true = the whole-world composite image.
    map_whole_world: bool,
    /// Cached GPU texture for the (large) whole-world overview, uploaded once.
    world_tex: Option<egui::TextureHandle>,
    /// Detailed per-region tile textures (walls/scenery/icons), drawn over the
    /// overview when zoomed in. Keyed by region; bounded + cleared when large.
    world_tile_tex: std::collections::HashMap<(u32, u32), egui::TextureHandle>,
    /// Whole-world view transform: screen-px per image-px (0 = uninit -> default),
    /// and the top-left pan offset within the viewport.
    world_zoom: f32,
    world_pan: egui::Vec2,
    /// Region the whole-world view last auto-centered on (so we recenter when the
    /// selection moves to a new region, but don't fight manual panning).
    world_centered_on: Option<(i32, i32)>,
    /// Which log the bottom console panel is showing.
    console_tab: ConsoleTab,
    /// Last window title we pushed (only re-send when it changes).
    last_title: String,
    /// Per-player private-message composer (inspector).
    msg_text: String,
    /// Last selection we auto-scrolled the side lists to (so we scroll on change
    /// — e.g. selecting via the map — without fighting manual scrolling).
    scroll_sel: Selection,
}

/// A consistent copy of the shared state, taken under the lock once per frame so
/// rendering can freely borrow `&mut self` (selection, command sends).
struct View {
    snap: Snapshot,
    perf_hist: VecDeque<PerfSample>,
    player_hist: VecDeque<f32>,
    peak: usize,
    uptime: u64,
    events: Vec<String>,
    chat: Vec<String>,
    warning_count: usize,
    worldmap: Option<Arc<worldmap::WorldMap>>,
    tiles: Option<Arc<worldmap::TileStore>>,
}

impl eframe::App for PanelApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Live UI: repaint faster when the 3D scene is on (so it animates),
        // otherwise a few times a second to track the snapshot.
        let repaint_ms = if self.scene.enabled { 16 }
            else if !matches!(self.selected, Selection::None) { 33 }
            else { 120 };
        ctx.request_repaint_after(Duration::from_millis(repaint_ms));
        {
            let state = self.state.lock().unwrap();
            // Hold on the splash until the server is listening AND the
            // whole-world map has baked/loaded (first boot bakes; later boots
            // hit the disk cache and pass straight through).
            if !state.started || !state.map_bake_ready {
                // Keep repainting so the bake progress bar animates.
                ctx.request_repaint_after(Duration::from_millis(80));
                splash(ctx, &state);
                return;
            }
        }
        let view = {
            let s = self.state.lock().unwrap();
            View {
                snap: s.snapshot.clone(),
                perf_hist: s.perf_history.clone(),
                player_hist: s.player_history.clone(),
                peak: s.peak_players,
                uptime: s.started_at.map_or(0, |t| t.elapsed().as_secs()),
                events: s.events.iter().cloned().collect(),
                chat: s.chat.iter().cloned().collect(),
                warning_count: s.warning_count,
                worldmap: s.worldmap.clone(),
                tiles: s.tiles.clone(),
            }
        };
        self.left_panel(ctx, &view);
        self.right_map(ctx, &view);
        self.console_panel(ctx, &view);
        self.central_scene(ctx, &view);

        // Live window title: at-a-glance population + tick.
        let title = format!(
            "OS Control Panel — {} players · {} npcs · tick {}",
            view.snap.players.len(), view.snap.npcs.len(), view.snap.tick
        );
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }
}

impl PanelApp {
    /// Append a line to the activity feed (admin actions).
    fn log(&self, msg: impl Into<String>) {
        let mut s = self.state.lock().unwrap();
        let tick = s.snapshot.tick;
        push_event(&mut s.events, tick, msg.into());
    }

    /// Bottom panel: tabbed Activity / world-Chat log + a broadcast composer.
    fn console_panel(&mut self, ctx: &egui::Context, view: &View) {
        egui::TopBottomPanel::bottom("console").resizable(true).default_height(170.0).show(ctx, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.console_tab, ConsoleTab::Activity,
                    format!("Activity ({})", view.events.len()));
                ui.selectable_value(&mut self.console_tab, ConsoleTab::Chat,
                    format!("💬 Chat ({})", view.chat.len()));
            });
            let lines: &[String] = match self.console_tab {
                ConsoleTab::Activity => &view.events,
                ConsoleTab::Chat => &view.chat,
            };
            egui::ScrollArea::vertical().id_salt("console_log").stick_to_bottom(true)
                .auto_shrink([false, false]).max_height(110.0).show(ui, |ui| {
                if lines.is_empty() {
                    ui.weak(if self.console_tab == ConsoleTab::Chat {
                        "No public chat yet."
                    } else {
                        "No activity yet."
                    });
                }
                for e in lines {
                    let mut t = egui::RichText::new(e).monospace().size(11.0);
                    if e.starts_with('⚠') {
                        t = t.color(egui::Color32::from_rgb(255, 138, 101));
                    } else if e.starts_with('▶') {
                        t = t.color(egui::Color32::from_rgb(130, 199, 132));
                    } else if e.starts_with('◀') {
                        t = t.color(egui::Color32::from_white_alpha(150));
                    }
                    ui.label(t);
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Broadcast:");
                let tw = (ui.available_width() - 52.0).max(80.0);
                let resp = ui.add_sized(
                    [tw, 20.0],
                    egui::TextEdit::singleline(&mut self.broadcast_text)
                        .hint_text("message to all players…"),
                );
                let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (ui.button("Send").clicked() || enter) && !self.broadcast_text.trim().is_empty() {
                    let text = self.broadcast_text.clone();
                    let _ = self.cmd_tx.send(server::PanelCommand::Broadcast(text.clone()));
                    self.log(format!("📢 broadcast: {text}"));
                    self.broadcast_text.clear();
                }
            });
            ui.add_space(2.0);
        });
    }

    /// Left column: stats, scene toggle, stacked frame-time graph, players
    /// graph, and the selected-entity inspector.
    fn left_panel(&mut self, ctx: &egui::Context, view: &View) {
        let snap = &view.snap;
        egui::SidePanel::left("left_col").exact_width(300.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.heading("OS");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let paused = self.state.lock().unwrap().paused;
                        let label = if paused { "▶ Resume" } else { "⏸ Pause" };
                        if ui.button(label).on_hover_text("Freeze the panel view (server keeps running)").clicked() {
                            self.state.lock().unwrap().paused = !paused;
                        }
                        if paused {
                            if ui.button("⏭ Step").on_hover_text("Advance the frozen view by one tick").clicked() {
                                self.state.lock().unwrap().step_once = true;
                            }
                            ui.label(egui::RichText::new("PAUSED").strong().color(egui::Color32::from_rgb(255, 196, 0)));
                        }
                        if view.warning_count > 0 {
                            ui.label(egui::RichText::new(format!("⚠ {}", view.warning_count))
                                .strong().color(egui::Color32::from_rgb(255, 138, 101)))
                                .on_hover_text("Server warnings this session (see Activity)");
                        }
                    });
                });
                ui.label(egui::RichText::new("Server Control Panel").weak());
                ui.separator();

                kv(ui, "Uptime", &fmt_uptime(view.uptime));
                kv(ui, "Tick", &snap.tick.to_string());
                kv(ui, "Players", &format!("{}  (peak {})", snap.players.len(), view.peak));
                kv(ui, "NPCs", &snap.npcs.len().to_string());
                kv(ui, "Connections", &format!("{}  ({} js5)", snap.connections, snap.js5));

                ui.add_space(6.0);
                ui.checkbox(&mut self.scene.enabled, "3D scene view (selected player)");

                ui.add_space(8.0);
                ui.separator();
                // Stacked server-tick time-accounting graph (client-overlay style).
                let total = snap.total_ms();
                let free = (600.0 - total).max(0.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Server tick").strong());
                    ui.label(egui::RichText::new(format!("{total:.2} ms")).color(budget_color(total)));
                    ui.label(egui::RichText::new(format!("· {free:.0} of 600 free")).weak());
                });
                perf_graph(ui, &view.perf_hist, 120.0);

                ui.add_space(8.0);
                ui.label(egui::RichText::new("Players over time").strong());
                graph(ui, "players_g", &view.player_hist, 5.0);

                ui.add_space(8.0);
                egui::CollapsingHeader::new("Analytics").default_open(true).show(ui, |ui| {
                    analytics(ui, view);
                });

                ui.add_space(8.0);
                ui.separator();
                self.inspector(ui, view);
            });
        });
    }

    /// The selected-entity inspector (player: portrait/teleport/kick/message;
    /// npc: portrait + config details).
    fn inspector(&mut self, ui: &mut egui::Ui, view: &View) {
        let snap = &view.snap;
        match self.selected {
            Selection::Npc(nid) => {
                ui.label(egui::RichText::new("SELECTED NPC").weak().size(11.0));
                if let Some(n) = snap.npcs.iter().find(|n| n.nid == nid) {
                    let (tid, level, x, z, moving) = (n.type_id, n.level, n.x, n.z, n.moving);
                    if let Some(px) = self.scene.npc_portrait(tid, 120, 160) {
                        let tex = pix_bridge::upload_rgb(ui.ctx(), "npc_portrait", &px, 120, 160);
                        ui.vertical_centered(|ui| ui.image((tex.id(), egui::vec2(120.0, 160.0))));
                    }
                    let info = npc_info(tid);
                    if let Some((name, combat, size, ops)) = info {
                        kv(ui, "Name", &name);
                        if combat > 0 { kv(ui, "Combat", &combat.to_string()); }
                        kv(ui, "Size", &format!("{size}×{size}"));
                        if !ops.is_empty() { kv(ui, "Options", &ops.join(", ")); }
                    }
                    kv(ui, "nid", &n.nid.to_string());
                    kv(ui, "type", &tid.to_string());
                    kv(ui, "Coord", &format!("{level} : {x}, {z}"));
                    kv(ui, "State", if moving { "moving" } else { "idle" });
                } else {
                    self.selected = Selection::None;
                }
            }
            Selection::Player(pid) => {
                ui.label(egui::RichText::new("SELECTED PLAYER").weak().size(11.0));
                if let Some(p) = snap.players.iter().find(|p| p.pid == pid) {
                    let pname = p.name.clone();
                    // Live appearance portrait (the real character model, turning).
                    let mut worn = [0i32; 12];
                    for part in 0..7 {
                        if p.body[part] >= 0 {
                            worn[client::dash3d::player_model::BASE_PART_MAP[part]] = 256 + p.body[part];
                        }
                    }
                    let anim = if p.ready_anim >= 0 { p.ready_anim } else { 808 };
                    if let Some(px) = self.scene.portrait(worn, p.colours, p.gender == 1, anim, 120, 160) {
                        let tex = pix_bridge::upload_rgb(ui.ctx(), "portrait", &px, 120, 160);
                        ui.vertical_centered(|ui| ui.image((tex.id(), egui::vec2(120.0, 160.0))));
                    }
                    kv(ui, "Name", &p.name);
                    kv(ui, "pid", &p.pid.to_string());
                    kv(ui, "Coord", &format!("{} : {}, {}", p.level, p.x, p.z));
                    kv(ui, "Combat", &p.combat.to_string());
                    kv(ui, "Run energy", &format!("{}%", p.run_energy / 100));
                    if self.last_inspected != Some(pid) {
                        self.last_inspected = Some(pid);
                        self.tp_x = p.x;
                        self.tp_z = p.z;
                        self.tp_level = p.level;
                    }
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("TELEPORT").weak().size(11.0));
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut self.tp_x).prefix("x "));
                        ui.add(egui::DragValue::new(&mut self.tp_z).prefix("z "));
                        ui.add(egui::DragValue::new(&mut self.tp_level).prefix("lvl ").range(0..=3));
                    });
                    ui.horizontal(|ui| {
                        if ui.button("➡  Teleport").clicked() {
                            let (x, z, lvl) = (self.tp_x, self.tp_z, self.tp_level);
                            let _ = self.cmd_tx.send(server::PanelCommand::Teleport { pid, x, z, level: lvl });
                            self.log(format!("➡ teleported pid {pid} -> {lvl}:{x},{z}"));
                        }
                        if ui.button("⨯  Kick").clicked() {
                            let _ = self.cmd_tx.send(server::PanelCommand::Kick(pid));
                            self.log(format!("⨯ kicked pid {pid}"));
                            self.selected = Selection::None;
                        }
                    });

                    // Private message to just this player (individual chat).
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("MESSAGE PLAYER").weak().size(11.0));
                    ui.horizontal(|ui| {
                        // Reserve room for the Send button — an INFINITY-width
                        // TextEdit in a horizontal would force the panel wider.
                        let tw = (ui.available_width() - 52.0).max(60.0);
                        let resp = ui.add_sized([tw, 20.0],
                            egui::TextEdit::singleline(&mut self.msg_text).hint_text("private message…"));
                        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if (ui.button("Send").clicked() || enter) && !self.msg_text.trim().is_empty() {
                            let text = self.msg_text.clone();
                            let _ = self.cmd_tx.send(server::PanelCommand::Message { pid, text: text.clone() });
                            self.log(format!("✉ to {pname}: {text}"));
                            self.msg_text.clear();
                        }
                    });

                    // This player's recent public chat (individual chat view).
                    let prefix = format!("{pname}: ");
                    let mut mine: Vec<&String> = view.chat.iter().rev()
                        .filter(|l| l.starts_with(&prefix)).take(6).collect();
                    if !mine.is_empty() {
                        mine.reverse();
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("RECENT CHAT").weak().size(11.0));
                        for l in mine {
                            ui.label(egui::RichText::new(l.strip_prefix(&prefix).unwrap_or(l))
                                .monospace().size(11.0).color(egui::Color32::from_white_alpha(210)));
                        }
                    }
                } else {
                    self.selected = Selection::None;
                }
            }
            Selection::None => {
                ui.label(egui::RichText::new("select a player or npc").weak());
            }
        }
    }

    /// Right column: top-down world map + selectable player/npc lists.
    /// The real baked top-down map of the focused entity's region: the client
    /// minimap colours (semi-transparent) under a zone/region grid, with live
    /// entity dots. Click selects the nearest entity. Returns a new selection.
    fn region_map_canvas(&mut self, ui: &mut egui::Ui, view: &View) -> Option<Selection> {
        let snap = &view.snap;
        let focus = match self.selected {
            Selection::Player(pid) => snap.players.iter().find(|p| p.pid == pid).map(|p| (p.x, p.z, p.level)),
            Selection::Npc(nid) => snap.npcs.iter().find(|n| n.nid == nid).map(|n| (n.x, n.z, n.level)),
            Selection::None => snap.players.first().map(|p| (p.x, p.z, p.level)),
        };
        let Some((fx, fz, flevel)) = focus else {
            ui.add_space(8.0);
            ui.weak("No player online — select an entity, or toggle “whole world”.");
            return None;
        };
        let (rx, ry) = ((fx >> 6) as u32, (fz >> 6) as u32);
        let (ox, oz) = scene::map_origin(rx, ry);
        let tiles = scene::MAP_TILES as i32;

        // Bake the region's minimap colours (cached); upload as a texture.
        let tex = match self.scene.bake_map(rx, ry, flevel) {
            Some((pixels, mw)) => Some(pix_bridge::upload_rgb_linear(ui.ctx(), "region_map", pixels, mw, mw)),
            None => None,
        };

        let side = ui.available_width().min(360.0);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, egui::Color32::from_rgb(18, 20, 26));
        if let Some(tex) = &tex {
            // Partially transparent so the grid + dots read clearly on top.
            painter.image(
                tex.id(), rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::from_white_alpha(170),
            );
        }

        // World tile (wx,wz) -> canvas point (north = up, so flip z).
        let to_canvas = |wx: i32, wz: i32| -> egui::Pos2 {
            let lx = (wx - ox) as f32 + 0.5;
            let lz = (wz - oz) as f32 + 0.5;
            egui::pos2(
                rect.left() + lx / tiles as f32 * rect.width(),
                rect.top() + (tiles as f32 - lz) / tiles as f32 * rect.height(),
            )
        };

        // Grid: faint 8-tile zone lines, brighter 64-tile region boundaries.
        for t in 0..=tiles {
            let world_line = ox + t; // vertical line at this world x
            let is_region = world_line % 64 == 0;
            let is_zone = world_line % 8 == 0;
            if !is_zone {
                continue;
            }
            let col = if is_region {
                egui::Color32::from_rgba_unmultiplied(255, 235, 120, 90)
            } else {
                egui::Color32::from_white_alpha(16)
            };
            let x = rect.left() + t as f32 / tiles as f32 * rect.width();
            painter.vline(x, rect.y_range(), egui::Stroke::new(if is_region { 1.5 } else { 1.0 }, col));
            let wl_z = oz + t;
            let colz = if wl_z % 64 == 0 {
                egui::Color32::from_rgba_unmultiplied(255, 235, 120, 90)
            } else {
                egui::Color32::from_white_alpha(16)
            };
            let y = rect.top() + (tiles - t) as f32 / tiles as f32 * rect.height();
            painter.hline(rect.x_range(), y, egui::Stroke::new(if wl_z % 64 == 0 { 1.5 } else { 1.0 }, colz));
        }

        // Entity dots (only those within the baked build area).
        let in_area = |x: i32, z: i32| x >= ox && x < ox + tiles && z >= oz && z < oz + tiles;
        for n in &snap.npcs {
            if in_area(n.x, n.z) {
                painter.circle_filled(to_canvas(n.x, n.z), 2.5, egui::Color32::from_rgb(90, 150, 230));
            }
        }
        for p in &snap.players {
            if in_area(p.x, p.z) {
                let c = to_canvas(p.x, p.z);
                painter.circle_filled(c, 4.0, egui::Color32::LIGHT_GREEN);
                painter.text(c + egui::vec2(5.0, -2.0), egui::Align2::LEFT_CENTER, &p.name,
                             egui::FontId::proportional(11.0), egui::Color32::from_white_alpha(220));
            }
        }
        if let Some((sx, sz)) = match self.selected {
            Selection::Player(pid) => snap.players.iter().find(|p| p.pid == pid).map(|p| (p.x, p.z)),
            Selection::Npc(nid) => snap.npcs.iter().find(|n| n.nid == nid).map(|n| (n.x, n.z)),
            Selection::None => None,
        } {
            if in_area(sx, sz) {
                painter.circle_stroke(to_canvas(sx, sz), 7.0, egui::Stroke::new(2.0, egui::Color32::YELLOW));
            }
        }

        // Cursor -> world tile (for readout + double-click teleport).
        let cursor_tile = |pos: egui::Pos2| -> (i32, i32) {
            let px = ox + ((pos.x - rect.left()) / rect.width() * tiles as f32) as i32;
            let pz = oz + (tiles as f32 - (pos.y - rect.top()) / rect.height() * tiles as f32) as i32;
            (px, pz)
        };
        if let Some(pos) = resp.hover_pos() {
            if rect.contains(pos) {
                let (wx, wz) = cursor_tile(pos);
                painter.text(
                    rect.left_bottom() + egui::vec2(4.0, -3.0), egui::Align2::LEFT_BOTTOM,
                    format!("{wx},{wz}"),
                    egui::FontId::monospace(11.0), egui::Color32::from_white_alpha(190),
                );
            }
        }

        // Header line under the map.
        ui.label(egui::RichText::new(format!(
            "region {rx},{ry} · plane {flevel} · dbl-click = teleport selected player"
        )).size(11.0).weak());

        // Double-click -> teleport the selected player to that tile (god move).
        if resp.double_clicked() {
            if let (Selection::Player(pid), Some(pos)) = (self.selected, resp.interact_pointer_pos()) {
                let (wx, wz) = cursor_tile(pos);
                let _ = self.cmd_tx.send(server::PanelCommand::Teleport { pid, x: wx, z: wz, level: flevel });
                self.log(format!("⚡ teleported pid {pid} -> {wx},{wz} (plane {flevel})"));
            }
            return None;
        }
        // Single click -> nearest entity within ~2 tiles.
        if resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let (px, pz) = cursor_tile(pos);
                let mut best: Option<(Selection, i32)> = None;
                let mut consider = |sel: Selection, x: i32, z: i32| {
                    let d = (x - px).pow(2) + (z - pz).pow(2);
                    if best.is_none_or(|(_, bd)| d < bd) {
                        best = Some((sel, d));
                    }
                };
                for p in &snap.players { consider(Selection::Player(p.pid), p.x, p.z); }
                for n in &snap.npcs { consider(Selection::Npc(n.nid), n.x, n.z); }
                if let Some((sel, d)) = best {
                    if d <= 9 {
                        return Some(sel);
                    }
                }
            }
        }
        None
    }

    /// The whole-world composite map (one baked image, positioned correctly),
    /// with scroll-to-zoom + drag-to-pan, a region grid + entity dots. Click
    /// selects the nearest entity; double-click recenters. Returns a selection.
    fn world_map_canvas(&mut self, ui: &mut egui::Ui, view: &View) -> Option<Selection> {
        let snap = &view.snap;
        let Some(map) = view.worldmap.as_ref() else {
            ui.add_space(8.0);
            ui.weak("World map not baked yet.");
            return None;
        };
        // Upload the (large) image once and reuse the texture across frames.
        if self.world_tex.is_none() {
            self.world_tex = Some(pix_bridge::upload_rgb_linear(ui.ctx(), "worldmap", &map.image, map.w, map.h));
        }

        let (ox, oz, ex, ez) = map.bounds();
        let avail = egui::vec2(ui.available_width(), 380.0);
        let fit_zoom = (avail.x / map.w as f32).min(avail.y / map.h as f32);

        // Region-level zoom ≈ 1.5 regions tall — the default "zoomed into the
        // selected player's region" view.
        let region_zoom = (avail.y / 96.0).clamp(fit_zoom, fit_zoom * 60.0);
        let mut want_fit = false;
        let mut center_sel = false;
        ui.horizontal(|ui| {
            if ui.button("Fit world").clicked() { want_fit = true; }
            if ui.button("⌖ Selection").on_hover_text("Center + zoom on the selected entity").clicked() {
                center_sel = true;
            }
            ui.label(egui::RichText::new(format!("{:.0}%", self.world_zoom / fit_zoom * 100.0)).size(11.0).weak());
        });

        let (outer, resp) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());
        let img_size = |z: f32| egui::vec2(map.w as f32 * z, map.h as f32 * z);
        let img_px = |wx: i32, wz: i32| egui::vec2((wx - ox) as f32, ((ez - 1) - wz) as f32);
        let sel_region = self.selected_coord(snap).map(|(x, z)| (x >> 6, z >> 6));

        // First show -> default to the selected player's region, zoomed in.
        if self.world_zoom <= 0.0 {
            match self.selected_coord(snap).or_else(|| snap.players.first().map(|p| (p.x, p.z))) {
                Some((sx, sz)) => {
                    self.world_zoom = region_zoom;
                    self.world_pan = avail * 0.5 - img_px(sx, sz) * region_zoom;
                    self.world_centered_on = sel_region;
                }
                None => want_fit = true,
            }
        }
        // Follow the selection when it moves to a different region (unless the
        // user is mid-drag) — keeps the current zoom level.
        if !resp.dragged() && sel_region.is_some() && sel_region != self.world_centered_on {
            if let Some((sx, sz)) = self.selected_coord(snap) {
                self.world_pan = avail * 0.5 - img_px(sx, sz) * self.world_zoom;
                self.world_centered_on = sel_region;
            }
        }
        if want_fit {
            self.world_zoom = fit_zoom;
            self.world_pan = (avail - img_size(fit_zoom)) * 0.5;
            self.world_centered_on = None;
        }
        // Scroll-to-zoom about the cursor.
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if resp.hovered() && scroll != 0.0 {
            if let Some(cur) = ui.input(|i| i.pointer.hover_pos()) {
                let old = self.world_zoom.max(1e-6);
                let new = (old * (1.0 + scroll * 0.0015)).clamp(fit_zoom, fit_zoom * 60.0);
                let local = cur - outer.left_top() - self.world_pan; // image px * old
                self.world_pan += local - local * (new / old);
                self.world_zoom = new;
            }
        }
        if resp.dragged() {
            self.world_pan += resp.drag_delta();
        }
        if center_sel {
            if let Some((sx, sz)) = self.selected_coord(snap) {
                self.world_zoom = region_zoom;
                self.world_pan = avail * 0.5 - img_px(sx, sz) * region_zoom;
                self.world_centered_on = sel_region;
            }
        }

        // Clamp pan so the image can't drift entirely off-view.
        let z = self.world_zoom;
        let sz = img_size(z);
        let clamp1 = |p: f32, a: f32, s: f32| {
            let (lo, hi) = (f32::min(0.0, a - s), f32::max(0.0, a - s));
            p.clamp(lo, hi)
        };
        self.world_pan.x = clamp1(self.world_pan.x, avail.x, sz.x);
        self.world_pan.y = clamp1(self.world_pan.y, avail.y, sz.y);

        let origin = outer.left_top() + self.world_pan;
        let to_screen = |wx: i32, wz: i32| -> egui::Pos2 { origin + img_px(wx, wz) * z };

        let painter = ui.painter_at(outer);
        painter.rect_filled(outer, 2.0, egui::Color32::from_rgb(12, 13, 17));
        painter.image(
            self.world_tex.as_ref().unwrap().id(),
            egui::Rect::from_min_size(origin, sz),
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );

        // Detailed region tiles (walls/scenery/icons) overlaid when zoomed in —
        // the low-res overview is only a backdrop. Each region's 64×64 self sits
        // in the centre of its 104² build-area tile, so we sample uv [20..84]/104.
        if z * 64.0 > 110.0 {
            let max_rx = ex / 64 - 1;
            let max_ry = map.max_ry;
            let (min_rx, min_ry) = (map.min_rx, map.min_ry);
            // Visible region range from the viewport corners.
            let vis = |corner: egui::Pos2| {
                let ipx = (corner - origin) / z;
                (ox + ipx.x as i32, (ez - 1) - ipx.y as i32)
            };
            let (lx, tz_) = vis(outer.left_top());
            let (rxw, bz) = vis(outer.right_bottom());
            let rgx0 = (lx >> 6).clamp(min_rx, max_rx);
            let rgx1 = (rxw >> 6).clamp(min_rx, max_rx);
            let rgy0 = (bz >> 6).clamp(min_ry, max_ry);
            let rgy1 = (tz_ >> 6).clamp(min_ry, max_ry);
            const UV0: f32 = 20.0 / 104.0;
            const UV1: f32 = 84.0 / 104.0;
            if self.world_tile_tex.len() > 96 {
                self.world_tile_tex.clear();
            }
            for rgx in rgx0..=rgx1 {
                for rgy in rgy0..=rgy1 {
                    let key = (rgx as u32, rgy as u32);
                    let rrect = egui::Rect::from_two_pos(
                        to_screen(rgx * 64, (rgy + 1) * 64),
                        to_screen(rgx * 64 + 64, rgy * 64),
                    );
                    if !rrect.intersects(outer) {
                        continue;
                    }
                    // Detail tiles come from the pre-baked disk store (fast read,
                    // no World rebuild -> no lag). Upload to a texture once.
                    if !self.world_tile_tex.contains_key(&key) {
                        if let Some(store) = view.tiles.as_ref() {
                            if let Some((px, w)) = store.get(key.0, key.1) {
                                let tex = pix_bridge::upload_rgb_linear(ui.ctx(),
                                    &format!("tile_{}_{}", key.0, key.1), &px, w, w);
                                self.world_tile_tex.insert(key, tex);
                            }
                        }
                    }
                    if let Some(tex) = self.world_tile_tex.get(&key) {
                        painter.image(
                            tex.id(), rrect,
                            egui::Rect::from_min_max(egui::pos2(UV0, UV0), egui::pos2(UV1, UV1)),
                            egui::Color32::WHITE,
                        );
                    }
                }
            }
        }

        // Region grid (every 64 tiles) — only when zoomed in enough to read.
        if z * 64.0 > 14.0 {
            let grid = egui::Color32::from_white_alpha(24);
            let mut gx = ox;
            while gx <= ex {
                let x = to_screen(gx, oz).x;
                painter.vline(x, outer.y_range(), egui::Stroke::new(1.0, grid));
                gx += 64;
            }
            let mut gz = oz;
            while gz <= ez {
                let y = to_screen(ox, gz).y;
                painter.hline(outer.x_range(), y, egui::Stroke::new(1.0, grid));
                gz += 64;
            }
        }

        // Entity dots (npcs faint, players bright, selection ringed). Player
        // names label when zoomed in enough to be legible.
        let labels = z * 64.0 > 110.0;
        for n in &snap.npcs {
            painter.circle_filled(to_screen(n.x, n.z), 1.6, egui::Color32::from_rgb(90, 150, 230));
        }
        for p in &snap.players {
            let c = to_screen(p.x, p.z);
            painter.circle_filled(c, 2.8, egui::Color32::LIGHT_GREEN);
            if labels {
                painter.text(c + egui::vec2(5.0, -1.0), egui::Align2::LEFT_CENTER, &p.name,
                             egui::FontId::proportional(11.0), egui::Color32::from_white_alpha(230));
            }
        }
        if let Some((sx, sz2)) = self.selected_coord(snap) {
            painter.circle_stroke(to_screen(sx, sz2), 6.0, egui::Stroke::new(1.5, egui::Color32::YELLOW));
        }

        // Cursor -> world coord, for the readout + double-click teleport.
        let cursor_tile = |pos: egui::Pos2| -> (i32, i32) {
            let ipx = (pos - origin) / z;
            (ox + ipx.x.round() as i32, (ez - 1) - ipx.y.round() as i32)
        };

        // Hover tooltip: identify the entity nearest the cursor (within ~12px).
        if let Some(pos) = resp.hover_pos() {
            let mut best: Option<(String, egui::Color32, f32)> = None;
            let mut consider = |label: String, col: egui::Color32, at: egui::Pos2| {
                let d = at.distance(pos);
                if d < 12.0 && best.as_ref().is_none_or(|(_, _, bd)| d < *bd) {
                    best = Some((label, col, d));
                }
            };
            for p in &snap.players {
                consider(format!("{} (pid {})", p.name, p.pid), egui::Color32::LIGHT_GREEN, to_screen(p.x, p.z));
            }
            for n in &snap.npcs {
                let nm = npc_name(n.type_id).unwrap_or_else(|| format!("npc {}", n.type_id));
                consider(nm, egui::Color32::from_rgb(120, 170, 240), to_screen(n.x, n.z));
            }
            if let Some((label, col, _)) = best {
                let gp = egui::FontId::proportional(12.0);
                let galley = painter.layout_no_wrap(label, gp, col);
                let pad = egui::vec2(5.0, 3.0);
                let box_rect = egui::Rect::from_min_size(pos + egui::vec2(12.0, 12.0), galley.size() + pad * 2.0);
                painter.rect_filled(box_rect, 3.0, egui::Color32::from_black_alpha(210));
                painter.galley(box_rect.min + pad, galley, col);
            }
        }
        if let Some(pos) = resp.hover_pos() {
            let (wx, wz) = cursor_tile(pos);
            painter.text(
                outer.left_bottom() + egui::vec2(4.0, -3.0), egui::Align2::LEFT_BOTTOM,
                format!("{wx},{wz}  region {},{}", wx >> 6, wz >> 6),
                egui::FontId::monospace(11.0), egui::Color32::from_white_alpha(190),
            );
        }

        ui.label(egui::RichText::new(
            "drag = pan · scroll = zoom · dbl-click = teleport selected player",
        ).size(11.0).weak());

        // Double-click -> teleport the selected player to that tile (god move).
        if resp.double_clicked() {
            if let (Selection::Player(pid), Some(pos)) = (self.selected, resp.interact_pointer_pos()) {
                let (wx, wz) = cursor_tile(pos);
                let _ = self.cmd_tx.send(server::PanelCommand::Teleport { pid, x: wx, z: wz, level: 0 });
                self.log(format!("⚡ teleported pid {pid} -> {wx},{wz}"));
            }
            return None;
        }
        // Single click -> select nearest entity (tolerance scales with zoom).
        if resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let (wx, wz) = cursor_tile(pos);
                let mut best: Option<(Selection, i32)> = None;
                let mut consider = |sel: Selection, x: i32, zc: i32| {
                    let d = (x - wx).pow(2) + (zc - wz).pow(2);
                    if best.is_none_or(|(_, bd)| d < bd) {
                        best = Some((sel, d));
                    }
                };
                for p in &snap.players { consider(Selection::Player(p.pid), p.x, p.z); }
                for n in &snap.npcs { consider(Selection::Npc(n.nid), n.x, n.z); }
                let tol = ((8.0 / z) as i32).max(2);
                if let Some((sel, d)) = best {
                    if d <= tol * tol {
                        return Some(sel);
                    }
                }
            }
        }
        None
    }

    /// World-tile coord of the current selection, if any.
    fn selected_coord(&self, snap: &Snapshot) -> Option<(i32, i32)> {
        match self.selected {
            Selection::Player(pid) => snap.players.iter().find(|p| p.pid == pid).map(|p| (p.x, p.z)),
            Selection::Npc(nid) => snap.npcs.iter().find(|n| n.nid == nid).map(|n| (n.x, n.z)),
            Selection::None => None,
        }
    }

    fn right_map(&mut self, ctx: &egui::Context, view: &View) {
        let snap = &view.snap;
        let selected = self.selected;
        egui::SidePanel::right("world_map").resizable(true).default_width(360.0)
            .width_range(260.0..=620.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("World map").strong());
                ui.checkbox(&mut self.map_whole_world, "whole world");
            });
            let clicked: Option<Selection> = if self.map_whole_world {
                self.world_map_canvas(ui, view)
            } else {
                self.region_map_canvas(ui, view)
            };
            let _ = selected;
            if let Some(sel) = clicked {
                self.selected = sel;
            }

            ui.separator();
            ui.add(egui::TextEdit::singleline(&mut self.filter).desired_width(f32::INFINITY).hint_text("filter by name / id…"));
            let f = self.filter.trim().to_lowercase();
            let pmatch = |p: &PlayerRow| f.is_empty() || p.name.to_lowercase().contains(&f) || p.pid.to_string().contains(&f);
            let nmatch = |n: &NpcRow| f.is_empty() || n.nid.to_string().contains(&f) || n.type_id.to_string().contains(&f);
            let np = snap.players.iter().filter(|p| pmatch(p)).count();
            let nn = snap.npcs.iter().filter(|n| nmatch(n)).count();

            // Scroll the lists to the selection when it changed (e.g. picked on
            // the map / in 3D), without fighting manual scrolling.
            let scroll_to = self.scroll_sel != self.selected;
            ui.label(egui::RichText::new(format!("Players ({np})")).strong());
            egui::ScrollArea::vertical().id_salt("players").max_height(180.0).show(ui, |ui| {
                for p in snap.players.iter().filter(|p| pmatch(p)) {
                    let text = format!("{}  {}  ({}, {})", p.pid, p.name, p.x, p.z);
                    let sel = self.selected == Selection::Player(p.pid);
                    let r = ui.selectable_label(sel, text);
                    if r.clicked() {
                        self.selected = Selection::Player(p.pid);
                    }
                    if sel && scroll_to {
                        r.scroll_to_me(Some(egui::Align::Center));
                    }
                }
            });
            ui.label(egui::RichText::new(format!("NPCs ({nn})")).strong());
            egui::ScrollArea::vertical().id_salt("npcs").show(ui, |ui| {
                for n in snap.npcs.iter().filter(|n| nmatch(n)) {
                    let text = format!("{}  type {}  ({}, {})", n.nid, n.type_id, n.x, n.z);
                    let sel = self.selected == Selection::Npc(n.nid);
                    let r = ui.selectable_label(sel, text);
                    if r.clicked() {
                        self.selected = Selection::Npc(n.nid);
                    }
                    if sel && scroll_to {
                        r.scroll_to_me(Some(egui::Align::Center));
                    }
                }
            });
            self.scroll_sel = self.selected;
        });
    }

    /// Centre: the 3D admin scene of the selected player's region.
    fn central_scene(&mut self, ctx: &egui::Context, view: &View) {
        let snap = &view.snap;
        let selected = self.selected;
        egui::CentralPanel::default().show(ctx, |ui| {
            if !self.scene.enabled {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new("Enable “3D scene view” to render the world.").weak());
                });
                return;
            }
            // The scene follows whichever entity is selected — player OR npc.
            let focus = match selected {
                Selection::Player(pid) => snap.players.iter().find(|p| p.pid == pid).map(|p| (p.x, p.z, p.level, p.name.clone())),
                Selection::Npc(nid) => snap.npcs.iter().find(|n| n.nid == nid).map(|n| (n.x, n.z, n.level, format!("npc {}", n.type_id))),
                Selection::None => None,
            };
            let Some((fx, fz, flevel, fname)) = focus else {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new("Select a player or NPC to view its region in 3D.").weak());
                });
                return;
            };
            ui.horizontal(|ui| {
                if ui.button("⟲ Reset view").clicked() {
                    self.scene.reset_camera();
                }
                ui.separator();
                ui.label("Plane:");
                if ui.selectable_label(self.scene_view_level.is_none(), "auto").clicked() {
                    self.scene_view_level = None;
                }
                for l in 0..=3 {
                    if ui.selectable_label(self.scene_view_level == Some(l), l.to_string()).clicked() {
                        self.scene_view_level = Some(l);
                    }
                }
            });
            let level = self.scene_view_level.unwrap_or(flevel);
            ui.label(egui::RichText::new(format!(
                "3D scene · {} · coord {} : {}, {} · region {},{} · drag = orbit, scroll = zoom",
                fname, level, fx, fz, fx >> 6, fz >> 6
            )).strong());
            // Overlay every entity in this region (players named, npcs dots);
            // `picks` parallels `markers` so a click in 3D selects the entity.
            let region = (fx >> 6, fz >> 6);
            let mut markers = Vec::new();
            let mut picks: Vec<Selection> = Vec::new();
            for pl in &snap.players {
                if (pl.x >> 6, pl.z >> 6) == region {
                    let sel = self.selected == Selection::Player(pl.pid);
                    // 7 idk body parts -> the 12-slot worn appearance.
                    let mut worn = [0i32; 12];
                    for part in 0..7 {
                        if pl.body[part] >= 0 {
                            worn[client::dash3d::player_model::BASE_PART_MAP[part]] = 256 + pl.body[part];
                        }
                    }
                    markers.push(scene::Marker {
                        x: pl.x,
                        z: pl.z,
                        color: if sel { egui::Color32::YELLOW } else { egui::Color32::LIGHT_GREEN },
                        label: Some(pl.name.clone()),
                        kind: scene::MarkerKind::Player {
                            worn,
                            colours: pl.colours,
                            female: pl.gender == 1,
                            ready_anim: pl.ready_anim,
                            walk_anim: pl.walk_anim,
                            moving: pl.moving,
                        },
                    });
                    picks.push(Selection::Player(pl.pid));
                }
            }
            for n in &snap.npcs {
                if (n.x >> 6, n.z >> 6) == region {
                    let sel = self.selected == Selection::Npc(n.nid);
                    markers.push(scene::Marker {
                        x: n.x,
                        z: n.z,
                        color: if sel { egui::Color32::YELLOW } else { egui::Color32::from_rgb(90, 150, 230) },
                        label: None,
                        kind: scene::MarkerKind::Npc { type_id: n.type_id, moving: n.moving },
                    });
                    picks.push(Selection::Npc(n.nid));
                }
            }
            if let Some(i) = self.scene.show(ui, fx, fz, level, &markers) {
                if let Some(sel) = picks.get(i) {
                    self.selected = *sel;
                }
            }
        });
    }
}

/// Colour a tick-ms value by the 600ms budget.
fn budget_color(ms: f32) -> egui::Color32 {
    if ms > 600.0 {
        egui::Color32::RED
    } else if ms > 300.0 {
        egui::Color32::YELLOW
    } else {
        egui::Color32::LIGHT_GREEN
    }
}

/// Stacked frame-time graph: one bar per tick, segments [io, engine, output],
/// scaled to the peak (≥ 600ms budget), with a budget line + legend. Mirrors
/// the client's imgui perf overlay.
fn perf_graph(ui: &mut egui::Ui, hist: &VecDeque<PerfSample>, height: f32) {
    // Six non-overlapping phases, bottom->top, matching PerfSample::stack.
    const COLORS: [egui::Color32; 6] = [
        egui::Color32::from_rgb(120, 144, 156), // net in    — slate
        egui::Color32::from_rgb(126, 87, 194),  // world     — purple
        egui::Color32::from_rgb(239, 154, 154), // npcs      — rose
        egui::Color32::from_rgb(130, 199, 132), // players   — green
        egui::Color32::from_rgb(79, 195, 247),  // info pkts — sky
        egui::Color32::from_rgb(255, 167, 38),  // net out   — amber
    ];
    const NAMES: [&str; 6] = ["net in", "world", "npcs", "players", "info pkts", "net out"];
    const SCRIPT_COLOR: egui::Color32 = egui::Color32::from_rgb(255, 238, 88); // yellow
    const BUDGET: f32 = 600.0; // 600ms = one server tick

    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, egui::Color32::from_white_alpha(8));

    let peak = hist.iter().map(|s| s.total()).fold(BUDGET * 1.1, f32::max);
    let scale = rect.height() / peak;
    let bw = (rect.width() / TICK_HISTORY as f32).max(1.0);
    for (i, s) in hist.iter().enumerate() {
        let x = rect.left() + i as f32 * bw;
        let mut y = rect.bottom();
        for k in 0..6 {
            let h = s.stack[k] * scale;
            if h <= 0.0 {
                continue;
            }
            painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x, y - h), egui::pos2(x + bw, y)), 0.0, COLORS[k]);
            y -= h;
        }
        // RuneScript overlay: a tick mark at its (cross-cutting) height so its
        // share of the tick is visible without distorting the stack.
        let sh = s.scripts * scale;
        if sh > 0.0 {
            let sy = rect.bottom() - sh;
            painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x, sy - 1.0), egui::pos2(x + bw, sy + 1.0)), 0.0, SCRIPT_COLOR);
        }
    }
    let by = rect.bottom() - BUDGET * scale;
    painter.hline(rect.x_range(), by, egui::Stroke::new(1.0, egui::Color32::from_white_alpha(110)));
    painter.text(
        egui::pos2(rect.right() - 4.0, by - 2.0),
        egui::Align2::RIGHT_BOTTOM,
        "600ms budget",
        egui::FontId::proportional(10.0),
        egui::Color32::from_white_alpha(140),
    );

    // Legend with per-series averages (+ scripts).
    let mut avg = [0.0f32; 6];
    let mut script_avg = 0.0f32;
    if !hist.is_empty() {
        for s in hist {
            for k in 0..6 {
                avg[k] += s.stack[k];
            }
            script_avg += s.scripts;
        }
        let n = hist.len() as f32;
        for v in &mut avg {
            *v /= n;
        }
        script_avg /= n;
    }
    ui.horizontal_wrapped(|ui| {
        for k in 0..6 {
            ui.colored_label(COLORS[k], "■");
            ui.label(egui::RichText::new(format!("{} {:.2}ms", NAMES[k], avg[k])).size(11.0));
        }
        ui.colored_label(SCRIPT_COLOR, "▬");
        ui.label(egui::RichText::new(format!("runescript {script_avg:.2}ms")).size(11.0));
    });
}

/// Ordered boot stages emitted by the server (server::run `stage!` calls),
/// with display labels — drives the splash progress bar + checklist.
const BOOT_STAGES: [(&str, &str); 6] = [
    ("packing cache", "Packing Content -> cache"),
    ("verifying cache", "Verifying cache CRCs vs vanilla"),
    ("compiling scripts", "Compiling server scripts"),
    ("loading scripts", "Loading RuneScript"),
    ("loading map", "Loading world map + collision"),
    ("listening", "Listening for connections"),
];

/// Startup splash: branding + a determinate progress bar + a per-step checklist.
fn splash(ctx: &egui::Context, state: &PanelState) {
    let cur = BOOT_STAGES.iter().position(|(k, _)| *k == state.stage);
    let done = cur.map_or(0, |i| i); // steps fully completed = index of current
    let frac = (done as f32 + 0.5) / BOOT_STAGES.len() as f32;
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.18);
            ui.heading(egui::RichText::new("OS").size(64.0).strong());
            ui.label(egui::RichText::new("Server Control Panel").size(20.0).weak());
            ui.add_space(28.0);

            // Fatal: the server halted before listening (script compile error,
            // cache pack/verify failure, bind error, …). Show it instead of
            // spinning forever. script_error carries the nicer per-line detail.
            let fatal = state.script_error.as_deref().map(|e| ("script compile failed", e))
                .or_else(|| state.boot_error.as_deref().map(|e| ("startup failed", e)));
            if let Some((what, err)) = fatal {
                ui.colored_label(egui::Color32::from_rgb(255, 90, 90),
                    egui::RichText::new(format!("⛔ Server {what}")).size(18.0).strong());
                ui.add_space(8.0);
                let w = (ui.available_width() * 0.7).clamp(360.0, 720.0);
                ui.allocate_ui_with_layout(egui::vec2(w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    for line in err.lines() {
                        ui.label(egui::RichText::new(line).monospace().size(12.0)
                            .color(egui::Color32::from_rgb(255, 170, 170)));
                    }
                });
                ui.add_space(10.0);
                ui.label(egui::RichText::new("Fix the issue above, then relaunch.").weak());
                return;
            }

            let bar_w = (ui.available_width() * 0.5).clamp(280.0, 460.0);
            ui.allocate_ui_with_layout(
                egui::vec2(bar_w, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.add(egui::ProgressBar::new(frac).animate(true)
                        .text(format!("{} / {}", done.min(BOOT_STAGES.len()), BOOT_STAGES.len())));
                    ui.add_space(14.0);
                    for (i, (_, label)) in BOOT_STAGES.iter().enumerate() {
                        let (glyph, col) = match cur {
                            Some(c) if i < c => ("✔", egui::Color32::from_rgb(130, 199, 132)),
                            Some(c) if i == c => ("▶", egui::Color32::from_rgb(255, 213, 79)),
                            _ => ("○", egui::Color32::from_white_alpha(90)),
                        };
                        ui.horizontal(|ui| {
                            ui.colored_label(col, glyph);
                            ui.label(egui::RichText::new(*label).size(14.0)
                                .color(if cur.is_some_and(|c| i <= c) { egui::Color32::from_white_alpha(230) } else { egui::Color32::from_white_alpha(120) }));
                            if cur == Some(i) {
                                ui.add(egui::Spinner::new().size(14.0));
                            }
                        });
                        // The pack + verify steps get their own progress bars.
                        if cur == Some(i) {
                            let bar = match BOOT_STAGES[i].0 {
                                "packing cache" if state.pack_total > 0 => Some((state.pack_done, state.pack_total)),
                                "verifying cache" if state.verify_total > 0 => Some((state.verify_done, state.verify_total)),
                                _ => None,
                            };
                            if let Some((d, t)) = bar {
                                ui.add(egui::ProgressBar::new(d as f32 / t as f32)
                                    .desired_height(10.0)
                                    .text(format!("{d} / {t} groups")));
                            }
                        }
                    }

                    // Whole-world map bake (only meaningful on first boot / after
                    // the maps change; a cache hit flips straight to ready).
                    ui.add_space(14.0);
                    let (done, total, ready) = (state.map_bake_done, state.map_bake_total, state.map_bake_ready);
                    let (glyph, col) = if ready {
                        ("✔", egui::Color32::from_rgb(130, 199, 132))
                    } else {
                        ("▶", egui::Color32::from_rgb(255, 213, 79))
                    };
                    ui.horizontal(|ui| {
                        ui.colored_label(col, glyph);
                        ui.label(egui::RichText::new("Baking world map").size(14.0));
                        if !ready {
                            ui.add(egui::Spinner::new().size(14.0));
                        }
                    });
                    if !ready && total > 0 {
                        ui.add(egui::ProgressBar::new(done as f32 / total as f32)
                            .text(format!("{done} / {total} regions")));
                    }
                },
            );
        });
    });
}

fn kv(ui: &mut egui::Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(key).weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(value));
    });
}

/// Aggregate analytics over the live snapshot + perf window: tick stability,
/// entity activity, and the busiest NPC types / regions.
fn analytics(ui: &mut egui::Ui, view: &View) {
    let snap = &view.snap;

    // Tick stability over the rolling window.
    let hist = &view.perf_hist;
    if !hist.is_empty() {
        let n = hist.len() as f32;
        let totals: Vec<f32> = hist.iter().map(|s| s.total()).collect();
        let avg = totals.iter().sum::<f32>() / n;
        let max = totals.iter().cloned().fold(0.0, f32::max);
        let over = totals.iter().filter(|&&t| t > 600.0).count();
        kv(ui, "Tick avg", &format!("{avg:.1} ms"));
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Tick max").weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(format!("{max:.1} ms")).color(budget_color(max)));
            });
        });
        kv(ui, "Over budget", &format!("{over}/{} ticks", hist.len()));
    }

    // Entity activity.
    let moving_p = snap.players.iter().filter(|p| p.moving).count();
    let moving_n = snap.npcs.iter().filter(|n| n.moving).count();
    kv(ui, "Players moving", &format!("{moving_p}/{}", snap.players.len()));
    kv(ui, "NPCs moving", &format!("{moving_n}/{}", snap.npcs.len()));

    // Busiest NPC types.
    if !snap.npcs.is_empty() {
        let mut counts: std::collections::HashMap<i32, usize> = std::collections::HashMap::new();
        for n in &snap.npcs {
            *counts.entry(n.type_id).or_default() += 1;
        }
        let mut top: Vec<(i32, usize)> = counts.into_iter().collect();
        top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Top NPC types").weak().size(11.0));
        for (id, count) in top.into_iter().take(6) {
            let name = npc_name(id);
            kv(ui, &name.unwrap_or_else(|| format!("type {id}")), &format!("×{count}"));
        }
    }

    // Player distribution by region.
    if snap.players.len() > 1 {
        let mut regions: std::collections::HashMap<(i32, i32), usize> = std::collections::HashMap::new();
        for p in &snap.players {
            *regions.entry((p.x >> 6, p.z >> 6)).or_default() += 1;
        }
        if regions.len() > 1 {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(format!("Players across {} regions", regions.len())).weak().size(11.0));
        }
    }
}

/// Best-effort NPC config details: (name, combat level, size, right-click ops).
fn npc_info(id: i32) -> Option<(String, i32, i32, Vec<String>)> {
    std::panic::catch_unwind(|| {
        let t = client::config::npc_type::list(id);
        let name = {
            let n = t.name.trim();
            if n.is_empty() || n.eq_ignore_ascii_case("null") { format!("type {id}") } else { n.to_string() }
        };
        let ops: Vec<String> = t.op.iter().flatten()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        (name, t.vislevel, t.size.max(1), ops)
    })
    .ok()
}

/// Best-effort NPC display name from the installed config (None if unavailable).
fn npc_name(id: i32) -> Option<String> {
    std::panic::catch_unwind(|| {
        let t = client::config::npc_type::list(id);
        let name = t.name.trim();
        if name.is_empty() || name.eq_ignore_ascii_case("null") {
            None
        } else {
            Some(format!("{name} ({id})"))
        }
    })
    .ok()
    .flatten()
}

fn graph(ui: &mut egui::Ui, id: &str, hist: &VecDeque<f32>, include_top: f32) {
    let points: PlotPoints = hist.iter().enumerate().map(|(i, &v)| [i as f64, v as f64]).collect();
    Plot::new(id)
        .height(140.0)
        .include_y(0.0)
        .include_y(include_top as f64)
        .show_axes([false, true])
        .show(ui, |plot_ui| plot_ui.line(Line::new(points)));
}

fn fmt_uptime(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}h {m}m {s}s")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}
