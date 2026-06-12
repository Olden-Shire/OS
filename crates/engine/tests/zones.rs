//! Zone spatial index: membership tracks add/move/remove, and the info
//! builder's nearby queries pull from the build-area zones (not a full scan).

use engine::World;

#[test]
fn nearby_player_ids_uses_zone_index() {
    let mut w = World::new();
    let a = w.add_player("a".into(), 3222, 3222, 0).unwrap();
    let b = w.add_player("b".into(), 3225, 3226, 0).unwrap(); // a few tiles away
    let c = w.add_player("c".into(), 3400, 3400, 0).unwrap(); // far off

    let near = w.nearby_player_ids(3222, 3222, 0);
    assert!(near.contains(&a), "observer's own zone");
    assert!(near.contains(&b), "neighbour within view");
    assert!(!near.contains(&c), "distant player excluded");

    // Sorted ascending (stable info-stream order).
    let mut sorted = near.clone();
    sorted.sort_unstable();
    assert_eq!(near, sorted);
}

#[test]
fn membership_updates_when_a_player_walks_across_a_zone() {
    let mut w = World::new();
    let pid = w.add_player("walker".into(), 3216, 3216, 0).unwrap();
    let start_zone = engine::zone::zone_index(3216, 3216, 0);
    assert_eq!(w.zones.players_in(start_zone), &[pid]);

    // Walk east across the 8-tile zone boundary.
    w.handle_message(
        pid,
        protocol::client::ClientMessage::MoveClick { route: vec![(3226, 3216)], ctrl_held: true },
    );
    for _ in 0..6 {
        w.cycle();
    }

    let end_zone = engine::zone::zone_index(3226, 3216, 0);
    assert_ne!(start_zone, end_zone);
    assert!(w.zones.players_in(start_zone).is_empty(), "left the old zone");
    assert_eq!(w.zones.players_in(end_zone), &[pid], "registered in the new zone");
}

#[test]
fn drop_obj_tracks_in_zone_and_broadcasts_to_nearby_players() {
    let mut w = World::new();
    let pid = w.add_player("p".into(), 3222, 3222, 0).unwrap();
    w.players[pid].as_mut().unwrap().out.clear(); // drop the login packets

    w.drop_obj(995, 100, 3223, 3224, 0); // coins a couple tiles away

    // Tracked in the tile's zone.
    let objs = w.zones.objs_in(engine::zone::zone_index(3223, 3224, 0));
    assert_eq!(objs.len(), 1);
    assert_eq!((objs[0].id, objs[0].count, objs[0].x, objs[0].z), (995, 100, 3223, 3224));

    // The nearby player received the zone-base prefix (89) then OBJ_ADD (173).
    let out = &w.players[pid].as_ref().unwrap().out;
    assert!(out.iter().any(|m| m.opcode == 89), "UPDATE_ZONE_PARTIAL_FOLLOWS");
    let obj = out.iter().find(|m| m.opcode == 173).expect("OBJ_ADD");
    assert_eq!(obj.body.len(), 5, "slot(1) + count(2) + obj_id(2)");
}

#[test]
fn drop_obj_skips_far_players() {
    let mut w = World::new();
    let pid = w.add_player("p".into(), 3222, 3222, 0).unwrap();
    w.players[pid].as_mut().unwrap().out.clear();
    w.drop_obj(995, 1, 3400, 3400, 0); // far outside the player's view
    let out = &w.players[pid].as_ref().unwrap().out;
    assert!(out.iter().all(|m| m.opcode != 173), "no OBJ_ADD for a distant drop");
}

#[test]
fn take_obj_removes_from_zone_and_broadcasts_del() {
    let mut w = World::new();
    let pid = w.add_player("p".into(), 3222, 3222, 0).unwrap();
    w.drop_obj(995, 50, 3223, 3223, 0);
    w.players[pid].as_mut().unwrap().out.clear();

    let removed = w.take_obj(995, 3223, 3223, 0);
    assert_eq!(removed.map(|o| o.count), Some(50));
    assert!(w.zones.objs_in(engine::zone::zone_index(3223, 3223, 0)).is_empty());

    let out = &w.players[pid].as_ref().unwrap().out;
    assert!(out.iter().any(|m| m.opcode == 207), "OBJ_DEL broadcast");
}

