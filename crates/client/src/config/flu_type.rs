// @ObfuscatedName("ec") — jag::oldscape::configdecoder::FluType
//
// Underlay floor type. Loaded from config archive group 1. The
// decoder is tiny — only opcode 1 (colour). The HSL derivation runs
// in postDecode and stores hue/saturation/lightness/chroma in a form
// suited for the 11×11 averaging blur ClientBuild.finishBuild uses
// to smooth tile underlays.

#![allow(dead_code)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::io::packet::Packet;
use crate::js5::js5_loader::Js5Loader;
use crate::js5::js5_net;

// @ObfuscatedName("ec.n") — config archive slot.
pub static CONFIG_SLOT: AtomicI32 = AtomicI32::new(-1);

#[derive(Debug, Clone, Default)]
pub struct FluType {
    // @ObfuscatedName("ec.z")
    pub colour: i32,
    // @ObfuscatedName("ec.g")
    pub hue: i32,
    // @ObfuscatedName("ec.q")
    pub saturation: i32,
    // @ObfuscatedName("ec.i")
    pub lightness: i32,
    // @ObfuscatedName("ec.s")
    pub chroma: i32,
}

impl FluType {
    pub fn new() -> Self { Self::default() }

    // @ObfuscatedName("ec.i(Lev;II)V") — FluType.decode loop
    pub fn decode_all(&mut self, p: &mut Packet) {
        loop {
            let code = p.g1();
            if code == 0 { return; }
            self.decode(p, code);
        }
    }

    // @ObfuscatedName("ec.s(Lev;III)V") — FluType.decode(opcode)
    pub fn decode(&mut self, p: &mut Packet, code: i32) {
        if code == 1 { self.colour = p.g3(); }
    }

    // @ObfuscatedName("ec.q(I)V") — FluType.postDecode
    pub fn post_decode(&mut self) {
        self.get_hsl(self.colour);
    }

    // @ObfuscatedName("ec.u(IB)V") — FluType.getHsl
    pub fn get_hsl(&mut self, rgb: i32) {
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
        let h = h / 6.0;
        self.saturation = ((s * 256.0) as i32).clamp(0, 255);
        self.lightness = ((l * 256.0) as i32).clamp(0, 255);
        let chroma = if l > 0.5 {
            (1.0 - l) * s * 512.0
        } else {
            s * l * 512.0
        };
        self.chroma = (chroma as i32).max(1);
        self.hue = (self.chroma as f64 * h) as i32;
    }
}

pub struct FluStore {
    pub map: std::collections::HashMap<i32, FluType>,
}
pub static STORE: std::sync::LazyLock<Mutex<FluStore>> =
    std::sync::LazyLock::new(|| Mutex::new(FluStore { map: std::collections::HashMap::new() }));

// @ObfuscatedName("bf.g(IB)Lec;") — FluType.list(id)
//
// Crucially we only cache a decoded FluType when the JS5 fetch
// succeeded — otherwise the very first call (before the config
// archive has streamed in) would seed STORE with an all-zero FluType
// and every subsequent call would return the broken default,
// permanently turning every grass tile black.
pub fn list(id: i32) -> FluType {
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
                .and_then(|l| l.fetch_file(1, id))
        }
    };
    let Some(bytes) = bytes_opt else {
        // Don't cache — try again next frame once JS5 catches up.
        let mut t = FluType::new();
        t.post_decode();
        return t;
    };
    let mut t = FluType::new();
    let mut p = Packet::from_vec(bytes);
    t.decode_all(&mut p);
    t.post_decode();
    let cloned = t.clone();
    STORE.lock().unwrap().map.insert(id, t);
    cloned
}

// Readiness probe for the scene colour bake: true once the config
// bytes for `id` have actually streamed in and decoded (caching the
// result), false while JS5 is still fetching. Java never needs this —
// its loading steps fully preload the config archive before
// finishBuild runs; our lazy per-id fetch needs the build gated until
// every referenced id is real, or the baked tile colours freeze black.
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
                .and_then(|l| l.fetch_file(1, id))
        }
    };
    let Some(bytes) = bytes_opt else { return false };
    let mut t = FluType::new();
    let mut p = Packet::from_vec(bytes);
    t.decode_all(&mut p);
    t.post_decode();
    STORE.lock().unwrap().map.insert(id, t);
    true
}

// @ObfuscatedName("u.z(Lch;I)V") — FluType.init
pub fn init(_archive: &Js5Loader) {}

// @ObfuscatedName("u.r(I)V") — FluType.resetCache. Java only clears
// the recentUse LRU; the master STORE persists.
pub fn reset_cache() {
    STORE.lock().unwrap().map.clear();
}

pub fn install_archives(config_slot: i32) {
    CONFIG_SLOT.store(config_slot, Ordering::Relaxed);
}
