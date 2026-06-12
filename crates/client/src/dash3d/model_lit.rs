// @ObfuscatedName("fo") — jag::oldscape::dash3d::ModelLit extends ModelSource
//
// Output of ModelUnlit.light(): every triangle face carries three HSL
// vertex colours (gouraud), priorities, alphas, and the same shared
// vertex / index arrays as the source. Java's full pipeline computes
// per-vertex point normals + smooth shading; we ship a flat-per-face
// approximation as the first cut and layer the smoothing on top later.
//
// Renderer (objRender) sits below — projects each vertex via
// pix3d::project and dispatches each face to fill_triangle /
// gouraud_triangle.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use crate::dash3d::model_unlit::ModelUnlit;
use crate::dash3d::pix3d;

// jag::oldscape::dash3d::MousePickingHelper — the scene-hover picking
// statics that live on ModelLit in Java. Client sets mouse_check +
// mouse_x/mouse_y before the scene render; worldRender appends the
// typecode of every model whose screen bounds contain the mouse;
// Client drains `picked` afterwards to build the right-click menu.
pub struct MousePick {
    // @ObfuscatedName("fo.by") — m_mouseCheck
    pub mouse_check: bool,
    // @ObfuscatedName("fo.bx") — m_mouseX
    pub mouse_x: i32,
    // @ObfuscatedName("fo.bf") — m_mouseY
    pub mouse_y: i32,
    // @ObfuscatedName("fo.bo") / "fo.bu" — m_pickedEntityTypecode +
    // m_pickedEntityCount (Java: int[1000] + count; we grow a Vec and
    // truncate on reset).
    pub picked: Vec<i32>,
}

pub static MOUSE_PICK: Mutex<MousePick> = Mutex::new(MousePick {
    mouse_check: false,
    mouse_x: 0,
    mouse_y: 0,
    picked: Vec::new(),
});

#[derive(Debug, Default, Clone)]
pub struct ModelLit {
    // @ObfuscatedName("fo.i")
    pub num_points: i32,
    // @ObfuscatedName("fo.s")
    pub point_x: Vec<i32>,
    // @ObfuscatedName("fo.u")
    pub point_y: Vec<i32>,
    // @ObfuscatedName("fo.v")
    pub point_z: Vec<i32>,

    // @ObfuscatedName("fo.w")
    pub num_faces: i32,
    // @ObfuscatedName("fo.e")
    pub face_vertex_a: Vec<i32>,
    // @ObfuscatedName("fo.b")
    pub face_vertex_b: Vec<i32>,
    // @ObfuscatedName("fo.y")
    pub face_vertex_c: Vec<i32>,
    // @ObfuscatedName("fo.t")
    pub face_colour_a: Vec<i32>,
    // @ObfuscatedName("fo.f")
    pub face_colour_b: Vec<i32>,
    // @ObfuscatedName("fo.k")
    pub face_colour_c: Vec<i32>,
    // @ObfuscatedName("fo.o")
    pub face_priority: Option<Vec<i8>>,
    // @ObfuscatedName("fo.a")
    pub face_alpha: Option<Vec<i8>>,
    // @ObfuscatedName("fo.p")
    pub priority: i8,
    // @ObfuscatedName("fo.x") — per-face texture id (-1 = none).
    pub face_texture_id: Option<Vec<i16>>,
    // @ObfuscatedName("fo.h") — per-face textured-triangle index. Picks
    // an entry in face_texture_p/m/n for the texture coordinate basis.
    pub face_texture_axis: Option<Vec<i8>>,
    // @ObfuscatedName("fo.ad")
    pub num_t: i32,
    // @ObfuscatedName("fo.ac") — vertex P (origin of UV basis).
    pub face_texture_p: Option<Vec<i16>>,
    // @ObfuscatedName("fo.aa") — vertex M (U axis).
    pub face_texture_m: Option<Vec<i16>>,
    // @ObfuscatedName("fo.as") — vertex N (V axis).
    pub face_texture_n: Option<Vec<i16>>,
    // @ObfuscatedName("fo.am") — per-label vertex indices, derived from
    // vertex_label[]. animate2() walks these to apply bone transforms.
    pub label_vertices: Option<Vec<Vec<i32>>>,
    // @ObfuscatedName("fo.ap") — per-label face indices, derived from
    // face_label[]. animate2 type-5 (alpha animation) walks these.
    pub label_faces: Option<Vec<Vec<i32>>>,

    // @ObfuscatedName("fo.av") — pick-test mode flag. ModelLit's
    // worldRender consults this to decide between AABB and triangle
    // picking on hover. Java is `boolean`; defaults to false.
    pub use_aabb_mouse_check: bool,

    // @ObfuscatedName("fo.ak") — boundingCalc: 0 = invalidated,
    // 1 = cylinder valid, 2 = AABB valid. Rotations / translations /
    // resizes reset it; calcBoundingCylinder / calcAABB populate it.
    pub bounding_calc: i32,
    // @ObfuscatedName("fo.az") / "fo.an" — Y bounds (negated so
    // higher = above origin per Java).
    pub max_y: i32,
    pub min_y: i32,
    // @ObfuscatedName("fo.ar") — sqrt(max(x²+z²)) — cylinder picking radius.
    pub radius: i32,
    // @ObfuscatedName("fo.al" / "fo.aq") — view-space depth range
    // used by render2 priority scan.
    pub min_depth: i32,
    pub max_depth: i32,
}

impl ModelLit {
    // @ObfuscatedName("fo.<init>([Lfo;I)") — ModelLit merge constructor.
    // Verbatim port of ModelLit.java:217-326: concatenates several lit
    // models (player avatar + spotanim, player + carried loc) into one,
    // offsetting vertex / texture-triplet indices and widening the
    // per-face priority / alpha / texture arrays when any input has
    // them.
    pub fn merge(models: &[&ModelLit]) -> ModelLit {
        let mut copy_priority = false;
        let mut copy_alpha = false;
        let mut copy_texture_id = false;
        let mut copy_texture_axis = false;

        let mut out = ModelLit::default();
        out.priority = -1;

        let mut total_points = 0usize;
        let mut total_faces = 0usize;
        let mut total_t = 0usize;
        for m in models {
            total_points += m.num_points as usize;
            total_faces += m.num_faces as usize;
            total_t += m.num_t as usize;
            if m.face_priority.is_none() {
                if out.priority == -1 {
                    out.priority = m.priority;
                }
                if out.priority != m.priority {
                    copy_priority = true;
                }
            } else {
                copy_priority = true;
            }
            copy_alpha |= m.face_alpha.is_some();
            copy_texture_id |= m.face_texture_id.is_some();
            copy_texture_axis |= m.face_texture_axis.is_some();
        }

        out.point_x = vec![0; total_points];
        out.point_y = vec![0; total_points];
        out.point_z = vec![0; total_points];
        out.face_vertex_a = vec![0; total_faces];
        out.face_vertex_b = vec![0; total_faces];
        out.face_vertex_c = vec![0; total_faces];
        out.face_colour_a = vec![0; total_faces];
        out.face_colour_b = vec![0; total_faces];
        out.face_colour_c = vec![0; total_faces];
        if copy_priority {
            out.face_priority = Some(vec![0; total_faces]);
        }
        if copy_alpha {
            out.face_alpha = Some(vec![0; total_faces]);
        }
        if copy_texture_id {
            out.face_texture_id = Some(vec![-1; total_faces]);
        }
        if copy_texture_axis {
            out.face_texture_axis = Some(vec![-1; total_faces]);
        }
        if total_t > 0 {
            out.face_texture_p = Some(vec![0; total_t]);
            out.face_texture_m = Some(vec![0; total_t]);
            out.face_texture_n = Some(vec![0; total_t]);
        }

        let mut np = 0usize;
        let mut nf = 0usize;
        let mut nt = 0usize;
        for m in models {
            for f in 0..m.num_faces as usize {
                out.face_vertex_a[nf] = m.face_vertex_a[f] + np as i32;
                out.face_vertex_b[nf] = m.face_vertex_b[f] + np as i32;
                out.face_vertex_c[nf] = m.face_vertex_c[f] + np as i32;
                out.face_colour_a[nf] = m.face_colour_a[f];
                out.face_colour_b[nf] = m.face_colour_b[f];
                out.face_colour_c[nf] = m.face_colour_c[f];
                if copy_priority {
                    out.face_priority.as_mut().unwrap()[nf] = match m.face_priority.as_ref() {
                        Some(p) => p[f],
                        None => m.priority,
                    };
                }
                if copy_alpha {
                    if let Some(a) = m.face_alpha.as_ref() {
                        out.face_alpha.as_mut().unwrap()[nf] = a[f];
                    }
                }
                if copy_texture_id {
                    out.face_texture_id.as_mut().unwrap()[nf] = match m.face_texture_id.as_ref() {
                        Some(t) => t[f],
                        None => -1,
                    };
                }
                if copy_texture_axis {
                    let axis = m.face_texture_axis.as_ref().map_or(-1, |a| a[f]);
                    out.face_texture_axis.as_mut().unwrap()[nf] = if axis == -1 {
                        -1
                    } else {
                        (axis as i32 + nt as i32) as i8
                    };
                }
                nf += 1;
            }
            for t in 0..m.num_t as usize {
                out.face_texture_p.as_mut().unwrap()[nt] =
                    m.face_texture_p.as_ref().map_or(0, |v| v[t]) + np as i16;
                out.face_texture_m.as_mut().unwrap()[nt] =
                    m.face_texture_m.as_ref().map_or(0, |v| v[t]) + np as i16;
                out.face_texture_n.as_mut().unwrap()[nt] =
                    m.face_texture_n.as_ref().map_or(0, |v| v[t]) + np as i16;
                nt += 1;
            }
            for v in 0..m.num_points as usize {
                out.point_x[np] = m.point_x[v];
                out.point_y[np] = m.point_y[v];
                out.point_z[np] = m.point_z[v];
                np += 1;
            }
        }

        out.num_points = np as i32;
        out.num_faces = nf as i32;
        out.num_t = nt as i32;
        out
    }

