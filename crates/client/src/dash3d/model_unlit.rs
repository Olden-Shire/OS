// @ObfuscatedName("fw") — jag::oldscape::dash3d::ModelUnlit extends ModelSource
//
// Unlit triangle-mesh model. Two on-disk formats:
//   • ob3 (newer): trailing 0xFF 0xFF tag, header at len-23
//   • ob2 (older): header at len-18
// Both pack vertex deltas, face stripping codes, and texture descriptors
// in interleaved column streams. Decoders walk multiple Packet cursors
// over the same buffer to consume each column in parallel.
//
// Lighting (ModelLit), animation (SeqType), and rendering (objRender +
// Pix3D textured triangle) land in follow-up turns.

#![allow(dead_code)]

use crate::io::packet::Packet;

// @ObfuscatedName("fy") — jag::oldscape::dash3d::PointNormal. Accumulator
// for the per-vertex normals ModelUnlit.calculateNormals fills in.
#[derive(Debug, Default, Clone, Copy)]
pub struct PointNormal {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub w: i32,
}

// @ObfuscatedName("fp") — jag::oldscape::dash3d::FaceNormal. Per-face
// normal, used when faceRenderType == 1 (flat-shaded face).
#[derive(Debug, Default, Clone, Copy)]
pub struct FaceNormal {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Debug, Default, Clone)]
pub struct ModelUnlit {
    // @ObfuscatedName("fw.j")
    pub num_points: i32,
    // @ObfuscatedName("fw.z") / "fw.g" / "fw.q"
    pub point_x: Vec<i32>,
    pub point_y: Vec<i32>,
    pub point_z: Vec<i32>,
    // @ObfuscatedName("fw.i")
    pub num_faces: i32,
    // @ObfuscatedName("fw.s") / "fw.u" / "fw.v"
    pub face_vertex_a: Vec<i32>,
    pub face_vertex_b: Vec<i32>,
    pub face_vertex_c: Vec<i32>,
    // @ObfuscatedName("fw.w")
    pub face_render_type: Option<Vec<i8>>,
    // @ObfuscatedName("fw.e")
    pub face_priority: Option<Vec<i8>>,
    // @ObfuscatedName("fw.b")
    pub face_alpha: Option<Vec<i8>>,
    // @ObfuscatedName("fw.y")
    pub face_texture_axis: Option<Vec<i8>>,
    // @ObfuscatedName("fw.t")
    pub face_colour: Vec<i16>,
    // @ObfuscatedName("fw.f")
    pub face_texture_id: Option<Vec<i16>>,
    // @ObfuscatedName("fw.k")
    pub priority: i8,
    // @ObfuscatedName("fw.o")
    pub num_t: i32,
    // @ObfuscatedName("fw.a") / "fw.h" / "fw.x" / "fw.p"
    pub texture_render_type: Option<Vec<i8>>,
    pub face_texture_p: Option<Vec<i16>>,
    pub face_texture_m: Option<Vec<i16>>,
    pub face_texture_n: Option<Vec<i16>>,
    // @ObfuscatedName("fw.ad") / "fw.ac" / "fw.aa" / "fw.as" / "fw.am" / "fw.ap" / "fw.av"
    pub texture_scale_x: Option<Vec<i16>>,
    pub texture_scale_y: Option<Vec<i16>>,
    pub texture_scale_z: Option<Vec<i16>>,
    pub texture_rotation: Option<Vec<i16>>,
    pub texture_speed: Option<Vec<i16>>,
    pub texture_direction: Option<Vec<i16>>,
    pub texture_translation: Option<Vec<i8>>,
    // @ObfuscatedName("fw.ak") / "fw.az"
    pub vertex_label: Option<Vec<i32>>,
    pub face_label: Option<Vec<i32>>,
    // @ObfuscatedName("fw.bf") / "fw.bg" — populated by prepareAnim
    // from vertex_label / face_label; one entry per skeleton label
    // ID, each entry holds the vertex / face indices that share that
    // label. Animation playback reads these to know which points and
    // faces to transform per bone.
    pub label_vertices: Option<Vec<Vec<i32>>>,
    pub label_faces: Option<Vec<Vec<i32>>>,
    // @ObfuscatedName("fw.ao") / "fw.ag"
    pub ambient: i16,
    pub contrast: i16,
    // @ObfuscatedName("fw.al")
    pub point_normal: Option<Vec<PointNormal>>,
    // @ObfuscatedName("fw.ay")
    pub face_normal: Option<Vec<FaceNormal>>,

    // @ObfuscatedName("fw.aw") — sharedPointNormal. ModelUnlit.shareLight
    // sums adjacent loc / ground normals into here when two models meet
    // at the same world position; light() then uses these in place of
    // point_normal for that vertex so the per-corner intensity matches
    // across the seam. None until shareLight runs; Some(None) for any
    // vertex not paired with a neighbour. Java keeps it as PointNormal[]
    // where un-paired entries stay null.
    pub shared_point_normal: Option<Vec<Option<PointNormal>>>,

    // @ObfuscatedName("fw.ax") / "fw.bx" / "fw.bw" / "fw.bn" / "fw.bm" / "fw.bj"
    // Axis-aligned bounding cube. Populated by `calc_bounding_cube`;
    // shareLight uses these to reject obviously non-touching pairs
    // before the O(P×P) vertex match loop.
    pub bounds_calculated: bool,
    pub min_x: i32,
    pub max_x: i32,
    pub min_y: i32,
    pub max_y: i32,
    pub min_z: i32,
    pub max_z: i32,
}

impl ModelUnlit {
    pub fn from_bytes(src: Vec<u8>) -> Self {
        let mut m = ModelUnlit::default();
        let len = src.len();
        if len >= 2 && (src[len - 1] as i8) == -1 && (src[len - 2] as i8) == -1 {
            m.load_ob3(src);
        } else {
            m.load_ob2(src);
        }
        m
    }

