//! Skill XP table — the rev1 scale (`acc/4`, level 2 = 83 xp), matching the
//! client's `crate::skills::SKILLXP` / `base_level_for_xp` (the byte-level
//! truth) rather than Engine-TS's ×10 internal scale. `level_for_xp` and
//! `xp_for_level` drive the stats system.

use std::sync::LazyLock;

/// `SKILLXP[i]` = cumulative XP to reach level `i + 2` (index 0 = level 2).
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

/// Base level corresponding to an absolute XP value (1..=99). Clamped at 99
/// like Engine-TS `getLevelByExp` (`min(i+2, 99)`), since XP runs to 200M
/// while levels cap at 99.
pub fn level_for_xp(xp: i32) -> i32 {
    let mut level = 1i32;
    for (l, &threshold) in SKILLXP.iter().enumerate() {
        if xp >= threshold {
            level = (l as i32 + 2).min(99);
        }
    }
    level
}

/// Minimum XP required to be at `level` (1 → 0).
pub fn xp_for_level(level: i32) -> i32 {
    if level <= 1 {
        0
    } else {
        SKILLXP[(level - 2) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_osrs_thresholds() {
        assert_eq!(xp_for_level(2), 83);
        assert_eq!(xp_for_level(10), 1154);
        assert_eq!(xp_for_level(99), 13_034_431);
        assert_eq!(level_for_xp(0), 1);
        assert_eq!(level_for_xp(82), 1);
        assert_eq!(level_for_xp(83), 2);
        assert_eq!(level_for_xp(1154), 10);
        assert_eq!(level_for_xp(200_000_000), 99);
    }
}
