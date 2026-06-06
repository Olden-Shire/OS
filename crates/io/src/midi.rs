//! Jagex MIDI codec.
//!
//! OSRS rev1's cache stores songs and jingles in a column-oriented compressed format —
//! event type bytes, delta times, and per-event-type data streams (note numbers,
//! velocities, controller values, pitch bends, tempos) are kept in separate columns to
//! compress well under the subsequent gzip layer.
//!
//! [`decode`] reverses this into standard MIDI (`MThd` + `MTrk` chunks). [`encode`]
//! reverses the decoder to produce the exact original cache bytes — needed for
//! CRC-identical repack.
//!
//! ## On-disk layout
//!
//! ```text
//! [type byte stream]     // one per event; high nibble = channel XOR, low nibble = type:
//!                        //   0=note on  1=note off  2=ctrl  3=pitch
//!                        //   4=ch aftertouch  5=poly aftertouch  6=program
//!                        //   7=end-of-track (per-track terminator)
//!                        //  23=tempo (full byte; treated like type 7 = special)
//! [delta-time VLQ stream]                    // one VLQ per event
//! [controller number delta stream]           // bytes; running-sum mod 128 → ctrl number
//! [per-event-type value streams in fixed order]:
//!   var39  switch ctrls (64/65/120/121/123)
//!   var40  poly aftertouch value
//!   var41  channel aftertouch
//!   var42  pitch bend MSB
//!   var43  controller 1   (mod wheel)
//!   var44  controller 7   (volume)
//!   var45  controller 10  (pan)
//!   var46  note number    (shared: note-on, note-off, poly aftertouch)
//!   var47  note-on velocity
//!   var48  misc controllers
//!   var49  note-off velocity
//!   var50  controller 33  (bank LSB)
//!   var51  controller 39  (fine volume)
//!   var52  controller 42  (fine pan)
//!   var53  program change + ctrl 0/32 (bank MSB) shared
//!   var54  pitch bend LSB
//!   var55  controller 99  (NRPN MSB)
//!   var56  controller 98  (NRPN LSB)
//!   var57  controller 101 (RPN MSB)
//!   var58  controller 100 (RPN LSB)
//!   var59  tempo (3 bytes per tempo event)
//! [footer: num_tracks(u8) | division(u16 BE)]
//! ```
//!
//! Within each column, byte values are *delta-encoded* against a running accumulator that
//! spans the entire file (NOT reset per track). The decoder maintains separate accumulators
//! for note number, note-on velocity, note-off velocity, pitch bend, channel aftertouch,
//! poly aftertouch, controller number, and per-controller-number values (`var68[128]`).
//!
//! The channel byte is XOR-encoded: `var61 ^= type_byte >> 4` — i.e. only the *change* in
//! channel is stored, so runs of same-channel events have type bytes with high nibble 0.

use crate::Packet;

