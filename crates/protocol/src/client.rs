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
        // CLIENT_CHEAT (::commands) — the client (Client.doCheat) sends opcode
        // 30 with a p1 length + jstr body (sans `::`). 30 is already var-sized
        // in the group below; decode routes it to ClientCheat.
        155 => 4,  // IF_BUTTON — opcode + p4(component), no sub (deob 8514)
        93 => -1,  // clan/social

        // World-interaction opcodes — verified opcode-by-opcode against the
        // rev1 client's menu-action dispatch (Client.java useMenuOption). The
        // basic OP*1-5 carry the target id (+ tile for world entities); the
        // examine (E), spell-on (T), and use-with (U) variants carry extra
        // component / coord words. The server doesn't act on most of these yet,
        // but the sizes must be exact so it consumes and skips them without
        // desyncing the stream.

        // OPPLAYER1-8 — p2(slot)
        246 | 146 | 102 | 78 | 117 | 111 | 119 | 145 => 2,
        183 => 8,  // OPPLAYERT — p2 + p4 + p2
        226 => 10, // OPPLAYERU — p2 + p2 + p2 + p4

        // OPNPC1-5 — p2(nid)
        84 | 13 | 67 | 95 | 88 => 2,
        52 => 2,   // OPNPCE (examine) — p2(type)
        190 => 8,  // OPNPCT — p4 + p2 + p2
        106 => 10, // OPNPCU — p2 + p4 + p2 + p2

        // OPLOC1-5 — p2(loc) + p2(x) + p2(z)
        73 | 90 | 133 | 83 | 56 => 6,
        162 => 2,  // OPLOCE (examine) — p2(loc)
        247 => 12, // OPLOCT — p4 + p2 + p2 + p2 + p2
        241 => 14, // OPLOCU — p4 + p2 + p2 + p2 + p2 + p2

        // OPOBJ1-5 — p2(obj) + p2(x) + p2(z)
        243 | 177 | 224 | 139 | 77 => 6,
        49 => 2,   // OPOBJE (examine) — p2(obj)
        81 => 12,  // OPOBJT — p2 + p2 + p4 + p2 + p2
        235 => 14, // OPOBJU — p2 + p2 + p2 + p4 + p2 + p2

        // Interface-component ops — IF_BUTTON1-10 (Client.java ifButtonX), each
        // p4(component) + p2(sub).
        63 | 87 | 238 | 240 | 153 | 232 | 168 | 239 | 254 | 169 => 6,
        251 => 12, // IF_BUTTONT — p2 + p2 + p4 + p4
        242 => 6,  // RESUME_PAUSEBUTTON — p2 + p4
        27 => 4,   // RESUME_P_COUNTDIALOG — p4 (the entered amount)

        // OPHELD1-5 (item-on-X) — component + slot + item words, all 8 bytes.
        135 | 179 | 76 | 220 | 19 => 8,
        218 => 14, // OPHELDT — p2 + p2 + p2 + p4 + p4
        70 => 16,  // OPHELDU — p2 + p2 + p2 + p4 + p2 + p4

        // INV_BUTTON1-5 — p2(slot) + p4(component) + p2(item).
        21 | 202 | 6 | 186 | 40 => 8,
        2 => 8,    // INV_BUTTOND (inventory drag) — p4 + p2 + p2
        71 => 13,  // IDK_SAVEDESIGN — p1(gender) + 7×p1(part) + 5×p1(colour)

        // Zero-body control packets.
        38 => 0,   // IDLE_TIMER (10s anti-AFK warning)
        129 => 0,  // CLOSE_MODAL

        // MESSAGE_PUBLIC (public chat) — 1-byte length prefix; decoded + handled.
        205 => -1,

        // Social / chat — friend / ignore / clan-chat ops, each with a 1-byte
        // length prefix. The server skips them until the social system lands,
        // but the sizes keep the stream in sync.
        30 | 41 | 185 | 203 | 231 | 245 | 248 | 252 => -1,

        // SET_CHATFILTERSETTINGS (CHAT_SETMODE) — p1(public) p1(private)
        // p1(trade). Sent when the player changes a chat filter mode (e.g.
        // public → friends). The engine doesn't track per-player chat modes
        // yet, but the size must be known or the stream desyncs and the
        // connection is dropped (which the client mishandles as a hang).
        167 => 3,

        // Client→server opcodes the gameplay UI / cs2 scripts emit that the
        // engine doesn't act on yet. Their SIZES must still be known or the
        // read pump desyncs and drops the connection (the bug the 167 note
        // above describes). decode() returns NoOp for each.
        211 => -2,  // MESSAGE_PRIVATE — u16-length-prefixed (pjstr recipient
                    // + WordPack message).
        96 => -1,   // REPORT_ABUSE — u8-length-prefixed (name + reason + flag).
        223 => -1,  // RESUME_P_NAMEDIALOG — u8-length-prefixed string.
        127 => -1,  // RESUME_P_STRINGDIALOG — u8-length-prefixed string.

        _ => return None,
    })
}

