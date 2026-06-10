// @ObfuscatedName("fj") — jag::oldscape::configdecoder::ObjType
//
// Item / inventory type definitions. Loaded from config archive group
// 10 (one file per obj id). Decoder + post-decode + cert-template
// resolution ported verbatim. getModelUnlit / getModelLit / getSprite
// are stubbed until ModelUnlit + Pix3D land.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicI32, AtomicBool, Ordering};

use crate::dash3d::model_lit::ModelLit;
use crate::dash3d::model_unlit::ModelUnlit;
use crate::io::packet::Packet;
use crate::js5::js5_loader::Js5Loader;
use crate::js5::js5_net;

// @ObfuscatedName("fj.n") — config archive slot.
pub static CONFIG_SLOT: AtomicI32 = AtomicI32::new(-1);
// @ObfuscatedName("bb.j")
pub static MODELS_SLOT: AtomicI32 = AtomicI32::new(-1);
// @ObfuscatedName("cq.z")
pub static MEM_SERVER: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone)]
pub struct ObjType {
    // @ObfuscatedName("fj.u")
    pub id: i32,
    // @ObfuscatedName("fj.v")
    pub model: i32,
    // @ObfuscatedName("fj.w")
    pub name: String,
    // @ObfuscatedName("fj.e") / "fj.b"
    pub recol_s: Vec<i16>,
    pub recol_d: Vec<i16>,
    // @ObfuscatedName("fj.y") / "fj.t"
    pub retex_s: Vec<i16>,
    pub retex_d: Vec<i16>,
    // @ObfuscatedName("fj.f")
    pub zoom2d: i32,
    // @ObfuscatedName("fj.k") / "fj.o" / "fj.a"
    pub xan2d: i32,
    pub yan2d: i32,
    pub zan2d: i32,
    // @ObfuscatedName("fj.h") / "fj.x"
    pub xof2d: i32,
    pub yof2d: i32,
    // @ObfuscatedName("fj.p")
    pub stackable: i32,
    // @ObfuscatedName("fj.ad")
    pub cost: i32,
    // @ObfuscatedName("fj.ac")
    pub members: bool,
    // @ObfuscatedName("fj.aa") — ground options. Java declares
    // `String[] op` so the WHOLE array can be nulled when a members-
    // server obj is lockable on a F2P world (decode_finalize sets
    // `op = null; iop = null;`). We use `Option<...>` so a None outer
    // value matches Java's null-array semantic; `Some([...])` is the
    // post-decode default.
    pub op: Option<[Option<String>; 5]>,
    // @ObfuscatedName("fj.as") — inventory options. Same null-array
    // semantics as `op`.
    pub iop: Option<[Option<String>; 5]>,
    // @ObfuscatedName("fj.am") / "fj.ap" / "fj.av"
    pub manwear: i32,
    pub manwear2: i32,
    pub manwear_offset_y: i32,
    // @ObfuscatedName("fj.ak") / "fj.az" / "fj.an"
    pub womanwear: i32,
    pub womanwear2: i32,
    pub womanwear_offset_y: i32,
    // @ObfuscatedName("fj.ah") / "fj.ay"
    pub manwear3: i32,
    pub womanwear3: i32,
    // @ObfuscatedName("fj.al" / "fj.ab" / "fj.ao" / "fj.ag")
    pub manhead: i32,
    pub manhead2: i32,
    pub womanhead: i32,
    pub womanhead2: i32,
    // @ObfuscatedName("fj.ar") / "fj.aq"
    pub countobj: Option<[i32; 10]>,
    pub countco: Option<[i32; 10]>,
    // @ObfuscatedName("fj.at") / "fj.ae"
    pub certlink: i32,
    pub certtemplate: i32,
    // @ObfuscatedName("fj.au") / "fj.ax" / "fj.ai"
    pub resizex: i32,
    pub resizey: i32,
    pub resizez: i32,
    // @ObfuscatedName("fj.aj") / "fj.aw"
    pub ambient: i32,
    pub contrast: i32,
    // @ObfuscatedName("fj.af")
    pub team: i32,
}

