// @ObfuscatedName("ck") — jag::oldscape::movement::CollisionMap.
//
// Per-tile walk-flag grid. ClientBuild.addLoc / addWall / addScenery
// register their footprint here so pathing can later test "can the
// player move from (x,z) to (nx,nz)" via the flag bitset.
//
// Verbatim port of CollisionMap.java. The bit constants are deeply
// load-bearing — every flag tested by Pathfinder appears below and the
// numerals match the Java source 1:1.

#![allow(dead_code)]

#[derive(Debug, Clone)]
pub struct CollisionMap {
    // @ObfuscatedName("ck.am")
    pub start_x: i32,
    // @ObfuscatedName("ck.ap")
    pub start_z: i32,
    // @ObfuscatedName("ck.av")
    pub size_x: i32,
    // @ObfuscatedName("ck.ak")
    pub size_z: i32,
    // @ObfuscatedName("ck.az")
    pub flags: Vec<Vec<i32>>,
}

impl CollisionMap {
    pub fn new(size_x: i32, size_z: i32) -> Self {
        let mut map = Self {
            start_x: 0,
            start_z: 0,
            size_x,
            size_z,
            flags: vec![vec![0i32; size_z as usize]; size_x as usize],
        };
        map.reset();
        map
    }

    // @ObfuscatedName("ck.r(I)V") — CollisionMap.reset. Verbatim port
    // of CollisionMap.java:33-43.
    pub fn reset(&mut self) {
        for x in 0..self.size_x as usize {
            for z in 0..self.size_z as usize {
                if x == 0 || z == 0
                    || x >= self.size_x as usize - 5
                    || z >= self.size_z as usize - 5
                {
                    self.flags[x][z] = 16_777_215;
                } else {
                    self.flags[x][z] = 16_777_216;
                }
            }
        }
    }

    // @ObfuscatedName("ck.n(IIII)V") — CollisionMap.addCMap.
    pub fn add_c_map(&mut self, x: i32, z: i32, bits: i32) {
        if x < 0 || x >= self.size_x || z < 0 || z >= self.size_z { return; }
        self.flags[x as usize][z as usize] |= bits;
    }

    // @ObfuscatedName("ck.g(IIII)V") — CollisionMap.remCMap.
    pub fn rem_c_map(&mut self, x: i32, z: i32, bits: i32) {
        if x < 0 || x >= self.size_x || z < 0 || z >= self.size_z { return; }
        self.flags[x as usize][z as usize] &= !bits;
    }

