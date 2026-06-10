// @ObfuscatedName("fb") — jag::oldscape::configdecoder::FloType
//
// Terrain floor / overlay type. Loaded from config archive group 4.
// Holds the base colour, optional texture, optional mapcolour, plus
// the derived HSL values used by ClientBuild's lighting pass.

#![allow(dead_code)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::io::packet::Packet;
use crate::js5::js5_loader::Js5Loader;
use crate::js5::js5_net;

// @ObfuscatedName("by.n") — config archive slot.
pub static CONFIG_SLOT: AtomicI32 = AtomicI32::new(-1);

#[derive(Debug, Clone, Default)]
pub struct FloType {
    // @ObfuscatedName("fb.z")
    pub colour: i32,
    // @ObfuscatedName("fb.g")
    pub texture: i32,
    // @ObfuscatedName("fb.q")
    pub occlude: bool,
    // @ObfuscatedName("fb.i")
    pub mapcolour: i32,
    // @ObfuscatedName("fb.s") / "fb.u" / "fb.v"
    pub hue: i32,
    pub saturation: i32,
    pub lightness: i32,
    // @ObfuscatedName("fb.w") / "fb.e" / "fb.b"
    pub map_hue: i32,
    pub map_saturation: i32,
    pub map_lightness: i32,
}

impl FloType {
    pub fn new() -> Self {
        Self {
            colour: 0,
            texture: -1,
            occlude: true,
            mapcolour: -1,
            ..Default::default()
        }
    }

    // @ObfuscatedName("fb.i(Lev;III)V") — FloType.decode(code)
    pub fn decode(&mut self, p: &mut Packet, code: i32) {
        match code {
            1 => { self.colour = p.g3(); }
            2 => { self.texture = p.g1(); }
            5 => { self.occlude = false; }
            7 => { self.mapcolour = p.g3(); }
            _ => {}
        }
    }

    // @ObfuscatedName("fb.q(Lev;IB)V") — FloType.decode loop
    pub fn decode_all(&mut self, p: &mut Packet) {
        loop {
            let code = p.g1();
            if code == 0 { return; }
            self.decode(p, code);
        }
    }

    // @ObfuscatedName("fb.g(B)V") — FloType.postDecode
    pub fn post_decode(&mut self) {
        if self.mapcolour != -1 {
            let (h, s, l) = compute_hsl(self.mapcolour);
            self.map_hue = h;
            self.map_saturation = s;
            self.map_lightness = l;
        }
        let (h, s, l) = compute_hsl(self.colour);
        self.hue = h;
        self.saturation = s;
        self.lightness = l;
    }
}

// @ObfuscatedName("fb.s(II)V") — FloType.getHsl (RGB → HSL)
fn compute_hsl(rgb: i32) -> (i32, i32, i32) {
    let r = ((rgb >> 16) & 0xFF) as f64 / 256.0;
    let g = ((rgb >> 8) & 0xFF) as f64 / 256.0;
    let b = (rgb & 0xFF) as f64 / 256.0;
    let lo = r.min(g).min(b);
    let hi = r.max(g).max(b);
    let mut h = 0f64;
    let mut s = 0f64;
    let l = (lo + hi) / 2.0;
    if lo != hi {
        if l < 0.5 { s = (hi - lo) / (lo + hi); }
        else { s = (hi - lo) / (2.0 - hi - lo); }
        if r == hi { h = (g - b) / (hi - lo); }
        else if g == hi { h = (b - r) / (hi - lo) + 2.0; }
        else if b == hi { h = (r - g) / (hi - lo) + 4.0; }
    }
    let hue = (h / 6.0 * 256.0) as i32;
    let sat = ((s * 256.0) as i32).clamp(0, 255);
    let lit = ((l * 256.0) as i32).clamp(0, 255);
    (hue, sat, lit)
}

pub struct FloStore {
    pub map: std::collections::HashMap<i32, FloType>,
}
pub static STORE: std::sync::LazyLock<Mutex<FloStore>> =
    std::sync::LazyLock::new(|| Mutex::new(FloStore { map: std::collections::HashMap::new() }));

// @ObfuscatedName("cj.z(II)Lfb;") — FloType.list(id)
pub fn list(id: i32) -> FloType {
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
                .and_then(|l| l.fetch_file(4, id))
        }
    };
    // Same caching-failed-fetch trap as FluType — don't cache the
    // default-zero record or the tile rendering goes permanently bad.
    let Some(bytes) = bytes_opt else {
        let mut t = FloType::new();
        t.post_decode();
        return t;
    };
    let mut t = FloType::new();
    let mut p = Packet::from_vec(bytes);
    t.decode_all(&mut p);
    t.post_decode();
    let cloned = t.clone();
    STORE.lock().unwrap().map.insert(id, t);
    cloned
}

// Readiness probe for the scene colour bake — same contract as
// flu_type::is_loaded: true once the real config bytes decoded
// (cached), false while still streaming.
pub fn is_loaded(id: i32) -> bool {
    {
        let s = STORE.lock().unwrap();
        if s.map.contains_key(&id) {
            return true;
        }
    }
    let bytes_opt = {
        let config_slot = CONFIG_SLOT.load(Ordering::Relaxed);
        if config_slot < 0 {
            None
        } else {
            let mut reg = js5_net::LOADERS.lock().unwrap();
            reg.get_mut(config_slot as usize)
                .and_then(|o| o.as_mut())
                .and_then(|l| l.fetch_file(4, id))
        }
    };
    let Some(bytes) = bytes_opt else { return false };
    let mut t = FloType::new();
    let mut p = Packet::from_vec(bytes);
    t.decode_all(&mut p);
    t.post_decode();
    STORE.lock().unwrap().map.insert(id, t);
    true
}

pub fn init(_archive: &Js5Loader) {}

pub fn install_archives(config_slot: i32) {
    CONFIG_SLOT.store(config_slot, Ordering::Relaxed);
}
