// @ObfuscatedName("fp") — jag::oldscape::configdecoder::InvType
//
// Inventory definition. Loaded from config archive group 5. Only field
// is `size` (the inventory's slot count).

#![allow(dead_code)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::io::packet::Packet;
use crate::js5::js5_loader::Js5Loader;
use crate::js5::js5_net;

// @ObfuscatedName("fp.n")
pub static CONFIG_SLOT: AtomicI32 = AtomicI32::new(-1);

#[derive(Debug, Clone, Default)]
pub struct InvType {
    // @ObfuscatedName("fp.z")
    pub size: i32,
}

impl InvType {
    pub fn new() -> Self { Self::default() }
    // @ObfuscatedName("fp.z(Lev;I)V") — InvType.decode loop
    pub fn decode_all(&mut self, p: &mut Packet) {
        loop {
            let c = p.g1();
            if c == 0 { return; }
            self.decode(p, c);
        }
    }
    // @ObfuscatedName("fp.g(Lev;II)V") — InvType.decode(opcode)
    pub fn decode(&mut self, p: &mut Packet, code: i32) {
        if code == 2 { self.size = p.g2(); }
    }
}

pub struct InvStore { pub map: std::collections::HashMap<i32, InvType> }
pub static STORE: std::sync::LazyLock<Mutex<InvStore>> =
    std::sync::LazyLock::new(|| Mutex::new(InvStore { map: std::collections::HashMap::new() }));

// jagex3.config.InvType.resetCache — wired into the config-wide
// reset sweep that runs on logout / world hop.
pub fn reset_cache() {
    STORE.lock().unwrap().map.clear();
}

// jagex3.config.InvType.list (no Java @ObfuscatedName — public static).
pub fn list(id: i32) -> InvType {
    {
        let s = STORE.lock().unwrap();
        if let Some(t) = s.map.get(&id) { return t.clone(); }
    }
    let bytes_opt = {
        let cs = CONFIG_SLOT.load(Ordering::Relaxed);
        if cs < 0 { None }
        else {
            let mut reg = js5_net::LOADERS.lock().unwrap();
            reg.get_mut(cs as usize).and_then(|o| o.as_mut()).and_then(|l| l.fetch_file(5, id))
        }
    };
    let Some(bytes) = bytes_opt else { return InvType::new(); };
    let mut t = InvType::new();
    let mut p = Packet::from_vec(bytes);
    t.decode_all(&mut p);
    let cloned = t.clone();
    STORE.lock().unwrap().map.insert(id, t);
    cloned
}

pub fn init(_archive: &Js5Loader) {}

pub fn install_archives(config_slot: i32) {
    CONFIG_SLOT.store(config_slot, Ordering::Relaxed);
}