#[derive(Clone, Debug)]
pub enum ClientMessage {
    MoveClick { route: Vec<(i32, i32)>, ctrl_held: bool },
    ClientCheat { command: String },
    IfButton { op: i32, component: i32, sub: i32 },
    ResumePauseButton { component: i32, sub: i32 },
    /// The amount the player entered in a P_COUNTDIALOG "enter amount" dialog.
    ResumeCountDialog { value: i32 },
    /// Public chat: a colour (0-11), an effect (0-5), and the WordPack-packed
    /// message bytes (re-broadcast as-is to nearby players).
    MessagePublic { colour: i32, effect: i32, message: Vec<u8> },
    /// The player closed the open modal interface client-side (Escape / click
    /// away). Empty body; defers a modal close on the server.
    CloseModal,
    /// OPNPCE — the player examined an npc. Carries the npc CONFIG type id (the
    /// client resolves the multinpc redirect before sending); the server replies
    /// with that type's `desc` examine text.
    ExamineNpc { type_id: i32 },
    /// OPNPC1-5 — the player clicked one of an npc's ops (Talk-to, etc.). Carries
    /// the npc's server index and the 1-based op number; the server sets the
    /// matching ap/op interaction so the [opnpc<n>, …] script fires when in range.
    OpNpc { nid: i32, op: i32 },
    NoOp,
}

