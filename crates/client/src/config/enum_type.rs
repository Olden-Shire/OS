// @ObfuscatedName("fe") — jag::oldscape::configdecoder::EnumType
//
// Enum / lookup table loaded from config archive group 8. Maps an
// integer key onto either an int value or a string value (input /
// output type bytes pick which) with a default fallback.

#![allow(dead_code)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::io::packet::Packet;
use crate::js5::js5_loader::Js5Loader;
use crate::js5::js5_net;

// @ObfuscatedName("fe.n")
pub static CONFIG_SLOT: AtomicI32 = AtomicI32::new(-1);

#[derive(Debug, Clone, Default)]
pub struct EnumType {
    // @ObfuscatedName("fe.z")
    pub inputtype: i32,
    // @ObfuscatedName("fe.g")
    pub outputtype: i32,
    // @ObfuscatedName("fe.q")
    pub default_string: String,
    // @ObfuscatedName("fe.i")
    pub default_int: i32,
    // @ObfuscatedName("fe.s")
    pub count: i32,
    // @ObfuscatedName("fe.u")
    pub keys: Vec<i32>,
    // @ObfuscatedName("fe.v")
    pub int_values: Vec<i32>,
    // @ObfuscatedName("fe.w")
    pub string_values: Vec<String>,
}

impl EnumType {
    pub fn new() -> Self {
        Self { default_string: "null".to_string(), ..Default::default() }
    }
    // @ObfuscatedName("fe.g(Lev;I)V") — EnumType.decode loop
    pub fn decode_all(&mut self, p: &mut Packet) {
        loop {
            let c = p.g1();
            if c == 0 { return; }
            self.decode(p, c);
        }
    }
    // @ObfuscatedName("fe.q(Lev;IB)V") — EnumType.decode(opcode)
    pub fn decode(&mut self, p: &mut Packet, code: i32) {
        match code {
            1 => { self.inputtype = p.g1(); }
            2 => { self.outputtype = p.g1(); }
            3 => { self.default_string = p.gjstr(); }
            4 => { self.default_int = p.g4(); }
            5 => {
                self.count = p.g2();
                self.keys = Vec::with_capacity(self.count as usize);
                self.string_values = Vec::with_capacity(self.count as usize);
                for _ in 0..self.count {
                    self.keys.push(p.g4());
                    self.string_values.push(p.gjstr());
                }
            }
            6 => {
                self.count = p.g2();
                self.keys = Vec::with_capacity(self.count as usize);
                self.int_values = Vec::with_capacity(self.count as usize);
                for _ in 0..self.count {
                    self.keys.push(p.g4());
                    self.int_values.push(p.g4());
                }
            }
            _ => {}
        }
    }

    // Linear-scan key→int lookup. Java ScriptRunner inlines this at
    // every enum opcode site; we hoist for symmetry with get_string_value
    // and to give callers a tighter interface. Returns default_int
    // when key is not present.
    pub fn get_int_value(&self, key: i32) -> i32 {
        for (i, &k) in self.keys.iter().enumerate() {
            if k == key {
                return self.int_values.get(i).copied().unwrap_or(self.default_int);
            }
        }
        self.default_int
    }

    // Linear-scan key→string lookup. Returns default_string when key
    // is not present.
    pub fn get_string_value(&self, key: i32) -> String {
        for (i, &k) in self.keys.iter().enumerate() {
            if k == key {
                return self.string_values.get(i).cloned().unwrap_or_else(|| self.default_string.clone());
            }
        }
        self.default_string.clone()
    }
}

pub struct EnumStore { pub map: std::collections::HashMap<i32, EnumType> }
pub static STORE: std::sync::LazyLock<Mutex<EnumStore>> =
    std::sync::LazyLock::new(|| Mutex::new(EnumStore { map: std::collections::HashMap::new() }));

// jagex3.config.EnumType.resetCache — wired into the config-wide
// reset sweep that runs on logout / world hop.
pub fn reset_cache() {
    STORE.lock().unwrap().map.clear();
}

// @ObfuscatedName("ek.z(II)Lfe;") — EnumType.list(id)
pub fn list(id: i32) -> EnumType {
    {
        let s = STORE.lock().unwrap();
        if let Some(t) = s.map.get(&id) { return t.clone(); }
    }
    let bytes_opt = {
        let cs = CONFIG_SLOT.load(Ordering::Relaxed);
        if cs < 0 { None }
        else {
            let mut reg = js5_net::LOADERS.lock().unwrap();
            reg.get_mut(cs as usize).and_then(|o| o.as_mut()).and_then(|l| l.fetch_file(8, id))
        }
    };
    let Some(bytes) = bytes_opt else { return EnumType::new(); };
    let mut t = EnumType::new();
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