    // @ObfuscatedName("fo.ay(IIIIIIII)Z") — ModelLit.isMouseRoughlyInsideTriangle.
    // Verbatim port of ModelLit.java:1514-1526. Cheap bbox-only
    // point-in-triangle reject used during render2 mouse picking
    // before the per-triangle fine pick.
    pub fn is_mouse_roughly_inside_triangle(
        x: i32, y: i32,
        y_a: i32, y_b: i32, y_c: i32,
        x_a: i32, x_b: i32, x_c: i32,
    ) -> bool {
        if y < y_a && y < y_b && y < y_c { return false; }
        if y > y_a && y > y_b && y > y_c { return false; }
        if x < x_a && x < x_b && x < x_c { return false; }
        if x > x_a && x > x_b && x > x_c { return false; }
        true
    }

    // Transpose vertex_label / face_label arrays into per-label vec of
    // indices. Java does this inline in ModelUnlit.light; we hoist it
    // since both label_vertices and label_faces share the same shape.
    fn build_label_groups(labels: Option<&[i32]>, n: usize) -> Option<Vec<Vec<i32>>> {
        let labels = labels?;
        // First pass: count.
        let mut counts: Vec<usize> = Vec::new();
        for i in 0..n {
            let l = labels.get(i).copied().unwrap_or(0) as usize;
            if l >= counts.len() { counts.resize(l + 1, 0); }
            counts[l] += 1;
        }
        let mut groups: Vec<Vec<i32>> = counts.iter()
            .map(|&c| Vec::with_capacity(c))
            .collect();
        for i in 0..n {
            let l = labels.get(i).copied().unwrap_or(0) as usize;
            groups[l].push(i as i32);
        }
        Some(groups)
    }

    // @ObfuscatedName("fw.ay(II)I") — ModelUnlit.getColour. Combines an
    // HSL colour with a light intensity, clamping the lightness band.
    fn get_colour(colour: i32, intensity: i32) -> i32 {
        let mut v = (colour & 0x7F) * intensity >> 7;
        if v < 2 { v = 2; } else if v > 126 { v = 126; }
        (colour & 0xFF80) + v
    }

    // @ObfuscatedName("fw.am(IIIIII)Lfo;") — ModelUnlit.light. Smooth
    // Gouraud shading via per-vertex point normals. Caller must have
    // run `calculate_normals()` on `src` (we call it ourselves if it
    // hasn't run yet).
    pub fn light(src: &mut ModelUnlit, ambient: i32, contrast: i32, lx: i32, ly: i32, lz: i32) -> Self {
        src.calculate_normals();
        let distance = ((lx * lx + ly * ly + lz * lz) as f64).sqrt() as i32;
        let scale = ((contrast * distance) >> 8).max(1);

        let mut lit = ModelLit::default();
        lit.num_points = src.num_points;
        lit.point_x = src.point_x.clone();
        lit.point_y = src.point_y.clone();
        lit.point_z = src.point_z.clone();
        lit.num_faces = src.num_faces;
        lit.face_vertex_a = src.face_vertex_a.clone();
        lit.face_vertex_b = src.face_vertex_b.clone();
        lit.face_vertex_c = src.face_vertex_c.clone();
        lit.face_priority = src.face_priority.clone();
        lit.face_alpha = src.face_alpha.clone();
        lit.priority = src.priority;
        lit.face_texture_id = src.face_texture_id.clone();
        // Texture-table compaction (Java ModelUnlit.light 1642-1681).
        // Keeps only the (P, M, N) anchor triples that are BOTH
        // referenced by some face AND have textureRenderType == 0
        // (plain vertex-anchored mapping), renumbering faceTextureAxis
        // to match. Crucially, faces whose texture has renderType != 0
        // (scaled / rotated types) get axis -1 — the renderer then
        // anchors UVs on the face's own vertices. Without this remap,
        // their raw axis points at anchor data that isn't a plain
        // vertex triple, producing garbage UVs (black rug centres,
        // garbled banners / wall shields).
        if src.num_t > 0 && src.face_texture_axis.is_some() {
            let num_t = src.num_t as usize;
            let num_faces = src.num_faces as usize;
            let fta = src.face_texture_axis.as_ref().unwrap();
            let trt = src.texture_render_type.as_ref();
            let render_type = |t: usize| trt.map_or(0, |v| v[t]);
            let mut axis = vec![0i32; num_t];
            for f in 0..num_faces {
                if fta[f] != -1 {
                    axis[(fta[f] as i32 & 0xFF) as usize] += 1;
                }
            }
            let mut count = 0usize;
            for t in 0..num_t {
                if axis[t] > 0 && render_type(t) == 0 {
                    count += 1;
                }
            }
            let mut p = vec![0i16; count];
            let mut m = vec![0i16; count];
            let mut n = vec![0i16; count];
            let mut cursor = 0usize;
            for t in 0..num_t {
                if axis[t] > 0 && render_type(t) == 0 {
                    p[cursor] = src.face_texture_p.as_ref().map_or(0, |v| v[t]);
                    m[cursor] = src.face_texture_m.as_ref().map_or(0, |v| v[t]);
                    n[cursor] = src.face_texture_n.as_ref().map_or(0, |v| v[t]);
                    axis[t] = cursor as i32;
                    cursor += 1;
                } else {
                    axis[t] = -1;
                }
            }
            lit.num_t = count as i32;
            lit.face_texture_p = Some(p);
            lit.face_texture_m = Some(m);
            lit.face_texture_n = Some(n);
            let mut new_axis = vec![-1i8; num_faces];
            for f in 0..num_faces {
                if fta[f] != -1 {
                    new_axis[f] = axis[(fta[f] as i32 & 0xFF) as usize] as i8;
                }
            }
            lit.face_texture_axis = Some(new_axis);
        } else {
            lit.face_texture_axis = src.face_texture_axis.clone();
            lit.face_texture_p = src.face_texture_p.clone();
            lit.face_texture_m = src.face_texture_m.clone();
            lit.face_texture_n = src.face_texture_n.clone();
            lit.num_t = src.num_t;
        }
        // Build label_vertices / label_faces — Java does this in
        // ModelUnlit.light at line 1660-1680 by transposing the
        // vertex_label[] / face_label[] arrays.
        lit.label_vertices = Self::build_label_groups(
            src.vertex_label.as_deref(), src.num_points as usize);
        lit.label_faces = Self::build_label_groups(
            src.face_label.as_deref(), src.num_faces as usize);
        lit.face_colour_a = vec![0i32; src.num_faces as usize];
        lit.face_colour_b = vec![0i32; src.num_faces as usize];
        lit.face_colour_c = vec![0i32; src.num_faces as usize];

        let point_normals = src.point_normal.as_ref().unwrap();
        // Java's per-vertex lookup goes:
        //   PointNormal n = sharedPointNormal != null && sharedPointNormal[v] != null
        //                   ? sharedPointNormal[v]
        //                   : pointNormal[v];
        // When World.shareLight has paired this vertex with one on an
        // adjacent model, both sides see the summed normal so their
        // gouraud corner colours match across the seam. We mirror it
        // via a closure to keep the per-face fetch terse.
        let shared = src.shared_point_normal.as_ref();
        let normal_at = |v: usize| -> crate::dash3d::model_unlit::PointNormal {
            if let Some(sh) = shared {
                if let Some(Some(n)) = sh.get(v) { return *n; }
            }
            point_normals[v]
        };

        for f in 0..src.num_faces as usize {
            let colour = (src.face_colour[f] as i32) & 0xFFFF;
            let ftype = src.face_render_type.as_ref().map_or(0, |v| v[f]);
            let alpha = src.face_alpha.as_ref().map_or(0, |v| v[f]);
            let mut effective_type = ftype;
            if alpha == -2 { effective_type = 3; }
            if alpha == -1 { effective_type = 2; }
            let tex_id = src.face_texture_id.as_ref().map_or(-1i32, |v| v[f] as i32);
            if tex_id == -1 {
                if effective_type == 0 {
                    let a = src.face_vertex_a[f] as usize;
                    let b = src.face_vertex_b[f] as usize;
                    let c = src.face_vertex_c[f] as usize;
                    let na = normal_at(a);
                    let nb = normal_at(b);
                    let nc = normal_at(c);
                    let denom_a = (na.w * scale).max(1);
                    let denom_b = (nb.w * scale).max(1);
                    let denom_c = (nc.w * scale).max(1);
                    let ia = (na.z * lz + na.x * lx + na.y * ly) / denom_a + ambient;
                    let ib = (nb.z * lz + nb.x * lx + nb.y * ly) / denom_b + ambient;
                    let ic = (nc.z * lz + nc.x * lx + nc.y * ly) / denom_c + ambient;
                    lit.face_colour_a[f] = Self::get_colour(colour, ia);
                    lit.face_colour_b[f] = Self::get_colour(colour, ib);
                    lit.face_colour_c[f] = Self::get_colour(colour, ic);
                } else if effective_type == 1 {
                    // Flat-shaded — Java line 1743-1747: only face_colour_a
                    // and face_colour_c get set (B is left at default 0
                    // since obj_render's `cc_idx == -1` branch only
                    // reads A).
                    let intensity = if let Some(fnt) = src.face_normal.as_ref() {
                        let fn_ = fnt[f];
                        (fn_.z * lz + fn_.x * lx + fn_.y * ly) / (scale + scale / 2).max(1) + ambient
                    } else { ambient };
                    lit.face_colour_a[f] = Self::get_colour(colour, intensity);
                    lit.face_colour_c[f] = -1;
                } else if effective_type == 3 {
                    // Type 3 = "untextured flat at colour 128" (Java
                    // line 1748-1750). face_colour_c == -1 is the
                    // flat-render marker consumed by obj_render's
                    // fill_triangle branch.
                    lit.face_colour_a[f] = 128;
                    lit.face_colour_c[f] = -1;
                } else {
                    // Type 2 untextured — Java line 1751-1752 sets
                    // only C = -2 (skip marker). A/B retain default 0.
                    lit.face_colour_c[f] = -2;
                }
            } else if effective_type == 0 {
                // Textured gouraud (Java 1754-1780) — same shared-first
                // normal lookup as the untextured branch; without it,
                // textured wall/fence runs light each segment from its
                // own normals and the seams show as brightness steps.
                let a = src.face_vertex_a[f] as usize;
                let b = src.face_vertex_b[f] as usize;
                let c = src.face_vertex_c[f] as usize;
                let na = normal_at(a);
                let nb = normal_at(b);
                let nc = normal_at(c);
                let denom_a = (na.w * scale).max(1);
                let denom_b = (nb.w * scale).max(1);
                let denom_c = (nc.w * scale).max(1);
                let ia = (na.z * lz + na.x * lx + na.y * ly) / denom_a + ambient;
                let ib = (nb.z * lz + nb.x * lx + nb.y * ly) / denom_b + ambient;
                let ic = (nc.z * lz + nc.x * lx + nc.y * ly) / denom_c + ambient;
                lit.face_colour_a[f] = Self::tex_light(ia);
                lit.face_colour_b[f] = Self::tex_light(ib);
                lit.face_colour_c[f] = Self::tex_light(ic);
            } else if effective_type == 1 {
                // Textured flat (Java 1781-1785).
                let intensity = if let Some(fnt) = src.face_normal.as_ref() {
                    let fn_ = fnt[f];
                    (fn_.z * lz + fn_.x * lx + fn_.y * ly) / (scale + scale / 2).max(1) + ambient
                } else { ambient };
                lit.face_colour_a[f] = Self::tex_light(intensity);
                lit.face_colour_c[f] = -1;
            } else {
                // Textured type 2/3 (Java 1786-1787) — skip marker.
                lit.face_colour_c[f] = -2;
            }
        }
        lit
    }

