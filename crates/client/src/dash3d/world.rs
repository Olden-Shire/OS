// @ObfuscatedName("aq") — jag::oldscape::dash3d::World.
//
// The 3D scene host: the squares[level][x][z] grid that ClientBuild
// populates (setGround / setWall / setDecor / addScenery / ...) and
// the renderAll → fill octant-walk renderer that draws it back-to-
// front each frame. Verbatim port of World.java.
//
// Java keeps the per-frame render state (camera, fill bookkeeping,
// occluder lists, frustum cache) in statics; only one World instance
// ever exists, so we keep them as instance fields — same lifetime,
// no global locks.
//
// Sprites: Java stores object references in Square.sprites[] and uses
// identity comparison in delSprite. We keep all Sprites in
// `sprite_pool` and store pool indices in the squares — index
// equality is the same identity relation.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::Arc;

use crate::dash3d::ground::Ground;
use crate::dash3d::model_source::{share_light_pair, ModelSource};
use crate::dash3d::occlude::Occlude;
use crate::dash3d::pix3d;
use crate::dash3d::scene_tile::{Decor, GroundDecor, GroundObject, QuickGround, Square, Wall};
use crate::dash3d::sprite::Sprite;
use crate::dash3d::texture_manager;

// @ObfuscatedName("aq.ao") — LEVELS.
pub const LEVELS: usize = 4;

// @ObfuscatedName("aq.au") — PRETAB.
pub const PRETAB: [i32; 9] = [19, 55, 38, 155, 255, 110, 137, 205, 76];

// @ObfuscatedName("aq.ax") — MIDTAB.
pub const MIDTAB: [i32; 9] = [160, 192, 80, 96, 0, 144, 80, 48, 160];

// @ObfuscatedName("aq.ai") — POSTTAB.
pub const POSTTAB: [i32; 9] = [76, 8, 137, 4, 0, 1, 38, 2, 19];

// @ObfuscatedName("aq.aj") — MIDDEP_16.
pub const MIDDEP_16: [i32; 9] = [0, 0, 2, 0, 0, 2, 1, 1, 0];

// @ObfuscatedName("aq.aw") — MIDDEP_32.
pub const MIDDEP_32: [i32; 9] = [2, 0, 0, 2, 0, 0, 0, 4, 4];

// @ObfuscatedName("aq.af") — MIDDEP_64.
pub const MIDDEP_64: [i32; 9] = [0, 4, 4, 8, 0, 0, 8, 0, 0];

// @ObfuscatedName("aq.bh") — MIDDEP_128.
pub const MIDDEP_128: [i32; 9] = [1, 1, 0, 0, 0, 8, 0, 0, 8];

// @ObfuscatedName("aq.bi") — MINIMAP_SHAPE. Per overlay shape, a 4×4
// bit grid: 0 = overlay pixel, 1 = underlay pixel.
pub const MINIMAP_SHAPE: [[i32; 16]; 13] = [
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
    [1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1],
    [1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0],
    [0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1],
    [0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
    [1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1],
    [1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0],
    [1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1],
    [1, 1, 1, 1, 1, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1],
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 1],
];

// @ObfuscatedName("aq.bs") — MINIMAP_ROTATE. Pixel-index remap per
// overlay rotation.
pub const MINIMAP_ROTATE: [[usize; 16]; 4] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [12, 8, 4, 0, 13, 9, 5, 1, 14, 10, 6, 2, 15, 11, 7, 3],
    [15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
    [3, 7, 11, 15, 2, 6, 10, 14, 1, 5, 9, 13, 0, 4, 8, 12],
];

pub struct World {
    // @ObfuscatedName("aq.r") — lowMem. Java default is true; Client
    // sets it from the detail toggle. Our port always runs full
    // detail, so the textured-ground average-RGB fallback only fires
    // if a caller flips this on.
    pub low_mem: bool,
    // @ObfuscatedName("aq.d")
    pub max_tile_level: i32,
    // @ObfuscatedName("aq.l")
    pub max_tile_x: i32,
    // @ObfuscatedName("aq.m")
    pub max_tile_z: i32,
    // @ObfuscatedName("aq.c") — heightmap [level][x+1][z+1].
    pub groundh: Vec<Vec<Vec<i32>>>,
    // @ObfuscatedName("aq.n")
    pub squares: Vec<Vec<Vec<Option<Square>>>>,
    // @ObfuscatedName("aq.j")
    pub min_level: i32,
    // @ObfuscatedName("aq.z") / "aq.g" — dynamic sprite list (entity
    // models re-added every frame). Pool indices.
    pub dynamic_sprites: Vec<usize>,
    // @ObfuscatedName("aq.q") — per-tile occlusion memo, ±cycleNo.
    pub occlusion_cycle: Vec<Vec<Vec<i32>>>,

    // custom — backing storage for Java's Sprite object references.
    sprite_pool: Vec<Option<Sprite>>,
    sprite_free: Vec<usize>,

    // ── Render state (Java statics) ──────────────────────────────────
    // @ObfuscatedName("aq.e") — fillLeft.
    pub fill_left: i32,
    // @ObfuscatedName("aq.b") — maxLevel (render gate this frame).
    pub max_level: i32,
    // @ObfuscatedName("aq.y") — cycleNo.
    pub cycle_no: i32,
    // @ObfuscatedName("aq.t") / "aq.f" / "aq.k" / "aq.o"
    pub min_x: i32,
    pub max_x: i32,
    pub min_z: i32,
    pub max_z: i32,
    // @ObfuscatedName("aq.a") / "aq.h" — camera tile.
    pub gx: i32,
    pub gz: i32,
    // @ObfuscatedName("aq.x") / "aq.p" / "aq.ad" — camera world coords.
    pub cx: i32,
    pub cy: i32,
    pub cz: i32,
    // @ObfuscatedName("aq.ac") / "aq.aa" / "aq.as" / "aq.am"
    pub camera_sin_x: i32,
    pub camera_cos_x: i32,
    pub camera_sin_y: i32,
    pub camera_cos_y: i32,
    // @ObfuscatedName("aq.av") / "aq.ak" / "aq.az" / "aq.an" — mouse
    // pick request (screen coords in our absolute viewport space).
    pub click: bool,
    pub click_lev: i32,
    pub click_x: i32,
    pub click_y: i32,
    // @ObfuscatedName("aq.ah") / "aq.ay" — picked ground tile.
    pub ground_x: i32,
    pub ground_z: i32,
    // @ObfuscatedName("aq.ag") / "aq.ar" — per-level occluders.
    pub occluders: Vec<Vec<Occlude>>,
    // @ObfuscatedName("aq.aq") / "aq.at" — active subset this frame.
    pub active_occluders: Vec<Occlude>,
    // @ObfuscatedName("aq.ae") — fillQueue of (level, x, z).
    fill_queue: VecDeque<(usize, usize, usize)>,
    // @ObfuscatedName("aq.bk") — visBacking[8][32][51][51].
    pub vis_backing: Vec<Vec<Vec<Vec<bool>>>>,
    // @ObfuscatedName("aq.bv") — visBackingDirty: Java keeps a slice
    // reference into visBacking; we store the (pitch, yaw) indices.
    vis_dirty: (usize, usize),
    // @ObfuscatedName("aq.bg" / "aq.bl" / "aq.bt" / "aq.bw" / "aq.by" /
    // "aq.bx") — frustum-precalc projection params (origin + clip).
    pub x_orig: i32,
    pub y_orig: i32,
    pub x_clip: i32,
    pub y_clip: i32,
    pub x_clip2: i32,
    pub y_clip2: i32,
}

impl World {
    // World.java:237-245 constructor.
    pub fn new(max_tile_level: i32, max_tile_x: i32, max_tile_z: i32,
               heightmap: Vec<Vec<Vec<i32>>>) -> Self {
        let mut squares = Vec::with_capacity(max_tile_level as usize);
        let mut occlusion_cycle = Vec::with_capacity(max_tile_level as usize);
        for _ in 0..max_tile_level {
            squares.push((0..max_tile_x).map(|_| {
                (0..max_tile_z).map(|_| None).collect::<Vec<Option<Square>>>()
            }).collect::<Vec<_>>());
            occlusion_cycle.push(vec![vec![0i32; max_tile_z as usize + 1]; max_tile_x as usize + 1]);
        }
        Self {
            low_mem: false,
            max_tile_level,
            max_tile_x,
            max_tile_z,
            groundh: heightmap,
            squares,
            min_level: 0,
            dynamic_sprites: Vec::new(),
            occlusion_cycle,
            sprite_pool: Vec::new(),
            sprite_free: Vec::new(),
            fill_left: 0,
            max_level: 0,
            cycle_no: 0,
            min_x: 0, max_x: 0, min_z: 0, max_z: 0,
            gx: 0, gz: 0,
            cx: 0, cy: 0, cz: 0,
            camera_sin_x: 0, camera_cos_x: 0,
            camera_sin_y: 0, camera_cos_y: 0,
            click: false, click_lev: 0, click_x: 0, click_y: 0,
            ground_x: -1, ground_z: -1,
            occluders: (0..LEVELS).map(|_| Vec::new()).collect(),
            active_occluders: Vec::new(),
            fill_queue: VecDeque::new(),
            vis_backing: Vec::new(),
            vis_dirty: (0, 0),
            x_orig: 0, y_orig: 0,
            x_clip: 0, y_clip: 0, x_clip2: 0, y_clip2: 0,
        }
    }

    fn in_bounds(&self, level: i32, x: i32, z: i32) -> bool {
        level >= 0 && level < self.max_tile_level
            && x >= 0 && x < self.max_tile_x
            && z >= 0 && z < self.max_tile_z
    }

    fn sq(&self, level: i32, x: i32, z: i32) -> Option<&Square> {
        self.squares.get(level as usize)?
            .get(x as usize)?.get(z as usize)?.as_ref()
    }

    fn sq_mut(&mut self, level: i32, x: i32, z: i32) -> Option<&mut Square> {
        self.squares.get_mut(level as usize)?
            .get_mut(x as usize)?.get_mut(z as usize)?.as_mut()
    }

    fn ensure_square(&mut self, level: i32, x: i32, z: i32) {
        let slot = &mut self.squares[level as usize][x as usize][z as usize];
        if slot.is_none() {
            *slot = Some(Square::new(level, x, z));
        }
    }

    // Java's repeated `for (lvl = arg0; lvl >= 0; lvl--) if null new`.
    fn ensure_column(&mut self, level: i32, x: i32, z: i32) {
        for l in (0..=level).rev() {
            self.ensure_square(l, x, z);
        }
    }

    // @ObfuscatedName("aq.r()V") — World.resetMap.
    pub fn reset_map(&mut self) {
        for level in 0..self.max_tile_level as usize {
            for x in 0..self.max_tile_x as usize {
                for z in 0..self.max_tile_z as usize {
                    self.squares[level][x][z] = None;
                }
            }
        }
        for level in 0..LEVELS {
            self.occluders[level].clear();
        }
        self.dynamic_sprites.clear();
        self.sprite_pool.clear();
        self.sprite_free.clear();
    }

    // @ObfuscatedName("aq.d(I)V") — World.fillBaseLevel.
    pub fn fill_base_level(&mut self, level: i32) {
        self.min_level = level;
        for x in 0..self.max_tile_x {
            for z in 0..self.max_tile_z {
                self.ensure_square(level, x, z);
            }
        }
    }

    // @ObfuscatedName("aq.l(II)V") — World.pushDown. Shifts each level's
    // square down one (bridge tiles), keeping the displaced level-0
    // square as level 0's linkedSquare.
    pub fn push_down(&mut self, x: i32, z: i32) {
        let (xu, zu) = (x as usize, z as usize);
        let tile = self.squares[0][xu][zu].take();
        for i in 0..3usize {
            let moved = self.squares[i + 1][xu][zu].take();
            self.squares[i][xu][zu] = moved;
            if let Some(sq) = self.squares[i][xu][zu].as_mut() {
                sq.level -= 1;
                // Per-tile sprite levels follow the square down — only
                // the sprite anchored at this tile (layer-2 typecode,
                // min tile == here), per Java.
                let sprite_ids: Vec<usize> = sq.sprites.clone();
                for id in sprite_ids {
                    if let Some(sp) = self.sprite_pool.get_mut(id).and_then(|s| s.as_mut()) {
                        if (sp.typecode >> 29 & 0x3) == 2 && sp.min_tile_x == x && sp.min_tile_z == z {
                            sp.level -= 1;
                        }
                    }
                }
            }
        }
        if self.squares[0][xu][zu].is_none() {
            self.squares[0][xu][zu] = Some(Square::new(0, x, z));
        }
        self.squares[0][xu][zu].as_mut().unwrap().linked_square = tile.map(Box::new);
        self.squares[3][xu][zu] = None;
    }

