//! Server→client messages. Each builder returns a [`ServerPacket`]
//! (opcode + body); the connection layer frames it (opcode byte +
//! optional size prefix) when flushing.
//!
//! Opcodes/sizes mirror Engine2007
//! src/network/os1/server/codec/game/*Encoder.ts; the byte layouts
//! are the same ones crates/client's packet handlers decode.

use io::packet::Packet;

/// How the packet length is framed on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeKind {
    /// Fixed body size — no length prefix.
    Fixed(usize),
    /// 1-byte length prefix.
    Var1,
    /// 2-byte length prefix.
    Var2,
}

#[derive(Clone, Debug)]
pub struct ServerPacket {
    pub opcode: u8,
    pub size: SizeKind,
    pub body: Vec<u8>,
}

impl ServerPacket {
    /// Frame onto `out`: opcode (ISAAC-encrypted by the caller when
    /// the stream is ciphered — rev1 login currently runs without),
    /// then the size prefix, then the body.
    pub fn frame(&self, out: &mut Packet) {
        out.p1(self.opcode as i32);
        match self.size {
            SizeKind::Fixed(n) => debug_assert_eq!(n, self.body.len()),
            SizeKind::Var1 => out.p1(self.body.len() as i32),
            SizeKind::Var2 => out.p2(self.body.len() as i32),
        }
        out.pdata(&self.body, 0, self.body.len());
    }
}

fn body(capacity: usize) -> Packet {
    Packet::new(capacity)
}

fn finish(opcode: u8, size: SizeKind, p: Packet) -> ServerPacket {
    let mut data = p.data;
    data.truncate(p.pos as usize);
    ServerPacket { opcode, size, body: data }
}

// ── Simple messages ────────────────────────────────────────────────

/// MESSAGE_GAME (100) — chatbox line.
pub fn message_game(text: &str) -> ServerPacket {
    let mut p = body(text.len() + 1);
    p.pjstr(text);
    finish(100, SizeKind::Var1, p)
}

/// IF_OPENTOP (147).
pub fn if_opentop(interface_id: i32) -> ServerPacket {
    let mut p = body(2);
    p.p2_alt1(interface_id);
    finish(147, SizeKind::Fixed(2), p)
}

/// IF_OPENSUB (184) — `component` is the packed (parent << 16 | child)
/// attachment point, `sub_id` the interface group to open, `kind`
/// 0 = modal, 1 = overlay.
pub fn if_opensub(component: i32, sub_id: i32, kind: i32) -> ServerPacket {
    let mut p = body(7);
    p.p1_alt2(kind);
    p.p2_alt2(sub_id);
    p.p4_alt1(component);
    finish(184, SizeKind::Fixed(7), p)
}

/// LOGOUT (224).
pub fn logout() -> ServerPacket {
    ServerPacket { opcode: 224, size: SizeKind::Fixed(0), body: Vec::new() }
}

/// MIDI_SONG (211).
pub fn midi_song(id: i32) -> ServerPacket {
    let mut p = body(2);
    p.p2_alt1(id);
    finish(211, SizeKind::Fixed(2), p)
}

/// MIDI_JINGLE (53) — trailing 3 bytes are read but unused by the
/// client (length-of-jingle in the original).
pub fn midi_jingle(id: i32) -> ServerPacket {
    let mut p = body(5);
    p.p2_alt2(id);
    p.p1(0);
    p.p1(0);
    p.p1(0);
    finish(53, SizeKind::Fixed(5), p)
}

/// SYNTH_SOUND (229).
pub fn synth_sound(id: i32, loops: i32, delay: i32) -> ServerPacket {
    let mut p = body(5);
    p.p2(id);
    p.p1(loops);
    p.p2(delay);
    finish(229, SizeKind::Fixed(5), p)
}

/// UPDATE_RUNENERGY (41).
pub fn update_runenergy(value: i32) -> ServerPacket {
    let mut p = body(1);
    p.p1(value);
    finish(41, SizeKind::Fixed(1), p)
}

/// UPDATE_RUNWEIGHT (1).
pub fn update_runweight(value: i32) -> ServerPacket {
    let mut p = body(2);
    p.p2(value);
    finish(1, SizeKind::Fixed(2), p)
}

/// UPDATE_STAT (208).
pub fn update_stat(stat: i32, level: i32, experience: i32) -> ServerPacket {
    let mut p = body(6);
    p.p1_alt1(level);
    p.p1_alt1(stat);
    p.p4(experience);
    finish(208, SizeKind::Fixed(6), p)
}

/// REBUILD_NORMAL (21) — recentres the client's 104×104 build area on
/// the zone containing the absolute tile. `xtea_keys` supplies the
/// 4-int key set per visible mapsquare in (mx, mz) iteration order;
/// pass zeros for unencrypted map data.
pub fn rebuild_normal<F: FnMut(i32, i32) -> [i32; 4]>(
    abs_x: i32, abs_z: i32, mut xtea_keys: F,
) -> ServerPacket {
    let zx = abs_x >> 3;
    let zz = abs_z >> 3;

    let mut p = body(256);
    p.p2(abs_z - ((zz - 6) << 3));
    p.p2_alt1(abs_x - ((zx - 6) << 3));

    let mut mx = (zx - 6) >> 3;
    while mx <= (zx + 6) >> 3 {
        let mut mz = (zz - 6) >> 3;
        while mz <= (zz + 6) >> 3 {
            let key = xtea_keys(mx, mz);
            for k in key {
                p.p4_alt2(k);
            }
            mz += 1;
        }
        mx += 1;
    }

    p.p1_alt2(0);

    p.p2(zx);
    p.p2_alt3(zz);
    finish(21, SizeKind::Var2, p)
}

/// The 104×104 build-area origin tile for a centre zone.
pub fn build_area_origin(abs_x: i32, abs_z: i32) -> (i32, i32) {
    (((abs_x >> 3) - 6) << 3, ((abs_z >> 3) - 6) << 3)
}
