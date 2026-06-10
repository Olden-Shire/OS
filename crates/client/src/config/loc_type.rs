// @ObfuscatedName("ey") — jag::oldscape::configdecoder::LocType
//
// Loaded from config archive group 6 (one file per loc id). Holds
// dimensions, model+shape arrays, walk/range blockers, recolour /
// retexture tables, animation, and a multivarbit/multivarp branch
// table. Decoder + model resolution (checkModel / buildModel /
// getModel / getTempModel) ported verbatim.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicI32, Ordering};

use crate::dash3d::model_lit::ModelLit;
use crate::dash3d::model_unlit::ModelUnlit;
use crate::io::packet::Packet;
use crate::js5::js5_loader::Js5Loader;
use crate::js5::js5_net;

// @ObfuscatedName("j.j") — config archive slot.
pub static CONFIG_SLOT: AtomicI32 = AtomicI32::new(-1);
// @ObfuscatedName("ey.z") — models archive slot.
pub static MODELS_SLOT: AtomicI32 = AtomicI32::new(-1);
// @ObfuscatedName("ey.n")
pub static LOW_MEM: AtomicI32 = AtomicI32::new(0);

#[derive(Debug, Clone)]
pub struct LocType {
    // @ObfuscatedName("ey.v")
    pub id: i32,
    // @ObfuscatedName("ey.w")
    pub model: Option<Vec<i32>>,
    // @ObfuscatedName("ey.e")
    pub shape: Option<Vec<i32>>,
    // @ObfuscatedName("ey.b")
    pub name: String,
    // @ObfuscatedName("ey.y") / "ey.t"
    pub recol_s: Vec<i16>,
    pub recol_d: Vec<i16>,
    // @ObfuscatedName("ey.f") / "ey.k"
    pub retex_s: Vec<i16>,
    pub retex_d: Vec<i16>,
    // @ObfuscatedName("ey.o")
    pub width: i32,
    // @ObfuscatedName("ey.a")
    pub length: i32,
    // @ObfuscatedName("ey.h")
    pub blockwalk: i32,
    // @ObfuscatedName("ey.x")
    pub blockrange: bool,
    // @ObfuscatedName("ey.p")
    pub active: i32,
    // @ObfuscatedName("ey.ad")
    pub skew_type: i32,
    // @ObfuscatedName("ey.ac")
    pub sharelight: bool,
    // @ObfuscatedName("ey.aa")
    pub occlude: bool,
    // @ObfuscatedName("ey.as")
    pub anim: i32,
    // @ObfuscatedName("ey.am")
    pub wallwidth: i32,
    // @ObfuscatedName("ey.ap")
    pub ambient: i32,
    // @ObfuscatedName("ey.av")
    pub contrast: i32,
    // @ObfuscatedName("ey.ak")
    pub op: [Option<String>; 5],
    // @ObfuscatedName("ey.az")
    pub mapfunction: i32,
    // @ObfuscatedName("ey.an")
    pub mapscene: i32,
    // @ObfuscatedName("ey.ah")
    pub mirror: bool,
    // @ObfuscatedName("ey.ay")
    pub shadow: bool,
    // @ObfuscatedName("ey.al") / "ey.ab" / "ey.ao"
    pub resizex: i32,
    pub resizey: i32,
    pub resizez: i32,
    // @ObfuscatedName("ey.ag") / "ey.ar" / "ey.aq"
    pub offsetx: i32,
    pub offsety: i32,
    pub offsetz: i32,
    // @ObfuscatedName("ey.at")
    pub forceapproach: i32,
    // @ObfuscatedName("ey.ae")
    pub forcedecor: bool,
    // @ObfuscatedName("ey.au")
    pub breakroutefinding: bool,
    // @ObfuscatedName("ey.ax")
    pub raiseobject: i32,
    // @ObfuscatedName("ey.ai")
    pub multiloc: Option<Vec<i32>>,
    // @ObfuscatedName("ey.aj") / "ey.aw"
    pub multivarbit: i32,
    pub multivarp: i32,
    // @ObfuscatedName("ey.af") / "ey.bh" / "ey.bi" / "ey.bs"
    pub bgsound_sound: i32,
    pub bgsound_range: i32,
    pub bgsound_mindelay: i32,
    pub bgsound_maxdelay: i32,
    // @ObfuscatedName("ey.bk")
    pub bgsound_random: Option<Vec<i32>>,
}

