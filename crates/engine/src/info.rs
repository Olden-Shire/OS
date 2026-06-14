//! PLAYER_INFO (113) / NPC_INFO (167) builders — the server half of
//! the rev1 entity-update protocol. Bit layouts and alt-codec choices
//! match the client decode in crates/client/src/login.rs
//! (get_player_pos* / get_npc_pos*) byte-for-byte.

use io::packet::Packet;
use protocol::{ServerPacket, SizeKind};

use crate::entity::npc;
use crate::entity::player;
use crate::world::{World, MAX_PLAYERS, VIEW_DISTANCE};

/// Cap on new entities introduced per packet so a crowded zone
/// doesn't blow the packet budget in one tick.
const MAX_ADDS_PER_TICK: usize = 25;

fn within_view(ax: i32, az: i32, bx: i32, bz: i32) -> bool {
    (ax - bx).abs() <= VIEW_DISTANCE && (az - bz).abs() <= VIEW_DISTANCE
}

// ══════════════════════════════════════════════════════════════════
// PLAYER_INFO (113)
// ══════════════════════════════════════════════════════════════════

struct ExtendedEntry {
    pid: usize,
    force_appearance: bool,
}

pub fn build_player_info(world: &mut World, pid: usize) -> ServerPacket {
    // Take the observer's view state out so the rest of the player
    // array stays freely readable.
    let (mut view, mut seen) = {
        let p = world.players[pid].as_mut().expect("observer");
        (std::mem::take(&mut p.view_players), std::mem::take(&mut p.seen_appearance))
    };

    let me = world.players[pid].as_ref().expect("observer");
    let (my_x, my_z, my_level) = (me.entity.x, me.entity.z, me.entity.level);
    // Build-area origin of *this* observer — exact-move coords are scene-local.
    let (my_origin_x, my_origin_z) = (me.origin_x, me.origin_z);

    let mut buf = Packet::new(512);
    buf.bit_start();
    let mut extended: Vec<ExtendedEntry> = Vec::new();

    // ── Local block ───────────────────────────────────────────────
    {
        let needs_appearance = seen[pid] != me.appearance_seq;
        let flagged = me.entity.masks != 0 || needs_appearance;
        let e = &me.entity;

        if e.tele {
            buf.p_bit(1, 1);
            buf.p_bit(2, 3);
            buf.p_bit(2, e.level);
            buf.p_bit(7, me.local_x());
            buf.p_bit(1, i32::from(flagged));
            buf.p_bit(7, me.local_z());
            buf.p_bit(1, i32::from(e.jump));
        } else if e.run_dir != -1 {
            buf.p_bit(1, 1);
            buf.p_bit(2, 2);
            buf.p_bit(3, e.walk_dir);
            buf.p_bit(3, e.run_dir);
            buf.p_bit(1, i32::from(flagged));
        } else if e.walk_dir != -1 {
            buf.p_bit(1, 1);
            buf.p_bit(2, 1);
            buf.p_bit(3, e.walk_dir);
            buf.p_bit(1, i32::from(flagged));
        } else if flagged {
            buf.p_bit(1, 1);
            buf.p_bit(2, 0);
        } else {
            buf.p_bit(1, 0);
        }

        if flagged {
            extended.push(ExtendedEntry { pid, force_appearance: needs_appearance });
        }
    }

    // ── Old vis ───────────────────────────────────────────────────
    buf.p_bit(8, view.len() as i32);
    let mut kept: Vec<usize> = Vec::with_capacity(view.len());
    for &tid in view.iter() {
        let target = world.players[tid].as_ref();
        let keep = target.map_or(false, |t| {
            !t.entity.tele
                && t.entity.level == my_level
                && within_view(t.entity.x, t.entity.z, my_x, my_z)
        });
        if !keep {
            // Remove — teleported targets re-enter via new vis below.
            buf.p_bit(1, 1);
            buf.p_bit(2, 3);
            continue;
        }
        let t = target.unwrap();
        let needs_appearance = seen[tid] != t.appearance_seq;
        let flagged = t.entity.masks != 0 || needs_appearance;
        let e = &t.entity;

        if e.run_dir != -1 {
            buf.p_bit(1, 1);
            buf.p_bit(2, 2);
            buf.p_bit(3, e.walk_dir);
            buf.p_bit(3, e.run_dir);
            buf.p_bit(1, i32::from(flagged));
        } else if e.walk_dir != -1 {
            buf.p_bit(1, 1);
            buf.p_bit(2, 1);
            buf.p_bit(3, e.walk_dir);
            buf.p_bit(1, i32::from(flagged));
        } else if flagged {
            buf.p_bit(1, 1);
            buf.p_bit(2, 0);
        } else {
            buf.p_bit(1, 0);
        }

        if flagged {
            extended.push(ExtendedEntry { pid: tid, force_appearance: needs_appearance });
        }
        kept.push(tid);
    }
    view = kept;

    // ── New vis ───────────────────────────────────────────────────
    // Candidates come from the build-area zones around the observer, not a
    // full 2047-slot scan; `within_view` still narrows to the exact range.
    let mut adds = 0usize;
    for tid in world.nearby_player_ids(my_x, my_z, my_level) {
        if adds >= MAX_ADDS_PER_TICK || view.len() >= 255 {
            break;
        }
        if tid == pid || tid >= MAX_PLAYERS - 1 || view.contains(&tid) {
            continue;
        }
        let Some(t) = world.players[tid].as_ref() else { continue; };
        if t.entity.level != my_level || !within_view(t.entity.x, t.entity.z, my_x, my_z) {
            continue;
        }

        buf.p_bit(11, tid as i32);
        // Client read order: dz(5), dir(3), dx(5), jump(1), flag(1).
        buf.p_bit(5, (t.entity.z - my_z) & 0x1F);
        buf.p_bit(3, t.entity.orientation_dir()); // current facing (ANGLE_TO_DIR idx)
        buf.p_bit(5, (t.entity.x - my_x) & 0x1F);
        buf.p_bit(1, 1); // snap (no walk interpolation on entry)
        buf.p_bit(1, 1); // extended follows — appearance at minimum

        extended.push(ExtendedEntry { pid: tid, force_appearance: true });
        view.push(tid);
        adds += 1;
    }
    buf.p_bit(11, 2047);
    buf.bit_end();

    // ── Extended info ─────────────────────────────────────────────
    for entry in &extended {
        let t = world.players[entry.pid].as_ref().expect("extended target");
        write_player_extended(&mut buf, t, entry.force_appearance, my_origin_x, my_origin_z);
        if entry.force_appearance || (t.entity.masks & player::MASK_APPEARANCE) != 0 {
            seen[entry.pid] = t.appearance_seq;
        }
    }

    // Restore observer view state.
    let p = world.players[pid].as_mut().expect("observer");
    p.view_players = view;
    p.seen_appearance = seen;

    let mut body = buf.data;
    body.truncate(buf.pos);
    ServerPacket { opcode: 113, size: SizeKind::Var2, body }
}

