//! Software model renderer — 1:1 port of `jagex3.dash3d.ModelLit.{objRender, render2}`
//! perspective pipeline using our ported `Pix3D` rasterizer.
//!
//! Lighting is NOT computed here. Pass a [`crate::LitModel`] built via [`crate::model_light`]
//! to get the per-face per-vertex HSL values Java's render path expects. When `lit` is
//! `None`, faces render flat with raw `face_colour` (useful as a toggle and for models
//! that have no lighting baked).
//!
//! Render pipeline (mirroring Java `ModelLit.objRender(arg0..arg6)`):
//!
//! 1. Rotate model vertices: roll (Z, `arg2`) → model pitch (X, `arg0`) → yaw (Y, `arg1`).
//! 2. Translate by `(origin_x, origin_y, origin_z)` = (arg4, arg5, arg6).
//! 3. Camera tilt (X rotation by `camera_pitch` = `arg3`) — applied AFTER translation.
//! 4. Perspective project: `screen_x = (x_t << 9) / z_tilted + origin_screen_x`,
//!    `screen_y = (y_tilted << 9) / z_tilted + origin_screen_y`. Focal length 512.
//! 5. Back-face cull via screen-space signed area.
//! 6. Depth-bucket sort by face priority then average vertex screen Z.
//! 7. For each face: `pix3d.flat_triangle(...)` with palette[hsl + lambert].

use cache::model::Model;

use crate::model_lit::LitModel;
use crate::pix2d::Pix2D;
use crate::pix3d::{cos_table, sin_table, Pix3D};

const FOCAL_SHIFT: i32 = 9; // focal length = 1 << 9 = 512

pub struct ModelRenderer {
    // Per-render scratch — preallocated and grown as needed.
    pub vertex_screen_x: Vec<i32>,
    pub vertex_screen_y: Vec<i32>,
    pub vertex_screen_z: Vec<i32>, // depth for sorting (post-tilt camera-space z)
    pub face_visible: Vec<bool>,
    /// Per-face draw order: indices sorted back-to-front within each priority bucket.
    pub draw_order: Vec<u32>,
}

impl Default for ModelRenderer {
    fn default() -> Self {
        Self {
            vertex_screen_x: Vec::new(),
            vertex_screen_y: Vec::new(),
            vertex_screen_z: Vec::new(),
            face_visible: Vec::new(),
            draw_order: Vec::new(),
        }
    }
}