    // @ObfuscatedName("aq.m(IIIIIIII)V") — World.setOcclude. Java is
    // static over the shared occluder lists; instance method here.
    pub fn set_occlude(&mut self, level: i32, kind: i32,
                       min_x: i32, max_x: i32, min_z: i32, max_z: i32,
                       min_y: i32, max_y: i32) {
        let mut occ = Occlude::new(kind, min_x, max_x, min_z, max_z, min_y, max_y);
        occ.min_tile_x = min_x / 128;
        occ.max_tile_x = max_x / 128;
        occ.min_tile_z = min_z / 128;
        occ.max_tile_z = max_z / 128;
        self.occluders[level as usize].push(occ);
    }

    // @ObfuscatedName("aq.c(IIII)V") — World.setLayer.
    pub fn set_layer(&mut self, level: i32, x: i32, z: i32, draw_level: i32) {
        if let Some(sq) = self.sq_mut(level, x, z) {
            sq.draw_level = draw_level;
        }
    }

    // @ObfuscatedName("aq.n(IIIIIIIIIIIIIIIIIIII)V") — World.setGround.
    // Verbatim port of World.java:349-375. Args positional per Java:
    // (level, x, z, shape, rotation, texture,
    //  hNW, hNE, hSE, hSW,
    //  underNW, underNE, underSE, underSW,
    //  overNW, overNE, overSE, overSW,
    //  minimapUnderlayRgb, minimapOverlayRgb)
    #[allow(clippy::too_many_arguments)]
    pub fn set_ground(&mut self, level: i32, x: i32, z: i32, shape: i32, rotation: i32,
                      texture: i32,
                      h_nw: i32, h_ne: i32, h_se: i32, h_sw: i32,
                      under_nw: i32, under_ne: i32, under_se: i32, under_sw: i32,
                      over_nw: i32, over_ne: i32, over_se: i32, over_sw: i32,
                      minimap_under: i32, minimap_over: i32) {
        if shape == 0 {
            let qg = QuickGround::new(under_nw, under_ne, under_se, under_sw,
                                      -1, minimap_under, false);
            self.ensure_column(level, x, z);
            self.sq_mut(level, x, z).unwrap().quick_ground = Some(qg);
        } else if shape == 1 {
            let qg = QuickGround::new(over_nw, over_ne, over_se, over_sw,
                                      texture, minimap_over,
                                      h_nw == h_ne && h_nw == h_se && h_nw == h_sw);
            self.ensure_column(level, x, z);
            self.sq_mut(level, x, z).unwrap().quick_ground = Some(qg);
        } else {
            let ground = Ground::new(shape, rotation, texture, x, z,
                                     h_nw, h_ne, h_se, h_sw,
                                     under_nw, under_ne, under_se, under_sw,
                                     over_nw, over_ne, over_se, over_sw,
                                     minimap_under, minimap_over);
            self.ensure_column(level, x, z);
            self.sq_mut(level, x, z).unwrap().ground = Some(ground);
        }
    }

    // @ObfuscatedName("aq.j(IIIILfu;II)V") — World.setGroundDecor.
    pub fn set_ground_decor(&mut self, level: i32, x: i32, z: i32, y: i32,
                            model: Option<Arc<ModelSource>>,
                            typecode: i32, typecode2: i32) {
        let Some(model) = model else { return };
        let gd = GroundDecor {
            model: Some(model),
            x: x * 128 + 64,
            z: z * 128 + 64,
            y,
            typecode,
            typecode2,
        };
        self.ensure_square(level, x, z);
        self.sq_mut(level, x, z).unwrap().ground_decor = Some(gd);
    }

    // @ObfuscatedName("aq.z(IIIILfu;ILfu;Lfu;)V") — World.setObj.
    // Measures the tallest layer-2 sprite-loc minY on the tile so the
    // obj stack renders on TOP of e.g. a table.
    pub fn set_obj(&mut self, level: i32, x: i32, z: i32, y: i32,
                   top: Option<Arc<ModelSource>>, typecode: i32,
                   bottom: Option<Arc<ModelSource>>, middle: Option<Arc<ModelSource>>) {
        let mut height = 0;
        if let Some(sq) = self.sq(level, x, z) {
            for &id in &sq.sprites {
                if let Some(sp) = self.sprite_pool.get(id).and_then(|s| s.as_ref()) {
                    if (sp.typecode2 & 0x100) == 0x100 {
                        if let Some(m) = sp.model.as_ref() {
                            if let Some(min_y) = m.lit_bounded_min_y() {
                                if min_y > height {
                                    height = min_y;
                                }
                            }
                        }
                    }
                }
            }
        }
        let go = GroundObject {
            x: x * 128 + 64,
            z: z * 128 + 64,
            y,
            top_obj: top,
            bottom_obj: bottom,
            middle_obj: middle,
            typecode,
            height,
        };
        self.ensure_square(level, x, z);
        self.sq_mut(level, x, z).unwrap().ground_object = Some(go);
    }

    // @ObfuscatedName("aq.g(IIIILfu;Lfu;IIII)V") — World.setWall.
    #[allow(clippy::too_many_arguments)]
    pub fn set_wall(&mut self, level: i32, x: i32, z: i32, y: i32,
                    model_a: Option<Arc<ModelSource>>, model_b: Option<Arc<ModelSource>>,
                    type_a: i32, type_b: i32, typecode: i32, typecode2: i32) {
        if model_a.is_none() && model_b.is_none() {
            return;
        }
        let wall = Wall {
            typecode,
            typecode2,
            x: x * 128 + 64,
            z: z * 128 + 64,
            y,
            model_a,
            model_b,
            type_a,
            type_b,
        };
        self.ensure_column(level, x, z);
        self.sq_mut(level, x, z).unwrap().wall = Some(wall);
    }

    // @ObfuscatedName("aq.q(IIIILfu;Lfu;IIIIII)V") — World.setDecor.
    #[allow(clippy::too_many_arguments)]
    pub fn set_decor(&mut self, level: i32, x: i32, z: i32, y: i32,
                     model: Option<Arc<ModelSource>>, model2: Option<Arc<ModelSource>>,
                     wshape: i32, yof: i32, xof: i32, zof: i32,
                     typecode: i32, typecode2: i32) {
        let Some(model) = model else { return };
        let decor = Decor {
            typecode,
            typecode2,
            x: x * 128 + 64,
            z: z * 128 + 64,
            y,
            model: Some(model),
            model2,
            wshape,
            yof,
            xof,
            zof,
        };
        self.ensure_column(level, x, z);
        self.sq_mut(level, x, z).unwrap().decor = Some(decor);
    }

    // @ObfuscatedName("aq.i(IIIIIILfu;III)Z") — World.addScenery.
    #[allow(clippy::too_many_arguments)]
    pub fn add_scenery(&mut self, level: i32, x: i32, z: i32, y: i32,
                       span_x: i32, span_z: i32,
                       model: Option<Arc<ModelSource>>, yaw: i32,
                       typecode: i32, typecode2: i32) -> bool {
        let Some(model) = model else { return true };
        let world_x = x * 128 + span_x * 64;
        let world_z = z * 128 + span_z * 64;
        self.set_sprite(level, x, z, span_x, span_z, world_x, world_z, y,
                        Some(model), yaw, false, typecode, typecode2)
    }

    // @ObfuscatedName("aq.s(IIIIILfu;IIZ)Z") — World.addDynamic
    // (radius variant — entities). `extend_by_yaw` widens the span one
    // tile in the facing direction (walking entities mid-step).
    #[allow(clippy::too_many_arguments)]
    pub fn add_dynamic(&mut self, level: i32, x: i32, z: i32, y: i32,
                       radius: i32, model: Option<Arc<ModelSource>>,
                       yaw: i32, typecode: i32, extend_by_yaw: bool) -> bool {
        let Some(model) = model else { return true };
        let mut min_wx = x - radius;
        let mut min_wz = z - radius;
        let mut max_wx = x + radius;
        let mut max_wz = z + radius;
        if extend_by_yaw {
            if yaw > 640 && yaw < 1408 {
                max_wz += 128;
            }
            if yaw > 1152 && yaw < 1920 {
                max_wx += 128;
            }
            if yaw > 1664 || yaw < 384 {
                min_wz -= 128;
            }
            if yaw > 128 && yaw < 896 {
                min_wx -= 128;
            }
        }
        let min_tx = min_wx / 128;
        let min_tz = min_wz / 128;
        let max_tx = max_wx / 128;
        let max_tz = max_wz / 128;
        self.set_sprite(level, min_tx, min_tz,
                        max_tx - min_tx + 1, max_tz - min_tz + 1,
                        x, z, y, Some(model), yaw, true, typecode, 0)
    }

    // @ObfuscatedName("aq.u(IIIIILfu;IIIIII)Z") — World.addDynamic
    // (explicit tile-span variant).
    #[allow(clippy::too_many_arguments)]
    pub fn add_dynamic_span(&mut self, level: i32, x: i32, z: i32, y: i32,
                            model: Option<Arc<ModelSource>>, yaw: i32, typecode: i32,
                            min_tile_x: i32, min_tile_z: i32,
                            max_tile_x: i32, max_tile_z: i32) -> bool {
        let Some(model) = model else { return true };
        self.set_sprite(level, min_tile_x, min_tile_z,
                        max_tile_x - min_tile_x + 1, max_tile_z - min_tile_z + 1,
                        x, z, y, Some(model), yaw, true, typecode, 0)
    }

    // @ObfuscatedName("aq.v(IIIIIIIILfu;IZII)Z") — World.setSprite.
    // The core multi-tile placement: stamps the sprite into every tile
    // of its span with per-tile edge bits, capacity-capped at 5.
    #[allow(clippy::too_many_arguments)]
    pub fn set_sprite(&mut self, level: i32, tile_x: i32, tile_z: i32,
                      span_x: i32, span_z: i32,
                      world_x: i32, world_z: i32, world_y: i32,
                      model: Option<Arc<ModelSource>>, yaw: i32,
                      dynamic: bool, typecode: i32, typecode2: i32) -> bool {
        for tx in tile_x..tile_x + span_x {
            for tz in tile_z..tile_z + span_z {
                if tx < 0 || tz < 0 || tx >= self.max_tile_x || tz >= self.max_tile_z {
                    return false;
                }
                if let Some(sq) = self.sq(level, tx, tz) {
                    if sq.sprite_count() >= 5 {
                        return false;
                    }
                }
            }
        }
        let sprite = Sprite {
            typecode,
            typecode2,
            level,
            x: world_x,
            z: world_z,
            y: world_y,
            model,
            yaw,
            min_tile_x: tile_x,
            min_tile_z: tile_z,
            max_tile_x: tile_x + span_x - 1,
            max_tile_z: tile_z + span_z - 1,
            distance: 0,
            cycle: 0,
        };
        let id = match self.sprite_free.pop() {
            Some(slot) => {
                self.sprite_pool[slot] = Some(sprite);
                slot
            }
            None => {
                self.sprite_pool.push(Some(sprite));
                self.sprite_pool.len() - 1
            }
        };
        for tx in tile_x..tile_x + span_x {
            for tz in tile_z..tile_z + span_z {
                let mut span_bits = 0;
                if tx > tile_x {
                    span_bits += 1;
                }
                if tx < tile_x + span_x - 1 {
                    span_bits += 4;
                }
                if tz > tile_z {
                    span_bits += 8;
                }
                if tz < tile_z + span_z - 1 {
                    span_bits += 2;
                }
                self.ensure_column(level, tx, tz);
                let sq = self.sq_mut(level, tx, tz).unwrap();
                sq.sprites.push(id);
                sq.sprite_span.push(span_bits);
                sq.sprite_spans |= span_bits;
            }
        }
        if dynamic {
            self.dynamic_sprites.push(id);
        }
        true
    }

    // @ObfuscatedName("aq.w()V") — World.removeSprites. Clears all
    // dynamic (entity) sprites — runs every frame before re-adding.
    pub fn remove_sprites(&mut self) {
        let ids = std::mem::take(&mut self.dynamic_sprites);
        for id in ids {
            self.del_sprite(id);
        }
    }

    // @ObfuscatedName("aq.e(Lau;)V") — World.delSprite.
    pub fn del_sprite(&mut self, id: usize) {
        let Some(sp) = self.sprite_pool.get(id).and_then(|s| s.as_ref()) else { return };
        let (level, min_tx, max_tx, min_tz, max_tz) =
            (sp.level, sp.min_tile_x, sp.max_tile_x, sp.min_tile_z, sp.max_tile_z);
        for tx in min_tx..=max_tx {
            for tz in min_tz..=max_tz {
                if let Some(sq) = self.sq_mut(level, tx, tz) {
                    if let Some(pos) = sq.sprites.iter().position(|&s| s == id) {
                        sq.sprites.remove(pos);
                        sq.sprite_span.remove(pos);
                    }
                    sq.sprite_spans = 0;
                    for &bits in &sq.sprite_span {
                        sq.sprite_spans |= bits;
                    }
                }
            }
        }
        self.sprite_pool[id] = None;
        self.sprite_free.push(id);
    }