impl LocType {
    pub fn new(id: i32) -> Self {
        Self {
            id,
            model: None, shape: None,
            name: "null".to_string(),
            recol_s: Vec::new(), recol_d: Vec::new(),
            retex_s: Vec::new(), retex_d: Vec::new(),
            width: 1, length: 1,
            blockwalk: 2, blockrange: true,
            active: -1, skew_type: -1,
            sharelight: false, occlude: false,
            anim: -1, wallwidth: 16,
            ambient: 0, contrast: 0,
            op: [None, None, None, None, None],
            mapfunction: -1, mapscene: -1,
            mirror: false, shadow: true,
            resizex: 128, resizey: 128, resizez: 128,
            offsetx: 0, offsety: 0, offsetz: 0,
            forceapproach: 0,
            forcedecor: false, breakroutefinding: false,
            raiseobject: -1,
            multiloc: None,
            multivarbit: -1, multivarp: -1,
            bgsound_sound: -1, bgsound_range: 0,
            bgsound_mindelay: 0, bgsound_maxdelay: 0,
            bgsound_random: None,
        }
    }

    // @ObfuscatedName("ey.i(Lev;I)V") — LocType.decode
    pub fn decode_all(&mut self, p: &mut Packet) {
        loop {
            let code = p.g1();
            if code == 0 { return; }
            self.decode(p, code);
        }
    }

    // @ObfuscatedName("ey.s(Lev;II)V") — LocType.decode(opcode)
    pub fn decode(&mut self, p: &mut Packet, code: i32) {
        let low_mem = LOW_MEM.load(Ordering::Relaxed) != 0;
        match code {
            1 => {
                let count = p.g1();
                if count > 0 {
                    if self.model.is_none() || low_mem {
                        let mut sh = Vec::with_capacity(count as usize);
                        let mut md = Vec::with_capacity(count as usize);
                        for _ in 0..count {
                            md.push(p.g2());
                            sh.push(p.g1());
                        }
                        self.shape = Some(sh);
                        self.model = Some(md);
                    } else {
                        p.pos += count * 3;
                    }
                }
            }
            2 => { self.name = p.gjstr(); }
            5 => {
                let count = p.g1();
                if count > 0 {
                    if self.model.is_none() || low_mem {
                        self.shape = None;
                        let mut md = Vec::with_capacity(count as usize);
                        for _ in 0..count { md.push(p.g2()); }
                        self.model = Some(md);
                    } else {
                        p.pos += count * 2;
                    }
                }
            }
            14 => { self.width = p.g1(); }
            15 => { self.length = p.g1(); }
            17 => { self.blockwalk = 0; self.blockrange = false; }
            18 => { self.blockrange = false; }
            19 => { self.active = p.g1(); }
            21 => { self.skew_type = 0; }
            22 => { self.sharelight = true; }
            23 => { self.occlude = true; }
            24 => {
                self.anim = p.g2();
                if self.anim == 65535 { self.anim = -1; }
            }
            27 => { self.blockwalk = 1; }
            28 => { self.wallwidth = p.g1(); }
            29 => { self.ambient = p.g1b() as i32; }
            39 => { self.contrast = (p.g1b() as i32) * 25; }
            30..=34 => {
                let idx = (code - 30) as usize;
                let s = p.gjstr();
                self.op[idx] = if s.eq_ignore_ascii_case("hidden") { None } else { Some(s) };
            }
            40 => {
                let count = p.g1() as usize;
                self.recol_s = (0..count).map(|_| p.g2() as i16).collect();
                self.recol_d = (0..count).map(|_| p.g2() as i16).collect();
            }
            41 => {
                let count = p.g1() as usize;
                self.retex_s = (0..count).map(|_| p.g2() as i16).collect();
                self.retex_d = (0..count).map(|_| p.g2() as i16).collect();
            }
            60 => { self.mapfunction = p.g2(); }
            62 => { self.mirror = true; }
            64 => { self.shadow = false; }
            65 => { self.resizex = p.g2(); }
            66 => { self.resizey = p.g2(); }
            67 => { self.resizez = p.g2(); }
            68 => { self.mapscene = p.g2(); }
            69 => { self.forceapproach = p.g1(); }
            70 => { self.offsetx = p.g2b(); }
            71 => { self.offsety = p.g2b(); }
            72 => { self.offsetz = p.g2b(); }
            73 => { self.forcedecor = true; }
            74 => { self.breakroutefinding = true; }
            75 => { self.raiseobject = p.g1(); }
            77 => {
                self.multivarbit = p.g2();
                if self.multivarbit == 65535 { self.multivarbit = -1; }
                self.multivarp = p.g2();
                if self.multivarp == 65535 { self.multivarp = -1; }
                let count = p.g1() as usize;
                let mut m = Vec::with_capacity(count + 1);
                for _ in 0..=count {
                    let v = p.g2();
                    m.push(if v == 65535 { -1 } else { v });
                }
                self.multiloc = Some(m);
            }
            78 => {
                self.bgsound_sound = p.g2();
                self.bgsound_range = p.g1();
            }
            79 => {
                self.bgsound_mindelay = p.g2();
                self.bgsound_maxdelay = p.g2();
                self.bgsound_range = p.g1();
                let count = p.g1() as usize;
                self.bgsound_random = Some((0..count).map(|_| p.g2()).collect());
            }
            81 => { self.skew_type = p.g1() * 256; }
            _ => { /* unknown — ignore */ }
        }
    }