    // @ObfuscatedName("ck.d(IIIIZI)V") — CollisionMap.addWall.
    // Verbatim port of CollisionMap.java:47-168. Handles kind 0
    // (straight), 1/3 (corners), and 2 (T-junction), plus the
    // blockrange variant that adds the ranged-attack mask in the high
    // bits (0x100..0x10000 block, 0x10000+ blockrange).
    pub fn add_wall(&mut self, world_x: i32, world_z: i32, kind: i32, rotation: i32, blockrange: bool) {
        let x = world_x - self.start_x;
        let z = world_z - self.start_z;
        if kind == 0 {
            match rotation {
                0 => { self.add_c_map(x, z, 128); self.add_c_map(x - 1, z, 8); }
                1 => { self.add_c_map(x, z, 2);   self.add_c_map(x, z + 1, 32); }
                2 => { self.add_c_map(x, z, 8);   self.add_c_map(x + 1, z, 128); }
                3 => { self.add_c_map(x, z, 32);  self.add_c_map(x, z - 1, 2); }
                _ => {}
            }
        }
        if kind == 1 || kind == 3 {
            match rotation {
                0 => { self.add_c_map(x, z, 1);  self.add_c_map(x - 1, z + 1, 16); }
                1 => { self.add_c_map(x, z, 4);  self.add_c_map(x + 1, z + 1, 64); }
                2 => { self.add_c_map(x, z, 16); self.add_c_map(x + 1, z - 1, 1); }
                3 => { self.add_c_map(x, z, 64); self.add_c_map(x - 1, z - 1, 4); }
                _ => {}
            }
        }
        if kind == 2 {
            match rotation {
                0 => { self.add_c_map(x, z, 130); self.add_c_map(x - 1, z, 8);   self.add_c_map(x, z + 1, 32); }
                1 => { self.add_c_map(x, z, 10);  self.add_c_map(x, z + 1, 32);  self.add_c_map(x + 1, z, 128); }
                2 => { self.add_c_map(x, z, 40);  self.add_c_map(x + 1, z, 128); self.add_c_map(x, z - 1, 2); }
                3 => { self.add_c_map(x, z, 160); self.add_c_map(x, z - 1, 2);   self.add_c_map(x - 1, z, 8); }
                _ => {}
            }
        }
        if blockrange {
            if kind == 0 {
                match rotation {
                    0 => { self.add_c_map(x, z, 65536); self.add_c_map(x - 1, z, 4096); }
                    1 => { self.add_c_map(x, z, 1024);  self.add_c_map(x, z + 1, 16384); }
                    2 => { self.add_c_map(x, z, 4096);  self.add_c_map(x + 1, z, 65536); }
                    3 => { self.add_c_map(x, z, 16384); self.add_c_map(x, z - 1, 1024); }
                    _ => {}
                }
            }
            if kind == 1 || kind == 3 {
                match rotation {
                    0 => { self.add_c_map(x, z, 512);   self.add_c_map(x - 1, z + 1, 8192); }
                    1 => { self.add_c_map(x, z, 2048);  self.add_c_map(x + 1, z + 1, 32768); }
                    2 => { self.add_c_map(x, z, 8192);  self.add_c_map(x + 1, z - 1, 512); }
                    3 => { self.add_c_map(x, z, 32768); self.add_c_map(x - 1, z - 1, 2048); }
                    _ => {}
                }
            }
            if kind == 2 {
                match rotation {
                    0 => { self.add_c_map(x, z, 66560); self.add_c_map(x - 1, z, 4096);   self.add_c_map(x, z + 1, 16384); }
                    1 => { self.add_c_map(x, z, 5120);  self.add_c_map(x, z + 1, 16384);  self.add_c_map(x + 1, z, 65536); }
                    2 => { self.add_c_map(x, z, 20480); self.add_c_map(x + 1, z, 65536);  self.add_c_map(x, z - 1, 1024); }
                    3 => { self.add_c_map(x, z, 81920); self.add_c_map(x, z - 1, 1024);   self.add_c_map(x - 1, z, 4096); }
                    _ => {}
                }
            }
        }
    }

    // @ObfuscatedName("ck.l(IIIIZI)V") — CollisionMap.addLoc. Verbatim
    // port of CollisionMap.java:172-188. Footprint covers
    // (lw × ll) tiles starting at (world_x, world_z). When blockrange
    // is set, the ranged-attack mask 0x20000 is OR'd in alongside the
    // walk-block mask 0x100.
    pub fn add_loc(&mut self, world_x: i32, world_z: i32, lw: i32, ll: i32, blockrange: bool) {
        let mut bits = 256;
        if blockrange { bits += 131072; }
        let bx = world_x - self.start_x;
        let bz = world_z - self.start_z;
        for x in bx..(lw + bx) {
            if x < 0 || x >= self.size_x { continue; }
            for z in bz..(ll + bz) {
                if z < 0 || z >= self.size_z { continue; }
                self.add_c_map(x, z, bits);
            }
        }
    }

    // @ObfuscatedName("ck.m(III)V") — CollisionMap.blockGround.
    pub fn block_ground(&mut self, world_x: i32, world_z: i32) {
        let x = (world_x - self.start_x) as usize;
        let z = (world_z - self.start_z) as usize;
        self.flags[x][z] |= 0x200000;
    }

    // @ObfuscatedName("ck.c(IIB)V") — CollisionMap.blockGroundDecor.
    pub fn block_ground_decor(&mut self, world_x: i32, world_z: i32) {
        let x = (world_x - self.start_x) as usize;
        let z = (world_z - self.start_z) as usize;
        self.flags[x][z] |= 0x40000;
    }

