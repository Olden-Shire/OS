// @ObfuscatedName("ep") — jag::oldscape::ClientScript
//
// cs2 script container + cache. Scripts live in JS5 archive 12
// (Client.scripts); each file is bytecode with a 12-byte trailer:
// g4 instructionCount, g2 intLocalCount, g2 stringLocalCount,
// g2 intArgCount, g2 stringArgCount. Verbatim port of
// ClientScript.java.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex};

use crate::io::packet::Packet;
use crate::js5::js5_net;

pub struct ClientScript {
    // @ObfuscatedName("ep.j")
    pub instructions: Vec<i32>,
    // @ObfuscatedName("ep.z")
    pub int_operands: Vec<i32>,
    // @ObfuscatedName("ep.g")
    pub string_operands: Vec<Option<String>>,
    // @ObfuscatedName("ep.q")
    pub int_local_count: usize,
    // @ObfuscatedName("ep.i")
    pub string_local_count: usize,
    // @ObfuscatedName("ep.s")
    pub int_arg_count: usize,
    // @ObfuscatedName("ep.u")
    pub string_arg_count: usize,
    // "was in 468, not in os" — official caches ship no names.
    pub name: Option<String>,
    // Cache key (the script id) — used by the error reporter.
    pub key: i32,
}

// @ObfuscatedName("ep.n") — LruCache(128); plain map until the LRU
// port lands, matching the loc/obj model cache convention.
pub static CACHE: std::sync::LazyLock<Mutex<HashMap<i32, Arc<ClientScript>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

// JS5 slot for archive 12 — published by Client after openJs5 so the
// interpreter doesn't need &Client to resolve scripts.
pub static SCRIPTS_SLOT: AtomicI32 = AtomicI32::new(-1);

pub fn install_archive(scripts_slot: i32) {
    SCRIPTS_SLOT.store(scripts_slot, Ordering::Relaxed);
}

pub fn reset_cache() {
    CACHE.lock().unwrap().clear();
}

// @ObfuscatedName("bq.z(II)Lep;") — ClientScript.get. Verbatim port of
// ClientScript.java:40-85.
pub fn get(id: i32) -> Option<Arc<ClientScript>> {
    if let Some(s) = CACHE.lock().unwrap().get(&id) {
        return Some(Arc::clone(s));
    }

    let data = {
        let slot = SCRIPTS_SLOT.load(Ordering::Relaxed);
        if slot < 0 {
            return None;
        }
        let mut reg = js5_net::LOADERS.lock().unwrap();
        let loader = reg.get_mut(slot as usize).and_then(|o| o.as_mut())?;
        loader.fetch_file(id, 0)?
    };
    if data.len() < 12 {
        return None;
    }

    let mut buf = Packet::from_vec(data);
    let total = buf.data.len();
    buf.pos = total as i32 - 12;

    let instruction_count = buf.g4() as usize;
    let int_local_count = buf.g2() as usize;
    let string_local_count = buf.g2() as usize;
    let int_arg_count = buf.g2() as usize;
    let string_arg_count = buf.g2() as usize;

    // A corrupt trailer would make the decode loop run off the end;
    // Java would AIOOBE inside executeScript's catch — bail here.
    if instruction_count > total {
        return None;
    }

    buf.pos = 0;
    let name = buf.fastgstr();

    let mut script = ClientScript {
        instructions: vec![0; instruction_count],
        int_operands: vec![0; instruction_count],
        string_operands: vec![None; instruction_count],
        int_local_count,
        string_local_count,
        int_arg_count,
        string_arg_count,
        name,
        key: id,
    };

    let mut i = 0usize;
    while (buf.pos as usize) < total - 12 && i < instruction_count {
        let op = buf.g2();
        if op == 3 {
            script.string_operands[i] = Some(buf.gjstr());
        } else if op >= 100 || op == 21 || op == 38 || op == 39 {
            script.int_operands[i] = buf.g1();
        } else {
            script.int_operands[i] = buf.g4();
        }
        script.instructions[i] = op;
        i += 1;
    }

    let arc = Arc::new(script);
    CACHE.lock().unwrap().insert(id, Arc::clone(&arc));
    Some(arc)
}