impl ObjType {
    pub fn new(id: i32) -> Self {
        Self {
            id,
            model: 0,
            name: "null".to_string(),
            recol_s: Vec::new(), recol_d: Vec::new(),
            retex_s: Vec::new(), retex_d: Vec::new(),
            zoom2d: 2000,
            xan2d: 0, yan2d: 0, zan2d: 0,
            xof2d: 0, yof2d: 0,
            stackable: 0,
            cost: 1,
            members: false,
            op: Some([None, None, Some("Take".to_string()), None, None]),
            iop: Some([None, None, None, None, Some("Drop".to_string())]),
            manwear: -1, manwear2: -1, manwear_offset_y: 0,
            womanwear: -1, womanwear2: -1, womanwear_offset_y: 0,
            manwear3: -1, womanwear3: -1,
            manhead: -1, manhead2: -1, womanhead: -1, womanhead2: -1,
            countobj: None, countco: None,
            certlink: -1, certtemplate: -1,
            resizex: 128, resizey: 128, resizez: 128,
            ambient: 0, contrast: 0, team: 0,
        }
    }

    // @ObfuscatedName("fj.q(Lev;B)V") — ObjType.decode
    pub fn decode_all(&mut self, p: &mut Packet) {
        loop {
            let code = p.g1();
            if code == 0 { return; }
            self.decode(p, code);
        }
    }