    // @ObfuscatedName("aq.b(IIII)V") — World.moveDecor. Scales the
    // decor offset by `amount / 16` (door-open nudges).
    pub fn move_decor(&mut self, level: i32, x: i32, z: i32, amount: i32) {
        if let Some(sq) = self.sq_mut(level, x, z) {
            if let Some(d) = sq.decor.as_mut() {
                d.xof = d.xof * amount / 16;
                d.zof = d.zof * amount / 16;
            }
        }
    }

    // @ObfuscatedName("aq.y(III)V") — World.delWall.
    pub fn del_wall(&mut self, level: i32, x: i32, z: i32) {
        if let Some(sq) = self.sq_mut(level, x, z) {
            sq.wall = None;
        }
    }

    // @ObfuscatedName("aq.t(III)V") — World.delDecor.
    pub fn del_decor(&mut self, level: i32, x: i32, z: i32) {
        if let Some(sq) = self.sq_mut(level, x, z) {
            sq.decor = None;
        }
    }

    // @ObfuscatedName("aq.f(III)V") — World.delLoc. Removes the
    // sprite-style loc anchored at this tile.
    pub fn del_loc(&mut self, level: i32, x: i32, z: i32) {
        let Some(sq) = self.sq(level, x, z) else { return };
        let mut found: Option<usize> = None;
        for &id in &sq.sprites {
            if let Some(sp) = self.sprite_pool.get(id).and_then(|s| s.as_ref()) {
                if (sp.typecode >> 29 & 0x3) == 2 && sp.min_tile_x == x && sp.min_tile_z == z {
                    found = Some(id);
                    break;
                }
            }
        }
        if let Some(id) = found {
            self.del_sprite(id);
        }
    }

    // @ObfuscatedName("aq.k(III)V") — World.delGroundDecor.
    pub fn del_ground_decor(&mut self, level: i32, x: i32, z: i32) {
        if let Some(sq) = self.sq_mut(level, x, z) {
            sq.ground_decor = None;
        }
    }

    // @ObfuscatedName("aq.o(III)V") — World.delObj.
    pub fn del_obj(&mut self, level: i32, x: i32, z: i32) {
        if let Some(sq) = self.sq_mut(level, x, z) {
            sq.ground_object = None;
        }
    }

    // ── Per-tile accessors (World.java:690-784) ──────────────────────

    // @ObfuscatedName("aq.a(III)Lat;") — World.getWall.
    pub fn get_wall(&self, level: i32, x: i32, z: i32) -> Option<&Wall> {
        self.sq(level, x, z)?.wall.as_ref()
    }

    // @ObfuscatedName("aq.h(III)Lbh;") — World.getDecor.
    pub fn get_decor(&self, level: i32, x: i32, z: i32) -> Option<&Decor> {
        self.sq(level, x, z)?.decor.as_ref()
    }

    // @ObfuscatedName("aq.x(III)Lau;") — World.getScene.
    pub fn get_scene(&self, level: i32, x: i32, z: i32) -> Option<&Sprite> {
        let sq = self.sq(level, x, z)?;
        for &id in &sq.sprites {
            let sp = self.sprite_pool.get(id)?.as_ref()?;
            if (sp.typecode >> 29 & 0x3) == 2 && sp.min_tile_x == x && sp.min_tile_z == z {
                return Some(sp);
            }
        }
        None
    }

    // @ObfuscatedName("aq.p(III)Laf;") — World.getGd.
    pub fn get_gd(&self, level: i32, x: i32, z: i32) -> Option<&GroundDecor> {
        self.sq(level, x, z)?.ground_decor.as_ref()
    }

    pub fn get_ground_object(&self, level: i32, x: i32, z: i32) -> Option<&GroundObject> {
        self.sq(level, x, z)?.ground_object.as_ref()
    }

    // @ObfuscatedName("aq.ad(III)I") — World.wallType.
    pub fn wall_type(&self, level: i32, x: i32, z: i32) -> i32 {
        self.get_wall(level, x, z).map_or(0, |w| w.typecode)
    }

    // @ObfuscatedName("aq.ac(III)I") — World.decorType.
    pub fn decor_type(&self, level: i32, x: i32, z: i32) -> i32 {
        self.get_decor(level, x, z).map_or(0, |d| d.typecode)
    }

    // @ObfuscatedName("aq.aa(III)I") — World.sceneType.
    pub fn scene_type(&self, level: i32, x: i32, z: i32) -> i32 {
        self.get_scene(level, x, z).map_or(0, |s| s.typecode)
    }

    // @ObfuscatedName("aq.as(III)I") — World.gdType.
    pub fn gd_type(&self, level: i32, x: i32, z: i32) -> i32 {
        self.get_gd(level, x, z).map_or(0, |g| g.typecode)
    }

    // @ObfuscatedName("aq.am(IIII)I") — World.typecode2.
    pub fn typecode2(&self, level: i32, x: i32, z: i32, typecode: i32) -> i32 {
        let Some(sq) = self.sq(level, x, z) else { return -1 };
        if let Some(w) = sq.wall.as_ref() {
            if w.typecode == typecode {
                return w.typecode2 & 0xFF;
            }
        }
        if let Some(d) = sq.decor.as_ref() {
            if d.typecode == typecode {
                return d.typecode2 & 0xFF;
            }
        }
        if let Some(g) = sq.ground_decor.as_ref() {
            if g.typecode == typecode {
                return g.typecode2 & 0xFF;
            }
        }
        for &id in &sq.sprites {
            if let Some(sp) = self.sprite_pool.get(id).and_then(|s| s.as_ref()) {
                if sp.typecode == typecode {
                    return sp.typecode2 & 0xFF;
                }
            }
        }
        -1
    }

    // ── shareLight (World.java:786-910) ──────────────────────────────

    // @ObfuscatedName("aq.ap(III)V") — World.shareLight. Walks every
    // tile, pairs each still-unlit model with its unlit neighbours
    // (summing vertex normals across the seam), then lights it in
    // place. Runs once at the end of ClientBuild.finishBuild.
    pub fn share_light(&mut self, light_x: i32, light_y: i32, light_z: i32) {
        for level in 0..self.max_tile_level {
            for x in 0..self.max_tile_x {
                for z in 0..self.max_tile_z {
                    // Wall pair.
                    let wall_models = self.sq(level, x, z).and_then(|sq| {
                        sq.wall.as_ref().map(|w| (w.model_a.clone(), w.model_b.clone()))
                    });
                    if let Some((model_a, model_b)) = wall_models {
                        if let Some(a) = model_a.filter(|m| m.is_unlit()) {
                            self.share_light_loc(&a, level, x, z, 1, 1);
                            if let Some(b) = model_b.filter(|m| m.is_unlit()) {
                                self.share_light_loc(&b, level, x, z, 1, 1);
                                share_light_pair(&a, &b, 0, 0, 0, false);
                                b.light_in_place(light_x, light_y, light_z);
                            }
                            a.light_in_place(light_x, light_y, light_z);
                        }
                    }
                    // Sprites.
                    let sprite_ids: Vec<usize> = self.sq(level, x, z)
                        .map(|sq| sq.sprites.clone())
                        .unwrap_or_default();
                    for id in sprite_ids {
                        let info = self.sprite_pool.get(id).and_then(|s| s.as_ref()).map(|sp| {
                            (sp.model.clone(), sp.max_tile_x - sp.min_tile_x + 1,
                             sp.max_tile_z - sp.min_tile_z + 1)
                        });
                        if let Some((Some(m), span_x, span_z)) = info {
                            if m.is_unlit() {
                                self.share_light_loc(&m, level, x, z, span_x, span_z);
                                m.light_in_place(light_x, light_y, light_z);
                            }
                        }
                    }
                    // Ground decor.
                    let gd_model = self.sq(level, x, z)
                        .and_then(|sq| sq.ground_decor.as_ref())
                        .and_then(|gd| gd.model.clone());
                    if let Some(m) = gd_model.filter(|m| m.is_unlit()) {
                        self.share_light_gd(&m, level, x, z);
                        m.light_in_place(light_x, light_y, light_z);
                    }
                }
            }
        }
    }

    // @ObfuscatedName("aq.av(Lfw;III)V") — World.shareLightGd. Pairs a
    // ground decor with the decor on the +X / +Z / +X+Z / +X-Z tiles.
    // (Java's `arg3 < this.maxTileX` on the Z test is a Jagex bug we
    // keep for fidelity — maxTileX == maxTileZ in practice.)
    pub fn share_light_gd(&self, model: &Arc<ModelSource>, level: i32, x: i32, z: i32) {
        let pair_with = |tx: i32, tz: i32, off_x: i32, off_z: i32| {
            if let Some(other) = self.sq(level, tx, tz)
                .and_then(|sq| sq.ground_decor.as_ref())
                .and_then(|gd| gd.model.clone())
            {
                if other.is_unlit() {
                    share_light_pair(model, &other, off_x, 0, off_z, true);
                }
            }
        };
        if x < self.max_tile_x {
            pair_with(x + 1, z, 128, 0);
        }
        if z < self.max_tile_x {
            pair_with(x, z + 1, 0, 128);
        }
        if x < self.max_tile_x && z < self.max_tile_z {
            pair_with(x + 1, z + 1, 128, 128);
        }
        if x < self.max_tile_x && z > 0 {
            pair_with(x + 1, z - 1, 128, -128);
        }
    }

    // @ObfuscatedName("aq.ak(Lfw;IIIII)V") — World.shareLightLoc.
    // Pairs `model` (anchored at (level, x, z), spanning span_x × span_z
    // tiles) with the unlit walls + sprites in the surrounding window,
    // on this level and one above (bridges).
    pub fn share_light_loc(&self, model: &Arc<ModelSource>,
                           level: i32, x: i32, z: i32, span_x: i32, span_z: i32) {
        let mut first_level = true;
        let mut min_tx = x;
        let max_tx = x + span_x;
        let min_tz = z - 1;
        let max_tz = z + span_z;
        let gh = &self.groundh;
        let base_h = (gh[level as usize][x as usize + 1][z as usize]
            + gh[level as usize][x as usize][z as usize]
            + gh[level as usize][x as usize][z as usize + 1]
            + gh[level as usize][x as usize + 1][z as usize + 1]) / 4;
        for lvl in level..=level + 1 {
            if lvl == self.max_tile_level {
                continue;
            }
            for tx in min_tx..=max_tx {
                if tx < 0 || tx >= self.max_tile_x {
                    continue;
                }
                for tz in min_tz..=max_tz {
                    if tz < 0 || tz >= self.max_tile_z {
                        continue;
                    }
                    // Java's same-level inner-window skip.
                    if first_level && tx < max_tx && tz < max_tz && (tz >= z || x == tx) {
                        continue;
                    }
                    let Some(sq) = self.sq(lvl, tx, tz) else { continue };
                    let l = lvl as usize;
                    let y_off = (gh[l][tx as usize + 1][tz as usize]
                        + gh[l][tx as usize][tz as usize]
                        + gh[l][tx as usize][tz as usize + 1]
                        + gh[l][tx as usize + 1][tz as usize + 1]) / 4
                        - base_h;
                    if let Some(w) = sq.wall.as_ref() {
                        let off_x = (tx - x) * 128 + (1 - span_x) * 64;
                        let off_z = (tz - z) * 128 + (1 - span_z) * 64;
                        if let Some(a) = w.model_a.as_ref().filter(|m| m.is_unlit()) {
                            share_light_pair(model, a, off_x, y_off, off_z, first_level);
                        }
                        if let Some(b) = w.model_b.as_ref().filter(|m| m.is_unlit()) {
                            share_light_pair(model, b, off_x, y_off, off_z, first_level);
                        }
                    }
                    for &id in &sq.sprites {
                        let Some(sp) = self.sprite_pool.get(id).and_then(|s| s.as_ref()) else { continue };
                        let Some(m) = sp.model.as_ref().filter(|m| m.is_unlit()) else { continue };
                        let o_span_x = sp.max_tile_x - sp.min_tile_x + 1;
                        let o_span_z = sp.max_tile_z - sp.min_tile_z + 1;
                        let off_x = (sp.min_tile_x - x) * 128 + (o_span_x - span_x) * 64;
                        let off_z = (sp.min_tile_z - z) * 128 + (o_span_z - span_z) * 64;
                        share_light_pair(model, m, off_x, y_off, off_z, first_level);
                    }
                }
            }
            min_tx -= 1;
            first_level = false;
        }
    }

    // ── Minimap raster (World.java:912-968) ──────────────────────────