    // @ObfuscatedName("fw.al(I)I") — ModelUnlit.getTexLight
    fn tex_light(mut i: i32) -> i32 {
        if i < 2 { i = 2; } else if i > 126 { i = 126; }
        i
    }

    // @ObfuscatedName("fo.x(Lfr;ILfr;I[I)V") — ModelLit.maskAnimate
    //
    // Blends a base animation with a "mask" animation. `mask_labels` is
    // an ascending list of bone label indices that should be driven by
    // the MASK frame instead of the base — used by two-handed equipped
    // items to override the player's arm/torso bones while leaving
    // leg/head bones on the base animation.
    //
    // The arg4 array (Rust: `mask_labels`) is iterated in lock-step
    // with the base frame's bone list — `threshold` is the next label
    // boundary that demarcates "mask region begins here". Java's
    // condition `var10 != var12 || type==0` reads: apply BASE when the
    // bone label hasn't hit the threshold AND it's not an origin bone
    // (type 0 always applies from base). The mask pass uses the
    // inverted predicate `var14 == var16 || type==0`.
    //
    // Verbatim port of Java's nested-while structure at
    // ModelLit.java:558-602.
    pub fn mask_animate(
        &mut self,
        base_fs: &crate::dash3d::anim_frame_set::AnimFrameSet, base_frame: i32,
        mask_fs: &crate::dash3d::anim_frame_set::AnimFrameSet, mask_frame: i32,
        mask_labels: Option<&[i32]>,
    ) {
        if base_frame == -1 { return; }
        // Java line 564-567: if there's no mask data, fall back to a
        // straight base animation. Same on -1 mask frame.
        let Some(mask_labels) = mask_labels else {
            self.animate(base_fs, base_frame);
            return;
        };
        if mask_frame == -1 {
            self.animate(base_fs, base_frame);
            return;
        }
        let Some(base_af) = base_fs.list.get(&base_frame).cloned() else { return };
        let Some(mask_af) = mask_fs.list.get(&mask_frame).cloned() else {
            self.animate(base_fs, base_frame);
            return;
        };
        let anim_base = &base_af.base;
        let mut origin = (0i32, 0i32, 0i32);

        // Base pass — applies to bones NOT in the masked label set.
        let mut var17 = 1usize;
        let mut threshold = *mask_labels.first().unwrap_or(&i32::MAX);
        for i in 0..base_af.size as usize {
            let bone_idx = base_af.ti[i];
            while bone_idx > threshold {
                threshold = mask_labels.get(var17).copied().unwrap_or(i32::MAX);
                var17 += 1;
            }
            let bone_idx_u = bone_idx as usize;
            let ty = anim_base.r#type.get(bone_idx_u).copied().unwrap_or(0);
            // Java: `var10 != var12 || type == 0` — when the bone label
            // hasn't hit a masked threshold, apply base; type-0 origin
            // bones always apply.
            if threshold != bone_idx || ty == 0 {
                let labels = anim_base.labels.get(bone_idx_u).cloned().unwrap_or_default();
                self.animate2(ty, &labels, base_af.tx[i], base_af.ty[i], base_af.tz[i], &mut origin);
            }
        }

        // Mask pass — applies to bones in the masked label set.
        origin = (0i32, 0i32, 0i32);
        let mut var18 = 1usize;
        let mut threshold = *mask_labels.first().unwrap_or(&i32::MAX);
        for i in 0..mask_af.size as usize {
            let bone_idx = mask_af.ti[i];
            while bone_idx > threshold {
                threshold = mask_labels.get(var18).copied().unwrap_or(i32::MAX);
                var18 += 1;
            }
            let bone_idx_u = bone_idx as usize;
            let ty = anim_base.r#type.get(bone_idx_u).copied().unwrap_or(0);
            // Java: `var14 == var16 || type == 0` — inverted from above.
            if threshold == bone_idx || ty == 0 {
                let labels = anim_base.labels.get(bone_idx_u).cloned().unwrap_or_default();
                self.animate2(ty, &labels, mask_af.tx[i], mask_af.ty[i], mask_af.tz[i], &mut origin);
            }
        }
    }

    // @ObfuscatedName("fo.ad()V") — ModelLit.rotate90. Verbatim port
    // of ModelLit.java:732-739.
    pub fn rotate90(&mut self) {
        for i in 0..self.num_points as usize {
            let temp = self.point_x[i];
            self.point_x[i] = self.point_z[i];
            self.point_z[i] = -temp;
        }
        self.bounding_calc = 0;
    }

    // @ObfuscatedName("fo.ac()V") — ModelLit.rotate180.
    pub fn rotate180(&mut self) {
        for i in 0..self.num_points as usize {
            self.point_x[i] = -self.point_x[i];
            self.point_z[i] = -self.point_z[i];
        }
        self.bounding_calc = 0;
    }

    // @ObfuscatedName("fo.aa()V") — ModelLit.rotate270.
    pub fn rotate270(&mut self) {
        for i in 0..self.num_points as usize {
            let temp = self.point_z[i];
            self.point_z[i] = self.point_x[i];
            self.point_x[i] = -temp;
        }
        self.bounding_calc = 0;
    }

    // @ObfuscatedName("fo.as(I)V") — ModelLit.rotateXAxis. Verbatim
    // port of ModelLit.java:763-772 — rotates the Y/Z plane (around
    // the X axis); matches Java's naming convention.
    pub fn rotate_x_axis(&mut self, theta: i32) {
        let sin = crate::dash3d::pix3d::sin_table();
        let cos = crate::dash3d::pix3d::cos_table();
        let s = sin[theta as usize];
        let c = cos[theta as usize];
        for i in 0..self.num_points as usize {
            let new_y = (self.point_y[i] * c - self.point_z[i] * s) >> 16;
            self.point_z[i] = (self.point_z[i] * c + self.point_y[i] * s) >> 16;
            self.point_y[i] = new_y;
        }
        self.bounding_calc = 0;
    }

    // @ObfuscatedName("fo.am(III)V") — ModelLit.translate.
    pub fn translate(&mut self, dx: i32, dy: i32, dz: i32) {
        for i in 0..self.num_points as usize {
            self.point_x[i] += dx;
            self.point_y[i] += dy;
            self.point_z[i] += dz;
        }
        self.bounding_calc = 0;
    }

    // @ObfuscatedName("fo.ap(III)V") — ModelLit.resize.
    // Resize factors are in 1/128ths; LocType resizex/y/z are 128 = 100%.
    pub fn resize(&mut self, sx: i32, sy: i32, sz: i32) {
        for i in 0..self.num_points as usize {
            self.point_x[i] = self.point_x[i] * sx / 128;
            self.point_y[i] = self.point_y[i] * sy / 128;
            self.point_z[i] = self.point_z[i] * sz / 128;
        }
        self.bounding_calc = 0;
    }

    // @ObfuscatedName("fo.y(Z)Lfo;") — ModelLit.copyForAnim.
    //
    // Returns a fresh clone of this model that the animator can
    // mutate without touching the cached canonical instance. Java
    // reuses scratch buffers (tempModel/tempFTran) for perf; we just
    // clone. `copy_alpha = false` means the new model gets a fresh
    // zero alpha vector so animate2 type-5 (alpha animation) can
    // write fresh values.
    pub fn copy_for_anim(&self, copy_alpha: bool) -> ModelLit {
        let mut copy = self.clone();
        if !copy_alpha {
            copy.face_alpha = Some(vec![0i8; self.num_faces as usize]);
        }
        copy.bounding_calc = 0;
        copy
    }

    // @ObfuscatedName("fo.t(Z)Lfo;") — ModelLit.copyForAnim2.
    // Functionally identical to copyForAnim in Rust (the Java
    // distinction is just which static scratch buffer to reuse).
    pub fn copy_for_anim2(&self, copy_alpha: bool) -> ModelLit {
        self.copy_for_anim(copy_alpha)
    }

