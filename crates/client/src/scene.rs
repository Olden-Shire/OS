// jagex3.dash3d scene renderer. Hooks into the IfType type-5 component
// with `clientCode == 1337`.
//
// Renders a perspective 3D heightmap of the parsed `client_build::STATE`
// terrain. Each tile becomes 2 triangles; vertex positions come from
// `ground_h` and colour from FloType (resolved via floor_t1/floors).
// Real loc model rendering still pending ModelLit + objRender.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::client_build;
use crate::config::{flo_type, flu_type, loc_type};
use crate::dash3d::{ground::Ground, model_lit::ModelLit, model_unlit::ModelUnlit, pix3d};
use crate::graphics::pix2d;
use crate::js5::js5_net;

// custom — Lit-model cache keyed by ((loc_id << 5) | shape). Java's
// equivalents are LocType.modelCacheNormal / modelCacheTransform
// (`ey.mc1` / `ey.mc2`) which are LRU(500) / LRU(30). We use a plain
// HashMap until the LRU port lands.
pub static MODEL_CACHE: std::sync::LazyLock<Mutex<HashMap<i32, Arc<ModelLit>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

// Per-loc persistent animation state, keyed by (level, tile_x, tile_z,
// loc_id). Java's `ClientLocAnim` holds `animFrame` and `animCycle` per
// instance and advances them from `Client.loopCycle` each render via
// `ClientLocAnim.getTempModel`. We previously stubbed this with a
// deterministic phase_seed → loc-id hash, which works for cyclic anims
// (idle torch flicker) but breaks one-shot anims (door swing): the door
// would re-trigger every frame instead of stopping after one play.
//
// `finished` flag mirrors Java's `this.anim = null` after the inner
// `do/while` falls through — once a one-shot anim has played, we render
// the base model from then on. The varbit/varp swap normally happens
// server-side around the same time, so the next frame picks up the
// "open door" multiloc child geometry.
#[derive(Debug, Clone)]
struct LocAnimState {
    anim_id: i32,
    anim_frame: i32,
    anim_cycle: i32,
    finished: bool,
}
static LOC_ANIM_STATE: std::sync::LazyLock<Mutex<HashMap<(i32, i32, i32, i32), LocAnimState>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

// Per-instance lit-model cache for sharelight locs. Keyed by
// (level, tile_x, tile_z, resolved_id, kind, rotation). Java's
// World.shareLight pairs adjacent loc models and sums their per-vertex
// normals so the gouraud corner colours agree across the seam — the
// resulting lit model depends on the loc's NEIGHBOURS, not just its
// own (id, kind, rotation), so we cache per instance.
//
// `gen` mirrors `state.locs.len()` (poor-man's generation counter — the
// scene rebuild replaces the whole list, bumping the count). When `gen`
// changes we drop the cache and re-run the sharelight pass on the new
// scene.
struct SharelightCache {
    generation: usize,
    by_instance: HashMap<(i32, i32, i32, i32, i32, i32), Arc<ModelLit>>,
}
static SHARELIT_CACHE: std::sync::LazyLock<Mutex<SharelightCache>> =
    std::sync::LazyLock::new(|| Mutex::new(SharelightCache { generation: usize::MAX, by_instance: HashMap::new() }));

// Per-level occluder lists, rebuilt when the scene changes (same
// `state.locs.len()` generation as the sharelight cache). Java's
// World.occluders is similarly per-level; calcOcclude picks the
// camera-level slice per frame.
struct OccluderCache {
    generation: usize,
    per_level: Vec<Vec<crate::dash3d::occlude::Occlude>>,
}
static OCCLUDER_CACHE: std::sync::LazyLock<Mutex<OccluderCache>> =
    std::sync::LazyLock::new(|| Mutex::new(OccluderCache { generation: usize::MAX, per_level: Vec::new() }));

// Drop all per-loc anim state. Hooked into the ClientBuild reset path so
// loading a new map doesn't leave us holding state for locs that don't
// exist any more.
pub fn clear_loc_anim_state() {
    LOC_ANIM_STATE.lock().unwrap().clear();
}

// custom — JS5 slot for the models archive. Java holds it on
// LocType.models / ObjType.models / NpcType.models / IfType.models.
const MODELS_SLOT: i32 = 7;

// custom — Camera state piped over from Client.update_orbit_camera
// every frame. Java keeps the equivalents as Client.gw/gn/gj/gk/gx
// statics (camX/camY/camZ/camPitch/camYaw) — we mirror the orbit
// values into a Mutex so the renderer can read them without a Client
// borrow.
pub struct Camera {
    pub yaw: i32,    // mirrors orbitCameraYaw, 0..2047
    pub pitch: i32,  // mirrors orbitCameraPitch, 128..383
    // Camera-to-player orbit radius. Java's mouse-wheel zoom adjusts
    // the camera distance (cameraDistance) rather than the focal
    // length. Default 1100 puts the camera at OSRS's standard
    // top-down angle.
    pub distance: i32,
}
impl Camera {
    pub const fn new() -> Self {
        Self { yaw: 0, pitch: 256, distance: 1100 }
    }
    pub fn update(&mut self, yaw: i32, pitch: i32, distance: i32) {
        self.yaw = yaw;
        self.pitch = pitch;
        self.distance = distance;
    }
}

// Java's hard-coded focal length — `<< 9` in
// `(view_X << 9) / view_Z + originX`. The texture-mapping math uses
// pixel-space X_offsets, so this MUST stay 512 or UV will ramp at the
// wrong rate and textures will wrap multiple times across a face.
pub const FOCAL_LENGTH: i32 = 512;
pub static CAMERA: Mutex<Camera> = Mutex::new(Camera::new());

// @ObfuscatedName("client.bw") — Client.loopCycle mirror. Used by the
// animation system to pick the current keyframe; ticks once per
// mainloop iteration. We expose it here so the scene render can read
// it without holding a borrow on `Client`.
pub static LOOP_CYCLE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

// Java's ClientBuild.hueOff / ligOff (lines 101 / 105) — random
// per-build offsets applied to FluType / FloType HSL when computing the
// minimap colour. The result is that two adjacent scenes (e.g. before
// and after a region reload) get subtly different minimap tints,
// matching OSRS's "fresh-build feel". We use fixed mid-range values for
// determinism (no PRNG in our render path); the visual goal is just to
// produce the jittered palette lookup rather than the raw FluType.colour
// that pre-finishBuild paths used.
pub static MINIMAP_HUE_OFF: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-4);
pub static MINIMAP_LIG_OFF: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-8);

