// @ObfuscatedName("aw") — jag::oldscape::sound::PcmStream (abstract).
//
// One active voice on the mix bus. Each stream returns PCM samples
// when `do_mix` is called; Mixer calls it once per audio block and
// drops the stream when `do_mix` returns `true`.

#![allow(dead_code)]

use super::pcm_streamable::PcmStreamable;

pub trait PcmStream: PcmStreamable {
    /// Mix this stream's samples into `buf[off..off+len]`. Java's
    /// abstract `int doMix(int[], int, int)` returns -1 when the
    /// stream is done; we return `true` for the same outcome.
    fn do_mix(&mut self, buf: &mut [i32], off: i32, len: i32) -> bool;

    /// @ObfuscatedName("aw.f(I)V") — PcmStream.pretendToMix(int len).
    /// Advance the stream's internal clock by `len` samples without
    /// emitting audio. Used when the stream is masked / muted.
    fn pretend_to_mix(&mut self, _len: i32) {}

    /// @ObfuscatedName("aw.k()Lez;") — PcmStream.substream.
    /// Some streams (Java's MidiMixer / Mixer-wrapped voices) expose
    /// a chained inner stream. Default returns None.
    fn substream(&mut self) -> Option<&mut dyn PcmStream> { None }

    /// @ObfuscatedName("dx.c()I") — PcmStream.priority. Verbatim
    /// port of PcmStream.java:23-26 — base priority for an active
    /// voice in the mix bus's priority-bucket queue. Subclasses
    /// (WaveStream) override to scale by volume/age. The base 255
    /// sentinel keeps unprioritised voices at the lowest bucket.
    fn priority(&self) -> i32 { 255 }

    /// @ObfuscatedName("dx.g([III)V") — PcmStream.maybeMix. Verbatim
    /// port of PcmStream.java:30-36. Routes to `do_mix` when `active`,
    /// otherwise to `pretend_to_mix` (advances the clock without
    /// emitting audio). Subclasses generally don't override this —
    /// they override the two underlying methods.
    fn maybe_mix(&mut self, buf: &mut [i32], off: i32, len: i32) -> bool {
        if self.is_active() {
            self.do_mix(buf, off, len)
        } else {
            self.pretend_to_mix(len);
            false
        }
    }

    /// @ObfuscatedName("aw.j()Z") — PcmStream.isActive. Default
    /// returns true; voices that go silent should override to false
    /// so maybe_mix routes through pretend_to_mix.
    fn is_active(&self) -> bool { true }

    /// Set the voice's volume (Java's raw `applyVolume(int)` scale).
    /// Default no-op; `WaveStream` overrides. Used by BgSound to fade
    /// positional ambient loops by player distance.
    fn set_secondary_volume(&mut self, _vol: i32) {}
}
