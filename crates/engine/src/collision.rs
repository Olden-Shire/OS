//! Server-side collision map.
//!
//! OS is 2007-rev, so the authoritative collision model is the one the
//! Java client uses (`jagex3 ... CollisionMap`, already ported verbatim in
//! `crates/client/src/dash3d/collision_map.rs`). This module reuses that exact
//! flag scheme and footprint logic, but world-spanning (region-bucketed) and
//! per-level instead of a single 104×104 scene grid, so the RuneScript engine
//! can answer the map ops (`map_blocked`, `lineofsight`, `lineofwalk`,
//! `map_findsquare`, …) and pathfind script-driven moves.
//!
//! The bit constants are deeply load-bearing and match the Java source 1:1:
//!   * cardinal walls — N `0x2`, E `0x8`, S `0x20`, W `0x80`
//!   * corner walls   — NW `0x1`, NE `0x4`, SE `0x10`, SW `0x40`
//!   * loc (full walk block) `0x100`, ground decor `0x40000`, floor `0x200000`
//!   * projectile/range variants — N `0x400`, E `0x1000`, S `0x4000`, W `0x10000`,
//!     corners NW `0x200`/NE `0x800`/SE `0x2000`/SW `0x8000`, loc `0x20000`
//!
//! The Engine-TS reference (`GameMap.ts` → rsmod-pathfinder) provides the *op*
//! semantics (isMapBlocked = WALK_BLOCKED, isIndoors = ROOF, line-of-sight uses
//! the projectile flags). rsmod ships only as a WASM blob, so the LOS / walk /
//! pathfinding here are clean tile-stepping implementations over the authentic
//! 2007 flags rather than a byte-port of rsmod's sub-tile interpolation; they
//! agree with rsmod on every cardinal/wall case and approximate it only in the
//! sub-tile diagonal corner cases (decomposed conservatively here).

#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};

const RS: usize = 64; // region edge in tiles
const RS_I: i32 = 64;
const LEVELS: usize = 4;

// ── flag bits (1:1 with CollisionMap.java) ──────────────────────────────
pub const WALL_NW: i32 = 0x1;
pub const WALL_N: i32 = 0x2;
pub const WALL_NE: i32 = 0x4;
pub const WALL_E: i32 = 0x8;
pub const WALL_SE: i32 = 0x10;
pub const WALL_S: i32 = 0x20;
pub const WALL_SW: i32 = 0x40;
pub const WALL_W: i32 = 0x80;
pub const LOC: i32 = 0x100;
pub const GROUND_DECOR: i32 = 0x40000;
pub const FLOOR: i32 = 0x200000;
// projectile / ranged-attack variants
pub const RWALL_NW: i32 = 0x200;
pub const RWALL_N: i32 = 0x400;
pub const RWALL_NE: i32 = 0x800;
pub const RWALL_E: i32 = 0x1000;
pub const RWALL_SE: i32 = 0x2000;
pub const RWALL_S: i32 = 0x4000;
pub const RWALL_SW: i32 = 0x8000;
pub const RWALL_W: i32 = 0x10000;
pub const LOC_RANGE: i32 = 0x20000;
/// Entity-occupancy flags (rsmod custom flags `CollisionFlag.NPC` / `.PLAYER`).
/// An npc with `BlockWalk::Npc` stamps `NPC` on its footprint; a player or
/// `BlockWalk::All` npc also stamps `PLAYER`. Passed as the `extra_flag` to
/// [`WorldCollision::can_travel`] so entities route around each other 1:1 with
/// Engine-TS (a moving npc checks `BLOCK_* | NPC`, a player checks `| PLAYER`).
pub const NPC: i32 = 0x80000;
pub const PLAYER: i32 = 0x100000;

/// A tile you cannot stand on — Engine-TS `WALK_BLOCKED` minus entity flags
/// (rsmod `FLOOR | FLOOR_DECORATION | LOC`; our `GROUND_DECOR == FLOOR_DECORATION`).
pub const WALK_BLOCKED: i32 = FLOOR | LOC | GROUND_DECOR;

