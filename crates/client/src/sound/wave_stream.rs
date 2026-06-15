// @ObfuscatedName("aw")
// jag::oldscape::sound::WaveStream — single playing voice.
//
// Java's WaveStream has 16 hand-unrolled mixer variants (forward/backward,
// 1:1 vs resampled, mono vs stereo, ramping vs steady). We collapse to
// one unified mixer that handles the same semantics — stereo output with
// linear interpolation, per-sample volume/pan ramping, loop/reverse — and
// matches Java's `>> 6` mixing scale and signed-byte sample format.

#![allow(dead_code)]

use std::sync::Arc;

use crate::sound::wave::Wave;

pub struct WaveStream {
    // @ObfuscatedName("aw.r")
    pub sound: Arc<Wave>,
    // @ObfuscatedName("aw.d") — position in samples × 256
    pub position: i32,
    // @ObfuscatedName("aw.l") — pitch in samples × 256 per output sample. Negative = reverse.
    pub pitch: i32,
    // @ObfuscatedName("aw.m")
    pub volume: i32,
    // @ObfuscatedName("aw.c")
    pub pan: i32,
    // @ObfuscatedName("aw.n")
    pub volume_mono: i32,
    // @ObfuscatedName("aw.j")
    pub volume_left: i32,
    // @ObfuscatedName("aw.z")
    pub volume_right: i32,
    // @ObfuscatedName("aw.g")
    pub loop_count: i32,
    // @ObfuscatedName("aw.q")
    pub loop_start: i32,
    // @ObfuscatedName("aw.i")
    pub loop_end: i32,
    // @ObfuscatedName("aw.s")
    pub loop_reversed: bool,
    // @ObfuscatedName("aw.u")
    pub volume_change_delta: i32,
    // @ObfuscatedName("aw.v")
    pub vc_speed_mono: i32,
    // @ObfuscatedName("aw.w")
    pub vc_speed_left: i32,
    // @ObfuscatedName("aw.e")
    pub vc_speed_right: i32,
    pub active: bool,
    pub finished: bool,
}

const VOLUME_RAMP_OUT_MARKER: i32 = i32::MIN;

impl WaveStream {
    pub fn new_rate_fine_vol_pan(sound: Arc<Wave>, pitch: i32, volume: i32, pan: i32) -> Self {
        let loop_start = sound.loop_start_position;
        let loop_end = sound.loop_end_position;
        let loop_reversed = sound.loop_reversed;
        let mut me = Self {
            sound, position: 0, pitch, volume, pan,
            volume_mono: 0, volume_left: 0, volume_right: 0,
            loop_count: 0, loop_start, loop_end, loop_reversed,
            volume_change_delta: 0,
            vc_speed_mono: 0, vc_speed_left: 0, vc_speed_right: 0,
            active: true, finished: false,
        };
        me.set_mlr_vol();
        me
    }

    pub fn set_mlr_vol(&mut self) {
        self.volume_mono = self.volume;
        self.volume_left = get_l_vol(self.volume, self.pan);
        self.volume_right = get_r_vol(self.volume, self.pan);
    }

    pub fn set_loop_count(&mut self, n: i32) { self.loop_count = n; }
    pub fn set_rate_raw(&mut self, rate: i32) {
        if self.pitch < 0 { self.pitch = -rate; } else { self.pitch = rate; }
    }

    // @ObfuscatedName("aw.aw(Leq;III)Law;") — WaveStream.newRatePercent.
    // Java alt constructor: rate is given as a percent of the wave's
    // native sampling frequency. Used by BgSound when looping ambient
    // wavefiles at "100%" speed. SIMPLIFIED FORMULA: does not match
    // Java semantics — Java uses (samplingFrequency * 256 * rate) /
    // (PcmPlayer.frequency * 100). Use `new_rate_percent_full` for
    // Java-faithful behavior.
    pub fn new_rate_percent(sound: Arc<Wave>, rate_percent: i32, volume: i32, pan: i32) -> Self {
        let fine = (sound.sampling_frequency * rate_percent / 100).max(1);
        Self::new_rate_fine_vol_pan(sound, fine, volume, pan)
    }