/// Decode a fully-buffered packet body. Returns None for opcodes the
/// engine doesn't act on yet (consumed as NoOp).
pub fn decode(opcode: u8, buf: &mut Packet, length: usize) -> ClientMessage {
    match opcode {
        176 | 60 | 214 => decode_move_click(opcode, buf, length),
        30 => ClientMessage::ClientCheat { command: buf.gjstr() },
        155 => {
            // IF_BUTTON — the deob sends `p1Enc(155); p4(component)` and nothing
            // else (4-byte body, no sub). Reading a phantom 2-byte sub here
            // over-read into the following packets and desynced the stream.
            let component = buf.g4();
            ClientMessage::IfButton { op: 1, component, sub: -1 }
        }
        // IF_BUTTON1-10 — a component op click: p4(component) + p2(sub). The
        // opcode selects which of the 10 right-click ops fired.
        63 | 87 | 238 | 240 | 153 | 232 | 168 | 239 | 254 | 169 => {
            let op = match opcode {
                63 => 1, 87 => 2, 238 => 3, 240 => 4, 153 => 5,
                232 => 6, 168 => 7, 239 => 8, 254 => 9, 169 => 10,
                _ => unreachable!(),
            };
            let component = buf.g4();
            let sub = buf.g2();
            ClientMessage::IfButton { op, component, sub }
        }
        // OPNPC1-5 — an npc op click (Talk-to, etc.): p2_alt3(npc index). The
        // opcode selects the op; the client builder is send_op_entity_g2_alt3.
        84 | 13 | 67 | 95 | 88 => {
            let op = match opcode {
                84 => 1, 13 => 2, 67 => 3, 95 => 4, 88 => 5,
                _ => unreachable!(),
            };
            let nid = buf.g2_alt3();
            ClientMessage::OpNpc { nid, op }
        }
        // RESUME_PAUSEBUTTON — the player clicked "continue" on a paused dialog:
        // p2_alt2(sub) + p4(component).
        242 => {
            let sub = buf.g2_alt2();
            let component = buf.g4();
            ClientMessage::ResumePauseButton { component, sub }
        }
        // MESSAGE_PUBLIC — p1(colour), p1(effect), then the WordPack-packed
        // message (the remaining body bytes), re-broadcast to nearby players.
        205 => {
            let colour = buf.g1();
            let effect = buf.g1();
            let msg_len = length.saturating_sub(2);
            let mut message = vec![0u8; msg_len];
            buf.gdata(&mut message, 0, msg_len);
            ClientMessage::MessagePublic { colour, effect, message }
        }
        // RESUME_P_COUNTDIALOG — the entered amount (p4).
        27 => ClientMessage::ResumeCountDialog { value: buf.g4() },
        // CLOSE_MODAL — the player dismissed the open interface (empty body).
        129 => ClientMessage::CloseModal,
        // OPNPCE — examine npc: p2(type id) (client sends `p1_enc(52); p2(id)`).
        52 => ClientMessage::ExamineNpc { type_id: buf.g2() },
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

#[cfg(test)]
mod tests {
    use super::{decode, packet_size, ClientMessage};
    use io::packet::Packet;

    #[test]
    fn message_public_is_sized_and_decodes_colour_effect_and_body() {
        // MESSAGE_PUBLIC rides a 1-byte length prefix (rev1 ScriptRunner `psize1`).
        assert_eq!(packet_size(205), Some(-1), "MESSAGE_PUBLIC var-byte length");

        // Body: p1(colour=3), p1(effect=1), then the packed message bytes.
        let mut buf = Packet::from_vec(vec![3, 1, 0xAB, 0xCD, 0xEF]);
        match decode(205, &mut buf, 5) {
            ClientMessage::MessagePublic { colour, effect, message } => {
                assert_eq!(colour, 3);
                assert_eq!(effect, 1);
                assert_eq!(message, vec![0xAB, 0xCD, 0xEF], "rest-of-body is the packed text");
            }
            other => panic!("expected MessagePublic, got {other:?}"),
        }
    }

    #[test]
    fn resume_countdialog_decodes_the_entered_amount() {
        assert_eq!(packet_size(27), Some(4), "RESUME_P_COUNTDIALOG carries a p4 amount");
        let mut buf = Packet::from_vec(vec![0, 0, 0, 77]); // p4 = 77
        assert!(matches!(decode(27, &mut buf, 4),
                         ClientMessage::ResumeCountDialog { value: 77 }));
    }

    #[test]
    fn close_modal_is_a_zero_body_packet() {
        assert_eq!(packet_size(129), Some(0), "CLOSE_MODAL has no body");
        let mut buf = Packet::from_vec(vec![]);
        assert!(matches!(decode(129, &mut buf, 0), ClientMessage::CloseModal));
    }

    // Opcode/size for every world-interaction packet, taken straight from the
    // rev1 client's menu-action dispatch (Client.java useMenuOption). Guards
    // against regressing back to the wrong-rev opcodes that were here before.
    #[test]
    fn interaction_packet_sizes_match_rev1_client() {
        // OPPLAYER1-8 (p2 slot).
        for op in [246, 146, 102, 78, 117, 111, 119, 145] {
            assert_eq!(packet_size(op), Some(2), "OPPLAYER {op}");
        }
        // OPNPC1-5 (p2 nid) — note OPNPC5 is 88, not the old (wrong) 73.
        for op in [84, 13, 67, 95, 88] {
            assert_eq!(packet_size(op), Some(2), "OPNPC {op}");
        }
        assert_eq!(packet_size(52), Some(2), "OPNPCE");
        // OPLOC1-5 (p2 loc + p2 x + p2 z) — 73 is OPLOC1 (was wrongly an OPNPC).
        for op in [73, 90, 133, 83, 56] {
            assert_eq!(packet_size(op), Some(6), "OPLOC {op}");
        }
        assert_eq!(packet_size(162), Some(2), "OPLOCE");
        // OPOBJ1-5 (p2 obj + p2 x + p2 z).
        for op in [243, 177, 224, 139, 77] {
            assert_eq!(packet_size(op), Some(6), "OPOBJ {op}");
        }
        assert_eq!(packet_size(49), Some(2), "OPOBJE");
        // Targeted / use-with variants.
        assert_eq!(packet_size(190), Some(8), "OPNPCT");
        assert_eq!(packet_size(247), Some(12), "OPLOCT");
        assert_eq!(packet_size(235), Some(14), "OPOBJU");

        // The old wrong-rev opcodes must no longer be sized as 6-byte world
        // interactions (251 is now IF_BUTTONT, sized below).
        for stale in [245, 172, 96, 97, 33, 221, 54, 200, 91, 212, 185, 20] {
            assert!(packet_size(stale).map_or(true, |s| s != 6),
                    "stale opcode {stale} should not be a 6-byte interaction");
        }
    }

    #[test]
    fn interface_and_held_packet_sizes_match_rev1_client() {
        // IF_BUTTON1-10 (p4 component + p2 sub).
        for op in [63, 87, 238, 240, 153, 232, 168, 239, 254, 169] {
            assert_eq!(packet_size(op), Some(6), "IF_BUTTON {op}");
        }
        assert_eq!(packet_size(251), Some(12), "IF_BUTTONT");
        assert_eq!(packet_size(242), Some(6), "RESUME_PAUSEBUTTON");
        // OPHELD1-5 (8) + T/U.
        for op in [135, 179, 76, 220, 19] {
            assert_eq!(packet_size(op), Some(8), "OPHELD {op}");
        }
        assert_eq!(packet_size(218), Some(14), "OPHELDT");
        assert_eq!(packet_size(70), Some(16), "OPHELDU");
        // INV_BUTTON1-5 + drag, design save, control + social.
        for op in [21, 202, 6, 186, 40, 2] {
            assert_eq!(packet_size(op), Some(8), "INV_BUTTON/D {op}");
        }
        assert_eq!(packet_size(71), Some(13), "IDK_SAVEDESIGN");
        assert_eq!(packet_size(38), Some(0), "IDLE_TIMER");
        assert_eq!(packet_size(129), Some(0), "CLOSE_MODAL");
        for op in [30, 41, 185, 203, 231, 245, 248, 252] {
            assert_eq!(packet_size(op), Some(-1), "social/chat var-length {op}");
        }
    }

    #[test]
    fn if_button_ops_decode_with_op_index_and_sub() {
        use super::{decode, ClientMessage};
        use io::packet::Packet;
        // IF_BUTTON1-10 body: p4(component) + p2(sub).
        let component = (548 << 16) | 6;
        let sub = 17;
        for (opcode, expect_op) in [(63, 1), (87, 2), (153, 5), (169, 10)] {
            let mut p = Packet::new(8);
            p.p4(component);
            p.p2(sub);
            p.pos = 0;
            match decode(opcode, &mut p, 6) {
                ClientMessage::IfButton { op, component: c, sub: s } => {
                    assert_eq!(op, expect_op, "opcode {opcode} -> op {expect_op}");
                    assert_eq!(c, component);
                    assert_eq!(s, sub);
                }
                other => panic!("expected IfButton, got {other:?}"),
            }
        }
    }

    #[test]
    fn client_cheat_decodes_from_opcode_30() {
        use super::{decode, packet_size, ClientMessage};
        use io::packet::Packet;
        // Client.doCheat sends opcode 30 + p1 length + jstr (with the `::` stripped).
        assert_eq!(packet_size(30), Some(-1), "CLIENT_CHEAT is var-length");
        let mut p = Packet::new(32);
        p.pjstr("if 231");
        p.pos = 0;
        match decode(30, &mut p, 7) {
            ClientMessage::ClientCheat { command } => assert_eq!(command, "if 231"),
            other => panic!("expected ClientCheat, got {other:?}"),
        }
    }

    // Every opcode the rev1 client (Client.java) can send must be sized so the
    // server consumes-and-skips it rather than dropping the connection on an
    // unknown opcode. This is the full out.p1Enc(..) set from the client.
    #[test]
    fn every_rev1_client_opcode_is_sized() {
        const SENT_BY_CLIENT: &[u8] = &[
            2, 6, 13, 19, 21, 22, 30, 38, 40, 41, 49, 52, 56, 60, 63, 67, 70, 71,
            72, 73, 76, 77, 78, 79, 81, 83, 84, 87, 88, 90, 95, 102, 106, 111,
            117, 119, 129, 133, 135, 139, 145, 146, 153, 155, 161, 162, 168, 169,
            176, 177, 178, 179, 183, 185, 186, 190, 197, 202, 203, 210, 214, 218,
            220, 224, 226, 228, 231, 232, 235, 238, 239, 240, 241, 242, 243, 245,
            246, 247, 248, 251, 252, 254,
        ];
        for &op in SENT_BY_CLIENT {
            assert!(packet_size(op).is_some(),
                    "opcode {op} is sent by the client but has no size — server would drop");
        }
    }
}
