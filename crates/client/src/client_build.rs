// @ObfuscatedName("l") — jag::oldscape::ClientBuild
//
// Map archive byte stream parsers. mapBuildLoop polls JS5 for the
// m_X_Z (ground) and l_X_Z (loc) groups requested by RebuildNormal,
// runs loadGround/loadGroundSquare on the ground data, and
// loadLocations on the loc data (after XTEA decrypt). Heightmap + tile
// floors land in the static arrays below; loc placements collect into
// `locs` for downstream world building.

#![allow(dead_code)]

use std::sync::Mutex;

use crate::io::packet::Packet;

// @ObfuscatedName("l.r")
pub const GROUND_H_SIZE: usize = 105;
// @ObfuscatedName("l.d") / floor* — all 4 × 104 × 104.
pub const FLOOR_SIZE: usize = 104;

pub struct State {
    // @ObfuscatedName("l.r")
    pub ground_h: Vec<Vec<Vec<i32>>>,
    // @ObfuscatedName("l.d")
    pub mapl: Vec<Vec<Vec<u8>>>,
    // @ObfuscatedName("l.m")
    pub floor_t1: Vec<Vec<Vec<i8>>>,
    // @ObfuscatedName("l.c")
    pub floor_t2: Vec<Vec<Vec<i8>>>,
    // @ObfuscatedName("l.n")
    pub floors: Vec<Vec<Vec<i8>>>,
    // @ObfuscatedName("l.j")
    pub floor_r: Vec<Vec<Vec<i8>>>,
    // @ObfuscatedName("l.l") — lowest plane where the local player is
    // standing under a "low ceiling" tile (255 means default world).
    pub minusedlevel: i32,
    // @ObfuscatedName("l.p") — shadow strength per (level, x, z). Java
    // populates this during addLoc; finishBuild folds it into the
    // lightmap. Sized 4 × 105 × 105.
    pub shadow: Vec<Vec<Vec<u8>>>,
    // @ObfuscatedName("l.k") — pre-computed lightmap (105 × 105). Java
    // builds this once in finishBuild from ground_h normals + shadow.
    // We currently recompute per frame in scene.rs; this field is the
    // cache slot for the future port.
    pub lightmap: Vec<Vec<i32>>,
    // @ObfuscatedName("l.o") — per-tile occlusion bit grid. The
    // run-length walker in dash3d/occlude.rs consumes this to build
    // AABB occluders. Sized 4 × 105 × 105.
    pub mapo: Vec<Vec<Vec<u16>>>,
    // @ObfuscatedName("l.a") / "l.h" — per-build hue / lightness offset
    // applied to the minimap HSL palette. Java randomises these in
    // finishBuild (±2 drift, clamped); we use fixed values until the
    // RNG context is wired.
    pub hue_off: i32,
    pub lig_off: i32,
    // jagex3.dash3d.Loc — placeholder. We record (level, x, z, id,
    // rotation, kind) so a later port can resolve LocType and add them
    // to the World.
    pub locs: Vec<Loc>,
}

#[derive(Debug, Clone)]
pub struct Loc {
    pub level: i32,
    pub x: i32,
    pub z: i32,
    pub id: i32,
    pub rotation: i32,
    pub kind: i32,
}

impl State {
    fn new() -> Self {
        Self {
            ground_h: vec![vec![vec![0i32; GROUND_H_SIZE]; GROUND_H_SIZE]; 4],
            mapl: vec![vec![vec![0u8; FLOOR_SIZE]; FLOOR_SIZE]; 4],
            floor_t1: vec![vec![vec![0i8; FLOOR_SIZE]; FLOOR_SIZE]; 4],
            floor_t2: vec![vec![vec![0i8; FLOOR_SIZE]; FLOOR_SIZE]; 4],
            floors: vec![vec![vec![0i8; FLOOR_SIZE]; FLOOR_SIZE]; 4],
            floor_r: vec![vec![vec![0i8; FLOOR_SIZE]; FLOOR_SIZE]; 4],
            minusedlevel: 99,
            shadow: vec![vec![vec![0u8; GROUND_H_SIZE]; GROUND_H_SIZE]; 4],
            lightmap: vec![vec![0i32; GROUND_H_SIZE]; GROUND_H_SIZE],
            mapo: vec![vec![vec![0u16; GROUND_H_SIZE]; GROUND_H_SIZE]; 4],
            // Java's randomised values; we use mid-range defaults.
            hue_off: -4,
            lig_off: -8,
            locs: Vec::new(),
        }
    }
}

// 8x8 sub-region rotation arithmetic extracted from Java's
// `loadGroundRegion` (ClientBuild.java:189-215). Same family as
// `RegionRotate::DX/DZ` but operating on 8-tile sub-region coords.
// Given a (local_x, local_z) inside an 8x8 block and a rotation
// 0..3, returns the rotated (dx, dz) offset to add to the block's
// base. Pure arithmetic — no globals, no Mutex.
pub fn region_rotate_8x8(local_x: i32, local_z: i32, rotation: i32) -> (i32, i32) {
    let lx = local_x & 0x7;
    let lz = local_z & 0x7;
    let r = rotation & 0x3;
    let dx = match r {
        0 => lx,
        1 => lz,
        2 => 7 - lx,
        _ => 7 - lz,
    };
    let dz = match r {
        0 => lz,
        1 => 7 - lx,
        2 => 7 - lz,
        _ => lx,
    };
    (dx, dz)
}

// @ObfuscatedName("l.s(IIII)I") — ClientBuild.getTable. Verbatim port
// of the HSL → palette-index helper at ClientBuild.java:1210. Used by
// the underlay/overlay corner colour pipeline and by the minimap.
//
//   index = (hue/4 << 10) | (sat/32 << 7) | (lightness/2)
//   sat halves progressively as lightness exceeds 179/192/217/243.
pub fn get_table(hue: i32, mut sat: i32, light: i32) -> i32 {
    if light > 179 { sat /= 2; }
    if light > 192 { sat /= 2; }
    if light > 217 { sat /= 2; }
    if light > 243 { sat /= 2; }
    light / 2 + ((hue / 4) << 10) + ((sat / 32) << 7)
}

// @ObfuscatedName("l.j(IIB)I") — ClientBuild.getUCol. Modulates an HSL
// palette index by a per-corner lightmap intensity.
//   -1 input → 12345678 sentinel ("skip face").
//   Otherwise: clamp((idx & 0x7F) * intensity / 128, 2, 126) merged
//              with high byte of idx.
pub fn get_u_col(idx: i32, intensity: i32) -> i32 {
    if idx == -1 { return 12345678; }
    let mut v = ((idx & 0x7F) * intensity) / 128;
    if v < 2 { v = 2; } else if v > 126 { v = 126; }
    (idx & 0xFF80) + v
}

// @ObfuscatedName("l.e(III)I") — ClientBuild.getOCol. Overlay companion
// to getUCol:
//   -2 → 12345678 (skip)
//   -1 → intensity.clamp(2, 126) (textured face: light only)
//   else → same modulation as getUCol.
pub fn get_o_col(idx: i32, intensity: i32) -> i32 {
    if idx == -2 { return 12345678; }
    if idx == -1 { return intensity.clamp(2, 126); }
    let mut v = ((idx & 0x7F) * intensity) / 128;
    if v < 2 { v = 2; } else if v > 126 { v = 126; }
    (idx & 0xFF80) + v
}

