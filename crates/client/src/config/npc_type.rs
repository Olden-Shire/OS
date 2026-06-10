// @ObfuscatedName("em") — jag::oldscape::configdecoder::NpcType
//
// NPC definition. Loaded from config archive group 9. Holds model id
// list, animation set, recolour/retexture tables, ops, resize, ambient
// / contrast, multinpc (varbit-driven variants), and a few visibility
// flags. Decoder + post-decode + cache ported verbatim. Model
// resolution + animation land alongside the broader dash3d port.

#![allow(dead_code)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::io::packet::Packet;
use crate::js5::js5_loader::Js5Loader;
use crate::js5::js5_net;

// @ObfuscatedName("em.n") / "dy.j"
pub static CONFIG_SLOT: AtomicI32 = AtomicI32::new(-1);
pub static MODELS_SLOT: AtomicI32 = AtomicI32::new(-1);

#[derive(Debug, Clone, Default)]
pub struct NpcType {
    // @ObfuscatedName("em.q")
    pub id: i32,
    // @ObfuscatedName("em.i")
    pub name: String,
    // @ObfuscatedName("em.s")
    pub size: i32,
    // @ObfuscatedName("em.u")
    pub model: Option<Vec<i32>>,
    // @ObfuscatedName("em.v")
    pub head: Option<Vec<i32>>,
    // @ObfuscatedName("em.w") / "em.e" / "em.b"
    pub readyanim: i32,
    pub turnleftanim: i32,
    pub turnrightanim: i32,
    // @ObfuscatedName("em.y") / "em.t" / "em.f" / "em.k"
    pub walkanim: i32,
    pub walkanim_b: i32,
    pub walkanim_r: i32,
    pub walkanim_l: i32,
    // @ObfuscatedName("em.o") / "em.a" / "em.h" / "em.x"
    pub recol_s: Vec<i16>,
    pub recol_d: Vec<i16>,
    pub retex_s: Vec<i16>,
    pub retex_d: Vec<i16>,
    // @ObfuscatedName("em.p")
    pub op: [Option<String>; 5],
    // @ObfuscatedName("em.ad")
    pub minimap: bool,
    // @ObfuscatedName("em.ac")
    pub vislevel: i32,
    // @ObfuscatedName("em.aa") / "em.as"
    pub resizeh: i32,
    pub resizev: i32,
    // @ObfuscatedName("em.am")
    pub alwaysontop: bool,
    // @ObfuscatedName("em.ap") / "em.av"
    pub ambient: i32,
    pub contrast: i32,
    // @ObfuscatedName("em.ak")
    pub headicon: i32,
    // @ObfuscatedName("em.az")
    pub turnspeed: i32,
    // @ObfuscatedName("em.an")
    pub multinpc: Option<Vec<i32>>,
    // @ObfuscatedName("em.ah") / "em.ay"
    pub multivarbit: i32,
    pub multivarp: i32,
    // @ObfuscatedName("em.al")
    pub active: bool,
    // @ObfuscatedName("em.ab")
    pub walksmoothing: bool,
}

impl NpcType {
    pub fn new(id: i32) -> Self {
        Self {
            id,
            name: "null".to_string(),
            size: 1,
            model: None, head: None,
            readyanim: -1,
            turnleftanim: -1, turnrightanim: -1,
            walkanim: -1, walkanim_b: -1, walkanim_r: -1, walkanim_l: -1,
            recol_s: Vec::new(), recol_d: Vec::new(),
            retex_s: Vec::new(), retex_d: Vec::new(),
            op: [None, None, None, None, None],
            minimap: true,
            vislevel: -1,
            resizeh: 128, resizev: 128,
            alwaysontop: false,
            ambient: 0, contrast: 0,
            headicon: -1,
            turnspeed: 32,
            multinpc: None,
            multivarbit: -1, multivarp: -1,
            active: true,
            walksmoothing: true,
        }
    }

    // @ObfuscatedName("em.i(Lev;I)V") — NpcType.decode loop
    pub fn decode_all(&mut self, p: &mut Packet) {
        loop {
            let code = p.g1();
            if code == 0 { return; }
            self.decode(p, code);
        }
    }

