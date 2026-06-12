//! `jagex3.midi2.MidiParser` — walks a standard MIDI byte buffer track-by-track.
//!
//! Not a self-contained MIDI file parser: it indexes track byte ranges up front, then
//! callers `set_track` to position the cursor, `process_delta_time` / `get_event` to walk
//! events, and `next_track_to_play` / `all_tracks_finished` to schedule across tracks. The
//! synth's player consumes events one at a time, the file's patch-discovery walk reuses
//! the same machinery.
//!
//! Event encoding returned by `get_event`:
//! - `0` — sysex / system message we don't dispatch on
//! - `1` — end-of-track meta (0xFF 0x2F)
//! - `2` — tempo meta (0xFF 0x51) — `tempo` and `base_time` are updated as a side effect
//! - `3` — other meta (skipped)
//! - other — packed channel-voice message: `status | (data1 << 8) | (data2 << 16)`

use io::packet::Packet;

/// Per-status-byte payload length (matches Java `MidiParser.msgLen`).
const MSG_LEN: [i8; 128] = [
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    0, 1, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

pub struct MidiParser {
    pub packet: Packet,
    pub division: i32,
    pub track_start_pos: Vec<i32>,
    pub track_current_pos: Vec<i32>,
    pub track_current_tick: Vec<i32>,
    pub track_current_status: Vec<i32>,
    pub tempo: i32,
    pub base_time: i64,
}

impl Default for MidiParser {
    fn default() -> Self {
        Self {
            packet: Packet::default(),
            division: 0,
            track_start_pos: Vec::new(),
            track_current_pos: Vec::new(),
            track_current_tick: Vec::new(),
            track_current_status: Vec::new(),
            tempo: 500_000,
            base_time: 0,
        }
    }
}

impl MidiParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_midi(src: Vec<u8>) -> Self {
        let mut me = Self::default();
        me.set_midi(src);
        me
    }

    /// Index the standard-MIDI buffer. Reads MThd to get track count + division, then
    /// scans MTrk chunks recording each track's start offset.
    pub fn set_midi(&mut self, src: Vec<u8>) {
        self.packet.data = src;
        self.packet.pos = 10;
        let track_count = self.packet.g2() as usize;
        self.division = self.packet.g2();
        self.tempo = 500_000;
        self.track_start_pos = Vec::with_capacity(track_count);
        let mut indexed = 0usize;
        while indexed < track_count {
            let chunk = self.packet.g4();
            let len = self.packet.g4() as usize;
            if chunk == 0x4d54_726b {
                self.track_start_pos.push(self.packet.pos as i32);
                indexed += 1;
            }
            self.packet.pos += len;
        }
        self.base_time = 0;
        self.track_current_pos = self.track_start_pos.clone();
        self.track_current_tick = vec![0; track_count];
        self.track_current_status = vec![0; track_count];
    }

    pub fn drop_midi(&mut self) {
        self.packet.data = Vec::new();
        self.track_start_pos.clear();
        self.track_current_pos.clear();
        self.track_current_tick.clear();
        self.track_current_status.clear();
    }

    pub fn got_midi(&self) -> bool {
        !self.packet.data.is_empty()
    }

    pub fn track_count(&self) -> usize {
        self.track_current_pos.len()
    }

    pub fn set_track(&mut self, t: usize) {
        self.packet.pos = self.track_current_pos[t] as usize;
    }

    pub fn unset_track(&mut self, t: usize) {
        self.track_current_pos[t] = self.packet.pos as i32;
    }

    /// Mark the currently-set track as finished. Java uses `-1` in `trackCurrentPos`; we
    /// follow that — `next_track_to_play` skips negative entries.
    pub fn finish_track(&mut self) {
        self.packet.pos = usize::MAX; // poisons unset_track to write -1.
    }

    pub fn process_delta_time(&mut self, t: usize) {
        let d = self.packet.g_midi_var_len();
        self.track_current_tick[t] = self.track_current_tick[t].wrapping_add(d);
    }

    pub fn get_event(&mut self, t: usize) -> i32 {
        self.get_event2(t)
    }

    fn get_event2(&mut self, t: usize) -> i32 {
        let first = self.packet.data[self.packet.pos] as i8;
        let status;
        if first < 0 {
            status = first as u8 as i32;
            self.track_current_status[t] = status;
            self.packet.pos += 1;
        } else {
            status = self.track_current_status[t];
        }
        if status != 240 && status != 247 {
            return self.get_event3(t, status);
        }
        let payload_len = self.packet.g_midi_var_len() as usize;
        if status == 247 && payload_len > 0 {
            let realtime = self.packet.data[self.packet.pos] as u8 as i32;
            let is_realtime = (241..=243).contains(&realtime)
                || realtime == 246
                || realtime == 248
                || (250..=252).contains(&realtime)
                || realtime == 254;
            if is_realtime {
                self.packet.pos += 1;
                self.track_current_status[t] = realtime;
                return self.get_event3(t, realtime);
            }
        }
        self.packet.pos += payload_len;
        0
    }

    fn get_event3(&mut self, t: usize, status: i32) -> i32 {
        if status != 255 {
            let n = MSG_LEN[(status - 128) as usize];
            let mut packed = status;
            if n >= 1 {
                packed |= self.packet.g1() << 8;
            }
            if n >= 2 {
                packed |= self.packet.g1() << 16;
            }
            return packed;
        }
        let meta = self.packet.g1();
        let mut len = self.packet.g_midi_var_len() as usize;
        match meta {
            47 => {
                self.packet.pos += len;
                1
            }
            81 => {
                let new_tempo = self.packet.g3();
                len -= 3;
                let tick = self.track_current_tick[t] as i64;
                self.base_time += (self.tempo as i64 - new_tempo as i64) * tick;
                self.tempo = new_tempo;
                self.packet.pos += len;
                2
            }
            _ => {
                self.packet.pos += len;
                3
            }
        }
    }

    pub fn time_from_tick(&self, tick: i32) -> i64 {
        self.tempo as i64 * tick as i64 + self.base_time
    }

    pub fn next_track_to_play(&self) -> Option<usize> {
        let mut best = None;
        let mut best_tick = i32::MAX;
        for (i, &pos) in self.track_current_pos.iter().enumerate() {
            if pos >= 0 && self.track_current_tick[i] < best_tick {
                best = Some(i);
                best_tick = self.track_current_tick[i];
            }
        }
        best
    }

    pub fn all_tracks_finished(&self) -> bool {
        self.track_current_pos.iter().all(|&p| p < 0)
    }

    pub fn restart(&mut self, base: i64) {
        self.base_time = base;
        for t in 0..self.track_current_pos.len() {
            self.track_current_tick[t] = 0;
            self.track_current_status[t] = 0;
            self.packet.pos = self.track_start_pos[t] as usize;
            self.process_delta_time(t);
            self.track_current_pos[t] = self.packet.pos as i32;
        }
    }
}
