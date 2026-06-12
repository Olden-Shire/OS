// @ObfuscatedName("fd") — jag::oldscape::configdecoder::IdkType
//
// "Identity kit" — player appearance body part. Loaded from config
// archive group 3. Holds a model id list, head model overlays, recolour
// / retexture tables, and a `disable` flag for slots the player can't
// equip. The model-combiner pipeline lives in jagex3.dash3d.ModelUnlit.

#![allow(dead_code)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::io::packet::Packet;
use crate::js5::js5_loader::Js5Loader;
use crate::js5::js5_net;

// @ObfuscatedName("fd.n") / "fd.j"
pub static CONFIG_SLOT: AtomicI32 = AtomicI32::new(-1);
pub static MODELS_SLOT: AtomicI32 = AtomicI32::new(-1);
// @ObfuscatedName("dl.z")
pub static NUM_DEFINITIONS: AtomicI32 = AtomicI32::new(0);

#[derive(Debug, Clone, Default)]
pub struct IdkType {
    // @ObfuscatedName("fd.q")
    pub type_: i32,
    // @ObfuscatedName("fd.i")
    pub model: Vec<i32>,
    // @ObfuscatedName("fd.s") / "fd.u" / "fd.v" / "fd.w"
    pub recol_s: Vec<i16>,
    pub recol_d: Vec<i16>,
    pub retex_s: Vec<i16>,
    pub retex_d: Vec<i16>,
    // @ObfuscatedName("fd.e")
    pub head: [i32; 5],
    // @ObfuscatedName("fd.b")
    pub disable: bool,
}

impl IdkType {
    pub fn new() -> Self {
        Self {
            type_: -1,
            head: [-1, -1, -1, -1, -1],
            ..Default::default()
        }
    }
    // @ObfuscatedName("fd.q(Lev;I)V") — IdkType.decode loop
    pub fn decode_all(&mut self, p: &mut Packet) {
        loop {
            let c = p.g1();
            if c == 0 { return; }
            self.decode(p, c);
        }
    }
    // @ObfuscatedName("fd.i(Lev;II)V") — IdkType.decode(opcode)
    pub fn decode(&mut self, p: &mut Packet, code: i32) {
        match code {
            1 => self.type_ = p.g1(),
            2 => {
                let n = p.g1() as usize;
                self.model = (0..n).map(|_| p.g2()).collect();
            }
            3 => self.disable = true,
            40 => {
                let n = p.g1() as usize;
                // Interleaved (src, dst) pairs on the wire (Java reads
                // g2 src then g2 dst per iteration) - a split read
                // scrambles multi-pair recolours.
                self.recol_s = Vec::with_capacity(n);
                self.recol_d = Vec::with_capacity(n);
                for _ in 0..n {
                    self.recol_s.push(p.g2() as i16);
                    self.recol_d.push(p.g2() as i16);
                }
            }
            41 => {
                let n = p.g1() as usize;
                // Interleaved (src, dst) pairs on the wire (Java reads
                // g2 src then g2 dst per iteration) - a split read
                // scrambles multi-pair retexours.
                self.retex_s = Vec::with_capacity(n);
                self.retex_d = Vec::with_capacity(n);
                for _ in 0..n {
                    self.retex_s.push(p.g2() as i16);
                    self.retex_d.push(p.g2() as i16);
                }
            }
            60..=69 => {
                let idx = (code - 60) as usize;
                self.head[idx] = p.g2();
            }
            _ => {}
        }
    }
}

pub struct IdkStore { pub map: std::collections::HashMap<i32, IdkType> }
pub static STORE: std::sync::LazyLock<Mutex<IdkStore>> =
    std::sync::LazyLock::new(|| Mutex::new(IdkStore { map: std::collections::HashMap::new() }));

// @ObfuscatedName("p.g(II)Lfd;") — IdkType.list(id)
pub fn list(id: i32) -> IdkType {
    {
        let s = STORE.lock().unwrap();
        if let Some(t) = s.map.get(&id) { return t.clone(); }
    }
    let bytes_opt = {
        let cs = CONFIG_SLOT.load(Ordering::Relaxed);
        if cs < 0 { None }
        else {
            let mut reg = js5_net::LOADERS.lock().unwrap();
            reg.get_mut(cs as usize).and_then(|o| o.as_mut()).and_then(|l| l.fetch_file(3, id))
        }
    };
    let Some(bytes) = bytes_opt else { return IdkType::new(); };
    let mut t = IdkType::new();
    let mut p = Packet::from_vec(bytes);
    t.decode_all(&mut p);
    let cloned = t.clone();
    STORE.lock().unwrap().map.insert(id, t);
    cloned
}

