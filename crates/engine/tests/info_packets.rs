//! Smoke tests for the PLAYER_INFO / NPC_INFO builders: encode a few
//! scenarios and decode the bitstream back with the same reads the
//! rev1 client performs.

use engine::World;
use io::packet::Packet;

fn bits(body: Vec<u8>) -> Packet {
    let mut p = Packet::from_vec(body);
    p.bit_start();
    p
}

#[test]
fn local_teleport_block() {
    let mut world = World::new();
    let pid = world.add_player("test".into(), 3222, 3222, 0).unwrap();
    world.cycle();

    // First cycle: the login tele + appearance flag must be present.
    let p = world.players[pid].as_mut().unwrap();
    let info = p.out.iter().find(|m| m.opcode == 113).expect("player info");
    let mut r = bits(info.body.clone());

    assert_eq!(r.g_bit(1), 1, "has info");
    assert_eq!(r.g_bit(2), 3, "teleport mode");
    let _level = r.g_bit(2);
    let lx = r.g_bit(7);
    let flagged = r.g_bit(1);
    let lz = r.g_bit(7);
    let _jump = r.g_bit(1);
    assert_eq!(flagged, 1, "appearance must be flagged on login");
    // 3222 & build area origin: zone 402 → origin (396)*8 = 3168 → local 54.
    assert_eq!(lx, 54);
    assert_eq!(lz, 54);

    // old vis count 0, newvis terminator.
    assert_eq!(r.g_bit(8), 0);
    assert_eq!(r.g_bit(11), 2047);
    r.bit_end();

    // Extended: flags byte with appearance bit.
    let flags = r.g1();
    assert_ne!(flags & 0x2, 0, "appearance mask");
}

#[test]
fn second_player_enters_via_newvis() {
    let mut world = World::new();
    let a = world.add_player("a".into(), 3222, 3222, 0).unwrap();
    let b = world.add_player("b".into(), 3224, 3220, 0).unwrap();
    world.cycle(); // both flagged + tele on cycle 1
    for p in world.players.iter_mut().flatten() {
        p.out.clear();
    }
    world.cycle(); // second cycle: steady state, b tracked by a

    let pa = world.players[a].as_mut().unwrap();
    let info = pa.out.iter().find(|m| m.opcode == 113).expect("player info");
    let mut r = bits(info.body.clone());

    assert_eq!(r.g_bit(1), 0, "local: no info in steady state");
    assert_eq!(r.g_bit(8), 1, "tracking one player");
    assert_eq!(r.g_bit(1), 0, "tracked: no change");
    assert_eq!(r.g_bit(11), 2047, "no further adds");
    let _ = b;
}

#[test]
fn local_exact_move_block() {
    let mut world = World::new();
    let pid = world.add_player("mover".into(), 3222, 3222, 0).unwrap();
    world.cycle(); // cycle 1: login tele + appearance (now seen)
    world.players[pid].as_mut().unwrap().out.clear();

    // Force a 3-tile glide east over 8 cycles, facing east (3). 0x100 is the
    // player EXACT_MOVE info bit.
    world.players[pid].as_mut().unwrap().entity.set_exact_move(
        3222, 3222, 3225, 3222, 0, 8, 3, 0x100,
    );
    world.cycle(); // cycle 2: exact-move rides the extended block

    let p = world.players[pid].as_mut().unwrap();
    let info = p.out.iter().find(|m| m.opcode == 113).expect("player info");
    let mut r = bits(info.body.clone());

    // Local block: exact-move snaps the true tile, so it rides teleport mode
    // and carries our scene-local tile coords.
    assert_eq!(r.g_bit(1), 1, "has info");
    assert_eq!(r.g_bit(2), 3, "teleport mode");
    let _level = r.g_bit(2);
    let lx = r.g_bit(7);
    assert_eq!(r.g_bit(1), 1, "flagged by the exact-move mask");
    let lz = r.g_bit(7);
    let _jump = r.g_bit(1);

    assert_eq!(r.g_bit(8), 0, "no tracked players");
    assert_eq!(r.g_bit(11), 2047, "newvis terminator");
    r.bit_end();

    // Extended flags: EXACT_MOVE (0x100) forces the two-byte BIG layout.
    let mut flags = r.g1();
    if flags & 0x40 != 0 {
        flags += r.g1() << 8;
    }
    assert_ne!(flags & 0x100, 0, "EXACT_MOVE flagged");
    assert_eq!(flags & 0x2, 0, "appearance already seen — not re-sent");

    // Exact-move payload — getPlayerPosExtended read order, scene-local coords.
    let start_x = r.g1();
    let start_z = r.g1_alt2();
    let end_x = r.g1();
    let end_z = r.g1_alt1();
    let end_cycle = r.g2_alt2();
    let start_cycle = r.g2();
    let facing = r.g1_alt2();

    assert_eq!(end_x, lx, "glide ends on the (new) true tile");
    assert_eq!(start_x, lx - 3, "glide starts 3 tiles west");
    assert_eq!((start_z, end_z), (lz, lz), "no north/south movement");
    assert_eq!((start_cycle, end_cycle), (0, 8), "cycle deltas");
    assert_eq!(facing, 3, "facing east");
}