    // @ObfuscatedName("fw.<init>([Lfw;I)") — ModelUnlit merge
    // constructor (ModelUnlit.java:809-979). Concatenates several
    // unlit models into one, deduplicating shared vertices via
    // addPoint. Used by LocType.buildModel for multi-model locs.
    pub fn merge(models: &[&ModelUnlit]) -> Self {
        let mut copy_render_type = false;
        let mut copy_priority = false;
        let mut copy_alpha = false;
        let mut copy_label = false;
        let mut copy_texture_id = false;
        let mut copy_texture_axis = false;

        let mut out = ModelUnlit::default();
        out.priority = -1;
        let mut total_points = 0usize;
        let mut total_faces = 0usize;
        let mut total_t = 0usize;
        for model in models {
            total_points += model.num_points as usize;
            total_faces += model.num_faces as usize;
            total_t += model.num_t as usize;
            if model.face_priority.is_none() {
                if out.priority == -1 {
                    out.priority = model.priority;
                }
                if out.priority != model.priority {
                    copy_priority = true;
                }
            } else {
                copy_priority = true;
            }
            copy_render_type |= model.face_render_type.is_some();
            copy_alpha |= model.face_alpha.is_some();
            copy_label |= model.face_label.is_some();
            copy_texture_id |= model.face_texture_id.is_some();
            copy_texture_axis |= model.face_texture_axis.is_some();
        }

        out.point_x = vec![0; total_points];
        out.point_y = vec![0; total_points];
        out.point_z = vec![0; total_points];
        out.vertex_label = Some(vec![0; total_points]);
        out.face_vertex_a = vec![0; total_faces];
        out.face_vertex_b = vec![0; total_faces];
        out.face_vertex_c = vec![0; total_faces];
        if copy_render_type {
            out.face_render_type = Some(vec![0i8; total_faces]);
        }
        if copy_priority {
            out.face_priority = Some(vec![0i8; total_faces]);
        }
        if copy_alpha {
            out.face_alpha = Some(vec![0i8; total_faces]);
        }
        if copy_label {
            out.face_label = Some(vec![0; total_faces]);
        }
        if copy_texture_id {
            out.face_texture_id = Some(vec![0i16; total_faces]);
        }
        if copy_texture_axis {
            out.face_texture_axis = Some(vec![0i8; total_faces]);
        }
        out.face_colour = vec![0i16; total_faces];
        if total_t > 0 {
            out.texture_render_type = Some(vec![0i8; total_t]);
            out.face_texture_p = Some(vec![0i16; total_t]);
            out.face_texture_m = Some(vec![0i16; total_t]);
            out.face_texture_n = Some(vec![0i16; total_t]);
            out.texture_scale_x = Some(vec![0i16; total_t]);
            out.texture_scale_y = Some(vec![0i16; total_t]);
            out.texture_scale_z = Some(vec![0i16; total_t]);
            out.texture_rotation = Some(vec![0i16; total_t]);
            out.texture_translation = Some(vec![0i8; total_t]);
            out.texture_speed = Some(vec![0i16; total_t]);
            out.texture_direction = Some(vec![0i16; total_t]);
        }

        out.num_points = 0;
        out.num_faces = 0;
        out.num_t = 0;
        for model in models {
            for f in 0..model.num_faces as usize {
                let nf = out.num_faces as usize;
                if copy_render_type {
                    if let Some(src) = model.face_render_type.as_ref() {
                        out.face_render_type.as_mut().unwrap()[nf] = src[f];
                    }
                }
                if copy_priority {
                    out.face_priority.as_mut().unwrap()[nf] = match model.face_priority.as_ref() {
                        None => model.priority,
                        Some(src) => src[f],
                    };
                }
                if copy_alpha {
                    if let Some(src) = model.face_alpha.as_ref() {
                        out.face_alpha.as_mut().unwrap()[nf] = src[f];
                    }
                }
                if copy_label {
                    if let Some(src) = model.face_label.as_ref() {
                        out.face_label.as_mut().unwrap()[nf] = src[f];
                    }
                }
                if copy_texture_id {
                    out.face_texture_id.as_mut().unwrap()[nf] = match model.face_texture_id.as_ref() {
                        None => -1,
                        Some(src) => src[f],
                    };
                }
                if copy_texture_axis {
                    let axis = model.face_texture_axis.as_ref().map_or(-1i8, |src| src[f]);
                    out.face_texture_axis.as_mut().unwrap()[nf] = if axis == -1 {
                        -1
                    } else {
                        (axis as i32 + out.num_t) as i8
                    };
                }
                out.face_colour[nf] = model.face_colour[f];
                out.face_vertex_a[nf] = out.add_point(model, model.face_vertex_a[f]);
                out.face_vertex_b[nf] = out.add_point(model, model.face_vertex_b[f]);
                out.face_vertex_c[nf] = out.add_point(model, model.face_vertex_c[f]);
                out.num_faces += 1;
            }
            for t in 0..model.num_t as usize {
                let nt = out.num_t as usize;
                let render_type = model.texture_render_type.as_ref().map_or(0i8, |v| v[t]);
                out.texture_render_type.as_mut().unwrap()[nt] = render_type;
                if render_type == 0 {
                    let p = model.face_texture_p.as_ref().map_or(0, |v| v[t]) as i32;
                    let m = model.face_texture_m.as_ref().map_or(0, |v| v[t]) as i32;
                    let n = model.face_texture_n.as_ref().map_or(0, |v| v[t]) as i32;
                    out.face_texture_p.as_mut().unwrap()[nt] = out.add_point(model, p) as i16;
                    out.face_texture_m.as_mut().unwrap()[nt] = out.add_point(model, m) as i16;
                    out.face_texture_n.as_mut().unwrap()[nt] = out.add_point(model, n) as i16;
                }
                if (1..=3).contains(&render_type) {
                    out.face_texture_p.as_mut().unwrap()[nt] = model.face_texture_p.as_ref().map_or(0, |v| v[t]);
                    out.face_texture_m.as_mut().unwrap()[nt] = model.face_texture_m.as_ref().map_or(0, |v| v[t]);
                    out.face_texture_n.as_mut().unwrap()[nt] = model.face_texture_n.as_ref().map_or(0, |v| v[t]);
                    out.texture_scale_x.as_mut().unwrap()[nt] = model.texture_scale_x.as_ref().map_or(0, |v| v[t]);
                    out.texture_scale_y.as_mut().unwrap()[nt] = model.texture_scale_y.as_ref().map_or(0, |v| v[t]);
                    out.texture_scale_z.as_mut().unwrap()[nt] = model.texture_scale_z.as_ref().map_or(0, |v| v[t]);
                    out.texture_rotation.as_mut().unwrap()[nt] = model.texture_rotation.as_ref().map_or(0, |v| v[t]);
                    out.texture_translation.as_mut().unwrap()[nt] = model.texture_translation.as_ref().map_or(0, |v| v[t]);
                    out.texture_speed.as_mut().unwrap()[nt] = model.texture_speed.as_ref().map_or(0, |v| v[t]);
                }
                if render_type == 2 {
                    out.texture_direction.as_mut().unwrap()[nt] = model.texture_direction.as_ref().map_or(0, |v| v[t]);
                }
                out.num_t += 1;
            }
        }
        out
    }

    // @ObfuscatedName("fw.f(Lfw;I)I") — ModelUnlit.addPoint. Dedup
    // merge of one source vertex into this model.
    fn add_point(&mut self, src: &ModelUnlit, vertex: i32) -> i32 {
        let v = vertex as usize;
        let x = src.point_x[v];
        let y = src.point_y[v];
        let z = src.point_z[v];
        for i in 0..self.num_points as usize {
            if self.point_x[i] == x && self.point_y[i] == y && self.point_z[i] == z {
                return i as i32;
            }
        }
        let np = self.num_points as usize;
        self.point_x[np] = x;
        self.point_y[np] = y;
        self.point_z[np] = z;
        if let Some(src_labels) = src.vertex_label.as_ref() {
            self.vertex_label.as_mut().unwrap()[np] = src_labels[v];
        }
        self.num_points += 1;
        np as i32
    }