    // @ObfuscatedName("ey.t(B)Ley;") — LocType.getMultiLoc.
    // When the loc has `multiloc` set, pick the active child id based
    // on the current varbit or varp value. Java returns null when the
    // var is out of range or the slot is -1; we mirror that so callers
    // can fall back to this loc's own definition.
    pub fn get_multi_loc(&self) -> Option<LocType> {
        let multiloc = self.multiloc.as_ref()?;
        let v = if self.multivarbit != -1 {
            crate::config::var_cache::get_varbit(self.multivarbit)
        } else if self.multivarp != -1 {
            crate::config::var_cache::get_varp(self.multivarp)
        } else {
            -1
        };
        if v < 0 { return None; }
        let idx = v as usize;
        let child_id = *multiloc.get(idx)?;
        if child_id == -1 { return None; }
        list(child_id)
    }

    // @ObfuscatedName("ey.q(B)V") — LocType.postDecode
    pub fn post_decode(&mut self) {
        if self.active == -1 {
            self.active = 0;
            if self.model.is_some()
                && (self.shape.is_none() || self.shape.as_ref().map(|v| v[0]) == Some(10))
            {
                self.active = 1;
            }
            for i in 0..5 {
                if self.op[i].is_some() { self.active = 1; }
            }
        }
        if self.raiseobject == -1 {
            self.raiseobject = if self.blockwalk == 0 { 0 } else { 1 };
        }
    }

    // @ObfuscatedName("ey.u(II)Z") — LocType.checkModel (shape-specific).
    // Verbatim port of LocType.java:402-421. The shape table maps each
    // loc-shape kind (0..22) to a slot in `model[]`; checkModel(shape)
    // queues just the one slot if present. Shape 10 (the "any" wildcard)
    // falls back to the all-models check. Returns true if every needed
    // model is locally cached.
    pub fn check_model(&self, shape: i32) -> bool {
        if let Some(shapes) = self.shape.as_ref() {
            let models = self.model.as_ref();
            for (i, &s) in shapes.iter().enumerate() {
                if s == shape {
                    if let Some(ms) = models {
                        let models_slot = MODELS_SLOT.load(Ordering::Relaxed);
                        if models_slot < 0 { return true; }
                        let mut reg = js5_net::LOADERS.lock().unwrap();
                        let Some(loader) = reg.get_mut(models_slot as usize)
                            .and_then(|o| o.as_mut()) else { return true; };
                        let mid = ms.get(i).copied().unwrap_or(-1);
                        return loader.request_download(mid & 0xFFFF, 0);
                    }
                    return true;
                }
            }
            return true;
        }
        if self.model.is_none() { return true; }
        if shape == 10 {
            return self.check_model_all();
        }
        true
    }

    // @ObfuscatedName("ey.v(I)Z") — LocType.checkModelAll. Queues all
    // referenced model groups; returns true once they're all available.
    pub fn check_model_all(&self) -> bool {
        let Some(models) = self.model.as_ref() else { return true; };
        let models_slot = MODELS_SLOT.load(Ordering::Relaxed);
        if models_slot < 0 { return true; }
        let mut reg = js5_net::LOADERS.lock().unwrap();
        let Some(loader) = reg.get_mut(models_slot as usize).and_then(|o| o.as_mut()) else {
            return true;
        };
        let mut all = true;
        for &mid in models {
            all &= loader.request_download(mid & 0xFFFF, 0);
        }
        all
    }