#[test]
fn send_zone_objs_resends_pre_existing_items() {
    let mut w = World::new();
    w.drop_obj(1042, 1, 3223, 3224, 0); // dropped with nobody nearby — just tracked
    let pid = w.add_player("late".into(), 3222, 3222, 0).unwrap();
    w.players[pid].as_mut().unwrap().out.clear();

    w.send_zone_objs(pid); // the rebuild path
    let out = &w.players[pid].as_ref().unwrap().out;
    assert!(out.iter().any(|m| m.opcode == 173), "pre-existing item re-sent");
}

#[test]
fn add_loc_tracks_and_broadcasts_change() {
    let mut w = World::new();
    let pid = w.add_player("p".into(), 3222, 3222, 0).unwrap();
    w.players[pid].as_mut().unwrap().out.clear();

    w.add_loc(1530, 0, 1, 3223, 3223, 0); // a door: shape 0, angle 1
    let locs = w.zones.locs_in(engine::zone::zone_index(3223, 3223, 0));
    assert_eq!(locs.len(), 1);
    assert_eq!((locs[0].id, locs[0].shape, locs[0].angle), (1530, 0, 1));

    let out = &w.players[pid].as_ref().unwrap().out;
    let pkt = out.iter().find(|m| m.opcode == 154).expect("LOC_ADD_CHANGE");
    assert_eq!(pkt.body.len(), 4, "id(2) + shape/angle(1) + slot(1)");
}

#[test]
fn loc_change_replaces_same_shape_then_del() {
    let mut w = World::new();
    let pid = w.add_player("p".into(), 3222, 3222, 0).unwrap();
    w.add_loc(1530, 0, 1, 3223, 3223, 0);
    w.add_loc(1531, 0, 3, 3223, 3223, 0); // retype same tile+shape -> replace

    let locs = w.zones.locs_in(engine::zone::zone_index(3223, 3223, 0));
    assert_eq!(locs.len(), 1, "a change replaces, not appends");
    assert_eq!((locs[0].id, locs[0].angle), (1531, 3));

    w.players[pid].as_mut().unwrap().out.clear();
    assert!(w.del_loc(3223, 3223, 0, 0).is_some());
    assert!(w.zones.locs_in(engine::zone::zone_index(3223, 3223, 0)).is_empty());
    let out = &w.players[pid].as_ref().unwrap().out;
    assert!(out.iter().any(|m| m.opcode == 7), "LOC_DEL broadcast");
}

#[test]
fn owned_drop_is_private_then_reveals_then_despawns() {
    let mut w = World::new();
    let owner = w.add_player("owner".into(), 3222, 3222, 0).unwrap();
    let obs = w.add_player("obs".into(), 3224, 3224, 0).unwrap(); // nearby observer
    for p in [owner, obs] {
        w.players[p].as_mut().unwrap().out.clear();
    }

    w.drop_obj_owned(995, 100, 3223, 3223, 0, owner as i32);
    // Private: only the owner gets the immediate OBJ_ADD.
    assert!(w.players[owner].as_ref().unwrap().out.iter().any(|m| m.opcode == 173));
    assert!(w.players[obs].as_ref().unwrap().out.iter().all(|m| m.opcode != 173));

    // Run to the reveal tick — the observer now gets OBJ_REVEAL (215).
    for p in [owner, obs] {
        w.players[p].as_mut().unwrap().out.clear();
    }
    for _ in 0..engine::zone::OBJ_REVEAL_TICKS {
        w.cycle();
    }
    assert!(w.players[obs].as_ref().unwrap().out.iter().any(|m| m.opcode == 215),
        "obj goes public via OBJ_REVEAL");

    // Run to despawn — OBJ_DEL (207) + gone from the zone.
    for p in [owner, obs] {
        w.players[p].as_mut().unwrap().out.clear();
    }
    for _ in 0..(engine::zone::OBJ_DESPAWN_TICKS - engine::zone::OBJ_REVEAL_TICKS) {
        w.cycle();
    }
    assert!(w.players[owner].as_ref().unwrap().out.iter().any(|m| m.opcode == 207),
        "obj despawns via OBJ_DEL");
    assert!(w.zones.objs_in(engine::zone::zone_index(3223, 3223, 0)).is_empty());
}