    // @ObfuscatedName("em.s(Lev;II)V") — NpcType.decode(opcode)
    pub fn decode(&mut self, p: &mut Packet, code: i32) {
        match code {
            1 => {
                let n = p.g1() as usize;
                let mut m = Vec::with_capacity(n);
                for _ in 0..n { m.push(p.g2()); }
                self.model = Some(m);
            }
            2 => { self.name = p.gjstr(); }
            12 => { self.size = p.g1(); }
            13 => { self.readyanim = p.g2(); }
            14 => { self.walkanim = p.g2(); }
            15 => { self.turnleftanim = p.g2(); }
            16 => { self.turnrightanim = p.g2(); }
            17 => {
                self.walkanim = p.g2();
                self.walkanim_b = p.g2();
                self.walkanim_r = p.g2();
                self.walkanim_l = p.g2();
            }
            30..=34 => {
                let idx = (code - 30) as usize;
                let s = p.gjstr();
                self.op[idx] = if s.eq_ignore_ascii_case("hidden") { None } else { Some(s) };
            }
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
            60 => {
                let n = p.g1() as usize;
                let mut h = Vec::with_capacity(n);
                for _ in 0..n { h.push(p.g2()); }
                self.head = Some(h);
            }
            93 => { self.minimap = false; }
            95 => { self.vislevel = p.g2(); }
            97 => { self.resizeh = p.g2(); }
            98 => { self.resizev = p.g2(); }
            99 => { self.alwaysontop = true; }
            100 => { self.ambient = p.g1b() as i32; }
            101 => { self.contrast = (p.g1b() as i32) * 5; }
            102 => { self.headicon = p.g2(); }
            103 => { self.turnspeed = p.g2(); }
            106 => {
                self.multivarbit = p.g2();
                if self.multivarbit == 65535 { self.multivarbit = -1; }
                self.multivarp = p.g2();
                if self.multivarp == 65535 { self.multivarp = -1; }
                let n = p.g1() as usize;
                let mut m = Vec::with_capacity(n + 1);
                for _ in 0..=n {
                    let v = p.g2();
                    m.push(if v == 65535 { -1 } else { v });
                }
                self.multinpc = Some(m);
            }
            107 => { self.active = false; }
            109 => { self.walksmoothing = false; }
            _ => {}
        }
    }
}

// @ObfuscatedName("em.ag") — NpcType model cache (Java LruCache(50);
// plain map per the loc/obj cache convention).
pub static MODEL_CACHE: std::sync::LazyLock<Mutex<std::collections::HashMap<i32, std::sync::Arc<crate::dash3d::model_lit::ModelLit>>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

pub struct NpcStore {
    pub map: std::collections::HashMap<i32, NpcType>,
}
pub static STORE: std::sync::LazyLock<Mutex<NpcStore>> =
    std::sync::LazyLock::new(|| Mutex::new(NpcStore { map: std::collections::HashMap::new() }));

pub fn reset_cache() {
    STORE.lock().unwrap().map.clear();
}

impl NpcType {
    // @ObfuscatedName("em.h(B)Lem;") — NpcType.getMultiNpc. Verbatim
    // port of NpcType.java:391. Resolves the current multinpc variant
    // by reading multivarbit / multivarp and indexing into the
    // multinpc array. Returns None when the var resolves out of range
    // (Java returns null; consumers treat as "no npc here").
    pub fn get_multi_npc(&self) -> Option<NpcType> {
        let multinpc = self.multinpc.as_ref()?;
        let v = if self.multivarbit != -1 {
            crate::config::var_cache::get_varbit(self.multivarbit)
        } else if self.multivarp != -1 {
            crate::config::var_cache::get_varp(self.multivarp)
        } else {
            -1
        };
        if v < 0 || v as usize >= multinpc.len() { return None; }
        let id = multinpc[v as usize];
        if id == -1 { return None; }
        Some(list(id))
    }

    // @ObfuscatedName("em.t(B)Z") — NpcType.isMultiNpcVisible.
    // Returns true if this multinpc would resolve to a real npc id.
    // Used by the scene render to cull invisible multinpc variants.
    pub fn is_multi_npc_visible(&self) -> bool {
        self.get_multi_npc().is_some()
    }

    // @ObfuscatedName("em.u(Leo;ILeo;IB)Lfo;") — NpcType.getTempModel.
    // Verbatim port of NpcType.java:274-337: multinpc redirect, merged
    // + recoloured body model lit (ambient+64, contrast+850, -30/-50/
    // -30) into the cache, then per call the primary/secondary seq
    // animation (split when both run) and the h/v resize.
    pub fn get_temp_model(
        &self,
        primary: Option<&crate::config::seq_type::SeqType>,
        primary_frame: i32,
        secondary: Option<&crate::config::seq_type::SeqType>,
        secondary_frame: i32,
    ) -> Option<crate::dash3d::model_lit::ModelLit> {
        use crate::dash3d::model_lit::ModelLit;
        use crate::dash3d::model_unlit::ModelUnlit;

        if self.multinpc.is_some() {
            let npc = self.get_multi_npc()?;
            return npc.get_temp_model(primary, primary_frame, secondary, secondary_frame);
        }

        let base = {
            let cached = MODEL_CACHE.lock().unwrap().get(&self.id).cloned();
            match cached {
                Some(m) => m,
                None => {
                    let model_ids = self.model.as_ref()?;
                    let mut parts: Vec<ModelUnlit> = Vec::with_capacity(model_ids.len());
                    for &id in model_ids {
                        let bytes = {
                            let slot = MODELS_SLOT.load(Ordering::Relaxed);
                            if slot < 0 { return None; }
                            let mut reg = js5_net::LOADERS.lock().unwrap();
                            let loader = reg.get_mut(slot as usize).and_then(|o| o.as_mut())?;
                            loader.fetch_file(id, 0)?
                        };
                        let part = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            ModelUnlit::from_bytes(bytes)
                        })).ok()?;
                        parts.push(part);
                    }
                    let mut unlit = if parts.len() == 1 {
                        parts.remove(0)
                    } else {
                        ModelUnlit::merge(&parts.iter().collect::<Vec<_>>())
                    };
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

        let mut model = match (primary, secondary) {
            (Some(p), Some(s)) => p.split_animate_model(&base, primary_frame, s, secondary_frame),
            (Some(p), None) => p.animate_model(&base, primary_frame),
            (None, Some(s)) => s.animate_model(&base, secondary_frame),
            (None, None) => base.copy_for_anim(true),
        };

        if self.resizeh != 128 || self.resizev != 128 {
            model.resize(self.resizeh, self.resizev, self.resizeh);
        }

        Some(model)
    }