    // @ObfuscatedName("ey.w/.e/.b") — LocType.model_key. Pure helper
    // distilled from LocType.java:439-444 / 477-482 / 501-506. Java
    // inlines this packed key (id, shape_idx, rotation) at every
    // model-cache lookup site; we hoist it so the eventual cache
    // wiring can share a single key shape.
    pub fn model_key(&self, shape_idx: i32, rotation: i32) -> i64 {
        if self.shape.is_none() {
            ((self.id as i64) << 10) + rotation as i64
        } else {
            ((self.id as i64) << 10) + ((shape_idx as i64) << 3) + rotation as i64
        }
    }

    // @ObfuscatedName("ey.y") — LocType.find_shape_slot. Pure linear
    // scan of `self.shape` returning the index of the first match,
    // or -1 when not found. Open-coded in Java buildModel /
    // checkModel; hoisted here for reuse.
    pub fn find_shape_slot(&self, shape: i32) -> i32 {
        let Some(shapes) = self.shape.as_ref() else { return -1; };
        for (i, &s) in shapes.iter().enumerate() {
            if s == shape { return i as i32; }
        }
        -1
    }

    // @ObfuscatedName("ey.y") — LocType.compute_mirror. Pure helper
    // distilled from LocType.java:582-583. When the loc has a shape
    // array, mirror flips iff (shape == 2 && rotation > 3). Otherwise
    // mirror flips iff (rotation > 3). Returns the effective mirror
    // flag the loc should be drawn with.
    pub fn compute_mirror(&self, shape: i32, rotation: i32) -> bool {
        let extra = if self.shape.is_some() {
            shape == 2 && rotation > 3
        } else {
            rotation > 3
        };
        self.mirror ^ extra
    }

    // @ObfuscatedName("ey.k(B)Z") — LocType.hasBgSound. Verbatim port
    // of LocType.java:665-678. Returns true iff THIS loc or any of
    // its multiloc children has a bgsound assignment. Used by
    // BgSound::addSound gating to skip locs that won't emit audio.
    pub fn has_bg_sound(&self) -> bool {
        let Some(multiloc) = self.multiloc.as_ref() else {
            return self.bgsound_sound != -1 || self.bgsound_random.is_some();
        };
        for &child_id in multiloc {
            if child_id == -1 { continue; }
            let Some(child) = list(child_id) else { continue; };
            if child.bgsound_sound != -1 || child.bgsound_random.is_some() {
                return true;
            }
        }
        false
    }