#[test]
fn timed_loc_reverts_after_its_duration() {
    let mut w = World::new();
    let pid = w.add_player("p".into(), 3222, 3222, 0).unwrap();
    w.add_loc_timed(1530, 0, 1, 3223, 3223, 0, 5); // e.g. a door open for 5 ticks
    let zidx = engine::zone::zone_index(3223, 3223, 0);
    assert_eq!(w.zones.locs_in(zidx).len(), 1);

    w.players[pid].as_mut().unwrap().out.clear();
    for _ in 0..5 {
        w.cycle();
    }
    assert!(w.zones.locs_in(zidx).is_empty(), "loc reverted after its duration");
    assert!(w.players[pid].as_ref().unwrap().out.iter().any(|m| m.opcode == 7),
        "LOC_DEL broadcast on revert");
}

#[test]
fn obj_count_restacks_and_broadcasts() {
    use io::packet::Packet;
    let mut w = World::new();
    let pid = w.add_player("p".into(), 3222, 3222, 0).unwrap();
    w.drop_obj(995, 100, 3223, 3223, 0); // a public coin pile of 100
    w.players[pid].as_mut().unwrap().out.clear();

    assert!(w.change_obj_count(995, 3223, 3223, 0, 100, 250), "pile matched");
    // The stored pile now reads 250.
    let objs = w.zones.objs_in(engine::zone::zone_index(3223, 3223, 0));
    assert_eq!(objs[0].count, 250);
    // A wrong old-count finds nothing.
    assert!(!w.change_obj_count(995, 3223, 3223, 0, 100, 5), "stale old-count ignored");

    let out = &w.players[pid].as_ref().unwrap().out;
    let pkt = out.iter().find(|m| m.opcode == 106).expect("OBJ_COUNT");
    assert_eq!(pkt.body.len(), 7);
    let mut r = Packet::from_vec(pkt.body.clone());
    assert_eq!(r.g1(), ((3223 & 7) << 4) | (3223 & 7), "packed slot");
    assert_eq!(r.g2(), 995, "obj id");
    assert_eq!(r.g2(), 100, "old count");
    assert_eq!(r.g2(), 250, "new count");
}

#[test]
fn loc_anim_broadcasts_to_nearby_players() {
    use io::packet::Packet;
    let mut w = World::new();
    let pid = w.add_player("p".into(), 3222, 3222, 0).unwrap();
    w.players[pid].as_mut().unwrap().out.clear();

    w.loc_anim(456, 0, 1, 3224, 3223, 0); // play seq 456 on a shape-0/angle-1 loc

    let out = &w.players[pid].as_ref().unwrap().out;
    assert!(out.iter().any(|m| m.opcode == 89), "zone base prefix");
    let pkt = out.iter().find(|m| m.opcode == 6).expect("LOC_ANIM");
    assert_eq!(pkt.body.len(), 4);
    let mut r = Packet::from_vec(pkt.body.clone());
    assert_eq!(r.g2_alt2(), 456, "seq");
    assert_eq!(r.g1_alt2(), ((3224 & 7) << 4) | (3223 & 7), "packed slot");
    let sa = r.g1_alt3();
    assert_eq!((sa >> 2, sa & 0x3), (0, 1), "shape/angle");
}

