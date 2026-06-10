// @ObfuscatedName("eu") — jag::oldscape::configdecoder::SpotType
//
// Spot animation. Loaded from config archive group 13. Holds the
// model id, optional animation seq, recolour / retexture tables,
// resize / angle / ambient / contrast.

#![allow(dead_code)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::io::packet::Packet;
use crate::js5::js5_loader::Js5Loader;
use crate::js5::js5_net;

// @ObfuscatedName("eu.n") / "eu.j"
pub static CONFIG_SLOT: AtomicI32 = AtomicI32::new(-1);
pub static MODELS_SLOT: AtomicI32 = AtomicI32::new(-1);

#[derive(Debug, Clone, Default)]
pub struct SpotType {
    // @ObfuscatedName("eu.q")
    pub id: i32,
    // @ObfuscatedName("eu.i")
    pub model: i32,
    // @ObfuscatedName("eu.s")
    pub anim: i32,
    // @ObfuscatedName("eu.u") / "eu.v" / "eu.w" / "eu.e"
    pub recol_s: Vec<i16>,
    pub recol_d: Vec<i16>,
    pub retex_s: Vec<i16>,
    pub retex_d: Vec<i16>,
    // @ObfuscatedName("eu.b") / "eu.y"
    pub resizeh: i32,
    pub resizev: i32,
    // @ObfuscatedName("eu.t")
    pub angle: i32,
    // @ObfuscatedName("eu.f") / "eu.k"
    pub ambient: i32,
    pub contrast: i32,
}

impl SpotType {
    pub fn new(id: i32) -> Self {
        Self {
            id, model: 0, anim: -1,
            recol_s: Vec::new(), recol_d: Vec::new(),
            retex_s: Vec::new(), retex_d: Vec::new(),
            resizeh: 128, resizev: 128,
            angle: 0, ambient: 0, contrast: 0,
        }
    }
    // @ObfuscatedName("eu.g(Lev;I)V") — SpotType.decode loop
    pub fn decode_all(&mut self, p: &mut Packet) {
        loop {
            let c = p.g1();
            if c == 0 { return; }
            self.decode(p, c);
        }
    }
    // @ObfuscatedName("eu.q(Lev;II)V") — SpotType.decode(opcode)
    pub fn decode(&mut self, p: &mut Packet, code: i32) {
        match code {
            1 => self.model = p.g2(),
            2 => self.anim = p.g2(),
            4 => self.resizeh = p.g2(),
            5 => self.resizev = p.g2(),
            6 => self.angle = p.g2(),
            7 => self.ambient = p.g1(),
            8 => self.contrast = p.g1(),
            40 => {
                let n = p.g1() as usize;
                self.recol_s = (0..n).map(|_| p.g2() as i16).collect();
                self.recol_d = (0..n).map(|_| p.g2() as i16).collect();
            }
            41 => {
                let n = p.g1() as usize;
                self.retex_s = (0..n).map(|_| p.g2() as i16).collect();
                self.retex_d = (0..n).map(|_| p.g2() as i16).collect();
            }
            _ => {}
        }
    }
}

// @ObfuscatedName("eu.x") — SpotType model cache (Java LruCache(30);
// plain map per the loc/obj cache convention).
pub static MODEL_CACHE: std::sync::LazyLock<Mutex<std::collections::HashMap<i32, std::sync::Arc<crate::dash3d::model_lit::ModelLit>>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