    // @ObfuscatedName("et.ac(Leq;II)Let;") — WaveStream.newRatePercent.
    // Verbatim port of WaveStream.java:104-107. Java-faithful version
    // taking the PcmPlayer's output frequency as an argument (in Java
    // it reads the static PcmPlayer.frequency). Returns None when the
    // wave has no samples (Java returns null).
    //
    // Formula: pitch = (samplingFrequency * 256 * rate_percent) /
    //                  (pcm_frequency * 100)
    // and volume is left-shifted 6 bits to match Java's 16384-volume
    // scaling.
    pub fn new_rate_percent_full(
        sound: Arc<Wave>, rate_percent: i32, volume: i32,
        pcm_frequency: i32,
    ) -> Option<Self> {
        if sound.samples.is_empty() { return None; }
        let pitch = ((sound.sampling_frequency as i64) * 256i64 * (rate_percent as i64)
            / ((pcm_frequency as i64) * 100i64)) as i32;
        Some(Self::new_rate_fine_vol_pan(sound, pitch, volume << 6, 8192))
    }

    // @ObfuscatedName("aw.am(I)V") — WaveStream.setPosition. Seeks to
    // an absolute sample position (Java's units; we multiply by 256
    // to match the fixed-point format the mixer iterates in).
    pub fn set_position(&mut self, sample: i32) {
        self.position = sample << 8;
    }

    // @ObfuscatedName("aw.ay(Z)V") — WaveStream.setReverse. Flips the
    // pitch sign so the mixer steps backward through `samples`.
    pub fn set_reverse(&mut self, reverse: bool) {
        if reverse {
            if self.pitch > 0 { self.pitch = -self.pitch; }
        } else if self.pitch < 0 {
            self.pitch = -self.pitch;
        }
    }

    // @ObfuscatedName("aw.ap(II)V") — WaveStream.applyVolume. Direct
    // (non-ramped) volume / pan assignment.
    pub fn apply_volume(&mut self, vol: i32, pan: i32) {
        self.volume = vol;
        self.pan = pan;
        self.volume_change_delta = 0;
        self.set_mlr_vol();
    }

    // @ObfuscatedName("et.applyVolume(I)V") — WaveStream.applyVolume(int).
    // Verbatim port of WaveStream.java:131-133: `setVolPanFine(vol << 6,
    // getPanFine())` — the raw (un-shifted) volume convenience used by
    // BgSound; keeps the current pan.
    pub fn apply_volume_int(&mut self, vol: i32) {
        let pan = self.get_pan_fine();
        self.apply_volume(vol << 6, pan);
    }

    // @ObfuscatedName("et.av(I)V") — WaveStream.setVolumeFine. Verbatim
    // port of WaveStream.java:137-138. Keeps the current pan but
    // updates the volume to the fine value (already in 6-bit-shifted
    // scale, matching Java's applyVolume convention).
    pub fn set_volume_fine(&mut self, vol: i32) {
        let pan = self.get_pan_fine();
        self.apply_volume(vol, pan);
    }

    // @ObfuscatedName("et.az()I") — WaveStream.getVolumeFine. Verbatim
    // port of WaveStream.java:152-154. Folds the i32::MIN sentinel (used
    // during fades) back into 0 so callers see a sane volume.
    pub fn get_volume_fine(&self) -> i32 {
        if self.volume == i32::MIN { 0 } else { self.volume }
    }

    // @ObfuscatedName("et.an()I") — WaveStream.getPanFine. Verbatim
    // port of WaveStream.java:158-160. Clamps the sentinel -2 (pan
    // not set yet) to -1 so it's safely usable as a signed offset.
    pub fn get_pan_fine(&self) -> i32 {
        if self.pan < 0 { -1 } else { self.pan }
    }

    // @ObfuscatedName("et.al()V") — WaveStream.skipRampNounLink.
    // Verbatim port of WaveStream.java:186-197. Snaps a pending volume
    // ramp to its final value (without removing the voice from the
    // mixer list). Called by MidiPlayer.allSoundOff prep paths that
    // re-add the still-ramping voice for one more tick.
    pub fn skip_ramp_no_unlink(&mut self) {
        if self.volume_change_delta == 0 { return; }
        if self.volume == i32::MIN { self.volume = 0; }
        self.volume_change_delta = 0;
        self.set_mlr_vol();
    }

    // @ObfuscatedName("aw.av()Z") — WaveStream.isRamping.
    pub fn is_ramping(&self) -> bool {
        self.volume_change_delta > 0
    }