#[test]
fn map_anim_broadcasts_a_tile_spotanim() {
    use io::packet::Packet;
    let mut w = World::new();
    let pid = w.add_player("p".into(), 3222, 3222, 0).unwrap();
    w.players[pid].as_mut().unwrap().out.clear();

    w.map_anim(120, 0, 30, 3223, 3224, 0); // spotanim 120, height 0, delay 30

    let out = &w.players[pid].as_ref().unwrap().out;
    assert!(out.iter().any(|m| m.opcode == 89), "zone base prefix");
    let pkt = out.iter().find(|m| m.opcode == 20).expect("MAP_ANIM");
    assert_eq!(pkt.body.len(), 6);
    let mut r = Packet::from_vec(pkt.body.clone());
    let slot = r.g1();
    assert_eq!(slot, ((3223 & 7) << 4) | (3224 & 7), "packed tile-in-zone");
    assert_eq!(r.g2(), 120, "spotanim");
    assert_eq!(r.g1(), 0, "height");
    assert_eq!(r.g2(), 30, "delay");
}

#[test]
fn map_projanim_broadcasts_a_projectile_with_signed_delta() {
    use io::packet::Packet;
    let mut w = World::new();
    let pid = w.add_player("p".into(), 3222, 3222, 0).unwrap();
    w.players[pid].as_mut().unwrap().out.clear();

    // Fire from (3225,3225) to (3222,3227): dx=-3, dz=+2; homing on npc target 7.
    w.map_projanim(220, 3225, 3225, 3222, 3227, 7, 40, 36, 5, 25, 16, 0, 0);

    let out = &w.players[pid].as_ref().unwrap().out;
    assert!(out.iter().any(|m| m.opcode == 89), "zone base prefix");
    let pkt = out.iter().find(|m| m.opcode == 32).expect("MAP_PROJANIM");
    assert_eq!(pkt.body.len(), 15);
    let mut r = Packet::from_vec(pkt.body.clone());
    assert_eq!(r.g1(), ((3225 & 7) << 4) | (3225 & 7), "packed source tile");
    assert_eq!(r.g1b() as i32, -3, "signed dstX delta");
    assert_eq!(r.g1b() as i32, 2, "signed dstZ delta");
    assert_eq!(r.g2b() as i32, 7, "homing target");
    assert_eq!(r.g2(), 220, "projectile spotanim");
    assert_eq!(r.g1(), 10, "srcHeight/4 (40/4)");
    assert_eq!(r.g1(), 9, "dstHeight/4 (36/4)");
    assert_eq!(r.g2(), 5, "start delay");
    assert_eq!(r.g2(), 25, "end delay");
    assert_eq!(r.g1(), 16, "peak");
    assert_eq!(r.g1(), 0, "arc");
}

#[test]
fn reorient_keeps_facing_a_moving_target() {
    let mut w = World::new();
    let a = w.add_player("a".into(), 3222, 3222, 0).unwrap();
    let b = w.add_npc(1, 3225, 3222, 0).unwrap(); // npc directly east of a

    // a interacts with npc b — hold it as the facing target.
    w.players[a].as_mut().unwrap().entity.set_face_entity(b as i32);
    w.cycle();
    assert_eq!(
        w.players[a].as_ref().unwrap().entity.orientation_dir(),
        4, // east
        "faces the npc to its east",
    );

    // The npc moves north of a; next tick a re-faces it (reorient).
    w.npcs[b].as_mut().unwrap().entity.teleport(3222, 3225, 0, false);
    w.cycle();
    assert_eq!(
        w.players[a].as_ref().unwrap().entity.orientation_dir(),
        1, // north
        "re-faces north once the target has moved",
    );
}

#[test]
fn removing_a_player_clears_zone_membership() {
    let mut w = World::new();
    let pid = w.add_player("gone".into(), 3300, 3300, 0).unwrap();
    let zone = engine::zone::zone_index(3300, 3300, 0);
    assert_eq!(w.zones.players_in(zone), &[pid]);
    w.remove_player(pid);
    assert!(w.zones.players_in(zone).is_empty());
}