/// rsmod `CollisionType` — chooses which flag set [`WorldCollision::can_travel`]
/// tests, derived from an entity's `MoveRestrict` (Engine-TS
/// `PathingEntity.getCollisionStrategy`). `Normal` is the walk case used by
/// players and ordinary npcs; `Indoors`/`Outdoors` add a roof constraint.
/// `Blocked`/`LineOfSight` (exotic npc move-restricts) fall back to `Normal`'s
/// wall semantics — documented approximation, no 2007 npc uses them on tutorial
/// island.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CollisionStrategy {
    #[default]
    Normal,
    Blocked,
    Indoors,
    Outdoors,
    LineOfSight,
}

/// Per-region flag + roof grids, indexed `[level][x*RS + z]` (local 0..63).
#[derive(Clone)]
struct RegionFlags {
    flags: Vec<i32>,
    roof: Vec<bool>,
}

impl RegionFlags {
    fn new() -> Self {
        Self {
            flags: vec![0; RS * RS * LEVELS],
            roof: vec![false; RS * RS * LEVELS],
        }
    }
}

#[inline]
fn region_key(x: i32, z: i32) -> u32 {
    (((x >> 6) & 0xff) as u32) << 8 | ((z >> 6) & 0xff) as u32
}

#[inline]
fn local_index(level: i32, x: i32, z: i32) -> usize {
    let lx = (x & 63) as usize;
    let lz = (z & 63) as usize;
    (level as usize) * RS * RS + lx * RS + lz
}

/// Engine-TS `ZoneMap.zoneIndex` — 8×8 zone id (for multiway lookup).
#[inline]
pub fn zone_index(x: i32, z: i32, level: i32) -> u32 {
    (((x >> 3) & 0x7ff) | (((z >> 3) & 0x7ff) << 11) | ((level & 0x3) << 22)) as u32
}

/// 8-way step directions, OSRS order isn't needed here — just (dx, dz).
const DIAGS: [(i32, i32); 8] = [
    (0, 1),   // N
    (1, 1),   // NE
    (1, 0),   // E
    (1, -1),  // SE
    (0, -1),  // S
    (-1, -1), // SW
    (-1, 0),  // W
    (-1, 1),  // NW
];

#[derive(Clone, Default)]
pub struct WorldCollision {
    regions: HashMap<u32, RegionFlags>,
    /// Zone indices flagged multiway (PvP). No 2007 data source in our cache
    /// yet, so this is empty by default and `is_multiway` returns false.
    pub multiway: std::collections::HashSet<u32>,
}

impl WorldCollision {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// True once any region collision has been loaded — lets movement / map
    /// ops stay permissive in unit tests (which never load a map).
    #[must_use]
    pub fn is_loaded(&self) -> bool {
        !self.regions.is_empty()
    }

    #[inline]
    fn flag(&self, x: i32, z: i32, level: i32) -> i32 {
        if !(0..LEVELS as i32).contains(&level) {
            return 0;
        }
        self.regions
            .get(&region_key(x, z))
            .map_or(0, |r| r.flags[local_index(level, x, z)])
    }

    fn region_mut(&mut self, x: i32, z: i32) -> &mut RegionFlags {
        self.regions.entry(region_key(x, z)).or_insert_with(RegionFlags::new)
    }

    // @ObfuscatedName addCMap — OR bits into a tile.
    fn add(&mut self, x: i32, z: i32, level: i32, bits: i32) {
        if !(0..LEVELS as i32).contains(&level) {
            return;
        }
        let i = local_index(level, x, z);
        self.region_mut(x, z).flags[i] |= bits;
    }

    // @ObfuscatedName remCMap — clear bits from a tile.
    fn remove(&mut self, x: i32, z: i32, level: i32, bits: i32) {
        if !(0..LEVELS as i32).contains(&level) {
            return;
        }
        if let Some(r) = self.regions.get_mut(&region_key(x, z)) {
            r.flags[local_index(level, x, z)] &= !bits;
        }
    }

