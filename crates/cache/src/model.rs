//! 3D model meshes from archive 7. Port of `jagex3.dash3d.ModelUnlit`.
//!
//! Two formats coexist in the rev1 cache:
//!
//! * **Ob3** — newer, indicated by the last 2 bytes being `[0xFF, 0xFF]`. Richer texture
//!   metadata (multiple texture render types: simple/complex/cube).
//! * **Ob2** — legacy. Anything not matching the Ob3 sentinel.
//!
//! Both decoders use the same trick: a "trailer" header at the END of the file giving counts
//! and chunk lengths, with the body containing multiple interleaved data streams that get
//! read via separate `Packet` cursors at known offsets. Vertex coords are delta-encoded;
//! face vertex indices use an "order" code (1/2/3/4) that compresses sequential triangles.
//!
//! Server uses this for bounding-box / collision computation per loc shape variant. The
//! `Model::bounds` method computes axis-aligned bounds from the decoded vertices on demand;
//! we don't pre-compute since most loaded models aren't queried for bounds.

use io::Packet;

#[derive(Debug, Default, Clone)]
pub struct Model {
    pub num_points: i32,
    pub num_faces: i32,
    pub num_t: i32,

    pub point_x: Vec<i32>,
    pub point_y: Vec<i32>,
    pub point_z: Vec<i32>,

    pub face_vertex_a: Vec<i32>,
    pub face_vertex_b: Vec<i32>,
    pub face_vertex_c: Vec<i32>,

    pub face_render_type: Option<Vec<i8>>,
    pub face_priority: Option<Vec<i8>>,
    pub face_alpha: Option<Vec<i8>>,
    pub face_texture_axis: Option<Vec<i8>>,
    pub face_colour: Vec<i16>,
    pub face_texture_id: Option<Vec<i16>>,
    /// Single priority byte when `face_priority` is `None`.
    pub priority: i8,

    pub texture_render_type: Option<Vec<i8>>,
    pub face_texture_p: Option<Vec<i16>>,
    pub face_texture_m: Option<Vec<i16>>,
    pub face_texture_n: Option<Vec<i16>>,
    pub texture_scale_x: Option<Vec<i16>>,
    pub texture_scale_y: Option<Vec<i16>>,
    pub texture_scale_z: Option<Vec<i16>>,
    pub texture_rotation: Option<Vec<i16>>,
    pub texture_speed: Option<Vec<i16>>,
    pub texture_direction: Option<Vec<i16>>,
    pub texture_translation: Option<Vec<i8>>,

    pub vertex_label: Option<Vec<i32>>,
    pub face_label: Option<Vec<i32>>,
}

