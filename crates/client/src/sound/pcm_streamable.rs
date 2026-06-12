// @ObfuscatedName("ez") — jag::oldscape::sound::PcmStreamable.
//
// Tiny abstract base — Java extends Linkable to support intrusive
// LinkList membership. The only field is `position`, the running
// sample counter the mixer uses for scheduling.

#![allow(dead_code)]

pub trait PcmStreamable {
    /// @ObfuscatedName("ez.j") — PcmStreamable.position. Sample-index
    /// clock used by Mixer when chaining streams.
    fn position(&self) -> i64;

    /// Advance the clock by `n` samples. Default impl is a no-op so
    /// passive consumers can ignore it.
    fn advance(&mut self, _n: i64) {}
}