    // @ObfuscatedName("et.c()I") — WaveStream.priority. Verbatim port
    // of WaveStream.java:67-77. Returns the 0-255 priority score the
    // mixer uses for voice culling: starts from `volume_mono*3 >> 6`
    // (abs), then attenuates by position progress for non-looped
    // voices, or by loop-start displacement for looped voices.
    // Higher = more important / less likely to drop.
    pub fn priority(&self) -> i32 {
        let mut var1 = (self.volume_mono * 3) >> 6;
        // Java's `(x >>> 31) + (x ^ x >> 31)` is an arithmetic-shift
        // abs idiom; .abs() does the same for non-INT_MIN values.
        var1 = var1.wrapping_abs();
        let samples_len = self.sound.samples.len() as i32;
        if self.loop_count == 0 {
            if samples_len > 0 {
                var1 -= self.position * var1 / (samples_len << 8);
            }
        } else if self.loop_count >= 0 && samples_len > 0 {
            var1 -= self.loop_start * var1 / samples_len;
        }
        if var1 > 255 { 255 } else { var1 }
    }

    // @ObfuscatedName("et.z()I") — WaveStream.selfMixCost. Verbatim
    // port of WaveStream.java:325-328. Silent voices (volume==0 with
    // no pending ramp) cost nothing toward the mixer's max-cost cap;
    // every other voice costs 1. Java differs from the previous
    // Rust impl that returned 1 or 2.
    pub fn self_mix_cost(&self) -> i32 {
        if self.volume == 0 && self.volume_change_delta == 0 { 0 } else { 1 }
    }

    // @ObfuscatedName("et.aq()I") — WaveStream.getRateRaw. Verbatim
    // port of WaveStream.java:295-298. Returns |pitch| since reverse
    // playback is encoded as a negative pitch.
    pub fn get_rate_raw(&self) -> i32 {
        self.pitch.wrapping_abs()
    }

    // Pure loop-span predicate. Both do_mix and pretend_to_mix
    // inline `(loop_end - loop_start) > 0` as the gate for loop
    // arithmetic; this is the named version (no @ObfuscatedName in
    // Java — extracted from inline use sites).
    pub fn loop_span_valid(&self) -> bool {
        self.loop_end - self.loop_start > 0
    }

    pub fn ramp_vol_pan_fine(&mut self, ramp_len: i32, vol: i32, pan: i32) {
        if ramp_len == 0 {
            self.volume = vol; self.pan = pan; self.volume_change_delta = 0; self.set_mlr_vol();
            return;
        }
        let new_l = get_l_vol(vol, pan);
        let new_r = get_r_vol(vol, pan);
        if self.volume_left == new_l && self.volume_right == new_r {
            self.volume_change_delta = 0;
            return;
        }
        let mut limit = (vol - self.volume_mono).abs();
        limit = limit.max((new_l - self.volume_left).abs());
        limit = limit.max((new_r - self.volume_right).abs());
        let used = ramp_len.min(limit.max(1));
        self.volume_change_delta = used;
        self.volume = vol; self.pan = pan;
        self.vc_speed_mono = (vol - self.volume_mono) / used;
        self.vc_speed_left = (new_l - self.volume_left) / used;
        self.vc_speed_right = (new_r - self.volume_right) / used;
    }

    pub fn ramp_out(&mut self, ramp_len: i32) {
        if ramp_len == 0 {
            self.volume = 0; self.volume_change_delta = 0; self.set_mlr_vol(); self.finished = true;
            return;
        }
        if self.volume_left == 0 && self.volume_right == 0 {
            self.volume_change_delta = 0;
            self.volume = 0; self.volume_mono = 0;
            self.finished = true;
            return;
        }
        let mut limit = self.volume_mono.abs();
        limit = limit.max(self.volume_left.abs()).max(self.volume_right.abs());
        let used = ramp_len.min(limit.max(1));
        self.volume_change_delta = used;
        self.volume = VOLUME_RAMP_OUT_MARKER;
        self.vc_speed_mono = -self.volume_mono / used;
        self.vc_speed_left = -self.volume_left / used;
        self.vc_speed_right = -self.volume_right / used;
    }

    pub fn is_finished(&self) -> bool {
        self.finished
            || self.position < 0
            || self.position >= (self.sound.samples.len() as i32) << 8
    }

