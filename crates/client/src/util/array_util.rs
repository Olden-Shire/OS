// @ObfuscatedName("p") — jag::oldscape::util::ArrayUtil.
//
// Tiny static helpers — zero-fill (the 8-unrolled loop in Java is
// just a perf hand-tuning; Rust's `fill(0)` lowers to the equivalent
// vectorised intrinsic on debug+).

#![allow(dead_code)]

// @ObfuscatedName("p.r([II)V") — ArrayUtil.clear (whole buffer).
pub fn clear(buf: &mut [i32]) {
    buf.fill(0);
}

// @ObfuscatedName("bj.q([III)V") — ArrayUtil.clear (range overload).
// Verbatim port of ArrayUtil.java:13-29 — zeros buf[start..start+len].
// The 8-wide unroll in Java is just a perf hand-tuning; Rust's
// slice fill compiles to the equivalent vectorised intrinsic.
pub fn clear_range(buf: &mut [i32], start: usize, len: usize) {
    let end = (start + len).min(buf.len());
    if start < end {
        buf[start..end].fill(0);
    }
}