// Local player tile coords (`ClientPlayer.x` / `ClientPlayer.z`).
// Camera pivots around this position. Updated by Client whenever the
// player teleports / walks. Packed into a single AtomicU64 (X in the
// low 32 bits, Z in the high 32) so paired reads can't tear into
// (old_x, new_z) — visible as one-frame jitter on the camera and
// minimap when the player moves across a tile boundary.
// Sentinel value `u64::MAX` (both halves all-1) means "no player yet,
// fall back to map centre".
pub static PLAYER_TILE_XZ: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(u64::MAX);

pub fn store_player_tile(x: i32, z: i32) {
    let packed = ((x as u32 as u64)) | ((z as u32 as u64) << 32);
    PLAYER_TILE_XZ.store(packed, std::sync::atomic::Ordering::Relaxed);
}

pub fn load_player_tile() -> (i32, i32) {
    let packed = PLAYER_TILE_XZ.load(std::sync::atomic::Ordering::Relaxed);
    if packed == u64::MAX { return (-1, -1); }
    let x = (packed & 0xFFFF_FFFF) as u32 as i32;
    let z = ((packed >> 32) & 0xFFFF_FFFF) as u32 as i32;
    (x, z)
}

// @ObfuscatedName("ey.getModel") — port of LocType.getModel +
// buildModel. Picks the right base model for the (kind, rotation) pair,
// bakes in mirror + rotation (rotate90/180/270) + recolour/retexture +
// resize + offset, then runs the smooth-Gouraud light pass. We key the
// cache by (id, kind, rotation) so each rotated variant is a separate
// ModelLit, matching Java's mc2 cache keyed by `(id << 10) | (kind << 3) | rot`.
// One-shot occluder build for the current scene. Walks state.locs to
// populate the mapo bit grid (mirroring Java's ClientBuild.addLoc per-
// kind/rotation OR pattern at lines 526/545/553/561/569/614-624), then
// runs the run-length walker (occlude::build_occluders, port of
// finishBuild lines 982-1117) to convert connected runs of bits into
// AABB occluders.
//
// This is independent of sharelight; both are cached against the same
// `state.locs.len()` generation but live in separate statics for
// clarity. Cost is one-time per scene rebuild.
fn ensure_occluders_built() {
    let target_gen = client_build::STATE.lock().unwrap().locs.len();
    {
        let cache = OCCLUDER_CACHE.lock().unwrap();
        if cache.generation == target_gen { return; }
    }

    // mapo[level][x][z] — u16 to fit the 0..12 bit positions used by the
    // run-length walker. Indexed 0..=104 to match Java's tile grid.
    let mut mapo: Vec<Vec<Vec<u16>>> = (0..4).map(|_| {
        (0..=104).map(|_| vec![0u16; 105]).collect()
    }).collect();

    let state = client_build::STATE.lock().unwrap();
    // Snapshot locs + groundh so we can drop the state lock before the
    // heavy run-length walk.
    let locs_snapshot: Vec<_> = state.locs.iter().map(|l| (l.level, l.x, l.z, l.id, l.rotation, l.kind)).collect();
    let groundh: Vec<Vec<Vec<i32>>> = state.ground_h.iter().map(|lvl| {
        lvl.iter().map(|c| c.to_vec()).collect()
    }).collect();
    drop(state);

    // Populate mapo from the loc list. The bit constants 0x249 / 0x492 /
    // 0x924 each set the corresponding direction bit at ALL four output
    // levels — so a wall at level 0 contributes to occluders at levels
    // 0, 1, 2, 3 if the run extends upward.
    let or_bits = |mapo: &mut Vec<Vec<Vec<u16>>>, lvl: i32, x: i32, z: i32, bits: u16| {
        if lvl < 0 || lvl >= 4 || x < 0 || x > 104 || z < 0 || z > 104 { return; }
        mapo[lvl as usize][x as usize][z as usize] |= bits;
    };
    for (level, lx, lz, id, rotation, kind) in &locs_snapshot {
        let Some(lt) = loc_type::list(*id) else { continue };
        let lt = if lt.multiloc.is_some() {
            if let Some(c) = lt.get_multi_loc() { c } else { continue }
        } else { lt };
        let r = rotation & 0x3;
        match *kind {
            0 => {
                if lt.occlude {
                    match r {
                        0 => or_bits(&mut mapo, *level, *lx, *lz, 0x249),
                        1 => or_bits(&mut mapo, *level, *lx, *lz + 1, 0x492),
                        2 => or_bits(&mut mapo, *level, *lx + 1, *lz, 0x249),
                        3 => or_bits(&mut mapo, *level, *lx, *lz, 0x492),
                        _ => {}
                    }
                }
            }
            2 => {
                if lt.occlude {
                    match r {
                        0 => {
                            or_bits(&mut mapo, *level, *lx, *lz, 0x249);
                            or_bits(&mut mapo, *level, *lx, *lz + 1, 0x492);
                        }
                        1 => {
                            or_bits(&mut mapo, *level, *lx, *lz + 1, 0x492);
                            or_bits(&mut mapo, *level, *lx + 1, *lz, 0x249);
                        }
                        2 => {
                            or_bits(&mut mapo, *level, *lx + 1, *lz, 0x249);
                            or_bits(&mut mapo, *level, *lx, *lz, 0x492);
                        }
                        3 => {
                            or_bits(&mut mapo, *level, *lx, *lz, 0x492);
                            or_bits(&mut mapo, *level, *lx, *lz, 0x249);
                        }
                        _ => {}
                    }
                }
            }
            k if (12..=17).contains(&k) && k != 13 && *level > 0 => {
                or_bits(&mut mapo, *level, *lx, *lz, 0x924);
            }
            _ => {}
        }
    }

    // Run the run-length walker to produce per-level occluder lists.
    let per_level = crate::dash3d::occlude::build_occluders(&mut mapo, &groundh);

    let mut cache = OCCLUDER_CACHE.lock().unwrap();
    cache.generation = target_gen;
    cache.per_level = per_level;
}

