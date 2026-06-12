//! 1:1 port of `jagex3.dash3d.ModelUnlit.{calculateNormals, light}` and the static
//! helpers `getColour` / `getTexLight`. This is what `Client.drawInterface` (and every
//! other model-rendering call site) relies on: lighting is applied ONCE at load time,
//! producing per-face per-vertex HSL values (`faceColourA/B/C`) that the render path
//! just looks up.
//!
//! ## Sentinels in `face_colour_c`
//!
//! - `-2` → hidden, skip during render (Java `render2` early-exits on this).
//! - `-1` → flat-shaded face; render uses `colourTable[face_colour_a[f]]`.
//! - else → gouraud face; render uses the three values as the three vertex HSLs.
//!
//! ## `face_render_type` semantics (consumed here, not at render)
//!
//! - `0` (default) → per-vertex Gouraud; face contributes to all three vertex normals.
//! - `1` → flat shading; face stores its own FaceNormal, vertex normals unchanged.
//! - `2` → shared-light marker (set by `shareLight`); skipped during normal calc and
//!   later treated as hidden/skipped in `light()`.
//! - `3` → alpha-sentinel translucent (`face_alpha == -2` upgrades type to 3).
//!
//! ## `face_alpha` overrides
//!
//! Per Java `light()`: `alpha == -1` → type 2, `alpha == -2` → type 3. These happen
//! BEFORE the type dispatch below, so the post-override type is what's checked.

use cache::model::Model;

/// Aggregated per-vertex normal: sum of unit-length (256-scaled) adjacent face normals,
/// `w` is the count of contributing faces. Matches Java `PointNormal {x, y, z, w}`.
#[derive(Clone, Copy, Default)]
pub struct PointNormal {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub w: i32,
}

