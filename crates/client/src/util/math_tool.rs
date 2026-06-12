// @ObfuscatedName("ai") — jag::oldscape::util::MathTool.
//
// Tiny static helpers — HCF, bit counters. Decimator uses `hcf` to
// build its resample table; getPlayerPosNewVis uses `bitsRequired` to
// size relative-position deltas.

#![allow(dead_code)]

// @ObfuscatedName("ai.r(II)I") — MathTool.hcf. Euclidean GCD.
pub fn hcf(mut a: i32, mut b: i32) -> i32 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a.abs()
}

// @ObfuscatedName("ai.d(I)I") — MathTool.bitCount via SWAR popcount.
pub fn bit_count(mut n: i32) -> i32 {
    n -= (n >> 1) & 0x5555_5555;
    n = (n & 0x3333_3333) + ((n >> 2) & 0x3333_3333);
    n = (n + (n >> 4)) & 0x0F0F_0F0F;
    n += n >> 8;
    n += n >> 16;
    n & 0x3F
}

// @ObfuscatedName("ai.l(I)I") — MathTool.bitsRequired. Smallest N
// such that 2^N >= value (count of bits to encode value).
pub fn bits_required(value: i32) -> i32 {
    let mut bits = 0;
    let mut v = value as u32;
    while v != 0 {
        v >>= 1;
        bits += 1;
    }
    bits
}
