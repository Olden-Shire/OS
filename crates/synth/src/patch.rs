//! `jagex3.midi2.Patch` — an instrument: 128 notes × (wave, pitch, volume, pan, envelope).
//!
//! Loaded from the cache `patches` archive. The on-disk format is dense and column-oriented
//! (separate run-length-encoded streams for envelope choice, secondary-note, pan, volume,
//! wave id, etc.) — this decoder mirrors the Java `Patch(byte[])` constructor literally so
//! cache compatibility is exact.

use io::packet::Packet;

use crate::envelope::EnvelopeSet;

/// Per-note placeholder for a wave reference. After [`Patch::load_waves`] (TODO — needs
/// the wave loader), `wave_id` is zero and `sound` is populated. Until then `wave_id` is
/// the cache id (low bit picks JagFX vs JagVorbis: `(id & 1) == 0` → JagFX, else Vorbis;
/// id is `>> 2` to get the archive group).
#[derive(Debug, Default, Clone)]
pub struct NoteSound {
    pub wave_id: i32,
}

pub struct Patch {
    pub volume: i32,
    pub note_pitch: [i16; 128],
    pub note_volume: [i8; 128],
    pub note_pan: [u8; 128],
    pub note_secondary: [i8; 128],
    pub note_wave_id: [i32; 128],
    /// Index into `envelopes`; `usize::MAX` means "no envelope" (none assigned).
    pub note_envelope: [usize; 128],
    pub envelopes: Vec<EnvelopeSet>,
}