pub fn init(_archive: &Js5Loader) {}

// @ObfuscatedName("br.r(I)V") — IdkType.resetCache.
pub fn reset_cache() {
    STORE.lock().unwrap().map.clear();
}

impl IdkType {
    // @ObfuscatedName("fd.s(I)Z") — IdkType.checkModel. Verbatim port
    // of IdkType.java:136-149. Returns true iff every required model
    // file is already cached locally (request_download returns true
    // when the file is on disk and false when it kicks off a fetch).
    // Used by the appearance composition pass to skip frames where the
    // worn-model assets haven't streamed in yet.
    pub fn check_model(&self) -> bool {
        if self.model.is_empty() { return true; }
        let models_slot = MODELS_SLOT.load(Ordering::Relaxed);
        if models_slot < 0 { return false; }
        let mut reg = crate::js5::js5_net::LOADERS.lock().unwrap();
        let loader = match reg.get_mut(models_slot as usize).and_then(|o| o.as_mut()) {
            Some(l) => l,
            None => return false,
        };
        let mut all_ready = true;
        for &id in &self.model {
            if !loader.request_download(id, 0) {
                all_ready = false;
            }
        }
        all_ready
    }

    // @ObfuscatedName("fd.u(S)Lfw;") — IdkType.getModelNoCheck. Verbatim
    // port of IdkType.java:153-183: load + merge the body-part models,
    // then recolour/retexture. Caller has already verified availability
    // via check_model.
    pub fn get_model_no_check(&self) -> Option<crate::dash3d::model_unlit::ModelUnlit> {
        use crate::dash3d::model_unlit::ModelUnlit;
        if self.model.is_empty() {
            return None;
        }
        let mut parts: Vec<ModelUnlit> = Vec::with_capacity(self.model.len());
        for &id in &self.model {
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

    // @ObfuscatedName("fd.v(B)Z") — IdkType.checkHead. Verbatim port of
    // IdkType.java:187-195. Same idea as check_model but on the head[5]
    // array; -1 slots are skipped.
    pub fn check_head(&self) -> bool {
        let models_slot = MODELS_SLOT.load(Ordering::Relaxed);
        if models_slot < 0 { return false; }
        let mut reg = crate::js5::js5_net::LOADERS.lock().unwrap();
        let loader = match reg.get_mut(models_slot as usize).and_then(|o| o.as_mut()) {
            Some(l) => l,
            None => return false,
        };
        let mut all_ready = true;
        for &id in &self.head {
            if id != -1 && !loader.request_download(id, 0) {
                all_ready = false;
            }
        }
        all_ready
    }
}

impl IdkType {
    // @ObfuscatedName("fd.w(B)Lfw;") — IdkType.getHeadNoCheck. Verbatim
    // port of IdkType.java:199-222: merge the up-to-5 chathead overlay
    // models then recolour/retexture.
    pub fn get_head_no_check(&self) -> Option<crate::dash3d::model_unlit::ModelUnlit> {
        use crate::dash3d::model_unlit::ModelUnlit;
        let mut parts: Vec<ModelUnlit> = Vec::new();
        for &id in &self.head {
            if id == -1 {
                continue;
            }
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
}

pub fn install_archives(config_slot: i32, models_slot: i32) {
    CONFIG_SLOT.store(config_slot, Ordering::Relaxed);
    MODELS_SLOT.store(models_slot, Ordering::Relaxed);
    // Java IdkType.java:60 — `numDefinitions = configClient.getFileIdLimit(3)`.
    // Group 3 of the config archive holds IDK (player kit) records.
    let limit = {
        let reg = crate::js5::js5_net::LOADERS.lock().unwrap();
        reg.get(config_slot as usize)
            .and_then(|o| o.as_ref())
            .map(|l| l.get_file_id_limit(3))
            .unwrap_or(0)
    };
    NUM_DEFINITIONS.store(limit, Ordering::Relaxed);
}