    // @ObfuscatedName("ey.y(IIB)Lfw;") — LocType.buildModel. Verbatim
    // port of LocType.java:533-640: raw model load (mc1 cache, mirror
    // baked into the cache key), multi-model merge for shapeless
    // kind-10 locs, then the per-loc transform chain (rotate-X for
    // hanging decor, quarter-turns, recolour/retexture, resize,
    // offset).
    pub fn build_model(&self, shape: i32, rotation: i32) -> Option<ModelUnlit> {
        let load_raw = |model_id: i32, mirrored: bool| -> Option<Arc<ModelUnlit>> {
            let key = (model_id as i64) + if mirrored { 65536 } else { 0 };
            if let Some(m) = MC1.lock().unwrap().get(&key) {
                return Some(Arc::clone(m));
            }
            let bytes = {
                let slot = MODELS_SLOT.load(Ordering::Relaxed);
                if slot < 0 { return None; }
                let mut reg = js5_net::LOADERS.lock().unwrap();
                let loader = reg.get_mut(slot as usize).and_then(|o| o.as_mut())?;
                loader.fetch_file(model_id & 0xFFFF, 0)?
            };
            let mut m = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                ModelUnlit::from_bytes(bytes)
            })).ok()?;
            if mirrored {
                m.mirror();
            }
            let arc = Arc::new(m);
            MC1.lock().unwrap().insert(key, Arc::clone(&arc));
            Some(arc)
        };

        let base: ModelUnlit = if self.shape.is_none() {
            if shape != 10 {
                return None;
            }
            let models = self.model.as_ref()?;
            // Java's dead `shape == 2` mirror flip kept verbatim.
            let mut mirrored = self.mirror;
            if shape == 2 && rotation > 3 {
                mirrored = !mirrored;
            }
            if models.len() == 1 {
                (*load_raw(models[0], mirrored)?).clone()
            } else {
                let mut parts: Vec<Arc<ModelUnlit>> = Vec::with_capacity(models.len());
                for &mid in models {
                    parts.push(load_raw(mid, mirrored)?);
                }
                let refs: Vec<&ModelUnlit> = parts.iter().map(|a| a.as_ref()).collect();
                ModelUnlit::merge(&refs)
            }
        } else {
            let slot = self.find_shape_slot(shape);
            if slot == -1 {
                return None;
            }
            let model_id = self.model.as_ref()?.get(slot as usize).copied()?;
            let mirrored = self.mirror ^ (rotation > 3);
            (*load_raw(model_id, mirrored)?).clone()
        };

        let mut model = base;
        if shape == 4 && rotation > 3 {
            model.rotate_x_axis(256);
            model.translate(45, 0, -45);
        }
        let r = rotation & 0x3;
        if r == 1 {
            model.rotate90();
        } else if r == 2 {
            model.rotate180();
        } else if r == 3 {
            model.rotate270();
        }
        for i in 0..self.recol_s.len() {
            model.recolour(self.recol_s[i], self.recol_d[i]);
        }
        for i in 0..self.retex_s.len() {
            model.retexture(self.retex_s[i], self.retex_d[i]);
        }
        if self.resizex != 128 || self.resizey != 128 || self.resizez != 128 {
            model.resize(self.resizex, self.resizey, self.resizez);
        }
        if self.offsetx != 0 || self.offsety != 0 || self.offsetz != 0 {
            model.translate(self.offsetx, self.offsety, self.offsetz);
        }
        Some(model)
    }

    // @ObfuscatedName("ey.w(II[[IIIII)Lfu;") — LocType.getModel.
    // Verbatim port of LocType.java:438-472. Returns the ModelSource
    // ClientBuild.addLoc places into the World: a lit static model,
    // or (for sharelight locs) a per-placement unlit copy that
    // World.shareLight pairs + lights at the end of the build.
    // `groundh` is the current level's heightmap slice; (anchor_x, h,
    // anchor_z) the placement anchor for hillSkew.
    pub fn get_model(&self, shape: i32, rotation: i32, groundh: &[Vec<i32>],
                     anchor_x: i32, h: i32, anchor_z: i32)
                     -> Option<Arc<crate::dash3d::model_source::ModelSource>> {
        use crate::dash3d::model_source::ModelSource;
        let key = self.model_key(shape, rotation);
        let cached = MC2.lock().unwrap().get(&key).cloned();
        let cached = match cached {
            Some(c) => c,
            None => {
                let mut unlit = self.build_model(shape, rotation)?;
                let entry = if self.sharelight {
                    unlit.ambient = (self.ambient + 64) as i16;
                    unlit.contrast = (self.contrast + 768) as i16;
                    unlit.calculate_normals();
                    CachedLocModel::UnlitProto(Arc::new(unlit))
                } else {
                    let lit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        ModelLit::light(&mut unlit, self.ambient + 64, self.contrast + 768,
                                        -50, -10, -50)
                    })).ok()?;
                    CachedLocModel::Lit(Arc::new(lit))
                };
                MC2.lock().unwrap().insert(key, entry.clone());
                entry
            }
        };
        match cached {
            CachedLocModel::UnlitProto(proto) => {
                let mut copy = proto.copy_for_share_light();
                if self.skew_type >= 0 {
                    copy.hill_skew_in_place(groundh, anchor_x, h, anchor_z, self.skew_type);
                }
                Some(ModelSource::unlit(copy))
            }
            CachedLocModel::Lit(lit) => {
                if self.skew_type >= 0 {
                    if let Some(skewed) = lit.hill_skew(groundh, anchor_x, h, anchor_z,
                                                        self.skew_type) {
                        return Some(ModelSource::lit(Arc::new(skewed)));
                    }
                }
                Some(ModelSource::lit(lit))
            }
        }
    }

    // @ObfuscatedName("ey.b(II[[IIIILeo;IB)Lfo;") — LocType.getTempModel.
    // Verbatim port of LocType.java:500-529 — the per-frame composer
    // ClientLocAnim.getTempModel calls: cached lit base (mc3), then
    // SeqType.animateModel90 for the current frame, then hillSkew
    // (in place — the animated copy is already private).
    pub fn get_temp_model(&self, shape: i32, rotation: i32, groundh: &[Vec<i32>],
                          anchor_x: i32, h: i32, anchor_z: i32,
                          seq: Option<&crate::config::seq_type::SeqType>,
                          frame: i32) -> Option<Arc<ModelLit>> {
        let key = self.model_key(shape, rotation);
        let base = {
            let cached = MC3.lock().unwrap().get(&key).cloned();
            match cached {
                Some(m) => m,
                None => {
                    let mut unlit = self.build_model(shape, rotation)?;
                    let lit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        ModelLit::light(&mut unlit, self.ambient + 64, self.contrast + 768,
                                        -50, -10, -50)
                    })).ok()?;
                    let arc = Arc::new(lit);
                    MC3.lock().unwrap().insert(key, Arc::clone(&arc));
                    arc
                }
            }
        };
        if seq.is_none() && self.skew_type == -1 {
            return Some(base);
        }
        let mut composed: ModelLit = match seq {
            Some(s) => s.animate_model_90(&base, frame, rotation),
            None => (*base).clone(),
        };
        if self.skew_type >= 0 {
            if let Some(skewed) = composed.hill_skew(groundh, anchor_x, h, anchor_z,
                                                     self.skew_type) {
                composed = skewed;
            }
        }
        Some(Arc::new(composed))
    }
}