    pub fn do_mix(&mut self, out: &mut [i32], frame_start: usize, n: usize) {
        if self.finished { return; }
        if self.volume == 0 && self.volume_change_delta == 0 {
            self.pretend_to_mix(n as i32);
            return;
        }
        let total_samples = self.sound.samples.len() as i32;
        if total_samples == 0 { self.finished = true; return; }
        let max_pos = total_samples << 8;
        let loop_lo = self.loop_start << 8;
        let loop_hi = self.loop_end << 8;
        let loop_span = loop_hi - loop_lo;
        if loop_span <= 0 { self.loop_count = 0; }
        if self.position < 0 {
            if self.pitch <= 0 { self.finished = true; return; }
            self.position = 0;
        }
        if self.position >= max_pos {
            if self.pitch >= 0 { self.finished = true; return; }
            self.position = max_pos - 1;
        }
        let samples = Arc::clone(&self.sound);
        let samples = &samples.samples[..];
        let mut frames_done = 0usize;
        while frames_done < n {
            let mut chunk = n - frames_done;
            if self.loop_count != 0 && loop_span > 0 {
                let bound = if self.pitch >= 0 { loop_hi } else { loop_lo };
                let pitch_mag = self.pitch.unsigned_abs() as i32;
                if pitch_mag > 0 {
                    let dist = if self.pitch >= 0 {
                        ((bound - self.position - 1) / pitch_mag).max(0) + 1
                    } else {
                        ((self.position - bound) / pitch_mag).max(0) + 1
                    };
                    chunk = chunk.min(dist as usize);
                }
            }
            for f in 0..chunk {
                let idx = (self.position >> 8) as usize;
                let frac = (self.position & 0xFF) as i32;
                if idx >= samples.len() { self.finished = true; break; }
                let s0 = samples[idx] as i32;
                let s1 = if idx + 1 < samples.len() { samples[idx + 1] as i32 } else { s0 };
                let interp = (s0 << 8) + frac * (s1 - s0);
                let l;
                let r;
                if self.volume_change_delta > 0 {
                    l = self.volume_left * interp >> 6;
                    r = self.volume_right * interp >> 6;
                    self.volume_left += self.vc_speed_left;
                    self.volume_right += self.vc_speed_right;
                    self.volume_mono += self.vc_speed_mono;
                    self.volume_change_delta -= 1;
                    if self.volume_change_delta == 0 {
                        self.finalise_ramp();
                        if self.finished {
                            let pos = (frame_start + frames_done + f) * 2;
                            if pos + 1 < out.len() {
                                out[pos] = out[pos].wrapping_add(l);
                                out[pos + 1] = out[pos + 1].wrapping_add(r);
                            }
                            self.position = self.position.wrapping_add(self.pitch);
                            return;
                        }
                    }
                } else {
                    l = self.volume_left * interp >> 6;
                    r = self.volume_right * interp >> 6;
                }
                let pos = (frame_start + frames_done + f) * 2;
                if pos + 1 < out.len() {
                    out[pos] = out[pos].wrapping_add(l);
                    out[pos + 1] = out[pos + 1].wrapping_add(r);
                }
                self.position = self.position.wrapping_add(self.pitch);
            }
            frames_done += chunk;
            if self.loop_count != 0 && loop_span > 0 {
                if self.pitch >= 0 && self.position >= loop_hi {
                    if self.loop_reversed {
                        self.position = loop_hi + loop_hi - 1 - self.position;
                        self.pitch = -self.pitch;
                    } else {
                        self.position = ((self.position - loop_lo) % loop_span) + loop_lo;
                    }
                    if self.loop_count > 0 { self.loop_count -= 1; }
                } else if self.pitch < 0 && self.position < loop_lo {
                    if self.loop_reversed {
                        self.position = loop_lo + loop_lo - 1 - self.position;
                        self.pitch = -self.pitch;
                    } else {
                        self.position = loop_hi - 1 - ((loop_hi - 1 - self.position) % loop_span);
                    }
                    if self.loop_count > 0 { self.loop_count -= 1; }
                }
            }
            if self.position < 0 || self.position >= max_pos { self.finished = true; return; }
        }
    }

