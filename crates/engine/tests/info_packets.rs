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
    assert_eq!(r.g_bit(15), 32767, "terminator");
}