    // @ObfuscatedName("fo.k()V") — ModelLit.calcBoundingCylinder.
    // Verbatim port of ModelLit.java:483-509.
    pub fn calc_bounding_cylinder(&mut self) {
        if self.bounding_calc == 1 { return; }
        self.bounding_calc = 1;
        self.min_y = 0;
        self.max_y = 0;
        self.radius = 0;
        for i in 0..self.num_points as usize {
            let px = self.point_x[i];
            let py = self.point_y[i];
            let pz = self.point_z[i];
            if -py > self.min_y { self.min_y = -py; }
            if py > self.max_y { self.max_y = py; }
            let r2 = px * px + pz * pz;
            if r2 > self.radius { self.radius = r2; }
        }
        self.radius = ((self.radius as f64).sqrt() + 0.99) as i32;
        self.min_depth =
            (((self.min_y * self.min_y + self.radius * self.radius) as f64).sqrt() + 0.99) as i32;
        self.max_depth = self.min_depth
            + (((self.radius * self.radius + self.max_y * self.max_y) as f64).sqrt() + 0.99) as i32;
    }

    // @ObfuscatedName("fo.b([[IIIIZI)Lfo;") — ModelLit.hillSkew.
    // Verbatim port of ModelLit.java:330-407 (copy=true form). Bends
    // the model's vertices to follow the heightmap under its
    // footprint. Returns None when the terrain is flat under the
    // model or the footprint leaves the map — Java returns `this`
    // unchanged in those cases, so the caller keeps its original.
    pub fn hill_skew(&self, groundh: &[Vec<i32>], x: i32, y: i32, z: i32,
                     blend: i32) -> Option<ModelLit> {
        // calcBoundingCylinder values (computed fresh when the cache
        // isn't populated — the model may be behind a shared Arc).
        let (radius, b_min_y) = if self.bounding_calc == 1 {
            (self.radius, self.min_y)
        } else {
            let mut min_y = 0i32;
            let mut r2max = 0i32;
            for i in 0..self.num_points as usize {
                let px = self.point_x[i];
                let py = self.point_y[i];
                let pz = self.point_z[i];
                if -py > min_y { min_y = -py; }
                let r2 = px * px + pz * pz;
                if r2 > r2max { r2max = r2; }
            }
            (((r2max as f64).sqrt() + 0.99) as i32, min_y)
        };
        let var7 = x - radius;
        let var8 = radius + x;
        let var9 = z - radius;
        let var10 = radius + z;
        if var7 < 0
            || ((var8 + 128) >> 7) as usize >= groundh.len()
            || var9 < 0
            || ((var10 + 128) >> 7) as usize >= groundh[0].len()
        {
            return None;
        }
        let t11 = (var7 >> 7) as usize;
        let t12 = ((var8 + 127) >> 7) as usize;
        let t13 = (var9 >> 7) as usize;
        let t14 = ((var10 + 127) >> 7) as usize;
        if groundh[t11][t13] == y && groundh[t12][t13] == y
            && groundh[t11][t14] == y && groundh[t12][t14] == y
        {
            return None;
        }
        let mut out = self.clone();
        if blend == 0 {
            for i in 0..self.num_points as usize {
                let wx = self.point_x[i] + x;
                let wz = self.point_z[i] + z;
                let sub_x = wx & 0x7F;
                let sub_z = wz & 0x7F;
                let tx = (wx >> 7) as usize;
                let tz = (wz >> 7) as usize;
                let top = ((128 - sub_x) * groundh[tx][tz] + groundh[tx + 1][tz] * sub_x) >> 7;
                let bot = ((128 - sub_x) * groundh[tx][tz + 1] + groundh[tx + 1][tz + 1] * sub_x) >> 7;
                let h = ((128 - sub_z) * top + sub_z * bot) >> 7;
                out.point_y[i] = self.point_y[i] + h - y;
            }
        } else if b_min_y != 0 {
            for i in 0..self.num_points as usize {
                let var27 = (-self.point_y[i] << 16) / b_min_y;
                if var27 < blend {
                    let wx = self.point_x[i] + x;
                    let wz = self.point_z[i] + z;
                    let sub_x = wx & 0x7F;
                    let sub_z = wz & 0x7F;
                    let tx = (wx >> 7) as usize;
                    let tz = (wz >> 7) as usize;
                    let top = ((128 - sub_x) * groundh[tx][tz] + groundh[tx + 1][tz] * sub_x) >> 7;
                    let bot = ((128 - sub_x) * groundh[tx][tz + 1] + groundh[tx + 1][tz + 1] * sub_x) >> 7;
                    let h = ((128 - sub_z) * top + sub_z * bot) >> 7;
                    out.point_y[i] = (h - y) * (blend - var27) / blend + self.point_y[i];
                }
            }
        }
        out.bounding_calc = 0;
        Some(out)
    }

    // @ObfuscatedName("fo.o()V") — ModelLit.calcAABB.
    pub fn calc_aabb(&mut self) {
        if self.bounding_calc == 2 { return; }
        self.bounding_calc = 2;
        self.radius = 0;
        for i in 0..self.num_points as usize {
            let px = self.point_x[i];
            let py = self.point_y[i];
            let pz = self.point_z[i];
            let r2 = py * py + px * px + pz * pz;
            if r2 > self.radius { self.radius = r2; }
        }
        self.radius = ((self.radius as f64).sqrt() + 0.99) as i32;
        self.min_depth = self.radius;
        self.max_depth = self.radius + self.radius;
    }

    // @ObfuscatedName("fo.a()I") — ModelLit.getRadiusCylinder.
    pub fn get_radius_cylinder(&mut self) -> i32 {
        self.calc_bounding_cylinder();
        self.radius
    }

    // @ObfuscatedName("fo.h(Lfr;I)V") — ModelLit.animate. Applies a
    // single keyframe's bone transforms to the model. `frame` is the
    // file id of the AnimFrame within the AnimFrameSet.
    pub fn animate(&mut self, fs: &crate::dash3d::anim_frame_set::AnimFrameSet, frame: i32) {
        if self.label_vertices.is_none() || frame == -1 { return; }
        let af = match fs.list.get(&frame) {
            Some(a) => a.clone(),
            None => return,
        };
        let base = &af.base;
        let mut origin = (0i32, 0i32, 0i32);
        for i in 0..af.size as usize {
            let bone_idx = af.ti[i];
            let t = base.r#type.get(bone_idx as usize).copied().unwrap_or(0);
            let labels = &base.labels.get(bone_idx as usize).cloned().unwrap_or_default();
            self.animate2(t, labels, af.tx[i], af.ty[i], af.tz[i], &mut origin);
        }
    }

    // @ObfuscatedName("fo.p(I[IIII)V") — ModelLit.animate2.
    //
    // Bone-level transformation applied to the vertices (or faces, for
    // type 5) that this bone owns. `origin` is the shared rotation /
    // scale anchor accumulated by preceding type-0 bones (Java's
    // static oX/oY/oZ).
    pub fn animate2(&mut self, ty: i32, labels: &[i32], arg2: i32, arg3: i32, arg4: i32, origin: &mut (i32, i32, i32)) {
        let label_vertices = match &self.label_vertices {
            Some(v) => v,
            None => return,
        };
        if ty == 0 {
            // Set origin to the centroid of all vertices the labelled
            // bones own, plus the per-frame offset.
            let mut count = 0i32;
            let (mut ox, mut oy, mut oz) = (0i32, 0i32, 0i32);
            for &lab in labels {
                if (lab as usize) < label_vertices.len() {
                    let verts = &label_vertices[lab as usize];
                    for &v in verts {
                        let vu = v as usize;
                        ox += self.point_x[vu];
                        oy += self.point_y[vu];
                        oz += self.point_z[vu];
                        count += 1;
                    }
                }
            }
            if count > 0 {
                *origin = (ox / count + arg2, oy / count + arg3, oz / count + arg4);
            } else {
                *origin = (arg2, arg3, arg4);
            }
        } else if ty == 1 {
            // Translate.
            for &lab in labels {
                if (lab as usize) < label_vertices.len() {
                    let verts = label_vertices[lab as usize].clone();
                    for &v in &verts {
                        let vu = v as usize;
                        self.point_x[vu] += arg2;
                        self.point_y[vu] += arg3;
                        self.point_z[vu] += arg4;
                    }
                }
            }
        } else if ty == 2 {
            // Rotate around origin. arg2/arg3/arg4 are (X-axis,
            // Y-axis, Z-axis) rotations in the bone's units * 8 → 2048
            // step convention.
            let sin_t = pix3d::sin_table();
            let cos_t = pix3d::cos_table();
            for &lab in labels {
                if (lab as usize) < label_vertices.len() {
                    let verts = label_vertices[lab as usize].clone();
                    for &v in &verts {
                        let vu = v as usize;
                        let mut px = self.point_x[vu] - origin.0;
                        let mut py = self.point_y[vu] - origin.1;
                        let mut pz = self.point_z[vu] - origin.2;
                        let rx_idx = ((arg2 & 0xFF) * 8) as usize & 0x7FF;
                        let ry_idx = ((arg3 & 0xFF) * 8) as usize & 0x7FF;
                        let rz_idx = ((arg4 & 0xFF) * 8) as usize & 0x7FF;
                        // Z rotation (around Z axis).
                        if rz_idx != 0 {
                            let s = sin_t[rz_idx];
                            let c = cos_t[rz_idx];
                            let nx = (py * s + px * c) >> 16;
                            py = (py * c - px * s) >> 16;
                            px = nx;
                        }
                        // X rotation (around X axis).
                        if rx_idx != 0 {
                            let s = sin_t[rx_idx];
                            let c = cos_t[rx_idx];
                            let ny = (py * c - pz * s) >> 16;
                            pz = (pz * c + py * s) >> 16;
                            py = ny;
                        }
                        // Y rotation (around Y axis).
                        if ry_idx != 0 {
                            let s = sin_t[ry_idx];
                            let c = cos_t[ry_idx];
                            let nz = (pz * s + px * c) >> 16;
                            pz = (pz * c - px * s) >> 16;
                            px = nz;
                        }
                        self.point_x[vu] = px + origin.0;
                        self.point_y[vu] = py + origin.1;
                        self.point_z[vu] = pz + origin.2;
                    }
                }
            }
        } else if ty == 3 {
            // Scale around origin (arg2/3/4 are scale factors in
            // 128-baseline, e.g., 256 = 2× magnification).
            for &lab in labels {
                if (lab as usize) < label_vertices.len() {
                    let verts = label_vertices[lab as usize].clone();
                    for &v in &verts {
                        let vu = v as usize;
                        let px = self.point_x[vu] - origin.0;
                        let py = self.point_y[vu] - origin.1;
                        let pz = self.point_z[vu] - origin.2;
                        self.point_x[vu] = (px * arg2 / 128) + origin.0;
                        self.point_y[vu] = (py * arg3 / 128) + origin.1;
                        self.point_z[vu] = (pz * arg4 / 128) + origin.2;
                    }
                }
            }
        } else if ty == 5 {
            // Animate face alpha. Only applies when label_faces and
            // face_alpha exist.
            let label_faces = match &self.label_faces {
                Some(v) => v.clone(),
                None => return,
            };
            let Some(alphas) = self.face_alpha.as_mut() else { return };
            for &lab in labels {
                if (lab as usize) < label_faces.len() {
                    for &f in &label_faces[lab as usize] {
                        let fu = f as usize;
                        let mut alpha = (alphas[fu] as i32 & 0xFF) + arg2 * 8;
                        if alpha < 0 { alpha = 0; }
                        else if alpha > 255 { alpha = 255; }
                        alphas[fu] = alpha as i8;
                    }
                }
            }
        }
    }