pub static STATE: std::sync::LazyLock<Mutex<State>> =
    std::sync::LazyLock::new(|| Mutex::new(State::new()));

// @ObfuscatedName("l.init") — ClientBuild.init
pub fn init() {
    let mut s = STATE.lock().unwrap();
    *s = State::new();
}

// @ObfuscatedName("ck.r([BIIII[Lck;I)V") — ClientBuild.loadGround
//
// Decodes one 64×64 ground chunk into ground_h + floor_* + mapl at
// (base_x..base_x+64, base_z..base_z+64). Clears the 0x1000000
// "unloaded" collision bit for every in-bounds tile of the chunk
// first (ClientBuild.java:155-163) — tryMove's block masks include
// that bit, so tiles never touched here stay unwalkable.
pub fn load_ground(
    bytes: &[u8],
    base_x: i32,
    base_z: i32,
    perlin_off_x: i32,
    perlin_off_z: i32,
    collision: Option<&mut [Option<crate::dash3d::CollisionMap>; 4]>,
) {
    if let Some(cms) = collision {
        for cm in cms.iter_mut().flatten() {
            for x in 0..64 {
                for z in 0..64 {
                    let tx = base_x + x;
                    let tz = base_z + z;
                    if tx > 0 && tx < 103 && tz > 0 && tz < 103 {
                        cm.flags[tx as usize][tz as usize] &= 0xFEFFFFFFu32 as i32;
                    }
                }
            }
        }
    }
    let mut p = Packet::from_vec(bytes.to_vec());
    let mut s = STATE.lock().unwrap();
    for level in 0..4 {
        for x in 0..64 {
            for z in 0..64 {
                load_ground_square(&mut s, &mut p, level, base_x + x, base_z + z,
                    perlin_off_x, perlin_off_z, 0);
            }
        }
    }
}

// @ObfuscatedName("dz.c(Lev;IIIIIII)V") — ClientBuild.loadGroundSquare
fn load_ground_square(
    s: &mut State,
    p: &mut Packet,
    level: i32,
    x: i32,
    z: i32,
    perlin_off_x: i32,
    perlin_off_z: i32,
    rotation: i32,
) {
    if x < 0 || x >= 104 || z < 0 || z >= 104 {
        // Out of range — consume bytes so the stream stays aligned.
        loop {
            let v = p.g1();
            if v == 0 { break; }
            if v == 1 { p.g1(); break; }
            if v <= 49 { p.g1(); }
        }
        return;
    }
    let xi = x as usize;
    let zi = z as usize;
    s.mapl[level as usize][xi][zi] = 0;
    loop {
        let v = p.g1();
        if v == 0 {
            if level == 0 {
                s.ground_h[0][xi][zi] = -perlin_noise(x + 932731 + perlin_off_x, z + 556238 + perlin_off_z) * 8;
            } else {
                s.ground_h[level as usize][xi][zi] = s.ground_h[(level - 1) as usize][xi][zi] - 240;
            }
            break;
        }
        if v == 1 {
            let mut h = p.g1();
            if h == 1 { h = 0; }
            if level == 0 {
                s.ground_h[0][xi][zi] = -h * 8;
            } else {
                s.ground_h[level as usize][xi][zi] = s.ground_h[(level - 1) as usize][xi][zi] - h * 8;
            }
            break;
        }
        if v <= 49 {
            s.floor_t2[level as usize][xi][zi] = p.g1b();
            s.floors[level as usize][xi][zi] = ((v - 2) / 4) as i8;
            s.floor_r[level as usize][xi][zi] = ((v - 2 + rotation) & 0x3) as i8;
        } else if v <= 81 {
            s.mapl[level as usize][xi][zi] = (v - 49) as u8;
        } else {
            s.floor_t1[level as usize][xi][zi] = (v - 81) as i8;
        }
    }
}

// @ObfuscatedName("as.n([BIII)Z") — ClientBuild.checkLocations
//
// Walks the loc stream; for each entry resolves LocType and triggers
// its model preload via check_model_all. Returns false if any model
// hasn't fully landed yet so the caller stays at state 25.
pub fn check_locations(bytes: &[u8], base_x: i32, base_z: i32) -> bool {
    use crate::config::loc_type;
    let mut p = Packet::from_vec(bytes.to_vec());
    let mut loc_id: i32 = -1;
    let mut all_ready = true;
    'outer: loop {
        let dgrp = p.gsmart();
        if dgrp == 0 { return all_ready; }
        loc_id += dgrp;
        let mut pos: i32 = 0;
        let mut handled = false;
        loop {
            while !handled {
                let dpos = p.gsmart();
                if dpos == 0 { continue 'outer; }
                pos += dpos - 1;
                let local_z = pos & 0x3F;
                let local_x = (pos >> 6) & 0x3F;
                let _info = p.g1();
                let world_x = base_x + local_x;
                let world_z = base_z + local_z;
                if world_x > 0 && world_z > 0 && world_x < 103 && world_z < 103 {
                    match loc_type::list(loc_id) {
                        Some(t) => {
                            if !t.check_model_all() {
                                all_ready = false;
                            }
                        }
                        // Loc config itself hasn't streamed yet — the
                        // build would silently skip this loc forever,
                        // so hold the map load until it decodes.
                        None => {
                            all_ready = false;
                        }
                    }
                    handled = true;
                }
            }
            let next = p.gsmart();
            if next == 0 { break; }
            p.g1();
        }
    }
}

// @ObfuscatedName("dk.j([BIILaq;[Lck;I)V") — ClientBuild.loadLocations
pub fn load_locations(bytes: &[u8], base_x: i32, base_z: i32) {
    let mut p = Packet::from_vec(bytes.to_vec());
    let mut loc_id: i32 = -1;
    let mut s = STATE.lock().unwrap();
    'outer: loop {
        let dgrp = p.gsmart();
        if dgrp == 0 { break 'outer; }
        loc_id += dgrp;
        let mut pos: i32 = 0;
        loop {
            let dpos = p.gsmart();
            if dpos == 0 { break; }
            pos += dpos - 1;
            let local_z = pos & 0x3F;
            let local_x = (pos >> 6) & 0x3F;
            let level = pos >> 12;
            let info = p.g1();
            let kind = info >> 2;
            let rotation = info & 0x3;
            let world_x = base_x + local_x;
            let world_z = base_z + local_z;
            if world_x > 0 && world_z > 0 && world_x < 103 && world_z < 103 {
                // Java (ClientBuild.java:352-361) keeps the RAW level for
                // placement + heights — the bridge `-1` adjustment picks
                // only WHICH COLLISION MAP gets the loc (and skips
                // collision when it goes negative). World.pushDown later
                // shifts bridge squares into the level-0 render slot.
                // Decrementing the placement level here put bridge
                // parapets at level 0 with riverbed heights (walls flat
                // on the water) and dropped level-0 locs on bridge tiles.
                s.locs.push(Loc {
                    level,
                    x: world_x,
                    z: world_z,
                    id: loc_id,
                    rotation,
                    kind,
                });
            }
        }
    }
}