/// Decode a Jagex MIDI cache payload into standard MIDI bytes.
#[must_use]
pub fn decode(src: &[u8]) -> Vec<u8> {
    let mut p = Packet::from_vec(src.to_vec());

    // ── Footer (last 3 bytes) ─────────────────────────────────────────────
    p.pos = src.len() - 3;
    let num_tracks = p.g1() as usize;
    let division = p.g2();

    // ── Pass 1: walk type stream, count events per type ──────────────────
    p.pos = 0;
    let mut output_size = num_tracks * 10 + 14; // var4
    let mut tempo_count = 0usize;       // var5
    let mut ctrl_count = 0usize;        // var6
    let mut note_on_count = 0usize;     // var7
    let mut note_off_count = 0usize;    // var8
    let mut pitch_count = 0usize;       // var9
    let mut ch_aftertouch = 0usize;     // var10
    let mut poly_aftertouch = 0usize;   // var11
    let mut program_count = 0usize;     // var12 (later includes bank-change ctrls 0/32)

    for _ in 0..num_tracks {
        let mut prev_low: i32 = -1;
        loop {
            let full = p.g1();
            if prev_low != full {
                output_size += 1;
            }
            let low = full & 0xF;
            prev_low = low;
            if full == 7 {
                break;
            }
            if full == 23 { tempo_count += 1; }
            else if low == 0 { note_on_count += 1; }
            else if low == 1 { note_off_count += 1; }
            else if low == 2 { ctrl_count += 1; }
            else if low == 3 { pitch_count += 1; }
            else if low == 4 { ch_aftertouch += 1; }
            else if low == 5 { poly_aftertouch += 1; }
            else if low == 6 { program_count += 1; }
            else { panic!("Jagex MIDI: unknown event type byte {full}"); }
        }
    }

    let var16 = tempo_count * 5 + output_size;
    let var17 = (note_on_count + note_off_count + ctrl_count + pitch_count + poly_aftertouch) * 2 + var16;
    let var18 = ch_aftertouch + program_count + var17;

    // ── Pass 2: count delta-time bytes ───────────────────────────────────
    let delta_start = p.pos;
    let total_events = num_tracks + tempo_count + ctrl_count + note_on_count
        + note_off_count + pitch_count + ch_aftertouch + poly_aftertouch + program_count;
    for _ in 0..total_events {
        p.g_midi_var_len();
    }
    let midi_size = p.pos - delta_start + var18;

    // ── Pass 3: walk controller-number stream, count per-ctrl-type ───────
    let _ctrl_num_stream_start = p.pos;
    let mut mod_wheel = 0usize;     // var24 (controller 1)
    let mut bank_lsb = 0usize;      // var25 (controller 33)
    let mut volume = 0usize;        // var26 (controller 7)
    let mut fine_volume = 0usize;   // var27 (controller 39)
    let mut pan = 0usize;           // var28 (controller 10)
    let mut fine_pan = 0usize;      // var29 (controller 42)
    let mut nrpn_msb = 0usize;      // var30 (controller 99)
    let mut nrpn_lsb = 0usize;      // var31 (controller 98)
    let mut rpn_msb = 0usize;       // var32 (controller 101)
    let mut rpn_lsb = 0usize;       // var33 (controller 100)
    let mut switch_ctrls = 0usize;  // var34 (controllers 64/65/120/121/123)
    let mut misc_ctrls = 0usize;    // var35

    let mut running_ctrl_num = 0i32; // var36
    for _ in 0..ctrl_count {
        running_ctrl_num = (running_ctrl_num + p.g1()) & 0x7F;
        match running_ctrl_num {
            0 | 32 => program_count += 1, // bank change shares the program stream (var53)
            1 => mod_wheel += 1,
            33 => bank_lsb += 1,
            7 => volume += 1,
            39 => fine_volume += 1,
            10 => pan += 1,
            42 => fine_pan += 1,
            99 => nrpn_msb += 1,
            98 => nrpn_lsb += 1,
            101 => rpn_msb += 1,
            100 => rpn_lsb += 1,
            64 | 65 | 120 | 121 | 123 => switch_ctrls += 1,
            _ => misc_ctrls += 1,
        }
    }

    // ── Pass 4: compute stream offsets ────────────────────────────────────
    let mut off = p.pos;
    let off_switch_ctrls = off; off += switch_ctrls;
    let off_poly_value = off;   off += poly_aftertouch;
    let off_ch_aftertouch = off; off += ch_aftertouch;
    let off_pitch_msb = off;    off += pitch_count;
    let off_mod_wheel = off;    off += mod_wheel;
    let off_volume = off;       off += volume;
    let off_pan = off;          off += pan;
    let off_note_num = off;     off += note_on_count + note_off_count + poly_aftertouch;
    let off_note_on_vel = off;  off += note_on_count;
    let off_misc_ctrl = off;    off += misc_ctrls;
    let off_note_off_vel = off; off += note_off_count;
    let off_bank_lsb = off;     off += bank_lsb;
    let off_fine_volume = off;  off += fine_volume;
    let off_fine_pan = off;     off += fine_pan;
    let off_program = off;      off += program_count;
    let off_pitch_lsb = off;    off += pitch_count;
    let off_nrpn_msb = off;     off += nrpn_msb;
    let off_nrpn_lsb = off;     off += nrpn_lsb;
    let off_rpn_msb = off;      off += rpn_msb;
    let off_rpn_lsb = off;      off += rpn_lsb;
    let off_tempo = off;        // off += tempo_count * 3 (we don't need off after this)

    // ── Pass 5: emit standard MIDI ────────────────────────────────────────
    let mut out = Vec::with_capacity(midi_size);
    let mut mid = Packet::from_vec(out.split_off(0));

    // MThd chunk
    mid.p4(0x4D54_6864u32 as i32); // "MThd"
    mid.p4(6);
    mid.p2(if num_tracks > 1 { 1 } else { 0 });
    mid.p2(num_tracks as i32);
    mid.p2(division);

    // Delta-time cursor (sequential reads via gMidiVarLen)
    p.pos = delta_start;

    // Per-stream cursors — re-read into the source data by direct indexing.
    let mut c_type = 0usize;              // var38 — type stream re-cursor (from byte 0)
    let mut c_switch = off_switch_ctrls;  // var39
    let mut c_poly_val = off_poly_value;  // var40
    let mut c_ch_aft = off_ch_aftertouch; // var41
    let mut c_pitch_msb = off_pitch_msb;  // var42
    let mut c_mod = off_mod_wheel;        // var43
    let mut c_vol = off_volume;           // var44
    let mut c_pan = off_pan;              // var45
    let mut c_note_num = off_note_num;    // var46
    let mut c_note_on_v = off_note_on_vel;// var47
    let mut c_misc = off_misc_ctrl;       // var48
    let mut c_note_off_v = off_note_off_vel; // var49
    let mut c_bank_lsb = off_bank_lsb;    // var50
    let mut c_fine_vol = off_fine_volume; // var51
    let mut c_fine_pan = off_fine_pan;    // var52
    let mut c_program = off_program;      // var53
    let mut c_pitch_lsb = off_pitch_lsb;  // var54
    let mut c_nrpn_msb = off_nrpn_msb;    // var55
    let mut c_nrpn_lsb = off_nrpn_lsb;    // var56
    let mut c_rpn_msb = off_rpn_msb;      // var57
    let mut c_rpn_lsb = off_rpn_lsb;      // var58
    let mut c_tempo = off_tempo;          // var59
    // ctrl-number stream cursor — sits between delta-time stream end and value streams.
    let mut c_ctrl_num_re = off_switch_ctrls - ctrl_count;

    // Running-state accumulators — span the whole file (NOT reset per track).
    let mut channel = 0i32;       // var61
    let mut note_num = 0i32;      // var62
    let mut note_on_vel = 0i32;   // var63
    let mut note_off_vel = 0i32;  // var64
    let mut pitch_bend = 0i32;    // var65
    let mut ch_aft = 0i32;        // var66
    let mut poly_aft_val = 0i32;  // var67
    let mut ctrl_val = [0i32; 128]; // var68
    let mut ctrl_num = 0i32;      // var69

    let src_data = p.data.clone(); // own a copy so we can both index it and use the cursor

    'tracks: for _ in 0..num_tracks {
        // "MTrk" + reserve length
        mid.p4(0x4D54_726Bu32 as i32);
        mid.pos += 4;
        let track_payload_start = mid.pos;
        let mut prev_status: i32 = -1;

        loop {
            let delta = p.g_midi_var_len();
            mid.p_midi_var_len(delta);

            let type_byte = i32::from(src_data[c_type]);
            c_type += 1;
            let status_changed = prev_status != type_byte;
            prev_status = type_byte & 0xF;

            if type_byte == 7 {
                if status_changed {
                    mid.p1(0xFF);
                }
                mid.p1(0x2F);
                mid.p1(0x00);
                mid.psize4((mid.pos - track_payload_start) as i32);
                continue 'tracks;
            }

            if type_byte == 23 {
                if status_changed {
                    mid.p1(0xFF);
                }
                mid.p1(0x51); // tempo meta
                mid.p1(0x03);
                mid.p1(i32::from(src_data[c_tempo])); c_tempo += 1;
                mid.p1(i32::from(src_data[c_tempo])); c_tempo += 1;
                mid.p1(i32::from(src_data[c_tempo])); c_tempo += 1;
            } else {
                channel ^= type_byte >> 4;
                let event_type = type_byte & 0xF;
                match event_type {
                    0 => {
                        // note on
                        if status_changed { mid.p1(channel + 0x90); }
                        note_num = note_num.wrapping_add(signed(src_data[c_note_num]));
                        c_note_num += 1;
                        note_on_vel = note_on_vel.wrapping_add(signed(src_data[c_note_on_v]));
                        c_note_on_v += 1;
                        mid.p1(note_num & 0x7F);
                        mid.p1(note_on_vel & 0x7F);
                    }
                    1 => {
                        // note off
                        if status_changed { mid.p1(channel + 0x80); }
                        note_num = note_num.wrapping_add(signed(src_data[c_note_num]));
                        c_note_num += 1;
                        note_off_vel = note_off_vel.wrapping_add(signed(src_data[c_note_off_v]));
                        c_note_off_v += 1;
                        mid.p1(note_num & 0x7F);
                        mid.p1(note_off_vel & 0x7F);
                    }
                    2 => {
                        // controller change
                        if status_changed { mid.p1(channel + 0xB0); }
                        ctrl_num = (ctrl_num + i32::from(src_data[c_ctrl_num_re])) & 0x7F;
                        c_ctrl_num_re += 1;
                        mid.p1(ctrl_num);
                        let value_byte = match ctrl_num {
                            0 | 32 => { let b = src_data[c_program]; c_program += 1; b }
                            1 => { let b = src_data[c_mod]; c_mod += 1; b }
                            33 => { let b = src_data[c_bank_lsb]; c_bank_lsb += 1; b }
                            7 => { let b = src_data[c_vol]; c_vol += 1; b }
                            39 => { let b = src_data[c_fine_vol]; c_fine_vol += 1; b }
                            10 => { let b = src_data[c_pan]; c_pan += 1; b }
                            42 => { let b = src_data[c_fine_pan]; c_fine_pan += 1; b }
                            99 => { let b = src_data[c_nrpn_msb]; c_nrpn_msb += 1; b }
                            98 => { let b = src_data[c_nrpn_lsb]; c_nrpn_lsb += 1; b }
                            101 => { let b = src_data[c_rpn_msb]; c_rpn_msb += 1; b }
                            100 => { let b = src_data[c_rpn_lsb]; c_rpn_lsb += 1; b }
                            64 | 65 | 120 | 121 | 123 => { let b = src_data[c_switch]; c_switch += 1; b }
                            _ => { let b = src_data[c_misc]; c_misc += 1; b }
                        };
                        let v = ctrl_val[ctrl_num as usize].wrapping_add(signed(value_byte));
                        ctrl_val[ctrl_num as usize] = v;
                        mid.p1(v & 0x7F);
                    }
                    3 => {
                        // pitch bend
                        if status_changed { mid.p1(channel + 0xE0); }
                        let inc_lsb = pitch_bend.wrapping_add(signed(src_data[c_pitch_lsb]));
                        c_pitch_lsb += 1;
                        pitch_bend = inc_lsb.wrapping_add(signed(src_data[c_pitch_msb]) << 7);
                        c_pitch_msb += 1;
                        mid.p1(pitch_bend & 0x7F);
                        mid.p1((pitch_bend >> 7) & 0x7F);
                    }
                    4 => {
                        // channel aftertouch
                        if status_changed { mid.p1(channel + 0xD0); }
                        ch_aft = ch_aft.wrapping_add(signed(src_data[c_ch_aft]));
                        c_ch_aft += 1;
                        mid.p1(ch_aft & 0x7F);
                    }
                    5 => {
                        // poly aftertouch
                        if status_changed { mid.p1(channel + 0xA0); }
                        note_num = note_num.wrapping_add(signed(src_data[c_note_num]));
                        c_note_num += 1;
                        poly_aft_val = poly_aft_val.wrapping_add(signed(src_data[c_poly_val]));
                        c_poly_val += 1;
                        mid.p1(note_num & 0x7F);
                        mid.p1(poly_aft_val & 0x7F);
                    }
                    6 => {
                        // program change — value comes from the program stream
                        if status_changed { mid.p1(channel + 0xC0); }
                        mid.p1(i32::from(src_data[c_program]));
                        c_program += 1;
                    }
                    _ => panic!("Jagex MIDI: bad event type {event_type} during decode"),
                }
            }
        }
    }

    debug_assert_eq!(mid.data.len(), midi_size, "MIDI output size mismatch");
    mid.data
}