fn write_player_extended(
    buf: &mut Packet,
    t: &player::Player,
    force_appearance: bool,
    obs_origin_x: i32,
    obs_origin_z: i32,
) {
    let e = &t.entity;
    let mut flags = e.masks;
    if force_appearance {
        flags |= player::MASK_APPEARANCE;
    }
    // The builders never queue public chat without text etc., so the
    // mask bits drive the payload writes 1:1.
    if flags > 0xff {
        flags |= player::MASK_BIG;
        buf.p1(flags & 0xff);
        buf.p1(flags >> 8);
    } else {
        buf.p1(flags);
    }

    // Client mask processing order — getPlayerPosExtended.
    if (flags & player::MASK_PUBLIC_CHAT) != 0 {
        // getPlayerPosExtended reads: g2(colour<<8 | effect), g1(rights),
        // g1(length), then the WordPack-packed message bytes.
        buf.p2((t.chat_colour << 8) | (t.chat_effect & 0xff));
        buf.p1(t.chat_rights);
        buf.p1(t.chat_message.len() as i32);
        buf.pdata(&t.chat_message, 0, t.chat_message.len());
    }

    if (flags & player::MASK_APPEARANCE) != 0 {
        let app = t.appearance_bytes();
        buf.p1_alt3(app.len() as i32);
        buf.pdata_alt1(&app);
    }

    if (flags & player::MASK_EXACT_MOVE) != 0 {
        // Scene-local tile coords (absolute − observer build-area origin), then
        // the end/start cycle deltas and the facing — getPlayerPosExtended order.
        buf.p1(e.exact_start_x - obs_origin_x);
        buf.p1_alt2(e.exact_start_z - obs_origin_z);
        buf.p1(e.exact_end_x - obs_origin_x);
        buf.p1_alt1(e.exact_end_z - obs_origin_z);
        buf.p2_alt2(e.exact_move_end);
        buf.p2(e.exact_move_start);
        buf.p1_alt2(e.exact_move_facing);
    }

    if (flags & player::MASK_FACE_ENTITY) != 0 {
        buf.p2_alt3(e.face_entity);
    }

    if (flags & player::MASK_FACE_COORD) != 0 {
        // Half-tile units: tile*2+1 centres the facing on the tile.
        buf.p2_alt2(e.face_x * 2 + 1);
        buf.p2_alt1(e.face_z * 2 + 1);
    }

    if (flags & player::MASK_ANIM) != 0 {
        buf.p2_alt2(e.anim_id);
        buf.p1_alt2(e.anim_delay);
    }

    if (flags & player::MASK_SPOTANIM) != 0 {
        buf.p2_alt1(e.spotanim_id);
        buf.p4((e.spotanim_height << 16) | (e.spotanim_delay & 0xffff));
    }

    if (flags & player::MASK_DAMAGE) != 0 {
        buf.p1_alt1(e.damage_taken);
        buf.p1_alt3(e.damage_type);
        buf.p1(t.levels[player::STAT_HITPOINTS]);
        buf.p1_alt2(t.base_levels[player::STAT_HITPOINTS]);
    }

    if (flags & player::MASK_SAY) != 0 {
        buf.pjstr(e.chat.as_deref().unwrap_or(""));
    }

    if (flags & player::MASK_DAMAGE2) != 0 {
        buf.p1_alt1(e.damage_taken2);
        buf.p1_alt3(e.damage_type2);
        buf.p1_alt1(t.levels[player::STAT_HITPOINTS]);
        buf.p1(t.base_levels[player::STAT_HITPOINTS]);
    }
}