    // @ObfuscatedName("fj.i(Lev;II)V")
    pub fn decode(&mut self, p: &mut Packet, code: i32) {
        match code {
            1 => { self.model = p.g2(); }
            2 => { self.name = p.gjstr(); }
            4 => { self.zoom2d = p.g2(); }
            5 => { self.xan2d = p.g2(); }
            6 => { self.yan2d = p.g2(); }
            7 => {
                let mut v = p.g2();
                if v > 32767 { v -= 65536; }
                self.xof2d = v;
            }
            8 => {
                let mut v = p.g2();
                if v > 32767 { v -= 65536; }
                self.yof2d = v;
            }
            11 => { self.stackable = 1; }
            12 => { self.cost = p.g4(); }
            16 => { self.members = true; }
            23 => { self.manwear = p.g2(); self.manwear_offset_y = p.g1(); }
            24 => { self.manwear2 = p.g2(); }
            25 => { self.womanwear = p.g2(); self.womanwear_offset_y = p.g1(); }
            26 => { self.womanwear2 = p.g2(); }
            30..=34 => {
                let idx = (code - 30) as usize;
                let s = p.gjstr();
                let slot = if s.eq_ignore_ascii_case("hidden") { None } else { Some(s) };
                let arr = self.op.get_or_insert_with(Default::default);
                arr[idx] = slot;
            }
            35..=39 => {
                let idx = (code - 35) as usize;
                let s = Some(p.gjstr());
                let arr = self.iop.get_or_insert_with(Default::default);
                arr[idx] = s;
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
            78 => { self.manwear3 = p.g2(); }
            79 => { self.womanwear3 = p.g2(); }
            90 => { self.manhead = p.g2(); }
            91 => { self.womanhead = p.g2(); }
            92 => { self.manhead2 = p.g2(); }
            93 => { self.womanhead2 = p.g2(); }
            95 => { self.zan2d = p.g2(); }
            97 => { self.certlink = p.g2(); }
            98 => { self.certtemplate = p.g2(); }
            100..=109 => {
                if self.countobj.is_none() {
                    self.countobj = Some([0; 10]);
                    self.countco = Some([0; 10]);
                }
                let idx = (code - 100) as usize;
                self.countobj.as_mut().unwrap()[idx] = p.g2();
                self.countco.as_mut().unwrap()[idx] = p.g2();
            }
            110 => { self.resizex = p.g2(); }
            111 => { self.resizey = p.g2(); }
            112 => { self.resizez = p.g2(); }
            113 => { self.ambient = p.g1b() as i32; }
            114 => { self.contrast = (p.g1b() as i32) * 5; }
            115 => { self.team = p.g1(); }
            _ => { /* unknown — ignore */ }
        }
    }

    // @ObfuscatedName("fj.s(Lfj;Lfj;I)V") — ObjType.genCert
    pub fn gen_cert(&mut self, template: &ObjType, link: &ObjType) {
        self.model = template.model;
        self.zoom2d = template.zoom2d;
        self.xan2d = template.xan2d;
        self.yan2d = template.yan2d;
        self.zan2d = template.zan2d;
        self.xof2d = template.xof2d;
        self.yof2d = template.yof2d;
        self.recol_s = template.recol_s.clone();
        self.recol_d = template.recol_d.clone();
        self.retex_s = template.retex_s.clone();
        self.retex_d = template.retex_d.clone();
        self.name = link.name.clone();
        self.members = link.members;
        self.cost = link.cost;
        self.stackable = 1;
    }
}

// @ObfuscatedName("fj.g") — recentUse cache, here unbounded for now.
pub struct ObjStore {
    pub map: std::collections::HashMap<i32, ObjType>,
}
pub static STORE: std::sync::LazyLock<Mutex<ObjStore>> =
    std::sync::LazyLock::new(|| Mutex::new(ObjStore { map: std::collections::HashMap::new() }));

pub fn reset_cache() {
    STORE.lock().unwrap().map.clear();
}

// @ObfuscatedName("bb.b(II)Ljava/lang/String;") — ObjType.invNumber.
// Verbatim port of ObjType.java:543-552. Formats an inventory cost
// (gp) with a color-coded short-suffix: yellow for <100k, white K
// for <10M, green M for â‰¥10M. Used by tooltip + price-check UIs.
pub fn inv_number(cost: i32) -> String {
    if cost < 100_000 {
        format!("<col=ffff00>{}</col>", cost)
    } else if cost < 10_000_000 {
        format!("<col=ffffff>{}{}</col>", cost / 1000, crate::text::THOUSAND_SHORT)
    } else {
        format!("<col=00ff80>{}{}</col>", cost / 1_000_000, crate::text::MILLION_SHORT)
    }
}

// @ObfuscatedName("bb.z(II)Lfj;") — ObjType.list(id)
impl ObjType {
    // @ObfuscatedName("fj.w(II)Lfj;") — ObjType.getStackSizeAlt.
    // Verbatim port of ObjType.java:429-444. Walks countobj/countco
    // pairs: the largest threshold whose `count` is â‰¥ countco[i]
    // wins. Used for coin piles and other stack-sized inventory
    // icons; returns `self` (no alt) when count == 1 or no rules
    // match.
    pub fn get_stack_size_alt(&self, count: i32) -> Option<ObjType> {
        let countobj = self.countobj?;
        let countco = self.countco?;
        if count <= 1 { return Some(self.clone()); }
        let mut obj_id = -1i32;
        for i in 0..10 {
            if count >= countco[i] && countco[i] != 0 {
                obj_id = countobj[i];
            }
        }
        if obj_id == -1 { return Some(self.clone()); }
        list(obj_id)
    }

    // @ObfuscatedName("fj.y(ZI)Z") — ObjType.checkWearModel. Verbatim
    // port of ObjType.java:555-580. Queues up to three worn-equipment
    // model groups (manwear/2/3 or woman variants) through the models
    // archive's request_download path and reports whether ALL slots
    // are present locally. Mirrors IdkType.check_model.
    pub fn check_wear_model(&self, gender: bool) -> bool {
        let (w1, w2, w3) = if gender {
            (self.womanwear, self.womanwear2, self.womanwear3)
        } else {
            (self.manwear, self.manwear2, self.manwear3)
        };
        if w1 == -1 { return true; }
        let models_slot = MODELS_SLOT.load(Ordering::Relaxed);
        if models_slot < 0 { return false; }
        let mut reg = crate::js5::js5_net::LOADERS.lock().unwrap();
        let loader = match reg.get_mut(models_slot as usize).and_then(|o| o.as_mut()) {
            Some(l) => l,
            None => return false,
        };
        let mut status = true;
        if !loader.request_download(w1, 0) {
            status = false;
        } else if w2 != -1 && !loader.request_download(w2, 0) {
            status = false;
        } else if w3 != -1 && !loader.request_download(w3, 0) {
            status = false;
        }
        status
    }