impl Patch {
    /// Decode a patch from the cache `patches` archive. Panics on malformed input — this
    /// matches the Java behaviour (`ArrayIndexOutOfBoundsException` / `RuntimeException`).
    #[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
    pub fn decode(src: &[u8]) -> Self {
        let mut buf = Packet::from_vec(src.to_vec());

        // ── three null-terminated length-RLE tables, each followed by an inline data
        //    chunk that the RLE indexes into (var4=secondary-note, var8=pan, var12=envelope).
        //    The data chunk after each RLE is (RLE.len() + 1) bytes long (matches Java's
        //    `pos += var3` after `var3++`). var12 has no following data chunk.
        let var4 = read_zstr_bytes(&buf);
        buf.pos += var4.len() + 1; // past the RLE bytes + null
        let var4_offset = buf.pos; // start of secondary-note data
        buf.pos += var4.len() + 1; // skip past the data

        let var8 = read_zstr_bytes(&buf);
        buf.pos += var8.len() + 1;
        let var10 = buf.pos;
        buf.pos += var8.len() + 1;

        let var12 = read_zstr_bytes(&buf);
        let var11 = var12.len() + 1;
        buf.pos += var11;

        // ── envelope-id remap table (var14) — first envelope is implicit index 1 ─────────
        let mut var14 = vec![0i8; var11];
        let var16;
        if var11 > 1 {
            var14[1] = 1;
            let mut last = 1i32;
            let mut next_new = 2i32;
            for i in 2..var11 {
                let raw = buf.g1();
                if raw == 0 {
                    last = next_new;
                    next_new += 1;
                } else {
                    let mut v = raw;
                    if v <= last {
                        v -= 1;
                    }
                    last = v;
                }
                var14[i] = last as i8;
            }
            var16 = next_new as usize;
        } else {
            var16 = var11;
        }

        // ── allocate envelope set & wire attack/release point counts ─────────────────────
        let mut envelopes: Vec<EnvelopeSet> = (0..var16).map(|_| EnvelopeSet::default()).collect();
        for env in &mut envelopes {
            let attack_points = buf.g1();
            if attack_points > 0 {
                env.attack_volume = Some(vec![0i8; (attack_points * 2) as usize]);
            }
            let release_points = buf.g1();
            if release_points > 0 {
                let mut rv = vec![0i8; (release_points * 2 + 2) as usize];
                rv[1] = 64;
                env.release_volume = Some(rv);
            }
        }

        // ── optional global volume / pan curves (var25, var27) ──────────────────────────
        let var24 = buf.g1();
        let mut var25: Option<Vec<i8>> = if var24 > 0 { Some(vec![0i8; (var24 * 2) as usize]) } else { None };
        let var26 = buf.g1();
        let mut var27: Option<Vec<i8>> = if var26 > 0 { Some(vec![0i8; (var26 * 2) as usize]) } else { None };

        // ── RLE run-counts (var29) for the secondary-pitch, pan and volume per-note loops ─
        let var29 = read_zstr_bytes(&buf);
        buf.pos += var29.len() + 1;

        // ── note pitch: low byte first as running delta, then high byte (also delta) ─────
        let mut note_pitch = [0i16; 128];
        let mut accum = 0i32;
        for p in &mut note_pitch {
            accum = accum.wrapping_add(buf.g1());
            *p = accum as i16;
        }
        let mut accum_hi = 0i32;
        for p in &mut note_pitch {
            accum_hi = accum_hi.wrapping_add(buf.g1());
            *p = (*p as i32).wrapping_add(accum_hi << 8) as i16;
        }

        // ── note wave id (RLE'd by var29) — and tucks pitch sign bit into note_pitch ─────
        let mut note_wave_id = [0i32; 128];
        {
            let mut run = 0i32;
            let mut idx = 0usize;
            let mut current_id = 0i32;
            for n in 0..128 {
                if run == 0 {
                    run = if idx < var29.len() { var29[idx] as i32 } else { -1 };
                    idx += 1;
                    current_id = buf.g_midi_var_len();
                }
                // top bit of pitch flips when wave id is "looped" (bit 1 of (id-1))
                note_pitch[n] = (note_pitch[n] as i32).wrapping_add(((current_id - 1) & 0x2) << 14) as i16;
                note_wave_id[n] = current_id;
                run -= 1;
            }
        }

        // ── secondary note (RLE'd by var4, only when wave id != 0) ──────────────────────
        let mut note_secondary = [0i8; 128];
        {
            let mut run = 0i32;
            let mut idx = 0usize;
            let mut cur = 0i32;
            let mut src_pos = var4_offset;
            for n in 0..128 {
                if note_wave_id[n] != 0 {
                    if run == 0 {
                        run = if idx < var4.len() { var4[idx] as i32 } else { -1 };
                        idx += 1;
                        cur = buf.data[src_pos] as i32 - 1;
                        src_pos += 1;
                    }
                    note_secondary[n] = cur as i8;
                    run -= 1;
                }
            }
        }

        // ── pan (RLE'd by var8, only when wave id != 0) ─────────────────────────────────
        let mut note_pan = [0u8; 128];
        {
            let mut run = 0i32;
            let mut idx = 0usize;
            let mut cur = 0i32;
            let mut src_pos = var10;
            for n in 0..128 {
                if note_wave_id[n] != 0 {
                    if run == 0 {
                        run = if idx < var8.len() { var8[idx] as i32 } else { -1 };
                        idx += 1;
                        cur = (buf.data[src_pos] as i32 + 16) << 2;
                        src_pos += 1;
                    }
                    note_pan[n] = cur as u8;
                    run -= 1;
                }
            }
        }

        // ── envelope assignment per note (RLE'd by var12 → indirected through var14) ────
        let mut note_envelope = [usize::MAX; 128];
        {
            let mut run = 0i32;
            let mut idx = 0usize;
            let mut chosen: usize = 0;
            for n in 0..128 {
                if note_wave_id[n] != 0 {
                    if run == 0 {
                        chosen = var14[idx.min(var14.len().saturating_sub(1))] as usize;
                        run = if idx < var12.len() { var12[idx] as i32 } else { -1 };
                        idx += 1;
                    }
                    note_envelope[n] = chosen;
                    run -= 1;
                }
            }
        }

        // ── note volume (RLE'd by var29 again — only reads new value when wave id > 0) ──
        let mut note_volume = [0i8; 128];
        {
            let mut run = 0i32;
            let mut idx = 0usize;
            let mut cur = 0i32;
            for n in 0..128 {
                if run == 0 {
                    run = if idx < var29.len() { var29[idx] as i32 } else { -1 };
                    idx += 1;
                    if note_wave_id[n] > 0 {
                        cur = buf.g1() + 1;
                    }
                }
                note_volume[n] = cur as i8;
                run -= 1;
            }
        }

        let volume = buf.g1() + 1;

        // ── envelope point bodies (attack value, release value) ─────────────────────────
        for env in &mut envelopes {
            if let Some(av) = env.attack_volume.as_mut() {
                let mut i = 1;
                while i < av.len() {
                    av[i] = buf.g1b();
                    i += 2;
                }
            }
            if let Some(rv) = env.release_volume.as_mut() {
                let mut i = 3;
                while i < rv.len().saturating_sub(2) {
                    rv[i] = buf.g1b();
                    i += 2;
                }
            }
        }
        if let Some(v) = var25.as_mut() {
            let mut i = 1;
            while i < v.len() {
                v[i] = buf.g1b();
                i += 2;
            }
        }
        if let Some(v) = var27.as_mut() {
            let mut i = 1;
            while i < v.len() {
                v[i] = buf.g1b();
                i += 2;
            }
        }

        // ── envelope point times (release first, then attack) ───────────────────────────
        for env in &mut envelopes {
            if let Some(rv) = env.release_volume.as_mut() {
                let mut accum = 0i32;
                let mut i = 2;
                while i < rv.len() {
                    accum = accum + 1 + buf.g1();
                    rv[i] = accum as i8;
                    i += 2;
                }
            }
        }
        for env in &mut envelopes {
            if let Some(av) = env.attack_volume.as_mut() {
                let mut accum = 0i32;
                let mut i = 2;
                while i < av.len() {
                    accum = accum + 1 + buf.g1();
                    av[i] = accum as i8;
                    i += 2;
                }
            }
        }

        // ── apply global volume curve over note_volume ──────────────────────────────────
        if let Some(v) = var25.as_mut() {
            apply_volume_curve(v, &mut note_volume, &mut buf);
        }
        // ── apply global pan curve over note_pan ────────────────────────────────────────
        if let Some(v) = var27.as_mut() {
            apply_pan_curve(v, &mut note_pan, &mut buf);
        }

        // ── envelope decay + speeds + vibrato ───────────────────────────────────────────
        for env in &mut envelopes {
            env.decay_volume = buf.g1();
        }
        for env in &mut envelopes {
            if env.attack_volume.is_some() {
                env.attack_speed = buf.g1();
            }
            if env.release_volume.is_some() {
                env.release_speed = buf.g1();
            }
            if env.decay_volume > 0 {
                env.decay_speed = buf.g1();
            }
        }
        for env in &mut envelopes {
            env.vibrato_frequency = buf.g1();
        }
        for env in &mut envelopes {
            if env.vibrato_frequency > 0 {
                env.vibrato_amplitude = buf.g1();
            }
        }
        for env in &mut envelopes {
            if env.vibrato_amplitude > 0 {
                env.vibrato_ramp_time = buf.g1();
            }
        }

        Self { volume, note_pitch, note_volume, note_pan, note_secondary, note_wave_id, note_envelope, envelopes }
    }
}