// One-shot sharelight build for the current scene. Walks all locs with
// `LocType.sharelight = true`, decodes their ModelUnlit instances, then
// pair-sums normals between adjacent ones via ModelUnlit::share_light.
// Finally lights each and stores the result by instance key.
//
// Java does this once at the end of ClientBuild.finishBuild via
// World.shareLight → World.shareLightLoc, which walks neighbours in a
// 3x3 surrounding area (+1 in X & Z, both same-level and one-level-up
// for bridge handling). The shared normal sum produces a smooth
// gouraud transition across the seam between e.g. two fence segments.
//
// Cost: O(N × 9) pair calls where N is the number of sharelight locs in
// view, each pair O(P²) inside `share_light`. On a typical scene with
// ~500 sharelight locs and ~80 points each this is ~30M ops — one-time
// at scene rebuild, well under a second.
fn ensure_sharelight_scene_built() {
    let target_gen = client_build::STATE.lock().unwrap().locs.len();
    {
        let cache = SHARELIT_CACHE.lock().unwrap();
        if cache.generation == target_gen { return; }
    }

    // Collect (level, x, z, loc_id, kind, rotation, resolved_lt, model_id, mirror)
    // for every sharelight loc. We mirror Java's setDecor / setWall
    // variants by enumerating the same (kind, rotation) remap pairs
    // add_loc_draws uses, since share_light needs ALL of them to be
    // present and adjacent for the pair-up to find them.
    type LocKey = (i32, i32, i32, i32, i32, i32);
    let mut units: HashMap<LocKey, ModelUnlit> = HashMap::new();
    let mut light_args: HashMap<LocKey, (i32, i32)> = HashMap::new();
    let mut y_anchor: HashMap<LocKey, i32> = HashMap::new();

    let state = client_build::STATE.lock().unwrap();
    let locs_snapshot: Vec<_> = state.locs.iter().map(|l| (l.level, l.x, l.z, l.id, l.rotation, l.kind)).collect();
    // ground_h snapshot for the shareLight Y offset calculation.
    let groundh = state.ground_h.clone();
    drop(state);

    for (level, lx, lz, id, rotation, src_kind) in &locs_snapshot {
        // Enumerate the model variants this loc emits (same as
        // add_loc_draws — kinds 2/8 emit two models).
        let r4 = rotation & 0x3;
        let variants: Vec<(i32, i32)> = match src_kind {
            0 | 1 | 3 | 4 | 9 | 10 | 22 => vec![(*src_kind, *rotation)],
            2 => vec![(2, rotation + 4), (2, (r4 + 1) & 0x3)],
            5 => vec![(4, *rotation)],
            6 => vec![(4, rotation + 4)],
            7 => vec![(4, ((r4 + 2) & 0x3) + 4)],
            8 => vec![(4, rotation + 4), (4, ((r4 + 2) & 0x3) + 4)],
            11 => vec![(10, *rotation)],
            k if *k >= 12 => vec![(*k, *rotation)],
            _ => vec![(*src_kind, *rotation)],
        };

        for (kind, rot) in variants {
            let Some((lt, model_id, mirror)) = resolve_loc_model(*id, kind, rot) else { continue };
            if !lt.sharelight { continue; }
            let resolved_id = lt.id;
            let Some(mut un) = build_loc_unlit(&lt, model_id, kind, rot, mirror) else { continue };
            un.calc_bounding_cube();
            un.calculate_normals();

            let key: LocKey = (*level, *lx, *lz, resolved_id, kind, rot);
            let ambient = (lt.ambient + 64).max(0);
            let contrast = (lt.contrast + 768).max(1);
            // Anchor Y: average of the four corner heights — same as
            // add_loc_draws's h_val. shareLight uses this to compute the
            // Y offset between paired models living on different tiles.
            let l = *level as usize;
            let lx_u = (*lx).clamp(0, 103) as usize;
            let lz_u = (*lz).clamp(0, 103) as usize;
            let h_avg = (
                groundh[l][lx_u + 1][lz_u]
                + groundh[l][lx_u][lz_u]
                + groundh[l][lx_u][lz_u + 1]
                + groundh[l][lx_u + 1][lz_u + 1]
            ) / 4;

            units.insert(key, un);
            light_args.insert(key, (ambient, contrast));
            y_anchor.insert(key, h_avg);
        }
    }

    // Pair-up pass. For each unit, look for neighbours on the 3x3
    // surrounding tiles + same-level above (Java's shareLightLoc
    // iterates `var12 = arg1; var12 <= arg1 + 1`). On the level-up
    // pass `var8--` extends the search window by one tile to handle
    // bridge geometry that straddles two levels.
    //
    // We mark already-paired (a, b) so we don't double-process the
    // mirror call (share_light updates BOTH sides in one go).
    let keys: Vec<LocKey> = units.keys().copied().collect();
    let key_set: std::collections::HashSet<LocKey> = keys.iter().copied().collect();
    let mut processed: std::collections::HashSet<(LocKey, LocKey)> = std::collections::HashSet::new();

    // Helper: split units so we can mutably borrow two distinct entries.
    // HashMap doesn't support that natively, so we drain into Options
    // keyed by key, mutate, then re-insert at the end.
    let mut workspace: HashMap<LocKey, Option<ModelUnlit>> = units.drain().map(|(k, v)| (k, Some(v))).collect();

    for a_key in &keys {
        let (a_level, ax, az, _a_id, _a_kind, _a_rot) = *a_key;
        for dx in -1..=1i32 {
            for dz in -1..=1i32 {
                if dx == 0 && dz == 0 {
                    // Same tile — pair different (id/kind/rot) entries.
                } else if (dx != 0) && (dz != 0) {
                    // Diagonal — Java's shareLightLoc skips these for
                    // the same-tile inner pass (the `var7` flag); only
                    // the same-row / same-column tiles pair.
                    continue;
                }
                let bx = ax + dx;
                let bz = az + dz;
                // Try each candidate B key sharing the same tile.
                for b_key in &keys {
                    let (b_level, bx2, bz2, _b_id, _b_kind, _b_rot) = *b_key;
                    if b_level != a_level || bx2 != bx || bz2 != bz { continue; }
                    if a_key == b_key { continue; }
                    let pair = if a_key < b_key { (*a_key, *b_key) } else { (*b_key, *a_key) };
                    if processed.contains(&pair) { continue; }
                    if !key_set.contains(a_key) || !key_set.contains(b_key) { continue; }

                    // Pull both models out simultaneously, do the pair,
                    // and put them back. Two separate HashMap.remove
                    // calls leave us holding owned values that don't
                    // alias each other in safe Rust.
                    let Some(mut a_un) = workspace.get_mut(a_key).and_then(|o| o.take()) else { continue };
                    let Some(mut b_un) = workspace.get_mut(b_key).and_then(|o| o.take()) else {
                        workspace.insert(*a_key, Some(a_un));
                        continue;
                    };

                    let y_off = y_anchor[a_key] - y_anchor[b_key];
                    ModelUnlit::share_light(&mut a_un, &mut b_un, dx * 128, y_off, dz * 128, false);

                    *workspace.get_mut(a_key).unwrap() = Some(a_un);
                    *workspace.get_mut(b_key).unwrap() = Some(b_un);
                    processed.insert(pair);
                }
            }
        }
    }

    // Light each and insert into the cache.
    let mut cache = SHARELIT_CACHE.lock().unwrap();
    cache.by_instance.clear();
    cache.generation = target_gen;
    for (key, slot) in workspace.iter_mut() {
        let Some(un) = slot.take() else { continue };
        let (ambient, contrast) = light_args[key];
        let lit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ModelLit::from_unlit_flat(&un, ambient, contrast, -50, -10, -50)
        })).ok();
        if let Some(lit) = lit {
            cache.by_instance.insert(*key, Arc::new(lit));
        }
    }
}

