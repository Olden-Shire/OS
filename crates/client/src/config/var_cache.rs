// @ObfuscatedName("cm") — jag::oldscape::var::VarCache.
//
// Mirrors the server-driven player vars (varps) and their bit-packed
// children (varbits). Engine2007 streams varp updates via opcodes
// (decoded in login.rs); VarBitType.list resolves a varbit to its
// parent varp + bit range.
//
// Java keeps two parallel arrays: `var` (the client's live view) and
// `varServ` (the last-known server value). The VARP_SYNC reconciliation
// at Client.java:5787 / 6342 / 7143 walks both arrays and re-emits any
// `var[i] != varServ[i]` deltas back to the server so the client can
// catch up after a packet drop. Both arrays are size 2000 (Java
// hardcodes this — not bound by VarpType.NUM_DEFINITIONS).
//
// Used by LocType.getMultiLoc: the loc picks one of its `multiloc`
// entries based on `var[multivarp]` or `getVarbit(multivarbit)`. Doors
// open/closed, gates state, machinery states all depend on this.

#![allow(dead_code)]

use std::sync::Mutex;

use crate::config::var_bit_type;

// @ObfuscatedName("cm.r") — precomputed bit-mask table.
//   mask[b] = 2^(b+1) - 1
// So mask[0]=1, mask[1]=3, …, mask[30]=0x7FFFFFFF, mask[31] overflows
// i32 to -1 (which is the full 32-bit mask). Java relies on the
// overflow at index 31; we mirror it.
static MASK: std::sync::LazyLock<[i32; 32]> = std::sync::LazyLock::new(|| {
    let mut acc: i64 = 2;
    let mut m = [0i32; 32];
    for b in 0..32 {
        m[b] = (acc - 1) as i32;
        acc += acc;
    }
    m
});

// Java declares `var` and `varServ` as static `int[2000]`. We keep the
// same length so out-of-range varbit ids land in-bounds the same way
// Java does (Java will OutOfBoundsException on >= 2000; we silently
// return 0 instead, since panicking the renderer over a malformed
// gamepack isn't useful).
const NUM_VARP: usize = 2000;

// @ObfuscatedName("cm.l") — client-side player var array.
pub static VAR: Mutex<[i32; NUM_VARP]> = Mutex::new([0; NUM_VARP]);

// @ObfuscatedName("cm.d") — server-shadow varp array (the values the
// server believes the client has). VARP_SYNC re-syncs from this to
// `var` when reconciliation fires.
pub static VAR_SERV: Mutex<[i32; NUM_VARP]> = Mutex::new([0; NUM_VARP]);

// Direct varp read — Java's `cm.l[id]`.
pub fn get_varp(id: i32) -> i32 {
    if id < 0 || (id as usize) >= NUM_VARP { return 0; }
    let v = VAR.lock().unwrap();
    v[id as usize]
}

// Direct varp write — Java's `cm.l[id] = value`.
pub fn set_varp(id: i32, value: i32) {
    if id < 0 || (id as usize) >= NUM_VARP { return; }
    let mut v = VAR.lock().unwrap();
    v[id as usize] = value;
}

// Server-shadow read — used by VARP_SYNC.
pub fn get_var_serv(id: i32) -> i32 {
    if id < 0 || (id as usize) >= NUM_VARP { return 0; }
    let v = VAR_SERV.lock().unwrap();
    v[id as usize]
}

// Server-shadow write — VARP_SYNC and SETVAR opcodes.
pub fn set_var_serv(id: i32, value: i32) {
    if id < 0 || (id as usize) >= NUM_VARP { return; }
    let mut v = VAR_SERV.lock().unwrap();
    v[id as usize] = value;
}

// @ObfuscatedName("cc.r(II)I") — VarCache.getVarbit. Verbatim port:
//   mask[endbit - startbit]
//   var[basevar] >> startbit & mask
// (Note: Java uses [endbit - startbit], NOT [endbit - startbit + 1].)
pub fn get_varbit(id: i32) -> i32 {
    if id < 0 { return 0; }
    let vb = var_bit_type::list(id);
    if vb.basevar < 0 { return 0; }
    let idx = (vb.endbit - vb.startbit) as usize;
    let mask = MASK[idx.min(31)];
    (get_varp(vb.basevar) >> vb.startbit) & mask
}

// @ObfuscatedName("client.varcInt[]") — cs2 client-side int vars.
// Java stores a fixed-size int[] keyed by id; we mirror with a Vec
// that grows on demand. These are *not* server-replicated — they're
// strictly cs2 scratch space (UI state, drag offsets, etc).
pub static VARC_INT: std::sync::LazyLock<Mutex<Vec<i32>>> =
    std::sync::LazyLock::new(|| Mutex::new(vec![0i32; 2048]));

// @ObfuscatedName("client.varcStr[]") — same as VARC_INT but for
// strings. Null in Java; we keep "" and map to "null" on read per
// Java's `value == null ? "null" : value` semantic.
pub static VARC_STR: std::sync::LazyLock<Mutex<Vec<Option<String>>>> =
    std::sync::LazyLock::new(|| Mutex::new(vec![None; 2048]));

pub fn get_varc_int(id: i32) -> i32 {
    if id < 0 { return 0; }
    let s = VARC_INT.lock().unwrap();
    s.get(id as usize).copied().unwrap_or(0)
}

pub fn set_varc_int(id: i32, value: i32) {
    if id < 0 { return; }
    let mut s = VARC_INT.lock().unwrap();
    if (id as usize) >= s.len() {
        s.resize((id as usize) + 1, 0);
    }
    s[id as usize] = value;
}

// Java returns "null" for unset / null values.
pub fn get_varc_str(id: i32) -> String {
    if id < 0 { return "null".to_string(); }
    let s = VARC_STR.lock().unwrap();
    s.get(id as usize)
        .and_then(|o| o.clone())
        .unwrap_or_else(|| "null".to_string())
}

pub fn set_varc_str(id: i32, value: Option<String>) {
    if id < 0 { return; }
    let mut s = VARC_STR.lock().unwrap();
    if (id as usize) >= s.len() {
        s.resize((id as usize) + 1, None);
    }
    s[id as usize] = value;
}

// @ObfuscatedName("cc.d(III)V") — VarCache.setVarbit. Verbatim port:
//   value &= mask                       (clamp to bit width)
//   varp = (varp & ~(mask << startbit)) | (value << startbit)
// Used by RUNCLIENTSCRIPT to mutate varbits without round-tripping
// through the server.
pub fn set_varbit(id: i32, value: i32) {
    if id < 0 { return; }
    let vb = var_bit_type::list(id);
    if vb.basevar < 0 { return; }
    let idx = (vb.endbit - vb.startbit) as usize;
    let mask = MASK[idx.min(31)];
    let clamped = value & mask;
    let current = get_varp(vb.basevar);
    let cleared = current & !(mask << vb.startbit);
    let updated = cleared | (clamped << vb.startbit);
    set_varp(vb.basevar, updated);
}