    // @ObfuscatedName("ck.j(IIIIZI)V") — CollisionMap.delWall. Mirror
    // of addWall but masks the bits off. Verbatim port of
    // CollisionMap.java:214-335.
    pub fn del_wall(&mut self, world_x: i32, world_z: i32, kind: i32, rotation: i32, blockrange: bool) {
        let x = world_x - self.start_x;
        let z = world_z - self.start_z;
        if kind == 0 {
            match rotation {
                0 => { self.rem_c_map(x, z, 128); self.rem_c_map(x - 1, z, 8); }
                1 => { self.rem_c_map(x, z, 2);   self.rem_c_map(x, z + 1, 32); }
                2 => { self.rem_c_map(x, z, 8);   self.rem_c_map(x + 1, z, 128); }
                3 => { self.rem_c_map(x, z, 32);  self.rem_c_map(x, z - 1, 2); }
                _ => {}
            }
        }
        if kind == 1 || kind == 3 {
            match rotation {
                0 => { self.rem_c_map(x, z, 1);  self.rem_c_map(x - 1, z + 1, 16); }
                1 => { self.rem_c_map(x, z, 4);  self.rem_c_map(x + 1, z + 1, 64); }
                2 => { self.rem_c_map(x, z, 16); self.rem_c_map(x + 1, z - 1, 1); }
                3 => { self.rem_c_map(x, z, 64); self.rem_c_map(x - 1, z - 1, 4); }
                _ => {}
            }
        }
        if kind == 2 {
            match rotation {
                0 => { self.rem_c_map(x, z, 130); self.rem_c_map(x - 1, z, 8);   self.rem_c_map(x, z + 1, 32); }
                1 => { self.rem_c_map(x, z, 10);  self.rem_c_map(x, z + 1, 32);  self.rem_c_map(x + 1, z, 128); }
                2 => { self.rem_c_map(x, z, 40);  self.rem_c_map(x + 1, z, 128); self.rem_c_map(x, z - 1, 2); }
                3 => { self.rem_c_map(x, z, 160); self.rem_c_map(x, z - 1, 2);   self.rem_c_map(x - 1, z, 8); }
                _ => {}
            }
        }
        if blockrange {
            if kind == 0 {
                match rotation {
                    0 => { self.rem_c_map(x, z, 65536); self.rem_c_map(x - 1, z, 4096); }
                    1 => { self.rem_c_map(x, z, 1024);  self.rem_c_map(x, z + 1, 16384); }
                    2 => { self.rem_c_map(x, z, 4096);  self.rem_c_map(x + 1, z, 65536); }
                    3 => { self.rem_c_map(x, z, 16384); self.rem_c_map(x, z - 1, 1024); }
                    _ => {}
                }
            }
            if kind == 1 || kind == 3 {
                match rotation {
                    0 => { self.rem_c_map(x, z, 512);   self.rem_c_map(x - 1, z + 1, 8192); }
                    1 => { self.rem_c_map(x, z, 2048);  self.rem_c_map(x + 1, z + 1, 32768); }
                    2 => { self.rem_c_map(x, z, 8192);  self.rem_c_map(x + 1, z - 1, 512); }
                    3 => { self.rem_c_map(x, z, 32768); self.rem_c_map(x - 1, z - 1, 2048); }
                    _ => {}
                }
            }
            if kind == 2 {
                match rotation {
                    0 => { self.rem_c_map(x, z, 66560); self.rem_c_map(x - 1, z, 4096);  self.rem_c_map(x, z + 1, 16384); }
                    1 => { self.rem_c_map(x, z, 5120);  self.rem_c_map(x, z + 1, 16384); self.rem_c_map(x + 1, z, 65536); }
                    2 => { self.rem_c_map(x, z, 20480); self.rem_c_map(x + 1, z, 65536); self.rem_c_map(x, z - 1, 1024); }
                    3 => { self.rem_c_map(x, z, 81920); self.rem_c_map(x, z - 1, 1024);  self.rem_c_map(x - 1, z, 4096); }
                    _ => {}
                }
            }
        }
    }

    // @ObfuscatedName("ck.z(IIIIIZI)V") — CollisionMap.delLoc. Verbatim
    // port of CollisionMap.java:339-360. Note the rotation 1/3 swap of
    // (lw, ll) — Java mutates its arg2/arg3 in place.
    pub fn del_loc(&mut self, world_x: i32, world_z: i32, lw: i32, ll: i32, rotation: i32, blockrange: bool) {
        let mut bits = 256;
        if blockrange { bits += 131072; }
        let bx = world_x - self.start_x;
        let bz = world_z - self.start_z;
        let (lw, ll) = if rotation == 1 || rotation == 3 { (ll, lw) } else { (lw, ll) };
        for x in bx..(lw + bx) {
            if x < 0 || x >= self.size_x { continue; }
            for z in bz..(ll + bz) {
                if z < 0 || z >= self.size_z { continue; }
                self.rem_c_map(x, z, bits);
            }
        }
    }