// Resolve multiloc and pick the correct model id + mirror flag for
// (kind, rotation). Returns (resolved_loc_type, model_id, mirror) so
// callers can apply transforms + lighting consistently. Used both by
// `fetch_loc_model`'s normal path and the sharelight scene-build pass.
fn resolve_loc_model(loc_id: i32, kind: i32, rotation: i32) -> Option<(crate::config::loc_type::LocType, i32, bool)> {
    let lt_initial = loc_type::list(loc_id)?;
    let lt = if lt_initial.multiloc.is_some() {
        lt_initial.get_multi_loc()?
    } else {
        lt_initial
    };
    let models = lt.model.as_ref()?;
    let (model_id, mirror_xor) = match &lt.shape {
        Some(shape_list) => {
            let mut chosen = None;
            for (i, &s) in shape_list.iter().enumerate() {
                if s == kind {
                    chosen = Some(models.get(i).copied().unwrap_or(0));
                    break;
                }
            }
            (chosen?, rotation > 3)
        }
        None => {
            if kind != 10 { return None; }
            (*models.first()?, false)
        }
    };
    if model_id <= 0 { return None; }
    let mirror = lt.mirror ^ mirror_xor;
    Some((lt, model_id, mirror))
}

// Build the un-lit model for (loc, kind, rotation) — mirror, rotate,
// recolour, retexture, resize, translate. Stops short of `light()` so
// `share_light` can stitch normals across adjacent locs before the
// final lighting pass.
fn build_loc_unlit(lt: &crate::config::loc_type::LocType, model_id: i32, kind: i32, rotation: i32, mirror: bool) -> Option<ModelUnlit> {
    let bytes = {
        let mut reg = js5_net::LOADERS.lock().unwrap();
        let loader = reg.get_mut(MODELS_SLOT as usize).and_then(|o| o.as_mut())?;
        loader.fetch_file(model_id & 0xFFFF, 0)?
    };
    let lt = lt.clone();
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let mut un = ModelUnlit::from_bytes(bytes);
        if mirror { un.mirror(); }
        if kind == 4 && rotation > 3 {
            un.rotate_x_axis(256);
            un.translate(45, 0, -45);
        }
        let r3 = rotation & 0x3;
        if r3 == 1 { un.rotate90(); }
        else if r3 == 2 { un.rotate180(); }
        else if r3 == 3 { un.rotate270(); }
        for i in 0..lt.recol_s.len() {
            un.recolour(lt.recol_s[i], lt.recol_d[i]);
        }
        for i in 0..lt.retex_s.len() {
            un.retexture(lt.retex_s[i], lt.retex_d[i]);
        }
        if lt.resizex != 128 || lt.resizey != 128 || lt.resizez != 128 {
            un.resize(lt.resizex, lt.resizey, lt.resizez);
        }
        if lt.offsetx != 0 || lt.offsety != 0 || lt.offsetz != 0 {
            un.translate(lt.offsetx, lt.offsety, lt.offsetz);
        }
        un
    })).ok()
}

fn fetch_loc_model(loc_id: i32, kind: i32, rotation: i32, anim_frame: Option<i32>, instance: Option<(i32, i32, i32)>) -> Option<Arc<ModelLit>> {
    // Resolve multiloc + pick model id once for both the normal and
    // sharelight paths.
    let (lt, model_id, mirror) = resolve_loc_model(loc_id, kind, rotation)?;
    let resolved_id = lt.id;
    let anim = lt.anim;

    // Sharelight locs (fences, hedges, palisade walls — anything with
    // `LocType.sharelight = true`) go through a per-instance cache that
    // World.shareLight populates by pair-summing normals between
    // neighbouring loc models. Without per-instance caching the shared
    // normals would be wrong: an identical fence at a different tile
    // has different neighbours, so the summed normals differ.
    if lt.sharelight {
        if let Some((lvl, lx, lz)) = instance {
            ensure_sharelight_scene_built();
            let key = (lvl, lx, lz, resolved_id, kind, rotation);
            let arc = {
                let c = SHARELIT_CACHE.lock().unwrap();
                c.by_instance.get(&key).map(Arc::clone)
            };
            if let Some(arc) = arc {
                if let (true, Some(frame_idx)) = (anim >= 0, anim_frame) {
                    if let Some(animated) = animate_loc_model(&arc, anim, frame_idx) {
                        return Some(animated);
                    }
                }
                return Some(arc);
            }
            // Cache miss after build — the loc wasn't visible to the
            // sharelight pass for some reason; fall through to the
            // per-type cache as a safe fallback.
        }
    }

    let cache_key = (resolved_id << 10) | ((kind & 0x1F) << 3) | (rotation & 0x7);
    if let Some(arc) = {
        let c = MODEL_CACHE.lock().unwrap();
        c.get(&cache_key).map(Arc::clone)
    } {
        if let (true, Some(frame_idx)) = (anim >= 0, anim_frame) {
            if let Some(animated) = animate_loc_model(&arc, anim, frame_idx) {
                return Some(animated);
            }
        }
        return Some(arc);
    }

    let un = build_loc_unlit(&lt, model_id, kind, rotation, mirror)?;
    let ambient = (lt.ambient + 64).max(0);
    let contrast = (lt.contrast + 768).max(1);
    let lit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ModelLit::from_unlit_flat(&un, ambient, contrast, -50, -10, -50)
    })).ok()?;
    let arc = Arc::new(lit);
    MODEL_CACHE.lock().unwrap().insert(cache_key, Arc::clone(&arc));
    if let (true, Some(frame_idx)) = (anim >= 0, anim_frame) {
        if let Some(animated) = animate_loc_model(&arc, anim, frame_idx) {
            return Some(animated);
        }
    }
    Some(arc)
}