impl Model {
    /// Decode a model from its raw bytes. Auto-selects Ob2 vs Ob3 based on the trailing
    /// sentinel.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Self {
        let mut m = Self::default();
        let n = bytes.len();
        if n >= 2 && bytes[n - 1] as u8 == 0xFF && bytes[n - 2] as u8 == 0xFF {
            m.load_ob3(bytes);
        } else {
            m.load_ob2(bytes);
        }
        m
    }

    /// Axis-aligned bounding box. Returns `None` for empty models.
    #[must_use]
    pub fn bounds(&self) -> Option<((i32, i32, i32), (i32, i32, i32))> {
        if self.point_x.is_empty() {
            return None;
        }
        let mut min = (i32::MAX, i32::MAX, i32::MAX);
        let mut max = (i32::MIN, i32::MIN, i32::MIN);
        for i in 0..self.point_x.len() {
            let (x, y, z) = (self.point_x[i], self.point_y[i], self.point_z[i]);
            if x < min.0 { min.0 = x; }  if x > max.0 { max.0 = x; }
            if y < min.1 { min.1 = y; }  if y > max.1 { max.1 = y; }
            if z < min.2 { min.2 = z; }  if z > max.2 { max.2 = z; }
        }
        Some((min, max))
    }

    fn load_ob3(&mut self, src: &[u8]) {
        let mk = || Packet::from_vec(src.to_vec());
        let (mut p1, mut p3, mut p4, mut p5, mut p6, mut p7, mut p8) =
            (mk(), mk(), mk(), mk(), mk(), mk(), mk());

        p1.pos = src.len() - 23;
        let num_points = p1.g2();
        let num_faces = p1.g2();
        let num_t = p1.g1();

        let has_face_render_type = p1.g1(); // var12
        let has_priorities = p1.g1();
        let has_face_alpha = p1.g1(); // var14
        let has_face_labels = p1.g1(); // var15
        let has_face_texture_id = p1.g1(); // var16
        let has_vertex_labels = p1.g1(); // var17
        let data_len_x = p1.g2(); // var18
        let data_len_y = p1.g2(); // var19
        let data_len_z = p1.g2(); // var20
        let data_len_face_index = p1.g2(); // var21
        let data_len_face_axis = p1.g2(); // var22

        // Count textures per render type.
        let mut simple_textures = 0;
        let mut complex_textures = 0;
        let mut cube_textures = 0;
        if num_t > 0 {
            let mut types = vec![0i8; num_t as usize];
            p1.pos = 0;
            for slot in types.iter_mut().take(num_t as usize) {
                *slot = p1.g1b();
                match *slot {
                    0 => simple_textures += 1,
                    1..=3 => {
                        complex_textures += 1;
                        if *slot == 2 {
                            cube_textures += 1;
                        }
                    }
                    _ => {}
                }
            }
            self.texture_render_type = Some(types);
        }

        // Compute the offset table — each stream sits at a fixed byte position in `src`.
        let mut o = num_points + num_t;
        let off_render_type = o;
        if has_face_render_type == 1 { o += num_faces; }
        let off_face_index_order = o;
        o += num_faces;
        let off_face_priority = o;
        if has_priorities == 255 { o += num_faces; }
        let off_face_label = o;
        if has_face_labels == 1 { o += num_faces; }
        let off_vertex_label = o;
        if has_vertex_labels == 1 { o += num_points; }
        let off_face_alpha = o;
        if has_face_alpha == 1 { o += num_faces; }
        let off_face_index = o;
        o += data_len_face_index;
        let off_face_texture_id = o;
        if has_face_texture_id == 1 { o += num_faces * 2; }
        let off_face_texture_axis = o;
        o += data_len_face_axis;
        let off_face_colour = o;
        o += num_faces * 2;
        let off_vertex_x = o;
        o += data_len_x;
        let off_vertex_y = o;
        o += data_len_y;
        let off_vertex_z = o;
        o += data_len_z;
        let off_simple_tex = o;
        o += simple_textures * 6;
        let off_complex_p = o;
        o += complex_textures * 6;
        let off_complex_scale = o;
        o += complex_textures * 6;
        let off_complex_rot = o;
        o += complex_textures * 2;
        let off_complex_trans = o;
        o += complex_textures;
        let off_complex_speed = o;
        // Remaining bytes are cube texture directions + a trailing version byte (var84).

        self.num_points = num_points;
        self.num_faces = num_faces;
        self.num_t = num_t;
        self.point_x = vec![0; num_points as usize];
        self.point_y = vec![0; num_points as usize];
        self.point_z = vec![0; num_points as usize];
        self.face_vertex_a = vec![0; num_faces as usize];
        self.face_vertex_b = vec![0; num_faces as usize];
        self.face_vertex_c = vec![0; num_faces as usize];

        if has_vertex_labels == 1 { self.vertex_label = Some(vec![0; num_points as usize]); }
        if has_face_render_type == 1 { self.face_render_type = Some(vec![0; num_faces as usize]); }
        if has_priorities == 255 {
            self.face_priority = Some(vec![0; num_faces as usize]);
        } else {
            self.priority = has_priorities as i8;
        }
        if has_face_alpha == 1 { self.face_alpha = Some(vec![0; num_faces as usize]); }
        if has_face_labels == 1 { self.face_label = Some(vec![0; num_faces as usize]); }
        if has_face_texture_id == 1 { self.face_texture_id = Some(vec![0; num_faces as usize]); }
        if has_face_texture_id == 1 && num_t > 0 {
            self.face_texture_axis = Some(vec![0; num_faces as usize]);
        }
        self.face_colour = vec![0; num_faces as usize];
        if num_t > 0 {
            self.face_texture_p = Some(vec![0; num_t as usize]);
            self.face_texture_m = Some(vec![0; num_t as usize]);
            self.face_texture_n = Some(vec![0; num_t as usize]);
            if complex_textures > 0 {
                self.texture_scale_x = Some(vec![0; complex_textures as usize]);
                self.texture_scale_y = Some(vec![0; complex_textures as usize]);
                self.texture_scale_z = Some(vec![0; complex_textures as usize]);
                self.texture_rotation = Some(vec![0; complex_textures as usize]);
                self.texture_translation = Some(vec![0; complex_textures as usize]);
                self.texture_speed = Some(vec![0; complex_textures as usize]);
            }
            if cube_textures > 0 {
                self.texture_direction = Some(vec![0; cube_textures as usize]);
            }
        }

        // Decode vertices.
        p1.pos = num_t as usize;
        p3.pos = off_vertex_x as usize;
        p4.pos = off_vertex_y as usize;
        p5.pos = off_vertex_z as usize;
        p6.pos = off_vertex_label as usize;

        let mut last_x = 0i32;
        let mut last_y = 0i32;
        let mut last_z = 0i32;
        for v in 0..num_points as usize {
            let order = p1.g1();
            let dx = if order & 0x1 != 0 { p3.gsmarts() } else { 0 };
            let dy = if order & 0x2 != 0 { p4.gsmarts() } else { 0 };
            let dz = if order & 0x4 != 0 { p5.gsmarts() } else { 0 };
            self.point_x[v] = last_x + dx;
            self.point_y[v] = last_y + dy;
            self.point_z[v] = last_z + dz;
            last_x = self.point_x[v];
            last_y = self.point_y[v];
            last_z = self.point_z[v];
            if let Some(vl) = &mut self.vertex_label {
                vl[v] = p6.g1();
            }
        }

        // Decode per-face data.
        p1.pos = off_face_colour as usize;
        p3.pos = off_render_type as usize;
        p4.pos = off_face_priority as usize;
        p5.pos = off_face_alpha as usize;
        p6.pos = off_face_label as usize;
        p7.pos = off_face_texture_id as usize;
        p8.pos = off_face_texture_axis as usize;

        for f in 0..num_faces as usize {
            self.face_colour[f] = p1.g2() as i16;
            if let Some(rt) = &mut self.face_render_type { rt[f] = p3.g1b(); }
            if let Some(fp) = &mut self.face_priority { fp[f] = p4.g1b(); }
            if let Some(fa) = &mut self.face_alpha { fa[f] = p5.g1b(); }
            if let Some(fl) = &mut self.face_label { fl[f] = p6.g1(); }
            if let Some(ft) = &mut self.face_texture_id {
                ft[f] = (p7.g2() - 1) as i16;
                if self.face_texture_axis.is_some() && ft[f] != -1
                    && let Some(fta) = &mut self.face_texture_axis
                {
                    fta[f] = (p8.g1() - 1) as i8;
                }
            }
        }

        // Face vertex indices via "order" codes 1..=4.
        p1.pos = off_face_index as usize;
        p3.pos = off_face_index_order as usize;
        let mut a = 0i32;
        let mut b = 0i32;
        let mut c = 0i32;
        let mut last = 0i32;
        for f in 0..num_faces as usize {
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
                _ => panic!("Model Ob3: unknown face-index order {order}"),
            }
            self.face_vertex_a[f] = a;
            self.face_vertex_b[f] = b;
            self.face_vertex_c[f] = c;
        }

        // Per-texture metadata (varies by type 0/1/2/3).
        p1.pos = off_simple_tex as usize;
        p3.pos = off_complex_p as usize;
        p4.pos = off_complex_scale as usize;
        p5.pos = off_complex_rot as usize;
        p6.pos = off_complex_trans as usize;
        p7.pos = off_complex_speed as usize;

        if let Some(types) = &self.texture_render_type {
            for t in 0..num_t as usize {
                let tt = types[t] as u8;
                match tt {
                    0 => {
                        let ftp = self.face_texture_p.as_mut().unwrap();
                        let ftm = self.face_texture_m.as_mut().unwrap();
                        let ftn = self.face_texture_n.as_mut().unwrap();
                        ftp[t] = p1.g2() as i16;
                        ftm[t] = p1.g2() as i16;
                        ftn[t] = p1.g2() as i16;
                    }
                    1 | 2 | 3 => {
                        let ftp = self.face_texture_p.as_mut().unwrap();
                        let ftm = self.face_texture_m.as_mut().unwrap();
                        let ftn = self.face_texture_n.as_mut().unwrap();
                        let sx = self.texture_scale_x.as_mut().unwrap();
                        let sy = self.texture_scale_y.as_mut().unwrap();
                        let sz = self.texture_scale_z.as_mut().unwrap();
                        let rot = self.texture_rotation.as_mut().unwrap();
                        let trn = self.texture_translation.as_mut().unwrap();
                        let spd = self.texture_speed.as_mut().unwrap();
                        ftp[t] = p3.g2() as i16;
                        ftm[t] = p3.g2() as i16;
                        ftn[t] = p3.g2() as i16;
                        sx[t] = p4.g2() as i16;
                        sy[t] = p4.g2() as i16;
                        sz[t] = p4.g2() as i16;
                        rot[t] = p5.g2() as i16;
                        trn[t] = p6.g1b();
                        spd[t] = p7.g2() as i16;
                        if tt == 2 {
                            let td = self.texture_direction.as_mut().unwrap();
                            td[t] = p7.g2() as i16;
                        }
                    }
                    _ => {}
                }
            }
        }

        // Skip the trailing "UnusedAJ" version block — purely informational.
        let _ = off_complex_speed;
    }

    fn load_ob2(&mut self, src: &[u8]) {
        let mut has_render_type = false;
        let mut has_texture_id = false;

        let mut trailer = Packet::from_vec(src.to_vec());
        trailer.pos = src.len() - 18;

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
        let off_vertex_order = pos; pos += num_points;
        let off_face_index_order = pos; pos += num_faces;
        let off_face_priority = pos;
        if priority == 255 { pos += num_faces; }
        let off_face_label = pos;
        if has_face_labels == 1 { pos += num_faces; }
        let off_face_info = pos;
        if has_face_info == 1 { pos += num_faces; }
        let off_vertex_label = pos;
        if has_vertex_labels == 1 { pos += num_points; }
        let off_face_alpha = pos;
        if has_face_alpha == 1 { pos += num_faces; }
        let off_face_index = pos; pos += data_len_face_index;
        let off_face_colour = pos; pos += num_faces * 2;
        let off_face_texture_axis = pos; pos += num_t * 6;
        let off_vertex_x = pos; pos += data_len_x;
        let off_vertex_y = pos; pos += data_len_y;
        let off_vertex_z = pos; pos += data_len_z;
        let _ = pos;

        self.num_points = num_points;
        self.num_faces = num_faces;
        self.num_t = num_t;
        self.point_x = vec![0; num_points as usize];
        self.point_y = vec![0; num_points as usize];
        self.point_z = vec![0; num_points as usize];
        self.face_vertex_a = vec![0; num_faces as usize];
        self.face_vertex_b = vec![0; num_faces as usize];
        self.face_vertex_c = vec![0; num_faces as usize];

        if num_t > 0 {
            self.texture_render_type = Some(vec![0; num_t as usize]);
            self.face_texture_p = Some(vec![0; num_t as usize]);
            self.face_texture_m = Some(vec![0; num_t as usize]);
            self.face_texture_n = Some(vec![0; num_t as usize]);
        }
        if has_vertex_labels == 1 {
            self.vertex_label = Some(vec![0; num_points as usize]);
        }
        if has_face_info == 1 {
            self.face_render_type = Some(vec![0; num_faces as usize]);
            self.face_texture_axis = Some(vec![0; num_faces as usize]);
            self.face_texture_id = Some(vec![0; num_faces as usize]);
        }
        if priority == 255 {
            self.face_priority = Some(vec![0; num_faces as usize]);
        } else {
            self.priority = priority as i8;
        }
        if has_face_alpha == 1 {
            self.face_alpha = Some(vec![0; num_faces as usize]);
        }
        if has_face_labels == 1 {
            self.face_label = Some(vec![0; num_faces as usize]);
        }
        self.face_colour = vec![0; num_faces as usize];

        let mk = || Packet::from_vec(src.to_vec());
        let (mut p_order, mut p_x, mut p_y, mut p_z, mut p_vl) = (mk(), mk(), mk(), mk(), mk());
        p_order.pos = off_vertex_order as usize;
        p_x.pos = off_vertex_x as usize;
        p_y.pos = off_vertex_y as usize;
        p_z.pos = off_vertex_z as usize;
        p_vl.pos = off_vertex_label as usize;

        let mut dx = 0i32;
        let mut dy = 0i32;
        let mut dz = 0i32;
        for v in 0..num_points as usize {
            let order = p_order.g1();
            let x = if order & 0x1 != 0 { p_x.gsmarts() } else { 0 };
            let y = if order & 0x2 != 0 { p_y.gsmarts() } else { 0 };
            let z = if order & 0x4 != 0 { p_z.gsmarts() } else { 0 };
            self.point_x[v] = dx + x;
            self.point_y[v] = dy + y;
            self.point_z[v] = dz + z;
            dx = self.point_x[v];
            dy = self.point_y[v];
            dz = self.point_z[v];
            if let Some(vl) = &mut self.vertex_label {
                vl[v] = p_vl.g1();
            }
        }

        let (mut p_colour, mut p_info, mut p_pri, mut p_alpha, mut p_label) =
            (mk(), mk(), mk(), mk(), mk());
        p_colour.pos = off_face_colour as usize;
        p_info.pos = off_face_info as usize;
        p_pri.pos = off_face_priority as usize;
        p_alpha.pos = off_face_alpha as usize;
        p_label.pos = off_face_label as usize;

        for f in 0..num_faces as usize {
            self.face_colour[f] = p_colour.g2() as i16;
            if has_face_info == 1 {
                let info = p_info.g1();
                let rt = self.face_render_type.as_mut().unwrap();
                let fta = self.face_texture_axis.as_mut().unwrap();
                let ft = self.face_texture_id.as_mut().unwrap();
                if info & 0x1 == 1 {
                    rt[f] = 1;
                    has_render_type = true;
                } else {
                    rt[f] = 0;
                }
                if info & 0x2 == 2 {
                    fta[f] = (info >> 2) as i8;
                    ft[f] = self.face_colour[f];
                    self.face_colour[f] = 127;
                    if ft[f] != -1 {
                        has_texture_id = true;
                    }
                } else {
                    fta[f] = -1;
                    ft[f] = -1;
                }
            }
            if priority == 255 { self.face_priority.as_mut().unwrap()[f] = p_pri.g1b(); }
            if has_face_alpha == 1 { self.face_alpha.as_mut().unwrap()[f] = p_alpha.g1b(); }
            if has_face_labels == 1 { self.face_label.as_mut().unwrap()[f] = p_label.g1(); }
        }

        let (mut p_fi, mut p_fio) = (mk(), mk());
        p_fi.pos = off_face_index as usize;
        p_fio.pos = off_face_index_order as usize;
        let mut a = 0i32;
        let mut b = 0i32;
        let mut c = 0i32;
        let mut last = 0i32;
        for f in 0..num_faces as usize {
            let order = p_fio.g1();
            match order {
                1 => {
                    a = p_fi.gsmarts() + last;
                    b = p_fi.gsmarts() + a;
                    c = p_fi.gsmarts() + b;
                    last = c;
                }
                2 => { b = c; c = p_fi.gsmarts() + last; last = c; }
                3 => { a = c; c = p_fi.gsmarts() + last; last = c; }
                4 => {
                    let tmp = a;
                    a = b;
                    b = tmp;
                    c = p_fi.gsmarts() + last;
                    last = c;
                }
                _ => panic!("Model Ob2: unknown face-index order {order}"),
            }
            self.face_vertex_a[f] = a;
            self.face_vertex_b[f] = b;
            self.face_vertex_c[f] = c;
        }

        let mut p_axis = mk();
        p_axis.pos = off_face_texture_axis as usize;
        for t in 0..num_t as usize {
            if let Some(rt) = &mut self.texture_render_type { rt[t] = 0; }
            self.face_texture_p.as_mut().unwrap()[t] = p_axis.g2() as i16;
            self.face_texture_m.as_mut().unwrap()[t] = p_axis.g2() as i16;
            self.face_texture_n.as_mut().unwrap()[t] = p_axis.g2() as i16;
        }

        // Post-decode fixups matching Java.
        if let Some(fta) = self.face_texture_axis.as_ref() {
            let mut has_texture = false;
            let ftp = self.face_texture_p.as_ref();
            let ftm = self.face_texture_m.as_ref();
            let ftn = self.face_texture_n.as_ref();
            for f in 0..num_faces as usize {
                let axis = fta[f] as u8;
                if axis != 255 {
                    let ai = axis as usize;
                    let matches_vertices = ftp.is_some_and(|v| v[ai] as u16 as i32 == self.face_vertex_a[f])
                        && ftm.is_some_and(|v| v[ai] as u16 as i32 == self.face_vertex_b[f])
                        && ftn.is_some_and(|v| v[ai] as u16 as i32 == self.face_vertex_c[f]);
                    if !matches_vertices {
                        has_texture = true;
                    }
                }
            }
            if !has_texture {
                self.face_texture_axis = None;
            } else {
                // Apply the per-face "matches → -1" sentinel.
                let fta = self.face_texture_axis.as_mut().unwrap();
                for f in 0..num_faces as usize {
                    let axis = fta[f] as u8;
                    if axis != 255 {
                        let ai = axis as usize;
                        let matches_vertices = self.face_texture_p.as_ref().is_some_and(|v| v[ai] as u16 as i32 == self.face_vertex_a[f])
                            && self.face_texture_m.as_ref().is_some_and(|v| v[ai] as u16 as i32 == self.face_vertex_b[f])
                            && self.face_texture_n.as_ref().is_some_and(|v| v[ai] as u16 as i32 == self.face_vertex_c[f]);
                        if matches_vertices {
                            fta[f] = -1;
                        }
                    }
                }
            }
        }

        if !has_texture_id {
            self.face_texture_id = None;
        }
        if !has_render_type {
            self.face_render_type = None;
        }
    }
}
