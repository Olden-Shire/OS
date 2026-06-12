// @ObfuscatedName("ar") — jag::oldscape::dash3d::Ground
//
// Per-tile triangulated overlay+underlay mesh. ClientBuild.finishBuild
// calls World.setGround with `arg3 = floors+1` (the overlay shape, 2..13
// — 0/1 take the QuickGround simple path). The Ground constructor walks
// the `defShapeP` table to place vertices (corner / mid-edge / inner
// offsets), then `defShapeF` to emit the per-shape triangle list with
// per-face overlay-vs-underlay colouring.
//
// The two tables encode the full 13 ground sub-triangulations:
//   0  : flat full-overlay  (4 corners → 2 tris, all overlay)
//   1  : flat full-underlay (4 corners → 2 tris, all underlay)
//   2  : flat half/half     (4 corners → 2 tris, one over / one under)
//   3-6: triangle overlays  (overlay covers a single triangle)
//   7-9: pentagon overlays  (overlay covers a 5-corner region)
//   10-12: small inner overlays (overlay covers an inset region)
// Indices 0..7 are corner+mid-edge nodes (1-8) and 9..16 are interior
// offset nodes used for the small/pentagon overlay variants.

#![allow(dead_code)]

// @ObfuscatedName("ar.k") — m_defShapeP
// One row per overlay shape; entries are vertex node IDs (1..16).
pub const DEF_SHAPE_P: &[&[i32]] = &[
    &[1, 3, 5, 7],
    &[1, 3, 5, 7],
    &[1, 3, 5, 7],
    &[1, 3, 5, 7, 6],
    &[1, 3, 5, 7, 6],
    &[1, 3, 5, 7, 6],
    &[1, 3, 5, 7, 6],
    &[1, 3, 5, 7, 2, 6],
    &[1, 3, 5, 7, 2, 8],
    &[1, 3, 5, 7, 2, 8],
    &[1, 3, 5, 7, 11, 12],
    &[1, 3, 5, 7, 11, 12],
    &[1, 3, 5, 7, 13, 14],
];

// @ObfuscatedName("ar.o") — m_defShapeF
// One row per overlay shape, packed as quads: (overlay_flag, a, b, c).
pub const DEF_SHAPE_F: &[&[i32]] = &[
    &[0, 1, 2, 3, 0, 0, 1, 3],
    &[1, 1, 2, 3, 1, 0, 1, 3],
    &[0, 1, 2, 3, 1, 0, 1, 3],
    &[0, 0, 1, 2, 0, 0, 2, 4, 1, 0, 4, 3],
    &[0, 0, 1, 4, 0, 0, 4, 3, 1, 1, 2, 4],
    &[0, 0, 4, 3, 1, 0, 1, 2, 1, 0, 2, 4],
    &[0, 1, 2, 4, 1, 0, 1, 4, 1, 0, 4, 3],
    &[0, 4, 1, 2, 0, 4, 2, 5, 1, 0, 4, 5, 1, 0, 5, 3],
    &[0, 4, 1, 2, 0, 4, 2, 3, 0, 4, 3, 5, 1, 0, 4, 5],
    &[0, 0, 4, 5, 1, 4, 1, 2, 1, 4, 2, 3, 1, 4, 3, 5],
    &[0, 0, 1, 5, 0, 1, 4, 5, 0, 1, 2, 4, 1, 0, 5, 3, 1, 5, 4, 3, 1, 4, 2, 3],
    &[1, 0, 1, 5, 1, 1, 4, 5, 1, 1, 2, 4, 0, 0, 5, 3, 0, 5, 4, 3, 0, 4, 2, 3],
    &[1, 0, 5, 4, 1, 0, 1, 5, 0, 0, 4, 3, 0, 4, 5, 3, 0, 5, 2, 3, 0, 1, 2, 5],
];

