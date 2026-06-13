// Boots the CLIENT crate inside jaged the same way the game boots:
// one init point that loads every archive into the global loader
// registry, runs the full step-60 install block (configs, textures,
// anims, fonts), and claims the audio device — so every view finds the
// client subsystems ready instead of lazy-initialising piecemeal.
//
// The client populates its loaders from the network; here each loader
// gets its raw index (decode_index decompresses internally) plus every
// packed group straight from disk. Disk groups carry a 2-byte version
// trailer the net format lacks — stripped before insert (the index
// CRCs cover the trailer-less bytes too).

use std::sync::atomic::Ordering;
use std::sync::Mutex;

use client::sound::pcm_player::PcmPlayer;

/// Regions present in the open cache, discovered once at init by name-
/// hash probing (m{x}_{y}). The maps browser lists THESE — one entry
/// per region — instead of the raw m/l group-id pairs.
pub static REGIONS: Mutex<Vec<(u32, u32)>> = Mutex::new(Vec::new());

/// Everything the views need, built once at cache-open. Mirrors the
/// client's own boot: loaders → installs → audio → per-frame tick.
pub struct ClientSystems {
    /// cpal stream + SharedManager (the in-game midi2 stack). None only
    /// if the audio device failed; the error is surfaced once.
    pub player: Option<PcmPlayer>,
    pub audio_error: Option<String>,
    pub loaders_installed: usize,
    /// A headless `Client` — the same struct the game runs, holding the
    /// p11/p12/b12 fonts and the per-tick `world_update_num` the real
    /// interface renderer + model-animation path read. Views call the
    /// client's own `draw_interface` / `animate_interface` through it so
    /// jaged renders exactly what the game does. Boxed (it's large).
    pub client: Box<client::client::Client>,
    /// LOOP_CYCLE value at the last interface animate, for the per-frame
    /// tick delta `animate_interface` advances model frames by.
    pub last_if_cycle: i32,
}

/// Archive ids double as loader slots (the same scheme the client's
/// openJs5 uses): anims=0 bases=1 config=2 interfaces=3 jagFX=4 maps=5
/// songs=6 models=7 sprites=8 textures=9 binary=10 jingles=11
/// scripts=12 fontMetrics=13 vorbis=14 patches=15.
const ANIMS: i32 = 0;
const BASES: i32 = 1;
const CONFIG: i32 = 2;
const INTERFACES: i32 = 3;
const MODELS: i32 = 7;
const SPRITES: i32 = 8;
const TEXTURES: i32 = 9;
const FONT_METRICS: i32 = 13;

pub fn init(cache: &mut cache::Cache) -> ClientSystems {
    let loaders_installed = install_local_loaders(cache);

    // Region discovery for the maps browser: probe every plausible
    // m{x}_{y} name hash against the maps index (cheap HashMap hits).
    {
        let index = cache.index(cache::MAPS_ARCHIVE);
        let mut regions = Vec::new();
        for x in 0u32..100 {
            for y in 0u32..256 {
                let hash = io::cp1252::name_hash(&format!("m{x}_{y}"));
                if index.find_group_by_hash(hash).is_some() {
                    regions.push((x, y));
                }
            }
        }
        *REGIONS.lock().unwrap() = regions;
    }

    // The client's step-60 install block (client.rs:8259-8277) —
    // configs, textures, anim frame sets, minimap/overlay sprites.
    client::config::if_type::install_archives(INTERFACES, SPRITES, FONT_METRICS, MODELS);
    client::config::loc_type::install_archives(CONFIG, MODELS);
    client::config::obj_type::install_archives(CONFIG, MODELS);
    client::config::flo_type::install_archives(CONFIG);
    client::config::flu_type::install_archives(CONFIG);
    client::dash3d::texture_manager::install_archives(TEXTURES, SPRITES);
    client::config::seq_type::install_archives(CONFIG, ANIMS, BASES);
    client::dash3d::anim_frame_set::install_archives(ANIMS, BASES);
    client::config::npc_type::install_archives(CONFIG, MODELS);
    client::config::varp_type::install_archives(CONFIG);
    client::config::var_bit_type::install_archives(CONFIG);
    client::config::enum_type::install_archives(CONFIG);
    client::config::inv_type::install_archives(CONFIG);
    client::config::idk_type::install_archives(CONFIG, MODELS);
    client::config::spot_type::install_archives(CONFIG, MODELS);
    client::minimap::install(SPRITES);
    client::overlays::install(SPRITES);

    // Audio up-front like the game's startCommon — views just play.
    let (player, audio_error) = match PcmPlayer::init(22050, true) {
        Ok(p) => (Some(p), None),
        Err(e) => (None, Some(format!("audio init failed: {e}"))),
    };

    // Headless Client + the three interface fonts (client step 50:
    // PixLoader.makePixFont over the sprites + fontMetrics loaders).
    // Without these, draw_interface renders no text.
    let mut client = Box::new(client::client::Client::new());
    {
        let mut reg = client::js5::js5_net::LOADERS.lock().unwrap();
        let (sprites_idx, fm_idx) = (SPRITES as usize, FONT_METRICS as usize);
        if sprites_idx < reg.len() && fm_idx < reg.len() {
            // Disjoint split so both loaders borrow mutably at once.
            let (lo, hi) = reg.split_at_mut(fm_idx);
            if let (Some(sl), Some(fl)) = (lo[sprites_idx].as_mut(), hi[0].as_mut()) {
                client.p11 = client::graphics::pix_loader::make_pix_font(sl, fl, "p11_full", "");
                client.p12 = client::graphics::pix_loader::make_pix_font(sl, fl, "p12_full", "");
                client.b12 = client::graphics::pix_loader::make_pix_font(sl, fl, "b12_full", "");
            }
        }
    }

    ClientSystems { player, audio_error, loaders_installed, client, last_if_cycle: 0 }
}