impl SpotType {
    // @ObfuscatedName("eu.i(IS)Lfo;") — SpotType.getTempModel2.
    // Verbatim port of SpotType.java:135-179: load + recolour +
    // retexture + light(ambient+64, contrast+850, -30, -50, -30) into
    // the cache, then per call animate the requested frame (or
    // copyForAnim2 when unanimated), resize, and snap-rotate.
    pub fn get_temp_model2(&self, frame: i32) -> Option<crate::dash3d::model_lit::ModelLit> {
        use crate::dash3d::model_lit::ModelLit;
        use crate::dash3d::model_unlit::ModelUnlit;

        let base = {
            let cached = MODEL_CACHE.lock().unwrap().get(&self.id).cloned();
            match cached {
                Some(m) => m,
                None => {
                    let bytes = {
                        let slot = MODELS_SLOT.load(Ordering::Relaxed);
                        if slot < 0 { return None; }
                        let mut reg = js5_net::LOADERS.lock().unwrap();
                        let loader = reg.get_mut(slot as usize).and_then(|o| o.as_mut())?;
                        loader.fetch_file(self.model, 0)?
                    };
                    let mut unlit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        ModelUnlit::from_bytes(bytes)
                    })).ok()?;
                    for i in 0..self.recol_s.len() {
                        unlit.recolour(self.recol_s[i], self.recol_d[i]);
                    }
                    for i in 0..self.retex_s.len() {
                        unlit.retexture(self.retex_s[i], self.retex_d[i]);
                    }
                    let lit = ModelLit::light(&mut unlit, self.ambient + 64,
                                              self.contrast + 850, -30, -50, -30);
                    let arc = std::sync::Arc::new(lit);
                    MODEL_CACHE.lock().unwrap().insert(self.id, std::sync::Arc::clone(&arc));
                    arc
                }
            }
        };

        let mut model = if self.anim == -1 || frame == -1 {
            base.copy_for_anim2(true)
        } else {
            crate::config::seq_type::list(self.anim).animate_model_2(&base, frame)
        };

        if self.resizeh != 128 || self.resizev != 128 {
            model.resize(self.resizeh, self.resizev, self.resizeh);
        }
        if self.angle != 0 {
            if self.angle == 90 {
                model.rotate90();
            }
            if self.angle == 180 {
                model.rotate90();
                model.rotate90();
            }
            if self.angle == 270 {
                model.rotate90();
                model.rotate90();
                model.rotate90();
            }
        }
        Some(model)
    }
}

pub struct SpotStore { pub map: std::collections::HashMap<i32, SpotType> }
pub static STORE: std::sync::LazyLock<Mutex<SpotStore>> =
    std::sync::LazyLock::new(|| Mutex::new(SpotStore { map: std::collections::HashMap::new() }));

// @ObfuscatedName("eu.z(IB)Leu;") — SpotType.list(id).
//
// Don't cache failed fetches — the config archive may not be fully
// loaded when first accessed. Java's recentUse caches on miss, but
// that interacts poorly with our streaming loader.
pub fn list(id: i32) -> SpotType {
    {
        let s = STORE.lock().unwrap();
        if let Some(t) = s.map.get(&id) { return t.clone(); }
    }
    let bytes_opt = {
        let cs = CONFIG_SLOT.load(Ordering::Relaxed);
        if cs < 0 { None }
        else {
            let mut reg = js5_net::LOADERS.lock().unwrap();
            reg.get_mut(cs as usize).and_then(|o| o.as_mut()).and_then(|l| l.fetch_file(13, id))
        }
    };
    let Some(bytes) = bytes_opt else { return SpotType::new(id); };
    let mut t = SpotType::new(id);
    let mut p = Packet::from_vec(bytes);
    t.decode_all(&mut p);
    let cloned = t.clone();
    STORE.lock().unwrap().map.insert(id, t);
    cloned
}

pub fn init(_archive: &Js5Loader) {}

// @ObfuscatedName("co.r(I)V") — SpotType.resetCache. Java clears both
// recentUse and modelCache; we only have STORE so the call is a
// one-liner. When the per-instance ModelLit cache lands, extend.
pub fn reset_cache() {
    STORE.lock().unwrap().map.clear();
}

pub fn install_archives(config_slot: i32, models_slot: i32) {
    CONFIG_SLOT.store(config_slot, Ordering::Relaxed);
    MODELS_SLOT.store(models_slot, Ordering::Relaxed);
}