// @ObfuscatedName("dm.t(III)I") — ClientBuild.perlinNoise
//
// Fixed-point perlin noise driving default terrain heights. Three
// octaves of `interpolated_noise` blended with descending amplitudes,
// then scaled into 10..60 range.
fn perlin_noise(x: i32, z: i32) -> i32 {
    let v = interpolated_noise(x + 45365, z + 91923, 4) - 128
        + ((interpolated_noise(x + 10294, z + 37821, 2) - 128) >> 1)
        + ((interpolated_noise(x, z, 1) - 128) >> 2);
    let mut out = ((v as f64) * 0.3) as i32 + 35;
    if out < 10 { out = 10; }
    if out > 60 { out = 60; }
    out
}

// @ObfuscatedName("dn.s(IIIB)I") — ClientBuild.interpolatedNoise
fn interpolated_noise(x: i32, z: i32, step: i32) -> i32 {
    let qx = x.div_euclid(step);
    let rx = x & (step - 1);
    let qz = z.div_euclid(step);
    let rz = z & (step - 1);
    let s00 = smooth_noise(qx, qz);
    let s10 = smooth_noise(qx + 1, qz);
    let s01 = smooth_noise(qx, qz + 1);
    let s11 = smooth_noise(qx + 1, qz + 1);
    let cx = (65536 - cos_table()[(rx * 1024 / step) as usize]) >> 1;
    let mx0 = ((65536 - cx) * s00 >> 16) + (s10 * cx >> 16);
    let cx2 = (65536 - cos_table()[(rx * 1024 / step) as usize]) >> 1;
    let mx1 = ((65536 - cx2) * s01 >> 16) + (s11 * cx2 >> 16);
    let cz = (65536 - cos_table()[(rz * 1024 / step) as usize]) >> 1;
    ((65536 - cz) * mx0 >> 16) + (mx1 * cz >> 16)
}

// @ObfuscatedName("cw.u(III)I") — ClientBuild.smoothNoise
fn smooth_noise(x: i32, z: i32) -> i32 {
    let corners = noise(x - 1, z - 1) + noise(x + 1, z - 1) + noise(x - 1, z + 1) + noise(x + 1, z + 1);
    let edges = noise(x - 1, z) + noise(x + 1, z) + noise(x, z - 1) + noise(x, z + 1);
    let center = noise(x, z);
    center / 4 + corners / 16 + edges / 8
}

// @ObfuscatedName("ef.v(III)I") — ClientBuild.noise
fn noise(x: i32, z: i32) -> i32 {
    let v = z.wrapping_mul(57).wrapping_add(x);
    let s = (v << 13) ^ v;
    let mixed = s.wrapping_mul(s).wrapping_mul(15731).wrapping_add(789221)
        .wrapping_mul(s).wrapping_add(1376312589) & i32::MAX;
    (mixed >> 19) & 0xFF
}

// @ObfuscatedName("Pix3D.cosTable") — 1024-entry fixed-point cosine.
// Sourced from Pix3D's static init; lazy so the table only allocates
// when perlinNoise is actually invoked.
fn cos_table() -> &'static [i32; 1024] {
    use std::sync::OnceLock;
    static T: OnceLock<[i32; 1024]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0i32; 1024];
        for i in 0..1024 {
            t[i] = ((((i as f64) / 1024.0 * std::f64::consts::TAU).cos()) * 65536.0) as i32;
        }
        t
    })
}

// @ObfuscatedName("bk.r(I)V") — ClientBuild.quit. Verbatim port of
// ClientBuild.java:113-126. Drops every scratch array so a fresh
// REBUILD can re-allocate from zero. Used by Client.logout +
// quit_to_login flows.
//
// Our State struct holds Vec-backed scratch arrays; truncating to 0
// matches the Java "= null" semantics for GC pressure.
pub fn quit_state() {
    let mut s = STATE.lock().unwrap();
    // Clear the scratch arrays the Rust State actually holds; the
    // Java side has a handful of additional "totals" arrays
    // (huetot/sattot/ligtot/comtot/tot) used by finishBuild's
    // lightmap synthesis — those don't exist on the Rust path yet
    // because finishBuild is deferred.
    s.floors.clear();
    s.shadow.clear();
    s.lightmap.clear();
    s.mapo.clear();
}

// @ObfuscatedName("dy.d(IIIIB)V") — ClientBuild.fadeAdjacent.
// Verbatim port of ClientBuild.java:130-149. Walks the (arg0..arg0+
// arg2) × (arg1..arg1+arg3) rectangle, slamming shadow=127 on every
// tile and pulling the boundary tile's ground_h from the *interior*
// side (so the seam blends instead of stepping). Used at region
// edges during multi-tile mapBuild loads.
pub fn fade_adjacent(start_x: i32, start_z: i32, w: i32, h: i32) {
    let mut s = STATE.lock().unwrap();
    for z in start_z..=(start_z + h) {
        for x in start_x..=(start_x + w) {
            if !(0..104).contains(&x) || !(0..104).contains(&z) { continue; }
            let xu = x as usize;
            let zu = z as usize;
            s.shadow[0][xu][zu] = 127;
            if start_x == x && x > 0 {
                s.ground_h[0][xu][zu] = s.ground_h[0][xu - 1][zu];
            }
            if start_x + w == x && x < 103 {
                s.ground_h[0][xu][zu] = s.ground_h[0][xu + 1][zu];
            }
            if start_z == z && z > 0 {
                s.ground_h[0][xu][zu] = s.ground_h[0][xu][zu - 1];
            }
            if start_z + h == z && z < 103 {
                s.ground_h[0][xu][zu] = s.ground_h[0][xu][zu + 1];
            }
        }
    }
}

// @ObfuscatedName("l.WSHAPE0") — wall edge bits by rotation.
pub const WSHAPE0: [i32; 4] = [1, 2, 4, 8];
// @ObfuscatedName("l.WSHAPE1") — wall corner bits by rotation.
pub const WSHAPE1: [i32; 4] = [16, 32, 64, 128];
// @ObfuscatedName("l.DECORXOF") / "l.DECORZOF" — kind-5 decor offsets.
pub const DECORXOF: [i32; 4] = [1, 0, -1, 0];
pub const DECORZOF: [i32; 4] = [0, -1, 0, 1];
// @ObfuscatedName("l.DECORXOF2") / "l.DECORZOF2" — kind-6/8 offsets.
pub const DECORXOF2: [i32; 4] = [1, -1, -1, 1];
pub const DECORZOF2: [i32; 4] = [-1, -1, 1, 1];

