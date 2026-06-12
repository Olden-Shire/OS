// @ObfuscatedName("fc") — jag::oldscape::configdecoder::VarBitType
//
// Bit-packed slice of a VarpType. Config archive group 14. Holds
// (basevar, startbit, endbit) which slice a sub-range of the parent
// varp value.

#![allow(dead_code)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::io::packet::Packet;
use crate::js5::js5_loader::Js5Loader;
use crate::js5::js5_net;

// @ObfuscatedName("fc.n")
pub static CONFIG_SLOT: AtomicI32 = AtomicI32::new(-1);

#[derive(Debug, Clone, Default)]
pub struct VarBitType {
    // @ObfuscatedName("fc.z")
    pub basevar: i32,
    // @ObfuscatedName("fc.g")
    pub startbit: i32,
    // @ObfuscatedName("fc.q")
    pub endbit: i32,
}

impl VarBitType {
    pub fn new() -> Self { Self::default() }
    // @ObfuscatedName("fc.g(Lev;B)V") — VarBitType.decode loop
    pub fn decode_all(&mut self, p: &mut Packet) {
        loop {
            let c = p.g1();
            if c == 0 { return; }
            self.decode(p, c);
        }
    }
    // @ObfuscatedName("fc.q(Lev;II)V") — VarBitType.decode(opcode)
    pub fn decode(&mut self, p: &mut Packet, code: i32) {
        if code == 1 {
            self.basevar = p.g2();
            self.startbit = p.g1();
            self.endbit = p.g1();
        }
    }
}

pub struct VarBitStore { pub map: std::collections::HashMap<i32, VarBitType> }
pub static STORE: std::sync::LazyLock<Mutex<VarBitStore>> =
    std::sync::LazyLock::new(|| Mutex::new(VarBitStore { map: std::collections::HashMap::new() }));

// @ObfuscatedName("q.z(II)Lfc;") — VarBitType.list(id)
pub fn list(id: i32) -> VarBitType {
    {
        let s = STORE.lock().unwrap();
        if let Some(t) = s.map.get(&id) { return t.clone(); }
    }
    let bytes_opt = {
        let cs = CONFIG_SLOT.load(Ordering::Relaxed);
        if cs < 0 { None }
        else {
            let mut reg = js5_net::LOADERS.lock().unwrap();
            reg.get_mut(cs as usize).and_then(|o| o.as_mut()).and_then(|l| l.fetch_file(14, id))
        }
    };
    // Don't cache a failed fetch — retry next call.
    let Some(bytes) = bytes_opt else { return VarBitType::new(); };
    let mut t = VarBitType::new();
    let mut p = Packet::from_vec(bytes);
    t.decode_all(&mut p);
    let cloned = t.clone();
    STORE.lock().unwrap().map.insert(id, t);
    cloned
}

// @ObfuscatedName("q.r()V") — VarBitType.resetCache. Verbatim port
// of VarBitType.java:75-77. Drops every cached entry; wired into
// the Config-wide reset sweep that runs on logout / world hop.
pub fn reset_cache() {
    STORE.lock().unwrap().map.clear();
}

pub fn init(_archive: &Js5Loader) {}

pub fn install_archives(config_slot: i32) {
    CONFIG_SLOT.store(config_slot, Ordering::Relaxed);
}
