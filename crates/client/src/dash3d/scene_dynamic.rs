// Dynamic scene entities — Sprite (dynamic loc), LocChange (timed
// loc swap), MapSpotAnim (spotanim projectile/impact). Each is small
// and self-contained; grouped together for the same reason as
// scene_tile.rs.

#![allow(dead_code)]

// @ObfuscatedName("fy") — jag::oldscape::dash3d::ModelSource (abstract).
//
// Java's abstract base for anything that yields a ModelLit at render
// time (Loc, ClientLocAnim, ClientNpc, ClientPlayer, ClientProj). In
// Rust we model it as a small trait the renderer can dispatch on; the
// individual entity structs implement `get_temp_model` to compose
// their model.
pub trait ModelSource {
    /// Returns the (model_id, kind, rotation) triple the renderer
    /// resolves through `fetch_loc_model`. Returns `None` if the
    /// source isn't ready (e.g. ClientNpc.type still null).
    fn temp_model_ref(&self) -> Option<(i32, i32, i32)>;
}

// @ObfuscatedName("av") — jag::oldscape::dash3d::Sprite. Dynamic-loc
// record stamped into Square.sprites[]. Java holds up to 5 of these
// per tile (kind 9-12 sprites placed by addScenery).
#[derive(Debug, Clone)]
pub struct Sprite {
    // @ObfuscatedName("av.j")
    pub level: i32,
    pub min_tile_x: i32,
    pub max_tile_x: i32,
    pub min_tile_z: i32,
    pub max_tile_z: i32,
    pub yaw: i32,
    pub cycle: i32,
    pub model_id: i32,
    pub typecode: i32,
    // @ObfuscatedName("av.e") — bit-packed: high 2 bits are loc kind,
    // bits 21..28 are rotation, low 21 bits are loc id.
    pub info_bits: i32,
}

impl Sprite {
    pub fn new(level: i32, min_x: i32, max_x: i32, min_z: i32, max_z: i32,
               model_id: i32, yaw: i32, typecode: i32, info: i32) -> Self {
        Self {
            level,
            min_tile_x: min_x,
            max_tile_x: max_x,
            min_tile_z: min_z,
            max_tile_z: max_z,
            yaw,
            cycle: 0,
            model_id,
            typecode,
            info_bits: info,
        }
    }
}

// @ObfuscatedName("bs") — jag::oldscape::dash3d::LocChange. Temporary
// loc state override with a tick-counted lifetime. Drives door-swing,
// trapdoor-open, chest-open animations from server LOC_ANIM packets.
#[derive(Debug, Clone)]
pub struct LocChange {
    // @ObfuscatedName("bs.j")
    pub level: i32,
    // @ObfuscatedName("bs.z")
    pub x: i32,
    // @ObfuscatedName("bs.g")
    pub z: i32,
    // @ObfuscatedName("bs.q") — kind / shape (0-22).
    pub kind: i32,
    // @ObfuscatedName("bs.i") — rotation (0-7).
    pub rotation: i32,
    // @ObfuscatedName("bs.s") — new loc id (or -1 to delete).
    pub new_id: i32,
    // @ObfuscatedName("bs.u") — loop cycle this change was queued at.
    pub start_cycle: i32,
    // @ObfuscatedName("bs.v") — loop cycle this change expires.
    pub end_cycle: i32,
    // @ObfuscatedName("bs.w") — saved (previous) loc id / kind /
    // rotation so the change can be reverted at end_cycle.
    pub saved_id: i32,
    pub saved_kind: i32,
    pub saved_rotation: i32,
}

impl LocChange {
    pub fn new(level: i32, x: i32, z: i32, kind: i32, rotation: i32,
               new_id: i32, start_cycle: i32, end_cycle: i32,
               saved_id: i32, saved_kind: i32, saved_rotation: i32) -> Self {
        Self {
            level, x, z, kind, rotation, new_id,
            start_cycle, end_cycle,
            saved_id, saved_kind, saved_rotation,
        }
    }
}

