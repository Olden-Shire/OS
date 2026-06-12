// @ObfuscatedName("bs") — jag::oldscape::dash3d::Occlude.
// (The owning class `aq` is jag::oldscape::dash3d::World — that's where
// setOcclude / calcOcclude / occluded live in Java.)
//
// Axis-aligned occluder used by World.testPoint to cull primitives
// behind solid walls / roofs. Each occluder is one of:
//   type 1: vertical plane perpendicular to X (a wall facing east/west)
//   type 2: vertical plane perpendicular to Z (a wall facing north/south)
//   type 4: horizontal plane (a roof / floor)
//
// Build flow (Java's ClientBuild.finishBuild lines 982-1117):
//   1. Walls and tall scenery, while being added to the scene, OR per-tile
//      mapo bits indicating which directions they block.
//   2. After all locs are placed, finishBuild walks each level looking
//      for connected runs of mapo bits in each direction. Runs ≥ 8 tiles
//      (≥ 4 for horizontal) become an Occlude AABB via setOcclude.
//   3. Per frame, calcOcclude picks the subset whose tile range is
//      within ±25 of the camera tile and computes per-side delta
//      vectors for the testPoint scaling.
//
// Test flow (Java's occluded(x, y, z) at World.java:2215):
//   Given a world point, for each active occluder of mode 1..5:
//     compute distance along the occluder's primary axis from camera
//     to point; scale the occluder's perpendicular bounds by that
//     distance; if the point is inside the scaled rectangle, it's
//     behind the occluder → cull.

#![allow(dead_code)]

#[derive(Debug, Clone, Copy)]
pub struct Occlude {
    pub min_tile_x: i32,
    pub max_tile_x: i32,
    pub min_tile_z: i32,
    pub max_tile_z: i32,
    pub kind: i32,  // Java's .type (1, 2, or 4)
    pub min_x: i32,
    pub max_x: i32,
    pub min_z: i32,
    pub max_z: i32,
    pub min_y: i32,
    pub max_y: i32,
    pub mode: i32,
    pub min_delta_x: i32,
    pub max_delta_x: i32,
    pub min_delta_y: i32,
    pub max_delta_y: i32,
    pub min_delta_z: i32,
    pub max_delta_z: i32,
}

impl Occlude {
    pub fn new(kind: i32, min_x: i32, max_x: i32, min_z: i32, max_z: i32, min_y: i32, max_y: i32) -> Self {
        Self {
            kind,
            min_tile_x: min_x / 128,
            max_tile_x: max_x / 128,
            min_tile_z: min_z / 128,
            max_tile_z: max_z / 128,
            min_x, max_x, min_z, max_z, min_y, max_y,
            mode: 0,
            min_delta_x: 0, max_delta_x: 0,
            min_delta_y: 0, max_delta_y: 0,
            min_delta_z: 0, max_delta_z: 0,
        }
    }
}