    pub fn pretend_to_mix(&mut self, n: i32) {
        if self.volume_change_delta > 0 {
            if n >= self.volume_change_delta {
                if self.volume == VOLUME_RAMP_OUT_MARKER {
                    self.volume = 0; self.volume_left = 0; self.volume_right = 0; self.volume_mono = 0;
                    self.finished = true;
                }
                self.volume_change_delta = 0;
                self.set_mlr_vol();
            } else {
                self.volume_mono += self.vc_speed_mono * n;
                self.volume_left += self.vc_speed_left * n;
                self.volume_right += self.vc_speed_right * n;
                self.volume_change_delta -= n;
            }
        }
        let max_pos = (self.sound.samples.len() as i32) << 8;
        let loop_lo = self.loop_start << 8;
        let loop_hi = self.loop_end << 8;
        let loop_span = loop_hi - loop_lo;
        if loop_span <= 0 { self.loop_count = 0; }
        if self.position < 0 {
            if self.pitch <= 0 { self.finished = true; return; }
            self.position = 0;
        }
        if self.position >= max_pos {
            if self.pitch >= 0 { self.finished = true; return; }
            self.position = max_pos - 1;
        }
        self.position = self.position.wrapping_add(self.pitch.wrapping_mul(n));
        if self.loop_count != 0 && loop_span > 0 {
            if self.loop_reversed {
                while self.loop_count != 0
                    && ((self.pitch >= 0 && self.position >= loop_hi)
                        || (self.pitch < 0 && self.position < loop_lo))
                {
                    if self.pitch >= 0 {
                        self.position = loop_hi + loop_hi - 1 - self.position;
                    } else {
                        self.position = loop_lo + loop_lo - 1 - self.position;
                    }
                    self.pitch = -self.pitch;
                    if self.loop_count > 0 { self.loop_count -= 1; }
                }
            } else if self.pitch >= 0 && self.position >= loop_hi {
                self.position = ((self.position - loop_lo) % loop_span) + loop_lo;
                if self.loop_count > 0 { self.loop_count -= 1; }
            } else if self.pitch < 0 && self.position < loop_lo {
                let off = loop_hi - 1 - self.position;
                self.position = loop_hi - 1 - off.rem_euclid(loop_span);
                if self.loop_count > 0 { self.loop_count -= 1; }
            }
        }
        if self.position < 0 || self.position >= max_pos { self.finished = true; }
    }

    fn finalise_ramp(&mut self) {
        if self.volume == VOLUME_RAMP_OUT_MARKER {
            self.volume = 0; self.volume_left = 0; self.volume_right = 0; self.volume_mono = 0;
            self.finished = true;
        } else {
            self.set_mlr_vol();
        }
    }
}

fn get_l_vol(vol: i32, pan: i32) -> i32 {
    if pan < 0 { vol } else { (vol as f64 * ((16384 - pan) as f64 * 1.220_703_125e-4).sqrt() + 0.5) as i32 }
}
fn get_r_vol(vol: i32, pan: i32) -> i32 {
    if pan < 0 { -vol } else { (vol as f64 * (pan as f64 * 1.220_703_125e-4).sqrt() + 0.5) as i32 }
}

// Bridge WaveStream into the Mixer's voice list. Java's WaveStream IS a
// PcmStream subclass; we expose the same contract so `Mixer.add_stream`
// can hold it as `Box<dyn PcmStream>`. The trait's sample-offset
// do_mix maps 1:1 to the inherent frame-based mixer (off/len are
// frames; the inherent writer indexes `out[(frame_start+f)*2]`).
impl crate::sound::pcm_streamable::PcmStreamable for WaveStream {
    // WaveStream voices don't chain off the mixer clock; Java only reads
    // PcmStreamable.position for stream sequencing, which WaveStream
    // doesn't use.
    fn position(&self) -> i64 { 0 }
}

impl crate::sound::pcm_stream::PcmStream for WaveStream {
    fn do_mix(&mut self, buf: &mut [i32], off: i32, len: i32) -> bool {
        WaveStream::do_mix(self, buf, off as usize, len as usize);
        self.is_finished()
    }
    fn pretend_to_mix(&mut self, len: i32) {
        WaveStream::pretend_to_mix(self, len);
    }
    fn priority(&self) -> i32 {
        WaveStream::priority(self)
    }
    fn is_active(&self) -> bool {
        !self.is_finished()
    }
    fn set_secondary_volume(&mut self, vol: i32) {
        self.apply_volume_int(vol);
    }
}