    // @ObfuscatedName("aq.az([IIIIII)V") — World.render2DGround.
    // Rasterizes one tile into a 4×4 pixel block of the minimap image.
    pub fn render_2d_ground(&self, pixels: &mut [i32], mut offset: usize, stride: usize,
                            level: i32, x: i32, z: i32) {
        let Some(sq) = self.sq(level, x, z) else { return };
        if let Some(qg) = sq.quick_ground.as_ref() {
            let rgb = qg.minimap_rgb;
            if rgb != 0 {
                for _ in 0..4 {
                    pixels[offset] = rgb;
                    pixels[offset + 1] = rgb;
                    pixels[offset + 2] = rgb;
                    pixels[offset + 3] = rgb;
                    offset += stride;
                }
            }
            return;
        }
        if let Some(g) = sq.ground.as_ref() {
            let shape = g.overlay_shape as usize;
            let rotation = g.overlay_rotation as usize;
            let overlay = g.minimap_overlay;
            let underlay = g.minimap_underlay;
            let shape_row = &MINIMAP_SHAPE[shape.min(MINIMAP_SHAPE.len() - 1)];
            let rot_row = &MINIMAP_ROTATE[rotation & 0x3];
            let mut i = 0;
            if overlay != 0 {
                for _ in 0..4 {
                    pixels[offset] = if shape_row[rot_row[i]] == 0 { overlay } else { underlay };
                    pixels[offset + 1] = if shape_row[rot_row[i + 1]] == 0 { overlay } else { underlay };
                    pixels[offset + 2] = if shape_row[rot_row[i + 2]] == 0 { overlay } else { underlay };
                    pixels[offset + 3] = if shape_row[rot_row[i + 3]] == 0 { overlay } else { underlay };
                    i += 4;
                    offset += stride;
                }
                return;
            }
            for _ in 0..4 {
                if shape_row[rot_row[i]] != 0 {
                    pixels[offset] = underlay;
                }
                if shape_row[rot_row[i + 1]] != 0 {
                    pixels[offset + 1] = underlay;
                }
                if shape_row[rot_row[i + 2]] != 0 {
                    pixels[offset + 2] = underlay;
                }
                if shape_row[rot_row[i + 3]] != 0 {
                    pixels[offset + 3] = underlay;
                }
                i += 4;
                offset += stride;
            }
        }
    }

    // ── Frustum pre-calc (World.java:970-1050) ───────────────────────