#[inline]
fn signed(b: u8) -> i32 {
    i32::from(b as i8)
}

/// Encode one delta-stream byte. Jagex picks the unsigned 7-bit representative
/// `(current - prev) & 0x7F` (always 0..127). The running accumulator advances by
/// adding that byte interpreted as `i8` — so subsequent emits stay aligned with the
/// decoder's identical update rule.
#[inline]
fn encode_delta(current: i32, prev: &mut i32) -> u8 {
    let byte = (current.wrapping_sub(*prev) & 0x7F) as u8;
    *prev = prev.wrapping_add(i32::from(byte as i8));
    byte
}

/// Encode standard MIDI bytes back to the Jagex column format. Inverse of [`decode`].
///
/// Designed to be bit-exact reverse: feeding `encode(decode(jagex_bytes))` back must
/// produce the original `jagex_bytes` for CRC-identical repack.
#[must_use]
pub fn encode(src: &[u8]) -> Vec<u8> {
    let mut r = StdMidiReader::new(src);
    r.expect_tag(b"MThd");
    let header_len = r.read_be32();
    assert_eq!(header_len, 6, "Jagex MIDI encode: MThd len != 6");
    let _format = r.read_be16();
    let num_tracks = r.read_be16() as usize;
    let division = r.read_be16();

    // Per-column buffers — mirror the decoder's stream offsets.
    let mut types = Vec::<u8>::new();
    let mut deltas = Vec::<u8>::new();          // VLQs, raw bytes
    let mut ctrl_nums = Vec::<u8>::new();        // deltas, raw bytes
    let mut s_switch = Vec::<u8>::new();         // var39
    let mut s_poly_val = Vec::<u8>::new();       // var40
    let mut s_ch_aft = Vec::<u8>::new();         // var41
    let mut s_pitch_msb = Vec::<u8>::new();      // var42
    let mut s_mod = Vec::<u8>::new();            // var43
    let mut s_vol = Vec::<u8>::new();            // var44
    let mut s_pan = Vec::<u8>::new();            // var45
    let mut s_note_num = Vec::<u8>::new();       // var46 (note-on + note-off + poly)
    let mut s_note_on_v = Vec::<u8>::new();      // var47
    let mut s_misc = Vec::<u8>::new();           // var48
    let mut s_note_off_v = Vec::<u8>::new();     // var49
    let mut s_bank_lsb = Vec::<u8>::new();       // var50
    let mut s_fine_vol = Vec::<u8>::new();       // var51
    let mut s_fine_pan = Vec::<u8>::new();       // var52
    let mut s_program = Vec::<u8>::new();        // var53 (program-change + bank-change ctrl 0/32)
    let mut s_pitch_lsb = Vec::<u8>::new();      // var54
    let mut s_nrpn_msb = Vec::<u8>::new();       // var55
    let mut s_nrpn_lsb = Vec::<u8>::new();       // var56
    let mut s_rpn_msb = Vec::<u8>::new();        // var57
    let mut s_rpn_lsb = Vec::<u8>::new();        // var58
    let mut s_tempo = Vec::<u8>::new();          // var59

    // Running state — span the whole file (matches decoder; NOT reset per track).
    let mut channel = 0i32;
    let mut note_num = 0i32;
    let mut note_on_vel = 0i32;
    let mut note_off_vel = 0i32;
    let mut pitch_bend = 0i32;
    let mut ch_aft = 0i32;
    let mut poly_aft_val = 0i32;
    let mut ctrl_val = [0i32; 128];
    let mut ctrl_num_running = 0i32;

    for _ in 0..num_tracks {
        r.expect_tag(b"MTrk");
        let track_len = r.read_be32() as usize;
        let track_end = r.pos + track_len;

        // Running status — standard MIDI; the decoder emits status only when the type
        // byte changes (high-nibble != 0 OR low-nibble different from prev).
        let mut status: u8 = 0;

        while r.pos < track_end {
            // Delta time
            let (delta_bytes, _delta_val) = r.read_vlq_with_bytes();
            deltas.extend_from_slice(&delta_bytes);

            let first = r.peek();
            if first & 0x80 != 0 {
                status = first;
                r.pos += 1;
            }

            if status == 0xFF {
                // Meta — we only handle tempo (0x51) and end-of-track (0x2F).
                let meta_type = r.read_u8();
                let meta_len = r.read_vlq();
                match meta_type {
                    0x2F => {
                        assert_eq!(meta_len, 0, "Jagex MIDI encode: bad end-of-track");
                        let xor = channel; // unused for end-of-track but compute as 0 below
                        let _ = xor;
                        let high_nibble = 0u8; // end-of-track doesn't change channel
                        types.push((high_nibble << 4) | 7);
                        // end-of-track doesn't consume any value stream
                    }
                    0x51 => {
                        assert_eq!(meta_len, 3, "Jagex MIDI encode: bad tempo length");
                        let b0 = r.read_u8();
                        let b1 = r.read_u8();
                        let b2 = r.read_u8();
                        types.push(23);
                        s_tempo.extend_from_slice(&[b0, b1, b2]);
                    }
                    other => panic!("Jagex MIDI encode: unsupported meta event 0x{other:02X}"),
                }
                // After a meta, status resets so next event has a status byte. Match by
                // requiring it: we leave `status = 0xFF` so the next event will be read
                // as data; but standard MIDI requires a status byte after meta. The
                // decoder's output always re-emits status after meta, so we'll see one.
            } else {
                let event_type = status & 0xF0;
                let event_channel = (status & 0x0F) as i32;
                let high_nibble = (channel ^ event_channel) as u8 & 0x0F;
                let low_nibble: u8 = match event_type {
                    0x90 => 0,  // note on
                    0x80 => 1,  // note off
                    0xB0 => 2,  // controller
                    0xE0 => 3,  // pitch bend
                    0xD0 => 4,  // channel aftertouch
                    0xA0 => 5,  // poly aftertouch
                    0xC0 => 6,  // program change
                    other => panic!("Jagex MIDI encode: unsupported status 0x{other:02X}"),
                };
                types.push((high_nibble << 4) | low_nibble);
                channel ^= high_nibble as i32;

                match event_type {
                    0x90 => {
                        let n = r.read_u8() as i32;
                        let v = r.read_u8() as i32;
                        s_note_num.push(encode_delta(n, &mut note_num));
                        s_note_on_v.push(encode_delta(v, &mut note_on_vel));
                    }
                    0x80 => {
                        let n = r.read_u8() as i32;
                        let v = r.read_u8() as i32;
                        s_note_num.push(encode_delta(n, &mut note_num));
                        s_note_off_v.push(encode_delta(v, &mut note_off_vel));
                    }
                    0xB0 => {
                        let cn = r.read_u8() as i32;
                        let vv = r.read_u8() as i32;
                        ctrl_nums.push(encode_delta(cn, &mut ctrl_num_running));
                        let vb = encode_delta(vv, &mut ctrl_val[cn as usize]);
                        let dst: &mut Vec<u8> = match cn {
                            0 | 32 => &mut s_program,
                            1 => &mut s_mod,
                            33 => &mut s_bank_lsb,
                            7 => &mut s_vol,
                            39 => &mut s_fine_vol,
                            10 => &mut s_pan,
                            42 => &mut s_fine_pan,
                            99 => &mut s_nrpn_msb,
                            98 => &mut s_nrpn_lsb,
                            101 => &mut s_rpn_msb,
                            100 => &mut s_rpn_lsb,
                            64 | 65 | 120 | 121 | 123 => &mut s_switch,
                            _ => &mut s_misc,
                        };
                        dst.push(vb);
                    }
                    0xE0 => {
                        let lsb_emit = r.read_u8() as i32;
                        let msb_emit = r.read_u8() as i32;
                        let v = lsb_emit | (msb_emit << 7); // 14-bit
                        // Take delta mod 16384 so both LSB and MSB bytes land in 0..127
                        // (Jagex's convention — same "always-positive" pattern as the
                        // other delta streams). The accumulator wraps mod 16384 at emit
                        // time via `pitch_bend & 0x7F` + `(pitch_bend >> 7) & 0x7F`.
                        let raw_delta = v.wrapping_sub(pitch_bend);
                        let effective = raw_delta.rem_euclid(16384);
                        let lsb_byte = (effective & 0x7F) as u8;
                        let msb_byte = ((effective >> 7) & 0x7F) as u8;
                        s_pitch_lsb.push(lsb_byte);
                        s_pitch_msb.push(msb_byte);
                        pitch_bend = pitch_bend
                            + i32::from(lsb_byte as i8)
                            + (i32::from(msb_byte as i8) << 7);
                    }
                    0xD0 => {
                        let v = r.read_u8() as i32;
                        s_ch_aft.push(encode_delta(v, &mut ch_aft));
                    }
                    0xA0 => {
                        let n = r.read_u8() as i32;
                        let vv = r.read_u8() as i32;
                        s_note_num.push(encode_delta(n, &mut note_num));
                        s_poly_val.push(encode_delta(vv, &mut poly_aft_val));
                    }
                    0xC0 => {
                        let prog = r.read_u8();
                        s_program.push(prog);
                    }
                    _ => unreachable!(),
                }
            }
        }
    }

    // ── Assemble final byte buffer ──────────────────────────────────────
    let mut out = Vec::with_capacity(
        types.len() + deltas.len() + ctrl_nums.len()
            + s_switch.len() + s_poly_val.len() + s_ch_aft.len() + s_pitch_msb.len()
            + s_mod.len() + s_vol.len() + s_pan.len() + s_note_num.len()
            + s_note_on_v.len() + s_misc.len() + s_note_off_v.len() + s_bank_lsb.len()
            + s_fine_vol.len() + s_fine_pan.len() + s_program.len() + s_pitch_lsb.len()
            + s_nrpn_msb.len() + s_nrpn_lsb.len() + s_rpn_msb.len() + s_rpn_lsb.len()
            + s_tempo.len() + 3,
    );
    out.extend_from_slice(&types);
    out.extend_from_slice(&deltas);
    out.extend_from_slice(&ctrl_nums);
    out.extend_from_slice(&s_switch);
    out.extend_from_slice(&s_poly_val);
    out.extend_from_slice(&s_ch_aft);
    out.extend_from_slice(&s_pitch_msb);
    out.extend_from_slice(&s_mod);
    out.extend_from_slice(&s_vol);
    out.extend_from_slice(&s_pan);
    out.extend_from_slice(&s_note_num);
    out.extend_from_slice(&s_note_on_v);
    out.extend_from_slice(&s_misc);
    out.extend_from_slice(&s_note_off_v);
    out.extend_from_slice(&s_bank_lsb);
    out.extend_from_slice(&s_fine_vol);
    out.extend_from_slice(&s_fine_pan);
    out.extend_from_slice(&s_program);
    out.extend_from_slice(&s_pitch_lsb);
    out.extend_from_slice(&s_nrpn_msb);
    out.extend_from_slice(&s_nrpn_lsb);
    out.extend_from_slice(&s_rpn_msb);
    out.extend_from_slice(&s_rpn_lsb);
    out.extend_from_slice(&s_tempo);

    // Footer: num_tracks (u8) | division (u16 BE)
    out.push(num_tracks as u8);
    out.push((division >> 8) as u8);
    out.push(division as u8);

    out
}