    // Back-compat alias: the old name kept since several call sites
    // still reference it. Routes to the proper smooth-Gouraud path.
    pub fn from_unlit_flat(src: &ModelUnlit, ambient: i32, contrast: i32, lx: i32, ly: i32, lz: i32) -> Self {
        let mut owned = ModelUnlit {
            num_points: src.num_points,
            point_x: src.point_x.clone(),
            point_y: src.point_y.clone(),
            point_z: src.point_z.clone(),
            num_faces: src.num_faces,
            face_vertex_a: src.face_vertex_a.clone(),
            face_vertex_b: src.face_vertex_b.clone(),
            face_vertex_c: src.face_vertex_c.clone(),
            face_render_type: src.face_render_type.clone(),
            face_priority: src.face_priority.clone(),
            face_alpha: src.face_alpha.clone(),
            face_texture_axis: src.face_texture_axis.clone(),
            face_colour: src.face_colour.clone(),
            face_texture_id: src.face_texture_id.clone(),
            priority: src.priority,
            num_t: src.num_t,
            texture_render_type: src.texture_render_type.clone(),
            face_texture_p: src.face_texture_p.clone(),
            face_texture_m: src.face_texture_m.clone(),
            face_texture_n: src.face_texture_n.clone(),
            texture_scale_x: src.texture_scale_x.clone(),
            texture_scale_y: src.texture_scale_y.clone(),
            texture_scale_z: src.texture_scale_z.clone(),
            texture_rotation: src.texture_rotation.clone(),
            texture_speed: src.texture_speed.clone(),
            texture_direction: src.texture_direction.clone(),
            texture_translation: src.texture_translation.clone(),
            vertex_label: src.vertex_label.clone(),
            face_label: src.face_label.clone(),
            label_vertices: src.label_vertices.clone(),
            label_faces: src.label_faces.clone(),
            ambient: src.ambient,
            contrast: src.contrast,
            point_normal: None,
            face_normal: None,
            shared_point_normal: src.shared_point_normal.clone(),
            bounds_calculated: src.bounds_calculated,
            min_x: src.min_x, max_x: src.max_x,
            min_y: src.min_y, max_y: src.max_y,
            min_z: src.min_z, max_z: src.max_z,
        };
        Self::light(&mut owned, ambient, contrast, lx, ly, lz)
    }

    // @ObfuscatedName("fo.ax(IIIIIII)V") — ModelLit.objRender.
    //
    // `model_yaw` is the 0..2047 rotation around Y applied before the
    // camera transform (Java's objRender arg1). `skew_y_override` is
    // the per-vertex Y replacement used when LocType.skewType >= 0 and
    // hillSkew has been applied by the caller — None when no skew.
    pub fn obj_render(
        &self,
        tx: i32, ty: i32, tz: i32,
        model_yaw: i32,
        pitch: i32, yaw: i32, zoom: i32,
    ) {
        self.obj_render_with_skew(tx, ty, tz, model_yaw, pitch, yaw, zoom, None);
    }
    pub fn obj_render_with_skew(
        &self,
        tx: i32, ty: i32, tz: i32,
        model_yaw: i32,
        pitch: i32, yaw: i32, zoom: i32,
        skew_y_override: Option<&[i32]>,
    ) {
        let palette = pix3d::colour_table();
        let sin_t = pix3d::sin_table();
        let cos_t = pix3d::cos_table();
        let myaw = (model_yaw & 0x7FF) as usize;
        let s = sin_t[myaw];
        let c = cos_t[myaw];
        let (origin_x, origin_y) = pix3d::origin();
        let mut screen_x = vec![i32::MIN; self.num_points as usize];
        let mut screen_y = vec![i32::MIN; self.num_points as usize];
        let mut view_z = vec![i32::MIN; self.num_points as usize];
        // View-space coords mirror Java's vertexViewSpaceX/Y/Z — post
        // yaw + pitch, pre-zoom-divide. textureTriangleAffine needs
        // these directly for the (P, M, N) anchor coords.
        let mut view_x = vec![0i32; self.num_points as usize];
        let mut view_y = vec![0i32; self.num_points as usize];
        for p in 0..self.num_points as usize {
            let lx = self.point_x[p];
            let ly = skew_y_override.map_or(self.point_y[p], |o| o[p]);
            let lz = self.point_z[p];
            let rx = (lz * s + lx * c) >> 16;
            let rz = (lz * c - lx * s) >> 16;
            let wx = rx + tx;
            let wy = ly + ty;
            let wz = rz + tz;
            let (sx, sy, vx, vy, vz) = pix3d::project_with_view_space(
                wx, wy, wz, pitch, yaw, zoom, origin_x, origin_y);
            screen_x[p] = sx;
            screen_y[p] = sy;
            view_x[p] = vx;
            view_y[p] = vy;
            view_z[p] = vz;
        }
        self.render_faces(&screen_x, &screen_y, &view_x, &view_y, &view_z,
                          zoom, origin_x, origin_y, false, 0);
    }