    // @ObfuscatedName("fj.f(ZB)Z") — ObjType.checkHeadModel. Verbatim
    // port of ObjType.java:633-652. Same as check_wear_model but for
    // the 1-2 head models (used in chat-head + dialog portrait paths).
    pub fn check_head_model(&self, gender: bool) -> bool {
        let (h1, h2) = if gender {
            (self.womanhead, self.womanhead2)
        } else {
            (self.manhead, self.manhead2)
        };
        if h1 == -1 { return true; }
        let models_slot = MODELS_SLOT.load(Ordering::Relaxed);
        if models_slot < 0 { return false; }
        let mut reg = crate::js5::js5_net::LOADERS.lock().unwrap();
        let loader = match reg.get_mut(models_slot as usize).and_then(|o| o.as_mut()) {
            Some(l) => l,
            None => return false,
        };
        let mut status = true;
        if !loader.request_download(h1, 0) {
            status = false;
        } else if h2 != -1 && !loader.request_download(h2, 0) {
            status = false;
        }
        status
    }

    // @ObfuscatedName("fj.u(II)Lfw;") — ObjType.getModelUnlit.
    // Verbatim port of ObjType.java:341-377: countobj redirect, model
    // load, resize, recolour, retexture — stops before lighting so
    // worn-equipment composition can merge several unlit models.
    pub fn get_model_unlit(&self, count: i32) -> Option<ModelUnlit> {
        if let (Some(countobj), Some(countco)) = (self.countobj.as_ref(), self.countco.as_ref()) {
            if count > 1 {
                let mut real = -1;
                for i in 0..10 {
                    if count >= countco[i] && countco[i] != 0 {
                        real = countobj[i];
                    }
                }
                if real != -1 {
                    return list(real)?.get_model_unlit(1);
                }
            }
        }
        let bytes = {
            let slot = MODELS_SLOT.load(Ordering::Relaxed);
            if slot < 0 { return None; }
            let mut reg = js5_net::LOADERS.lock().unwrap();
            let loader = reg.get_mut(slot as usize).and_then(|o| o.as_mut())?;
            loader.fetch_file(self.model & 0xFFFF, 0)?
        };
        let mut model = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ModelUnlit::from_bytes(bytes)
        })).ok()?;
        if self.resizex != 128 || self.resizey != 128 || self.resizez != 128 {
            model.resize(self.resizex, self.resizey, self.resizez);
        }
        for i in 0..self.recol_s.len() {
            model.recolour(self.recol_s[i], self.recol_d[i]);
        }
        for i in 0..self.retex_s.len() {
            model.retexture(self.retex_s[i], self.retex_d[i]);
        }
        Some(model)
    }

    // @ObfuscatedName("fj.v(IB)Lfo;") — ObjType.getModelLit. Verbatim
    // port of ObjType.java:381-425: countobj redirect, per-id LRU
    // cache, unlit build + light(ambient+64, contrast+768, -50,-10,-50),
    // AABB mouse-check flag.
    pub fn get_model_lit(&self, count: i32) -> Option<Arc<ModelLit>> {
        if let (Some(countobj), Some(countco)) = (self.countobj.as_ref(), self.countco.as_ref()) {
            if count > 1 {
                let mut real = -1;
                for i in 0..10 {
                    if count >= countco[i] && countco[i] != 0 {
                        real = countobj[i];
                    }
                }
                if real != -1 {
                    return list(real)?.get_model_lit(1);
                }
            }
        }
        // @ObfuscatedName("fj.r") — modelCache (Java LruCache(50); a
        // plain map until the LRU port lands, matching the loc model
        // cache convention in scene.rs).
        if let Some(m) = MODEL_CACHE.lock().unwrap().get(&self.id) {
            return Some(Arc::clone(m));
        }
        let mut unlit = self.get_model_unlit_unredirected()?;
        let lit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut lit = ModelLit::light(&mut unlit, self.ambient + 64, self.contrast + 768,
                                          -50, -10, -50);
            lit.use_aabb_mouse_check = true;
            lit.calc_bounding_cylinder();
            lit
        })).ok()?;
        let arc = Arc::new(lit);
        MODEL_CACHE.lock().unwrap().insert(self.id, Arc::clone(&arc));
        Some(arc)
    }

    // getModelLit's inline rebuild (Java repeats the load/resize/
    // recolour/retexture block rather than calling getModelUnlit, to
    // skip the countobj redirect it already resolved).
    fn get_model_unlit_unredirected(&self) -> Option<ModelUnlit> {
        let bytes = {
            let slot = MODELS_SLOT.load(Ordering::Relaxed);
            if slot < 0 { return None; }
            let mut reg = js5_net::LOADERS.lock().unwrap();
            let loader = reg.get_mut(slot as usize).and_then(|o| o.as_mut())?;
            loader.fetch_file(self.model & 0xFFFF, 0)?
        };
        let mut model = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ModelUnlit::from_bytes(bytes)
        })).ok()?;
        if self.resizex != 128 || self.resizey != 128 || self.resizez != 128 {
            model.resize(self.resizex, self.resizey, self.resizez);
        }
        for i in 0..self.recol_s.len() {
            model.recolour(self.recol_s[i], self.recol_d[i]);
        }
        for i in 0..self.retex_s.len() {
            model.retexture(self.retex_s[i], self.retex_d[i]);
        }
        Some(model)
    }
}