// @ObfuscatedName("ey.o") — LocType.mc1: raw loaded models keyed by
// model id (+65536 when mirrored). Java LruCache(500).
pub static MC1: std::sync::LazyLock<Mutex<std::collections::HashMap<i64, Arc<ModelUnlit>>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

// @ObfuscatedName("ey.a") — LocType.mc2: built scene models keyed by
// (id, shape, rotation). Lit for normal locs, an unlit prototype
// (normals calculated) for sharelight locs. Java LruCache(30).
#[derive(Clone)]
pub enum CachedLocModel {
    Lit(Arc<ModelLit>),
    UnlitProto(Arc<ModelUnlit>),
}
pub static MC2: std::sync::LazyLock<Mutex<std::collections::HashMap<i64, CachedLocModel>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

// @ObfuscatedName("ey.h") — LocType.mc3: lit bases for the animated
// getTempModel path. Java LruCache(30).
pub static MC3: std::sync::LazyLock<Mutex<std::collections::HashMap<i64, Arc<ModelLit>>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

// LocType.resetCache companion for the model caches.
pub fn reset_model_caches() {
    MC1.lock().unwrap().clear();
    MC2.lock().unwrap().clear();
    MC3.lock().unwrap().clear();
}

// @ObfuscatedName("ey.g") — recentUse LRU. We use an unbounded HashMap;
// memory cost of cached LocTypes is fine on modern systems.
pub struct LocStore {
    pub map: std::collections::HashMap<i32, LocType>,
}
// @ObfuscatedName(— LocType.resetCache). Clears the in-memory cache.
pub fn reset_cache() {
    STORE.lock().unwrap().map.clear();
}

pub static STORE: std::sync::LazyLock<Mutex<LocStore>> =
    std::sync::LazyLock::new(|| Mutex::new(LocStore { map: std::collections::HashMap::new() }));

// @ObfuscatedName("fj.g(IB)Ley;") — LocType.list(id).
//
// Java caches a default-decoded stub even on fetch failure; we don't
// (matches our other config types and avoids caching transient
// streaming misses). See `feedback_no_half_implementations.md` note —
// the matching-Java path is correct but caused a regression where
// streaming-failed locs got default-cached and stayed gray.
pub fn list(id: i32) -> Option<LocType> {
    {
        let s = STORE.lock().unwrap();
        if let Some(t) = s.map.get(&id) { return Some(t.clone()); }
    }
    let config_slot = CONFIG_SLOT.load(Ordering::Relaxed);
    if config_slot < 0 { return None; }
    let bytes = {
        let mut reg = js5_net::LOADERS.lock().unwrap();
        let loader = reg.get_mut(config_slot as usize).and_then(|o| o.as_mut())?;
        loader.fetch_file(6, id)?
    };
    let mut t = LocType::new(id);
    let mut p = Packet::from_vec(bytes);
    t.decode_all(&mut p);
    t.post_decode();
    if t.breakroutefinding {
        t.blockwalk = 0;
        t.blockrange = false;
    }
    let cloned = t.clone();
    STORE.lock().unwrap().map.insert(id, t);
    Some(cloned)
}

// @ObfuscatedName("av.z(Lch;Lch;ZI)V") — LocType.init. Keep the call-site
// signature stable for config::init dispatch.
pub fn init(_archive: &Js5Loader) {}

pub fn install_archives(config_slot: i32, models_slot: i32) {
    CONFIG_SLOT.store(config_slot, Ordering::Relaxed);
    MODELS_SLOT.store(models_slot, Ordering::Relaxed);
}