// @ObfuscatedName("eo.v(Lfo;IIB)Lfo;") — SeqType.animateModel90, but
// rotation is already baked into the model so we skip the pre-rotate.
//
// Takes an explicit frame index (computed by advance_loc_anim from the
// per-loc cycle state), clones the base model, applies the AnimFrame,
// returns Arc.
fn animate_loc_model(base: &Arc<ModelLit>, anim_id: i32, frame_idx: i32) -> Option<Arc<ModelLit>> {
    use crate::config::seq_type;
    use crate::dash3d::anim_frame_set;
    let seq = seq_type::list(anim_id);
    let frames = seq.frames.as_ref()?;
    if frames.is_empty() { return None; }
    let idx = (frame_idx as usize).min(frames.len() - 1);
    let raw_frame = *frames.get(idx)?;
    let frameset_id = raw_frame >> 16;
    let frame_id = raw_frame & 0xFFFF;
    let fs = anim_frame_set::get(frameset_id)?;
    let mut animated = (**base).clone();
    animated.animate(&fs, frame_id);
    Some(Arc::new(animated))
}

// Per-loc anim advance. Mirrors `ClientLocAnim.getTempModel` (line 67-89
// of the Java source): walks the loopCycle delta forward through the
// SeqType's per-frame delay table, advancing `anim_frame` past every
// expired delay, then saves the residual into `anim_cycle` so the next
// call picks up where this one left off.
//
// Returns:
//   Some(frame_idx) — render this frame
//   None           — anim finished (one-shot done) OR not animated;
//                    caller renders the base model. Java's "anim = null"
//                    fallthrough has the same effect.
//
// `loops` is Java's wrap-back amount. -1 means "play once" (after the
// final frame, `anim_frame - (-1) = anim_frame + 1` overshoots and the
// outer do/while exits → finished). Positive values reset the frame
// pointer back into range each time it overflows (cyclic anim).
pub fn advance_loc_anim(level: i32, lx: i32, lz: i32, id: i32, anim_id: i32, loop_cycle: i32) -> Option<i32> {
    use crate::config::seq_type;
    if anim_id < 0 { return None; }
    let seq = seq_type::list(anim_id);
    let frames = seq.frames.as_ref()?;
    let delays = seq.delay.as_ref()?;
    if frames.is_empty() || delays.is_empty() { return None; }
    let frame_count = frames.len() as i32;

    let key = (level, lx, lz, id);
    let mut store = LOC_ANIM_STATE.lock().unwrap();
    let st = store.entry(key).or_insert_with(|| {
        // First time we see this loc. Java's ClientLocAnim constructor
        // sets animCycle = loopCycle - 1 (so the first render advances
        // by exactly 1 cycle's worth) and animFrame = 0. For looping
        // anims (`loops != -1`) it then randomises both so adjacent
        // torches don't tick in lockstep — we use a deterministic hash
        // of the loc key for the same desync effect without an RNG.
        let mut anim_frame = 0i32;
        let mut anim_cycle = loop_cycle - 1;
        if seq.loops != -1 {
            // Stable hash from the loc key → pick an initial frame and
            // back-date the cycle by a fraction of that frame's delay.
            let h = (id.wrapping_mul(2654435761u32 as i32))
                ^ (lx.wrapping_mul(83492791u32 as i32))
                ^ (lz.wrapping_mul(2246822519u32 as i32))
                ^ (level.wrapping_mul(374761393u32 as i32));
            let hu = h as u32;
            anim_frame = (hu % frame_count as u32) as i32;
            let d = *delays.get(anim_frame as usize).unwrap_or(&1).max(&1);
            let back = (hu / frame_count as u32) as i32 % d.max(1);
            anim_cycle -= back;
        }
        LocAnimState { anim_id, anim_frame, anim_cycle, finished: false }
    });
    // Multiloc swap (varbit changed → different SeqType): reset state so
    // the new anim starts from the top rather than playing at the old
    // cycle's mid-point.
    if st.anim_id != anim_id {
        st.anim_id = anim_id;
        st.anim_frame = 0;
        st.anim_cycle = loop_cycle - 1;
        st.finished = false;
    }
    if st.finished {
        return None;
    }
    let mut var1 = loop_cycle - st.anim_cycle;
    if var1 > 100 && seq.loops > 0 {
        var1 = 100;
    }
    let loops = seq.loops;
    let mut anim_frame = st.anim_frame;
    'outer: loop {
        loop {
            let delay = *delays.get(anim_frame as usize).unwrap_or(&1);
            if var1 <= delay {
                break 'outer;
            }
            var1 -= delay;
            anim_frame += 1;
            if anim_frame >= frame_count {
                break;
            }
        }
        anim_frame -= loops;
        if anim_frame < 0 || anim_frame >= frame_count {
            st.finished = true;
            return None;
        }
    }
    st.anim_frame = anim_frame;
    st.anim_cycle = loop_cycle - var1;
    Some(anim_frame)
}

// IfType.clientCode dispatch values used in Client.drawLayer to swap
// the normal type-5 component render for the gameplay viewport /
// minimap. The values 1337 / 1338 are hardcoded inline in Java
// (Client.java:10158, 10166); we hoist them to constants for the
// renderer's dispatch.
pub const CLIENT_CODE_VIEWPORT: i32 = 1337;
pub const CLIENT_CODE_MINIMAP: i32 = 1338;

// custom — Java uses 128 inline everywhere for the tile-to-world ratio
// (one tile = 128 subtile-world units, since SUBTILE_GRID = 8).
const TILE_UNIT: i32 = 128;

// @ObfuscatedName("Client.minimapDraw") dispatch — the IfType
// clientCode 1338 component. The full minimap subsystem (512x512
// render2DGround image, rotation/zoom blit, dots, arrows, flag,
// compass, mask tables) lives in crate::minimap; this shim keeps the
// interface renderer's call site stable. Java passes only the
// component top-left; width/height come from the mapback sprite.
pub fn draw_minimap(x: i32, y: i32, _w: i32, _h: i32) {
    crate::minimap::draw(x, y);
}

// The built World for the current scene, rebuilt when ClientBuild
// finishes a new map (same `state.locs.len()` generation convention
// as the other scene caches). Java equivalent: `Client.world`, built
// once by ClientBuild.finishBuild per REBUILD.
pub struct WorldCache {
    pub generation: usize,
    pub world: Option<crate::dash3d::world::World>,
}
pub static WORLD_CACHE: std::sync::LazyLock<Mutex<WorldCache>> =
    std::sync::LazyLock::new(|| Mutex::new(WorldCache { generation: usize::MAX, world: None }));