    // @ObfuscatedName("fw.o([[IIIIZI)Lfw;") — ModelUnlit.hillSkew.
    // In-place variant (Java's copy=false); build paths clone first.
    // Returns false when out of range / already flat (Java's
    // early-return-this cases).
    pub fn hill_skew_in_place(&mut self, groundh: &[Vec<i32>], x: i32, y: i32, z: i32,
                              blend: i32) -> bool {
        self.calc_bounding_cube();
        let var7 = self.min_x + x;
        let var8 = self.max_x + x;
        let var9 = self.max_z + z;
        let var10 = self.min_z + z;
        if var7 < 0
            || ((var8 + 128) >> 7) as usize >= groundh.len()
            || var9 < 0
            || ((var10 + 128) >> 7) as usize >= groundh[0].len()
        {
            return false;
        }
        let t11 = (var7 >> 7) as usize;
        let t12 = ((var8 + 127) >> 7) as usize;
        let t13 = (var9 >> 7) as usize;
        let t14 = ((var10 + 127) >> 7) as usize;
        if groundh[t11][t13] == y && groundh[t12][t13] == y
            && groundh[t11][t14] == y && groundh[t12][t14] == y
        {
            return false;
        }
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
                self.point_y[i] += h - y;
            }
        } else {
            let min_y = self.min_y;
            if min_y == 0 {
                return false;
            }
            for i in 0..self.num_points as usize {
                let var27 = (-self.point_y[i] << 16) / min_y;
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
                    self.point_y[i] += (h - y) * (blend - var27) / blend;
                }
            }
        }
        self.geometry_changed();
        true
    }

    // @ObfuscatedName("fw.y([B)V") — ModelUnlit.loadOb3Engine200
    pub fn load_ob3(&mut self, src: Vec<u8>) {
        let len = src.len() as i32;
        // 7 cursors into the same buffer — Java uses var1..var8.
        let mut p1 = Packet::from_vec(src.clone());
        let mut p3 = Packet::from_vec(src.clone());
        let mut p4 = Packet::from_vec(src.clone());
        let mut p5 = Packet::from_vec(src.clone());
        let mut p6 = Packet::from_vec(src.clone());
        let mut p7 = Packet::from_vec(src.clone());
        let mut p8 = Packet::from_vec(src);

        p1.pos = len - 23;
        let num_points = p1.g2();
        let num_faces = p1.g2();
        let num_t = p1.g1();
        let has_face_info = p1.g1();
        let has_priorities = p1.g1();
        let has_face_alpha = p1.g1();
        let has_face_labels = p1.g1();
        // Java ModelUnlit.java:198-199 reads var16 (face_textures flag)
        // then var17 (vertex_labels flag). Earlier versions of this port
        // swapped these two bytes, silently disabling textures on any
        // model that had textures but no vertex labels (i.e. most
        // textured loc models — they aren't skeletal-animated).
        let has_face_textures = p1.g1();
        let has_vertex_labels = p1.g1();
        let data_len_x = p1.g2();
        let data_len_y = p1.g2();
        let data_len_z = p1.g2();
        let data_len_face_index = p1.g2();
        let data_len_texture_axis = p1.g2();

        let mut simple_tex = 0i32;
        let mut complex_tex = 0i32;
        let mut cube_tex = 0i32;
        if num_t > 0 {
            self.texture_render_type = Some(vec![0i8; num_t as usize]);
            p1.pos = 0;
            for i in 0..num_t as usize {
                let ttype = p1.g1b();
                self.texture_render_type.as_mut().unwrap()[i] = ttype;
                if ttype == 0 { simple_tex += 1; }
                if (1..=3).contains(&ttype) { complex_tex += 1; }
                if ttype == 2 { cube_tex += 1; }
            }
        }

        let mut off = num_points + num_t;
        let face_render_off = off;
        if has_face_info == 1 { off += num_faces; }

        let face_priority_off = off + num_faces;
        let mut off = face_priority_off;
        if has_priorities == 255 { off += num_faces; }
        let face_label_off = off;
        if has_face_labels == 1 { off += num_faces; }
        let vertex_label_off = off;
        if has_vertex_labels == 1 { off += num_points; }
        let face_alpha_off = off;
        if has_face_alpha == 1 { off += num_faces; }

        let face_index_end = data_len_face_index + off;
        let face_tex_id_off = face_index_end;
        let mut off2 = face_tex_id_off;
        if has_face_textures == 1 { off2 += num_faces * 2; }
        let vertex_x_off = data_len_texture_axis + off2;
        let face_colour_off = num_faces * 2 + vertex_x_off;
        let vertex_y_off = data_len_x + face_colour_off;
        let vertex_z_off = data_len_y + vertex_y_off;
        let texture_simple_off = data_len_z + vertex_z_off;
        let texture_complex_off = simple_tex * 6 + texture_simple_off;
        let texture_scale_off = complex_tex * 6 + texture_complex_off;
        let texture_rotation_off = complex_tex * 6 + texture_scale_off;
        let texture_translation_off = complex_tex * 2 + texture_rotation_off;
        let texture_speed_off = complex_tex + texture_translation_off;
        let texture_extra_end = complex_tex * 2 + cube_tex * 2 + texture_speed_off;

        self.num_points = num_points;
        self.num_faces = num_faces;
        self.num_t = num_t;
        self.point_x = vec![0i32; num_points as usize];
        self.point_y = vec![0i32; num_points as usize];
        self.point_z = vec![0i32; num_points as usize];
        self.face_vertex_a = vec![0i32; num_faces as usize];
        self.face_vertex_b = vec![0i32; num_faces as usize];
        self.face_vertex_c = vec![0i32; num_faces as usize];

        if has_vertex_labels == 1 { self.vertex_label = Some(vec![0i32; num_points as usize]); }
        if has_face_info == 1 { self.face_render_type = Some(vec![0i8; num_faces as usize]); }
        if has_priorities == 255 {
            self.face_priority = Some(vec![0i8; num_faces as usize]);
        } else {
            self.priority = has_priorities as i8;
        }
        if has_face_alpha == 1 { self.face_alpha = Some(vec![0i8; num_faces as usize]); }
        if has_face_labels == 1 { self.face_label = Some(vec![0i32; num_faces as usize]); }
        if has_face_textures == 1 { self.face_texture_id = Some(vec![0i16; num_faces as usize]); }
        if has_face_textures == 1 && num_t > 0 {
            self.face_texture_axis = Some(vec![0i8; num_faces as usize]);
        }
        self.face_colour = vec![0i16; num_faces as usize];
        if num_t > 0 {
            self.face_texture_p = Some(vec![0i16; num_t as usize]);
            self.face_texture_m = Some(vec![0i16; num_t as usize]);
            self.face_texture_n = Some(vec![0i16; num_t as usize]);
            if complex_tex > 0 {
                self.texture_scale_x = Some(vec![0i16; complex_tex as usize]);
                self.texture_scale_y = Some(vec![0i16; complex_tex as usize]);
                self.texture_scale_z = Some(vec![0i16; complex_tex as usize]);
                self.texture_rotation = Some(vec![0i16; complex_tex as usize]);
                self.texture_translation = Some(vec![0i8; complex_tex as usize]);
                self.texture_speed = Some(vec![0i16; complex_tex as usize]);
            }
            if cube_tex > 0 {
                self.texture_direction = Some(vec![0i16; cube_tex as usize]);
            }
        }

        // ── Vertex decode ────────────────────────────────────────
        p1.pos = num_t;
        p3.pos = vertex_x_off;
        p4.pos = vertex_y_off;
        p5.pos = vertex_z_off;
        p6.pos = vertex_label_off;

        let mut cx = 0i32;
        let mut cy = 0i32;
        let mut cz = 0i32;
        for i in 0..num_points as usize {
            let order = p1.g1();
            let dx = if order & 0x1 != 0 { p3.gsmarts() } else { 0 };
            let dy = if order & 0x2 != 0 { p4.gsmarts() } else { 0 };
            let dz = if order & 0x4 != 0 { p5.gsmarts() } else { 0 };
            self.point_x[i] = cx + dx;
            self.point_y[i] = cy + dy;
            self.point_z[i] = cz + dz;
            cx = self.point_x[i];
            cy = self.point_y[i];
            cz = self.point_z[i];
            if has_vertex_labels == 1 {
                self.vertex_label.as_mut().unwrap()[i] = p6.g1();
            }
        }

        // ── Face attribute decode ─────────────────────────────────
        p1.pos = face_colour_off;
        p3.pos = face_render_off;
        p4.pos = face_priority_off;
        p5.pos = face_alpha_off;
        p6.pos = face_label_off;
        p7.pos = face_tex_id_off;
        p8.pos = face_index_end;

        for i in 0..num_faces as usize {
            self.face_colour[i] = p1.g2() as i16;
            if has_face_info == 1 {
                self.face_render_type.as_mut().unwrap()[i] = p3.g1b();
            }
            if has_priorities == 255 {
                self.face_priority.as_mut().unwrap()[i] = p4.g1b();
            }
            if has_face_alpha == 1 {
                self.face_alpha.as_mut().unwrap()[i] = p5.g1b();
            }
            if has_face_labels == 1 {
                self.face_label.as_mut().unwrap()[i] = p6.g1();
            }
            if has_face_textures == 1 {
                self.face_texture_id.as_mut().unwrap()[i] = (p7.g2() - 1) as i16;
            }
            if self.face_texture_axis.is_some()
                && self.face_texture_id.as_ref().map_or(-1, |v| v[i] as i32) != -1
            {
                self.face_texture_axis.as_mut().unwrap()[i] = (p8.g1() - 1) as i8;
            }
        }

        // ── Face index (vertex-triple) decode ─────────────────────
        p1.pos = face_index_end + (if has_face_textures == 1 { num_faces * 2 } else { 0 })
            + data_len_texture_axis + num_faces * 2 + data_len_x + data_len_y + data_len_z
            - (data_len_face_index) - (num_faces * 2); // restore to faceIndexOffset
        // Java actually re-anchors with var33 / var30; the variable
        // tracking above mirrors theirs.
        p1.pos = face_index_end + (face_colour_off - face_index_end - (data_len_face_index));
        // The exact var33 / var30 chase is fragile; reset to the known
        // anchors below.
        p1.pos = face_index_end + data_len_texture_axis + (if has_face_textures == 1 { num_faces * 2 } else { 0 });
        // Java equivalent: var1.pos = var33 (= post tex-id + tex-axis),
        //                 var3.pos = var30 (= after texture render type +
        //                                    num_points + face_render flag).
        p1.pos = face_index_end + data_len_texture_axis + (if has_face_textures == 1 { num_faces * 2 } else { 0 });
        p3.pos = num_t + num_points + (if has_face_info == 1 { num_faces } else { 0 });

        // Reset face index walker.
        p1.pos = face_index_end - data_len_face_index;
        p3.pos = num_t + num_points;
        if has_face_info == 1 { p3.pos += num_faces; }

        let mut a = 0i32;
        let mut b = 0i32;
        let mut c = 0i32;
        let mut last = 0i32;
        for i in 0..num_faces as usize {
            let order = p3.g1();
            match order {
                1 => {
                    a = p1.gsmarts() + last;
                    b = p1.gsmarts() + a;
                    c = p1.gsmarts() + b;
                    last = c;
                }
                2 => {
                    b = c;
                    c = p1.gsmarts() + last;
                    last = c;
                }
                3 => {
                    a = c;
                    c = p1.gsmarts() + last;
                    last = c;
                }
                4 => {
                    let tmp = a;
                    a = b;
                    b = tmp;
                    c = p1.gsmarts() + last;
                    last = c;
                }
                _ => {}
            }
            self.face_vertex_a[i] = a;
            self.face_vertex_b[i] = b;
            self.face_vertex_c[i] = c;
        }

        // ── Texture descriptor decode ─────────────────────────────
        p1.pos = texture_simple_off;
        p3.pos = texture_complex_off;
        p4.pos = texture_scale_off;
        p5.pos = texture_rotation_off;
        p6.pos = texture_translation_off;
        p7.pos = texture_speed_off;
        for i in 0..num_t as usize {
            let ttype = (self.texture_render_type.as_ref().unwrap()[i] as i32) & 0xFF;
            match ttype {
                0 => {
                    self.face_texture_p.as_mut().unwrap()[i] = p1.g2() as i16;
                    self.face_texture_m.as_mut().unwrap()[i] = p1.g2() as i16;
                    self.face_texture_n.as_mut().unwrap()[i] = p1.g2() as i16;
                }
                1 | 3 => {
                    self.face_texture_p.as_mut().unwrap()[i] = p3.g2() as i16;
                    self.face_texture_m.as_mut().unwrap()[i] = p3.g2() as i16;
                    self.face_texture_n.as_mut().unwrap()[i] = p3.g2() as i16;
                    self.texture_scale_x.as_mut().unwrap()[i] = p4.g2() as i16;
                    self.texture_scale_y.as_mut().unwrap()[i] = p4.g2() as i16;
                    self.texture_scale_z.as_mut().unwrap()[i] = p4.g2() as i16;
                    self.texture_rotation.as_mut().unwrap()[i] = p5.g2() as i16;
                    self.texture_translation.as_mut().unwrap()[i] = p6.g1b();
                    self.texture_speed.as_mut().unwrap()[i] = p7.g2() as i16;
                }
                2 => {
                    self.face_texture_p.as_mut().unwrap()[i] = p3.g2() as i16;
                    self.face_texture_m.as_mut().unwrap()[i] = p3.g2() as i16;
                    self.face_texture_n.as_mut().unwrap()[i] = p3.g2() as i16;
                    self.texture_scale_x.as_mut().unwrap()[i] = p4.g2() as i16;
                    self.texture_scale_y.as_mut().unwrap()[i] = p4.g2() as i16;
                    self.texture_scale_z.as_mut().unwrap()[i] = p4.g2() as i16;
                    self.texture_rotation.as_mut().unwrap()[i] = p5.g2() as i16;
                    self.texture_translation.as_mut().unwrap()[i] = p6.g1b();
                    self.texture_speed.as_mut().unwrap()[i] = p7.g2() as i16;
                    self.texture_direction.as_mut().unwrap()[i] = p7.g2() as i16;
                }
                _ => {}
            }
        }
        // ── Trailing extras (Java reads but ignores) ──────────────
        p1.pos = texture_extra_end;
        let extra = p1.g1();
        if extra != 0 {
            p1.g2(); p1.g2(); p1.g2(); p1.g4();
        }
    }

    // @ObfuscatedName("fw.az()V") — ModelUnlit.calcBoundingCube
    // Walks every point and tracks min/max along each axis. Cached
    // via `bounds_calculated` so shareLight can call it freely from
    // both sides of a pair.
    pub fn calc_bounding_cube(&mut self) {
        if self.bounds_calculated { return; }
        let mut min_x = i32::MAX;
        let mut max_x = i32::MIN;
        let mut min_y = i32::MAX;
        let mut max_y = i32::MIN;
        let mut min_z = i32::MAX;
        let mut max_z = i32::MIN;
        for i in 0..self.num_points as usize {
            let x = self.point_x[i];
            let y = self.point_y[i];
            let z = self.point_z[i];
            if x < min_x { min_x = x; }
            if x > max_x { max_x = x; }
            if y < min_y { min_y = y; }
            if y > max_y { max_y = y; }
            if z < min_z { min_z = z; }
            if z > max_z { max_z = z; }
        }
        if self.num_points == 0 {
            min_x = 0; max_x = 0; min_y = 0; max_y = 0; min_z = 0; max_z = 0;
        }
        self.min_x = min_x;
        self.max_x = max_x;
        self.min_y = min_y;
        self.max_y = max_y;
        self.min_z = min_z;
        self.max_z = max_z;
        self.bounds_calculated = true;
    }

    // @ObfuscatedName("fw.an(Lfw;Lfw;IIIZ)V") — ModelUnlit.shareLight
    //
    // Pairs vertices that share a world position between two models,
    // and sums each side's accumulated normal into the OTHER side's
    // `shared_point_normal[v]`. After both halves run, `light()` reads
    // shared instead of raw, so the per-corner gouraud colours on each
    // model agree at the seam.
    //
    // `(ax, ay, az)` is model1's origin in model2's coordinate system —
    // a point in model1 at (px, py, pz) corresponds to model2's
    // (px - ax, py - ay, pz - az). Java's `arg5` (here `mark_for_type_2`)
    // promotes faces that became fully shared to render-type 2 (smooth
    // gouraud with no flat fill); we mirror that for ground decor calls
    // only — wall pairs pass `false`.
    pub fn share_light(model1: &mut ModelUnlit, model2: &mut ModelUnlit, ax: i32, ay: i32, az: i32, mark_for_type_2: bool) {
        model1.calc_bounding_cube();
        model1.calculate_normals();
        model2.calc_bounding_cube();
        model2.calculate_normals();

        // Lazy-allocate the shared arrays.
        if model1.shared_point_normal.is_none() {
            model1.shared_point_normal = Some(vec![None; model1.num_points as usize]);
        }
        if model2.shared_point_normal.is_none() {
            model2.shared_point_normal = Some(vec![None; model2.num_points as usize]);
        }

        // Java uses static `shareTic` + parallel `shareMap` / `shareMap2`
        // bitmasks to remember which vertex pairs got linked this call —
        // used after the loop to upgrade faces whose three corners are
        // all shared. We follow the same convention with local Vec<bool>
        // since rev1's static is per-pair anyway.
        let n1 = model1.num_points as usize;
        let n2 = model2.num_points as usize;
        let mut share_map1 = vec![false; n1];
        let mut share_map2 = vec![false; n2];
        let mut paired = 0i32;

        // Take ownership of model2's normals so we can read them while
        // mutating shared_point_normal on the same model. Restored at
        // the end.
        let model2_normals = model2.point_normal.take().expect("calc set point_normal");
        let model1_normals = model1.point_normal.take().expect("calc set point_normal");

        // Capture model2 invariants we need inside the inner loop so we
        // can borrow shared_point_normal mutably without aliasing.
        let m2_min_x = model2.min_x;
        let m2_max_x = model2.max_x;
        let m2_max_y = model2.max_y;
        let m2_min_z = model2.min_z;
        let m2_max_z = model2.max_z;
        let m2_px = model2.point_x.clone();
        let m2_py = model2.point_y.clone();
        let m2_pz = model2.point_z.clone();

        let m1_shared = model1.shared_point_normal.as_mut().unwrap();
        let m2_shared = model2.shared_point_normal.as_mut().unwrap();

        for p1 in 0..n1 {
            let n1_p = model1_normals[p1];
            if n1_p.w == 0 { continue; }
            let y_in2 = model1.point_y[p1] - ay;
            if y_in2 > m2_max_y { continue; }
            let x_in2 = model1.point_x[p1] - ax;
            if x_in2 < m2_min_x || x_in2 > m2_max_x { continue; }
            let z_in2 = model1.point_z[p1] - az;
            // VERBATIM PORT of Java's `var13 < model2.maxZ ||
            // var13 > model2.minZ` (ModelUnlit.java:1562). The operators
            // look inverted compared to the X check on the line above
            // (which uses min < ... > max) — almost certainly a Jagex
            // bug: with normal min<max bounds, this condition rejects
            // EVERY vertex except those at exactly min_z == max_z (a
            // degenerate model). Net effect: most calls to share_light
            // contribute nothing to shared normals. We mirror this
            // faithfully so the lit output matches Java 1:1; revisit
            // after the rev1 client has been verified against gamepacks.

            for p2 in 0..n2 {
                let n2_p = model2_normals[p2];
                if m2_px[p2] != x_in2 || m2_pz[p2] != z_in2 || m2_py[p2] != y_in2 || n2_p.w == 0 {
                    continue;
                }
                // Accumulate model2's normal into model1's shared slot.
                let s1 = m1_shared[p1].get_or_insert(n1_p);
                s1.x += n2_p.x;
                s1.y += n2_p.y;
                s1.z += n2_p.z;
                s1.w += n2_p.w;
                // And vice versa.
                let s2 = m2_shared[p2].get_or_insert(n2_p);
                s2.x += n1_p.x;
                s2.y += n1_p.y;
                s2.z += n1_p.z;
                s2.w += n1_p.w;
                paired += 1;
                share_map1[p1] = true;
                share_map2[p2] = true;
            }
        }

        model1.point_normal = Some(model1_normals);
        model2.point_normal = Some(model2_normals);

        // Java's `arg5` post-pass: a face whose three vertices ALL got
        // shared becomes faceRenderType = 2 (smooth, no fill). Only the
        // ground-decor adjacency calls request this (`true`); walls pass
        // `false` and keep their original render type.
        if paired >= 3 && mark_for_type_2 {
            fn upgrade_faces(m: &mut ModelUnlit, share_map: &[bool]) {
                let n = m.num_faces as usize;
                if m.face_render_type.is_none() {
                    m.face_render_type = Some(vec![0i8; n]);
                }
                let rt = m.face_render_type.as_mut().unwrap();
                for f in 0..n {
                    let a = m.face_vertex_a[f] as usize;
                    let b = m.face_vertex_b[f] as usize;
                    let c = m.face_vertex_c[f] as usize;
                    if share_map[a] && share_map[b] && share_map[c] {
                        rt[f] = 2;
                    }
                }
            }
            upgrade_faces(model1, &share_map1);
            upgrade_faces(model2, &share_map2);
        }
    }

    // @ObfuscatedName("fw.av()V") — ModelUnlit.calculateNormals
    //
    // Sums each face's normal into its three vertex slots so light()
    // can produce smooth Gouraud shading. faceRenderType 0 → contribute
    // to point normals, 1 → store on the face normal table only (flat
    // shaded face).
    pub fn calculate_normals(&mut self) {
        if self.point_normal.is_some() { return; }
        let mut pn = vec![PointNormal::default(); self.num_points as usize];
        let mut fn_table: Option<Vec<FaceNormal>> = None;
        for f in 0..self.num_faces as usize {
            let a = self.face_vertex_a[f] as usize;
            let b = self.face_vertex_b[f] as usize;
            let c = self.face_vertex_c[f] as usize;
            let dx_ab = self.point_x[b] - self.point_x[a];
            let dy_ab = self.point_y[b] - self.point_y[a];
            let dz_ab = self.point_z[b] - self.point_z[a];
            let dx_ac = self.point_x[c] - self.point_x[a];
            let dy_ac = self.point_y[c] - self.point_y[a];
            let dz_ac = self.point_z[c] - self.point_z[a];
            let mut nx = dy_ab * dz_ac - dz_ab * dy_ac;
            let mut ny = dz_ab * dx_ac - dx_ab * dz_ac;
            let mut nz = dx_ab * dy_ac - dy_ab * dx_ac;
            // Java's overflow scaler — keep normals in the 8192 range
            // so the per-vertex sums don't overflow.
            while nx > 8192 || ny > 8192 || nz > 8192 || nx < -8192 || ny < -8192 || nz < -8192 {
                nx >>= 1; ny >>= 1; nz >>= 1;
            }
            let len = (((nx as i64).pow(2) + (ny as i64).pow(2) + (nz as i64).pow(2)) as f64).sqrt() as i32;
            let len = len.max(1);
            let var16 = nx * 256 / len;
            let var17 = ny * 256 / len;
            let var18 = nz * 256 / len;
            let ftype = self.face_render_type.as_ref().map_or(0, |v| v[f]);
            if ftype == 0 {
                let n = &mut pn[a];
                n.x += var16; n.y += var17; n.z += var18; n.w += 1;
                let n = &mut pn[b];
                n.x += var16; n.y += var17; n.z += var18; n.w += 1;
                let n = &mut pn[c];
                n.x += var16; n.y += var17; n.z += var18; n.w += 1;
            } else if ftype == 1 {
                if fn_table.is_none() {
                    fn_table = Some(vec![FaceNormal::default(); self.num_faces as usize]);
                }
                let fnt = fn_table.as_mut().unwrap();
                fnt[f] = FaceNormal { x: var16, y: var17, z: var18 };
            }
        }
        self.point_normal = Some(pn);
        if fn_table.is_some() { self.face_normal = fn_table; }
    }

    // @ObfuscatedName("fw.aa(SS)V") — ModelUnlit.recolour
    pub fn recolour(&mut self, src: i16, dst: i16) {
        for i in 0..self.num_faces as usize {
            if self.face_colour[i] == src {
                self.face_colour[i] = dst;
            }
        }
    }

    // @ObfuscatedName("fw.as(SS)V") — ModelUnlit.retexture
    pub fn retexture(&mut self, src: i16, dst: i16) {
        let Some(tex) = self.face_texture_id.as_mut() else { return };
        for i in 0..self.num_faces as usize {
            if tex[i] == src { tex[i] = dst; }
        }
    }

    // @ObfuscatedName("fw.am()V") — ModelUnlit.mirror.
    //
    // Verbatim port of Java's mirror at ModelUnlit.java:1376-1386:
    // negate Z (NOT X) and swap face vertex A/C. Mirroring on Z keeps
    // the model's "depth" but flips its sidedness — used for the
    // mirror_xor path in fetch_loc_model when a loc's shape rotation
    // exceeds 3 (the rev1 cache stores wall-decor sprites at half
    // rotations and the renderer mirrors them in for the other half).
    // Previous port negated X by mistake — visible as inverted seams
    // on walls/decor at rotation > 3.
    pub fn mirror(&mut self) {
        for i in 0..self.num_points as usize {
            self.point_z[i] = -self.point_z[i];
        }
        for i in 0..self.num_faces as usize {
            let a = self.face_vertex_a[i];
            self.face_vertex_a[i] = self.face_vertex_c[i];
            self.face_vertex_c[i] = a;
        }
    }

    // @ObfuscatedName("fw.ak()V") — ModelUnlit.geometryChanged.
    // Verbatim port of ModelUnlit.java:1481-1486. Drops every cached
    // normal table and forces a bounds recompute on the next render
    // tick — called after any transform that invalidates the cached
    // normals (e.g. after a vertex rewrite during animation playback).
    pub fn geometry_changed(&mut self) {
        self.point_normal = None;
        self.shared_point_normal = None;
        self.face_normal = None;
        self.bounds_calculated = false;
    }

    // @ObfuscatedName("fw.ab()V") — ModelUnlit.prepareAnim. Verbatim
    // port of ModelUnlit.java:1247-1293. Transposes `vertex_label[]`
    // and `face_label[]` (one label-id per point/face) into
    // `label_vertices[label_id]` / `label_faces[label_id]` arrays so
    // the animation player can iterate every point or face on a given
    // bone in O(1). After the transpose Java nulls out the per-element
    // label arrays since they're never read again.
    pub fn prepare_anim(&mut self) {
        if let Some(labels) = self.vertex_label.take() {
            let mut counts = [0i32; 256];
            let mut max_label = 0i32;
            for v in 0..(self.num_points as usize) {
                let lbl = labels[v];
                counts[lbl as usize] += 1;
                if lbl > max_label { max_label = lbl; }
            }
            let mut buckets: Vec<Vec<i32>> = (0..=max_label as usize)
                .map(|lbl| Vec::with_capacity(counts[lbl] as usize))
                .collect();
            for v in 0..(self.num_points as usize) {
                let lbl = labels[v] as usize;
                buckets[lbl].push(v as i32);
            }
            self.label_vertices = Some(buckets);
        }

        if let Some(labels) = self.face_label.take() {
            let mut counts = [0i32; 256];
            let mut max_label = 0i32;
            for f in 0..(self.num_faces as usize) {
                let lbl = labels[f];
                counts[lbl as usize] += 1;
                if lbl > max_label { max_label = lbl; }
            }
            let mut buckets: Vec<Vec<i32>> = (0..=max_label as usize)
                .map(|lbl| Vec::with_capacity(counts[lbl] as usize))
                .collect();
            for f in 0..(self.num_faces as usize) {
                let lbl = labels[f] as usize;
                buckets[lbl].push(f as i32);
            }
            self.label_faces = Some(buckets);
        }
    }

    // @ObfuscatedName("fw.h()V") — ModelUnlit.rotate90.
    // Rotates the model 90° CW around Y so a wall along the model's
    // "west" edge moves to the "north" edge.
    pub fn rotate90(&mut self) {
        for i in 0..self.num_points as usize {
            let temp = self.point_x[i];
            self.point_x[i] = self.point_z[i];
            self.point_z[i] = -temp;
        }
    }
    // @ObfuscatedName("fw.x()V") — ModelUnlit.rotate180.
    pub fn rotate180(&mut self) {
        for i in 0..self.num_points as usize {
            self.point_x[i] = -self.point_x[i];
            self.point_z[i] = -self.point_z[i];
        }
    }
    // @ObfuscatedName("fw.p()V") — ModelUnlit.rotate270.
    pub fn rotate270(&mut self) {
        for i in 0..self.num_points as usize {
            let temp = self.point_x[i];
            self.point_x[i] = -self.point_z[i];
            self.point_z[i] = temp;
        }
    }
    // @ObfuscatedName("fw.ac(III)V") — ModelUnlit.translate.
    pub fn translate(&mut self, dx: i32, dy: i32, dz: i32) {
        for i in 0..self.num_points as usize {
            self.point_x[i] += dx;
            self.point_y[i] += dy;
            self.point_z[i] += dz;
        }
    }
    // @ObfuscatedName("fw.ad(I)V") — ModelUnlit.rotateXAxis.
    //
    // Verbatim port of Java's rotation at ModelUnlit.java:1329-1337.
    // Despite the name "rotateXAxis", Java actually rotates the X and
    // Z components (i.e. rotates AROUND the Y axis). We preserve that
    // — Jagex's naming is misleading but the only caller (wall-decor
    // kind 4 with rotation > 3, scene.rs) depends on this exact tilt.
    // Raw `sinTable[arg0]` indexing (no `& 0x7FF` mask) — callers
    // pass angles within [0, 2048).
    pub fn rotate_x_axis(&mut self, theta: i32) {
        let sin = crate::dash3d::pix3d::sin_table();
        let cos = crate::dash3d::pix3d::cos_table();
        let s = sin[theta as usize];
        let c = cos[theta as usize];
        for i in 0..self.num_points as usize {
            let new_x = (self.point_z[i] * s + self.point_x[i] * c) >> 16;
            self.point_z[i] = (self.point_z[i] * c - self.point_x[i] * s) >> 16;
            self.point_x[i] = new_x;
        }
    }

    // @ObfuscatedName("fw.ap(III)V") — ModelUnlit.resize
    pub fn resize(&mut self, sx: i32, sy: i32, sz: i32) {
        for i in 0..self.num_points as usize {
            self.point_x[i] = self.point_x[i] * sx / 128;
            self.point_y[i] = self.point_y[i] * sy / 128;
            self.point_z[i] = self.point_z[i] * sz / 128;
        }
    }

    // @ObfuscatedName("fw.t([B)V") — ModelUnlit.loadOb2Engine200
    pub fn load_ob2(&mut self, src: Vec<u8>) {
        let len = src.len() as i32;
        let mut trailer = Packet::from_vec(src.clone());
        trailer.pos = len - 18;
        let num_points = trailer.g2();
        let num_faces = trailer.g2();
        let num_t = trailer.g1();
        let has_face_info = trailer.g1();
        let priority = trailer.g1();
        let has_face_alpha = trailer.g1();
        let has_face_labels = trailer.g1();
        let has_vertex_labels = trailer.g1();
        let data_len_x = trailer.g2();
        let data_len_y = trailer.g2();
        let data_len_z = trailer.g2();
        let data_len_face_index = trailer.g2();

        let mut pos = 0i32;
        let vertex_order_off = pos; pos += num_points;
        let face_index_order_off = pos; pos += num_faces;
        let face_priority_off = pos;
        if priority == 255 { pos += num_faces; }
        let face_label_off = pos;
        if has_face_labels == 1 { pos += num_faces; }
        let face_info_off = pos;
        if has_face_info == 1 { pos += num_faces; }
        let vertex_label_off = pos;
        if has_vertex_labels == 1 { pos += num_points; }
        let face_alpha_off = pos;
        if has_face_alpha == 1 { pos += num_faces; }
        let face_index_off = pos; pos += data_len_face_index;
        let face_colour_off = pos; pos += num_faces * 2;
        let face_texture_axis_off = pos; pos += num_t * 6;
        let vertex_x_off = pos; pos += data_len_x;
        let vertex_y_off = pos; pos += data_len_y;
        let vertex_z_off = pos;
        let _ = vertex_z_off;

        self.num_points = num_points;
        self.num_faces = num_faces;
        self.num_t = num_t;
        self.point_x = vec![0i32; num_points as usize];
        self.point_y = vec![0i32; num_points as usize];
        self.point_z = vec![0i32; num_points as usize];
        self.face_vertex_a = vec![0i32; num_faces as usize];
        self.face_vertex_b = vec![0i32; num_faces as usize];
        self.face_vertex_c = vec![0i32; num_faces as usize];
        if num_t > 0 {
            self.texture_render_type = Some(vec![0i8; num_t as usize]);
            self.face_texture_p = Some(vec![0i16; num_t as usize]);
            self.face_texture_m = Some(vec![0i16; num_t as usize]);
            self.face_texture_n = Some(vec![0i16; num_t as usize]);
        }
        if has_vertex_labels == 1 {
            self.vertex_label = Some(vec![0i32; num_points as usize]);
        }
        if has_face_info == 1 {
            self.face_render_type = Some(vec![0i8; num_faces as usize]);
            self.face_texture_axis = Some(vec![0i8; num_faces as usize]);
            self.face_texture_id = Some(vec![0i16; num_faces as usize]);
        }
        if priority == 255 {
            self.face_priority = Some(vec![0i8; num_faces as usize]);
        } else {
            self.priority = priority as i8;
        }
        if has_face_alpha == 1 {
            self.face_alpha = Some(vec![0i8; num_faces as usize]);
        }
        if has_face_labels == 1 {
            self.face_label = Some(vec![0i32; num_faces as usize]);
        }
        self.face_colour = vec![0i16; num_faces as usize];

        // ── Vertex decode ───────────────────────────────────────────
        let mut point1 = Packet::from_vec(src.clone()); point1.pos = vertex_order_off;
        let mut point2 = Packet::from_vec(src.clone()); point2.pos = vertex_x_off;
        let mut point3 = Packet::from_vec(src.clone()); point3.pos = vertex_y_off;
        let mut point4 = Packet::from_vec(src.clone()); point4.pos = vertex_z_off;
        let mut point5 = Packet::from_vec(src.clone()); point5.pos = vertex_label_off;
        let mut dx = 0i32; let mut dy = 0i32; let mut dz = 0i32;
        for i in 0..num_points as usize {
            let order = point1.g1();
            let x = if order & 0x1 != 0 { point2.gsmarts() } else { 0 };
            let y = if order & 0x2 != 0 { point3.gsmarts() } else { 0 };
            let z = if order & 0x4 != 0 { point4.gsmarts() } else { 0 };
            self.point_x[i] = dx + x;
            self.point_y[i] = dy + y;
            self.point_z[i] = dz + z;
            dx = self.point_x[i]; dy = self.point_y[i]; dz = self.point_z[i];
            if has_vertex_labels == 1 {
                self.vertex_label.as_mut().unwrap()[i] = point5.g1();
            }
        }

        // ── Face attribute decode ───────────────────────────────────
        let mut face1 = Packet::from_vec(src.clone()); face1.pos = face_colour_off;
        let mut face2 = Packet::from_vec(src.clone()); face2.pos = face_info_off;
        let mut face3 = Packet::from_vec(src.clone()); face3.pos = face_priority_off;
        let mut face4 = Packet::from_vec(src.clone()); face4.pos = face_alpha_off;
        let mut face5 = Packet::from_vec(src.clone()); face5.pos = face_label_off;
        let mut has_render_type = false;
        let mut has_texture_id = false;
        for f in 0..num_faces as usize {
            self.face_colour[f] = face1.g2() as i16;
            if has_face_info == 1 {
                let v = face2.g1();
                if (v & 0x1) == 1 {
                    self.face_render_type.as_mut().unwrap()[f] = 1;
                    has_render_type = true;
                } else {
                    self.face_render_type.as_mut().unwrap()[f] = 0;
                }
                if (v & 0x2) == 2 {
                    self.face_texture_axis.as_mut().unwrap()[f] = (v >> 2) as i8;
                    self.face_texture_id.as_mut().unwrap()[f] = self.face_colour[f];
                    self.face_colour[f] = 127;
                    if self.face_texture_id.as_ref().unwrap()[f] != -1 { has_texture_id = true; }
                } else {
                    self.face_texture_axis.as_mut().unwrap()[f] = -1;
                    self.face_texture_id.as_mut().unwrap()[f] = -1;
                }
            }
            if priority == 255 {
                self.face_priority.as_mut().unwrap()[f] = face3.g1b();
            }
            if has_face_alpha == 1 {
                self.face_alpha.as_mut().unwrap()[f] = face4.g1b();
            }
            if has_face_labels == 1 {
                self.face_label.as_mut().unwrap()[f] = face5.g1();
            }
        }

        // ── Face index (vertex-triple) ──────────────────────────────
        let mut vertex1 = Packet::from_vec(src.clone()); vertex1.pos = face_index_off;
        let mut vertex2 = Packet::from_vec(src.clone()); vertex2.pos = face_index_order_off;
        let mut a = 0i32; let mut b = 0i32; let mut c = 0i32; let mut last = 0i32;
        for f in 0..num_faces as usize {
            let order = vertex2.g1();
            match order {
                1 => {
                    a = vertex1.gsmarts() + last;
                    b = vertex1.gsmarts() + a;
                    c = vertex1.gsmarts() + b;
                    last = c;
                }
                2 => {
                    b = c;
                    c = vertex1.gsmarts() + last;
                    last = c;
                }
                3 => {
                    a = c;
                    c = vertex1.gsmarts() + last;
                    last = c;
                }
                4 => {
                    let tmp = a;
                    a = b;
                    b = tmp;
                    c = vertex1.gsmarts() + last;
                    last = c;
                }
                _ => {}
            }
            self.face_vertex_a[f] = a;
            self.face_vertex_b[f] = b;
            self.face_vertex_c[f] = c;
        }

        let mut axis = Packet::from_vec(src); axis.pos = face_texture_axis_off;
        for f in 0..num_t as usize {
            self.texture_render_type.as_mut().unwrap()[f] = 0;
            self.face_texture_p.as_mut().unwrap()[f] = axis.g2() as i16;
            self.face_texture_m.as_mut().unwrap()[f] = axis.g2() as i16;
            self.face_texture_n.as_mut().unwrap()[f] = axis.g2() as i16;
        }

        // Java's post-decode pass at ModelUnlit.java:783-798 — for each
        // face whose texture axis points at a P/M/N triplet that's
        // identical to the face's own (a,b,c) vertex triple, set
        // axis[f] = -1 (the rasterizer's "use own vertices" sentinel).
        // If after the pass NO face still has a real texture axis,
        // null the whole array. Purely an optimisation today — the
        // renderer's `if axis < 0 { (a,b,c) }` branch already handles
        // -1 entries — but mirrors Java verbatim for diff stability.
        let mut clear_axis = false;
        if let Some(axis) = self.face_texture_axis.as_mut() {
            let mut has_texture = false;
            for f in 0..num_faces as usize {
                let slot = (axis[f] as i32) & 0xFF;
                if slot == 255 {
                    continue;
                }
                // Java only dereferences faceTextureP/M/N inside this
                // branch — models with axis bytes but numT == 0 leave
                // the arrays null and every slot at 255. A non-255
                // slot with no triplet arrays (or out of range) is
                // inconsistent data; fall back to the face's own
                // vertices like the rasterizer's axis<0 path.
                let p = self.face_texture_p.as_ref()
                    .and_then(|v| v.get(slot as usize))
                    .map(|&v| v as i32 & 0xFFFF);
                let m = self.face_texture_m.as_ref()
                    .and_then(|v| v.get(slot as usize))
                    .map(|&v| v as i32 & 0xFFFF);
                let n = self.face_texture_n.as_ref()
                    .and_then(|v| v.get(slot as usize))
                    .map(|&v| v as i32 & 0xFFFF);
                match (p, m, n) {
                    (Some(p), Some(m), Some(n)) => {
                        if p == self.face_vertex_a[f]
                            && m == self.face_vertex_b[f]
                            && n == self.face_vertex_c[f]
                        {
                            axis[f] = -1;
                        } else {
                            has_texture = true;
                        }
                    }
                    _ => {
                        axis[f] = -1;
                    }
                }
            }
            if !has_texture {
                clear_axis = true;
            }
        }
        if clear_axis {
            self.face_texture_axis = None;
        }
        if !has_texture_id { self.face_texture_id = None; }
        if !has_render_type { self.face_render_type = None; }
    }

    // @ObfuscatedName("fw.k()Lfw;") — ModelUnlit.copyForShareLight.
    // Verbatim port of ModelUnlit.java:1090-1145. Returns a shallow
    // clone that shares vertex / face / texture / normal arrays with
    // the source but owns a fresh `face_render_type` buffer — the
    // caller (shareLight) needs to mutate render types without
    // disturbing the donor model.
    pub fn copy_for_share_light(&self) -> Self {
        let mut copy = Self::default();
        if let Some(frt) = &self.face_render_type {
            copy.face_render_type = Some(frt.clone());
        }
        copy.num_points = self.num_points;
        copy.num_faces = self.num_faces;
        copy.num_t = self.num_t;
        copy.point_x = self.point_x.clone();
        copy.point_y = self.point_y.clone();
        copy.point_z = self.point_z.clone();
        copy.face_vertex_a = self.face_vertex_a.clone();
        copy.face_vertex_b = self.face_vertex_b.clone();
        copy.face_vertex_c = self.face_vertex_c.clone();
        copy.face_priority = self.face_priority.clone();
        copy.face_alpha = self.face_alpha.clone();
        copy.face_texture_axis = self.face_texture_axis.clone();
        copy.face_colour = self.face_colour.clone();
        copy.face_texture_id = self.face_texture_id.clone();
        copy.priority = self.priority;
        copy.texture_render_type = self.texture_render_type.clone();
        copy.face_texture_p = self.face_texture_p.clone();
        copy.face_texture_m = self.face_texture_m.clone();
        copy.face_texture_n = self.face_texture_n.clone();
        copy.texture_scale_x = self.texture_scale_x.clone();
        copy.texture_scale_y = self.texture_scale_y.clone();
        copy.texture_scale_z = self.texture_scale_z.clone();
        copy.texture_rotation = self.texture_rotation.clone();
        copy.texture_translation = self.texture_translation.clone();
        copy.texture_speed = self.texture_speed.clone();
        copy.texture_direction = self.texture_direction.clone();
        copy.vertex_label = self.vertex_label.clone();
        copy.face_label = self.face_label.clone();
        copy.label_vertices = self.label_vertices.clone();
        copy.label_faces = self.label_faces.clone();
        copy.point_normal = self.point_normal.clone();
        copy.face_normal = self.face_normal.clone();
        copy.ambient = self.ambient;
        copy.contrast = self.contrast;
        copy
    }

    // @ObfuscatedName("fw.al(I)I") — ModelUnlit.getTexLight. Verbatim
    // port of ModelUnlit.java:1828-1834. Clamps an HSL intensity to
    // the [2, 126] range used by texture-lit faces.
    pub fn get_tex_light(arg0: i32) -> i32 {
        if arg0 < 2 { 2 }
        else if arg0 > 126 { 126 }
        else { arg0 }
    }

    // @ObfuscatedName("fw.ay(II)I") — ModelUnlit.getColour. Verbatim
    // port of ModelUnlit.java:1816-1823. HSL face colour modulation:
    // multiplies the luminance nibble (low 7 bits) by an intensity
    // factor (>> 7), clamps to [2, 126], then preserves the hue/sat
    // high bits. Used by the light() pass — same shape as the
    // texture-lit fast path in get_tex_light but operating on a
    // packed HSL16 colour rather than a bare intensity.
    pub fn get_colour(arg0: i32, arg1: i32) -> i32 {
        let mut var2 = ((arg0 & 0x7F) * arg1) >> 7;
        if var2 < 2 { var2 = 2; }
        else if var2 > 126 { var2 = 126; }
        (arg0 & 0xFF80) + var2
    }
}