// ══════════════════════════════════════════════════════════════════
// NPC_INFO (167)
// ══════════════════════════════════════════════════════════════════

pub fn build_npc_info(world: &mut World, pid: usize) -> ServerPacket {
    let mut view = {
        let p = world.players[pid].as_mut().expect("observer");
        std::mem::take(&mut p.view_npcs)
    };

    let me = world.players[pid].as_ref().expect("observer");
    let (my_x, my_z, my_level) = (me.entity.x, me.entity.z, me.entity.level);

    let mut buf = Packet::new(512);
    buf.bit_start();
    let mut extended: Vec<usize> = Vec::new();

    // ── Old vis ───────────────────────────────────────────────────
    buf.p_bit(8, view.len() as i32);
    let mut kept: Vec<usize> = Vec::with_capacity(view.len());
    for &nid in view.iter() {
        let target = world.npcs[nid].as_ref();
        let keep = target.map_or(false, |n| {
            n.active
                && !n.entity.tele
                && n.entity.level == my_level
                && within_view(n.entity.x, n.entity.z, my_x, my_z)
        });
        if !keep {
            buf.p_bit(1, 1);
            buf.p_bit(2, 3);
            continue;
        }
        let n = target.unwrap();
        let flagged = n.entity.masks != 0;
        let e = &n.entity;

        if e.run_dir != -1 {
            buf.p_bit(1, 1);
            buf.p_bit(2, 2);
            buf.p_bit(3, e.walk_dir);
            buf.p_bit(3, e.run_dir);
            buf.p_bit(1, i32::from(flagged));
        } else if e.walk_dir != -1 {
            buf.p_bit(1, 1);
            buf.p_bit(2, 1);
            buf.p_bit(3, e.walk_dir);
            buf.p_bit(1, i32::from(flagged));
        } else if flagged {
            buf.p_bit(1, 1);
            buf.p_bit(2, 0);
        } else {
            buf.p_bit(1, 0);
        }

        if flagged {
            extended.push(nid);
        }
        kept.push(nid);
    }
    view = kept;

    // ── New vis ───────────────────────────────────────────────────
    // Candidates from the build-area zones, not a full NPC-slot scan.
    let mut adds = 0usize;
    for nid in world.nearby_npc_ids(my_x, my_z, my_level) {
        if adds >= MAX_ADDS_PER_TICK || view.len() >= 255 {
            break;
        }
        if view.contains(&nid) {
            continue;
        }
        let Some(n) = world.npcs[nid].as_ref() else { continue; };
        if !n.active || n.entity.level != my_level
            || !within_view(n.entity.x, n.entity.z, my_x, my_z)
        {
            continue;
        }

        let flagged = n.entity.masks != 0;
        buf.p_bit(15, nid as i32);
        // Client read order: dir(3), dz(5), flag(1), jump(1),
        // type(14), dx(5).
        buf.p_bit(3, n.entity.orientation_dir()); // current facing (ANGLE_TO_DIR idx)
        buf.p_bit(5, (n.entity.z - my_z) & 0x1F);
        buf.p_bit(1, i32::from(flagged));
        buf.p_bit(1, 1); // snap
        buf.p_bit(14, n.type_id);
        buf.p_bit(5, (n.entity.x - my_x) & 0x1F);

        if flagged {
            extended.push(nid);
        }
        view.push(nid);
        adds += 1;
    }
    // 15-bit `32767` terminator — needed iff an extended-info block follows.
    // The client's new-vis loop reads adds while `bitsLeft >= 27`; the byte-
    // aligned extended block leaves >= 27 bits after the last add, so without a
    // terminator the client reads the extended bytes as phantom npcs (java then
    // eager-loads a garbage npc model → "Invalid GZIP header"; rust desyncs the
    // extended read so real npcs never render). When NO extended block follows,
    // the packet ends and the loop stops naturally on <27 bits — writing a
    // terminator there would desync `pos != psize` (matches the player-info
    // encoder, which always has appearance extended so writes it unconditionally).
    if !extended.is_empty() {
        buf.p_bit(15, 32767);
    }
    buf.bit_end();

    // ── Extended info ─────────────────────────────────────────────
    for &nid in &extended {
        let n = world.npcs[nid].as_ref().expect("extended npc");
        write_npc_extended(&mut buf, n);
    }

    let p = world.players[pid].as_mut().expect("observer");
    p.view_npcs = view;

    let mut body = buf.data;
    body.truncate(buf.pos);
    ServerPacket { opcode: 167, size: SizeKind::Var2, body }
}