// @ObfuscatedName("fj.r") — ObjType.modelCache.
pub static MODEL_CACHE: std::sync::LazyLock<Mutex<std::collections::HashMap<i32, Arc<ModelLit>>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

pub fn list(id: i32) -> Option<ObjType> {
    {
        let s = STORE.lock().unwrap();
        if let Some(t) = s.map.get(&id) { return Some(t.clone()); }
    }
    let config_slot = CONFIG_SLOT.load(Ordering::Relaxed);
    if config_slot < 0 { return None; }
    let bytes = {
        let mut reg = js5_net::LOADERS.lock().unwrap();
        let loader = reg.get_mut(config_slot as usize).and_then(|o| o.as_mut())?;
        loader.fetch_file(10, id)?
    };
    let mut obj = ObjType::new(id);
    let mut p = Packet::from_vec(bytes);
    obj.decode_all(&mut p);
    if obj.certtemplate != -1 {
        let template = list(obj.certtemplate);
        let link = list(obj.certlink);
        if let (Some(t), Some(l)) = (template, link) {
            obj.gen_cert(&t, &l);
        }
    }
    // (Members-server downgrade follows.)
    if !MEM_SERVER.load(Ordering::Relaxed) && obj.members {
        obj.name = "Members object".to_string();
        // Java: `obj.op = null; obj.iop = null;` — whole arrays nulled
        // so any `obj.op != null` predicate downstream sees the lock.
        obj.op = None;
        obj.iop = None;
        obj.team = 0;
    }
    let cloned = obj.clone();
    STORE.lock().unwrap().map.insert(id, obj);
    Some(cloned)
}

pub fn init(_archive: &Js5Loader) {}

pub fn install_archives(config_slot: i32, models_slot: i32) {
    CONFIG_SLOT.store(config_slot, Ordering::Relaxed);
    MODELS_SLOT.store(models_slot, Ordering::Relaxed);
}