/// Standard OSRS game canvas the interface coordinate space targets.
pub const GAME_W: i32 = 765;
pub const GAME_H: i32 = 503;

/// Render one interface group exactly as the game does: advance its model
/// animations by the ticks elapsed since the last call, then run the
/// client's own `draw_interface` into a scratch Pix2D buffer and hand the
/// pixels back (0x00RRGGBB, GAME_W×GAME_H) for the view to upload. Drawing
/// covers the full game canvas — a group has no inherent size (it's opened
/// into a parent container whose dimensions aren't knowable from the group
/// alone), so clipping to any guessed "root bounds" would crop interfaces
/// that assume the full frame (banks, etc.). All engine setup lives in
/// `init`; this is pure per-frame render.
pub fn render_interface(sys: &mut ClientSystems, group: i32) -> Vec<i32> {
    use client::graphics::pix2d;

    const SLOT: i32 = INTERFACES;

    // Animate at the wall-clock 50Hz cadence the bridge tick drives:
    // advance by the LOOP_CYCLE ticks elapsed since our last render.
    let cycle = client::scene::LOOP_CYCLE.load(Ordering::Relaxed);
    let delta = (cycle - sys.last_if_cycle).max(0);
    sys.last_if_cycle = cycle;

    // open_interface (called inside animate_interface) decodes the group
    // into the STORE; animate advances type-6 model frames + spin.
    let subs: std::collections::HashMap<i32, client::client::SubInterface> =
        std::collections::HashMap::new();
    sys.client.world_update_num = delta;
    let mut visited = std::collections::HashSet::new();
    client::client::animate_interface(&sys.client, SLOT, group, &subs, &mut visited);

    // Bind a fresh scratch buffer so draw_interface paints into our image,
    // not whatever was last bound; restore the previous binding after.
    let scratch = vec![0i32; (GAME_W * GAME_H) as usize];
    let prev = pix2d::swap_pixels(scratch, GAME_W, GAME_H);
    pix2d::set_clipping(0, 0, GAME_W, GAME_H);

    // p11 cloned so the immutable font borrow doesn't collide with the
    // &mut Client draw_interface also wants (Client owns the font field).
    let p11 = sys.client.p11.clone();
    client::interface_render::draw_interface(
        group, 0, 0, GAME_W, GAME_H, 0, 0,
        p11.as_ref(), &subs, &mut Some(sys.client.as_mut()),
    );

    // Swap our drawn buffer back out, restoring the prior binding.
    let (drawn, _, _) = pix2d::swap_pixels(prev.0, prev.1, prev.2);
    drawn
}

/// Centre offset that places the 64×64 region in the middle of the
/// 104×104 client world, so the orbit camera has symmetric room on
/// every side and never clamps the eye at a corner. The region's
/// centre tile then sits at (BORDER+32, BORDER+32).
pub const REGION_BORDER: i32 = (104 - 64) / 2; // 20
/// World tile the region centre lands on — the camera pivot.
pub const REGION_PIVOT_TILE: i32 = REGION_BORDER + 32; // 52