// Drop the built scene so the next frame rebuilds from the new map
// data. Java's startRebuild replaces `Client.world` outright; we
// invalidate and let ensure_world_built reconstruct once the assets
// are ready.
pub fn invalidate_world() {
    let mut cache = WORLD_CACHE.lock().unwrap();
    cache.generation = usize::MAX;
    cache.world = None;
}

// The colour bake in finishBuild reads FluType / FloType HSL fields
// and overlay textures ONCE — Java's loading steps guarantee those
// archives streamed in before finishBuild runs. Our config fetches
// are lazy per-id, so the build must wait until every floor id the
// map references actually decodes; baking earlier freezes get_table(0,
// 0, 0) = palette-index-0 BLACK into every tile permanently. Each
// probe also queues the JS5 fetch, so this self-heals within a few
// frames of login.
fn scene_assets_ready() -> bool {
    let (flu_ids, flo_ids) = {
        let state = client_build::STATE.lock().unwrap();
        let mut flu = std::collections::HashSet::new();
        let mut flo = std::collections::HashSet::new();
        for level in 0..4usize {
            for x in 0..104usize {
                for z in 0..104usize {
                    let t1 = state.floor_t1[level][x][z] as i32 & 0xFF;
                    if t1 > 0 {
                        flu.insert(t1 - 1);
                    }
                    let t2 = state.floor_t2[level][x][z] as i32 & 0xFF;
                    if t2 > 0 {
                        flo.insert((t2 - 1) & 0xFF);
                    }
                }
            }
        }
        (flu, flo)
    };
    let mut ready = true;
    for id in flu_ids {
        if !flu_type::is_loaded(id) {
            ready = false;
        }
    }
    for id in flo_ids {
        if !flo_type::is_loaded(id) {
            ready = false;
        }
        // Overlay textures feed both the tile rasterizer and the
        // baked minimap colour (getAverageRgb) — require their texels.
        let fl = flo_type::list(id);
        if fl.texture >= 0
            && crate::dash3d::texture_manager::get_texels(fl.texture).is_none()
        {
            ready = false;
        }
    }
    ready
}

// Build the World scene graph from the decoded map state. Mirrors
// Java Client loadingStep 20 (resetVisCalc with the per-pitch camera
// height table, Client.java:1692-1700) + ClientBuild.finishBuild.
fn ensure_world_built() {
    let target_gen = client_build::STATE.lock().unwrap().locs.len();
    if target_gen == 0 {
        return;
    }
    {
        let cache = WORLD_CACHE.lock().unwrap();
        if cache.generation == target_gen && cache.world.is_some() {
            return;
        }
    }
    // Wait for the floor configs + overlay textures the bake reads;
    // retry next frame while JS5 streams them in.
    if !scene_assets_ready() {
        return;
    }
    let ground_h = client_build::STATE.lock().unwrap().ground_h.clone();
    let mut world = crate::dash3d::world::World::new(4, 104, 104, ground_h);
    world.fill_base_level(0);
    // RecalcCameraFrustumTileVisibility — per-pitch camera heights.
    let sin = pix3d::sin_table();
    let mut heights = [0i32; 9];
    for (i, slot) in heights.iter_mut().enumerate() {
        let pitch = (i as i32) * 32 + 128 + 15;
        let dist = pitch * 3 + 600;
        *slot = (dist * sin[pitch as usize]) >> 16;
    }
    world.reset_vis_calc(&heights, 500, 800, 512, 334);
    client_build::finish_build(&mut world, None, false, 0);
    eprintln!("[scene] world built: {} locs, floor configs + textures ready", target_gen);
    // The minimap bakes from the same Ground colour fields — force its
    // 512×512 image to rebuild against the fresh world.
    crate::minimap::MINIMAP.lock().unwrap().minimap_level = -1;
    let mut cache = WORLD_CACHE.lock().unwrap();
    cache.generation = target_gen;
    cache.world = Some(world);
}