/// Per-face unit normal (256-scaled) — only computed for faces with `render_type == 1`.
#[derive(Clone, Copy, Default)]
pub struct FaceNormal {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// Output of `calculate_normals`: both arrays sized to numPoints / numFaces. `face_normal`
/// is only populated for faces that needed it (render_type == 1); other entries stay
/// default. `point_normal[v].w == 0` means no adjacent gouraud face touched the vertex.
pub struct Normals {
    pub point_normal: Vec<PointNormal>,
    pub face_normal: Vec<FaceNormal>,
    pub has_face_normals: bool,
}

/// Port of `ModelUnlit.calculateNormals`. Mirrors the halving loop that keeps the cross
/// product components inside `[-8192, 8192]` before length normalisation, and the
/// `length = max(length, 1)` guard.
pub fn calculate_normals(model: &Model) -> Normals {
    let n_points = model.num_points as usize;
    let n_faces = model.num_faces as usize;
    let mut point_normal = vec![PointNormal::default(); n_points];
    let mut face_normal = vec![FaceNormal::default(); n_faces];
    let mut has_face_normals = false;

    for f in 0..n_faces {
        let a = model.face_vertex_a[f] as usize;
        let b = model.face_vertex_b[f] as usize;
        let c = model.face_vertex_c[f] as usize;
        if a >= n_points || b >= n_points || c >= n_points {
            continue;
        }

        let dx_ab = model.point_x[b] - model.point_x[a];
        let dy_ab = model.point_y[b] - model.point_y[a];
        let dz_ab = model.point_z[b] - model.point_z[a];
        let dx_ac = model.point_x[c] - model.point_x[a];
        let dy_ac = model.point_y[c] - model.point_y[a];
        let dz_ac = model.point_z[c] - model.point_z[a];

        let (mut nx, mut ny, mut nz) = (
            dy_ab.wrapping_mul(dz_ac).wrapping_sub(dz_ab.wrapping_mul(dy_ac)),
            dz_ab.wrapping_mul(dx_ac).wrapping_sub(dx_ab.wrapping_mul(dz_ac)),
            dx_ab.wrapping_mul(dy_ac).wrapping_sub(dy_ab.wrapping_mul(dx_ac)),
        );
        // Java `for (nz = ...; nx > 8192 || ny > 8192 || ... ; nz >>= 1) { nx>>=1; ny>>=1; }`
        // Halve all three components together until each is within the 8192 magnitude
        // band, so the subsequent `sqrt` stays inside i32 range.
        while nx > 8192 || ny > 8192 || nz > 8192 || nx < -8192 || ny < -8192 || nz < -8192 {
            nx >>= 1;
            ny >>= 1;
            nz >>= 1;
        }
        let length_sq = (nz as i64) * (nz as i64)
            + (nx as i64) * (nx as i64)
            + (ny as i64) * (ny as i64);
        let length = (length_sq as f64).sqrt() as i32;
        let length = length.max(1);

        // Java scales each face normal to length 256 BEFORE adding into vertex normals,
        // so every face contributes equally regardless of area. Don't substitute
        // area-weighted accumulation here.
        let unit_x = nx * 256 / length;
        let unit_y = ny * 256 / length;
        let unit_z = nz * 256 / length;

        let typ = face_type(model, f);
        if typ == 0 {
            let na = &mut point_normal[a];
            na.x += unit_x; na.y += unit_y; na.z += unit_z; na.w += 1;
            let nb = &mut point_normal[b];
            nb.x += unit_x; nb.y += unit_y; nb.z += unit_z; nb.w += 1;
            let nc = &mut point_normal[c];
            nc.x += unit_x; nc.y += unit_y; nc.z += unit_z; nc.w += 1;
        } else if typ == 1 {
            face_normal[f] = FaceNormal { x: unit_x, y: unit_y, z: unit_z };
            has_face_normals = true;
        }
    }

    Normals { point_normal, face_normal, has_face_normals }
}

/// Output of `light()`: the three baked vertex HSLs per face. Sentinels in `c` per
/// module-level docs.
pub struct LitModel {
    pub face_colour_a: Vec<i32>,
    pub face_colour_b: Vec<i32>,
    pub face_colour_c: Vec<i32>,
}

/// 1:1 port of `ModelUnlit.light(ambient, contrast, x, y, z)` for the non-textured
/// branches (textured branches stored under the same fields but use `getTexLight`
/// instead — wired when texture rendering lands).
///
/// `face_colour_c` is the sentinel-bearing field per module docs.
pub fn light(model: &Model, ambient: i32, contrast: i32, lx: i32, ly: i32, lz: i32) -> LitModel {
    let normals = calculate_normals(model);
    let n_faces = model.num_faces as usize;

    let distance_sq = (lz * lz + lx * lx + ly * ly) as f64;
    let distance = distance_sq.sqrt() as i32;
    let scale = (contrast * distance) >> 8;
    let scale_half_plus = scale / 2 + scale; // matches Java's flat-face divisor

    let mut face_colour_a = vec![0i32; n_faces];
    let mut face_colour_b = vec![0i32; n_faces];
    let mut face_colour_c = vec![0i32; n_faces];

    for f in 0..n_faces {
        let mut typ = face_type(model, f);
        let alpha = face_alpha(model, f);
        let texture_id = face_texture_id(model, f);

        // Java: `if (alpha == -2) type = 3; if (alpha == -1) type = 2;`
        if alpha == -2 {
            typ = 3;
        }
        if alpha == -1 {
            typ = 2;
        }

        if texture_id == -1 {
            // Untextured branch (`Pix3D.flatTriangle` / `Pix3D.gouraudTriangle`).
            if typ == 0 {
                let colour = (model.face_colour[f] as i32) & 0xFFFF;
                let a = model.face_vertex_a[f] as usize;
                let b = model.face_vertex_b[f] as usize;
                let c = model.face_vertex_c[f] as usize;
                let na = normals.point_normal[a];
                let nb = normals.point_normal[b];
                let nc = normals.point_normal[c];
                let ia = vertex_intensity(na, lx, ly, lz, scale, ambient);
                let ib = vertex_intensity(nb, lx, ly, lz, scale, ambient);
                let ic = vertex_intensity(nc, lx, ly, lz, scale, ambient);
                face_colour_a[f] = get_colour(colour, ia);
                face_colour_b[f] = get_colour(colour, ib);
                face_colour_c[f] = get_colour(colour, ic);
            } else if typ == 1 {
                let n = if normals.has_face_normals {
                    normals.face_normal[f]
                } else {
                    FaceNormal::default()
                };
                let intensity = (n.z * lz + n.x * lx + n.y * ly) / scale_half_plus + ambient;
                face_colour_a[f] = get_colour((model.face_colour[f] as i32) & 0xFFFF, intensity);
                face_colour_c[f] = -1;
            } else if typ == 3 {
                face_colour_a[f] = 128;
                face_colour_c[f] = -1;
            } else {
                // type 2 → hidden
                face_colour_c[f] = -2;
            }
        } else if typ == 0 {
            // Textured gouraud — stored as tex-light intensities. Render path still
            // emits flat-shaded HSL pending the texture sampler; treat as the
            // untextured gouraud path with hue=0/sat=0 so we don't crash, until
            // textures land. The numeric outputs match Java's `getTexLight` clamp.
            let a = model.face_vertex_a[f] as usize;
            let b = model.face_vertex_b[f] as usize;
            let c = model.face_vertex_c[f] as usize;
            let na = normals.point_normal[a];
            let nb = normals.point_normal[b];
            let nc = normals.point_normal[c];
            face_colour_a[f] = get_tex_light(vertex_intensity(na, lx, ly, lz, scale, ambient));
            face_colour_b[f] = get_tex_light(vertex_intensity(nb, lx, ly, lz, scale, ambient));
            face_colour_c[f] = get_tex_light(vertex_intensity(nc, lx, ly, lz, scale, ambient));
        } else if typ == 1 {
            let n = if normals.has_face_normals {
                normals.face_normal[f]
            } else {
                FaceNormal::default()
            };
            let intensity = (n.z * lz + n.x * lx + n.y * ly) / scale_half_plus + ambient;
            face_colour_a[f] = get_tex_light(intensity);
            face_colour_c[f] = -1;
        } else {
            face_colour_c[f] = -2;
        }
    }

    LitModel { face_colour_a, face_colour_b, face_colour_c }
}

/// `intensity = (n.z * lz + n.x * lx + n.y * ly) / (n.w * scale) + ambient`. Java guards
/// against `w == 0` implicitly because `pointNormal.w` is incremented by every adjacent
/// gouraud face — but isolated vertices can still hit zero, so we treat that as ambient.
fn vertex_intensity(n: PointNormal, lx: i32, ly: i32, lz: i32, scale: i32, ambient: i32) -> i32 {
    if n.w == 0 || scale == 0 {
        return ambient;
    }
    (n.z * lz + n.x * lx + n.y * ly) / (n.w * scale) + ambient
}

/// `ModelUnlit.getColour(arg0, arg1)` — scales the low-7 luminance bits of `arg0` by
/// `arg1/128`, clamps to `[2, 126]`, and reattaches the top 9 hue+sat bits.
pub fn get_colour(arg0: i32, arg1: i32) -> i32 {
    let mut var2 = (arg0 & 0x7F) * arg1 >> 7;
    if var2 < 2 {
        var2 = 2;
    } else if var2 > 126 {
        var2 = 126;
    }
    (arg0 & 0xFF80) + var2
}

/// `ModelUnlit.getTexLight(arg0)` — clamp to `[2, 126]`.
pub fn get_tex_light(arg0: i32) -> i32 {
    arg0.clamp(2, 126)
}

fn face_type(model: &Model, f: usize) -> i8 {
    match model.face_render_type.as_ref() {
        Some(types) => types[f],
        None => 0,
    }
}

fn face_alpha(model: &Model, f: usize) -> i8 {
    match model.face_alpha.as_ref() {
        Some(alphas) => alphas[f],
        None => 0,
    }
}

fn face_texture_id(model: &Model, f: usize) -> i16 {
    match model.face_texture_id.as_ref() {
        Some(ids) => ids[f],
        None => -1,
    }
}