#[derive(Debug, Clone)]
pub struct Ground {
    // @ObfuscatedName("ar.r")
    pub vertex_x: Vec<i32>,
    // @ObfuscatedName("ar.d")
    pub vertex_y: Vec<i32>,
    // @ObfuscatedName("ar.l")
    pub vertex_z: Vec<i32>,
    // @ObfuscatedName("ar.m") / "ar.c" / "ar.n"
    pub face_colour_a: Vec<i32>,
    pub face_colour_b: Vec<i32>,
    pub face_colour_c: Vec<i32>,
    // @ObfuscatedName("ar.j") / "ar.z" / "ar.g"
    pub face_vertex_a: Vec<i32>,
    pub face_vertex_b: Vec<i32>,
    pub face_vertex_c: Vec<i32>,
    // @ObfuscatedName("ar.q") — -1 for untextured faces.
    pub face_texture: Vec<i32>,
    // @ObfuscatedName("ar.i") — all 4 corners share the same height.
    pub flat: bool,
    // @ObfuscatedName("ar.s") / "ar.u"
    pub overlay_shape: i32,
    pub overlay_rotation: i32,
    // @ObfuscatedName("ar.v") / "ar.w"
    pub minimap_overlay: i32,
    pub minimap_underlay: i32,
}

impl Ground {
    // jag::oldscape::dash3d::Ground::<init>
    // arg0..arg2  = shape, rotation, texture
    // arg3, arg4  = tile X, Z (used to anchor vertex x/z in world units)
    // arg5..arg8  = NW/NE/SE/SW heights
    // arg9..arg12 = NW/NE/SE/SW underlay colours
    // arg13..arg16 = NW/NE/SE/SW overlay colours
    // arg17, arg18 = minimap underlay rgb, minimap overlay rgb
    pub fn new(
        shape: i32,
        rotation: i32,
        texture: i32,
        tile_x: i32,
        tile_z: i32,
        h_nw: i32,
        h_ne: i32,
        h_se: i32,
        h_sw: i32,
        c_nw_u: i32,
        c_ne_u: i32,
        c_se_u: i32,
        c_sw_u: i32,
        c_nw_o: i32,
        c_ne_o: i32,
        c_se_o: i32,
        c_sw_o: i32,
        minimap_under: i32,
        minimap_over: i32,
    ) -> Self {
        let mut flat = true;
        if h_nw != h_ne || h_nw != h_se || h_nw != h_sw {
            flat = false;
        }
        let unit: i32 = 128;
        let mid = unit / 2;
        let qtr = unit / 4;
        let three_qtr = unit * 3 / 4;
        let shape_us = (shape as usize).min(DEF_SHAPE_P.len() - 1);
        let nodes = DEF_SHAPE_P[shape_us];
        let n = nodes.len();
        let mut vertex_x = vec![0i32; n];
        let mut vertex_y = vec![0i32; n];
        let mut vertex_z = vec![0i32; n];
        let mut under_colour = vec![0i32; n];
        let mut over_colour = vec![0i32; n];
        let base_x = tile_x * unit;
        let base_z = tile_z * unit;
        for (i, &raw_node) in nodes.iter().enumerate() {
            let mut node = raw_node;
            // Rotation remap (matches Java exactly).
            if (node & 0x1) == 0 && node <= 8 {
                node = ((node - rotation - rotation - 1) & 0x7) + 1;
            }
            if node > 8 && node <= 12 {
                node = ((node - 9 - rotation) & 0x3) + 9;
            }
            if node > 12 && node <= 16 {
                node = ((node - 13 - rotation) & 0x3) + 13;
            }
            let (x, z, y, uc, oc) = match node {
                1 => (base_x, base_z, h_nw, c_nw_u, c_nw_o),
                2 => (mid + base_x, base_z, (h_nw + h_ne) >> 1, (c_nw_u + c_ne_u) >> 1, (c_nw_o + c_ne_o) >> 1),
                3 => (unit + base_x, base_z, h_ne, c_ne_u, c_ne_o),
                4 => (unit + base_x, mid + base_z, (h_ne + h_se) >> 1, (c_ne_u + c_se_u) >> 1, (c_ne_o + c_se_o) >> 1),
                5 => (unit + base_x, unit + base_z, h_se, c_se_u, c_se_o),
                6 => (mid + base_x, unit + base_z, (h_se + h_sw) >> 1, (c_se_u + c_sw_u) >> 1, (c_se_o + c_sw_o) >> 1),
                7 => (base_x, unit + base_z, h_sw, c_sw_u, c_sw_o),
                8 => (base_x, mid + base_z, (h_nw + h_sw) >> 1, (c_nw_u + c_sw_u) >> 1, (c_nw_o + c_sw_o) >> 1),
                9 => (mid + base_x, qtr + base_z, (h_nw + h_ne) >> 1, (c_nw_u + c_ne_u) >> 1, (c_nw_o + c_ne_o) >> 1),
                10 => (three_qtr + base_x, mid + base_z, (h_ne + h_se) >> 1, (c_ne_u + c_se_u) >> 1, (c_ne_o + c_se_o) >> 1),
                11 => (mid + base_x, three_qtr + base_z, (h_se + h_sw) >> 1, (c_se_u + c_sw_u) >> 1, (c_se_o + c_sw_o) >> 1),
                12 => (qtr + base_x, mid + base_z, (h_nw + h_sw) >> 1, (c_nw_u + c_sw_u) >> 1, (c_nw_o + c_sw_o) >> 1),
                13 => (qtr + base_x, qtr + base_z, h_nw, c_nw_u, c_nw_o),
                14 => (three_qtr + base_x, qtr + base_z, h_ne, c_ne_u, c_ne_o),
                15 => (three_qtr + base_x, three_qtr + base_z, h_se, c_se_u, c_se_o),
                _ => (qtr + base_x, three_qtr + base_z, h_sw, c_sw_u, c_sw_o),
            };
            vertex_x[i] = x;
            vertex_y[i] = y;
            vertex_z[i] = z;
            under_colour[i] = uc;
            over_colour[i] = oc;
        }
        let faces = DEF_SHAPE_F[shape_us];
        let face_count = faces.len() / 4;
        let mut face_vertex_a = vec![0i32; face_count];
        let mut face_vertex_b = vec![0i32; face_count];
        let mut face_vertex_c = vec![0i32; face_count];
        let mut face_colour_a = vec![0i32; face_count];
        let mut face_colour_b = vec![0i32; face_count];
        let mut face_colour_c = vec![0i32; face_count];
        let mut face_texture = vec![-1i32; face_count];
        let mut ptr = 0;
        for f in 0..face_count {
            let kind = faces[ptr];
            let mut a = faces[ptr + 1];
            let mut b = faces[ptr + 2];
            let mut c = faces[ptr + 3];
            ptr += 4;
            // Rotation maps the 4 "outer" corners (vertex IDs 0..3 in
            // face indices, which point at vertex_x[0..3] = the 4
            // corners) into the rotated corner order.
            if a < 4 { a = (a - rotation) & 0x3; }
            if b < 4 { b = (b - rotation) & 0x3; }
            if c < 4 { c = (c - rotation) & 0x3; }
            face_vertex_a[f] = a;
            face_vertex_b[f] = b;
            face_vertex_c[f] = c;
            if kind == 0 {
                face_colour_a[f] = under_colour[a as usize];
                face_colour_b[f] = under_colour[b as usize];
                face_colour_c[f] = under_colour[c as usize];
                face_texture[f] = -1;
            } else {
                face_colour_a[f] = over_colour[a as usize];
                face_colour_b[f] = over_colour[b as usize];
                face_colour_c[f] = over_colour[c as usize];
                face_texture[f] = texture;
            }
        }
        Self {
            vertex_x,
            vertex_y,
            vertex_z,
            face_colour_a,
            face_colour_b,
            face_colour_c,
            face_vertex_a,
            face_vertex_b,
            face_vertex_c,
            face_texture,
            flat,
            overlay_shape: shape,
            overlay_rotation: rotation,
            minimap_overlay: minimap_over,
            minimap_underlay: minimap_under,
        }
    }
}