fn write_npc_extended(buf: &mut Packet, n: &npc::Npc) {
    let e = &n.entity;
    let flags = e.masks;
    buf.p1(flags);

    // Client mask processing order — getNpcPosExtended.
    if (flags & npc::MASK_DAMAGE) != 0 {
        buf.p1(e.damage_taken);
        buf.p1_alt2(e.damage_type);
        buf.p1_alt1(n.levels[npc::NPC_STAT_HITPOINTS]);
        buf.p1_alt1(n.base_levels[npc::NPC_STAT_HITPOINTS]);
    }

    if (flags & npc::MASK_FACE_ENTITY) != 0 {
        buf.p2_alt1(e.face_entity);
    }

    if (flags & npc::MASK_FACE_COORD) != 0 {
        buf.p2_alt3(e.face_x * 2 + 1);
        buf.p2_alt3(e.face_z * 2 + 1);
    }

    if (flags & npc::MASK_SPOTANIM) != 0 {
        buf.p2_alt1(e.spotanim_id);
        buf.p4((e.spotanim_height << 16) | (e.spotanim_delay & 0xffff));
    }

    if (flags & npc::MASK_ANIM) != 0 {
        buf.p2_alt3(e.anim_id);
        buf.p1_alt1(e.anim_delay);
    }

    if (flags & npc::MASK_CHANGE_TYPE) != 0 {
        buf.p2(n.new_type);
    }

    if (flags & npc::MASK_SAY) != 0 {
        buf.pjstr(e.chat.as_deref().unwrap_or(""));
    }

    if (flags & npc::MASK_DAMAGE2) != 0 {
        buf.p1_alt3(e.damage_taken2);
        buf.p1_alt3(e.damage_type2);
        buf.p1_alt3(n.levels[npc::NPC_STAT_HITPOINTS]);
        buf.p1_alt1(n.base_levels[npc::NPC_STAT_HITPOINTS]);
    }
}