    // @ObfuscatedName("fo.az(ZZI)V") — ModelLit.render2: the shared
    // face-cull / depth-sort / dispatch stage behind both objRender
    // and worldRender. `picking` + `typecode` mirror Java's render2
    // args — when picking is set, the first face whose screen bbox
    // contains the mouse pushes `typecode` into MOUSE_PICK.picked
    // (Java: pickedEntityTypecode[pickedCount++]) and clears the flag.
    fn render_faces(
        &self,
        screen_x: &[i32], screen_y: &[i32],
        view_x: &[i32], view_y: &[i32], view_z: &[i32],
        zoom: i32, origin_x: i32, origin_y: i32,
        picking: bool, typecode: i32,
    ) {
        let palette = pix3d::colour_table();
        let mut picking = picking;
        let (pick_x, pick_y) = if picking {
            let p = MOUSE_PICK.lock().unwrap();
            (p.mouse_x, p.mouse_y)
        } else {
            (0, 0)
        };
        // Sort faces back-to-front in view space so closer faces paint
        // last. Higher face priority renders last (Java sorts by
        // priority bucket first, then depth within bucket).
        let global_priority = self.priority as i32;
        // Java ModelLit.render2 line 1113: front-face cull via
        // screen-space signed area `(yC - yB)*(xA - xB) - (yA - yB)*(xC - xB)`.
        // Triangles with cross <= 0 are back-facing and skipped.
        // faceColourC == -2 marks "skip" in Java (line 1064).
        // Java's near plane is z >= 50. Faces with any vertex below
        // route through render3ZClip (interpolates edges to the near
        // plane). Faces ENTIRELY below near are dropped.
        const NEAR_PLANE: i32 = 50;
        let face_order: Vec<(i32, i32, usize, bool)> = (0..self.num_faces as usize)
            .filter_map(|f| {
                if self.face_colour_c[f] == -2 { return None; }
                let a = self.face_vertex_a[f] as usize;
                let b = self.face_vertex_b[f] as usize;
                let c = self.face_vertex_c[f] as usize;
                let za = view_z[a]; let zb = view_z[b]; let zc = view_z[c];
                // Java render2 line 1077-1106: clipped-vertex faces use
                // a view-space normal cull (`vzB*Nz + vxB*Nx + vyB*Ny > 0`);
                // unclipped faces use the screen-space cross product
                // (`(yC-yB)*(xA-xB) - (yA-yB)*(xC-xB) > 0`).
                let any_clipped = za < NEAR_PLANE || zb < NEAR_PLANE || zc < NEAR_PLANE;
                let all_clipped = za < NEAR_PLANE && zb < NEAR_PLANE && zc < NEAR_PLANE;
                if all_clipped { return None; }
                if !any_clipped {
                    let xa = screen_x[a]; let xb = screen_x[b]; let xc = screen_x[c];
                    let ya = screen_y[a]; let yb = screen_y[b]; let yc = screen_y[c];
                    // Java render2 line 1108-1111: the mouse pick test
                    // runs on unclipped faces BEFORE the winding cull —
                    // back-facing triangles still count as a hover hit.
                    if picking && Self::is_mouse_roughly_inside_triangle(
                        pick_x, pick_y, ya, yb, yc, xa, xb, xc)
                    {
                        MOUSE_PICK.lock().unwrap().picked.push(typecode);
                        picking = false;
                    }
                    if (yc - yb) * (xa - xb) - (ya - yb) * (xc - xb) <= 0 { return None; }
                } else {
                    // View-space normal cull (Java line 1097-1101).
                    let vxa = view_x[a]; let vxb = view_x[b]; let vxc = view_x[c];
                    let vya = view_y[a]; let vyb = view_y[b]; let vyc = view_y[c];
                    let n_x = (vya - vyb) * (zc - zb) - (vyc - vyb) * (za - zb);
                    let n_y = (vxc - vxb) * (za - zb) - (vxa - vxb) * (zc - zb);
                    let n_z = (vxa - vxb) * (vyc - vyb) - (vxc - vxb) * (vya - vyb);
                    if zb * n_z + vxb * n_x + vyb * n_y <= 0 { return None; }
                }
                let prio = self.face_priority.as_ref()
                    .map_or(global_priority, |v| v[f] as i32);
                let depth = (view_z[a] + view_z[b] + view_z[c]) / 3;
                Some((prio, depth, f, any_clipped))
            })
            .collect();
        let mut face_order = face_order;
        // Build the render queue.
        //
        // - When face_priority is null: simple back-to-front by depth
        //   (Java ModelLit.render2 line 1128-1140).
        // - When face_priority is set: Java ModelLit.render2 line
        //   1142-1268 — bucket by priority 0..11, then walk priorities
        //   0..9 in order, interleaving priority-10/11 faces that
        //   exceed the mean depth of their threshold buckets:
        //     before priority 0 → 10/11 with depth > avg(prio 1+2)
        //     before priority 3 → 10/11 with depth > avg(prio 3+4)
        //     before priority 5 → 10/11 with depth > avg(prio 6+8)
        //   then render priorities 0..9, then any remaining 10/11.
        //   This is what lets near-camera hair/face faces punch
        //   through earlier body priorities on player and NPC models.
        if self.face_priority.is_some() {
            face_order.sort_by(|x, y| y.1.cmp(&x.1));
            let mut buckets: [Vec<(i32, usize, bool)>; 12] =
                Default::default();
            let mut depth_sum = [0i64; 12];
            for &(prio, depth, f, near_clipped) in face_order.iter() {
                let p = (prio.clamp(0, 11)) as usize;
                buckets[p].push((depth, f, near_clipped));
                if p < 10 { depth_sum[p] += depth as i64; }
            }
            // Pre-compute the three threshold means up front so the
            // borrow on `buckets` releases before we std::mem::take
            // the priority-10/11 queues below.
            let mean_of = |a: usize, b: usize| -> i32 {
                let n = (buckets[a].len() + buckets[b].len()) as i64;
                if n == 0 { 0 } else { ((depth_sum[a] + depth_sum[b]) / n) as i32 }
            };
            let mean_1_2 = mean_of(1, 2);
            let mean_3_4 = mean_of(3, 4);
            let mean_6_8 = mean_of(6, 8);
            let threshold_for = |bucket: usize| -> i32 {
                match bucket {
                    0 => mean_1_2,
                    3 => mean_3_4,
                    5 => mean_6_8,
                    _ => i32::MAX,
                }
            };
            // Priority 10 and 11 are kept as SEPARATE FIFOs (Java
            // line 1186-1267: var49 starts at tmpPriorityFaces[10],
            // switches to tmpPriorityFaces[11] only when 10 is fully
            // drained). All priority-10 faces draw before any
            // priority-11 face — order matters for layered detail
            // like hair vs eyebrows on player models.
            //
            // Within each queue, faces are already in back-to-front
            // order because we sorted face_order by descending depth
            // before bucketing, so the push order preserves depth
            // descending within each bucket.
            //
            // The threshold gates whether to render from the current
            // priority queue BEFORE draining the corresponding bucket
            // — faces with depth > avg(threshold_pair) punch through
            // earlier in the pipeline.
            let mut q10_idx = 0;
            let mut q11_idx = 0;
            // Mutable callback to pop the next available priority-10/11
            // face — switches from 10 → 11 when 10 is drained.
            let pop_hi = |q10: &Vec<(i32, usize, bool)>,
                          q11: &Vec<(i32, usize, bool)>,
                          q10_idx: &mut usize,
                          q11_idx: &mut usize| -> Option<(i32, usize, bool)> {
                if *q10_idx < q10.len() {
                    let v = q10[*q10_idx];
                    *q10_idx += 1;
                    Some(v)
                } else if *q11_idx < q11.len() {
                    let v = q11[*q11_idx];
                    *q11_idx += 1;
                    Some(v)
                } else {
                    None
                }
            };
            let peek_hi_depth = |q10: &Vec<(i32, usize, bool)>,
                                 q11: &Vec<(i32, usize, bool)>,
                                 q10_idx: usize,
                                 q11_idx: usize| -> i32 {
                if q10_idx < q10.len() { q10[q10_idx].0 }
                else if q11_idx < q11.len() { q11[q11_idx].0 }
                else { i32::MIN }
            };
            let q10 = std::mem::take(&mut buckets[10]);
            let q11 = std::mem::take(&mut buckets[11]);
            let mut render_queue: Vec<(usize, bool)> = Vec::with_capacity(self.num_faces as usize);
            for bucket in 0..10 {
                let thr = threshold_for(bucket);
                while peek_hi_depth(&q10, &q11, q10_idx, q11_idx) > thr {
                    if let Some((_, f, near_clipped)) = pop_hi(&q10, &q11, &mut q10_idx, &mut q11_idx) {
                        render_queue.push((f, near_clipped));
                    } else { break; }
                }
                for &(_, f, near_clipped) in buckets[bucket].iter() {
                    render_queue.push((f, near_clipped));
                }
            }
            while let Some((_, f, near_clipped)) = pop_hi(&q10, &q11, &mut q10_idx, &mut q11_idx) {
                render_queue.push((f, near_clipped));
            }
            face_order = render_queue.into_iter()
                .map(|(f, near_clipped)| (0, 0, f, near_clipped))
                .collect();
        } else {
            face_order.sort_by(|x, y| y.1.cmp(&x.1));
        }
        for (_, _, f, near_clipped) in face_order.into_iter() {
            let a = self.face_vertex_a[f] as usize;
            let b = self.face_vertex_b[f] as usize;
            let c = self.face_vertex_c[f] as usize;
            // Java ModelLit.draw line 1286-1290: Pix3D.trans = faceAlpha[face] & 0xFF.
            let trans = self.face_alpha.as_ref().map_or(0i32, |v| v[f] as i32 & 0xFF);
            pix3d::set_trans(trans);
            // Near-plane clipping path: interpolate any vertex with
            // view_z < NEAR_PLANE to the plane. Generates either 3
            // (triangle) or 4 (quad → 2 triangles) clipped vertices.
            // We only port the gouraud / flat path here — textured
            // faces fall back to dropping if any vertex is near (Java
            // does interpolate UV too, but that's another ~80 lines).
            if near_clipped {
                let tex_id = self.face_texture_id.as_ref().map_or(-1i32, |v| v[f] as i32);
                let face_colour_c = self.face_colour_c[f];
                let (ca_idx, cb_idx, cc_idx) =
                    (self.face_colour_a[f], self.face_colour_b[f], face_colour_c);
                // Interpolation helper: produce (sx, sy, c_idx) at the
                // intersection between the "behind" vertex and the
                // "in front" vertex along the near plane (z = NEAR_PLANE).
                let interp = |behind: usize, infront: usize,
                              behind_col: i32, infront_col: i32| -> (i32, i32, i32) {
                    let z_n = view_z[behind];
                    let z_f = view_z[infront];
                    let denom = (z_f - z_n).max(1);
                    let t = ((NEAR_PLANE - z_n) << 16) / denom;
                    let nx = (((view_x[infront] - view_x[behind]) * t) >> 16) + view_x[behind];
                    let ny = (((view_y[infront] - view_y[behind]) * t) >> 16) + view_y[behind];
                    let sx = (nx * zoom) / NEAR_PLANE + origin_x;
                    let sy = (ny * zoom) / NEAR_PLANE + origin_y;
                    let col = (((infront_col - behind_col) * t) >> 16) + behind_col;
                    (sx, sy, col)
                };
                // Per Java render3ZClip 1340-1407: build the up-to-4
                // clipped vertex list.
                let mut cx = [0i32; 4];
                let mut cy = [0i32; 4];
                let mut cc = [0i32; 4];
                let mut elements = 0usize;
                if view_z[a] >= NEAR_PLANE {
                    cx[elements] = screen_x[a];
                    cy[elements] = screen_y[a];
                    cc[elements] = ca_idx;
                    elements += 1;
                } else {
                    if view_z[c] >= NEAR_PLANE {
                        let (sx, sy, col) = interp(a, c, ca_idx, cc_idx);
                        cx[elements] = sx; cy[elements] = sy; cc[elements] = col;
                        elements += 1;
                    }
                    if view_z[b] >= NEAR_PLANE {
                        let (sx, sy, col) = interp(a, b, ca_idx, cb_idx);
                        cx[elements] = sx; cy[elements] = sy; cc[elements] = col;
                        elements += 1;
                    }
                }
                if view_z[b] >= NEAR_PLANE {
                    cx[elements] = screen_x[b];
                    cy[elements] = screen_y[b];
                    cc[elements] = cb_idx;
                    elements += 1;
                } else {
                    if view_z[a] >= NEAR_PLANE {
                        let (sx, sy, col) = interp(b, a, cb_idx, ca_idx);
                        cx[elements] = sx; cy[elements] = sy; cc[elements] = col;
                        elements += 1;
                    }
                    if view_z[c] >= NEAR_PLANE {
                        let (sx, sy, col) = interp(b, c, cb_idx, cc_idx);
                        cx[elements] = sx; cy[elements] = sy; cc[elements] = col;
                        elements += 1;
                    }
                }
                if view_z[c] >= NEAR_PLANE {
                    cx[elements] = screen_x[c];
                    cy[elements] = screen_y[c];
                    cc[elements] = cc_idx;
                    elements += 1;
                } else {
                    if view_z[b] >= NEAR_PLANE {
                        let (sx, sy, col) = interp(c, b, cc_idx, cb_idx);
                        cx[elements] = sx; cy[elements] = sy; cc[elements] = col;
                        elements += 1;
                    }
                    if view_z[a] >= NEAR_PLANE {
                        let (sx, sy, col) = interp(c, a, cc_idx, ca_idx);
                        cx[elements] = sx; cy[elements] = sy; cc[elements] = col;
                        elements += 1;
                    }
                }
                // Textured clipped path — Java's render3ZClip keeps
                // (P, M, N) view-space coords UNCHANGED (lines 1422-1434);
                // only screen X/Y interpolate to the near plane. The
                // texture mapping math relies on the original anchor
                // triangle to compute per-pixel UV.
                if tex_id >= 0 {
                    let axis = self.face_texture_axis.as_ref().map_or(-1i32, |v| v[f] as i32);
                    let (t_a, t_b, t_c) = if axis < 0 {
                        (a, b, c)
                    } else {
                        let ai = axis as usize;
                        let p = self.face_texture_p.as_ref().map_or(a as i16, |v| v[ai]) as usize;
                        let m = self.face_texture_m.as_ref().map_or(b as i16, |v| v[ai]) as usize;
                        let n = self.face_texture_n.as_ref().map_or(c as i16, |v| v[ai]) as usize;
                        (p, m, n)
                    };
                    let render_tex_tri = |i0: usize, i1: usize, i2: usize| {
                        pix3d::set_hclip(true);
                        pix3d::texture_triangle_affine(
                            cy[i0], cy[i1], cy[i2],
                            cx[i0], cx[i1], cx[i2],
                            cc[i0], cc[i1], cc[i2],
                            view_x[t_a], view_x[t_b], view_x[t_c],
                            view_y[t_a], view_y[t_b], view_y[t_c],
                            view_z[t_a], view_z[t_b], view_z[t_c],
                            tex_id,
                        );
                    };
                    if elements == 3 {
                        render_tex_tri(0, 1, 2);
                    } else if elements == 4 {
                        render_tex_tri(0, 1, 2);
                        render_tex_tri(0, 2, 3);
                    }
                    continue;
                }
                let render_tri = |i0: usize, i1: usize, i2: usize| {
                    if cc_idx == -1 {
                        pix3d::fill_triangle(
                            cx[i0], cy[i0], cx[i1], cy[i1], cx[i2], cy[i2],
                            palette[(cc[i0] as usize) & 0xFFFF],
                        );
                    } else {
                        // Pass raw HSL indices — gouraud_triangle
                        // interpolates in palette space and looks up
                        // colourTable per pixel (matches Java's
                        // gouraudRaster line 733).
                        pix3d::gouraud_triangle(
                            cx[i0], cy[i0], cc[i0],
                            cx[i1], cy[i1], cc[i1],
                            cx[i2], cy[i2], cc[i2],
                        );
                    }
                };
                if elements == 3 {
                    render_tri(0, 1, 2);
                } else if elements == 4 {
                    render_tri(0, 1, 2);
                    render_tri(0, 2, 3);
                }
                continue;
            }
            let tex_id = self.face_texture_id.as_ref().map_or(-1i32, |v| v[f] as i32);
            if tex_id >= 0 {
                // Pick the texture mapping triangle (P, M, N). Java's
                // ModelLit.draw consults face_texture_axis: if it's -1
                // / null the face is its own anchor (P=a, M=b, N=c);
                // otherwise look up face_texture_p/m/n[axis] for the
                // explicit anchor vertices.
                let axis = self.face_texture_axis.as_ref().map_or(-1i32, |v| v[f] as i32);
                let (t_a, t_b, t_c) = if axis < 0 {
                    (a, b, c)
                } else {
                    let ai = axis as usize;
                    let p = self.face_texture_p.as_ref().map_or(a as i16, |v| v[ai]) as usize;
                    let m = self.face_texture_m.as_ref().map_or(b as i16, |v| v[ai]) as usize;
                    let n = self.face_texture_n.as_ref().map_or(c as i16, |v| v[ai]) as usize;
                    (p, m, n)
                };
                // Java's ModelLit.draw line 1284: Pix3D.hclip is set
                // per face from faceClippedX[face] before the call.
                pix3d::set_hclip(pix3d::face_x_clipped(screen_x[a], screen_x[b], screen_x[c]));
                // Java's faceColourC[face] == -1 branch (ModelLit.java:
                // 1306-1310): type-1 flat-shaded textured faces store
                // only colA; B/C retain their default of 0. The
                // rasterizer needs a uniform tint, so Java passes
                // (colA, colA, colA). Without this check, the -1
                // sentinel goes through as a colour and the texture
                // gets a wild gradient.
                let (col_a, col_b, col_c) = if self.face_colour_c[f] == -1 {
                    let a_ = self.face_colour_a[f];
                    (a_, a_, a_)
                } else {
                    (self.face_colour_a[f], self.face_colour_b[f], self.face_colour_c[f])
                };
                pix3d::texture_triangle_affine(
                    screen_y[a], screen_y[b], screen_y[c],
                    screen_x[a], screen_x[b], screen_x[c],
                    col_a, col_b, col_c,
                    view_x[t_a], view_x[t_b], view_x[t_c],
                    view_y[t_a], view_y[t_b], view_y[t_c],
                    view_z[t_a], view_z[t_b], view_z[t_c],
                    tex_id,
                );
                continue;
            }
            // Java ModelLit.draw line 1311-1315: face_colour_c == -1
            // marks a "flat" face — render with face_colour_a only,
            // no gradient. Otherwise it's a smooth-shaded gouraud face.
            if self.face_colour_c[f] == -1 {
                pix3d::fill_triangle(
                    screen_x[a], screen_y[a],
                    screen_x[b], screen_y[b],
                    screen_x[c], screen_y[c],
                    palette[(self.face_colour_a[f] as usize) & 0xFFFF],
                );
            } else {
                // face_colour_a/b/c are HSL palette indices. Java's
                // gouraudTriangle interpolates the indices and looks up
                // colourTable per pixel; we pass them through raw.
                let ia = self.face_colour_a[f];
                let ib = self.face_colour_b[f];
                let ic = self.face_colour_c[f];
                if ia == ib && ib == ic {
                    pix3d::fill_triangle(
                        screen_x[a], screen_y[a],
                        screen_x[b], screen_y[b],
                        screen_x[c], screen_y[c],
                        palette[(ia as usize) & 0xFFFF],
                    );
                } else {
                    pix3d::gouraud_triangle(
                        screen_x[a], screen_y[a], ia,
                        screen_x[b], screen_y[b], ib,
                        screen_x[c], screen_y[c], ic,
                    );
                }
            }
        }
    }