    /// Mark a tile blocked for standing (terrain BLOCK_MAP_SQUARE) — Java
    /// `CollisionMap.blockGround`.
    pub fn block_ground(&mut self, x: i32, z: i32, level: i32) {
        self.add(x, z, level, FLOOR);
    }

    /// Mark/clear a roof tile (terrain REMOVE_ROOFS bit) for `map_indoors`.
    pub fn set_roof(&mut self, x: i32, z: i32, level: i32, on: bool) {
        if !(0..LEVELS as i32).contains(&level) {
            return;
        }
        let i = local_index(level, x, z);
        self.region_mut(x, z).roof[i] = on;
    }

    #[must_use]
    pub fn is_indoors(&self, x: i32, z: i32, level: i32) -> bool {
        if !(0..LEVELS as i32).contains(&level) {
            return false;
        }
        self.regions
            .get(&region_key(x, z))
            .is_some_and(|r| r.roof[local_index(level, x, z)])
    }

    #[must_use]
    pub fn is_multiway(&self, x: i32, z: i32, level: i32) -> bool {
        self.multiway.contains(&zone_index(x, z, level))
    }

    // @ObfuscatedName addWall / delWall — verbatim footprint from
    // CollisionMap.java, with an explicit `level` and absolute coords.
    fn wall(&mut self, x: i32, z: i32, level: i32, kind: i32, rot: i32, blockrange: bool, add: bool) {
        let op = |s: &mut Self, tx: i32, tz: i32, bits: i32| {
            if add { s.add(tx, tz, level, bits) } else { s.remove(tx, tz, level, bits) }
        };
        if kind == 0 {
            match rot {
                0 => { op(self, x, z, 128); op(self, x - 1, z, 8); }
                1 => { op(self, x, z, 2);   op(self, x, z + 1, 32); }
                2 => { op(self, x, z, 8);   op(self, x + 1, z, 128); }
                3 => { op(self, x, z, 32);  op(self, x, z - 1, 2); }
                _ => {}
            }
        } else if kind == 1 || kind == 3 {
            match rot {
                0 => { op(self, x, z, 1);  op(self, x - 1, z + 1, 16); }
                1 => { op(self, x, z, 4);  op(self, x + 1, z + 1, 64); }
                2 => { op(self, x, z, 16); op(self, x + 1, z - 1, 1); }
                3 => { op(self, x, z, 64); op(self, x - 1, z - 1, 4); }
                _ => {}
            }
        } else if kind == 2 {
            match rot {
                0 => { op(self, x, z, 130); op(self, x - 1, z, 8);   op(self, x, z + 1, 32); }
                1 => { op(self, x, z, 10);  op(self, x, z + 1, 32);  op(self, x + 1, z, 128); }
                2 => { op(self, x, z, 40);  op(self, x + 1, z, 128); op(self, x, z - 1, 2); }
                3 => { op(self, x, z, 160); op(self, x, z - 1, 2);   op(self, x - 1, z, 8); }
                _ => {}
            }
        }
        if blockrange {
            if kind == 0 {
                match rot {
                    0 => { op(self, x, z, 65536); op(self, x - 1, z, 4096); }
                    1 => { op(self, x, z, 1024);  op(self, x, z + 1, 16384); }
                    2 => { op(self, x, z, 4096);  op(self, x + 1, z, 65536); }
                    3 => { op(self, x, z, 16384); op(self, x, z - 1, 1024); }
                    _ => {}
                }
            } else if kind == 1 || kind == 3 {
                match rot {
                    0 => { op(self, x, z, 512);   op(self, x - 1, z + 1, 8192); }
                    1 => { op(self, x, z, 2048);  op(self, x + 1, z + 1, 32768); }
                    2 => { op(self, x, z, 8192);  op(self, x + 1, z - 1, 512); }
                    3 => { op(self, x, z, 32768); op(self, x - 1, z - 1, 2048); }
                    _ => {}
                }
            } else if kind == 2 {
                match rot {
                    0 => { op(self, x, z, 66560); op(self, x - 1, z, 4096);  op(self, x, z + 1, 16384); }
                    1 => { op(self, x, z, 5120);  op(self, x, z + 1, 16384); op(self, x + 1, z, 65536); }
                    2 => { op(self, x, z, 20480); op(self, x + 1, z, 65536); op(self, x, z - 1, 1024); }
                    3 => { op(self, x, z, 81920); op(self, x, z - 1, 1024);  op(self, x - 1, z, 4096); }
                    _ => {}
                }
            }
        }
    }