impl ModelRenderer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Render `model` into `pix2d` via `pix3d`. All angles in Jagex units (2048 = full turn).
    /// Argument order matches Java `ModelLit.objRender(arg0..arg6)`.
    ///
    /// - `model_pitch` (arg0): X rotation on the model BEFORE translation
    /// - `yaw` (arg1): Y rotation (model spin)
    /// - `roll` (arg2): Z rotation
    /// - `camera_pitch` (arg3): X rotation applied AFTER translation (camera tilt)
    /// - `origin_x/y/z` (arg4..6): translation in model space
    /// - `pix3d.origin_x/y` should be set to the screen-space pivot (component centre)
    ///   before calling — done via `pix3d.set_origin(...)`.
    /// - `lit`: pre-baked lighting from [`crate::model_light`]. When `Some`, faces use
    ///   the Java `render2` dispatch on `face_colour_c` sentinels (-1 flat, -2 hidden,
    ///   else gouraud). When `None`, faces render flat with raw `model.face_colour`.
    #[allow(clippy::too_many_arguments)]
    pub fn obj_render(
        &mut self,
        model: &Model,
        lit: Option<&LitModel>,
        pix2d: &mut Pix2D,
        pix3d: &Pix3D,
        model_pitch: i32,
        yaw: i32,
        roll: i32,
        camera_pitch: i32,
        origin_x: i32,
        origin_y: i32,
        origin_z: i32,
    ) {
        let n_points = model.num_points as usize;
        let n_faces = model.num_faces as usize;
        if n_points == 0 || n_faces == 0 {
            return;
        }
        if self.vertex_screen_x.len() < n_points {
            self.vertex_screen_x.resize(n_points, 0);
            self.vertex_screen_y.resize(n_points, 0);
            self.vertex_screen_z.resize(n_points, 0);
        }
        if self.face_visible.len() < n_faces {
            self.face_visible.resize(n_faces, false);
            self.draw_order.resize(n_faces, 0);
        }

        let sin_roll = sin_table(roll);
        let cos_roll = cos_table(roll);
        let sin_yaw = sin_table(yaw);
        let cos_yaw = cos_table(yaw);
        let sin_mpitch = sin_table(model_pitch);
        let cos_mpitch = cos_table(model_pitch);
        let sin_cpitch = sin_table(camera_pitch);
        let cos_cpitch = cos_table(camera_pitch);
        let pivot_depth =
            (origin_y as i64 * sin_cpitch as i64 + origin_z as i64 * cos_cpitch as i64) >> 16;

        for v in 0..n_points {
            let mut x = model.point_x[v];
            let mut y = model.point_y[v];
            let mut z = model.point_z[v];

            // Roll (Z, arg2): new_x = sin*y + cos*x, new_y = cos*y - sin*x
            if roll != 0 {
                let nx = (sin_roll as i64 * y as i64 + cos_roll as i64 * x as i64) >> 16;
                let ny = (cos_roll as i64 * y as i64 - sin_roll as i64 * x as i64) >> 16;
                x = nx as i32;
                y = ny as i32;
            }
            // Model pitch (X, arg0): new_y = cos*y - sin*z, new_z = sin*y + cos*z
            if model_pitch != 0 {
                let ny = (cos_mpitch as i64 * y as i64 - sin_mpitch as i64 * z as i64) >> 16;
                let nz = (sin_mpitch as i64 * y as i64 + cos_mpitch as i64 * z as i64) >> 16;
                y = ny as i32;
                z = nz as i32;
            }
            // Yaw (Y, arg1): new_x = sin*z + cos*x, new_z = cos*z - sin*x
            if yaw != 0 {
                let nx = (sin_yaw as i64 * z as i64 + cos_yaw as i64 * x as i64) >> 16;
                let nz = (cos_yaw as i64 * z as i64 - sin_yaw as i64 * x as i64) >> 16;
                x = nx as i32;
                z = nz as i32;
            }
            // Translate
            let xt = x + origin_x;
            let yt = y + origin_y;
            let zt = z + origin_z;
            // Camera pitch (X post-translate, arg3): new_y = cos*y - sin*z, new_z = sin*y + cos*z
            let y_tilted =
                ((cos_cpitch as i64 * yt as i64 - sin_cpitch as i64 * zt as i64) >> 16) as i32;
            let z_tilted =
                ((sin_cpitch as i64 * yt as i64 + cos_cpitch as i64 * zt as i64) >> 16) as i32;

            self.vertex_screen_z[v] = z_tilted - pivot_depth as i32;
            // Java's near-plane test (ModelLit.java:1031): `if (var50 >= 50)`. Vertices
            // with smaller tilted-z would project to enormous screen coordinates
            // (`xt * 512 / 5` lands tens of thousands of pixels away), which makes
            // any triangle touching that vertex span the whole canvas — the rasterizer
            // then fills huge bounding boxes with gouraud gradients that wrap into
            // bright palette indices, producing the stray "?"-shaped streaks. Java
            // marks them `-5000` and reroutes to `render3ZClip` for partial-triangle
            // near-plane clipping. We don't have render3ZClip yet (task #53), so we
            // SKIP those faces entirely — gaps at silhouettes are preferable to streaks.
            if z_tilted < 50 {
                self.vertex_screen_x[v] = i32::MIN;
                self.vertex_screen_y[v] = i32::MIN;
            } else {
                self.vertex_screen_x[v] = ((xt << FOCAL_SHIFT) / z_tilted) + pix3d.origin_x;
                self.vertex_screen_y[v] = ((y_tilted << FOCAL_SHIFT) / z_tilted) + pix3d.origin_y;
            }
        }

        // Compute per-face visibility, back-face cull, and average depth bucket index.
        // We need bucket indices ≥ 0 so we offset by min observed depth.
        let mut face_depth = vec![0i32; n_faces];
        let mut min_d = i32::MAX;
        let mut max_d = i32::MIN;
        for f in 0..n_faces {
            self.face_visible[f] = false;
            // Java `render2` early-exits on `faceColourC == -2` (hidden faces). Without a
            // LitModel, fall back to checking raw `face_alpha == -1` which is what the
            // unlit ModelUnlit code path would have done — both express the same intent.
            if let Some(l) = lit {
                if l.face_colour_c[f] == -2 {
                    continue;
                }
            } else if model.face_alpha.as_ref().is_some_and(|a| a[f] == -1) {
                continue;
            }
            let a = model.face_vertex_a[f] as usize;
            let b = model.face_vertex_b[f] as usize;
            let c = model.face_vertex_c[f] as usize;
            if a >= n_points || b >= n_points || c >= n_points {
                continue;
            }
            let xa = self.vertex_screen_x[a];
            let ya = self.vertex_screen_y[a];
            let xb = self.vertex_screen_x[b];
            let yb = self.vertex_screen_y[b];
            let xc = self.vertex_screen_x[c];
            let yc = self.vertex_screen_y[c];
            if xa == i32::MIN || xb == i32::MIN || xc == i32::MIN {
                continue;
            }
            let area2 = (yc - yb) * (xa - xb) - (ya - yb) * (xc - xb);
            if area2 <= 0 {
                continue;
            }
            self.face_visible[f] = true;
            let d = (self.vertex_screen_z[a] + self.vertex_screen_z[b] + self.vertex_screen_z[c]) / 3;
            face_depth[f] = d;
            if d < min_d { min_d = d; }
            if d > max_d { max_d = d; }
        }
        if min_d == i32::MAX {
            return;
        }

        // Build depth buckets. Larger bucket idx = farther from camera (back) = drawn first.
        // We shift so the smallest depth maps to bucket 0.
        let bucket_count = (max_d - min_d + 1) as usize;
        let mut depth_buckets: Vec<Vec<u32>> = vec![Vec::new(); bucket_count];
        for f in 0..n_faces {
            if self.face_visible[f] {
                let bucket = (face_depth[f] - min_d) as usize;
                depth_buckets[bucket].push(f as u32);
            }
        }

        if model.face_priority.is_none() {
            // Simple back-to-front depth-bucket render (no priority overrides).
            for bucket in depth_buckets.iter().rev() {
                for &fi in bucket {
                    self.render_face(model, lit, pix2d, pix3d, fi as usize);
                }
            }
            return;
        }

        // Priority-bucketed render — port of ModelLit.render2 priority section. Within
        // each priority bucket, faces are inserted in BACK-TO-FRONT order (because we
        // walk depth_buckets from far to near). Priority 10 and 11 are "always on top"
        // with depth-aware insertion thresholds derived from the average depth of
        // priorities 1+2, 3+4, and 6+8.
        let face_priority = model.face_priority.as_ref().unwrap();
        let mut p_buckets: Vec<Vec<u32>> = vec![Vec::new(); 12];
        let mut p_depths: Vec<Vec<i32>> = vec![Vec::new(); 12]; // only filled for pri 10/11
        let mut p_depth_sum = [0i32; 12];
        for (b_idx, bucket) in depth_buckets.iter().enumerate().rev() {
            let depth = b_idx as i32; // smaller bucket idx = closer
            for &fi in bucket {
                let pri = face_priority[fi as usize] as usize;
                if pri >= 12 {
                    continue;
                }
                p_buckets[pri].push(fi);
                if pri < 10 {
                    p_depth_sum[pri] += depth;
                } else {
                    p_depths[pri].push(depth);
                }
            }
        }
        let avg_12 = if !p_buckets[1].is_empty() || !p_buckets[2].is_empty() {
            (p_depth_sum[1] + p_depth_sum[2])
                / (p_buckets[1].len() + p_buckets[2].len()) as i32
        } else { 0 };
        let avg_34 = if !p_buckets[3].is_empty() || !p_buckets[4].is_empty() {
            (p_depth_sum[3] + p_depth_sum[4])
                / (p_buckets[3].len() + p_buckets[4].len()) as i32
        } else { 0 };
        let avg_68 = if !p_buckets[6].is_empty() || !p_buckets[8].is_empty() {
            (p_depth_sum[6] + p_depth_sum[8])
                / (p_buckets[6].len() + p_buckets[8].len()) as i32
        } else { 0 };

        // Special-priority cursor: walk pri 10 first, switch to pri 11 when exhausted.
        // Faces in each list are stored in render order (back→front).
        let mut sp_idx = 0usize;
        let mut sp_list = 10usize;
        // Outer loop over priorities 0..10. Specials inserted before pri 0, 3, 5.
        for priority in 0..10usize {
            let threshold = match priority {
                0 => Some(avg_12),
                3 => Some(avg_34),
                5 => Some(avg_68),
                _ => None,
            };
            if let Some(t) = threshold {
                // Drain specials with depth > threshold.
                loop {
                    // Switch from pri 10 to 11 if 10 exhausted.
                    if sp_idx >= p_buckets[sp_list].len() {
                        if sp_list == 10 {
                            sp_list = 11;
                            sp_idx = 0;
                            continue;
                        }
                        break;
                    }
                    let d = p_depths[sp_list][sp_idx];
                    if d <= t {
                        break;
                    }
                    let f = p_buckets[sp_list][sp_idx] as usize;
                    sp_idx += 1;
                    self.render_face(model, lit, pix2d, pix3d, f);
                }
            }
            // Render this priority's faces (in stored back→front order).
            for fi_idx in 0..p_buckets[priority].len() {
                let f = p_buckets[priority][fi_idx] as usize;
                self.render_face(model, lit, pix2d, pix3d, f);
            }
        }
        // Any remaining specials (priority 10 / 11) draw on top last.
        loop {
            if sp_idx >= p_buckets[sp_list].len() {
                if sp_list == 10 {
                    sp_list = 11;
                    sp_idx = 0;
                    continue;
                }
                break;
            }
            let f = p_buckets[sp_list][sp_idx] as usize;
            sp_idx += 1;
            self.render_face(model, lit, pix2d, pix3d, f);
        }
    }

    /// Java `ModelLit.render2` per-face dispatch (untextured branches only — textured
    /// branches deferred until texture sampler lands).
    fn render_face(
        &self,
        model: &Model,
        lit: Option<&LitModel>,
        pix2d: &mut Pix2D,
        pix3d: &Pix3D,
        f: usize,
    ) {
        let a = model.face_vertex_a[f] as usize;
        let b = model.face_vertex_b[f] as usize;
        let c = model.face_vertex_c[f] as usize;
        let (ya, yb, yc) = (
            self.vertex_screen_y[a],
            self.vertex_screen_y[b],
            self.vertex_screen_y[c],
        );
        let (xa, xb, xc) = (
            self.vertex_screen_x[a],
            self.vertex_screen_x[b],
            self.vertex_screen_x[c],
        );
        if let Some(l) = lit {
            let cc = l.face_colour_c[f];
            if cc == -2 {
                // hidden — already filtered upstream, defensive
                return;
            }
            if cc == -1 {
                // Java: `Pix3D.flatTriangle(...colourTable[faceColourA[face]])`
                let rgb = Pix3D::palette_lookup(l.face_colour_a[f]);
                pix3d.flat_triangle(pix2d, ya, yb, yc, xa, xb, xc, rgb);
            } else {
                // Java: `Pix3D.gouraudTriangle(...faceColourA, faceColourB, faceColourC)`
                pix3d.gouraud_triangle(
                    pix2d, ya, yb, yc, xa, xb, xc,
                    l.face_colour_a[f], l.face_colour_b[f], cc,
                );
            }
        } else {
            // Unlit fallback — raw face_colour, flat-shaded.
            let hsl = (model.face_colour[f] as i32) & 0xFFFF;
            let rgb = Pix3D::palette_lookup(hsl);
            pix3d.flat_triangle(pix2d, ya, yb, yc, xa, xb, xc, rgb);
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use cache::model::Model;

    /// A trivial single-face model (a triangle in the XY plane).
    fn unit_tri_model() -> Model {
        Model {
            num_points: 3,
            num_faces: 1,
            num_t: 0,
            point_x: vec![-50, 50, 0],
            point_y: vec![50, 50, -50],
            point_z: vec![0, 0, 0],
            face_vertex_a: vec![0],
            face_vertex_b: vec![1],
            face_vertex_c: vec![2],
            face_render_type: None,
            face_priority: None,
            face_alpha: None,
            face_texture_axis: None,
            face_colour: vec![0x4000],
            face_texture_id: None,
            priority: 0,
            texture_render_type: None,
            face_texture_p: None,
            face_texture_m: None,
            face_texture_n: None,
            texture_scale_x: None,
            texture_scale_y: None,
            texture_scale_z: None,
            texture_rotation: None,
            texture_speed: None,
            texture_direction: None,
            texture_translation: None,
            vertex_label: None,
            face_label: None,
        }
    }

    #[test]
    fn renders_triangle_to_buffer() {
        let mut p = Pix2D::new(128, 128);
        let mut p3 = Pix3D::new(&p);
        p3.set_origin(&p, 64, 64);
        let mut r = ModelRenderer::new();
        // Front-facing, distance ~200 along +Z. Unlit (raw face colour, flat shaded).
        r.obj_render(&unit_tri_model(), None, &mut p, &p3, 0, 0, 0, 0, 0, 0, 200);
        let nonzero = p.pixels.iter().filter(|&&px| px != 0).count();
        assert!(nonzero > 50, "expected rendered triangle, got {nonzero} pixels");
    }
}