pub fn draw_viewport(x: i32, y: i32, w: i32, h: i32) {
    pix2d::set_clipping(x, y, x + w, y + h);
    pix3d::set_clipping(x, y, x + w, y + h);
    // Reset trans — previous frame's loc render may have left it
    // non-zero, which would bleed translucency into the sky / tiles
    // / player avatar this frame.
    pix3d::set_trans(0);

    // Sky gradient backdrop.
    let split = h * 2 / 3;
    for row in 0..split {
        let t = row * 256 / split.max(1);
        let r = 96 + (170 - 96) * t / 256;
        let g = 132 + (198 - 132) * t / 256;
        let b = 196 + (226 - 196) * t / 256;
        pix2d::fill_rect(x, y + row, w, 1, (r << 16) | (g << 8) | b);
    }

    ensure_world_built();
    let state = client_build::STATE.lock().unwrap();
    // Player tile coords — Client mirrors the local ClientPlayer x/z
    // into the packed atomic each tick. Falls back to map centre when
    // the player hasn't been placed yet.
    let (raw_x, raw_z) = load_player_tile();
    let lp_x = if raw_x < 0 { 54 } else { raw_x.clamp(0, 103) };
    let lp_z = if raw_z < 0 { 54 } else { raw_z.clamp(0, 103) };

    // Bridge handling: Java's ClientBuild.finishBuild walks every tile
    // with `mapl[1] & 0x2` set and calls `World.pushDown` to shift
    // level-1's geometry into level-0's render slot. We pre-extract a
    // bridge mask up-front so closures don't keep a long-lived borrow
    // of State (which we need to drop mid-frame for the loc render).
    let bridge_mask: Vec<Vec<bool>> = (0..104).map(|x| {
        (0..104).map(|z| (state.mapl[1][x][z] & 0x2) != 0).collect()
    }).collect();
    let tile_level = |x: i32, z: i32| -> usize {
        if x < 0 || x >= 104 || z < 0 || z >= 104 { return 0; }
        if bridge_mask[x as usize][z as usize] { 1 } else { 0 }
    };
    // Java's `Client.lastBuiltLevel` is 0 for the default world; the
    // bridge bit gets baked into per-tile sampling via `tile_level`
    // instead of a global level constant.

    // Live camera from input — orbit yaw + pitch + distance.
    let cam = CAMERA.lock().unwrap();
    let cam_yaw = cam.yaw;
    let cam_pitch = cam.pitch;
    let cam_distance = cam.distance;
    drop(cam);
    let cam_zoom = FOCAL_LENGTH;

    // Camera orbits around the player at `cam_distance` units. Mouse-
    // wheel scroll adjusts the distance; focal length stays at 512 so
    // the texture rasterizer's pixel-space arithmetic is unaffected.
    let orbit_radius = cam_distance;
    let sin = crate::dash3d::pix3d::sin_table();
    let cos = crate::dash3d::pix3d::cos_table();
    // Java yaw runs 0..2047; sin/cos tables are 2048 entries already.
    let yaw_idx = (cam_yaw & 0x7FF) as usize;
    let pitch_idx = (cam_pitch & 0x7FF) as usize;
    // Camera world position — proper spherical orbit around the pivot
    // at (player_x, player_ground_y, player_z).
    //
    // The XZ orbit ring shrinks with pitch (cos_pitch factor): at
    // pitch=0 the camera is at full `radius` distance in XZ; at
    // pitch=90° it sits directly above the pivot with zero XZ offset.
    // Without that factor the camera traced a V instead of a sphere
    // (XZ distance stayed constant while only Y changed with pitch).
    //
    // For yaw direction conventions: the projection rotation maps a
    // point at +Z (world north) to view_x = sin(yaw) when yaw > 0,
    // which means yaw=0 → look NORTH, yaw increasing → look CCW from
    // above. So for the player to stay in front of camera:
    //   yaw=0    → camera SOUTH  of pivot  (cam_z = pivot_z − r)
    //   yaw=90°  → camera EAST   of pivot  (cam_x = pivot_x + r)
    //   yaw=180° → camera NORTH  of pivot
    //   yaw=270° → camera WEST   of pivot
    let pivot_x = lp_x * TILE_UNIT + 64;
    let pivot_z = lp_z * TILE_UNIT + 64;
    let pivot_y = state.ground_h[0][lp_x as usize][lp_z as usize];
    let sin_yaw = sin[yaw_idx];
    let cos_yaw = cos[yaw_idx];
    let sin_pitch = sin[pitch_idx];
    let cos_pitch = cos[pitch_idx];
    // Horizontal radius after pitch projection.
    let h_radius = (orbit_radius * cos_pitch) >> 16;
    let cam_world_x = pivot_x + ((h_radius * sin_yaw) >> 16);
    let cam_world_z = pivot_z - ((h_radius * cos_yaw) >> 16);
    let cam_world_y = pivot_y - ((orbit_radius * sin_pitch) >> 16);
    drop(state);

    // Arm the scene mouse pick (Java gameDrawMain 4156-4167): models
    // hovering under the cursor push their typecodes into MOUSE_PICK
    // during renderAll; the menu build consumes them after the draw.
    {
        let (mx, my) = {
            let m = crate::input::MOUSE.lock().unwrap();
            (m.mouse_x, m.mouse_y)
        };
        let mut p = crate::dash3d::model_lit::MOUSE_PICK.lock().unwrap();
        p.picked.clear();
        if mx >= x && mx < x + w && my >= y && my < y + h {
            p.mouse_check = true;
            // Our projection works in absolute screen coords (origin
            // at the viewport centre, absolute), so the pick point
            // stays absolute too — Java subtracts the viewport origin
            // because its Pix3D origin is viewport-relative.
            p.mouse_x = mx;
            p.mouse_y = my;
        } else {
            p.mouse_check = false;
        }
    }

    // Hand the frame to the 1:1 World renderer (renderAll -> fill).
    // Java: Client.gameDrawMain line 4173 `world.renderAll(camX, camY,
    // camZ, camPitch, camYaw, topLevel)`. Top level fixed at 3 until
    // the roof-check port (Client.roofCheck2) wires minusedlevel.
    // Entities (players/NPCs/projectiles/spotanims) were pushed into
    // the sprite grid by push_entities; Java removes them right after
    // renderAll (Client.java:4176).
    {
        let mut cache = WORLD_CACHE.lock().unwrap();
        if let Some(world) = cache.world.as_mut() {
            world.render_all(cam_world_x, cam_world_y, cam_world_z,
                             cam_pitch.clamp(128, 383), cam_yaw & 0x7FF, 3);
            world.remove_sprites();
        }
    }
    pix3d::set_trans(0);
    let _ = (pivot_x, pivot_y, pivot_z);

    // Publish the camera this frame rendered with, bump sceneCycle,
    // and run the post-scene overlay pass (Java gameDrawMain order:
    // renderAll → entityOverlays → coordArrow → otherOverlays).
    crate::overlays::set_frame_camera(cam_world_x, cam_world_y, cam_world_z,
                                      cam_pitch.clamp(128, 383), cam_yaw & 0x7FF);
    crate::overlays::SCENE_CYCLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    crate::overlays::draw(x, y, w, h);
}

// ══════════════════════════════════════════════════════════════════
// Entity push — Java gameDrawMain 4096-4101: addPlayers(true),
// addNpcs(true), addPlayers(false), addNpcs(false), addProjectiles(),
// addMapAnim(). Runs once per frame before renderAll; the dynamic
// sprites are removed again right after the render.
// ══════════════════════════════════════════════════════════════════

pub fn push_entities(c: &mut crate::client::Client) {
    let mut cache = WORLD_CACHE.lock().unwrap();
    let Some(world) = cache.world.as_mut() else { return; };
    let scene_cycle = crate::overlays::SCENE_CYCLE
        .load(std::sync::atomic::Ordering::Relaxed);

    push_players(c, world, scene_cycle, true);
    push_npcs(c, world, scene_cycle, true);
    push_players(c, world, scene_cycle, false);
    push_npcs(c, world, scene_cycle, false);
    push_projectiles(c, world);
    push_map_anims(c, world);
}

