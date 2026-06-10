//! Client→server packet metadata + decoders. Sizes mirror the
//! Engine2007 GameClientRepository bindings; -1 = 1-byte length
//! prefix, -2 = 2-byte.

use io::packet::Packet;

/// (opcode, declared size). Unlisted opcodes are unknown and the
/// connection should be dropped (or the opcode logged) — mirrors the
/// reference behaviour.
pub fn packet_size(opcode: u8) -> Option<i32> {
    Some(match opcode {
        // Movement (MOVE_GAMECLICK / MOVE_MINIMAPCLICK / MOVE_OPCLICK)
        176 | 60 | 214 => -1,

        // No-op tracking / camera / focus / idle packets.
        72 => -1,  // EVENT_MOUSE_MOVE
        228 => 0,  // idle
        210 => 4,
        161 => 4,  // EVENT_MOUSE_CLICK
        79 => 4,   // EVENT_CAMERA_POSITION
        178 => 1,  // window focus
        197 => 0,

        22 => 12,  // IF_BUTTOND (drag)
        92 => -1,  // CLIENT_CHEAT (::commands)
        155 => 6,  // IF_BUTTON
        93 => -1,  // clan/social

        // OPPLAYER1-8
        246 | 146 | 102 | 78 | 117 | 111 | 119 | 145 => 2,

        // OPNPC1-5 + OPNPCT/OPNPCU-era extras
        84 | 13 | 67 | 95 | 73 => 2,
        212 => 2, // OPNPC6 (examine)

        // OPLOC1-5
        245 | 172 | 96 | 97 | 33 => 6,
        185 => 4, // OPLOC6 (examine)

        // OPOBJ1-5
        221 | 54 | 200 | 251 | 91 => 6,
        20 => 2, // OPOBJ6 (examine)

        _ => return None,
    })
}

#[derive(Clone, Debug)]
pub enum ClientMessage {
    MoveClick { route: Vec<(i32, i32)>, ctrl_held: bool },
    ClientCheat { command: String },
    IfButton { op: i32, component: i32, sub: i32 },
    NoOp,
}

/// Decode a fully-buffered packet body. Returns None for opcodes the
/// engine doesn't act on yet (consumed as NoOp).
pub fn decode(opcode: u8, buf: &mut Packet, length: usize) -> ClientMessage {
    match opcode {
        176 | 60 | 214 => decode_move_click(opcode, buf, length),
        92 => ClientMessage::ClientCheat { command: buf.gjstr() },
        155 => {
            // IF_BUTTON — opindex-1 variant; component p4, sub p2.
            let component = buf.g4();
            let sub = buf.g2();
            ClientMessage::IfButton { op: 1, component, sub }
        }
        _ => ClientMessage::NoOp,
    }
}

// Mirrors Engine2007 MoveClickDecoder: checkpoint waypoints as signed
// byte deltas from the trailing absolute start coords (minimap clicks
// carry 14 bytes of extra input telemetry).
fn decode_move_click(opcode: u8, buf: &mut Packet, length: usize) -> ClientMessage {
    let offset = if opcode == 60 { 14 } else { 0 };
    let waypoints = (length as i32 - 3 - offset) / 2;

    let mut path: Vec<(i32, i32)> = Vec::new();
    for _ in 1..waypoints {
        let x = buf.g1b_alt2_signed();
        let z = buf.g1b_alt3() as i32;
        path.push((x, z));
    }

    let start_z = buf.g2_alt3();
    let ctrl_held = buf.g1();
    let start_x = buf.g2();

    let mut route: Vec<(i32, i32)> = Vec::with_capacity(path.len() + 1);
    route.push((start_x, start_z));
    for (dx, dz) in path {
        route.push((start_x + dx, start_z + dz));
    }

    ClientMessage::MoveClick { route, ctrl_held: ctrl_held == 1 }
}

trait PacketExt {
    fn g1b_alt2_signed(&mut self) -> i32;
}

impl PacketExt for Packet {
    fn g1b_alt2_signed(&mut self) -> i32 {
        // alt2 signed byte: -value & 0xFF reinterpreted as i8.
        let v = self.g1_alt2();
        if v > 0x7F { v - 0x100 } else { v }
    }
}