    // @ObfuscatedName("em.v(I)Lfw;") — NpcType.getHead. Verbatim port
    // of NpcType.java:341-387: multinpc redirect, load every head
    // model (all-or-nothing — Java's requestDownload pre-pass means a
    // single missing file returns null), merge multi-part heads, then
    // recolour/retexture. Returns the unlit chathead model.
    pub fn get_head(&self) -> Option<crate::dash3d::model_unlit::ModelUnlit> {
        use crate::dash3d::model_unlit::ModelUnlit;
        if self.multinpc.is_some() {
            return self.get_multi_npc()?.get_head();
        }

        let head = self.head.as_ref()?;

        let mut parts: Vec<ModelUnlit> = Vec::with_capacity(head.len());
        for &id in head {
            let bytes = {
                let slot = MODELS_SLOT.load(Ordering::Relaxed);
                if slot < 0 { return None; }
                let mut reg = js5_net::LOADERS.lock().unwrap();
                let loader = reg.get_mut(slot as usize).and_then(|o| o.as_mut())?;
                loader.fetch_file(id, 0)?
            };
            let part = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                ModelUnlit::from_bytes(bytes)
            })).ok()?;
            parts.push(part);
        }

        let mut model = if parts.len() == 1 {
            parts.remove(0)
        } else {
            ModelUnlit::merge(&parts.iter().collect::<Vec<_>>())
        };

        for i in 0..self.recol_s.len() {
            model.recolour(self.recol_s[i], self.recol_d[i]);
        }
        for i in 0..self.retex_s.len() {
            model.retexture(self.retex_s[i], self.retex_d[i]);
        }

        Some(model)
    }

    // Pure direction→anim dispatcher. Java's NpcType has
    // walkanim/walkanim_b/walkanim_l/walkanim_r as four separate
    // fields keyed by the entity's facing direction:
    //   0 = forward (walkanim)
    //   1 = back    (walkanim_b)
    //   2 = right   (walkanim_r)
    //   3 = left    (walkanim_l)
    // Returns the per-direction anim id, falling back to the forward
    // anim when a per-direction slot is -1 (Java's null check).
    pub fn active_walk_anim(&self, dir: i32) -> i32 {
        let alt = match dir {
            1 => self.walkanim_b,
            2 => self.walkanim_r,
            3 => self.walkanim_l,
            _ => self.walkanim,
        };
        if alt != -1 { alt } else { self.walkanim }
    }
}

// @ObfuscatedName("f.g(IB)Lem;") — NpcType.list(id)
pub fn list(id: i32) -> NpcType {
    {
        let s = STORE.lock().unwrap();
        if let Some(t) = s.map.get(&id) { return t.clone(); }
    }
    let bytes_opt = {
        let config_slot = CONFIG_SLOT.load(Ordering::Relaxed);
        if config_slot < 0 { None }
        else {
            let mut reg = js5_net::LOADERS.lock().unwrap();
            reg.get_mut(config_slot as usize)
                .and_then(|o| o.as_mut())
                .and_then(|l| l.fetch_file(9, id))
        }
    };
    let Some(bytes) = bytes_opt else { return NpcType::new(id); };
    let mut t = NpcType::new(id);
    let mut p = Packet::from_vec(bytes);
    t.decode_all(&mut p);
    let cloned = t.clone();
    STORE.lock().unwrap().map.insert(id, t);
    cloned
}

pub fn init(_archive: &Js5Loader) {}

pub fn install_archives(config_slot: i32, models_slot: i32) {
    CONFIG_SLOT.store(config_slot, Ordering::Relaxed);
    MODELS_SLOT.store(models_slot, Ordering::Relaxed);
}
