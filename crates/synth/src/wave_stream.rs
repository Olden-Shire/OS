//! `jagex3.sound.WaveStream` — a single playing voice.
//!
//! Java's WaveStream has 16 hand-unrolled mixer variants (forward/backward, 1:1 vs
//! resampled, mono vs stereo, ramping vs steady). The dispatch logic to pick among them
//! is `do_mix`. We implement the same semantics — stereo output with linear interpolation
//! and per-sample volume/pan ramping — in a single unified mixer, matching Java's `>> 6`
//! mixing scale, signed-byte sample format, and loop/reverse behaviour.

use std::sync::Arc;

use crate::wave::Wave;

/// Output is always stereo i32 (Jagex's mixer accumulator format). Caller clips down to
/// i16 at PcmPlayer write time.
pub struct WaveStream {
    pub sound: Arc<Wave>,
    /// position in samples × 256 (high 24 bits = sample index, low 8 = sub-sample).
    pub position: i32,
    /// pitch in samples × 256 per output sample. Negative = reverse playback.
    pub pitch: i32,
    pub volume: i32,
    pub pan: i32,
    pub volume_mono: i32,
    pub volume_left: i32,
    pub volume_right: i32,
    pub loop_count: i32,
    pub loop_start: i32,
    pub loop_end: i32,
    pub loop_reversed: bool,
    pub volume_change_delta: i32,
    pub vc_speed_mono: i32,
    pub vc_speed_left: i32,
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
            sound,
            position: 0,
            pitch,
            volume,
            pan,
            volume_mono: 0,
            volume_left: 0,
            volume_right: 0,
            loop_count: 0,
            loop_start,
            loop_end,
            loop_reversed,
            volume_change_delta: 0,
            vc_speed_mono: 0,
            vc_speed_left: 0,
            vc_speed_right: 0,
            active: true,
            finished: false,
        };
        me.set_mlr_vol();
        me
    }

    pub fn set_mlr_vol(&mut self) {
        self.volume_mono = self.volume;
        self.volume_left = get_l_vol(self.volume, self.pan);
        self.volume_right = get_r_vol(self.volume, self.pan);
    }

    pub fn set_loop_count(&mut self, n: i32) {
        self.loop_count = n;
    }

    pub fn set_position(&mut self, p: i32) {
        let max = (self.sound.samples.len() as i32) << 8;
        self.position = p.clamp(-1, max);
    }

    pub fn set_reverse(&mut self, r: bool) {
        let mag = self.pitch.unsigned_abs() as i32;
        self.pitch = if r { -mag } else { mag };
    }

    pub fn set_rate_raw(&mut self, rate: i32) {
        if self.pitch < 0 {
            self.pitch = -rate;
        } else {
            self.pitch = rate;
        }
    }

    pub fn get_rate_raw(&self) -> i32 {
        self.pitch.unsigned_abs() as i32
    }

    pub fn get_volume_fine(&self) -> i32 {
        if self.volume == VOLUME_RAMP_OUT_MARKER { 0 } else { self.volume }
    }

    pub fn get_pan_fine(&self) -> i32 {
        if self.pan < 0 { -1 } else { self.pan }
    }

    pub fn set_vol_pan_fine(&mut self, vol: i32, pan: i32) {
        self.volume = vol;
        self.pan = pan;
        self.volume_change_delta = 0;
        self.set_mlr_vol();
    }

    pub fn ramp_vol_pan_fine(&mut self, ramp_len: i32, vol: i32, pan: i32) {
        if ramp_len == 0 {
            self.set_vol_pan_fine(vol, pan);
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
        self.volume = vol;
        self.pan = pan;
        self.vc_speed_mono = (vol - self.volume_mono) / used;
        self.vc_speed_left = (new_l - self.volume_left) / used;
        self.vc_speed_right = (new_r - self.volume_right) / used;
    }

    pub fn ramp_out(&mut self, ramp_len: i32) {
        if ramp_len == 0 {
            self.set_vol_pan_fine(0, self.get_pan_fine());
            self.finished = true;
            return;
        }
        if self.volume_left == 0 && self.volume_right == 0 {
            self.volume_change_delta = 0;
            self.volume = 0;
            self.volume_mono = 0;
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

    pub fn is_ramping(&self) -> bool {
        self.volume_change_delta != 0
    }

    pub fn is_finished(&self) -> bool {
        self.finished
            || self.position < 0
            || self.position >= (self.sound.samples.len() as i32) << 8
    }

    /// Mix `n` stereo frames into `out` (interleaved L,R), starting at frame `frame_start`.
    /// `out.len()` must be ≥ 2 * (frame_start + n). Java's WaveStream.doMix dispatches into
    /// many variants; this unified implementation handles forward + backward + loops + ramp.
    pub fn do_mix(&mut self, out: &mut [i32], frame_start: usize, n: usize) {
        if self.finished {
            return;
        }
        if self.volume == 0 && self.volume_change_delta == 0 {
            self.pretend_to_mix(n as i32);
            return;
        }
        let total_samples = self.sound.samples.len() as i32;
        if total_samples == 0 {
            self.finished = true;
            return;
        }
        let max_pos = total_samples << 8;
        let loop_lo = self.loop_start << 8;
        let loop_hi = self.loop_end << 8;
        let loop_span = loop_hi - loop_lo;
        if loop_span <= 0 {
            self.loop_count = 0;
        }
        if self.position < 0 {
            if self.pitch <= 0 {
                self.finished = true;
                return;
            }
            self.position = 0;
        }
        if self.position >= max_pos {
            if self.pitch >= 0 {
                self.finished = true;
                return;
            }
            self.position = max_pos - 1;
        }
        let samples = Arc::clone(&self.sound);
        let samples = &samples.samples[..];
        let mut frames_done = 0usize;
        while frames_done < n {
            let mut chunk = n - frames_done;
            // Stop early at the loop boundary so the inner per-sample loop never sees an
            // out-of-bounds index. Apply for ANY non-zero loop count (finite OR infinite).
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
                if idx >= samples.len() {
                    self.finished = true;
                    break;
                }
                let s0 = samples[idx] as i32;
                // For the last sample (idx + 1 out of bounds) we hold s0 rather than
                // interpolating to garbage. Java passes an explicit "edge value" here;
                // for looped waves the outer wrap then sends position back to loop_lo so
                // the next chunk picks up cleanly.
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
                            // emit this last sample and bail
                            let pos = (frame_start + frames_done + f) * 2;
                            out[pos] = out[pos].wrapping_add(l);
                            out[pos + 1] = out[pos + 1].wrapping_add(r);
                            self.position = self.position.wrapping_add(self.pitch);
                            frames_done += f + 1;
                            return;
                        }
                    }
                } else {
                    l = self.volume_left * interp >> 6;
                    r = self.volume_right * interp >> 6;
                }
                let pos = (frame_start + frames_done + f) * 2;
                out[pos] = out[pos].wrapping_add(l);
                out[pos + 1] = out[pos + 1].wrapping_add(r);
                self.position = self.position.wrapping_add(self.pitch);
            }
            frames_done += chunk;
            // Handle loop wrap / end of sample
            if self.loop_count != 0 && loop_span > 0 {
                if self.pitch >= 0 && self.position >= loop_hi {
                    if self.loop_reversed {
                        self.position = loop_hi + loop_hi - 1 - self.position;
                        self.pitch = -self.pitch;
                    } else {
                        self.position = ((self.position - loop_lo) % loop_span) + loop_lo;
                    }
                    if self.loop_count > 0 {
                        self.loop_count -= 1;
                    }
                } else if self.pitch < 0 && self.position < loop_lo {
                    if self.loop_reversed {
                        self.position = loop_lo + loop_lo - 1 - self.position;
                        self.pitch = -self.pitch;
                    } else {
                        self.position = loop_hi - 1 - ((loop_hi - 1 - self.position) % loop_span);
                    }
                    if self.loop_count > 0 {
                        self.loop_count -= 1;
                    }
                }
            }
            if self.position < 0 || self.position >= max_pos {
                self.finished = true;
                return;
            }
        }
    }

    pub fn pretend_to_mix(&mut self, n: i32) {
        if self.volume_change_delta > 0 {
            if n >= self.volume_change_delta {
                if self.volume == VOLUME_RAMP_OUT_MARKER {
                    self.volume = 0;
                    self.volume_left = 0;
                    self.volume_right = 0;
                    self.volume_mono = 0;
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
        if loop_span <= 0 {
            self.loop_count = 0;
        }
        if self.position < 0 {
            if self.pitch <= 0 {
                self.finished = true;
                return;
            }
            self.position = 0;
        }
        if self.position >= max_pos {
            if self.pitch >= 0 {
                self.finished = true;
                return;
            }
            self.position = max_pos - 1;
        }
        self.position = self.position.wrapping_add(self.pitch.wrapping_mul(n));
        // Loop wrap: matches Java's pretendToMix loop handling. For loop_count != 0 we
        // wrap silently and keep playing; for loop_count == 0 (non-looped) we let the
        // position run off and finish the note.
        if self.loop_count != 0 && loop_span > 0 {
            if self.loop_reversed {
                // Bounce off loop boundaries — for the reverse-loop wave types.
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
                    if self.loop_count > 0 {
                        self.loop_count -= 1;
                    }
                }
            } else if self.pitch >= 0 && self.position >= loop_hi {
                self.position = ((self.position - loop_lo) % loop_span) + loop_lo;
                if self.loop_count > 0 {
                    self.loop_count -= 1;
                }
            } else if self.pitch < 0 && self.position < loop_lo {
                let off = loop_hi - 1 - self.position;
                self.position = loop_hi - 1 - off.rem_euclid(loop_span);
                if self.loop_count > 0 {
                    self.loop_count -= 1;
                }
            }
        }
        if self.position < 0 || self.position >= max_pos {
            self.finished = true;
        }
    }

    fn finalise_ramp(&mut self) {
        if self.volume == VOLUME_RAMP_OUT_MARKER {
            self.volume = 0;
            self.volume_left = 0;
            self.volume_right = 0;
            self.volume_mono = 0;
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