    // @ObfuscatedName("ck.q(III)V") — CollisionMap.unblockGroundDecor.
    pub fn unblock_ground_decor(&mut self, world_x: i32, world_z: i32) {
        let x = (world_x - self.start_x) as usize;
        let z = (world_z - self.start_z) as usize;
        self.flags[x][z] &= 0xFFFBFFFFu32 as i32;
    }

    // @ObfuscatedName("ck.i(IIIIIII)Z") — CollisionMap.testWall.
    // Verbatim port of CollisionMap.java:378-499.
    pub fn test_wall(&self, x0: i32, z0: i32, x1: i32, z1: i32, kind: i32, rotation: i32) -> bool {
        if x0 == x1 && z0 == z1 { return true; }
        let v7 = (x0 - self.start_x) as usize;
        let v8 = (z0 - self.start_z) as usize;
        let v9 = (x1 - self.start_x) as i32;
        let v10 = (z1 - self.start_z) as i32;
        let f = self.flags[v7][v8];
        if kind == 0 {
            match rotation {
                0 => {
                    if v9 - 1 == v7 as i32 && v8 as i32 == v10 { return true; }
                    if v7 as i32 == v9 && v10 + 1 == v8 as i32 && (f & 0x12C0120) == 0 { return true; }
                    if v7 as i32 == v9 && v10 - 1 == v8 as i32 && (f & 0x12C0102) == 0 { return true; }
                }
                1 => {
                    if v7 as i32 == v9 && v10 + 1 == v8 as i32 { return true; }
                    if v9 - 1 == v7 as i32 && v8 as i32 == v10 && (f & 0x12C0108) == 0 { return true; }
                    if v9 + 1 == v7 as i32 && v8 as i32 == v10 && (f & 0x12C0180) == 0 { return true; }
                }
                2 => {
                    if v9 + 1 == v7 as i32 && v8 as i32 == v10 { return true; }
                    if v7 as i32 == v9 && v10 + 1 == v8 as i32 && (f & 0x12C0120) == 0 { return true; }
                    if v7 as i32 == v9 && v10 - 1 == v8 as i32 && (f & 0x12C0102) == 0 { return true; }
                }
                3 => {
                    if v7 as i32 == v9 && v10 - 1 == v8 as i32 { return true; }
                    if v9 - 1 == v7 as i32 && v8 as i32 == v10 && (f & 0x12C0108) == 0 { return true; }
                    if v9 + 1 == v7 as i32 && v8 as i32 == v10 && (f & 0x12C0180) == 0 { return true; }
                }
                _ => {}
            }
        }
        if kind == 2 {
            match rotation {
                0 => {
                    if v9 - 1 == v7 as i32 && v8 as i32 == v10 { return true; }
                    if v7 as i32 == v9 && v10 + 1 == v8 as i32 { return true; }
                    if v9 + 1 == v7 as i32 && v8 as i32 == v10 && (f & 0x12C0180) == 0 { return true; }
                    if v7 as i32 == v9 && v10 - 1 == v8 as i32 && (f & 0x12C0102) == 0 { return true; }
                }
                1 => {
                    if v9 - 1 == v7 as i32 && v8 as i32 == v10 && (f & 0x12C0108) == 0 { return true; }
                    if v7 as i32 == v9 && v10 + 1 == v8 as i32 { return true; }
                    if v9 + 1 == v7 as i32 && v8 as i32 == v10 { return true; }
                    if v7 as i32 == v9 && v10 - 1 == v8 as i32 && (f & 0x12C0102) == 0 { return true; }
                }
                2 => {
                    if v9 - 1 == v7 as i32 && v8 as i32 == v10 && (f & 0x12C0108) == 0 { return true; }
                    if v7 as i32 == v9 && v10 + 1 == v8 as i32 && (f & 0x12C0120) == 0 { return true; }
                    if v9 + 1 == v7 as i32 && v8 as i32 == v10 { return true; }
                    if v7 as i32 == v9 && v10 - 1 == v8 as i32 { return true; }
                }
                3 => {
                    if v9 - 1 == v7 as i32 && v8 as i32 == v10 { return true; }
                    if v7 as i32 == v9 && v10 + 1 == v8 as i32 && (f & 0x12C0120) == 0 { return true; }
                    if v9 + 1 == v7 as i32 && v8 as i32 == v10 && (f & 0x12C0180) == 0 { return true; }
                    if v7 as i32 == v9 && v10 - 1 == v8 as i32 { return true; }
                }
                _ => {}
            }
        }
        if kind == 9 {
            if v7 as i32 == v9 && v10 + 1 == v8 as i32 && (f & 0x20) == 0 { return true; }
            if v7 as i32 == v9 && v10 - 1 == v8 as i32 && (f & 0x2)  == 0 { return true; }
            if v9 - 1 == v7 as i32 && v8 as i32 == v10 && (f & 0x8)  == 0 { return true; }
            if v9 + 1 == v7 as i32 && v8 as i32 == v10 && (f & 0x80) == 0 { return true; }
        }
        false
    }