#[test]
fn newvis_sends_current_facing_not_a_constant() {
    // A new observer should see an already-walking player turned the way they
    // are travelling, not snapped to a hardcoded direction.
    let mut world = World::new();
    let walker = world.add_player("walker".into(), 3260, 3260, 0).unwrap();
    // Walk east a couple of tiles so the orientation settles to "east" (4).
    world.handle_message(
        walker,
        protocol::client::ClientMessage::MoveClick { route: vec![(3264, 3260)], ctrl_held: false },
    );
    world.cycle();
    world.cycle();

    // Now a second player logs in nearby and observes the walker via new-vis.
    let observer = world.add_player("obs".into(), 3262, 3261, 0).unwrap();
    world.cycle();

    let p = world.players[observer].as_mut().unwrap();
    let info = p.out.iter().find(|m| m.opcode == 113).expect("player info");
    let mut r = bits(info.body.clone());

    // Skip the observer's own local block (login teleport).
    assert_eq!(r.g_bit(1), 1);
    assert_eq!(r.g_bit(2), 3, "local teleport");
    let _level = r.g_bit(2);
    let _lx = r.g_bit(7);
    let _flag = r.g_bit(1);
    let _lz = r.g_bit(7);
    let _jump = r.g_bit(1);

    // Old vis empty, then the walker enters via new-vis.
    assert_eq!(r.g_bit(8), 0, "no tracked players yet");
    let pid_bits = r.g_bit(11);
    assert_eq!(pid_bits as usize, walker, "walker added");
    // Client read order: dz(5), dir(3), dx(5), ...
    let _dz = r.g_bit(5);
    let dir = r.g_bit(3);
    assert_eq!(dir, 4, "observed facing east — the walker's travel direction");
}

#[test]
fn npc_newvis_block() {
    let mut world = World::new();
    let pid = world.add_player("test".into(), 3222, 3222, 0).unwrap();
    let _nid = world.add_npc(1, 3225, 3221, 0).unwrap();
    world.cycle();

    let p = world.players[pid].as_mut().unwrap();
    let info = p.out.iter().find(|m| m.opcode == 167).expect("npc info");
    let mut r = bits(info.body.clone());

    assert_eq!(r.g_bit(8), 0, "no tracked npcs yet");
    let nid = r.g_bit(15);
    assert_eq!(nid, 0, "first npc slot");
    let _dir = r.g_bit(3);
    let dz = r.g_bit(5);
    let _flag = r.g_bit(1);
    let _jump = r.g_bit(1);
    let type_id = r.g_bit(14);
    let dx = r.g_bit(5);
    assert_eq!(type_id, 1);
    // dz = 3221-3222 = -1 → 31 in 5-bit two's complement.
    assert_eq!(dz, 31);
    assert_eq!(dx, 3);
    // No 15-bit terminator follows — count(8)+add(44)=52 bits → 7 bytes. With
    // the old terminator the packet was 9 bytes the client couldn't finish.
    assert_eq!(info.body.len(), 7, "no trailing terminator bytes");
}

#[test]
fn empty_npc_info_is_just_the_count_byte() {
    // 0 npcs in view: the packet must be exactly 1 byte (the 8-bit count). The
    // old 15-bit terminator made it 3 bytes, which the client could never
    // finish reading (its new-vis loop stops at <27 bits left), throwing
    // `gnp1 pos:1 psize:3` and force-dropping the connection right after login.
    let mut world = World::new();
    let pid = world.add_player("solo".into(), 3222, 3222, 0).unwrap();
    world.cycle();
    let p = world.players[pid].as_mut().unwrap();
    let info = p.out.iter().find(|m| m.opcode == 167).expect("npc info");
    assert_eq!(info.body.len(), 1, "empty npc info is one byte, not three");

    let mut r = bits(info.body.clone());
    assert_eq!(r.g_bit(8), 0, "count = 0");
}