// ClientLocAnim as a Temp ModelSource — Java's ClientLocAnim extends
// ModelSource and composes the current anim frame in getTempModel
// (ClientLocAnim.java). The closure resolves multiloc + advances the
// persistent per-instance anim state (scene::advance_loc_anim keeps
// Java's animFrame/animCycle pair keyed by (level, x, z, id)) and
// hands LocType.getTempModel the heightmap for hillSkew.
fn loc_anim_source(loc_id: i32, shape: i32, rotation: i32,
                   level: i32, x: i32, z: i32,
                   anchor_x: i32, h: i32, anchor_z: i32)
                   -> std::sync::Arc<crate::dash3d::model_source::ModelSource> {
    use crate::dash3d::model_source::ModelSource;
    ModelSource::temp(std::sync::Arc::new(move || {
        let lt0 = crate::config::loc_type::list(loc_id)?;
        let lt = if lt0.multiloc.is_some() {
            lt0.get_multi_loc()?
        } else {
            lt0
        };
        let loop_cycle = crate::scene::LOOP_CYCLE.load(std::sync::atomic::Ordering::Relaxed);
        // Server-driven LOC_ANIM (opcode 6) overrides the LocType's
        // ambient anim — Java wraps the Square model in a ClientLocAnim;
        // we swap which seq this closure advances.
        let anim_id = crate::scene::server_loc_anim(level, x, z, loc_id).unwrap_or(lt.anim);
        let (seq, frame) = if anim_id >= 0 {
            match crate::scene::advance_loc_anim(level, x, z, loc_id, anim_id, loop_cycle) {
                Some(f) => (Some(crate::config::seq_type::list(anim_id)), f),
                None => (None, 0),
            }
        } else {
            (None, 0)
        };
        let st = STATE.lock().unwrap();
        let groundh = &st.ground_h[level as usize];
        lt.get_temp_model(shape, rotation, groundh, anchor_x, h, anchor_z,
                          seq.as_ref(), frame)
    }))
}