    // @ObfuscatedName("fo.av(IIIIIII)V") — ModelLit.objRender: the
    // orthographic-ish icon/interface render. Euler rotates (Z, X, Y
    // in that order), translates, then pitches around X by `view_x_an`
    // for the final view and projects with the fixed << 9 focal
    // length through the current Pix3D origin. Used by
    // ObjType.getSprite and the type-6 interface model component.
    // Args positionally mirror Java: (xan, yan, zan, view_xan, x, y, z).
    pub fn obj_render_icon(&self, rot_x: i32, rot_y: i32, rot_z: i32,
                           view_x_an: i32, tx: i32, ty: i32, tz: i32) {
        let sin_t = pix3d::sin_table();
        let cos_t = pix3d::cos_table();
        let (origin_x, origin_y) = pix3d::origin();
        let sin_x = sin_t[(rot_x & 0x7FF) as usize];
        let cos_x = cos_t[(rot_x & 0x7FF) as usize];
        let sin_y = sin_t[(rot_y & 0x7FF) as usize];
        let cos_y = cos_t[(rot_y & 0x7FF) as usize];
        let sin_z = sin_t[(rot_z & 0x7FF) as usize];
        let cos_z = cos_t[(rot_z & 0x7FF) as usize];
        let sin_v = sin_t[(view_x_an & 0x7FF) as usize];
        let cos_v = cos_t[(view_x_an & 0x7FF) as usize];
        let n = self.num_points as usize;
        let mut screen_x = vec![i32::MIN; n];
        let mut screen_y = vec![i32::MIN; n];
        let mut view_x = vec![0i32; n];
        let mut view_y = vec![0i32; n];
        let mut view_z = vec![0i32; n];
        for i in 0..n {
            let mut vx = self.point_x[i];
            let mut vy = self.point_y[i];
            let mut vz = self.point_z[i];
            if rot_z != 0 {
                let t = (sin_z * vy + cos_z * vx) >> 16;
                vy = (cos_z * vy - sin_z * vx) >> 16;
                vx = t;
            }
            if rot_x != 0 {
                let t = (cos_x * vy - sin_x * vz) >> 16;
                vz = (sin_x * vy + cos_x * vz) >> 16;
                vy = t;
            }
            if rot_y != 0 {
                let t = (sin_y * vz + cos_y * vx) >> 16;
                vz = (cos_y * vz - sin_y * vx) >> 16;
                vx = t;
            }
            let wx = tx + vx;
            let wy = ty + vy;
            let wz = tz + vz;
            let vy2 = (cos_v * wy - sin_v * wz) >> 16;
            let vz2 = (sin_v * wy + cos_v * wz) >> 16;
            if vz2 >= 50 {
                screen_x[i] = (wx << 9) / vz2 + origin_x;
                screen_y[i] = (vy2 << 9) / vz2 + origin_y;
            }
            view_x[i] = wx;
            view_y[i] = vy2;
            view_z[i] = vz2;
        }
        self.render_faces(&screen_x, &screen_y, &view_x, &view_y, &view_z,
                          512, origin_x, origin_y, false, 0);
    }