/// Read a null-terminated byte run starting at `buf.pos`. Leaves `buf.pos` unchanged
/// (caller advances past it). Bytes are returned as signed i8s because the patch decoder
/// treats them as such (run lengths can be encoded as negatives).
fn read_zstr_bytes(buf: &Packet) -> Vec<i8> {
    let mut out = Vec::new();
    let mut p = buf.pos;
    while buf.data[p] != 0 {
        out.push(buf.data[p] as i8);
        p += 1;
    }
    out
}

/// Apply the optional global volume curve onto each note's volume, mirroring the Java
/// reference's piecewise-linear interpolation (`var25` path in Patch.java).
///
/// All arithmetic is done in `i32` to match Java's byte→int promotion; the stored curve
/// bytes are signed but values are conceptually note indices 0..127.
fn apply_volume_curve(curve: &mut [i8], note_volume: &mut [i8; 128], buf: &mut Packet) {
    let first = buf.g1();
    curve[0] = first as i8;
    let mut accum = first;
    let mut i = 2;
    while i < curve.len() {
        accum = accum + 1 + buf.g1();
        curve[i] = accum as i8;
        i += 2;
    }

    let mut prev_x = curve[0] as i32;
    let mut prev_y = curve[1] as i32;
    let lo = prev_x.max(0) as usize;
    for n in 0..lo.min(128) {
        note_volume[n] = ((note_volume[n] as i32 * prev_y + 32) >> 6) as i8;
    }
    let mut k = 2;
    while k < curve.len() {
        let next_x = curve[k] as i32;
        let next_y = curve[k + 1] as i32;
        let span = next_x - prev_x;
        if span > 0 {
            let mut acc = span / 2 + span * prev_y;
            let from = prev_x.max(0) as usize;
            let to = next_x.max(0) as usize;
            for n in from..to.min(128) {
                let sign = acc >> 31;
                let scaled = (acc + sign) / span - sign;
                note_volume[n] = ((note_volume[n] as i32 * scaled + 32) >> 6) as i8;
                acc += next_y - prev_y;
            }
        }
        prev_x = next_x;
        prev_y = next_y;
        k += 2;
    }
    let hi = prev_x.max(0) as usize;
    for n in hi.min(128)..128 {
        note_volume[n] = ((note_volume[n] as i32 * prev_y + 32) >> 6) as i8;
    }
}

/// Apply the optional global pan curve, mirroring the Java reference's `var27` path.
/// Pan values clamp into `[0, 128]`.
fn apply_pan_curve(curve: &mut [i8], note_pan: &mut [u8; 128], buf: &mut Packet) {
    let first = buf.g1();
    curve[0] = first as i8;
    let mut accum = first;
    let mut i = 2;
    while i < curve.len() {
        accum = accum + 1 + buf.g1();
        curve[i] = accum as i8;
        i += 2;
    }

    let mut prev_x = curve[0] as i32;
    let mut prev_y = (curve[1] as i32) << 1;
    let lo = prev_x.max(0) as usize;
    for n in 0..lo.min(128) {
        let v = (note_pan[n] as i32) + prev_y;
        note_pan[n] = v.clamp(0, 128) as u8;
    }
    let mut k = 2;
    while k < curve.len() {
        let next_x = curve[k] as i32;
        let next_y = (curve[k + 1] as i32) << 1;
        let span = next_x - prev_x;
        if span > 0 {
            let mut acc = span / 2 + span * prev_y;
            let from = prev_x.max(0) as usize;
            let to = next_x.max(0) as usize;
            for n in from..to.min(128) {
                let sign = acc >> 31;
                let scaled = (acc + sign) / span - sign;
                let v = (note_pan[n] as i32) + scaled;
                note_pan[n] = v.clamp(0, 128) as u8;
                acc += next_y - prev_y;
            }
        }
        prev_x = next_x;
        prev_y = next_y;
        k += 2;
    }
    let hi = prev_x.max(0) as usize;
    for n in hi.min(128)..128 {
        let v = (note_pan[n] as i32) + prev_y;
        note_pan[n] = v.clamp(0, 128) as u8;
    }
}