    // @ObfuscatedName("aq.an([IIIII)V") — World.resetVisCalc. Builds
    // the visBacking[pitch][yaw][dx][dz] table: for each camera pitch
    // band (8 × 32 units) and yaw band (32 × 64 units), whether the
    // tile at offset (dx, dz) can project inside the viewport.
    // `heights` is the per-pitch camera Y table; `min_y`/`max_y` the
    // renderable vertical extent; (width, height) the viewport size.
    pub fn reset_vis_calc(&mut self, heights: &[i32], min_y: i32, max_y: i32,
                          width: i32, height: i32) {
        self.x_clip = 0;
        self.y_clip = 0;
        self.x_clip2 = width;
        self.y_clip2 = height;
        self.x_orig = width / 2;
        self.y_orig = height / 2;

        let sin_t = pix3d::sin_table();
        let cos_t = pix3d::cos_table();
        let mut backing = vec![vec![vec![vec![false; 53]; 53]; 32]; 9];
        let mut pitch = 128;
        while pitch <= 384 {
            let mut yaw = 0;
            while yaw < 2048 {
                self.camera_sin_x = sin_t[pitch as usize];
                self.camera_cos_x = cos_t[pitch as usize];
                self.camera_sin_y = sin_t[yaw as usize];
                self.camera_cos_y = cos_t[yaw as usize];
                let pi = ((pitch - 128) / 32) as usize;
                let yi = (yaw / 64) as usize;
                for dx in -26..=26i32 {
                    for dz in -26..=26i32 {
                        let wx = dx * 128;
                        let wz = dz * 128;
                        let mut visible = false;
                        let mut wy = -min_y;
                        while wy <= max_y {
                            if self.test_point(wx, heights[pi] + wy, wz) {
                                visible = true;
                                break;
                            }
                            wy += 128;
                        }
                        backing[pi][yi][(dx + 25 + 1) as usize][(dz + 25 + 1) as usize] = visible;
                    }
                }
                yaw += 64;
            }
            pitch += 32;
        }
        // Smear pass: a tile is visible if any neighbour in the
        // adjacent pitch/yaw band is. Note Java's `% 31` (not 32) on
        // the yaw neighbour — kept verbatim.
        self.vis_backing = vec![vec![vec![vec![false; 51]; 51]; 32]; 8];
        for pi in 0..8usize {
            for yi in 0..32usize {
                for dx in -25..25i32 {
                    for dz in -25..25i32 {
                        let mut visible = false;
                        'smear: for nx in -1..=1i32 {
                            for nz in -1..=1i32 {
                                let bx = (dx + nx + 25 + 1) as usize;
                                let bz = (dz + nz + 25 + 1) as usize;
                                if backing[pi][yi][bx][bz]
                                    || backing[pi][(yi + 1) % 31][bx][bz]
                                    || backing[pi + 1][yi][bx][bz]
                                    || backing[pi + 1][(yi + 1) % 31][bx][bz]
                                {
                                    visible = true;
                                    break 'smear;
                                }
                            }
                        }
                        self.vis_backing[pi][yi][(dx + 25) as usize][(dz + 25) as usize] = visible;
                    }
                }
            }
        }
    }

    // @ObfuscatedName("aq.ah(III)Z") — World.testPoint. Projects a
    // camera-relative point with the current camera trig and tests it
    // against the stored clip rect.
    pub fn test_point(&self, x: i32, y: i32, z: i32) -> bool {
        let view_x = (self.camera_cos_y * x + self.camera_sin_y * z) >> 16;
        let temp = (self.camera_cos_y * z - self.camera_sin_y * x) >> 16;
        let view_z = (self.camera_sin_x * y + self.camera_cos_x * temp) >> 16;
        let view_y = (self.camera_cos_x * y - self.camera_sin_x * temp) >> 16;
        if view_z < 50 || view_z > 3500 {
            return false;
        }
        let sx = (view_x << 9) / view_z + self.x_orig;
        let sy = (view_y << 9) / view_z + self.y_orig;
        sx >= self.x_clip && sx <= self.x_clip2 && sy >= self.y_clip && sy <= self.y_clip2
    }

    // @ObfuscatedName("aq.ay(III)V") — World.updateMousePicking.
    // `click_x`/`click_y` are mouse coords in the same space the
    // renderer's screen coords use (absolute viewport coords here).
    pub fn update_mouse_picking(&mut self, level: i32, x: i32, y: i32) {
        self.click = true;
        self.click_lev = level;
        self.click_x = x;
        self.click_y = y;
        self.ground_x = -1;
        self.ground_z = -1;
    }

    // ── Frame render (World.java:1063-1223) ──────────────────────────

    // @ObfuscatedName("aq.al(IIIIII)V") — World.renderAll. Camera world
    // coords + pitch (128..383) + yaw (0..2047) + top render level.
    pub fn render_all(&mut self, mut eye_x: i32, eye_y: i32, mut eye_z: i32,
                      pitch: i32, yaw: i32, top_level: i32) {
        if eye_x < 0 {
            eye_x = 0;
        } else if eye_x >= self.max_tile_x * 128 {
            eye_x = self.max_tile_x * 128 - 1;
        }
        if eye_z < 0 {
            eye_z = 0;
        } else if eye_z >= self.max_tile_z * 128 {
            eye_z = self.max_tile_z * 128 - 1;
        }
        self.cycle_no += 1;
        let sin_t = pix3d::sin_table();
        let cos_t = pix3d::cos_table();
        self.camera_sin_x = sin_t[(pitch & 0x7FF) as usize];
        self.camera_cos_x = cos_t[(pitch & 0x7FF) as usize];
        self.camera_sin_y = sin_t[(yaw & 0x7FF) as usize];
        self.camera_cos_y = cos_t[(yaw & 0x7FF) as usize];
        self.vis_dirty = ((((pitch - 128) / 32).clamp(0, 7)) as usize,
                          ((yaw / 64).clamp(0, 31)) as usize);
        self.cx = eye_x;
        self.cy = eye_y;
        self.cz = eye_z;
        self.gx = eye_x / 128;
        self.gz = eye_z / 128;
        self.max_level = top_level;
        self.min_x = (self.gx - 25).max(0);
        self.min_z = (self.gz - 25).max(0);
        self.max_x = (self.gx + 25).min(self.max_tile_x);
        self.max_z = (self.gz + 25).min(self.max_tile_z);
        self.calc_occlude();
        self.fill_left = 0;

        // Visibility-mark pass.
        for level in self.min_level..self.max_tile_level {
            for x in self.min_x..self.max_x {
                for z in self.min_z..self.max_z {
                    let visible = {
                        let dirty = self.vis_dirty;
                        let vis = if self.vis_backing.is_empty() {
                            true
                        } else {
                            self.vis_backing[dirty.0][dirty.1]
                                [(x - self.gx + 25) as usize][(z - self.gz + 25) as usize]
                        };
                        vis || self.groundh[level as usize][x as usize][z as usize] - eye_y >= 2000
                    };
                    let Some(sq) = self.sq_mut(level, x, z) else { continue };
                    if sq.draw_level <= top_level && visible {
                        sq.draw_front = true;
                        sq.draw_back = true;
                        sq.draw_sprites = sq.sprite_count() > 0;
                        self.fill_left += 1;
                    } else {
                        sq.draw_front = false;
                        sq.draw_back = false;
                        sq.check_loc_spans = 0;
                    }
                }
            }
        }

        // Two octant passes: front-gated, then forced.
        for pass in 0..2 {
            let front_first = pass == 0;
            for level in self.min_level..self.max_tile_level {
                for off_x in -25..=0i32 {
                    let far_x = self.gx + off_x;
                    let near_x = self.gx - off_x;
                    if far_x < self.min_x && near_x >= self.max_x {
                        continue;
                    }
                    for off_z in -25..=0i32 {
                        let far_z = self.gz + off_z;
                        let near_z = self.gz - off_z;
                        let mut tiles: [(i32, i32); 4] = [(-1, -1); 4];
                        let mut n = 0;
                        if far_x >= self.min_x {
                            if far_z >= self.min_z {
                                tiles[n] = (far_x, far_z);
                                n += 1;
                            }
                            if near_z < self.max_z {
                                tiles[n] = (far_x, near_z);
                                n += 1;
                            }
                        }
                        if near_x < self.max_x {
                            if far_z >= self.min_z {
                                tiles[n] = (near_x, far_z);
                                n += 1;
                            }
                            if near_z < self.max_z {
                                tiles[n] = (near_x, near_z);
                                n += 1;
                            }
                        }
                        for &(tx, tz) in tiles.iter().take(n) {
                            let pending = self.sq(level, tx, tz)
                                .map_or(false, |sq| sq.draw_front);
                            if pending {
                                self.fill(level, tx, tz, front_first);
                            }
                        }
                        if self.fill_left == 0 {
                            self.click = false;
                            return;
                        }
                    }
                }
            }
        }
        self.click = false;
    }

    // ── Fill (World.java:1225-1671) ──────────────────────────────────

    // @ObfuscatedName("aq.ab(Les;Z)V") — World.fill. The fill-queue
    // processor: renders a square's ground + walls + decor + sprites
    // in painter order, deferring squares whose closer neighbours are
    // still pending. Java's nested do/while maze decompiles to the
    // labeled flow below; semantics preserved exactly.
    pub fn fill(&mut self, level: i32, x: i32, z: i32, mut front_first: bool) {
        self.fill_queue.push_back((level as usize, x as usize, z as usize));
        'pop: loop {
            let Some((lvl_u, x_u, z_u)) = self.fill_queue.pop_front() else { return };
            let (lvl, x, z) = (lvl_u as i32, x_u as i32, z_u as i32);
            let Some((draw_back, draw_front, original_level, sprite_spans)) =
                self.sq(lvl, x, z).map(|sq| {
                    (sq.draw_back, sq.draw_front, sq.original_level, sq.sprite_spans)
                })
            else {
                continue 'pop;
            };
            if !draw_back {
                continue 'pop;
            }

            if draw_front {
                if front_first {
                    // Closer-neighbour gating: defer this square if a
                    // neighbour between it and the camera still has a
                    // pending back pass.
                    if lvl > 0 {
                        if self.sq(lvl - 1, x, z).map_or(false, |s| s.draw_back) {
                            continue 'pop;
                        }
                    }
                    if x <= self.gx && x > self.min_x {
                        let blocked = self.sq(lvl, x - 1, z).map_or(false, |s| {
                            s.draw_back && (s.draw_front || (sprite_spans & 0x1) == 0)
                        });
                        if blocked {
                            continue 'pop;
                        }
                    }
                    if x >= self.gx && x < self.max_x - 1 {
                        let blocked = self.sq(lvl, x + 1, z).map_or(false, |s| {
                            s.draw_back && (s.draw_front || (sprite_spans & 0x4) == 0)
                        });
                        if blocked {
                            continue 'pop;
                        }
                    }
                    if z <= self.gz && z > self.min_z {
                        let blocked = self.sq(lvl, x, z - 1).map_or(false, |s| {
                            s.draw_back && (s.draw_front || (sprite_spans & 0x8) == 0)
                        });
                        if blocked {
                            continue 'pop;
                        }
                    }
                    if z >= self.gz && z < self.max_z - 1 {
                        let blocked = self.sq(lvl, x, z + 1).map_or(false, |s| {
                            s.draw_back && (s.draw_front || (sprite_spans & 0x2) == 0)
                        });
                        if blocked {
                            continue 'pop;
                        }
                    }
                } else {
                    front_first = true;
                }
                self.front_pass(lvl, x, z, original_level);
            }

            // Blocked-wall retry: once every sprite that overlaps the
            // wall's blocking span has rendered, the deferred wall leaf
            // can draw.
            let check_spans = self.sq(lvl, x, z).map_or(0, |s| s.check_loc_spans);
            if check_spans != 0 {
                let (block_spans, sprite_list): (i32, Vec<(usize, i32)>) =
                    self.sq(lvl, x, z).map_or((0, Vec::new()), |s| {
                        (s.block_loc_spans,
                         s.sprites.iter().copied().zip(s.sprite_span.iter().copied()).collect())
                    });
                let mut ready = true;
                for (id, span) in sprite_list {
                    let cycle = self.sprite_pool.get(id)
                        .and_then(|s| s.as_ref())
                        .map_or(self.cycle_no, |sp| sp.cycle);
                    if cycle != self.cycle_no && (span & check_spans) == block_spans {
                        ready = false;
                        break;
                    }
                }
                if ready {
                    let wall = self.sq(lvl, x, z).and_then(|s| {
                        s.wall.as_ref().map(|w| {
                            (w.type_a, w.model_a.clone(), w.x, w.y, w.z, w.typecode)
                        })
                    });
                    if let Some((type_a, model_a, wx, wy, wz, typecode)) = wall {
                        if !self.wall_occluded(original_level, x, z, type_a) {
                            if let Some(m) = model_a {
                                m.world_render(0, self.camera_sin_x, self.camera_cos_x,
                                               self.camera_sin_y, self.camera_cos_y,
                                               wx - self.cx, wy - self.cy, wz - self.cz,
                                               typecode);
                            }
                        }
                    }
                    if let Some(sq) = self.sq_mut(lvl, x, z) {
                        sq.check_loc_spans = 0;
                    }
                }
            }

            // Sprite pass — if any sprite must wait on a pending
            // neighbour, the whole square is deferred (re-queued by
            // whoever finishes that neighbour).
            if self.sq(lvl, x, z).map_or(false, |s| s.draw_sprites) {
                self.sprite_pass(lvl, x, z, original_level);
                if self.sq(lvl, x, z).map_or(false, |s| s.draw_sprites) {
                    continue 'pop;
                }
            }

            // Back-pass gate chain: all four planar neighbours toward
            // the map edge must have completed their back pass, and the
            // wall must not still be span-blocked.
            if !self.sq(lvl, x, z).map_or(false, |s| s.draw_back) {
                continue 'pop;
            }
            if self.sq(lvl, x, z).map_or(0, |s| s.check_loc_spans) != 0 {
                continue 'pop;
            }
            if x <= self.gx && x > self.min_x {
                if self.sq(lvl, x - 1, z).map_or(false, |s| s.draw_back) {
                    continue 'pop;
                }
            }
            if x >= self.gx && x < self.max_x - 1 {
                if self.sq(lvl, x + 1, z).map_or(false, |s| s.draw_back) {
                    continue 'pop;
                }
            }
            if z <= self.gz && z > self.min_z {
                if self.sq(lvl, x, z - 1).map_or(false, |s| s.draw_back) {
                    continue 'pop;
                }
            }
            if z >= self.gz && z < self.max_z - 1 {
                if self.sq(lvl, x, z + 1).map_or(false, |s| s.draw_back) {
                    continue 'pop;
                }
            }

            // ── Back pass ────────────────────────────────────────────
            if let Some(sq) = self.sq_mut(lvl, x, z) {
                sq.draw_back = false;
            }
            self.fill_left -= 1;

            // Raised ground-object stack (on top of a table etc.).
            let go = self.sq(lvl, x, z).and_then(|s| {
                s.ground_object.as_ref().map(|g| {
                    (g.height, g.bottom_obj.clone(), g.middle_obj.clone(), g.top_obj.clone(),
                     g.x, g.y, g.z, g.typecode)
                })
            });
            if let Some((height, bottom, middle, top, gx_w, gy_w, gz_w, typecode)) = go {
                if height != 0 {
                    for m in [bottom, middle, top].into_iter().flatten() {
                        m.world_render(0, self.camera_sin_x, self.camera_cos_x,
                                       self.camera_sin_y, self.camera_cos_y,
                                       gx_w - self.cx, gy_w - self.cy - height, gz_w - self.cz,
                                       typecode);
                    }
                }
            }

            // Camera-facing wall leaves + decor (POSTTAB side).
            let back_wall_types = self.sq(lvl, x, z).map_or(0, |s| s.back_wall_types);
            if back_wall_types != 0 {
                let decor = self.sq(lvl, x, z).and_then(|s| {
                    s.decor.as_ref().map(|d| {
                        (d.model.clone(), d.model2.clone(), d.wshape, d.yof,
                         d.xof, d.zof, d.x, d.y, d.z, d.typecode)
                    })
                });
                if let Some((model, model2, wshape, yof, xof, zof, dx_w, dy_w, dz_w, typecode)) = decor {
                    let decor_min_y = model.as_ref().map_or(1000, |m| m.min_y());
                    if !self.sprite_occluded(original_level, x, z, decor_min_y) {
                        if (wshape & back_wall_types) != 0 {
                            if let Some(m) = model.as_ref() {
                                m.world_render(0, self.camera_sin_x, self.camera_cos_x,
                                               self.camera_sin_y, self.camera_cos_y,
                                               xof + (dx_w - self.cx), dy_w - self.cy,
                                               zof + (dz_w - self.cz), typecode);
                            }
                        } else if wshape == 0x100 {
                            let rel_x = dx_w - self.cx;
                            let rel_y = dy_w - self.cy;
                            let rel_z = dz_w - self.cz;
                            let var74 = if yof == 1 || yof == 2 { -rel_x } else { rel_x };
                            let var75 = if yof == 2 || yof == 3 { -rel_z } else { rel_z };
                            if var75 >= var74 {
                                if let Some(m) = model.as_ref() {
                                    m.world_render(0, self.camera_sin_x, self.camera_cos_x,
                                                   self.camera_sin_y, self.camera_cos_y,
                                                   xof + rel_x, rel_y, zof + rel_z, typecode);
                                }
                            } else if let Some(m2) = model2.as_ref() {
                                m2.world_render(0, self.camera_sin_x, self.camera_cos_x,
                                                self.camera_sin_y, self.camera_cos_y,
                                                rel_x, rel_y, rel_z, typecode);
                            }
                        }
                    }
                }
                let wall = self.sq(lvl, x, z).and_then(|s| {
                    s.wall.as_ref().map(|w| {
                        (w.type_a, w.type_b, w.model_a.clone(), w.model_b.clone(),
                         w.x, w.y, w.z, w.typecode)
                    })
                });
                if let Some((type_a, type_b, model_a, model_b, wx_w, wy_w, wz_w, typecode)) = wall {
                    if (type_b & back_wall_types) != 0
                        && !self.wall_occluded(original_level, x, z, type_b)
                    {
                        if let Some(m) = model_b.as_ref() {
                            m.world_render(0, self.camera_sin_x, self.camera_cos_x,
                                           self.camera_sin_y, self.camera_cos_y,
                                           wx_w - self.cx, wy_w - self.cy, wz_w - self.cz,
                                           typecode);
                        }
                    }
                    if (type_a & back_wall_types) != 0
                        && !self.wall_occluded(original_level, x, z, type_a)
                    {
                        if let Some(m) = model_a.as_ref() {
                            m.world_render(0, self.camera_sin_x, self.camera_cos_x,
                                           self.camera_sin_y, self.camera_cos_y,
                                           wx_w - self.cx, wy_w - self.cy, wz_w - self.cz,
                                           typecode);
                        }
                    }
                }
            }

            // Propagate: level above, then planar neighbours toward
            // the camera.
            if lvl < self.max_tile_level - 1 {
                if self.sq(lvl + 1, x, z).map_or(false, |s| s.draw_back) {
                    self.fill_queue.push_back((lvl_u + 1, x_u, z_u));
                }
            }
            if x < self.gx {
                if self.sq(lvl, x + 1, z).map_or(false, |s| s.draw_back) {
                    self.fill_queue.push_back((lvl_u, x_u + 1, z_u));
                }
            }
            if z < self.gz {
                if self.sq(lvl, x, z + 1).map_or(false, |s| s.draw_back) {
                    self.fill_queue.push_back((lvl_u, x_u, z_u + 1));
                }
            }
            if x > self.gx {
                if self.sq(lvl, x - 1, z).map_or(false, |s| s.draw_back) {
                    self.fill_queue.push_back((lvl_u, x_u - 1, z_u));
                }
            }
            if z > self.gz {
                if self.sq(lvl, x, z - 1).map_or(false, |s| s.draw_back) {
                    self.fill_queue.push_back((lvl_u, x_u, z_u - 1));
                }
            }
        }
    }

    // World.fill front pass (Java 1296-1445): ground + far-side walls
    // + decor + ground decor + flat ground objects, then far-neighbour
    // propagation for multi-tile sprites.
    fn front_pass(&mut self, lvl: i32, x: i32, z: i32, original_level: i32) {
        if let Some(sq) = self.sq_mut(lvl, x, z) {
            sq.draw_front = false;
        }

        // Bridge under-geometry (the displaced original square).
        let has_linked = self.sq(lvl, x, z).map_or(false, |s| s.linked_square.is_some());
        if has_linked {
            // Ground of the linked square.
            let linked_has_qg = self.sq(lvl, x, z)
                .and_then(|s| s.linked_square.as_deref())
                .map(|ls| (ls.quick_ground.is_some(), ls.ground.is_some()));
            if let Some((has_qg, has_ground)) = linked_has_qg {
                if !has_qg {
                    if has_ground && !self.ground_occluded(0, x, z) {
                        let g = self.sq_mut(lvl, x, z).unwrap()
                            .linked_square.as_mut().unwrap().ground.take();
                        if let Some(g) = g {
                            self.render_ground(&g, x, z);
                            self.sq_mut(lvl, x, z).unwrap()
                                .linked_square.as_mut().unwrap().ground = Some(g);
                        }
                    }
                } else if !self.ground_occluded(0, x, z) {
                    let qg = self.sq(lvl, x, z)
                        .and_then(|s| s.linked_square.as_deref())
                        .and_then(|ls| ls.quick_ground.clone());
                    if let Some(qg) = qg {
                        self.render_quick_ground(&qg, 0, x, z);
                    }
                }
            }
            // Linked wall (model A only) + linked sprites.
            let linked_wall = self.sq(lvl, x, z)
                .and_then(|s| s.linked_square.as_deref())
                .and_then(|ls| ls.wall.as_ref().map(|w| {
                    (w.model_a.clone(), w.x, w.y, w.z, w.typecode)
                }));
            if let Some((model_a, wx_w, wy_w, wz_w, typecode)) = linked_wall {
                if let Some(m) = model_a {
                    m.world_render(0, self.camera_sin_x, self.camera_cos_x,
                                   self.camera_sin_y, self.camera_cos_y,
                                   wx_w - self.cx, wy_w - self.cy, wz_w - self.cz, typecode);
                }
            }
            let linked_sprites: Vec<usize> = self.sq(lvl, x, z)
                .and_then(|s| s.linked_square.as_deref())
                .map(|ls| ls.sprites.clone())
                .unwrap_or_default();
            for id in linked_sprites {
                let info = self.sprite_pool.get(id).and_then(|s| s.as_ref()).map(|sp| {
                    (sp.model.clone(), sp.yaw, sp.x, sp.y, sp.z, sp.typecode)
                });
                if let Some((Some(m), yaw, sx, sy, sz, typecode)) = info {
                    m.world_render(yaw, self.camera_sin_x, self.camera_cos_x,
                                   self.camera_sin_y, self.camera_cos_y,
                                   sx - self.cx, sy - self.cy, sz - self.cz, typecode);
                }
            }
        }

        // Own ground.
        let mut ground_drawn = false;
        let has_qg = self.sq(lvl, x, z).map_or(false, |s| s.quick_ground.is_some());
        if !has_qg {
            let has_ground = self.sq(lvl, x, z).map_or(false, |s| s.ground.is_some());
            if has_ground && !self.ground_occluded(original_level, x, z) {
                ground_drawn = true;
                let g = self.sq_mut(lvl, x, z).unwrap().ground.take();
                if let Some(g) = g {
                    self.render_ground(&g, x, z);
                    self.sq_mut(lvl, x, z).unwrap().ground = Some(g);
                }
            }
        } else if !self.ground_occluded(original_level, x, z) {
            ground_drawn = true;
            let qg = self.sq(lvl, x, z).and_then(|s| s.quick_ground.clone());
            if let Some(qg) = qg {
                // Invisible-marker tiles still render during a click
                // frame so picking can hit them.
                if qg.colour_ne != 12345678 || (self.click && lvl <= self.click_lev) {
                    self.render_quick_ground(&qg, original_level, x, z);
                }
            }
        }

        // Far-side (PRETAB) wall + decor.
        let mut quadrant = 0usize;
        let mut pre_bits = 0;
        let has_wall_or_decor = self.sq(lvl, x, z)
            .map_or(false, |s| s.wall.is_some() || s.decor.is_some());
        if has_wall_or_decor {
            if self.gx == x {
                quadrant += 1;
            } else if self.gx < x {
                quadrant += 2;
            }
            if self.gz == z {
                quadrant += 3;
            } else if self.gz > z {
                quadrant += 6;
            }
            pre_bits = PRETAB[quadrant];
            if let Some(sq) = self.sq_mut(lvl, x, z) {
                sq.back_wall_types = POSTTAB[quadrant];
            }
        }

        let wall = self.sq(lvl, x, z).and_then(|s| {
            s.wall.as_ref().map(|w| {
                (w.type_a, w.type_b, w.model_a.clone(), w.model_b.clone(),
                 w.x, w.y, w.z, w.typecode)
            })
        });
        if let Some((type_a, type_b, model_a, model_b, wx_w, wy_w, wz_w, typecode)) = wall {
            // Wall-vs-sprite ordering state for multi-tile sprites.
            let (check, block, inverse) = if (type_a & MIDTAB[quadrant]) == 0 {
                (0, 0, 0)
            } else if type_a == 16 {
                (3, MIDDEP_16[quadrant], 3 - MIDDEP_16[quadrant])
            } else if type_a == 32 {
                (6, MIDDEP_32[quadrant], 6 - MIDDEP_32[quadrant])
            } else if type_a == 64 {
                (12, MIDDEP_64[quadrant], 12 - MIDDEP_64[quadrant])
            } else {
                (9, MIDDEP_128[quadrant], 9 - MIDDEP_128[quadrant])
            };
            if let Some(sq) = self.sq_mut(lvl, x, z) {
                sq.check_loc_spans = check;
                sq.block_loc_spans = block;
                sq.inverse_block_loc_spans = inverse;
            }
            if (type_a & pre_bits) != 0 && !self.wall_occluded(original_level, x, z, type_a) {
                if let Some(m) = model_a.as_ref() {
                    m.world_render(0, self.camera_sin_x, self.camera_cos_x,
                                   self.camera_sin_y, self.camera_cos_y,
                                   wx_w - self.cx, wy_w - self.cy, wz_w - self.cz, typecode);
                }
            }
            if (type_b & pre_bits) != 0 && !self.wall_occluded(original_level, x, z, type_b) {
                if let Some(m) = model_b.as_ref() {
                    m.world_render(0, self.camera_sin_x, self.camera_cos_x,
                                   self.camera_sin_y, self.camera_cos_y,
                                   wx_w - self.cx, wy_w - self.cy, wz_w - self.cz, typecode);
                }
            }
        }

        let decor = self.sq(lvl, x, z).and_then(|s| {
            s.decor.as_ref().map(|d| {
                (d.model.clone(), d.model2.clone(), d.wshape, d.yof,
                 d.xof, d.zof, d.x, d.y, d.z, d.typecode)
            })
        });
        if let Some((model, model2, wshape, yof, xof, zof, dx_w, dy_w, dz_w, typecode)) = decor {
            let decor_min_y = model.as_ref().map_or(1000, |m| m.min_y());
            if !self.sprite_occluded(original_level, x, z, decor_min_y) {
                if (wshape & pre_bits) != 0 {
                    if let Some(m) = model.as_ref() {
                        m.world_render(0, self.camera_sin_x, self.camera_cos_x,
                                       self.camera_sin_y, self.camera_cos_y,
                                       xof + (dx_w - self.cx), dy_w - self.cy,
                                       zof + (dz_w - self.cz), typecode);
                    }
                } else if wshape == 256 {
                    // Diagonal decor: pick the leaf facing the camera.
                    let rel_x = dx_w - self.cx;
                    let rel_y = dy_w - self.cy;
                    let rel_z = dz_w - self.cz;
                    let var27 = if yof == 1 || yof == 2 { -rel_x } else { rel_x };
                    let var28 = if yof == 2 || yof == 3 { -rel_z } else { rel_z };
                    if var28 < var27 {
                        if let Some(m) = model.as_ref() {
                            m.world_render(0, self.camera_sin_x, self.camera_cos_x,
                                           self.camera_sin_y, self.camera_cos_y,
                                           xof + rel_x, rel_y, zof + rel_z, typecode);
                        }
                    } else if let Some(m2) = model2.as_ref() {
                        m2.world_render(0, self.camera_sin_x, self.camera_cos_x,
                                        self.camera_sin_y, self.camera_cos_y,
                                        rel_x, rel_y, rel_z, typecode);
                    }
                }
            }
        }

        if ground_drawn {
            let gd = self.sq(lvl, x, z).and_then(|s| {
                s.ground_decor.as_ref().map(|g| {
                    (g.model.clone(), g.x, g.y, g.z, g.typecode)
                })
            });
            if let Some((Some(m), gd_x, gd_y, gd_z, typecode)) = gd {
                m.world_render(0, self.camera_sin_x, self.camera_cos_x,
                               self.camera_sin_y, self.camera_cos_y,
                               gd_x - self.cx, gd_y - self.cy, gd_z - self.cz, typecode);
            }
            let go = self.sq(lvl, x, z).and_then(|s| {
                s.ground_object.as_ref().map(|g| {
                    (g.height, g.bottom_obj.clone(), g.middle_obj.clone(), g.top_obj.clone(),
                     g.x, g.y, g.z, g.typecode)
                })
            });
            if let Some((height, bottom, middle, top, go_x, go_y, go_z, typecode)) = go {
                if height == 0 {
                    for m in [bottom, middle, top].into_iter().flatten() {
                        m.world_render(0, self.camera_sin_x, self.camera_cos_x,
                                       self.camera_sin_y, self.camera_cos_y,
                                       go_x - self.cx, go_y - self.cy, go_z - self.cz, typecode);
                    }
                }
            }
        }

        // Multi-tile sprite propagation toward the map edge.
        let spans = self.sq(lvl, x, z).map_or(0, |s| s.sprite_spans);
        if spans != 0 {
            if x < self.gx && (spans & 0x4) != 0 {
                if self.sq(lvl, x + 1, z).map_or(false, |s| s.draw_back) {
                    self.fill_queue.push_back((lvl as usize, (x + 1) as usize, z as usize));
                }
            }
            if z < self.gz && (spans & 0x2) != 0 {
                if self.sq(lvl, x, z + 1).map_or(false, |s| s.draw_back) {
                    self.fill_queue.push_back((lvl as usize, x as usize, (z + 1) as usize));
                }
            }
            if x > self.gx && (spans & 0x1) != 0 {
                if self.sq(lvl, x - 1, z).map_or(false, |s| s.draw_back) {
                    self.fill_queue.push_back((lvl as usize, (x - 1) as usize, z as usize));
                }
            }
            if z > self.gz && (spans & 0x8) != 0 {
                if self.sq(lvl, x, z - 1).map_or(false, |s| s.draw_back) {
                    self.fill_queue.push_back((lvl as usize, x as usize, (z - 1) as usize));
                }
            }
        }
    }

    // World.fill sprite pass (Java 1467-1559): renders this square's
    // pending sprites farthest-first; defers (drawSprites stays true)
    // when a sprite's span still overlaps an unrendered front tile.
    fn sprite_pass(&mut self, lvl: i32, x: i32, z: i32, original_level: i32) {
        let (sprite_ids, inverse_block) = match self.sq_mut(lvl, x, z) {
            Some(sq) => {
                sq.draw_sprites = false;
                (sq.sprites.clone(), sq.inverse_block_loc_spans)
            }
            None => return,
        };
        let mut buffer: Vec<usize> = Vec::with_capacity(sprite_ids.len());
        for id in sprite_ids {
            let Some((cycle, min_tx, max_tx, min_tz, max_tz)) =
                self.sprite_pool.get(id).and_then(|s| s.as_ref()).map(|sp| {
                    (sp.cycle, sp.min_tile_x, sp.max_tile_x, sp.min_tile_z, sp.max_tile_z)
                })
            else {
                continue;
            };
            if cycle == self.cycle_no {
                continue;
            }
            let mut deferred = false;
            'span: for tx in min_tx..=max_tx {
                for tz in min_tz..=max_tz {
                    let Some(sq) = self.sq(lvl, tx, tz) else { continue };
                    if sq.draw_front {
                        deferred = true;
                        break 'span;
                    }
                    if sq.check_loc_spans != 0 {
                        let mut edge = 0;
                        if tx > min_tx {
                            edge += 1;
                        }
                        if tx < max_tx {
                            edge += 4;
                        }
                        if tz > min_tz {
                            edge += 8;
                        }
                        if tz < max_tz {
                            edge += 2;
                        }
                        if (edge & sq.check_loc_spans) == inverse_block {
                            deferred = true;
                            break 'span;
                        }
                    }
                }
            }
            if deferred {
                if let Some(sq) = self.sq_mut(lvl, x, z) {
                    sq.draw_sprites = true;
                }
                continue;
            }
            // Distance: chebyshev-ish metric from the camera tile.
            if let Some(sp) = self.sprite_pool.get_mut(id).and_then(|s| s.as_mut()) {
                let mut dx = self.gx - min_tx;
                let far_x = max_tx - self.gx;
                if far_x > dx {
                    dx = far_x;
                }
                let dz = self.gz - min_tz;
                let far_z = max_tz - self.gz;
                sp.distance = if far_z > dz { dx + far_z } else { dx + dz };
            }
            buffer.push(id);
        }

        while !buffer.is_empty() {
            let mut best_dist = -50;
            let mut best: Option<usize> = None;
            for (i, &id) in buffer.iter().enumerate() {
                let Some(sp) = self.sprite_pool.get(id).and_then(|s| s.as_ref()) else { continue };
                if sp.cycle == self.cycle_no {
                    continue;
                }
                if sp.distance > best_dist {
                    best_dist = sp.distance;
                    best = Some(i);
                } else if sp.distance == best_dist {
                    if let Some(bi) = best {
                        let cur = self.sprite_pool[buffer[bi]].as_ref().unwrap();
                        let dx = sp.x - self.cx;
                        let dz = sp.z - self.cz;
                        let cx_d = cur.x - self.cx;
                        let cz_d = cur.z - self.cz;
                        if dx * dx + dz * dz > cx_d * cx_d + cz_d * cz_d {
                            best = Some(i);
                        }
                    }
                }
            }
            let Some(best_i) = best else { break };
            let id = buffer.remove(best_i);
            let Some((model, yaw, sx, sy, sz, typecode, min_tx, max_tx, min_tz, max_tz)) =
                self.sprite_pool.get_mut(id).and_then(|s| s.as_mut()).map(|sp| {
                    sp.cycle = self.cycle_no;
                    (sp.model.clone(), sp.yaw, sp.x, sp.y, sp.z, sp.typecode,
                     sp.min_tile_x, sp.max_tile_x, sp.min_tile_z, sp.max_tile_z)
                })
            else {
                continue;
            };
            let sprite_min_y = model.as_ref().map_or(1000, |m| m.min_y());
            if !self.sprite_occluded_span(original_level, min_tx, max_tx, min_tz, max_tz,
                                          sprite_min_y) {
                if let Some(m) = model {
                    m.world_render(yaw, self.camera_sin_x, self.camera_cos_x,
                                   self.camera_sin_y, self.camera_cos_y,
                                   sx - self.cx, sy - self.cy, sz - self.cz, typecode);
                }
            }
            for tx in min_tx..=max_tx {
                for tz in min_tz..=max_tz {
                    let Some(sq) = self.sq(lvl, tx, tz) else { continue };
                    let needs_push = sq.check_loc_spans != 0
                        || ((x != tx || z != tz) && sq.draw_back);
                    if needs_push {
                        self.fill_queue.push_back((lvl as usize, tx as usize, tz as usize));
                    }
                }
            }
        }
    }

    // ── Ground rasterizers (World.java:1673-1858) ────────────────────

    // @ObfuscatedName("aq.ao(Lai;IIIIIII)V") — World.renderQuickGround.
    // Projects the 4 tile corners with the cached camera trig and
    // rasterizes the 2 triangles (gouraud / textured / low-mem tinted).
    pub fn render_quick_ground(&mut self, underlay: &QuickGround, level: i32,
                               tile_x: i32, tile_z: i32) {
        let (sin_pitch, cos_pitch, sin_yaw, cos_yaw) =
            (self.camera_sin_x, self.camera_cos_x, self.camera_sin_y, self.camera_cos_y);
        let x0 = (tile_x << 7) - self.cx;
        let z0 = (tile_z << 7) - self.cz;
        let x1 = x0 + 128;
        let z1 = z0 + 128;

        let l = level as usize;
        let y_nw = self.groundh[l][tile_x as usize][tile_z as usize] - self.cy;
        let y_ne = self.groundh[l][tile_x as usize + 1][tile_z as usize] - self.cy;
        let y_se = self.groundh[l][tile_x as usize + 1][tile_z as usize + 1] - self.cy;
        let y_sw = self.groundh[l][tile_x as usize][tile_z as usize + 1] - self.cy;

        // Corner NW.
        let vx0 = (sin_yaw * z0 + cos_yaw * x0) >> 16;
        let vt0 = (cos_yaw * z0 - sin_yaw * x0) >> 16;
        let vy0 = (cos_pitch * y_nw - sin_pitch * vt0) >> 16;
        let vz0 = (sin_pitch * y_nw + cos_pitch * vt0) >> 16;
        if vz0 < 50 {
            return;
        }
        // Corner NE.
        let vx1 = (sin_yaw * z0 + cos_yaw * x1) >> 16;
        let vt1 = (cos_yaw * z0 - sin_yaw * x1) >> 16;
        let vy1 = (cos_pitch * y_ne - sin_pitch * vt1) >> 16;
        let vz1 = (sin_pitch * y_ne + cos_pitch * vt1) >> 16;
        if vz1 < 50 {
            return;
        }
        // Corner SE.
        let vx2 = (sin_yaw * z1 + cos_yaw * x1) >> 16;
        let vt2 = (cos_yaw * z1 - sin_yaw * x1) >> 16;
        let vy2 = (cos_pitch * y_se - sin_pitch * vt2) >> 16;
        let vz2 = (sin_pitch * y_se + cos_pitch * vt2) >> 16;
        if vz2 < 50 {
            return;
        }
        // Corner SW.
        let vx3 = (sin_yaw * z1 + cos_yaw * x0) >> 16;
        let vt3 = (cos_yaw * z1 - sin_yaw * x0) >> 16;
        let vy3 = (cos_pitch * y_sw - sin_pitch * vt3) >> 16;
        let vz3 = (sin_pitch * y_sw + cos_pitch * vt3) >> 16;
        if vz3 < 50 {
            return;
        }

        let (origin_x, origin_y) = pix3d::origin();
        let sx0 = (vx0 << 9) / vz0 + origin_x;
        let sy0 = (vy0 << 9) / vz0 + origin_y;
        let sx1 = (vx1 << 9) / vz1 + origin_x;
        let sy1 = (vy1 << 9) / vz1 + origin_y;
        let sx2 = (vx2 << 9) / vz2 + origin_x;
        let sy2 = (vy2 << 9) / vz2 + origin_y;
        let sx3 = (vx3 << 9) / vz3 + origin_x;
        let sy3 = (vy3 << 9) / vz3 + origin_y;

        pix3d::set_trans(0);

        // Triangle 1: NE / NW / SE corners.
        if (sy1 - sy3) * (sx2 - sx3) - (sx1 - sx3) * (sy2 - sy3) > 0 {
            pix3d::set_hclip(pix3d::face_x_clipped(sx2, sx3, sx1));
            if self.click
                && World::inside_triangle(self.click_x, self.click_y,
                                          sy2, sy3, sy1, sx2, sx3, sx1)
            {
                self.ground_x = tile_x;
                self.ground_z = tile_z;
            }
            if underlay.texture == -1 {
                if underlay.colour_ne != 12345678 {
                    pix3d::gouraud_triangle(sx2, sy2, underlay.colour_ne,
                                            sx3, sy3, underlay.colour_nw,
                                            sx1, sy1, underlay.colour_se);
                }
            } else if self.low_mem {
                let avg = texture_manager::get_average_rgb(underlay.texture);
                pix3d::gouraud_triangle(
                    sx2, sy2, Self::mul_lightness(avg, underlay.colour_ne),
                    sx3, sy3, Self::mul_lightness(avg, underlay.colour_nw),
                    sx1, sy1, Self::mul_lightness(avg, underlay.colour_se));
            } else if underlay.flat {
                pix3d::texture_triangle_affine(
                    sy2, sy3, sy1, sx2, sx3, sx1,
                    underlay.colour_ne, underlay.colour_nw, underlay.colour_se,
                    vx0, vx1, vx3, vy0, vy1, vy3, vz0, vz1, vz3,
                    underlay.texture);
            } else {
                pix3d::texture_triangle_affine(
                    sy2, sy3, sy1, sx2, sx3, sx1,
                    underlay.colour_ne, underlay.colour_nw, underlay.colour_se,
                    vx2, vx3, vx1, vy2, vy3, vy1, vz2, vz3, vz1,
                    underlay.texture);
            }
        }
        // Triangle 2: NW / NE / SW corners.
        if (sx0 - sx1) * (sy3 - sy1) - (sy0 - sy1) * (sx3 - sx1) > 0 {
            pix3d::set_hclip(pix3d::face_x_clipped(sx0, sx1, sx3));
            if self.click
                && World::inside_triangle(self.click_x, self.click_y,
                                          sy0, sy1, sy3, sx0, sx1, sx3)
            {
                self.ground_x = tile_x;
                self.ground_z = tile_z;
            }
            if underlay.texture == -1 {
                if underlay.colour_sw != 12345678 {
                    pix3d::gouraud_triangle(sx0, sy0, underlay.colour_sw,
                                            sx1, sy1, underlay.colour_se,
                                            sx3, sy3, underlay.colour_nw);
                }
            } else if self.low_mem {
                let avg = texture_manager::get_average_rgb(underlay.texture);
                pix3d::gouraud_triangle(
                    sx0, sy0, Self::mul_lightness(avg, underlay.colour_sw),
                    sx1, sy1, Self::mul_lightness(avg, underlay.colour_se),
                    sx3, sy3, Self::mul_lightness(avg, underlay.colour_nw));
            } else {
                pix3d::texture_triangle_affine(
                    sy0, sy1, sy3, sx0, sx1, sx3,
                    underlay.colour_sw, underlay.colour_se, underlay.colour_nw,
                    vx0, vx1, vx3, vy0, vy1, vy3, vz0, vz1, vz3,
                    underlay.texture);
            }
        }
    }

    // @ObfuscatedName("aq.ag(Lar;IIIIII)V") — World.renderGround.
    // Projects every Ground vertex and rasterizes its face list.
    pub fn render_ground(&mut self, overlay: &Ground, tile_x: i32, tile_z: i32) {
        let (sin_pitch, cos_pitch, sin_yaw, cos_yaw) =
            (self.camera_sin_x, self.camera_cos_x, self.camera_sin_y, self.camera_cos_y);
        let vertex_count = overlay.vertex_x.len();
        let mut draw_x = vec![0i32; vertex_count];
        let mut draw_y = vec![0i32; vertex_count];
        let mut tex_x = vec![0i32; vertex_count];
        let mut tex_y = vec![0i32; vertex_count];
        let mut tex_z = vec![0i32; vertex_count];
        let (origin_x, origin_y) = pix3d::origin();
        let has_texture = overlay.face_texture.iter().any(|&t| t != -1);
        for i in 0..vertex_count {
            let vx = overlay.vertex_x[i] - self.cx;
            let vy = overlay.vertex_y[i] - self.cy;
            let vz = overlay.vertex_z[i] - self.cz;
            let x = (sin_yaw * vz + cos_yaw * vx) >> 16;
            let temp = (cos_yaw * vz - sin_yaw * vx) >> 16;
            let y = (cos_pitch * vy - sin_pitch * temp) >> 16;
            let z = (sin_pitch * vy + cos_pitch * temp) >> 16;
            if z < 50 {
                return;
            }
            if has_texture {
                tex_x[i] = x;
                tex_y[i] = y;
                tex_z[i] = z;
            }
            draw_x[i] = (x << 9) / z + origin_x;
            draw_y[i] = (y << 9) / z + origin_y;
        }

        pix3d::set_trans(0);

        for f in 0..overlay.face_vertex_a.len() {
            let a = overlay.face_vertex_a[f] as usize;
            let b = overlay.face_vertex_b[f] as usize;
            let c = overlay.face_vertex_c[f] as usize;
            let x_a = draw_x[a];
            let x_b = draw_x[b];
            let x_c = draw_x[c];
            let y_a = draw_y[a];
            let y_b = draw_y[b];
            let y_c = draw_y[c];
            if (x_a - x_b) * (y_c - y_b) - (x_c - x_b) * (y_a - y_b) > 0 {
                pix3d::set_hclip(pix3d::face_x_clipped(x_a, x_b, x_c));
                if self.click
                    && World::inside_triangle(self.click_x, self.click_y,
                                              y_a, y_b, y_c, x_a, x_b, x_c)
                {
                    self.ground_x = tile_x;
                    self.ground_z = tile_z;
                }
                let tex = overlay.face_texture[f];
                if tex == -1 {
                    if overlay.face_colour_a[f] != 12345678 {
                        pix3d::gouraud_triangle(x_a, y_a, overlay.face_colour_a[f],
                                                x_b, y_b, overlay.face_colour_b[f],
                                                x_c, y_c, overlay.face_colour_c[f]);
                    }
                } else if self.low_mem {
                    let avg = texture_manager::get_average_rgb(tex);
                    pix3d::gouraud_triangle(
                        x_a, y_a, Self::mul_lightness(avg, overlay.face_colour_a[f]),
                        x_b, y_b, Self::mul_lightness(avg, overlay.face_colour_b[f]),
                        x_c, y_c, Self::mul_lightness(avg, overlay.face_colour_c[f]));
                } else if overlay.flat {
                    pix3d::texture_triangle_affine(
                        y_a, y_b, y_c, x_a, x_b, x_c,
                        overlay.face_colour_a[f], overlay.face_colour_b[f],
                        overlay.face_colour_c[f],
                        tex_x[0], tex_x[1], tex_x[3],
                        tex_y[0], tex_y[1], tex_y[3],
                        tex_z[0], tex_z[1], tex_z[3],
                        tex);
                } else {
                    pix3d::texture_triangle_affine(
                        y_a, y_b, y_c, x_a, x_b, x_c,
                        overlay.face_colour_a[f], overlay.face_colour_b[f],
                        overlay.face_colour_c[f],
                        tex_x[a], tex_x[b], tex_x[c],
                        tex_y[a], tex_y[b], tex_y[c],
                        tex_z[a], tex_z[b], tex_z[c],
                        tex);
                }
            }
        }
    }

    // @ObfuscatedName("aq.ar(II)I") — World.mulLightness.
    pub fn mul_lightness(hsl: i32, intensity: i32) -> i32 {
        let mut v = ((hsl & 0x7F) * intensity) >> 7;
        if v < 2 {
            v = 2;
        } else if v > 126 {
            v = 126;
        }
        (hsl & 0xFF80) + v
    }

    // @ObfuscatedName("aq.aq(IIIIIIII)Z") — World.insideTriangle.
    // Mouse-pick predicate: bbox-reject then 3 cross-product sign tests.
    pub fn inside_triangle(x: i32, y: i32,
                           ay: i32, by: i32, cy: i32,
                           ax: i32, bx: i32, cx: i32) -> bool {
        if y < ay && y < by && y < cy {
            return false;
        }
        if y > ay && y > by && y > cy {
            return false;
        }
        if x < ax && x < bx && x < cx {
            return false;
        }
        if x > ax && x > bx && x > cx {
            return false;
        }
        let v9 = (y - ay) * (bx - ax) - (x - ax) * (by - ay);
        let v10 = (y - cy) * (ax - cx) - (x - cx) * (ay - cy);
        let v11 = (y - by) * (cx - bx) - (x - bx) * (cy - by);
        v9 * v11 > 0 && v10 * v11 > 0
    }

    // ── Occlusion (World.java:1890-2276) ─────────────────────────────

    // @ObfuscatedName("aq.at()V") — World.calcOcclude. Activates the
    // occluders near the camera this frame and precomputes their
    // perspective deltas.
    pub fn calc_occlude(&mut self) {
        self.active_occluders.clear();
        let level = (self.max_level.clamp(0, LEVELS as i32 - 1)) as usize;
        let (vp, vy) = self.vis_dirty;
        let n = self.occluders[level].len();
        for i in 0..n {
            let mut occ = self.occluders[level][i];
            match occ.kind {
                1 => {
                    let tx = occ.min_tile_x - self.gx + 25;
                    if !(0..=50).contains(&tx) {
                        continue;
                    }
                    let mut tz0 = (occ.min_tile_z - self.gz + 25).max(0);
                    let tz1 = (occ.max_tile_z - self.gz + 25).min(50);
                    let mut dirty = false;
                    while tz0 <= tz1 {
                        if self.vis_visible(vp, vy, tx, tz0) {
                            dirty = true;
                            break;
                        }
                        tz0 += 1;
                    }
                    if !dirty {
                        continue;
                    }
                    let mut dist = self.cx - occ.min_x;
                    if dist > 32 {
                        occ.mode = 1;
                    } else {
                        if dist >= -32 {
                            continue;
                        }
                        occ.mode = 2;
                        dist = -dist;
                    }
                    occ.min_delta_z = ((occ.min_z - self.cz) << 8) / dist;
                    occ.max_delta_z = ((occ.max_z - self.cz) << 8) / dist;
                    occ.min_delta_y = ((occ.min_y - self.cy) << 8) / dist;
                    occ.max_delta_y = ((occ.max_y - self.cy) << 8) / dist;
                    self.active_occluders.push(occ);
                }
                2 => {
                    let tz = occ.min_tile_z - self.gz + 25;
                    if !(0..=50).contains(&tz) {
                        continue;
                    }
                    let mut tx0 = (occ.min_tile_x - self.gx + 25).max(0);
                    let tx1 = (occ.max_tile_x - self.gx + 25).min(50);
                    let mut dirty = false;
                    while tx0 <= tx1 {
                        if self.vis_visible(vp, vy, tx0, tz) {
                            dirty = true;
                            break;
                        }
                        tx0 += 1;
                    }
                    if !dirty {
                        continue;
                    }
                    let mut dist = self.cz - occ.min_z;
                    if dist > 32 {
                        occ.mode = 3;
                    } else {
                        if dist >= -32 {
                            continue;
                        }
                        occ.mode = 4;
                        dist = -dist;
                    }
                    occ.min_delta_x = ((occ.min_x - self.cx) << 8) / dist;
                    occ.max_delta_x = ((occ.max_x - self.cx) << 8) / dist;
                    occ.min_delta_y = ((occ.min_y - self.cy) << 8) / dist;
                    occ.max_delta_y = ((occ.max_y - self.cy) << 8) / dist;
                    self.active_occluders.push(occ);
                }
                4 => {
                    let dist = occ.min_y - self.cy;
                    if dist > 128 {
                        let tz0 = (occ.min_tile_z - self.gz + 25).max(0);
                        let tz1 = (occ.max_tile_z - self.gz + 25).min(50);
                        if tz0 <= tz1 {
                            let tx0 = (occ.min_tile_x - self.gx + 25).max(0);
                            let tx1 = (occ.max_tile_x - self.gx + 25).min(50);
                            let mut dirty = false;
                            'scan: for tx in tx0..=tx1 {
                                for tz in tz0..=tz1 {
                                    if self.vis_visible(vp, vy, tx, tz) {
                                        dirty = true;
                                        break 'scan;
                                    }
                                }
                            }
                            if dirty {
                                occ.mode = 5;
                                occ.min_delta_x = ((occ.min_x - self.cx) << 8) / dist;
                                occ.max_delta_x = ((occ.max_x - self.cx) << 8) / dist;
                                occ.min_delta_z = ((occ.min_z - self.cz) << 8) / dist;
                                occ.max_delta_z = ((occ.max_z - self.cz) << 8) / dist;
                                self.active_occluders.push(occ);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn vis_visible(&self, vp: usize, vy: usize, tx: i32, tz: i32) -> bool {
        if self.vis_backing.is_empty() {
            // resetVisCalc hasn't run — treat everything as visible so
            // occluders still activate (conservative, never wrong).
            return true;
        }
        let tx = tx.clamp(0, 50) as usize;
        let tz = tz.clamp(0, 50) as usize;
        self.vis_backing[vp][vy][tx][tz]
    }

    // @ObfuscatedName("aq.ae(III)Z") — World.groundOccluded. Memoized
    // per (level, x, z) with ±cycleNo stamps.
    pub fn ground_occluded(&mut self, level: i32, x: i32, z: i32) -> bool {
        let memo = self.occlusion_cycle[level as usize][x as usize][z as usize];
        if memo == -self.cycle_no {
            return false;
        }
        if memo == self.cycle_no {
            return true;
        }
        let wx = x << 7;
        let wz = z << 7;
        let l = level as usize;
        let occluded = self.occluded(wx + 1, self.groundh[l][x as usize][z as usize], wz + 1)
            && self.occluded(wx + 128 - 1, self.groundh[l][x as usize + 1][z as usize], wz + 1)
            && self.occluded(wx + 128 - 1, self.groundh[l][x as usize + 1][z as usize + 1], wz + 128 - 1)
            && self.occluded(wx + 1, self.groundh[l][x as usize][z as usize + 1], wz + 128 - 1);
        self.occlusion_cycle[level as usize][x as usize][z as usize] =
            if occluded { self.cycle_no } else { -self.cycle_no };
        occluded
    }

    // @ObfuscatedName("aq.au(IIII)Z") — World.wallOccluded. Tests a
    // wall leaf's sample points (per WSHAPE edge code) against the
    // active occluders.
    pub fn wall_occluded(&mut self, level: i32, x: i32, z: i32, wshape: i32) -> bool {
        if !self.ground_occluded(level, x, z) {
            return false;
        }
        let wx = x << 7;
        let wz = z << 7;
        let top = self.groundh[level as usize][x as usize][z as usize] - 1;
        let mid = top - 120;
        let high = top - 230;
        let diag = top - 238;
        if wshape < 16 {
            match wshape {
                1 => {
                    if wx > self.cx {
                        if !self.occluded(wx, top, wz) {
                            return false;
                        }
                        if !self.occluded(wx, top, wz + 128) {
                            return false;
                        }
                    }
                    if level > 0 {
                        if !self.occluded(wx, mid, wz) {
                            return false;
                        }
                        if !self.occluded(wx, mid, wz + 128) {
                            return false;
                        }
                    }
                    if !self.occluded(wx, high, wz) {
                        return false;
                    }
                    return self.occluded(wx, high, wz + 128);
                }
                2 => {
                    if wz < self.cz {
                        if !self.occluded(wx, top, wz + 128) {
                            return false;
                        }
                        if !self.occluded(wx + 128, top, wz + 128) {
                            return false;
                        }
                    }
                    if level > 0 {
                        if !self.occluded(wx, mid, wz + 128) {
                            return false;
                        }
                        if !self.occluded(wx + 128, mid, wz + 128) {
                            return false;
                        }
                    }
                    if !self.occluded(wx, high, wz + 128) {
                        return false;
                    }
                    return self.occluded(wx + 128, high, wz + 128);
                }
                4 => {
                    if wx < self.cx {
                        if !self.occluded(wx + 128, top, wz) {
                            return false;
                        }
                        if !self.occluded(wx + 128, top, wz + 128) {
                            return false;
                        }
                    }
                    if level > 0 {
                        if !self.occluded(wx + 128, mid, wz) {
                            return false;
                        }
                        if !self.occluded(wx + 128, mid, wz + 128) {
                            return false;
                        }
                    }
                    if !self.occluded(wx + 128, high, wz) {
                        return false;
                    }
                    return self.occluded(wx + 128, high, wz + 128);
                }
                8 => {
                    if wz > self.cz {
                        if !self.occluded(wx, top, wz) {
                            return false;
                        }
                        if !self.occluded(wx + 128, top, wz) {
                            return false;
                        }
                    }
                    if level > 0 {
                        if !self.occluded(wx, mid, wz) {
                            return false;
                        }
                        if !self.occluded(wx + 128, mid, wz) {
                            return false;
                        }
                    }
                    if !self.occluded(wx, high, wz) {
                        return false;
                    }
                    return self.occluded(wx + 128, high, wz);
                }
                _ => {}
            }
        }
        if !self.occluded(wx + 64, diag, wz + 64) {
            return false;
        }
        match wshape {
            16 => self.occluded(wx, high, wz + 128),
            32 => self.occluded(wx + 128, high, wz + 128),
            64 => self.occluded(wx + 128, high, wz),
            128 => self.occluded(wx, high, wz),
            _ => true,
        }
    }

    // @ObfuscatedName("aq.ax(IIII)Z") — World.spriteOccluded
    // (single-tile variant).
    pub fn sprite_occluded(&mut self, level: i32, x: i32, z: i32, min_y: i32) -> bool {
        if !self.ground_occluded(level, x, z) {
            return false;
        }
        let wx = x << 7;
        let wz = z << 7;
        let l = level as usize;
        self.occluded(wx + 1, self.groundh[l][x as usize][z as usize] - min_y, wz + 1)
            && self.occluded(wx + 128 - 1, self.groundh[l][x as usize + 1][z as usize] - min_y, wz + 1)
            && self.occluded(wx + 128 - 1, self.groundh[l][x as usize + 1][z as usize + 1] - min_y, wz + 128 - 1)
            && self.occluded(wx + 1, self.groundh[l][x as usize][z as usize + 1] - min_y, wz + 128 - 1)
    }

    // @ObfuscatedName("aq.ai(IIIIII)Z") — World.spriteOccluded
    // (tile-span variant).
    pub fn sprite_occluded_span(&mut self, level: i32, min_tx: i32, max_tx: i32,
                                min_tz: i32, max_tz: i32, min_y: i32) -> bool {
        if min_tx == max_tx && min_tz == max_tz {
            return self.sprite_occluded(level, min_tx, min_tz, min_y);
        }
        for tx in min_tx..=max_tx {
            for tz in min_tz..=max_tz {
                if self.occlusion_cycle[level as usize][tx as usize][tz as usize] == -self.cycle_no {
                    return false;
                }
            }
        }
        let wx0 = (min_tx << 7) + 1;
        let wz0 = (min_tz << 7) + 2;
        let wy = self.groundh[level as usize][min_tx as usize][min_tz as usize] - min_y;
        if !self.occluded(wx0, wy, wz0) {
            return false;
        }
        let wx1 = (max_tx << 7) - 1;
        if !self.occluded(wx1, wy, wz0) {
            return false;
        }
        let wz1 = (max_tz << 7) - 1;
        if !self.occluded(wx0, wy, wz1) {
            return false;
        }
        self.occluded(wx1, wy, wz1)
    }

    // @ObfuscatedName("aq.aj(III)Z") — World.occluded. Tests one world
    // point against every active occluder's perspective-scaled bounds.
    pub fn occluded(&self, x: i32, y: i32, z: i32) -> bool {
        for occ in &self.active_occluders {
            match occ.mode {
                1 => {
                    let d = occ.min_x - x;
                    if d > 0 {
                        let lo_z = ((occ.min_delta_z * d) >> 8) + occ.min_z;
                        let hi_z = ((occ.max_delta_z * d) >> 8) + occ.max_z;
                        let lo_y = ((occ.min_delta_y * d) >> 8) + occ.min_y;
                        let hi_y = ((occ.max_delta_y * d) >> 8) + occ.max_y;
                        if z >= lo_z && z <= hi_z && y >= lo_y && y <= hi_y {
                            return true;
                        }
                    }
                }
                2 => {
                    let d = x - occ.min_x;
                    if d > 0 {
                        let lo_z = ((occ.min_delta_z * d) >> 8) + occ.min_z;
                        let hi_z = ((occ.max_delta_z * d) >> 8) + occ.max_z;
                        let lo_y = ((occ.min_delta_y * d) >> 8) + occ.min_y;
                        let hi_y = ((occ.max_delta_y * d) >> 8) + occ.max_y;
                        if z >= lo_z && z <= hi_z && y >= lo_y && y <= hi_y {
                            return true;
                        }
                    }
                }
                3 => {
                    let d = occ.min_z - z;
                    if d > 0 {
                        let lo_x = ((occ.min_delta_x * d) >> 8) + occ.min_x;
                        let hi_x = ((occ.max_delta_x * d) >> 8) + occ.max_x;
                        let lo_y = ((occ.min_delta_y * d) >> 8) + occ.min_y;
                        let hi_y = ((occ.max_delta_y * d) >> 8) + occ.max_y;
                        if x >= lo_x && x <= hi_x && y >= lo_y && y <= hi_y {
                            return true;
                        }
                    }
                }
                4 => {
                    let d = z - occ.min_z;
                    if d > 0 {
                        let lo_x = ((occ.min_delta_x * d) >> 8) + occ.min_x;
                        let hi_x = ((occ.max_delta_x * d) >> 8) + occ.max_x;
                        let lo_y = ((occ.min_delta_y * d) >> 8) + occ.min_y;
                        let hi_y = ((occ.max_delta_y * d) >> 8) + occ.max_y;
                        if x >= lo_x && x <= hi_x && y >= lo_y && y <= hi_y {
                            return true;
                        }
                    }
                }
                5 => {
                    let d = y - occ.min_y;
                    if d > 0 {
                        let lo_x = ((occ.min_delta_x * d) >> 8) + occ.min_x;
                        let hi_x = ((occ.max_delta_x * d) >> 8) + occ.max_x;
                        let lo_z = ((occ.min_delta_z * d) >> 8) + occ.min_z;
                        let hi_z = ((occ.max_delta_z * d) >> 8) + occ.max_z;
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
}

// Back-compat shim: insideTriangle as a free function (mouse-pick
// predicates in ModelLit / Sprite paths use it without a World handle).
pub fn inside_triangle_static(
    x: i32, y: i32,
    ay: i32, by: i32, cy: i32,
    ax: i32, bx: i32, cx: i32,
) -> bool {
    World::inside_triangle(x, y, ay, by, cy, ax, bx, cx)
}