    // @ObfuscatedName("ck.s(IIIIIIB)Z") — CollisionMap.testWDecor.
    // Verbatim port of CollisionMap.java:503-560. Handles wall-decor
    // kinds 6/7 (which mutate rotation via `(rot + 2) & 3` for kind 7)
    // and kind 8 (free-standing decor blocking all four cardinals).
    pub fn test_w_decor(&self, x0: i32, z0: i32, x1: i32, z1: i32, kind: i32, mut rotation: i32) -> bool {
        if x0 == x1 && z0 == z1 { return true; }
        let v7 = (x0 - self.start_x) as i32;
        let v8 = (z0 - self.start_z) as i32;
        let v9 = (x1 - self.start_x) as i32;
        let v10 = (z1 - self.start_z) as i32;
        let f = self.flags[v7 as usize][v8 as usize];
        if kind == 6 || kind == 7 {
            if kind == 7 {
                rotation = (rotation + 2) & 0x3;
            }
            match rotation {
                0 => {
                    if v9 + 1 == v7 && v8 == v10 && (f & 0x80) == 0 { return true; }
                    if v7 == v9 && v10 - 1 == v8 && (f & 0x2) == 0 { return true; }
                }
                1 => {
                    if v9 - 1 == v7 && v8 == v10 && (f & 0x8) == 0 { return true; }
                    if v7 == v9 && v10 - 1 == v8 && (f & 0x2) == 0 { return true; }
                }
                2 => {
                    if v9 - 1 == v7 && v8 == v10 && (f & 0x8) == 0 { return true; }
                    if v7 == v9 && v10 + 1 == v8 && (f & 0x20) == 0 { return true; }
                }
                3 => {
                    if v9 + 1 == v7 && v8 == v10 && (f & 0x80) == 0 { return true; }
                    if v7 == v9 && v10 + 1 == v8 && (f & 0x20) == 0 { return true; }
                }
                _ => {}
            }
        }
        if kind == 8 {
            if v7 == v9 && v10 + 1 == v8 && (f & 0x20) == 0 { return true; }
            if v7 == v9 && v10 - 1 == v8 && (f & 0x2) == 0 { return true; }
            if v9 - 1 == v7 && v8 == v10 && (f & 0x8) == 0 { return true; }
            if v9 + 1 == v7 && v8 == v10 && (f & 0x80) == 0 { return true; }
        }
        false
    }

    // @ObfuscatedName("ck.u(IIIIIIII)Z") — CollisionMap.testLoc.
    // Verbatim port of CollisionMap.java:564-578. `access_bits` bit
    // 0x1 = block N approach, 0x2 = block E, 0x4 = block S, 0x8 = block W
    // (the OSRS LocType `accessFlags` packed nibble).
    pub fn test_loc(&self, x: i32, z: i32, loc_x: i32, loc_z: i32, loc_sx: i32, loc_sz: i32, access_bits: i32) -> bool {
        let east = loc_x + loc_sx - 1;
        let south = loc_z + loc_sz - 1;
        let fx = (x - self.start_x) as usize;
        let fz = (z - self.start_z) as usize;
        if x >= loc_x && x <= east && z >= loc_z && z <= south { return true; }
        let f = self.flags[fx][fz];
        if loc_x - 1 == x && z >= loc_z && z <= south && (f & 0x8) == 0 && (access_bits & 0x8) == 0 { return true; }
        if east + 1 == x && z >= loc_z && z <= south && (f & 0x80) == 0 && (access_bits & 0x2) == 0 { return true; }
        if loc_z - 1 == z && x >= loc_x && x <= east && (f & 0x2) == 0 && (access_bits & 0x4) == 0 { return true; }
        south + 1 == z && x >= loc_x && x <= east && (f & 0x20) == 0 && (access_bits & 0x1) == 0
    }
}