/// Minimal standard-MIDI byte reader for the encoder. No SysEx / channel-mode-message
/// support — we only need to round-trip what `decode` produced.
struct StdMidiReader<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> StdMidiReader<'a> {
    fn new(src: &'a [u8]) -> Self {
        Self { src, pos: 0 }
    }
    fn expect_tag(&mut self, tag: &[u8]) {
        let end = self.pos + tag.len();
        assert_eq!(&self.src[self.pos..end], tag, "Jagex MIDI encode: missing tag");
        self.pos = end;
    }
    fn read_be32(&mut self) -> u32 {
        let v = u32::from_be_bytes(self.src[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        v
    }
    fn read_be16(&mut self) -> i32 {
        let v = u16::from_be_bytes(self.src[self.pos..self.pos + 2].try_into().unwrap()) as i32;
        self.pos += 2;
        v
    }
    fn read_u8(&mut self) -> u8 {
        let v = self.src[self.pos];
        self.pos += 1;
        v
    }
    fn peek(&self) -> u8 {
        self.src[self.pos]
    }
    fn read_vlq(&mut self) -> u32 {
        let mut v = 0u32;
        loop {
            let b = self.src[self.pos];
            self.pos += 1;
            v = (v << 7) | u32::from(b & 0x7F);
            if b & 0x80 == 0 {
                return v;
            }
        }
    }
    fn read_vlq_with_bytes(&mut self) -> (Vec<u8>, u32) {
        let start = self.pos;
        let v = self.read_vlq();
        (self.src[start..self.pos].to_vec(), v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_header_matches_mthd() {
        // Tiny synthetic: 1 track, division=480, one note-on, one note-off, end.
        // Build the Jagex bytes by hand.
        let mut buf: Vec<u8> = Vec::new();
        // type stream: note-on, note-off, end-of-track
        buf.extend_from_slice(&[0x00, 0x01, 0x07]);
        // delta times: 0, 100, 0 — VLQ encoded
        buf.push(0x00);
        buf.push(0x64); // 100
        buf.push(0x00);
        // controller number stream: empty (no ctrl events)
        // value streams: note-num + note-on velocity + note-num + note-off velocity
        // Layout:
        //   switch=0  poly_val=0  ch_aft=0  pitch_msb=0  mod=0  vol=0  pan=0
        //   note_num=2 bytes (one note, used twice — note 60 each time, delta 60 then 0)
        //   note_on_vel=1 byte (100)
        //   misc=0  note_off_vel=1 byte (0)
        //   bank_lsb=0 fine_vol=0 fine_pan=0  program=0  pitch_lsb=0
        //   nrpn=0  rpn=0  tempo=0
        buf.push(60);  // first note number delta = 60 (running: 60)
        buf.push(0);   // second (note-off) note delta = 0 (running stays 60)
        buf.push(100); // note-on velocity delta = 100
        buf.push(0);   // note-off velocity delta = 0
        // Footer: num_tracks=1, division=480
        buf.push(1);
        let div: u16 = 480;
        buf.push((div >> 8) as u8);
        buf.push(div as u8);

        let out = decode(&buf);
        // Should start with MThd
        assert_eq!(&out[..4], b"MThd");
        // Header chunk length = 6
        assert_eq!(&out[4..8], &[0, 0, 0, 6]);
        // Format = 0 (single track), tracks = 1, division = 480
        assert_eq!(&out[8..10], &[0, 0]);
        assert_eq!(&out[10..12], &[0, 1]);
        assert_eq!(&out[12..14], &[0x01, 0xE0]);
        // Then MTrk
        assert_eq!(&out[14..18], b"MTrk");
    }
}
