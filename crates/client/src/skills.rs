// @ObfuscatedName("bm") — jag::oldscape::constants::skills.
//
// Static lookup tables for the skill system. `SKILLXP[i]` is the
// cumulative XP threshold to reach level i+2 — index 0 holds "xp
// needed for level 2" and index 98 holds "xp needed for level 99".
//
// Verbatim port of Skills.java:18-28.

#![allow(dead_code)]

use std::sync::LazyLock;

// @ObfuscatedName("bm.d") — Skills.used. 25-entry flag table; only
// the last two (Sailing / placeholders) are false in rev1.
pub const USED: [bool; 25] = [
    true, true, true, true, true, true, true, true, true, true,
    true, true, true, true, true, true, true, true, true, true,
    true, true, true, false, false,
];

// @ObfuscatedName("bm.l") — Skills.skillxp. Computed once at startup
// via the formula:
//     var2 = i + 1
//     var3 = (int)(var2 + Math.pow(2.0, var2 / 7.0) * 300.0)
//     var0 += var3
//     skillxp[i] = var0 / 4
pub static SKILLXP: LazyLock<[i32; 99]> = LazyLock::new(|| {
    let mut out = [0i32; 99];
    let mut acc = 0i64;
    for i in 0..99 {
        let v2 = (i + 1) as f64;
        let v3 = (v2 + (2.0f64).powf(v2 / 7.0) * 300.0) as i32;
        acc += v3 as i64;
        out[i] = (acc / 4) as i32;
    }
    out
});

// Helper used by the UPDATE_STAT packet decoder — walks SKILLXP to
// find the base level corresponding to an absolute xp value. Mirrors
// Java's inline loop at Client.java:6425-6429.
pub fn base_level_for_xp(xp: i32) -> i32 {
    let table = &*SKILLXP;
    let mut level = 1i32;
    for (l, &threshold) in table.iter().enumerate() {
        if xp >= threshold {
            level = (l as i32) + 2;
        }
    }
    level
}