    // @ObfuscatedName("fo.ak(IIIIIIII)V") — ModelLit.objRenderOrthog.
    // Verbatim port of ModelLit.java:858-911: identical Euler rotate
    // (Z, X, Y) + translate + view pitch to objRender, but projects
    // orthographically — both screen axes divide by the constant
    // `zoom` instead of the per-vertex view depth. Used by type-6
    // interface model components with the orthog flag.
    pub fn obj_render_icon_orthog(&self, rot_x: i32, rot_y: i32, rot_z: i32,
                                  view_x_an: i32, tx: i32, ty: i32, tz: i32,
                                  zoom: i32) {
        if zoom == 0 { return; }
        let sin_t = pix3d::sin_table();
        let cos_t = pix3d::cos_table();
        let (origin_x, origin_y) = pix3d::origin();
        let sin_x = sin_t[(rot_x & 0x7FF) as usize];
        let cos_x = cos_t[(rot_x & 0x7FF) as usize];
        let sin_y = sin_t[(rot_y & 0x7FF) as usize];
        let cos_y = cos_t[(rot_y & 0x7FF) as usize];
        let sin_z = sin_t[(rot_z & 0x7FF) as usize];
        let cos_z = cos_t[(rot_z & 0x7FF) as usize];
        let sin_v = sin_t[(view_x_an & 0x7FF) as usize];
        let cos_v = cos_t[(view_x_an & 0x7FF) as usize];
        let n = self.num_points as usize;
        let mut screen_x = vec![i32::MIN; n];
        let mut screen_y = vec![i32::MIN; n];
        let mut view_x = vec![0i32; n];
        let mut view_y = vec![0i32; n];
        let mut view_z = vec![0i32; n];
        for i in 0..n {
            let mut vx = self.point_x[i];
            let mut vy = self.point_y[i];
            let mut vz = self.point_z[i];
            if rot_z != 0 {
                let t = (sin_z * vy + cos_z * vx) >> 16;
                vy = (cos_z * vy - sin_z * vx) >> 16;
                vx = t;
            }
            if rot_x != 0 {
                let t = (cos_x * vy - sin_x * vz) >> 16;
                vz = (sin_x * vy + cos_x * vz) >> 16;
                vy = t;
            }
            if rot_y != 0 {
                let t = (sin_y * vz + cos_y * vx) >> 16;
                vz = (cos_y * vz - sin_y * vx) >> 16;
                vx = t;
            }
            let wx = tx + vx;
            let wy = ty + vy;
            let wz = tz + vz;
            let vy2 = (cos_v * wy - sin_v * wz) >> 16;
            let vz2 = (sin_v * wy + cos_v * wz) >> 16;
            screen_x[i] = (wx << 9) / zoom + origin_x;
            screen_y[i] = (vy2 << 9) / zoom + origin_y;
            view_x[i] = wx;
            view_y[i] = vy2;
            view_z[i] = vz2;
        }
        self.render_faces(&screen_x, &screen_y, &view_x, &view_y, &view_z,
                          512, origin_x, origin_y, false, 0);
    }

    // @ObfuscatedName("fo.z(IIIIIIIII)V") — ModelLit.worldRender.
    // Verbatim port of ModelLit.java:915-1050: the scene-render entry
    // used by World.fill for every wall / decor / ground-decor /
    // ground-object / sprite model. Args mirror Java positionally:
    // (modelYaw, sinPitch, cosPitch, sinYaw, cosYaw, relX, relY, relZ,
    // typecode) where rel* are world coords relative to the camera
    // (x - World.cx etc.).
    //
    // Bounding-cylinder cull → screen-band cull → mouse-pick band test
    // → vertex transform (fixed << 9 focal length, z >= 50 near plane)
    // → render2 (render_faces).
    pub fn world_render(
        &self,
        yaw: i32,
        sin_pitch: i32, cos_pitch: i32,
        sin_yaw: i32, cos_yaw: i32,
        rel_x: i32, rel_y: i32, rel_z: i32,
        typecode: i32,
    ) {
        // Java: if (this.boundingCalc != 1) calcBoundingCylinder().
        // The model is behind a shared Arc here, so when the cache
        // isn't populated we compute the cylinder values fresh
        // (same math as calc_bounding_cylinder, no mutation).
        let (radius, b_min_y) = if self.bounding_calc == 1 {
            (self.radius, self.min_y)
        } else {
            let mut min_y = 0i32;
            let mut r2max = 0i32;
            for i in 0..self.num_points as usize {
                let px = self.point_x[i];
                let py = self.point_y[i];
                let pz = self.point_z[i];
                if -py > min_y { min_y = -py; }
                let r2 = px * px + pz * pz;
                if r2 > r2max { r2max = r2; }
            }
            (((r2max as f64).sqrt() + 0.99) as i32, min_y)
        };

        // Java's Pix3D.minX/maxX/minY/maxY are origin-relative clip
        // bounds (minX = -originX, maxX = sizeX - originX); our State
        // stores absolute screen coords, so re-derive.
        let (origin_x, origin_y, j_min_x, j_max_x, j_min_y, j_max_y, far_clip) = {
            let s = pix3d::STATE.lock().unwrap();
            (s.origin_x, s.origin_y,
             s.min_x - s.origin_x, s.max_x - s.origin_x,
             s.min_y - s.origin_y, s.max_y - s.origin_y,
             s.model_far_clip)
        };

        let var10 = (cos_yaw * rel_z - sin_yaw * rel_x) >> 16;
        let var11 = (sin_pitch * rel_y + cos_pitch * var10) >> 16;
        let var12 = (radius * cos_pitch) >> 16;
        let var13 = var11 + var12;
        // Java: `var11 >= 3500` (ModelLit.java:927) — far_clip is 3500
        // plus the camera's extra zoom pull-back so models on rendered
        // tiles don't pop out when zoomed past Java's fixed distance.
        if var13 <= 50 || var11 >= far_clip {
            return;
        }
        let var14 = (sin_yaw * rel_z + cos_yaw * rel_x) >> 16;
        let var15 = (var14 - radius) << 9;
        if var15 / var13 >= j_max_x {
            return;
        }
        let var16 = (radius + var14) << 9;
        if var16 / var13 <= j_min_x {
            return;
        }
        let var17 = (cos_pitch * rel_y - sin_pitch * var10) >> 16;
        let var18 = (radius * sin_pitch) >> 16;
        let var19 = (var17 + var18) << 9;
        if var19 / var13 <= j_min_y {
            return;
        }
        let var20 = ((b_min_y * cos_pitch) >> 16) + var18;
        let var21 = (var17 - var20) << 9;
        if var21 / var13 >= j_max_y {
            return;
        }

        // Java's `clipped` flag (var22 band test) feeds `textured`
        // which gates the view-space store; we always store view
        // coords, so only the picking logic below consumes the rest.
        let mut picking = false;
        if typecode > 0 {
            let mut p = MOUSE_PICK.lock().unwrap();
            if p.mouse_check {
                let mut var27 = var11 - var12;
                if var27 <= 50 {
                    var27 = 50;
                }
                let (var28, var29) = if var14 > 0 {
                    (var15 / var13, var16 / var27)
                } else {
                    (var15 / var27, var16 / var13)
                };
                let (var30, var31) = if var17 > 0 {
                    (var21 / var13, var19 / var27)
                } else {
                    (var21 / var27, var19 / var13)
                };
                let var32 = p.mouse_x - origin_x;
                let var33 = p.mouse_y - origin_y;
                if var32 > var28 && var32 < var29 && var33 > var30 && var33 < var31 {
                    if self.use_aabb_mouse_check {
                        p.picked.push(typecode);
                    } else {
                        picking = true;
                    }
                }
            }
        }

        let n = self.num_points as usize;
        let mut screen_x = vec![i32::MIN; n];
        let mut screen_y = vec![i32::MIN; n];
        let mut view_x = vec![0i32; n];
        let mut view_y = vec![0i32; n];
        let mut view_z = vec![0i32; n];
        let (sin0, cos0) = if yaw != 0 {
            let t = (yaw & 0x7FF) as usize;
            (pix3d::sin_table()[t], pix3d::cos_table()[t])
        } else {
            (0, 0)
        };
        for i in 0..n {
            let mut vx = self.point_x[i];
            let vy = self.point_y[i];
            let mut vz = self.point_z[i];
            if yaw != 0 {
                let t = (sin0 * vz + cos0 * vx) >> 16;
                vz = (cos0 * vz - sin0 * vx) >> 16;
                vx = t;
            }
            let wx = rel_x + vx;
            let wy = rel_y + vy;
            let wz = rel_z + vz;
            let var46 = (sin_yaw * wz + cos_yaw * wx) >> 16;
            let var47 = (cos_yaw * wz - sin_yaw * wx) >> 16;
            let var49 = (cos_pitch * wy - sin_pitch * var47) >> 16;
            let var50 = (sin_pitch * wy + cos_pitch * var47) >> 16;
            // Java stores vertexScreenZ[i] = var50 - var11 (depth
            // relative to the model centre) for the render2 buckets;
            // render_faces sorts on absolute view_z which yields the
            // same order.
            if var50 >= 50 {
                screen_x[i] = (var46 << 9) / var50 + origin_x;
                screen_y[i] = (var49 << 9) / var50 + origin_y;
            }
            view_x[i] = var46;
            view_y[i] = var49;
            view_z[i] = var50;
        }
        self.render_faces(&screen_x, &screen_y, &view_x, &view_y, &view_z,
                          512, origin_x, origin_y, picking, typecode);
    }
}

pub type SharedModelLit = Arc<ModelLit>;
