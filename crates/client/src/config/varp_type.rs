// @ObfuscatedName("fg") — jag::oldscape::configdecoder::VarpType
//
// Player variable type. Loaded from config archive group 16. The only
// field the client uses is `clientcode`, which gates whether the var
// resets across logins.

#![allow(dead_code)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::io::packet::Packet;
use crate::js5::js5_loader::Js5Loader;
use crate::js5::js5_net;

// @ObfuscatedName("al.n") — config archive slot.
pub static CONFIG_SLOT: AtomicI32 = AtomicI32::new(-1);
// @ObfuscatedName("ey.j") — numDefinitions, computed from config.getFileIdLimit(16).
pub static NUM_DEFINITIONS: AtomicI32 = AtomicI32::new(0);

#[derive(Debug, Clone, Default)]
pub struct VarpType {
    // @ObfuscatedName("fg.g")
    pub clientcode: i32,
}

impl VarpType {
    pub fn new() -> Self { Self::default() }

    // @ObfuscatedName("fg.q(Lev;I)V") — VarpType.decode loop
    pub fn decode_all(&mut self, p: &mut Packet) {
        loop {
            let code = p.g1();
            if code == 0 { return; }
            self.decode(p, code);
        }
    }

    // @ObfuscatedName("fg.i(Lev;II)V") — VarpType.decode(opcode)
    pub fn decode(&mut self, p: &mut Packet, code: i32) {
        if code == 5 { self.clientcode = p.g2(); }
    }
}

pub struct VarpStore { pub map: std::collections::HashMap<i32, VarpType> }
pub static STORE: std::sync::LazyLock<Mutex<VarpStore>> =
    std::sync::LazyLock::new(|| Mutex::new(VarpStore { map: std::collections::HashMap::new() }));

// @ObfuscatedName("ez.g(II)Lfg;") — VarpType.list(id)
pub fn list(id: i32) -> VarpType {
    {
        let s = STORE.lock().unwrap();
        if let Some(t) = s.map.get(&id) { return t.clone(); }
    }
    let bytes_opt = {
        let cs = CONFIG_SLOT.load(Ordering::Relaxed);
        if cs < 0 { None }
        else {
            let mut reg = js5_net::LOADERS.lock().unwrap();
            reg.get_mut(cs as usize).and_then(|o| o.as_mut()).and_then(|l| l.fetch_file(16, id))
        }
    };
    let Some(bytes) = bytes_opt else { return VarpType::new(); };
    let mut t = VarpType::new();
    let mut p = Packet::from_vec(bytes);
    t.decode_all(&mut p);
    let cloned = t.clone();
    STORE.lock().unwrap().map.insert(id, t);
    cloned
}

// @ObfuscatedName("cy.z(Lch;I)V") — VarpType.init
pub fn init(_archive: &Js5Loader) {}

// @ObfuscatedName("cy.r(I)V") — VarpType.resetCache.
pub fn reset_cache() {
    STORE.lock().unwrap().map.clear();
}

pub fn install_archives(config_slot: i32) {
    CONFIG_SLOT.store(config_slot, Ordering::Relaxed);
    // Java VarpType.java:32 — `numDefinitions = configClient.getFileIdLimit(16)`.
    // Group 16 inside the config archive holds varp records.
    let limit = {
        let reg = crate::js5::js5_net::LOADERS.lock().unwrap();
        reg.get(config_slot as usize)
            .and_then(|o| o.as_ref())
            .map(|l| l.get_file_id_limit(16))
            .unwrap_or(0)
    };
    NUM_DEFINITIONS.store(limit, Ordering::Relaxed);
}