// @ObfuscatedName("l.z(IIIIIILaq;Lck;I)V") — ClientBuild.addLoc.
// Verbatim port of ClientBuild.java:413-730: places one decoded loc
// into the World (wall / decor / scenery / ground decor by kind),
// stamps the shadow + occlusion-bit grids, and registers collision.
#[allow(clippy::too_many_arguments)]
pub fn add_loc(level: i32, x: i32, z: i32, loc_id: i32, rotation: i32, kind: i32,
               world: &mut crate::dash3d::world::World,
               collision: Option<&mut crate::dash3d::CollisionMap>,
               low_mem: bool, last_built_level: i32,
               ground_level: i32, with_bgsound: bool, bake_lighting: bool) {
    // `ground_level` is the level whose ground-height grid is sampled for the
    // model anchor (== `level` during the build replay, but `level`+1 on bridge
    // tiles via changeLocUnchecked). `with_bgsound` is false on the loc-change
    // path so a re-placement doesn't double-register an ambient emitter.
    // `bake_lighting` writes the shadow/occlusion (mapo) arrays — true at build
    // time, false on the loc-change path (changeLocUnchecked skips them; the
    // lighting bake already happened). These unify ClientBuild.addLoc +
    // ClientBuild.changeLocUnchecked (both place a loc).
    use crate::config::loc_type;
    use crate::dash3d::model_source::ModelSource;
    use std::sync::Arc;

    let mut s = STATE.lock().unwrap();
    if low_mem && (s.mapl[0][x as usize][z as usize] & 0x2) == 0 {
        if (s.mapl[level as usize][x as usize][z as usize] & 0x10) != 0 {
            return;
        }
        let eff = if (s.mapl[level as usize][x as usize][z as usize] & 0x8) != 0 {
            0
        } else if level <= 0 || (s.mapl[1][x as usize][z as usize] & 0x2) == 0 {
            level
        } else {
            level - 1
        };
        if last_built_level != eff {
            return;
        }
    }
    if level < s.minusedlevel {
        s.minusedlevel = level;
    }
    let Some(lt) = loc_type::list(loc_id) else { return };
    let (lw, ll) = if rotation == 1 || rotation == 3 {
        (lt.length, lt.width)
    } else {
        (lt.width, lt.length)
    };
    let (hx0, hx1) = if x + lw <= 104 {
        ((lw >> 1) + x, ((lw + 1) >> 1) + x)
    } else {
        (x, x + 1)
    };
    let (hz0, hz1) = if z + ll <= 104 {
        ((ll >> 1) + z, ((ll + 1) >> 1) + z)
    } else {
        (z, z + 1)
    };
    let h_val = {
        let gh = &s.ground_h[ground_level as usize];
        (gh[hx0 as usize][hz0 as usize] + gh[hx1 as usize][hz0 as usize]
            + gh[hx0 as usize][hz1 as usize] + gh[hx1 as usize][hz1 as usize]) >> 2
    };
    let anchor_x = (x << 7) + (lw << 6);
    let anchor_z = (z << 7) + (ll << 6);
    // Typecode: loc id + tile, top bits flag interactivity.
    let mut typecode = (loc_id << 14) + (z << 7) + x + 1073741824;
    if lt.active == 0 {
        typecode = typecode.wrapping_sub(i32::MIN);
    }
    let mut typecode2 = (rotation << 6) + kind;
    if lt.raiseobject == 1 {
        typecode2 += 256;
    }
    if with_bgsound && lt.has_bg_sound() {
        crate::sound::bg_sound::add_sound(level, x, z, &lt, rotation);
    }
    // Model factory shared by the per-kind arms: lit static model when
    // the loc is inanimate, Temp ClientLocAnim source otherwise.
    let animated = lt.anim != -1 || lt.multiloc.is_some();
    let make_model = |s: &State, shape: i32, rot: i32| -> Option<Arc<ModelSource>> {
        if animated {
            Some(loc_anim_source(loc_id, shape, rot, level, x, z,
                                 anchor_x, h_val, anchor_z))
        } else {
            lt.get_model(shape, rot, &s.ground_h[ground_level as usize],
                         anchor_x, h_val, anchor_z)
        }
    };
    let (lvl, xu, zu) = (level as usize, x as usize, z as usize);

    if kind == 22 {
        if !low_mem || lt.active != 0 || lt.blockwalk == 1 || lt.forcedecor {
            let model = make_model(&s, 22, rotation);
            drop(s);
            world.set_ground_decor(level, x, z, h_val, model, typecode, typecode2);
            if lt.blockwalk == 1 {
                if let Some(cm) = collision {
                    cm.block_ground_decor(x, z);
                }
            }
        }
    } else if kind == 10 || kind == 11 {
        let model = make_model(&s, 10, rotation);
        drop(s);
        let placed = world.add_scenery(level, x, z, h_val, lw, ll, model.clone(),
                                       if kind == 11 { 256 } else { 0 },
                                       typecode, typecode2);
        if bake_lighting && model.is_some() && placed && lt.shadow {
            // Java: ModelLit radius/4 clamped 30, else 15 for anim'd.
            let strength = model.as_ref()
                .and_then(|m| m.lit_radius_cylinder())
                .map_or(15, |r| (r / 4).min(30));
            let mut s = STATE.lock().unwrap();
            for dx in 0..=lw {
                for dz in 0..=ll {
                    let sx = (x + dx) as usize;
                    let sz = (z + dz) as usize;
                    if sx < 105 && sz < 105 && strength as u8 > s.shadow[lvl][sx][sz] {
                        s.shadow[lvl][sx][sz] = strength as u8;
                    }
                }
            }
        }
        if lt.blockwalk != 0 {
            if let Some(cm) = collision {
                cm.add_loc(x, z, lw, ll, lt.blockrange);
            }
        }
    } else if kind >= 12 {
        let model = make_model(&s, kind, rotation);
        if bake_lighting && kind <= 17 && kind != 13 && level > 0 {
            s.mapo[lvl][xu][zu] |= 0x924;
        }
        drop(s);
        world.add_scenery(level, x, z, h_val, 1, 1, model, 0, typecode, typecode2);
        if lt.blockwalk != 0 {
            if let Some(cm) = collision {
                cm.add_loc(x, z, lw, ll, lt.blockrange);
            }
        }
    } else if kind == 0 {
        let model = make_model(&s, 0, rotation);
        match rotation {
            0 => {
                if bake_lighting && lt.shadow {
                    s.shadow[lvl][xu][zu] = 50;
                    s.shadow[lvl][xu][zu + 1] = 50;
                }
                if bake_lighting && lt.occlude {
                    s.mapo[lvl][xu][zu] |= 0x249;
                }
            }
            1 => {
                if bake_lighting && lt.shadow {
                    s.shadow[lvl][xu][zu + 1] = 50;
                    s.shadow[lvl][xu + 1][zu + 1] = 50;
                }
                if bake_lighting && lt.occlude {
                    s.mapo[lvl][xu][zu + 1] |= 0x492;
                }
            }
            2 => {
                if bake_lighting && lt.shadow {
                    s.shadow[lvl][xu + 1][zu] = 50;
                    s.shadow[lvl][xu + 1][zu + 1] = 50;
                }
                if bake_lighting && lt.occlude {
                    s.mapo[lvl][xu + 1][zu] |= 0x249;
                }
            }
            3 => {
                if bake_lighting && lt.shadow {
                    s.shadow[lvl][xu][zu] = 50;
                    s.shadow[lvl][xu + 1][zu] = 50;
                }
                if bake_lighting && lt.occlude {
                    s.mapo[lvl][xu][zu] |= 0x492;
                }
            }
            _ => {}
        }
        drop(s);
        world.set_wall(level, x, z, h_val, model, None,
                       WSHAPE0[(rotation & 0x3) as usize], 0, typecode, typecode2);
        if lt.blockwalk != 0 {
            if let Some(cm) = collision {
                cm.add_wall(x, z, kind, rotation, lt.blockrange);
            }
        }
        if lt.wallwidth != 16 {
            world.move_decor(level, x, z, lt.wallwidth);
        }
    } else if kind == 1 {
        let model = make_model(&s, 1, rotation);
        if bake_lighting && lt.shadow {
            match rotation {
                0 => s.shadow[lvl][xu][zu + 1] = 50,
                1 => s.shadow[lvl][xu + 1][zu + 1] = 50,
                2 => s.shadow[lvl][xu + 1][zu] = 50,
                3 => s.shadow[lvl][xu][zu] = 50,
                _ => {}
            }
        }
        drop(s);
        world.set_wall(level, x, z, h_val, model, None,
                       WSHAPE1[(rotation & 0x3) as usize], 0, typecode, typecode2);
        if lt.blockwalk != 0 {
            if let Some(cm) = collision {
                cm.add_wall(x, z, kind, rotation, lt.blockrange);
            }
        }
    } else if kind == 2 {
        let rot2 = (rotation + 1) & 0x3;
        let model_a = make_model(&s, 2, rotation + 4);
        let model_b = make_model(&s, 2, rot2);
        if bake_lighting && lt.occlude {
            match rotation {
                0 => {
                    s.mapo[lvl][xu][zu] |= 0x249;
                    s.mapo[lvl][xu][zu + 1] |= 0x492;
                }
                1 => {
                    s.mapo[lvl][xu][zu + 1] |= 0x492;
                    s.mapo[lvl][xu + 1][zu] |= 0x249;
                }
                2 => {
                    s.mapo[lvl][xu + 1][zu] |= 0x249;
                    s.mapo[lvl][xu][zu] |= 0x492;
                }
                3 => {
                    s.mapo[lvl][xu][zu] |= 0x492;
                    s.mapo[lvl][xu][zu] |= 0x249;
                }
                _ => {}
            }
        }
        drop(s);
        world.set_wall(level, x, z, h_val, model_a, model_b,
                       WSHAPE0[(rotation & 0x3) as usize],
                       WSHAPE0[rot2 as usize], typecode, typecode2);
        if lt.blockwalk != 0 {
            if let Some(cm) = collision {
                cm.add_wall(x, z, kind, rotation, lt.blockrange);
            }
        }
        if lt.wallwidth != 16 {
            world.move_decor(level, x, z, lt.wallwidth);
        }
    } else if kind == 3 {
        let model = make_model(&s, 3, rotation);
        if bake_lighting && lt.shadow {
            match rotation {
                0 => s.shadow[lvl][xu][zu + 1] = 50,
                1 => s.shadow[lvl][xu + 1][zu + 1] = 50,
                2 => s.shadow[lvl][xu + 1][zu] = 50,
                3 => s.shadow[lvl][xu][zu] = 50,
                _ => {}
            }
        }
        drop(s);
        world.set_wall(level, x, z, h_val, model, None,
                       WSHAPE1[(rotation & 0x3) as usize], 0, typecode, typecode2);
        if lt.blockwalk != 0 {
            if let Some(cm) = collision {
                cm.add_wall(x, z, kind, rotation, lt.blockrange);
            }
        }
    } else if kind == 9 {
        let model = make_model(&s, kind, rotation);
        drop(s);
        world.add_scenery(level, x, z, h_val, 1, 1, model, 0, typecode, typecode2);
        if lt.blockwalk != 0 {
            if let Some(cm) = collision {
                cm.add_loc(x, z, lw, ll, lt.blockrange);
            }
        }
        if lt.wallwidth != 16 {
            world.move_decor(level, x, z, lt.wallwidth);
        }
    } else if kind == 4 {
        let model = make_model(&s, 4, rotation);
        drop(s);
        world.set_decor(level, x, z, h_val, model, None,
                        WSHAPE0[(rotation & 0x3) as usize], 0, 0, 0, typecode, typecode2);
    } else if kind == 5 {
        let mut ww = 16;
        let wall_tc = world.wall_type(level, x, z);
        if wall_tc != 0 {
            if let Some(wlt) = loc_type::list((wall_tc >> 14) & 0x7FFF) {
                ww = wlt.wallwidth;
            }
        }
        let model = make_model(&s, 4, rotation);
        drop(s);
        let r = (rotation & 0x3) as usize;
        world.set_decor(level, x, z, h_val, model, None,
                        WSHAPE0[r], 0, DECORXOF[r] * ww, DECORZOF[r] * ww,
                        typecode, typecode2);
    } else if kind == 6 {
        let mut ww = 8;
        let wall_tc = world.wall_type(level, x, z);
        if wall_tc != 0 {
            if let Some(wlt) = loc_type::list((wall_tc >> 14) & 0x7FFF) {
                ww = wlt.wallwidth / 2;
            }
        }
        let model = make_model(&s, 4, rotation + 4);
        drop(s);
        let r = (rotation & 0x3) as usize;
        world.set_decor(level, x, z, h_val, model, None,
                        256, rotation, DECORXOF2[r] * ww, DECORZOF2[r] * ww,
                        typecode, typecode2);
    } else if kind == 7 {
        let rot2 = (rotation + 2) & 0x3;
        let model = make_model(&s, 4, rot2 + 4);
        drop(s);
        world.set_decor(level, x, z, h_val, model, None,
                        256, rot2, 0, 0, typecode, typecode2);
    } else if kind == 8 {
        let mut ww = 8;
        let wall_tc = world.wall_type(level, x, z);
        if wall_tc != 0 {
            if let Some(wlt) = loc_type::list((wall_tc >> 14) & 0x7FFF) {
                ww = wlt.wallwidth / 2;
            }
        }
        let rot2 = (rotation + 2) & 0x3;
        let model_a = make_model(&s, 4, rotation + 4);
        let model_b = make_model(&s, 4, rot2 + 4);
        drop(s);
        let r = (rotation & 0x3) as usize;
        world.set_decor(level, x, z, h_val, model_a, model_b,
                        256, rotation, DECORXOF2[r] * ww, DECORZOF2[r] * ww,
                        typecode, typecode2);
    }
}