// @ObfuscatedName("dl.dn(ZI)V") — Client.addPlayers. Verbatim port of
// Client.java:4205-4250. `local` draws only the local player (with
// the reserved 2047<<14 typecode); otherwise the tracked-player list.
fn push_players(c: &mut crate::client::Client,
                world: &mut crate::dash3d::world::World,
                scene_cycle: i32, local: bool) {
    use crate::dash3d::model_source::ModelSource;

    let level = c.minusedlevel;
    let loop_cycle = c.loop_cycle;
    let player_count = c.player_count;

    // Java 4206-4208 — arriving at the flagged tile clears the flag.
    if local {
        if let Some(lp) = c.local_player.as_ref() {
            if lp.entity.x >> 7 == c.minimap_flag_x && lp.entity.z >> 7 == c.minimap_flag_z {
                c.minimap_flag_x = 0;
            }
        }
    }

    let count = if local { 1 } else { c.player_count as usize };
    for i in 0..count {
        let (typecode, pid) = if local {
            (0x1ffc000, -1)
        } else {
            let pid = c.player_ids[i];
            (pid << 14, pid)
        };

        // Immutable pre-pass: position, readiness, crowd LOD inputs.
        let snapshot = {
            let player = if local {
                c.local_player.as_ref()
            } else {
                c.players.get(pid as usize).and_then(|o| o.as_ref())
            };
            let Some(p) = player else { continue; };
            if !p.ready() {
                continue;
            }
            (p.entity.x, p.entity.z,
             p.entity.secondary_seq_id == p.entity.readyanim,
             p.loc_model.is_some(),
             p.loc_start_cycle, p.loc_end_cycle,
             p.entity.yaw, p.entity.needs_forward_draw_padding,
             p.min_tile_x, p.min_tile_z, p.max_tile_x, p.max_tile_z)
        };
        let (px, pz, idle, has_loc, loc_start, loc_end, yaw, padding,
             min_tx, min_tz, max_tx, max_tz) = snapshot;

        // Java 4227-4229 — crowd LOD (rev1 lowMem clients also trigger
        // at >50; we run high-detail so only the hard 200 cap applies).
        let low_mem = player_count > 200 && !local && idle;

        let tx = px >> 7;
        let tz = pz >> 7;
        if !(0..104).contains(&tx) || !(0..104).contains(&tz) {
            continue;
        }

        let loc_window = has_loc && loop_cycle >= loc_start && loop_cycle < loc_end;

        if !loc_window {
            // Java 4236-4241 — tile-centred entities share one model
            // per tile per frame.
            if (px & 0x7F) == 64 && (pz & 0x7F) == 64 {
                if c.tile_last_occupied[tx as usize][tz as usize] == scene_cycle {
                    continue;
                }
                c.tile_last_occupied[tx as usize][tz as usize] = scene_cycle;
            }
        }

        let y = crate::client::get_av_h(px, pz, level);
        let model = {
            let player = if local {
                c.local_player.as_mut()
            } else {
                c.players.get_mut(pid as usize).and_then(|o| o.as_mut())
            };
            let Some(p) = player else { continue; };
            p.low_mem = if loc_window { false } else { low_mem };
            p.y = y;
            p.get_temp_model(loop_cycle)
        };
        let Some(model) = model else { continue; };

        let source = ModelSource::lit(std::sync::Arc::new(model));
        if loc_window {
            world.add_dynamic_span(level, px, pz, y, Some(source), yaw, typecode,
                                   min_tx, min_tz, max_tx, max_tz);
        } else {
            world.add_dynamic(level, px, pz, y, 60, Some(source), yaw, typecode,
                              padding);
        }
    }
}

// @ObfuscatedName("dw.do(ZB)V") — Client.addNpcs. Verbatim port of
// Client.java:4254-4277; `on_top` selects the alwaysontop pass.
fn push_npcs(c: &mut crate::client::Client,
             world: &mut crate::dash3d::world::World,
             scene_cycle: i32, on_top: bool) {
    use crate::dash3d::model_source::ModelSource;

    let level = c.minusedlevel;
    for i in 0..c.npc_count as usize {
        let Some(&nid) = c.npc_ids.get(i) else { continue; };
        let mut typecode = (nid << 14) + 0x20000000;

        let snapshot = {
            let Some(Some(n)) = c.npcs.get(nid as usize) else { continue; };
            if !n.ready() {
                continue;
            }
            let t = crate::config::npc_type::list(n.type_id);
            if t.alwaysontop != on_top || !t.is_multi_npc_visible() {
                continue;
            }
            (n.entity.x, n.entity.z, n.entity.size.max(1),
             n.entity.yaw, n.entity.needs_forward_draw_padding, t.active)
        };
        let (nx, nz, size, yaw, padding, active) = snapshot;

        let tx = nx >> 7;
        let tz = nz >> 7;
        if !(0..104).contains(&tx) || !(0..104).contains(&tz) {
            continue;
        }

        if size == 1 && (nx & 0x7F) == 64 && (nz & 0x7F) == 64 {
            if c.tile_last_occupied[tx as usize][tz as usize] == scene_cycle {
                continue;
            }
            c.tile_last_occupied[tx as usize][tz as usize] = scene_cycle;
        }

        if !active {
            // Java: var3 -= Integer.MIN_VALUE — flips the top bit so
            // the pick pass knows the npc has no ops.
            typecode = typecode.wrapping_sub(i32::MIN);
        }

        let model = {
            let Some(Some(n)) = c.npcs.get_mut(nid as usize) else { continue; };
            n.get_temp_model()
        };
        let Some(model) = model else { continue; };

        let off = size * 64 - 64;
        let y = crate::client::get_av_h(nx + off, nz + off, level);
        world.add_dynamic(level, nx, nz, y, off + 60,
                          Some(ModelSource::lit(std::sync::Arc::new(model))),
                          yaw, typecode, padding);
    }
}

// @ObfuscatedName("r.dx(I)V") — Client.addProjectiles (the addDynamic
// half; the retarget + cubic-arc motion runs in
// crate::client::add_projectiles, called here per frame like Java).
fn push_projectiles(c: &mut crate::client::Client,
                    world: &mut crate::dash3d::world::World) {
    use crate::dash3d::model_source::ModelSource;
    crate::client::add_projectiles(c);
    let level = c.minusedlevel;
    let loop_cycle = c.loop_cycle;
    for proj in &c.projectiles {
        if loop_cycle < proj.t1 {
            continue;
        }
        let Some(model) = proj.get_temp_model() else { continue; };
        world.add_dynamic(level, proj.x as i32, proj.z as i32, proj.y as i32, 60,
                          Some(ModelSource::lit(std::sync::Arc::new(model))),
                          proj.yaw, -1, false);
    }
}

// @ObfuscatedName("bf.dt(I)V") — Client.addMapAnim (the addDynamic
// half; lifecycle + frame stepping runs in crate::client::add_map_anims).
fn push_map_anims(c: &mut crate::client::Client,
                  world: &mut crate::dash3d::world::World) {
    use crate::dash3d::model_source::ModelSource;
    crate::client::add_map_anims(c);
    let loop_cycle = c.loop_cycle;
    for spot in &c.spotanims {
        if loop_cycle < spot.start_cycle {
            continue;
        }
        let Some(model) = spot.get_temp_model() else { continue; };
        world.add_dynamic(spot.level, spot.x, spot.z, spot.y, 60,
                          Some(ModelSource::lit(std::sync::Arc::new(model))),
                          0, -1, false);
    }
}