// @ObfuscatedName("bv") — jag::oldscape::dash3d::MapSpotAnim.
//
// SpotType-driven impact animation pinned to a world tile (cast
// effects, splat animations, ground-targeted spell impacts). Distinct
// from ClientProj which is the moving projectile that PRECEDES the
// impact; MapSpotAnim is the stationary impact at the destination.
#[derive(Debug, Clone)]
pub struct MapSpotAnim {
    // @ObfuscatedName("bv.j") — SpotType id.
    pub spotanim_id: i32,
    pub level: i32,
    pub tile_x: i32,
    pub tile_z: i32,
    // @ObfuscatedName("bv.q")
    pub height: i32,
    // @ObfuscatedName("bv.i") — first render cycle.
    pub start_cycle: i32,
    // @ObfuscatedName("bv.s") — current animation frame.
    pub anim_frame: i32,
    // @ObfuscatedName("bv.u") — running cycle counter for delay.
    pub anim_cycle: i32,
}

impl MapSpotAnim {
    pub fn new(spotanim_id: i32, level: i32, tile_x: i32, tile_z: i32,
               height: i32, start_cycle: i32) -> Self {
        Self {
            spotanim_id, level, tile_x, tile_z, height,
            start_cycle, anim_frame: 0, anim_cycle: 0,
        }
    }

    // @ObfuscatedName("bv.y(II)Z") — MapSpotAnim.doAnim. Advances the
    // frame by `cycles` ticks using SpotType.list(id).anim's delay
    // table. Returns `true` when the animation completes (caller
    // should drop the entity).
    pub fn do_anim(&mut self, cycles: i32) -> bool {
        use crate::config::{spot_type, seq_type};
        let spot = spot_type::list(self.spotanim_id);
        if spot.anim == -1 { return true; }
        let seq = seq_type::list(spot.anim);
        let Some(frames) = seq.frames.as_ref() else { return true; };
        let Some(delays) = seq.delay.as_ref() else { return true; };
        if frames.is_empty() || delays.is_empty() { return true; }
        self.anim_cycle += cycles;
        while (self.anim_frame as usize) < frames.len() {
            let d = delays[self.anim_frame as usize];
            if self.anim_cycle <= d { return false; }
            self.anim_cycle -= d;
            self.anim_frame += 1;
        }
        // Hit end of frames — apply Java's loop wrap.
        self.anim_frame -= seq.loops;
        if self.anim_frame < 0 || self.anim_frame >= frames.len() as i32 {
            return true;
        }
        false
    }
}

// @ObfuscatedName("cw") — jag::oldscape::util::RegionRotate.
// Verbatim port of RegionRotate.java. Used by instanced-region
// loaders to remap (dx, dz) within an 8×8 zone tile when the zone
// is placed at a non-zero rotation, and to handle the mirror flip
// when the rotation argument has bit 0 set (Java's `arg5 & 0x1`).
pub mod region_rotate {
    // @ObfuscatedName("bf.r(IIIIIIB)I") — RegionRotate.DX.
    // Verbatim port of RegionRotate.java:15-31.
    //
    // Args: (dx, dz, rotation, w, h, mirror). `mirror & 1` swaps
    // (w, h) before the rotate; the resulting per-quadrant case
    // matches Java 1:1.
    pub fn dx(dx: i32, dz: i32, rotation: i32, mut w: i32, mut h: i32, mirror: i32) -> i32 {
        if mirror & 0x1 == 1 {
            std::mem::swap(&mut w, &mut h);
        }
        match rotation & 0x3 {
            0 => dx,
            1 => dz,
            2 => 7 - dx - (w - 1),
            _ => 7 - dz - (h - 1),
        }
    }

    // @ObfuscatedName("bg.d(IIIIIII)I") — RegionRotate.DZ.
    // Verbatim port of RegionRotate.java:35-51.
    pub fn dz(dx: i32, dz: i32, rotation: i32, mut w: i32, mut h: i32, mirror: i32) -> i32 {
        if mirror & 0x1 == 1 {
            std::mem::swap(&mut w, &mut h);
        }
        match rotation & 0x3 {
            0 => dz,
            1 => 7 - dx - (w - 1),
            2 => 7 - dz - (h - 1),
            _ => dx,
        }
    }
}