// @ObfuscatedName(— ClientBuild.changeLocUnchecked, ClientBuild.java:1241).
// Places a single new loc (the apply half of a queued loc change). Java keeps
// this as a near-duplicate of addLoc; we reuse `add_loc`'s proven per-shape
// placement, sampling ground height at `eff_level` (bridge tiles) and skipping
// the bgsound registration. lowMem is gated by the caller (loc_change_unchecked)
// so `low_mem=false` here — which also makes the kind==22 ground-decor arm
// unconditional, matching changeLocUnchecked. (Java uses getModelLit vs addLoc's
// getModel; the Rust ModelSource the build path produces is already scene-lit, so
// reusing add_loc's get_model yields the same placement.)
// Read a build `mapl` flag bitmask for a tile (Java `ClientBuild.mapl[l][x][z]`).
// Used by loc_change_unchecked's bridge-tile effective-level calc.
#[must_use]
pub fn mapl_flag(level: i32, x: i32, z: i32) -> i32 {
    i32::from(STATE.lock().unwrap().mapl[level as usize][x as usize][z as usize])
}

#[allow(clippy::too_many_arguments)]
pub fn change_loc_unchecked(level: i32, eff_level: i32, x: i32, z: i32,
                            loc_id: i32, angle: i32, shape: i32,
                            world: &mut crate::dash3d::world::World,
                            collision: Option<&mut crate::dash3d::CollisionMap>) {
    add_loc(level, x, z, loc_id, angle, shape, world, collision,
            /* low_mem = */ false, /* last_built_level = */ 0,
            /* ground_level = */ eff_level, /* with_bgsound = */ false,
            /* bake_lighting = */ false);
}

