// Per-tile scene-graph data classes from `jagex3.dash3d`. Grouped here
// because they're all small, primarily-data carriers used by World's
// per-tile dispatch (`squares[level][x][z]`).
//
// Each struct mirrors a Java class with matching name; field tags
// preserve Java's @ObfuscatedName so diff-against-future-revisions
// still locates them.

#![allow(dead_code)]

use std::sync::Arc;

use crate::dash3d::ground::Ground;
use crate::dash3d::model_source::ModelSource;

// @ObfuscatedName("ai") — jag::oldscape::dash3d::QuickGround.
//
// Flat 4-corner tile shortcut for shapes 0 and 1 (the two
// non-triangulated cases). Verbatim field set from QuickGround.java.
#[derive(Debug, Clone, Default)]
pub struct QuickGround {
    // @ObfuscatedName("ai.r")
    pub colour_sw: i32,
    // @ObfuscatedName("ai.d")
    pub colour_se: i32,
    // @ObfuscatedName("ai.l")
    pub colour_ne: i32,
    // @ObfuscatedName("ai.m")
    pub colour_nw: i32,
    // @ObfuscatedName("ai.c")
    pub texture: i32,
    // @ObfuscatedName("ai.n")
    pub flat: bool,
    // @ObfuscatedName("ai.j")
    pub minimap_rgb: i32,
}

impl QuickGround {
    // QuickGround.java:30-38 constructor — positional args
    // (colourSW, colourSE, colourNE, colourNW, texture, minimapRgb, flat).
    pub fn new(colour_sw: i32, colour_se: i32, colour_ne: i32, colour_nw: i32,
               texture: i32, minimap_rgb: i32, flat: bool) -> Self {
        Self { colour_sw, colour_se, colour_ne, colour_nw, texture, flat, minimap_rgb }
    }
}

// @ObfuscatedName("at") — jag::oldscape::dash3d::Wall.
//
// Per-tile wall record: holds up-to-two model references (A is the
// main wall, B is the second leaf for L-walls), plus the WSHAPE
// edge codes and per-leaf type ids.
#[derive(Clone, Default)]
pub struct Wall {
    // @ObfuscatedName("at.j")
    pub x: i32,
    // @ObfuscatedName("at.z")
    pub y: i32,
    // @ObfuscatedName("at.g")
    pub z: i32,
    // @ObfuscatedName("at.q") / "at.i" — WSHAPE edge bit codes.
    pub type_a: i32,
    pub type_b: i32,
    // @ObfuscatedName("at.s") / "at.u" — ModelSource refs.
    pub model_a: Option<Arc<ModelSource>>,
    pub model_b: Option<Arc<ModelSource>>,
    // @ObfuscatedName("at.v") — typecode (used for click-pick rebuild).
    pub typecode: i32,
    // @ObfuscatedName("at.f") — secondary typecode (packed shape +
    // angle in the low byte; Java reads `& 0xFF` to extract).
    pub typecode2: i32,
}

// @ObfuscatedName("bh") — jag::oldscape::dash3d::Decor.
//
// Wall-decoration tile record. Holds the model + offset + wshape +
// additional yaw at render time (Decor.yof, used by kinds 6/7/8 to
// rotate the wall-mounted sprite about Y).
#[derive(Clone, Default)]
pub struct Decor {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub model: Option<Arc<ModelSource>>,
    pub model2: Option<Arc<ModelSource>>,
    // @ObfuscatedName("bh.s") — wall-edge bits this decor renders with.
    pub wshape: i32,
    // @ObfuscatedName("bh.f") — additional Y-axis yaw, 0..3 quarters.
    pub yof: i32,
    // @ObfuscatedName("bh.w") / "bh.e" — render-time world offsets.
    pub xof: i32,
    pub zof: i32,
    pub typecode: i32,
    pub typecode2: i32,
}

// @ObfuscatedName("af") — jag::oldscape::dash3d::GroundDecor.
//
// Flat-floor decoration (kind 22 in the LocType encoding). One model
// per tile, no rotation.
#[derive(Clone, Default)]
pub struct GroundDecor {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub model: Option<Arc<ModelSource>>,
    pub typecode: i32,
    pub typecode2: i32,
}

// @ObfuscatedName("ah") — jag::oldscape::dash3d::GroundObject.
//
// Stacked ground items (bottom / middle / top by stack value). Java
// holds ModelSource refs built from ObjType.getModelLit.
#[derive(Clone, Default)]
pub struct GroundObject {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub top_obj: Option<Arc<ModelSource>>,
    pub bottom_obj: Option<Arc<ModelSource>>,
    pub middle_obj: Option<Arc<ModelSource>>,
    pub typecode: i32,
    // @ObfuscatedName("ah.f") — offset above ground when the tile also
    // hosts a tall sprite-loc (World.setObj measures minY).
    pub height: i32,
}

// @ObfuscatedName("es") — jag::oldscape::dash3d::Square.
//
// Per-tile container — the basic World cell. Verbatim field set from
// Square.java. Sprites are pool indices into `World.sprite_pool` (Java
// stores object references; pool indices give us the same identity
// semantics without aliasing headaches).
#[derive(Default)]
pub struct Square {
    // @ObfuscatedName("es.m") / "es.c" / "es.n" — tile coords.
    pub level: i32,
    pub x: i32,
    pub z: i32,
    // @ObfuscatedName("es.j") — original level (bridge tiles keep the
    // level they were decoded at after pushDown shifts them).
    pub original_level: i32,
    // @ObfuscatedName("es.z")
    pub quick_ground: Option<QuickGround>,
    // @ObfuscatedName("es.g")
    pub ground: Option<Ground>,
    // @ObfuscatedName("es.q")
    pub wall: Option<Wall>,
    // @ObfuscatedName("es.i")
    pub decor: Option<Decor>,
    // @ObfuscatedName("es.s")
    pub ground_decor: Option<GroundDecor>,
    // @ObfuscatedName("es.u")
    pub ground_object: Option<GroundObject>,
    // @ObfuscatedName("es.v") / "es.w" — sprite slots (max 5 in Java).
    // Entries are indices into World.sprite_pool.
    pub sprites: Vec<usize>,
    // @ObfuscatedName("es.e") — per-slot span bits (parallel to sprites).
    pub sprite_span: Vec<i32>,
    // @ObfuscatedName("es.b") — OR of all span bits.
    pub sprite_spans: i32,
    // @ObfuscatedName("es.y") — render-level gate vs World.maxLevel.
    pub draw_level: i32,
    // @ObfuscatedName("es.t")
    pub draw_front: bool,
    // @ObfuscatedName("es.f")
    pub draw_back: bool,
    // @ObfuscatedName("es.k")
    pub draw_sprites: bool,
    // @ObfuscatedName("es.o") / "es.a" / "es.h" — wall-vs-sprite
    // ordering state for the fill pass.
    pub check_loc_spans: i32,
    pub block_loc_spans: i32,
    pub inverse_block_loc_spans: i32,
    // @ObfuscatedName("es.x")
    pub back_wall_types: i32,
    // @ObfuscatedName("es.p") — original Square displaced by pushDown
    // (bridge under-geometry, rendered before the deck).
    pub linked_square: Option<Box<Square>>,
}

impl Square {
    pub fn new(level: i32, x: i32, z: i32) -> Self {
        // Java's constructor sets only level/x/z — drawLevel stays at
        // its 0 default until ClientBuild calls setLayer.
        Self {
            level,
            x,
            z,
            original_level: level,
            ..Default::default()
        }
    }

    // Java's `spriteCount`.
    pub fn sprite_count(&self) -> usize {
        self.sprites.len()
    }
}