    // @ObfuscatedName addLoc / delLoc — rectangular footprint. The caller has
    // already swapped (lw, ll) for the rotation.
    fn loc(&mut self, x: i32, z: i32, level: i32, lw: i32, ll: i32, blockrange: bool, add: bool) {
        let mut bits = LOC;
        if blockrange {
            bits |= LOC_RANGE;
        }
        for tx in x..(x + lw) {
            for tz in z..(z + ll) {
                if add { self.add(tx, tz, level, bits) } else { self.remove(tx, tz, level, bits) }
            }
        }
    }

    fn ground_decor(&mut self, x: i32, z: i32, level: i32, add: bool) {
        if add { self.add(x, z, level, GROUND_DECOR) } else { self.remove(x, z, level, GROUND_DECOR) }
    }

    /// Apply (or remove) a loc's collision footprint, dispatching on its shape
    /// exactly like the client's `ClientBuild::add_loc`:
    ///   * shape 0/1/2/3 → wall, shape 9/10/11/≥12 → rectangular loc,
    ///     shape 22 → ground decor (only when blockwalk == 1),
    ///     shapes 4..=8 → wall decor (no collision).
    /// `width`/`length` are the loc's UN-rotated config size.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_loc(
        &mut self,
        x: i32,
        z: i32,
        level: i32,
        shape: i32,
        rotation: i32,
        width: i32,
        length: i32,
        blockwalk: i32,
        blockrange: bool,
        add: bool,
    ) {
        let (lw, ll) = if rotation == 1 || rotation == 3 {
            (length, width)
        } else {
            (width, length)
        };
        if shape == 22 {
            if blockwalk == 1 {
                self.ground_decor(x, z, level, add);
            }
        } else if blockwalk != 0 {
            if (0..=3).contains(&shape) {
                self.wall(x, z, level, shape, rotation, blockrange, add);
            } else {
                // shape 9, 10, 11, ≥12 all register a rectangular footprint.
                self.loc(x, z, level, lw, ll, blockrange, add);
            }
        }
    }

    /// Engine-TS `isMapBlocked` — true when the tile can't be stood on.
    #[must_use]
    pub fn is_blocked(&self, x: i32, z: i32, level: i32) -> bool {
        self.flag(x, z, level) & WALK_BLOCKED != 0
    }

    /// Can a 1×1 entity take a single cardinal step from `(x, z)` in
    /// direction `(dx, dz)` (one of N/E/S/W)? `los` switches the masks from
    /// walk-block to projectile-block (used by line-of-sight).
    fn can_cardinal(&self, x: i32, z: i32, level: i32, dx: i32, dz: i32, los: bool) -> bool {
        let nx = x + dx;
        let nz = z + dz;
        let dest = self.flag(nx, nz, level);
        let (block, wall) = if los {
            // projectiles ignore the floor/loc walk block; only loc-range and
            // the opposite-edge ranged wall stop them.
            let wall = match (dx, dz) {
                (0, 1) => RWALL_S,
                (0, -1) => RWALL_N,
                (1, 0) => RWALL_W,
                (-1, 0) => RWALL_E,
                _ => 0,
            };
            (LOC_RANGE, wall)
        } else {
            let wall = match (dx, dz) {
                (0, 1) => WALL_S,
                (0, -1) => WALL_N,
                (1, 0) => WALL_W,
                (-1, 0) => WALL_E,
                _ => 0,
            };
            (WALK_BLOCKED, wall)
        };
        dest & (block | wall) == 0
    }

    /// Line of walk (Engine-TS `isLineOfWalk`) / line of sight
    /// (`isLineOfSight`, `los = true`). Bresenham tile walk; diagonal steps are
    /// decomposed into two cardinal steps and BOTH must be clear (no corner
    /// cutting). Returns true when the whole segment is traversable.
    fn ray(&self, level: i32, x0: i32, z0: i32, x1: i32, z1: i32, los: bool) -> bool {
        if !self.is_loaded() {
            return true;
        }
        let mut x = x0;
        let mut z = z0;
        let dx = (x1 - x0).abs();
        let dz = (z1 - z0).abs();
        let sx = (x1 - x0).signum();
        let sz = (z1 - z0).signum();
        let mut err = dx - dz;
        while x != x1 || z != z1 {
            let e2 = 2 * err;
            let step_x = e2 > -dz;
            let step_z = e2 < dx;
            if step_x && step_z {
                // Diagonal: require an L-route (x-then-z) with both legs clear.
                if !self.can_cardinal(x, z, level, sx, 0, los) {
                    return false;
                }
                if !self.can_cardinal(x + sx, z, level, 0, sz, los) {
                    return false;
                }
                err -= dz;
                err += dx;
                x += sx;
                z += sz;
            } else if step_x {
                if !self.can_cardinal(x, z, level, sx, 0, los) {
                    return false;
                }
                err -= dz;
                x += sx;
            } else {
                if !self.can_cardinal(x, z, level, 0, sz, los) {
                    return false;
                }
                err += dx;
                z += sz;
            }
        }
        true
    }

    #[must_use]
    pub fn line_of_walk(&self, level: i32, x0: i32, z0: i32, x1: i32, z1: i32) -> bool {
        self.ray(level, x0, z0, x1, z1, false)
    }

    #[must_use]
    pub fn line_of_sight(&self, level: i32, x0: i32, z0: i32, x1: i32, z1: i32) -> bool {
        self.ray(level, x0, z0, x1, z1, true)
    }

    /// rsmod `canTravel` (Engine-TS `GameMap.canTravel`): can a `size`×`size`
    /// entity at `(x, z)` step one tile in 8-way direction `(dx, dz)`? The
    /// per-direction masks are the exact rsmod `BLOCK_*` constants (cardinal =
    /// `WALK_BLOCKED | <near wall>`, diagonal = the corner mask on the
    /// destination plus the two adjoining cardinals, which prevents corner
    /// cutting). `extra_flag` is OR'd into every tested tile so npc/player
    /// occupancy (`NPC`/`PLAYER`) blocks too. `strategy` adds the roof
    /// constraint for `Indoors`/`Outdoors`.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn can_travel(
        &self, level: i32, x: i32, z: i32, dx: i32, dz: i32, size: i32,
        extra_flag: i32, strategy: CollisionStrategy,
    ) -> bool {
        if dx == 0 && dz == 0 {
            return true;
        }
        if size <= 1 {
            return self.can_travel_unit(level, x, z, dx, dz, extra_flag, strategy);
        }
        // Multi-tile footprint: the entity occupies [x, x+size) × [z, z+size).
        // Conservative 1:1-for-the-edge check — every tile the footprint newly
        // sweeps into must be enterable as a unit from its in-footprint
        // neighbour. (size-1 npcs hit the exact rsmod path above; >1 npcs are
        // rare and never appear on tutorial island.)
        for tx in x..x + size {
            for tz in z..z + size {
                let inside_next = (tx + dx) >= x && (tx + dx) < x + size
                    && (tz + dz) >= z && (tz + dz) < z + size;
                if inside_next {
                    continue; // staying within the footprint, not a leading edge
                }
                if !self.can_travel_unit(level, tx, tz, dx, dz, extra_flag, strategy) {
                    return false;
                }
            }
        }
        true
    }

    fn can_travel_unit(
        &self, level: i32, x: i32, z: i32, dx: i32, dz: i32,
        extra_flag: i32, strategy: CollisionStrategy,
    ) -> bool {
        let w = WALK_BLOCKED;
        let blocked = |tx: i32, tz: i32, mask: i32| {
            self.flag(tx, tz, level) & (mask | extra_flag) != 0
        };
        let walkable = match (dx, dz) {
            (1, 0) => !blocked(x + 1, z, w | WALL_W),
            (-1, 0) => !blocked(x - 1, z, w | WALL_E),
            (0, 1) => !blocked(x, z + 1, w | WALL_S),
            (0, -1) => !blocked(x, z - 1, w | WALL_N),
            // BLOCK_NORTH_EAST on dest + BLOCK_EAST/BLOCK_NORTH on adjoiners.
            (1, 1) => {
                !blocked(x + 1, z + 1, w | WALL_W | WALL_S | WALL_SW)
                    && !blocked(x + 1, z, w | WALL_W)
                    && !blocked(x, z + 1, w | WALL_S)
            }
            (-1, 1) => {
                !blocked(x - 1, z + 1, w | WALL_E | WALL_S | WALL_SE)
                    && !blocked(x - 1, z, w | WALL_E)
                    && !blocked(x, z + 1, w | WALL_S)
            }
            (1, -1) => {
                !blocked(x + 1, z - 1, w | WALL_W | WALL_N | WALL_NW)
                    && !blocked(x + 1, z, w | WALL_W)
                    && !blocked(x, z - 1, w | WALL_N)
            }
            (-1, -1) => {
                !blocked(x - 1, z - 1, w | WALL_E | WALL_N | WALL_NE)
                    && !blocked(x - 1, z, w | WALL_E)
                    && !blocked(x, z - 1, w | WALL_N)
            }
            _ => return true,
        };
        if !walkable {
            return false;
        }
        match strategy {
            CollisionStrategy::Indoors => self.is_indoors(x + dx, z + dz, level),
            CollisionStrategy::Outdoors => !self.is_indoors(x + dx, z + dz, level),
            _ => true,
        }
    }

    /// Can a 1×1 entity move one step in 8-way direction `(dx, dz)`? Thin
    /// wrapper over [`can_travel`] for the BFS pathfinder (no entity occupancy).
    fn can_step(&self, x: i32, z: i32, level: i32, dx: i32, dz: i32) -> bool {
        self.can_travel(level, x, z, dx, dz, 1, 0, CollisionStrategy::Normal)
    }

    /// rsmod `changeNpc` (Engine-TS `changeNpcCollision`): stamp/clear the `NPC`
    /// occupancy flag over an npc's `size`×`size` footprint at `(x, z)`.
    pub fn change_npc(&mut self, x: i32, z: i32, level: i32, size: i32, add: bool) {
        for tx in x..x + size.max(1) {
            for tz in z..z + size.max(1) {
                if add { self.add(tx, tz, level, NPC) } else { self.remove(tx, tz, level, NPC) }
            }
        }
    }

    /// rsmod `changePlayer` (Engine-TS `changePlayerCollision`): stamp/clear the
    /// `PLAYER` occupancy flag over a `size`×`size` footprint at `(x, z)`.
    pub fn change_player(&mut self, x: i32, z: i32, level: i32, size: i32, add: bool) {
        for tx in x..x + size.max(1) {
            for tz in z..z + size.max(1) {
                if add { self.add(tx, tz, level, PLAYER) } else { self.remove(tx, tz, level, PLAYER) }
            }
        }
    }

    /// BFS pathfind from `(sx, sz)` toward `(dx, dz)` on `level`, returning the
    /// reduced waypoint list (turn points, destination last). Empty when the
    /// source region has no collision loaded (caller falls back to a direct
    /// move) or no route exists. `move_near` lets the path stop on the tile
    /// nearest the goal when the goal itself is unreachable. Search is bounded
    /// to a 128-tile box around the source.
    #[must_use]
    pub fn find_path(&self, level: i32, sx: i32, sz: i32, dx: i32, dz: i32, move_near: bool) -> Vec<(i32, i32)> {
        if !self.is_loaded() {
            return Vec::new();
        }
        if sx == dx && sz == dz {
            return Vec::new();
        }
        const R: i32 = 64; // half-window
        let (minx, maxx) = (sx - R, sx + R);
        let (minz, maxz) = (sz - R, sz + R);
        if dx < minx || dx > maxx || dz < minz || dz > maxz {
            // out of search window; caller decides (likely a direct walk)
            return Vec::new();
        }
        let w = (maxx - minx + 1) as usize;
        let h = (maxz - minz + 1) as usize;
        let idx = |x: i32, z: i32| ((x - minx) as usize) * h + (z - minz) as usize;
        let mut prev = vec![-1i32; w * h];
        let mut seen = vec![false; w * h];
        let start = idx(sx, sz);
        seen[start] = true;
        let mut queue = VecDeque::new();
        queue.push_back((sx, sz));
        let mut best = (sx, sz);
        let mut best_dist = (sx - dx).abs().max((sz - dz).abs());
        let mut reached = false;
        while let Some((x, z)) = queue.pop_front() {
            if x == dx && z == dz {
                reached = true;
                best = (x, z);
                break;
            }
            let d = (x - dx).abs().max((z - dz).abs());
            if d < best_dist {
                best_dist = d;
                best = (x, z);
            }
            for (ddx, ddz) in DIAGS {
                let nx = x + ddx;
                let nz = z + ddz;
                if nx < minx || nx > maxx || nz < minz || nz > maxz {
                    continue;
                }
                let ni = idx(nx, nz);
                if seen[ni] {
                    continue;
                }
                if !self.can_step(x, z, level, ddx, ddz) {
                    continue;
                }
                seen[ni] = true;
                prev[ni] = idx(x, z) as i32;
                queue.push_back((nx, nz));
            }
        }
        if !reached && !move_near {
            return Vec::new();
        }
        // Reconstruct full tile path from best back to start.
        let mut tiles = Vec::new();
        let mut cur = idx(best.0, best.1);
        loop {
            let x = minx + (cur / h) as i32;
            let z = minz + (cur % h) as i32;
            tiles.push((x, z));
            if cur == start {
                break;
            }
            let p = prev[cur];
            if p < 0 {
                break;
            }
            cur = p as usize;
        }
        tiles.reverse();
        // Reduce to turn points (drop the start tile).
        let mut waypoints = Vec::new();
        for i in 1..tiles.len() {
            let keep = if i == tiles.len() - 1 {
                true
            } else {
                let (px, pz) = tiles[i - 1];
                let (cx, cz) = tiles[i];
                let (nx, nz) = tiles[i + 1];
                (cx - px, cz - pz) != (nx - cx, nz - cz)
            };
            if keep {
                waypoints.push(tiles[i]);
            }
        }
        waypoints
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded() -> WorldCollision {
        let mut c = WorldCollision::new();
        // Touch a region so `is_loaded()` is true and queries are enforced.
        c.set_roof(3200, 3200, 0, false);
        c
    }

    #[test]
    fn blocked_floor_and_loc() {
        let mut c = loaded();
        assert!(!c.is_blocked(3200, 3200, 0));
        c.block_ground(3200, 3200, 0);
        assert!(c.is_blocked(3200, 3200, 0));
        // A rectangular loc blocks its whole footprint.
        c.apply_loc(3210, 3210, 0, 10, 0, 2, 2, 2, true, true);
        for x in 3210..3212 {
            for z in 3210..3212 {
                assert!(c.is_blocked(x, z, 0), "loc tile {x},{z}");
            }
        }
    }

    #[test]
    fn roof_indoors() {
        let mut c = loaded();
        assert!(!c.is_indoors(3200, 3200, 0));
        c.set_roof(3200, 3200, 0, true);
        assert!(c.is_indoors(3200, 3200, 0));
    }

    #[test]
    fn wall_blocks_line_of_walk_but_open_otherwise() {
        let mut c = loaded();
        // Clear straight walk east is fine.
        assert!(c.line_of_walk(0, 3200, 3200, 3205, 3200));
        // A straight wall (shape 0, rot 0 = west edge) on the tile at 3203
        // blocks crossing its west edge from 3202 → 3203.
        c.apply_loc(3203, 3200, 0, 0, 0, 1, 1, 2, false, true);
        assert!(!c.line_of_walk(0, 3200, 3200, 3205, 3200));
    }

    #[test]
    fn line_of_sight_ignores_walk_only_loc() {
        let mut c = loaded();
        // A loc that blocks walk but NOT range (blockrange = false) stops walk
        // but not sight.
        c.apply_loc(3203, 3200, 0, 10, 0, 1, 1, 2, false, true);
        assert!(!c.line_of_walk(0, 3200, 3200, 3206, 3200));
        assert!(c.line_of_sight(0, 3200, 3200, 3206, 3200));
        // A range-blocking loc stops sight too.
        c.apply_loc(3203, 3201, 0, 10, 0, 1, 1, 2, true, true);
        assert!(!c.line_of_sight(0, 3200, 3201, 3206, 3201));
    }

    #[test]
    fn find_path_routes_around_a_wall_loc() {
        let mut c = loaded();
        // Block a vertical run of floor at x=3203, z=3199..3201, forcing a detour.
        for z in 3199..=3201 {
            c.apply_loc(3203, z, 0, 10, 0, 1, 1, 2, false, true);
        }
        let path = c.find_path(0, 3200, 3200, 3206, 3200, true);
        assert!(!path.is_empty(), "should find a detour path");
        assert_eq!(*path.last().unwrap(), (3206, 3200), "ends at the goal");
    }

    #[test]
    fn can_travel_blocks_a_step_through_a_wall() {
        let mut c = loaded();
        // Open east step is fine.
        assert!(c.can_travel(0, 3200, 3200, 1, 0, 1, 0, CollisionStrategy::Normal));
        // A wall on the west edge of 3201 (shape 0 rot 0) blocks stepping east
        // from 3200 → 3201, but not the reverse-side approaches.
        c.apply_loc(3201, 3200, 0, 0, 0, 1, 1, 2, false, true);
        assert!(!c.can_travel(0, 3200, 3200, 1, 0, 1, 0, CollisionStrategy::Normal));
    }

    #[test]
    fn can_travel_diagonal_needs_both_adjoining_tiles() {
        let mut c = loaded();
        assert!(c.can_travel(0, 3200, 3200, 1, 1, 1, 0, CollisionStrategy::Normal));
        // Block the north tile (floor): the NE diagonal can no longer be taken
        // (corner cutting is disallowed).
        c.block_ground(3200, 3201, 0);
        assert!(!c.can_travel(0, 3200, 3200, 1, 1, 1, 0, CollisionStrategy::Normal));
    }

    #[test]
    fn npc_occupancy_blocks_only_with_the_npc_extra_flag() {
        let mut c = loaded();
        c.change_npc(3201, 3200, 0, 1, true);
        // An npc (extra_flag = NPC) can't step onto the occupied tile…
        assert!(!c.can_travel(0, 3200, 3200, 1, 0, 1, NPC, CollisionStrategy::Normal));
        // …but a player (extra_flag = PLAYER) walks right through it.
        assert!(c.can_travel(0, 3200, 3200, 1, 0, 1, PLAYER, CollisionStrategy::Normal));
        // Clearing the footprint frees the tile again.
        c.change_npc(3201, 3200, 0, 1, false);
        assert!(c.can_travel(0, 3200, 3200, 1, 0, 1, NPC, CollisionStrategy::Normal));
    }

    #[test]
    fn unloaded_is_permissive() {
        let c = WorldCollision::new();
        assert!(!c.is_blocked(3200, 3200, 0));
        assert!(c.line_of_walk(0, 3200, 3200, 3300, 3300));
        assert!(c.find_path(0, 3200, 3200, 3206, 3200, true).is_empty());
    }
}