/// Build the client scene World for one region's raw map bytes, placed
/// CENTRED in the 104×104 world (local tiles BORDER..BORDER+64). Heights
/// match the 2D `Region::decode` exactly — the perlin default-fill uses
/// the same region-base seed offset (region_x*64 + 932731), corrected
/// for the border so local tile BORDER maps to absolute region origin.
/// Returns the built world plus the centre-tile ground height (camera
/// pivot Y). Locs resolve their models through the bridged loaders
/// during finish_build, same as in-game.
pub fn build_region_world(
    rx: u32,
    ry: u32,
    land: &[u8],
    locs: Option<&[u8]>,
) -> (client::dash3d::world::World, i32) {
    use client::client_build;
    use client::dash3d::pix3d;

    let b = REGION_BORDER;
    client_build::init();
    // perlin_off so local tile `b` reads absolute region origin:
    //   load_ground uses (local + 932731 + perlin_off); we want
    //   (local - b) + region*64, i.e. perlin_off = region*64 - b.
    client_build::load_ground(land, b, b, rx as i32 * 64 - b, ry as i32 * 64 - b, None);
    if let Some(locs) = locs {
        client_build::load_locations(locs, b, b);
    }

    let ground_h = client_build::STATE.lock().unwrap().ground_h.clone();
    let pivot_y = ground_h[0][REGION_PIVOT_TILE as usize][REGION_PIVOT_TILE as usize];
    let mut world = client::dash3d::world::World::new(4, 104, 104, ground_h);
    world.fill_base_level(0);

    // RecalcCameraFrustumTileVisibility heights (scene::ensure_world_built).
    let sin = pix3d::sin_table();
    let mut heights = [0i32; 9];
    for (i, slot) in heights.iter_mut().enumerate() {
        let pitch = (i as i32) * 32 + 128 + 15;
        let dist = pitch * 3 + 600;
        *slot = (dist * sin[pitch as usize]) >> 16;
    }
    world.reset_vis_calc(&heights, 500, 800, 512, 334);
    client_build::finish_build(&mut world, None, false, 0);
    (world, pivot_y)
}

/// Per-frame game loop, the jaged analogue of the client mainloop tick:
/// midi fade/load state machine + animated-texture advance. Returns
/// true while audio is live so the caller keeps frames coming.
pub fn tick(sys: &ClientSystems, cycle: i32, tick_delta: i32) -> bool {
    // LOOP_CYCLE is the MONOTONIC game clock the animated-loc temp
    // closures read (advance_loc_anim takes per-loc deltas off it) —
    // torches/fires animate off this. Driven from wall-clock 50Hz by
    // the caller so anim speed is independent of repaint rate.
    client::scene::LOOP_CYCLE.store(cycle, std::sync::atomic::Ordering::Relaxed);
    // run_anims takes the per-FRAME tick DELTA (Java's worldUpdateNum,
    // reset to 0 each frame — Client.java:2799): animate() shifts texels
    // cumulatively, so it must advance by exactly the ticks elapsed this
    // frame, not the absolute cycle.
    if tick_delta > 0 {
        client::dash3d::texture_manager::run_anims(tick_delta);
    }
    if let Some(p) = sys.player.as_ref() {
        let manager = p.manager();
        let mut mgr = manager.lock();
        mgr.update_fade_out();
        mgr.try_advance_loading();
        return true;
    }
    false
}

/// Idempotent: archives already installed are left alone. Returns how
/// many loaders were (newly) installed.
fn install_local_loaders(cache: &mut cache::Cache) -> usize {
    let mut installed = 0usize;
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
        let mut loader =
            client::js5::js5_loader::Js5Loader::new(archive as i32, false, false, false);
        loader.base.decode_index(&index_raw);
        for gid in 0..loader.base.packed.len() {
            if let Ok(Some(raw)) = cache.read_raw(archive, gid as u32) {
                let end = raw.len().saturating_sub(2);
                loader.base.packed[gid] = Some(raw[..end].to_vec());
            }
        }
        loader.load_status.store(true, Ordering::SeqCst);
        reg[archive as usize] = Some(Box::new(loader));
        installed += 1;
    }
    installed
}
