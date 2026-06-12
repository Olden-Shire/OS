// @ObfuscatedName("ei.MidiFile(Lev;)V") — jag::oldscape::midi2::MidiFile ctor.
//
// Decodes the column-oriented Jagex MIDI cache payload (type stream +
// delta-time VLQ stream + per-event-type value columns + footer) into
// standard MIDI bytes (MThd + MTrk chunks). This is what Java's MidiFile
// constructor (jagex3.midi2.MidiFile.MidiFile(Packet)) does — verbatim
// port. Without it our songs-archive bytes are unparseable by MidiParser.

#![allow(dead_code)]

use crate::io::packet::Packet;

pub fn decode(src: &[u8]) -> Vec<u8> {
    let mut p = Packet::from_vec(src.to_vec());

    // Footer (last 3 bytes): num_tracks (u8) + division (u16 BE).
    p.pos = (src.len() - 3) as i32;
    let num_tracks = p.g1() as usize;
    let division = p.g2();

    // Pass 1: walk type stream, count events per type.
    p.pos = 0;
    let mut output_size = num_tracks * 10 + 14;
    let mut tempo_count = 0usize;
    let mut ctrl_count = 0usize;
    let mut note_on_count = 0usize;
    let mut note_off_count = 0usize;
    let mut pitch_count = 0usize;
    let mut ch_aftertouch = 0usize;
    let mut poly_aftertouch = 0usize;
    let mut program_count = 0usize;

    for _ in 0..num_tracks {
        let mut prev_low: i32 = -1;
        loop {
            let full = p.g1();
            if prev_low != full {
                output_size += 1;
            }
            let low = full & 0xF;
            prev_low = low;
            if full == 7 { break; }
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

    // Pass 2: count delta-time bytes.
    let delta_start = p.pos as usize;
    let total_events = num_tracks + tempo_count + ctrl_count + note_on_count
        + note_off_count + pitch_count + ch_aftertouch + poly_aftertouch + program_count;
    for _ in 0..total_events {
        p.gMidiVarLen();
    }
    let midi_size = p.pos as usize - delta_start + var18;

    // Pass 3: walk controller-number stream, count per-ctrl-type.
    let mut mod_wheel = 0usize;
    let mut bank_lsb = 0usize;
    let mut volume = 0usize;
    let mut fine_volume = 0usize;
    let mut pan = 0usize;
    let mut fine_pan = 0usize;
    let mut nrpn_msb = 0usize;
    let mut nrpn_lsb = 0usize;
    let mut rpn_msb = 0usize;
    let mut rpn_lsb = 0usize;
    let mut switch_ctrls = 0usize;
    let mut misc_ctrls = 0usize;

    let mut running_ctrl_num = 0i32;
    for _ in 0..ctrl_count {
        running_ctrl_num = (running_ctrl_num + p.g1()) & 0x7F;
        match running_ctrl_num {
            0 | 32 => program_count += 1,
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

    // Pass 4: compute stream offsets.
    let mut off = p.pos as usize;
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
    let off_tempo = off;

    // Pass 5: emit standard MIDI. The client's Packet writes by indexing
    // (no ensure-grow), so we pre-allocate to the size computed above.
    let mut mid = Packet::from_vec(vec![0u8; midi_size]);

    mid.p4(0x4D54_6864u32 as i32);
    mid.p4(6);
    mid.p2(if num_tracks > 1 { 1 } else { 0 });
    mid.p2(num_tracks as i32);
    mid.p2(division);

    p.pos = delta_start as i32;

    let mut c_type = 0usize;
    let mut c_switch = off_switch_ctrls;
    let mut c_poly_val = off_poly_value;
    let mut c_ch_aft = off_ch_aftertouch;
    let mut c_pitch_msb = off_pitch_msb;
    let mut c_mod = off_mod_wheel;
    let mut c_vol = off_volume;
    let mut c_pan = off_pan;
    let mut c_note_num = off_note_num;
    let mut c_note_on_v = off_note_on_vel;
    let mut c_misc = off_misc_ctrl;
    let mut c_note_off_v = off_note_off_vel;
    let mut c_bank_lsb = off_bank_lsb;
    let mut c_fine_vol = off_fine_volume;
    let mut c_fine_pan = off_fine_pan;
    let mut c_program = off_program;
    let mut c_pitch_lsb = off_pitch_lsb;
    let mut c_nrpn_msb = off_nrpn_msb;
    let mut c_nrpn_lsb = off_nrpn_lsb;
    let mut c_rpn_msb = off_rpn_msb;
    let mut c_rpn_lsb = off_rpn_lsb;
    let mut c_tempo = off_tempo;
    let mut c_ctrl_num_re = off_switch_ctrls - ctrl_count;

    let mut channel = 0i32;
    let mut note_num = 0i32;
    let mut note_on_vel = 0i32;
    let mut note_off_vel = 0i32;
    let mut pitch_bend = 0i32;
    let mut ch_aft = 0i32;
    let mut poly_aft_val = 0i32;
    let mut ctrl_val = [0i32; 128];
    let mut ctrl_num = 0i32;

    let src_data = p.data.clone();

    'tracks: for _ in 0..num_tracks {
        mid.p4(0x4D54_726Bu32 as i32);
        mid.pos += 4;
        let track_payload_start = mid.pos as usize;
        let mut prev_status: i32 = -1;

        loop {
            let delta = p.gMidiVarLen();
            mid.pMidiVarLen(delta);

            let type_byte = i32::from(src_data[c_type]);
            c_type += 1;
            let status_changed = prev_status != type_byte;
            prev_status = type_byte & 0xF;

            if type_byte == 7 {
                if status_changed { mid.p1(0xFF); }
                mid.p1(0x2F);
                mid.p1(0x00);
                mid.psize4((mid.pos as usize - track_payload_start) as i32);
                continue 'tracks;
            }

            if type_byte == 23 {
                if status_changed { mid.p1(0xFF); }
                mid.p1(0x51);
                mid.p1(0x03);
                mid.p1(i32::from(src_data[c_tempo])); c_tempo += 1;
                mid.p1(i32::from(src_data[c_tempo])); c_tempo += 1;
                mid.p1(i32::from(src_data[c_tempo])); c_tempo += 1;
            } else {
                channel ^= type_byte >> 4;
                let event_type = type_byte & 0xF;
                match event_type {
                    0 => {
                        if status_changed { mid.p1(channel + 0x90); }
                        note_num = note_num.wrapping_add(signed(src_data[c_note_num]));
                        c_note_num += 1;
                        note_on_vel = note_on_vel.wrapping_add(signed(src_data[c_note_on_v]));
                        c_note_on_v += 1;
                        mid.p1(note_num & 0x7F);
                        mid.p1(note_on_vel & 0x7F);
                    }
                    1 => {
                        if status_changed { mid.p1(channel + 0x80); }
                        note_num = note_num.wrapping_add(signed(src_data[c_note_num]));
                        c_note_num += 1;
                        note_off_vel = note_off_vel.wrapping_add(signed(src_data[c_note_off_v]));
                        c_note_off_v += 1;
                        mid.p1(note_num & 0x7F);
                        mid.p1(note_off_vel & 0x7F);
                    }
                    2 => {
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
                        if status_changed { mid.p1(channel + 0xE0); }
                        let inc_lsb = pitch_bend.wrapping_add(signed(src_data[c_pitch_lsb]));
                        c_pitch_lsb += 1;
                        pitch_bend = inc_lsb.wrapping_add(signed(src_data[c_pitch_msb]) << 7);
                        c_pitch_msb += 1;
                        mid.p1(pitch_bend & 0x7F);
                        mid.p1((pitch_bend >> 7) & 0x7F);
                    }
                    4 => {
                        if status_changed { mid.p1(channel + 0xD0); }
                        ch_aft = ch_aft.wrapping_add(signed(src_data[c_ch_aft]));
                        c_ch_aft += 1;
                        mid.p1(ch_aft & 0x7F);
                    }
                    5 => {
                        if status_changed { mid.p1(channel + 0xA0); }
                        note_num = note_num.wrapping_add(signed(src_data[c_note_num]));
                        c_note_num += 1;
                        poly_aft_val = poly_aft_val.wrapping_add(signed(src_data[c_poly_val]));
                        c_poly_val += 1;
                        mid.p1(note_num & 0x7F);
                        mid.p1(poly_aft_val & 0x7F);
                    }
                    6 => {
                        if status_changed { mid.p1(channel + 0xC0); }
                        mid.p1(i32::from(src_data[c_program]));
                        c_program += 1;
                    }
                    _ => panic!("Jagex MIDI: bad event type {event_type}"),
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