// @ObfuscatedName("fp.q(Laq;[Lck;I)V") — ClientBuild.finishBuild.
// Verbatim port of ClientBuild.java:734-1118 with one structural
// difference: Java runs addLoc inline while decoding l_X_Z groups;
// our loadLocations records placements into `state.locs`, so the
// addLoc replay happens here first (same stream order, so the
// wallType lookups for decor kinds 5/6/8 still see their walls).
pub fn finish_build(world: &mut crate::dash3d::world::World,
                    mut collision: Option<&mut [Option<crate::dash3d::CollisionMap>; 4]>,
                    low_mem: bool, last_built_level: i32) {
    use crate::config::{flo_type, flu_type};
    use crate::dash3d::pix3d;

    // addLoc replay (Java: loadLocations → addLoc inline). The bridge
    // `-1` (Java ClientBuild.java:353-360) applies ONLY to the collision
    // map pick: a level-1 loc on a bridge tile clips on the level-0 map,
    // and a level-0 loc there clips nowhere (var18 < 0 → null map). The
    // placement level and height sampling stay RAW.
    let locs: Vec<Loc> = STATE.lock().unwrap().locs.clone();
    let bridge_mask: Vec<Vec<bool>> = {
        let s = STATE.lock().unwrap();
        (0..104).map(|x| (0..104).map(|z| (s.mapl[1][x][z] & 0x2) == 2).collect()).collect()
    };
    for loc in &locs {
        let mut collision_level = loc.level;
        if bridge_mask[loc.x as usize][loc.z as usize] {
            collision_level -= 1;
        }
        let cm = if collision_level >= 0 {
            collision.as_deref_mut()
                .and_then(|cms| cms[collision_level.clamp(0, 3) as usize].as_mut())
        } else {
            None
        };
        add_loc(loc.level, loc.x, loc.z, loc.id, loc.rotation, loc.kind,
                world, cm, low_mem, last_built_level,
                /* ground_level = */ loc.level, /* with_bgsound = */ true,
                /* bake_lighting = */ true);
    }

    // Blocked-ground collision pass (Java 735-749).
    {
        let s = STATE.lock().unwrap();
        if let Some(cms) = collision.as_deref_mut() {
            for level in 0..4i32 {
                for x in 0..104i32 {
                    for z in 0..104i32 {
                        if (s.mapl[level as usize][x as usize][z as usize] & 0x1) == 1 {
                            let mut eff = level;
                            if (s.mapl[1][x as usize][z as usize] & 0x2) == 2 {
                                eff = level - 1;
                            }
                            if eff >= 0 {
                                if let Some(cm) = cms[eff as usize].as_mut() {
                                    cm.block_ground(x, z);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // hueOff / ligOff drift (Java 750-763 uses Math.random ±2; we keep
    // the deterministic per-build defaults from State::new and only
    // clamp, so repeated builds stay reproducible).
    {
        let mut s = STATE.lock().unwrap();
        s.hue_off = s.hue_off.clamp(-8, 8);
        s.lig_off = s.lig_off.clamp(-16, 16);
    }

    let palette = pix3d::colour_table();
    for level in 0..4i32 {
        // Lightmap pass (Java 764-780).
        {
            let mut s = STATE.lock().unwrap();
            let scale = ((5100.0f64).sqrt() as i32) * 768 >> 8;
            for z in 1..103usize {
                for x in 1..103usize {
                    let dx = s.ground_h[level as usize][x + 1][z]
                        - s.ground_h[level as usize][x - 1][z];
                    let dz = s.ground_h[level as usize][x][z + 1]
                        - s.ground_h[level as usize][x][z - 1];
                    let norm = (((dz * dz + dx * dx + 65536) as f64).sqrt()) as i32;
                    let nx = (dx << 8) / norm;
                    let ny = 65536 / norm;
                    let nz = (dz << 8) / norm;
                    let light = (nz * -50 + nx * -50 + ny * -10) / scale + 96;
                    let sh = &s.shadow[level as usize];
                    let blur = (sh[x][z] as i32 >> 1)
                        + (sh[x][z + 1] as i32 >> 3)
                        + (sh[x][z - 1] as i32 >> 2)
                        + (sh[x - 1][z] as i32 >> 2)
                        + (sh[x + 1][z] as i32 >> 3);
                    s.lightmap[x][z] = light - blur;
                }
            }
        }

        // Rolling-average underlay + setGround pass (Java 781-950).
        let mut huetot = [0i32; 104];
        let mut sattot = [0i32; 104];
        let mut ligtot = [0i32; 104];
        let mut comtot = [0i32; 104];
        let mut tot = [0i32; 104];
        for sweep_x in -5..109i32 {
            // Column accumulators slide along X.
            for z in 0..104usize {
                let xin = sweep_x + 5;
                if (0..104).contains(&xin) {
                    let t1 = {
                        let s = STATE.lock().unwrap();
                        s.floor_t1[level as usize][xin as usize][z] as i32 & 0xFF
                    };
                    if t1 > 0 {
                        let fl = flu_type::list(t1 - 1);
                        huetot[z] += fl.hue;
                        sattot[z] += fl.saturation;
                        ligtot[z] += fl.lightness;
                        comtot[z] += fl.chroma;
                        tot[z] += 1;
                    }
                }
                let xout = sweep_x - 5;
                if (0..104).contains(&xout) {
                    let t1 = {
                        let s = STATE.lock().unwrap();
                        s.floor_t1[level as usize][xout as usize][z] as i32 & 0xFF
                    };
                    if t1 > 0 {
                        let fl = flu_type::list(t1 - 1);
                        huetot[z] -= fl.hue;
                        sattot[z] -= fl.saturation;
                        ligtot[z] -= fl.lightness;
                        comtot[z] -= fl.chroma;
                        tot[z] -= 1;
                    }
                }
            }
            if !(1..103).contains(&sweep_x) {
                continue;
            }
            let x = sweep_x;
            let mut hue_acc = 0i32;
            let mut sat_acc = 0i32;
            let mut lig_acc = 0i32;
            let mut com_acc = 0i32;
            let mut n_acc = 0i32;
            for sweep_z in -5..109i32 {
                let zin = sweep_z + 5;
                if (0..104).contains(&zin) {
                    hue_acc += huetot[zin as usize];
                    sat_acc += sattot[zin as usize];
                    lig_acc += ligtot[zin as usize];
                    com_acc += comtot[zin as usize];
                    n_acc += tot[zin as usize];
                }
                let zout = sweep_z - 5;
                if (0..104).contains(&zout) {
                    hue_acc -= huetot[zout as usize];
                    sat_acc -= sattot[zout as usize];
                    lig_acc -= ligtot[zout as usize];
                    com_acc -= comtot[zout as usize];
                    n_acc -= tot[zout as usize];
                }
                if !(1..103).contains(&sweep_z) {
                    continue;
                }
                let z = sweep_z;
                // Per-tile work: snapshot what we need, drop the lock
                // before world.set_ground (no re-entrancy, just keeps
                // the critical section small).
                let (mapl0, mapl_lvl, mapl1, t1, t2, heights, lights, floors_v, floorr_v,
                     hue_off, lig_off, minused) = {
                    let s = STATE.lock().unwrap();
                    let l = level as usize;
                    let xu = x as usize;
                    let zu = z as usize;
                    ((s.mapl[0][xu][zu], s.mapl[l][xu][zu], s.mapl[1][xu][zu]),
                     s.mapl[l][xu][zu],
                     s.mapl[1][xu][zu],
                     s.floor_t1[l][xu][zu] as i32 & 0xFF,
                     s.floor_t2[l][xu][zu] as i32 & 0xFF,
                     [s.ground_h[l][xu][zu], s.ground_h[l][xu + 1][zu],
                      s.ground_h[l][xu + 1][zu + 1], s.ground_h[l][xu][zu + 1]],
                     [s.lightmap[xu][zu], s.lightmap[xu + 1][zu],
                      s.lightmap[xu + 1][zu + 1], s.lightmap[xu][zu + 1]],
                     s.floors[l][xu][zu] as i32,
                     s.floor_r[l][xu][zu],
                     s.hue_off, s.lig_off, s.minusedlevel)
                };
                let _ = mapl_lvl;
                if low_mem && (mapl0.0 & 0x2) == 0 {
                    if (mapl0.1 & 0x10) != 0 {
                        continue;
                    }
                    let eff = if (mapl0.1 & 0x8) != 0 {
                        0
                    } else if level <= 0 || (mapl1 & 0x2) == 0 {
                        level
                    } else {
                        level - 1
                    };
                    if last_built_level != eff {
                        continue;
                    }
                }
                if level < minused {
                    STATE.lock().unwrap().minusedlevel = level;
                }
                if t1 <= 0 && t2 <= 0 {
                    continue;
                }
                let [h_nw, h_ne, h_se, h_sw] = heights;
                let [l_nw, l_ne, l_se, l_sw] = lights;
                let mut under_idx = -1;
                let mut under_map_idx = -1;
                if t1 > 0 && com_acc > 0 && n_acc > 0 {
                    let hue = hue_acc * 256 / com_acc;
                    let sat = sat_acc / n_acc;
                    let lig = lig_acc / n_acc;
                    under_idx = get_table(hue, sat, lig);
                    let jh = (hue_off + hue) & 0xFF;
                    let jl = (lig_off + lig).clamp(0, 255);
                    under_map_idx = get_table(jh, sat, jl);
                }
                // Roof-tile occlusion bit for flat upper-level tiles
                // (Java 886-897).
                if level > 0 {
                    let mut occludes = true;
                    if t1 == 0 && floors_v != 0 {
                        occludes = false;
                    }
                    if t2 > 0 && !flo_type::list((t2 - 1) & 0xFF).occlude {
                        occludes = false;
                    }
                    if occludes && h_nw == h_ne && h_nw == h_se && h_nw == h_sw {
                        let mut s = STATE.lock().unwrap();
                        s.mapo[level as usize][x as usize][z as usize] |= 0x924;
                    }
                }
                let mut minimap_under = 0;
                if under_map_idx != -1 {
                    minimap_under = palette[(get_u_col(under_map_idx, 96) as usize) & 0xFFFF];
                }
                if t2 == 0 {
                    world.set_ground(level, x, z, 0, 0, -1,
                                     h_nw, h_ne, h_se, h_sw,
                                     get_u_col(under_idx, l_nw), get_u_col(under_idx, l_ne),
                                     get_u_col(under_idx, l_se), get_u_col(under_idx, l_sw),
                                     0, 0, 0, 0,
                                     minimap_under, 0);
                } else {
                    let shape = floors_v + 1;
                    let rotation = floorr_v as i32;
                    let fl = flo_type::list((t2 - 1) & 0xFF);
                    let mut texture = fl.texture;
                    let over_idx: i32;
                    let over_map_idx: i32;
                    if texture >= 0 {
                        over_map_idx = crate::dash3d::texture_manager::get_average_rgb(texture);
                        over_idx = -1;
                    } else if fl.colour == 16711935 {
                        over_idx = -2;
                        texture = -1;
                        over_map_idx = -2;
                    } else {
                        over_idx = get_table(fl.hue, fl.saturation, fl.lightness);
                        let jh = (hue_off + fl.hue) & 0xFF;
                        let jl = (lig_off + fl.lightness).clamp(0, 255);
                        over_map_idx = get_table(jh, fl.saturation, jl);
                    }
                    let mut minimap_over = 0;
                    if over_map_idx != -2 {
                        minimap_over = palette[(get_o_col(over_map_idx, 96) as usize) & 0xFFFF];
                    }
                    if fl.mapcolour != -1 {
                        let jh = (hue_off + fl.map_hue) & 0xFF;
                        let jl = (lig_off + fl.map_lightness).clamp(0, 255);
                        let idx = get_table(jh, fl.map_saturation, jl);
                        minimap_over = palette[(get_o_col(idx, 96) as usize) & 0xFFFF];
                    }
                    world.set_ground(level, x, z, shape, rotation & 0x3, texture,
                                     h_nw, h_ne, h_se, h_sw,
                                     get_u_col(under_idx, l_nw), get_u_col(under_idx, l_ne),
                                     get_u_col(under_idx, l_se), get_u_col(under_idx, l_sw),
                                     get_o_col(over_idx, l_nw), get_o_col(over_idx, l_ne),
                                     get_o_col(over_idx, l_se), get_o_col(over_idx, l_sw),
                                     minimap_under, minimap_over);
                }
            }
        }

        // setLayer pass (Java 951-964).
        {
            let s = STATE.lock().unwrap();
            for z in 1..103i32 {
                for x in 1..103i32 {
                    let l = level as usize;
                    let eff = if (s.mapl[l][x as usize][z as usize] & 0x8) != 0 {
                        0
                    } else if level <= 0 || (s.mapl[1][x as usize][z as usize] & 0x2) == 0 {
                        level
                    } else {
                        level - 1
                    };
                    world.set_layer(level, x, z, eff);
                }
            }
        }
    }

    // shareLight + bridge pushDown (Java 971-978).
    world.share_light(-50, -10, -50);
    {
        let s = STATE.lock().unwrap();
        let bridge: Vec<(i32, i32)> = (0..104i32)
            .flat_map(|x| (0..104i32).map(move |z| (x, z)))
            .filter(|&(x, z)| (s.mapl[1][x as usize][z as usize] & 0x2) == 2)
            .collect();
        drop(s);
        for (x, z) in bridge {
            world.push_down(x, z);
        }
    }

    // Occluder run-length walk (Java 979-1117).
    let mut mapo = {
        let s = STATE.lock().unwrap();
        s.mapo.clone()
    };
    let groundh = {
        let s = STATE.lock().unwrap();
        s.ground_h.clone()
    };
    let mut bit_x = 1u16;
    let mut bit_z = 2u16;
    let mut bit_y = 4u16;
    for top_level in 0..4i32 {
        if top_level > 0 {
            bit_x <<= 3;
            bit_z <<= 3;
            bit_y <<= 3;
        }
        for level in 0..=top_level {
            let l = level as usize;
            for z in 0..=104usize {
                for x in 0..=104usize {
                    if (mapo[l][x][z] & bit_x) != 0 {
                        let mut min_z = z;
                        let mut max_z = z;
                        let mut min_l = l;
                        let mut max_l = l;
                        while min_z > 0 && (mapo[l][x][min_z - 1] & bit_x) != 0 {
                            min_z -= 1;
                        }
                        while max_z < 104 && (mapo[l][x][max_z + 1] & bit_x) != 0 {
                            max_z += 1;
                        }
                        'down: while min_l > 0 {
                            for zz in min_z..=max_z {
                                if (mapo[min_l - 1][x][zz] & bit_x) == 0 {
                                    break 'down;
                                }
                            }
                            min_l -= 1;
                        }
                        'up: while (max_l as i32) < top_level {
                            for zz in min_z..=max_z {
                                if (mapo[max_l + 1][x][zz] & bit_x) == 0 {
                                    break 'up;
                                }
                            }
                            max_l += 1;
                        }
                        let area = (max_l + 1 - min_l) * (max_z - min_z + 1);
                        if area >= 8 {
                            let min_y = groundh[max_l][x][min_z] - 240;
                            let max_y = groundh[min_l][x][min_z];
                            world.set_occlude(top_level, 1,
                                              (x as i32) * 128, (x as i32) * 128,
                                              (min_z as i32) * 128, (max_z as i32) * 128 + 128,
                                              min_y, max_y);
                            for ll in min_l..=max_l {
                                for zz in min_z..=max_z {
                                    mapo[ll][x][zz] &= !bit_x;
                                }
                            }
                        }
                    }
                    if (mapo[l][x][z] & bit_z) != 0 {
                        let mut min_x = x;
                        let mut max_x = x;
                        let mut min_l = l;
                        let mut max_l = l;
                        while min_x > 0 && (mapo[l][min_x - 1][z] & bit_z) != 0 {
                            min_x -= 1;
                        }
                        while max_x < 104 && (mapo[l][max_x + 1][z] & bit_z) != 0 {
                            max_x += 1;
                        }
                        'down2: while min_l > 0 {
                            for xx in min_x..=max_x {
                                if (mapo[min_l - 1][xx][z] & bit_z) == 0 {
                                    break 'down2;
                                }
                            }
                            min_l -= 1;
                        }
                        'up2: while (max_l as i32) < top_level {
                            for xx in min_x..=max_x {
                                if (mapo[max_l + 1][xx][z] & bit_z) == 0 {
                                    break 'up2;
                                }
                            }
                            max_l += 1;
                        }
                        let area = (max_l + 1 - min_l) * (max_x - min_x + 1);
                        if area >= 8 {
                            let min_y = groundh[max_l][min_x][z] - 240;
                            let max_y = groundh[min_l][min_x][z];
                            world.set_occlude(top_level, 2,
                                              (min_x as i32) * 128, (max_x as i32) * 128 + 128,
                                              (z as i32) * 128, (z as i32) * 128,
                                              min_y, max_y);
                            for ll in min_l..=max_l {
                                for xx in min_x..=max_x {
                                    mapo[ll][xx][z] &= !bit_z;
                                }
                            }
                        }
                    }
                    if (mapo[l][x][z] & bit_y) != 0 {
                        let mut min_x = x;
                        let mut max_x = x;
                        let mut min_z = z;
                        let mut max_z = z;
                        while min_z > 0 && (mapo[l][x][min_z - 1] & bit_y) != 0 {
                            min_z -= 1;
                        }
                        while max_z < 104 && (mapo[l][x][max_z + 1] & bit_y) != 0 {
                            max_z += 1;
                        }
                        'west: while min_x > 0 {
                            for zz in min_z..=max_z {
                                if (mapo[l][min_x - 1][zz] & bit_y) == 0 {
                                    break 'west;
                                }
                            }
                            min_x -= 1;
                        }
                        'east: while max_x < 104 {
                            for zz in min_z..=max_z {
                                if (mapo[l][max_x + 1][zz] & bit_y) == 0 {
                                    break 'east;
                                }
                            }
                            max_x += 1;
                        }
                        if (max_x - min_x + 1) * (max_z - min_z + 1) >= 4 {
                            let y = groundh[l][min_x][min_z];
                            world.set_occlude(top_level, 4,
                                              (min_x as i32) * 128, (max_x as i32) * 128 + 128,
                                              (min_z as i32) * 128, (max_z as i32) * 128 + 128,
                                              y, y);
                            for xx in min_x..=max_x {
                                for zz in min_z..=max_z {
                                    mapo[l][xx][zz] &= !bit_y;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // Persist the consumed bit grid (Java mutates the static in place).
    STATE.lock().unwrap().mapo = mapo;
}

// @ObfuscatedName("bk.y(III)Z") — ClientBuild.changeLocAvailable.
// Verbatim port of ClientBuild.java:1228-1237. Maps the shape ID to
// LocType.checkModel's canonical form (11 → 10; 5..8 → 4) and asks
// whether all the model groups for that shape are cached locally.
//
// `loc_change_do_queue` calls this before applying a queued change
// so the geometry swap doesn't tear when the new model isn't loaded.
pub fn change_loc_available(loc_id: i32, mut shape: i32) -> bool {
    let Some(loc) = crate::config::loc_type::list(loc_id) else { return true; };
    if shape == 11 { shape = 10; }
    if (5..=8).contains(&shape) { shape = 4; }
    loc.check_model(shape)
}