// @ObfuscatedName("aq.aj(III)Z") — World.occluded.
//
// Returns true if `(x, y, z)` is hidden behind one of the active
// occluders. Verbatim port of the 5-mode delta-scaling test.
pub fn occluded(active: &[Occlude], x: i32, y: i32, z: i32) -> bool {
    for occ in active {
        match occ.mode {
            1 => {
                let d = occ.min_x - x;
                if d > 0 {
                    let lo_z = (occ.min_delta_z * d >> 8) + occ.min_z;
                    let hi_z = (occ.max_delta_z * d >> 8) + occ.max_z;
                    let lo_y = (occ.min_delta_y * d >> 8) + occ.min_y;
                    let hi_y = (occ.max_delta_y * d >> 8) + occ.max_y;
                    if z >= lo_z && z <= hi_z && y >= lo_y && y <= hi_y {
                        return true;
                    }
                }
            }
            2 => {
                let d = x - occ.min_x;
                if d > 0 {
                    let lo_z = (occ.min_delta_z * d >> 8) + occ.min_z;
                    let hi_z = (occ.max_delta_z * d >> 8) + occ.max_z;
                    let lo_y = (occ.min_delta_y * d >> 8) + occ.min_y;
                    let hi_y = (occ.max_delta_y * d >> 8) + occ.max_y;
                    if z >= lo_z && z <= hi_z && y >= lo_y && y <= hi_y {
                        return true;
                    }
                }
            }
            3 => {
                let d = occ.min_z - z;
                if d > 0 {
                    let lo_x = (occ.min_delta_x * d >> 8) + occ.min_x;
                    let hi_x = (occ.max_delta_x * d >> 8) + occ.max_x;
                    let lo_y = (occ.min_delta_y * d >> 8) + occ.min_y;
                    let hi_y = (occ.max_delta_y * d >> 8) + occ.max_y;
                    if x >= lo_x && x <= hi_x && y >= lo_y && y <= hi_y {
                        return true;
                    }
                }
            }
            4 => {
                let d = z - occ.min_z;
                if d > 0 {
                    let lo_x = (occ.min_delta_x * d >> 8) + occ.min_x;
                    let hi_x = (occ.max_delta_x * d >> 8) + occ.max_x;
                    let lo_y = (occ.min_delta_y * d >> 8) + occ.min_y;
                    let hi_y = (occ.max_delta_y * d >> 8) + occ.max_y;
                    if x >= lo_x && x <= hi_x && y >= lo_y && y <= hi_y {
                        return true;
                    }
                }
            }
            5 => {
                let d = y - occ.min_y;
                if d > 0 {
                    let lo_x = (occ.min_delta_x * d >> 8) + occ.min_x;
                    let hi_x = (occ.max_delta_x * d >> 8) + occ.max_x;
                    let lo_z = (occ.min_delta_z * d >> 8) + occ.min_z;
                    let hi_z = (occ.max_delta_z * d >> 8) + occ.max_z;
                    if x >= lo_x && x <= hi_x && z >= lo_z && z <= hi_z {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

// @ObfuscatedName("aq.at()V") — World.calcOcclude.
//
// Per-frame: pick the subset of occluders within ±25 tiles of the camera
// tile, set their mode based on which side of the occluder the camera is
// on, and compute the per-side delta vectors used by `occluded`. Returns
// the active set.
//
// We skip Java's visBackingDirty check (the precomputed view-frustum
// visibility table from resetVisCalc) — that's an extra culling layer,
// not a correctness gate. Without it we test against slightly more
// occluders per frame; the cull math itself stays sound.
pub fn calc_occlude(
    occluders_at_camera_level: &[Occlude],
    cam_tile_x: i32, cam_tile_z: i32,
    cam_x: i32, cam_y: i32, cam_z: i32,
) -> Vec<Occlude> {
    let mut active = Vec::new();
    for occ in occluders_at_camera_level {
        let mut occ = *occ;
        if occ.kind == 1 {
            // type 1: occluder is a vertical plane perpendicular to X.
            let _vx = occ.min_tile_x - cam_tile_x + 25;
            if _vx < 0 || _vx > 50 { continue; }
            let v9 = cam_x - occ.min_x;
            if v9 > 32 {
                occ.mode = 1;
            } else if v9 < -32 {
                occ.mode = 2;
                let v9 = -v9;
                let denom = v9.max(1);
                occ.min_delta_z = ((occ.min_z - cam_z) << 8) / denom;
                occ.max_delta_z = ((occ.max_z - cam_z) << 8) / denom;
                occ.min_delta_y = ((occ.min_y - cam_y) << 8) / denom;
                occ.max_delta_y = ((occ.max_y - cam_y) << 8) / denom;
                active.push(occ);
                continue;
            } else {
                continue;
            }
            let denom = v9.max(1);
            occ.min_delta_z = ((occ.min_z - cam_z) << 8) / denom;
            occ.max_delta_z = ((occ.max_z - cam_z) << 8) / denom;
            occ.min_delta_y = ((occ.min_y - cam_y) << 8) / denom;
            occ.max_delta_y = ((occ.max_y - cam_y) << 8) / denom;
            active.push(occ);
        } else if occ.kind == 2 {
            let _vz = occ.min_tile_z - cam_tile_z + 25;
            if _vz < 0 || _vz > 50 { continue; }
            let v14 = cam_z - occ.min_z;
            if v14 > 32 {
                occ.mode = 3;
            } else if v14 < -32 {
                occ.mode = 4;
                let v14 = -v14;
                let denom = v14.max(1);
                occ.min_delta_x = ((occ.min_x - cam_x) << 8) / denom;
                occ.max_delta_x = ((occ.max_x - cam_x) << 8) / denom;
                occ.min_delta_y = ((occ.min_y - cam_y) << 8) / denom;
                occ.max_delta_y = ((occ.max_y - cam_y) << 8) / denom;
                active.push(occ);
                continue;
            } else {
                continue;
            }
            let denom = v14.max(1);
            occ.min_delta_x = ((occ.min_x - cam_x) << 8) / denom;
            occ.max_delta_x = ((occ.max_x - cam_x) << 8) / denom;
            occ.min_delta_y = ((occ.min_y - cam_y) << 8) / denom;
            occ.max_delta_y = ((occ.max_y - cam_y) << 8) / denom;
            active.push(occ);
        } else if occ.kind == 4 {
            let v15 = occ.min_y - cam_y;
            if v15 > 128 {
                occ.mode = 5;
                let denom = v15.max(1);
                occ.min_delta_x = ((occ.min_x - cam_x) << 8) / denom;
                occ.max_delta_x = ((occ.max_x - cam_x) << 8) / denom;
                occ.min_delta_z = ((occ.min_z - cam_z) << 8) / denom;
                occ.max_delta_z = ((occ.max_z - cam_z) << 8) / denom;
                active.push(occ);
            }
        }
    }
    active
}

// @ObfuscatedName("aq.r(IIIIIIII)V") — World.setOcclude.
//
// Adds an occluder to the per-level list. The tile-range fields are
// computed from world coords / 128 (Java's tileSize).
pub fn set_occlude(out: &mut Vec<Occlude>, kind: i32, min_x: i32, max_x: i32, min_z: i32, max_z: i32, min_y: i32, max_y: i32) {
    out.push(Occlude::new(kind, min_x, max_x, min_z, max_z, min_y, max_y));
}

// Mapo run-length walker — Java's ClientBuild.finishBuild lines 982-1117.
// Bits in mapo: at output level L, the bit positions used are
//   1 << (3*L), 2 << (3*L), 4 << (3*L) for the X, Z, Y occluder types.
// At each output level L, we walk levels 0..=L looking for runs.
//
// `groundh[level][x][z]` is the same 4D height grid client_build holds.
pub fn build_occluders(
    mapo: &mut [Vec<Vec<u16>>],
    groundh: &[Vec<Vec<i32>>],
) -> Vec<Vec<Occlude>> {
    let mut out: Vec<Vec<Occlude>> = (0..4).map(|_| Vec::new()).collect();
    let mut bit_x: i32 = 1;
    let mut bit_z: i32 = 2;
    let mut bit_y: i32 = 4;
    for level_out in 0..4i32 {
        if level_out > 0 {
            bit_x <<= 3;
            bit_z <<= 3;
            bit_y <<= 3;
        }
        for level_in in 0..=level_out as usize {
            for x in 0..=104i32 {
                for z in 0..=104i32 {
                    // Inline reader — a closure capturing `&mapo` would
                    // collide with the `&mut mapo[...]` writes inside
                    // each match arm. Macro takes `&mapo` per-call so
                    // the borrow scope is tight.
                    macro_rules! read {
                        ($lvl:expr, $mx:expr, $mz:expr) => {{
                            let lvl_: usize = $lvl;
                            let mx_: i32 = $mx;
                            let mz_: i32 = $mz;
                            if mx_ < 0 || mx_ > 104 || mz_ < 0 || mz_ > 104 { 0 }
                            else { mapo[lvl_][mx_ as usize][mz_ as usize] as i32 }
                        }};
                    }
                    // Type 1 (X-aligned wall) run along Z, then extend
                    // upward in level.
                    if (read!(level_in, x, z) & bit_x) != 0 {
                        let mut z_lo = z;
                        let mut z_hi = z;
                        let mut l_lo = level_in as i32;
                        let mut l_hi = level_in as i32;
                        while z_lo > 0 && (read!(level_in, x, z_lo - 1) & bit_x) != 0 { z_lo -= 1; }
                        while z_hi < 104 && (read!(level_in, x, z_hi + 1) & bit_x) != 0 { z_hi += 1; }
                        'outer1a: while l_lo > 0 {
                            for cz in z_lo..=z_hi {
                                if (read!((l_lo - 1) as usize, x, cz) & bit_x) == 0 { break 'outer1a; }
                            }
                            l_lo -= 1;
                        }
                        'outer1b: while l_hi < level_out {
                            for cz in z_lo..=z_hi {
                                if (read!((l_hi + 1) as usize, x, cz) & bit_x) == 0 { break 'outer1b; }
                            }
                            l_hi += 1;
                        }
                        let area = (l_hi + 1 - l_lo) * (z_hi - z_lo + 1);
                        if area >= 8 {
                            let y_min = groundh[l_hi as usize].get(x as usize).and_then(|c| c.get(z_lo as usize)).copied().unwrap_or(0) - 240;
                            let y_max = groundh[l_lo as usize].get(x as usize).and_then(|c| c.get(z_lo as usize)).copied().unwrap_or(0);
                            set_occlude(&mut out[level_out as usize], 1,
                                x * 128, x * 128,
                                z_lo * 128, z_hi * 128 + 128,
                                y_min, y_max);
                            for l in l_lo..=l_hi {
                                for cz in z_lo..=z_hi {
                                    let lu = l as usize;
                                    if x >= 0 && (x as usize) < mapo[lu].len() && cz >= 0 && (cz as usize) < mapo[lu][x as usize].len() {
                                        mapo[lu][x as usize][cz as usize] &= !(bit_x as u16);
                                    }
                                }
                            }
                        }
                    }
                    // Type 2 (Z-aligned wall) run along X.
                    if (read!(level_in, x, z) & bit_z) != 0 {
                        let mut x_lo = x;
                        let mut x_hi = x;
                        let mut l_lo = level_in as i32;
                        let mut l_hi = level_in as i32;
                        while x_lo > 0 && (read!(level_in, x_lo - 1, z) & bit_z) != 0 { x_lo -= 1; }
                        while x_hi < 104 && (read!(level_in, x_hi + 1, z) & bit_z) != 0 { x_hi += 1; }
                        'outer2a: while l_lo > 0 {
                            for cx in x_lo..=x_hi {
                                if (read!((l_lo - 1) as usize, cx, z) & bit_z) == 0 { break 'outer2a; }
                            }
                            l_lo -= 1;
                        }
                        'outer2b: while l_hi < level_out {
                            for cx in x_lo..=x_hi {
                                if (read!((l_hi + 1) as usize, cx, z) & bit_z) == 0 { break 'outer2b; }
                            }
                            l_hi += 1;
                        }
                        let area = (l_hi + 1 - l_lo) * (x_hi - x_lo + 1);
                        if area >= 8 {
                            let y_min = groundh[l_hi as usize].get(x_lo as usize).and_then(|c| c.get(z as usize)).copied().unwrap_or(0) - 240;
                            let y_max = groundh[l_lo as usize].get(x_lo as usize).and_then(|c| c.get(z as usize)).copied().unwrap_or(0);
                            set_occlude(&mut out[level_out as usize], 2,
                                x_lo * 128, x_hi * 128 + 128,
                                z * 128, z * 128,
                                y_min, y_max);
                            for l in l_lo..=l_hi {
                                for cx in x_lo..=x_hi {
                                    let lu = l as usize;
                                    if cx >= 0 && (cx as usize) < mapo[lu].len() && z >= 0 && (z as usize) < mapo[lu][cx as usize].len() {
                                        mapo[lu][cx as usize][z as usize] &= !(bit_z as u16);
                                    }
                                }
                            }
                        }
                    }
                    // Type 4 (Y-aligned floor/roof) run in 2D.
                    if (read!(level_in, x, z) & bit_y) != 0 {
                        let mut x_lo = x;
                        let mut x_hi = x;
                        let mut z_lo = z;
                        let mut z_hi = z;
                        while z_lo > 0 && (read!(level_in, x, z_lo - 1) & bit_y) != 0 { z_lo -= 1; }
                        while z_hi < 104 && (read!(level_in, x, z_hi + 1) & bit_y) != 0 { z_hi += 1; }
                        'outer4a: while x_lo > 0 {
                            for cz in z_lo..=z_hi {
                                if (read!(level_in, x_lo - 1, cz) & bit_y) == 0 { break 'outer4a; }
                            }
                            x_lo -= 1;
                        }
                        'outer4b: while x_hi < 104 {
                            for cz in z_lo..=z_hi {
                                if (read!(level_in, x_hi + 1, cz) & bit_y) == 0 { break 'outer4b; }
                            }
                            x_hi += 1;
                        }
                        if (x_hi - x_lo + 1) * (z_hi - z_lo + 1) >= 4 {
                            let y = groundh[level_in].get(x_lo as usize).and_then(|c| c.get(z_lo as usize)).copied().unwrap_or(0);
                            set_occlude(&mut out[level_out as usize], 4,
                                x_lo * 128, x_hi * 128 + 128,
                                z_lo * 128, z_hi * 128 + 128,
                                y, y);
                            for cx in x_lo..=x_hi {
                                for cz in z_lo..=z_hi {
                                    if cx >= 0 && (cx as usize) < mapo[level_in].len() && cz >= 0 && (cz as usize) < mapo[level_in][cx as usize].len() {
                                        mapo[level_in][cx as usize][cz as usize] &= !(bit_y as u16);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    out
}
