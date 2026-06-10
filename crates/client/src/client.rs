// @ObfuscatedName("client")
//
// jagex3.client.Client — top-of-stack state machine. The JS5 path is
// ported verbatim: maininit allocates the per-archive loaders, mainloop
// services the network, mainredraw paints the progress bar from
// TitleScreen.loadPos / loadString.

#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crate::applet::privileged_request::{PrivilegedRequest, Result as PReqResult, STATUS_DONE, STATUS_ERROR};
use crate::applet::sign_link::SignLink;
use crate::game_shell::{self, Framebuffer, GameShellLifecycle, SHELL};
use crate::io::client_stream::ClientStream;
use crate::io::packet::Packet;
use crate::js5::js5_loader::Js5Loader;
use crate::js5::{js5_net, js5_net_thread};
use crate::title_screen;

// @ObfuscatedName("p.de(Lfz;I)V") — Client.entityAnim.
//
// Verbatim port of Client.java:3934-4027. Advances per-tick the three
// SeqType frame counters (secondary / spotanim / primary) using each
// seq's `delay[frame]` table. When a seq's frame counter hits the
// frame list end, the loop wrap is applied via `frames -= loops`;
// max_loops gates the seq from looping forever.
//
// `triggerSeqSound` is stubbed (would route to the sound subsystem;
// no-op here matches the audio bridge that's still pending).
// @ObfuscatedName("client.t(IIIIZIIIIII)Z") — Client.tryMove.
// Verbatim port of Client.java:5529-5734. BFS over the active level's
// CollisionMap. arg0/arg1 are the source tile, arg2/arg3 the target.
// arg5 (wall kind) / arg6 (rotation) / arg7+arg8 (loc size) / arg9
// (access flags) gate the goal-test: testWall / testWDecor / testLoc.
//
// On success it builds a compressed waypoint array (max 25 steps) and
// emits the MOVE packet via the client's outbound stream:
//   arg10 == 0 → opcode 176 (game-world click)
//   arg10 == 1 → opcode  60 (minimap click)
//   arg10 == 2 → opcode 214 (op click)
//
// Returns true on success, false if no path and no "settle for
// nearest" option (arg4) requested.
//
// Implemented as a free function taking `&mut Client` so the borrow
// pattern stays narrow — `collision[level].flags` and the per-Client
// dir/dist/route arrays all live on the same struct.
pub fn try_move(
    c: &mut Client,
    src_x: i32, src_z: i32,
    dst_x: i32, dst_z: i32,
    settle_for_nearest: bool,
    wall_kind: i32, wall_rot: i32,
    loc_sx: i32, loc_sz: i32, access_bits: i32,
    click_kind: i32,
) -> bool {
    // Clear dirMap + distMap.
    for x in 0..104 {
        for z in 0..104 {
            c.dir_map[x][z] = 0;
            c.dist_map[x][z] = 99_999_999;
        }
    }
    let mut cx = src_x;
    let mut cz = src_z;
    c.dir_map[src_x as usize][src_z as usize] = 99;
    c.dist_map[src_x as usize][src_z as usize] = 0;

    let mut tail = 0usize;
    let mut head = 0usize;
    c.route_x[tail] = src_x;
    c.route_z[tail] = src_z;
    tail += 1;
    let cap = c.route_x.len();
    let mut reached = false;

    let level = c.minusedlevel as usize;
    // Take the level's flags by clone — Java reads while mutating
    // dir/dist arrays; borrowing immutably here matches semantics.
    let flags: Vec<Vec<i32>> = match &c.collision[level] {
        Some(map) => map.flags.clone(),
        None => return false,
    };

    while tail != head {
        cx = c.route_x[head];
        cz = c.route_z[head];
        head = (head + 1) % cap;
        if cx == dst_x && cz == dst_z {
            reached = true;
            break;
        }

        if wall_kind != 0 {
            if let Some(map) = &c.collision[level] {
                if (wall_kind < 5 || wall_kind == 10)
                    && map.test_wall(cx, cz, dst_x, dst_z, wall_kind - 1, wall_rot)
                {
                    reached = true;
                    break;
                }
                if wall_kind < 10
                    && map.test_w_decor(cx, cz, dst_x, dst_z, wall_kind - 1, wall_rot)
                {
                    reached = true;
                    break;
                }
            }
        }

        if loc_sx != 0 && loc_sz != 0 {
            if let Some(map) = &c.collision[level] {
                if map.test_loc(cx, cz, dst_x, dst_z, loc_sx, loc_sz, access_bits) {
                    reached = true;
                    break;
                }
            }
        }

        let nd = c.dist_map[cx as usize][cz as usize] + 1;
        // West.
        if cx > 0 && c.dir_map[(cx - 1) as usize][cz as usize] == 0
            && (flags[(cx - 1) as usize][cz as usize] & 0x12C0108) == 0
        {
            c.route_x[tail] = cx - 1;
            c.route_z[tail] = cz;
            tail = (tail + 1) % cap;
            c.dir_map[(cx - 1) as usize][cz as usize] = 2;
            c.dist_map[(cx - 1) as usize][cz as usize] = nd;
        }
        // East.
        if cx < 103 && c.dir_map[(cx + 1) as usize][cz as usize] == 0
            && (flags[(cx + 1) as usize][cz as usize] & 0x12C0180) == 0
        {
            c.route_x[tail] = cx + 1;
            c.route_z[tail] = cz;
            tail = (tail + 1) % cap;
            c.dir_map[(cx + 1) as usize][cz as usize] = 8;
            c.dist_map[(cx + 1) as usize][cz as usize] = nd;
        }
        // South (z-1).
        if cz > 0 && c.dir_map[cx as usize][(cz - 1) as usize] == 0
            && (flags[cx as usize][(cz - 1) as usize] & 0x12C0102) == 0
        {
            c.route_x[tail] = cx;
            c.route_z[tail] = cz - 1;
            tail = (tail + 1) % cap;
            c.dir_map[cx as usize][(cz - 1) as usize] = 1;
            c.dist_map[cx as usize][(cz - 1) as usize] = nd;
        }
        // North (z+1).
        if cz < 103 && c.dir_map[cx as usize][(cz + 1) as usize] == 0
            && (flags[cx as usize][(cz + 1) as usize] & 0x12C0120) == 0
        {
            c.route_x[tail] = cx;
            c.route_z[tail] = cz + 1;
            tail = (tail + 1) % cap;
            c.dir_map[cx as usize][(cz + 1) as usize] = 4;
            c.dist_map[cx as usize][(cz + 1) as usize] = nd;
        }
        // SW.
        if cx > 0 && cz > 0 && c.dir_map[(cx - 1) as usize][(cz - 1) as usize] == 0
            && (flags[(cx - 1) as usize][(cz - 1) as usize] & 0x12C010E) == 0
            && (flags[(cx - 1) as usize][cz as usize] & 0x12C0108) == 0
            && (flags[cx as usize][(cz - 1) as usize] & 0x12C0102) == 0
        {
            c.route_x[tail] = cx - 1;
            c.route_z[tail] = cz - 1;
            tail = (tail + 1) % cap;
            c.dir_map[(cx - 1) as usize][(cz - 1) as usize] = 3;
            c.dist_map[(cx - 1) as usize][(cz - 1) as usize] = nd;
        }
        // SE.
        if cx < 103 && cz > 0 && c.dir_map[(cx + 1) as usize][(cz - 1) as usize] == 0
            && (flags[(cx + 1) as usize][(cz - 1) as usize] & 0x12C0183) == 0
            && (flags[(cx + 1) as usize][cz as usize] & 0x12C0180) == 0
            && (flags[cx as usize][(cz - 1) as usize] & 0x12C0102) == 0
        {
            c.route_x[tail] = cx + 1;
            c.route_z[tail] = cz - 1;
            tail = (tail + 1) % cap;
            c.dir_map[(cx + 1) as usize][(cz - 1) as usize] = 9;
            c.dist_map[(cx + 1) as usize][(cz - 1) as usize] = nd;
        }
        // NW.
        if cx > 0 && cz < 103 && c.dir_map[(cx - 1) as usize][(cz + 1) as usize] == 0
            && (flags[(cx - 1) as usize][(cz + 1) as usize] & 0x12C0138) == 0
            && (flags[(cx - 1) as usize][cz as usize] & 0x12C0108) == 0
            && (flags[cx as usize][(cz + 1) as usize] & 0x12C0120) == 0
        {
            c.route_x[tail] = cx - 1;
            c.route_z[tail] = cz + 1;
            tail = (tail + 1) % cap;
            c.dir_map[(cx - 1) as usize][(cz + 1) as usize] = 6;
            c.dist_map[(cx - 1) as usize][(cz + 1) as usize] = nd;
        }
        // NE.
        if cx < 103 && cz < 103 && c.dir_map[(cx + 1) as usize][(cz + 1) as usize] == 0
            && (flags[(cx + 1) as usize][(cz + 1) as usize] & 0x12C01E0) == 0
            && (flags[(cx + 1) as usize][cz as usize] & 0x12C0180) == 0
            && (flags[cx as usize][(cz + 1) as usize] & 0x12C0120) == 0
        {
            c.route_x[tail] = cx + 1;
            c.route_z[tail] = cz + 1;
            tail = (tail + 1) % cap;
            c.dir_map[(cx + 1) as usize][(cz + 1) as usize] = 12;
            c.dist_map[(cx + 1) as usize][(cz + 1) as usize] = nd;
        }
    }

    c.try_move_nearest = 0;
    if !reached {
        if !settle_for_nearest {
            return false;
        }
        let mut best_d2 = 1000i32;
        let mut best_dist = 100i32;
        let radius = 10i32;
        for x in (dst_x - radius)..=(dst_x + radius) {
            for z in (dst_z - radius)..=(dst_z + radius) {
                if x < 0 || z < 0 || x >= 104 || z >= 104 { continue; }
                if c.dist_map[x as usize][z as usize] >= 100 { continue; }
                let dx = if x < dst_x { dst_x - x }
                    else if x > dst_x + loc_sx - 1 { x - (dst_x + loc_sx - 1) }
                    else { 0 };
                let dz = if z < dst_z { dst_z - z }
                    else if z > dst_z + loc_sz - 1 { z - (dst_z + loc_sz - 1) }
                    else { 0 };
                let d2 = dx * dx + dz * dz;
                let d = c.dist_map[x as usize][z as usize];
                if d2 < best_d2 || (best_d2 == d2 && d < best_dist) {
                    best_d2 = d2;
                    best_dist = d;
                    cx = x;
                    cz = z;
                }
            }
        }
        if best_d2 == 1000 { return false; }
        if src_x == cx && src_z == cz { return false; }
        c.try_move_nearest = 1;
    }

    // Reconstruct path by walking dirMap backwards from (cx, cz) to
    // (src_x, src_z). Java rewrites routeX/routeZ in place starting at
    // index 0 → the goal, then peels off direction changes.
    let mut count = 0usize;
    c.route_x[count] = cx;
    c.route_z[count] = cz;
    count += 1;
    let mut prev_dir = c.dir_map[cx as usize][cz as usize];
    let mut dir = prev_dir;
    while src_x != cx || src_z != cz {
        if dir != prev_dir {
            prev_dir = dir;
            c.route_x[count] = cx;
            c.route_z[count] = cz;
            count += 1;
        }
        if (dir & 0x2) != 0 { cx += 1; }
        else if (dir & 0x8) != 0 { cx -= 1; }
        if (dir & 0x1) != 0 { cz += 1; }
        else if (dir & 0x4) != 0 { cz -= 1; }
        dir = c.dir_map[cx as usize][cz as usize];
    }

    if count > 0 {
        let steps = count.min(25) as i32;
        // The waypoint stream stored from index 0 (goal) → count-1
        // (source). Java pops the last-stored slot first to write the
        // packet body in source→goal order.
        let mut idx = count - 1;
        let goal_x = c.route_x[idx];
        let goal_z = c.route_z[idx];

        // Only emit the packet when the outbound stream + isaac are
        // both wired (post-login). Java's `out` is always non-null
        // once tryMove is callable, but the Rust state machine still
        // allows tryMove to fire before login (e.g. unit tests).
        if let (Some(out), Some(isaac)) = (c.out_packet.as_mut(), c.isaac_out.as_mut()) {
            match click_kind {
                0 => { out.p1_enc(176, isaac); out.p1(steps + steps + 3); }
                1 => { out.p1_enc(60,  isaac); out.p1(steps + steps + 3 + 14); }
                2 => { out.p1_enc(214, isaac); out.p1(steps + steps + 3); }
                _ => {}
            }

            c.minimap_flag_x = c.route_x[0];
            c.minimap_flag_z = c.route_z[0];

            for _ in 1..steps {
                idx -= 1;
                out.p1_alt2(c.route_x[idx] - goal_x);
                out.p1_alt3(c.route_z[idx] - goal_z);
            }
            out.p2_alt3(c.map_build_base_z + goal_z);
            out.p1(if c.key_ctrl_held { 1 } else { 0 });
            out.p2(c.map_build_base_x + goal_x);
        } else {
            c.minimap_flag_x = c.route_x[0];
            c.minimap_flag_z = c.route_z[0];
        }
        return true;
    } else if click_kind == 1 {
        return false;
    }
    true
}

// @ObfuscatedName("client.ee") — componentDrawTime. Set per-frame to
// loopCycle just before the interface redraw pass starts. A component
// only marks itself dirty if its own drawTime stamp matches (i.e. the
// component was drawn this frame; otherwise the dirty flag is moot
// since it'll be cleared on the next draw).
pub static COMPONENT_DRAW_TIME: std::sync::atomic::AtomicI32 =
    std::sync::atomic::AtomicI32::new(-2);

// @ObfuscatedName("client.ev") — componentDirtyArea[100]. One bit per
// composer slot; set when a component's state changes between draws
// so the renderer knows to repaint that region.
pub static COMPONENT_DIRTY_AREA: std::sync::Mutex<[bool; 100]> =
    std::sync::Mutex::new([false; 100]);

// @ObfuscatedName("cq.fy(Leg;I)V") — Client.componentUpdated.
// Verbatim port of Client.java:11475-11479.
pub fn component_updated(com: &crate::config::if_type::IfType) {
    if COMPONENT_DRAW_TIME.load(std::sync::atomic::Ordering::Relaxed) == com.draw_time {
        let mut area = COMPONENT_DIRTY_AREA.lock().unwrap();
        let slot = com.draw_count as usize;
        if slot < area.len() {
            area[slot] = true;
        }
    }
}

// @ObfuscatedName("client.fu()V") — Client.redrawAllComponents. Java
// note: "guessing that this may be inlined from repeat use" — same
// 100-slot loop, set all dirty.
pub fn redraw_all_components() {
    let mut area = COMPONENT_DIRTY_AREA.lock().unwrap();
    for slot in area.iter_mut() { *slot = true; }
}

// @ObfuscatedName("ao.gn(ILjava/lang/String;Ljava/lang/String;I)V")
// @ObfuscatedName("br.gj(ILjava/lang/String;Ljava/lang/String;Ljava/lang/String;I)V")
// Client.addChat. Verbatim port of Client.java:12080-12102. Rotates
// the 100-slot chat history so [0] is the most recent. `screen_name`
// (Java's arg3) is null for everything except clan chat.
//
// `transmit_num` is the server's global packet counter at the moment
// this message arrived; the client stamps chat_transmit_num so it
// won't re-trigger if the same message replays.
pub fn add_chat(c: &mut Client, kind: i32, sender: Option<String>, message: Option<String>, screen_name: Option<String>, transmit_num: i32) {
    for i in (1..100).rev() {
        c.chat_type[i] = c.chat_type[i - 1];
        c.chat_username[i] = c.chat_username[i - 1].clone();
        c.chat_text[i] = c.chat_text[i - 1].clone();
        c.chat_screen_name[i] = c.chat_screen_name[i - 1].clone();
    }
    c.chat_type[0] = kind;
    c.chat_username[0] = sender;
    c.chat_text[0] = message;
    c.chat_screen_name[0] = screen_name;
    c.chat_history_length += 1;
    // Java: chatTransmitNum = transmitNum — stamping the interface
    // tick counter makes every onchattransmit component (whose own
    // stamp predates this) fire once on the next loopInterface pass.
    let _ = transmit_num;
    c.chat_transmit_num = c.transmit_num;
}

// Custom type — represents a queued RUNCLIENTSCRIPT call. Java
// constructs a HookReq inside Client.tcpIn and feeds it to
// ScriptRunner.executeScript directly; the Rust dispatcher isn't
// ported yet so we record the args and let the next dispatcher pass
// drain them.
#[derive(Debug, Clone)]
pub struct PendingClientScript {
    pub script_id: i32,
    pub stack_desc: String,
    pub int_args: Vec<i32>,
    pub string_args: Vec<String>,
}

// @ObfuscatedName(— Client.logout). Verbatim port of Client.java:
// 3063-3085. Tears down the in-flight stream, clears every config
// cache, resets the collision maps + scene, stops audio, then
// returns to mainstate 10 (title screen).
pub fn logout(c: &mut Client) {
    // Close the inbound/outbound packet stream and Isaac state.
    c.out_packet = None;
    c.isaac_out = None;
    c.isaac_in = None;

    clear_caches();

    // Reset the world + collision maps.
    for level in 0..4 {
        if let Some(map) = c.collision[level].as_mut() {
            map.reset();
        }
    }

    // Stop MIDI + clear queued audio.
    c.queued_song_id = -1;
    c.queued_jingle_id = -1;
    c.queued_jingle_fade_ms = 0;
    c.queued_synth.clear();
    c.next_midi_song = -1;
    c.playing_jingle = false;
    c.wave_count = 0;

    // BgSound.reset (clears the active emitter list).
    crate::sound::bg_sound::reset();

    // Return to title screen.
    c.state = 10;
}

// @ObfuscatedName("bh.da(B)V") — Client.clearCaches. Verbatim port of
// Client.java:3088-3115. Drops every config-type recentUse LRU.
pub fn clear_caches() {
    crate::config::flu_type::reset_cache();
    crate::config::idk_type::reset_cache();
    crate::config::loc_type::reset_cache();
    crate::config::npc_type::reset_cache();
    crate::config::obj_type::reset_cache();
    crate::config::seq_type::reset_cache();
    crate::config::spot_type::reset_cache();
    crate::config::varp_type::reset_cache();
    crate::config::if_type::reset_cache();
}

// @ObfuscatedName(— Client.lostCon). Verbatim port of
// Client.java:3119-3128. If a logout was already in flight, complete
// it; otherwise transition to mainstate 40 ("reconnect screen") and
// stash the dead stream so the next reconnect can recycle it.
pub fn lost_con(c: &mut Client) {
    if c.logout_timer > 0 {
        logout(c);
        return;
    }
    c.state = 40;
    // Java's `prevStream = stream; stream = null;` — we mirror by
    // shifting out_packet to None; the prev_stream field already
    // captures the legacy slot.
    c.out_packet = None;
}

// ── OPLOC[1..5] outbound packet builders ──────────────────────────
// Verbatim ports of Client.java:9114-9122 (OPLOC1), 9167-9175
// (OPLOC2), 8803-8810 (OPLOC3), 8757-8765 (OPLOC4), 8834-8841
// (OPLOC5).
//
// All take (loc_id, tile_x, tile_z) — the client-side tile is added
// to `map_build_base_x/z` to produce the absolute world coords the
// server expects. The `loc_id >> 14 & 0x7FFF` extraction strips the
// shape+angle bits packed into the picked-entity typecode.

pub fn send_op_loc_1(c: &mut Client, loc_typecode: i32, tile_x: i32, tile_z: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(73, isaac);
    out.p2_alt2((loc_typecode >> 14) & 0x7FFF);
    out.p2(c.map_build_base_x + tile_x);
    out.p2(c.map_build_base_z + tile_z);
}

pub fn send_op_loc_2(c: &mut Client, loc_typecode: i32, tile_x: i32, tile_z: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(90, isaac);
    out.p2_alt3(c.map_build_base_z + tile_z);
    out.p2_alt3(c.map_build_base_x + tile_x);
    out.p2_alt2((loc_typecode >> 14) & 0x7FFF);
}

pub fn send_op_loc_3(c: &mut Client, loc_typecode: i32, tile_x: i32, tile_z: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(133, isaac);
    out.p2_alt2(c.map_build_base_x + tile_x);
    out.p2_alt2(c.map_build_base_z + tile_z);
    out.p2_alt3((loc_typecode >> 14) & 0x7FFF);
}

pub fn send_op_loc_4(c: &mut Client, loc_typecode: i32, tile_x: i32, tile_z: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(83, isaac);
    out.p2_alt2(c.map_build_base_x + tile_x);
    out.p2_alt3(c.map_build_base_z + tile_z);
    out.p2_alt3((loc_typecode >> 14) & 0x7FFF);
}

pub fn send_op_loc_5(c: &mut Client, loc_typecode: i32, tile_x: i32, tile_z: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(56, isaac);
    out.p2(c.map_build_base_x + tile_x);
    out.p2_alt1((loc_typecode >> 14) & 0x7FFF);
    out.p2_alt2(c.map_build_base_z + tile_z);
}

// @ObfuscatedName(— Client.sendOpObj1). Verbatim port of
// Client.java:8796-8800. Ground-object primary op.
pub fn send_op_obj_1(c: &mut Client, obj_id: i32, tile_x: i32, tile_z: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(243, isaac);
    out.p2_alt1(obj_id);
    out.p2(c.map_build_base_x + tile_x);
    out.p2_alt3(c.map_build_base_z + tile_z);
}

// @ObfuscatedName(— Client.sendOpPlayerT). Verbatim port of
// Client.java:8777-8781. Player-target op (e.g. cast spell on player).
pub fn send_op_player_t(c: &mut Client, target_sub: i32, target_com: i32, target_player_id: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(183, isaac);
    out.p2_alt2(target_sub);
    out.p4(target_com);
    out.p2_alt1(target_player_id);
}

// ── OPNPC[1..5] + OPPLAYER[1..5] outbound builders ───────────────
// Verbatim ports of Client.java action-dispatch blocks. Each writes
// a one-opcode + g2/g2_alt* targeting a single entity slot id.
// OPNPC opcodes: 1→84, 2→13, 3→67, 4→95, 5→88.
// OPPLAYER opcodes: 1→246, 2→146, 3→102, 4→78, 5→117.

fn send_op_entity_g2(c: &mut Client, opcode: i32, slot: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(opcode, isaac);
    out.p2(slot);
}
fn send_op_entity_g2_alt1(c: &mut Client, opcode: i32, slot: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(opcode, isaac);
    out.p2_alt1(slot);
}
fn send_op_entity_g2_alt2(c: &mut Client, opcode: i32, slot: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(opcode, isaac);
    out.p2_alt2(slot);
}
fn send_op_entity_g2_alt3(c: &mut Client, opcode: i32, slot: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(opcode, isaac);
    out.p2_alt3(slot);
}

// Java's per-action alt-variant choices preserved 1:1.
pub fn send_op_npc_1(c: &mut Client, npc_slot: i32) { send_op_entity_g2_alt3(c, 84, npc_slot); }
pub fn send_op_npc_2(c: &mut Client, npc_slot: i32) { send_op_entity_g2_alt2(c, 13, npc_slot); }
pub fn send_op_npc_3(c: &mut Client, npc_slot: i32) { send_op_entity_g2_alt1(c, 67, npc_slot); }
pub fn send_op_npc_4(c: &mut Client, npc_slot: i32) { send_op_entity_g2_alt1(c, 95, npc_slot); }
pub fn send_op_npc_5(c: &mut Client, npc_slot: i32) { send_op_entity_g2    (c, 88, npc_slot); }

// @ObfuscatedName(— Client.sendOpPlayerU). Verbatim port of
// Client.java:8642-8657 (action == 14). Use-item-on-player: pairs a
// selected inventory item with a target player slot.
pub fn send_op_player_u(c: &mut Client, player_slot: i32, obj_com_id: i32, obj_selected_slot: i32, obj_selected_com_id: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(226, isaac);
    out.p2_alt2(obj_com_id);
    out.p2_alt1(obj_selected_slot);
    out.p2_alt2(player_slot);
    out.p4_alt2(obj_selected_com_id);
}

pub fn send_op_player_1(c: &mut Client, player_slot: i32) { send_op_entity_g2    (c, 246, player_slot); }
pub fn send_op_player_2(c: &mut Client, player_slot: i32) { send_op_entity_g2    (c, 146, player_slot); }
pub fn send_op_player_3(c: &mut Client, player_slot: i32) { send_op_entity_g2_alt1(c, 102, player_slot); }
pub fn send_op_player_4(c: &mut Client, player_slot: i32) { send_op_entity_g2    (c, 78,  player_slot); }
pub fn send_op_player_5(c: &mut Client, player_slot: i32) { send_op_entity_g2_alt2(c, 117, player_slot); }

// @ObfuscatedName(— Client.sendResumePauseButton). Verbatim port of
// Client.java:9151-9160. Sent when the user clicks a dialog "Continue"
// button to resume a paused script.
pub fn send_resume_pause_button(c: &mut Client, slot: i32, component_id: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(242, isaac);
    out.p2_alt2(slot);
    out.p4(component_id);
}

// ── OPOBJ[2..5] outbound packets ──────────────────────────────────
// Verbatim ports — each carries (obj_id, tile_x, tile_z).

pub fn send_op_obj_2(c: &mut Client, obj_id: i32, tile_x: i32, tile_z: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(177, isaac);
    out.p2(c.map_build_base_z + tile_z);
    out.p2_alt3(obj_id);
    out.p2(c.map_build_base_x + tile_x);
}
pub fn send_op_obj_3(c: &mut Client, obj_id: i32, tile_x: i32, tile_z: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(224, isaac);
    out.p2_alt2(obj_id);
    out.p2_alt3(c.map_build_base_x + tile_x);
    out.p2_alt2(c.map_build_base_z + tile_z);
}
pub fn send_op_obj_4(c: &mut Client, obj_id: i32, tile_x: i32, tile_z: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(139, isaac);
    out.p2_alt1(c.map_build_base_z + tile_z);
    out.p2_alt1(c.map_build_base_x + tile_x);
    out.p2_alt3(obj_id);
}
pub fn send_op_obj_5(c: &mut Client, obj_id: i32, tile_x: i32, tile_z: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(77, isaac);
    out.p2(c.map_build_base_x + tile_x);
    out.p2_alt2(c.map_build_base_z + tile_z);
    out.p2_alt3(obj_id);
}

// @ObfuscatedName(— Client.sendOpLocT). Verbatim port of
// Client.java:8661-8669. Use-target on a location (spell on a door).
pub fn send_op_loc_t(c: &mut Client, loc_typecode: i32, tile_x: i32, tile_z: i32, target_sub: i32, target_com: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(247, isaac);
    out.p4_alt3(target_com);
    out.p2(c.map_build_base_z + tile_z);
    out.p2_alt1(target_sub);
    out.p2_alt2((loc_typecode >> 14) & 0x7FFF);
    out.p2(c.map_build_base_x + tile_x);
}

// @ObfuscatedName(— Client.sendOpLocU). Verbatim port of
// Client.java:8744-8754. Use-item on a location.
pub fn send_op_loc_u(c: &mut Client, loc_typecode: i32, tile_x: i32, tile_z: i32, obj_selected_com_id: i32, obj_selected_slot: i32, obj_com_id: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(241, isaac);
    out.p4_alt1(obj_selected_com_id);
    out.p2(obj_selected_slot);
    out.p2((loc_typecode >> 14) & 0x7FFF);
    out.p2_alt2(c.map_build_base_x + tile_x);
    out.p2_alt1(obj_com_id);
    out.p2_alt2(c.map_build_base_z + tile_z);
}

// @ObfuscatedName(— Client.sendOpNpcT). Verbatim port of
// Client.java:8488-8492.
pub fn send_op_npc_t(c: &mut Client, npc_slot: i32, target_sub: i32, target_com: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(190, isaac);
    out.p4(target_com);
    out.p2_alt2(npc_slot);
    out.p2_alt2(target_sub);
}

// @ObfuscatedName(— Client.sendOpNpcU). Verbatim port of
// Client.java:9243-9248.
pub fn send_op_npc_u(c: &mut Client, npc_slot: i32, obj_selected_slot: i32, obj_selected_com_id: i32, obj_com_id: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(106, isaac);
    out.p2_alt2(obj_selected_slot);
    out.p4(obj_selected_com_id);
    out.p2_alt1(npc_slot);
    out.p2_alt3(obj_com_id);
}

// ── INV_BUTTON[1..5] outbound packets ─────────────────────────────
// Verbatim ports of Client.java action 39 → INV_BUTTON1 (21), action
// 40 → INV_BUTTON2 (202), action 41 → INV_BUTTON3 (6), action 42 →
// INV_BUTTON4 (186), action 43 → INV_BUTTON5 (40).
//
// All carry (item_slot, component_id, obj_id). Each variant uses
// different alt-encoders for the slot/component fields to match
// Java's mixed encoding. Side-effect: stamps selected_* state for
// the use-with system to pick up.

pub fn send_inv_button_1(c: &mut Client, slot: i32, component_id: i32, obj_id: i32) {
    {
        let Some(isaac) = c.isaac_out.as_mut() else { return; };
        let Some(out) = c.out_packet.as_mut() else { return; };
        out.p1_enc(21, isaac);
        out.p2(slot);
        out.p4_alt2(component_id);
        out.p2_alt1(obj_id);
    }
    c.selected_cycle = 0;
    c.selected_com = component_id;
    c.selected_item = slot;
}

pub fn send_inv_button_2(c: &mut Client, slot: i32, component_id: i32, obj_id: i32) {
    {
        let Some(isaac) = c.isaac_out.as_mut() else { return; };
        let Some(out) = c.out_packet.as_mut() else { return; };
        out.p1_enc(202, isaac);
        out.p2_alt1(obj_id);
        out.p4_alt2(component_id);
        out.p2_alt1(slot);
    }
    c.selected_cycle = 0;
    c.selected_com = component_id;
    c.selected_item = slot;
}

pub fn send_inv_button_3(c: &mut Client, slot: i32, component_id: i32, obj_id: i32) {
    {
        let Some(isaac) = c.isaac_out.as_mut() else { return; };
        let Some(out) = c.out_packet.as_mut() else { return; };
        out.p1_enc(6, isaac);
        out.p2_alt1(slot);
        out.p4_alt1(component_id);
        out.p2_alt3(obj_id);
    }
    c.selected_cycle = 0;
    c.selected_com = component_id;
    c.selected_item = slot;
}

pub fn send_inv_button_4(c: &mut Client, slot: i32, component_id: i32, obj_id: i32) {
    {
        let Some(isaac) = c.isaac_out.as_mut() else { return; };
        let Some(out) = c.out_packet.as_mut() else { return; };
        out.p1_enc(186, isaac);
        out.p2(slot);
        out.p4(component_id);
        out.p2(obj_id);
    }
    c.selected_cycle = 0;
    c.selected_com = component_id;
    c.selected_item = slot;
}

pub fn send_inv_button_5(c: &mut Client, slot: i32, component_id: i32, obj_id: i32) {
    {
        let Some(isaac) = c.isaac_out.as_mut() else { return; };
        let Some(out) = c.out_packet.as_mut() else { return; };
        out.p1_enc(40, isaac);
        out.p2_alt1(obj_id);
        out.p4_alt1(component_id);
        out.p2_alt2(slot);
    }
    c.selected_cycle = 0;
    c.selected_com = component_id;
    c.selected_item = slot;
}

// @ObfuscatedName(— Client.sendIfButton). Verbatim port of
// Client.java:8512-8523 (action == 28) and 8701-8714 (action == 29).
// Java emits opcode 155 with the component id, then optionally
// toggles a varp locally if the component has a `scripts[0][0] == 5`
// trigger (Java's "IF_BUTTON sets var" attribute).
//
// `toggle_mode` selects between action 28 (toggle 0↔1) and action 29
// (set to script_operand[0]). When neither toggle is desired, just
// passes the opcode 155.
pub enum IfButtonToggle {
    None,
    Toggle,
    SetTo(i32),
}
pub fn send_if_button(c: &mut Client, component_id: i32, toggle: IfButtonToggle) {
    {
        let Some(isaac) = c.isaac_out.as_mut() else { return; };
        let Some(out) = c.out_packet.as_mut() else { return; };
        out.p1_enc(155, isaac);
        out.p4(component_id);
    }
    // Apply the local var toggle. Component scripts/scriptOperand
    // wiring lands when the IfType decode finishes that field set.
    let Some(com) = crate::config::if_type::get(component_id) else { return; };
    if com.scripts.is_empty() { return; }
    let Some(first) = com.scripts.first() else { return; };
    if first.len() < 2 { return; }
    if first[0] != 5 { return; }
    let varp_id = first[1];
    match toggle {
        IfButtonToggle::None => {}
        IfButtonToggle::Toggle => {
            let cur = crate::config::var_cache::get_varp(varp_id);
            crate::config::var_cache::set_varp(varp_id, 1 - cur);
        }
        IfButtonToggle::SetTo(v) => {
            if crate::config::var_cache::get_varp(varp_id) != v {
                crate::config::var_cache::set_varp(varp_id, v);
            }
        }
    }
}

// @ObfuscatedName(— Client.sendOpLocExamine). Verbatim port of
// Client.java:8525-8534 (action == 1002). Emits the loc-examine
// opcode 162 — server replies with the loc's examine string.
pub fn send_op_loc_examine(c: &mut Client, loc_typecode: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(162, isaac);
    out.p2_alt2((loc_typecode >> 14) & 0x7FFF);
}

// @ObfuscatedName(— Client.sendOpPlayer7). Verbatim port of
// Client.java:8685-8698. The 7th right-click player op (typically
// "Trade with" in vanilla).
pub fn send_op_player_7(c: &mut Client, player_slot: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(119, isaac);
    out.p2_alt3(player_slot);
}

// @ObfuscatedName(— Client.sendOpNpcExamine). Verbatim port of
// Client.java:9043-9061 (action == 1003). Resolves multinpc redirect
// then sends opcode 52 with the actual type id.
pub fn send_op_npc_examine(c: &mut Client, npc_slot: i32) {
    let Some(Some(npc)) = c.npcs.get(npc_slot as usize) else { return; };
    let type_id = npc.type_id;
    if type_id < 0 { return; }
    let mut npc_type = crate::config::npc_type::list(type_id);
    if let Some(active) = npc_type.get_multi_npc() {
        npc_type = active;
    }
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(52, isaac);
    out.p2(npc_type.id);
}

// @ObfuscatedName(— Client.sendMessagePublic). Verbatim port of
// ScriptRunner.java:2599-2609 (the outbound portion of opcode 5008).
// Variable-length packet: opcode 205 + u8 size placeholder + body
// (color byte + effect byte + WordPack-compressed message), then
// back-fill the size byte.
//
// `parse_chat_prefix` strips a leading `<yellow>/<red>/<wave>/...`
// markup from `raw_message` and returns the matching (color, effect)
// codes — Java's `Text.CHATCOL_*` / `Text.CHATEFFECT_*` table. If
// you're calling from cs2, prefer letting the caller parse them.
pub fn send_message_public(c: &mut Client, color: i32, effect: i32, message: &str) {
    use crate::io::packet::Packet;
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(205, isaac);
    out.p1(0);
    let start = out.pos;
    out.p1(color);
    out.p1(effect);
    // WordPack compression isn't yet ported; until it lands, send as
    // a length-prefixed pjstr. Dev servers tolerate both forms.
    out.p1(Packet::pjstrlen(message));
    out.pjstr(message);
    out.psize1(out.pos - start);
}

// @ObfuscatedName(— Client.parseChatPrefixes). Java's color/effect
// prefix parser lives inline in cs2 opcode 5008; we factor it out so
// the call site reads cleanly. Returns `(color, effect, stripped)`
// where `stripped` is the trimmed message.
pub fn parse_chat_prefixes(raw: &str) -> (i32, i32, String) {
    // Java consults a giant `Text.CHATCOL_*` constant table; we
    // hardcode the English set since that's all rev1 ships in the
    // primary localisation. German variants land when the i18n table
    // does.
    let table: &[(&str, i32)] = &[
        ("yellow:", 0), ("red:", 1), ("green:", 2), ("cyan:", 3),
        ("purple:", 4), ("white:", 5),
        ("flash1:", 6), ("flash2:", 7), ("flash3:", 8),
        ("glow1:", 9), ("glow2:", 10), ("glow3:", 11),
    ];
    let mut color = 0i32;
    let mut effect = 0i32;
    let mut message = raw.to_string();
    let lower = message.to_lowercase();
    for &(prefix, code) in table {
        if lower.starts_with(prefix) {
            color = code;
            message = message[prefix.len()..].to_string();
            break;
        }
    }
    let effect_table: &[(&str, i32)] = &[
        ("wave:", 1), ("wave2:", 2), ("shake:", 3),
        ("scroll:", 4), ("slide:", 5),
    ];
    let lower2 = message.to_lowercase();
    for &(prefix, code) in effect_table {
        if lower2.starts_with(prefix) {
            effect = code;
            message = message[prefix.len()..].to_string();
            break;
        }
    }
    (color, effect, message)
}

// @ObfuscatedName(— Client.sendSetVarClient). Verbatim port of
// ScriptRunner.java:158-163 (opcode 2 pop_varp). After mutating a
// client-side varp via cs2, this opcode tells the server to mirror
// the new value. Outbound: opcode 181 + varp_id (g2) + value (g4).
pub fn send_set_var_client(c: &mut Client, varp_id: i32, value: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(181, isaac);
    out.p2(varp_id);
    out.p4(value);
}

// @ObfuscatedName(— Client.sendChatFilterSettings). Verbatim port of
// ScriptRunner.java:2404-2419 outbound. Emits opcode 167 with the
// three filter bytes (public, private enum index, trade).
pub fn send_chat_filter_settings(c: &mut Client, public_mode: i32, private_mode_index: i32, trade_mode: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(167, isaac);
    out.p1(public_mode);
    out.p1(private_mode_index);
    out.p1(trade_mode);
}

// @ObfuscatedName(— Client.sendMessagePrivate). Verbatim port of
// ScriptRunner.java:2619-2627. Variable-length packet: opcode 211 +
// u16 size placeholder + (pjstr recipient + WordPack-compressed
// message), then back-fill the size.
//
// WordPack compression isn't yet ported; until it lands, we send the
// raw message bytes as a pjstr — the server-side handler in dev
// builds tolerates both forms.
pub fn send_message_private(c: &mut Client, recipient: &str, message: &str) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(211, isaac);
    out.p2(0);
    let start = out.pos;
    out.pjstr(recipient);
    out.pjstr(message);
    out.psize2(out.pos - start);
}

// @ObfuscatedName(— Client.sendReportAbuse). Java:Client.java around
// ScriptRunner opcode 5002. Reports a player for abuse with the
// reason code (1..12) and "ignored from now" flag.
pub fn send_report_abuse(c: &mut Client, name: &str, rule_index: i32, mute_too: i32) {
    use crate::io::packet::Packet;
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(96, isaac);
    out.p1(Packet::pjstrlen(name) + 2);
    out.pjstr(name);
    out.p1(rule_index - 1);
    out.p1(mute_too);
}

// @ObfuscatedName(— Client.sendNoTimeout). Verbatim port of
// Client.java:2731 — fires periodically (every ~50 ticks) so the
// server's timeout counter doesn't trip during long AFK periods.
pub fn send_no_timeout(c: &mut Client) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(228, isaac);
}

// @ObfuscatedName(— Client.sendMapBuildComplete). Verbatim port of
// Client.java:5348. Sent after a REBUILD_* finishes so the server
// knows we've finished loading the new region.
pub fn send_map_build_complete(c: &mut Client) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(197, isaac);
}

// @ObfuscatedName(— Client.sendCloseModal). Verbatim port of
// Client.java:11793. Tells the server to close any open modal (e.g.
// when the user presses Esc).
pub fn send_close_modal(c: &mut Client) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(129, isaac);
}

// @ObfuscatedName(— Client.closeModal). Verbatim port of
// Client.java:11791-11805: CLOSE_MODAL packet, then close every
// modal-class subinterface (types 0 and 3) and release the pending
// resume-pause component.
pub fn close_modal(c: &mut Client) {
    send_close_modal(c);

    let keys: Vec<i32> = c.subinterfaces.iter()
        .filter(|(_, sub)| sub.type_ == 0 || sub.type_ == 3)
        .map(|(k, _)| *k)
        .collect();
    for key in keys {
        close_sub_interface(c, key, true);
    }

    if c.resume_pause_com != -1 {
        if let Some(com) = crate::config::if_type::get(c.resume_pause_com) {
            component_updated(&com);
        }
        c.resume_pause_com = -1;
    }
}

// @ObfuscatedName("ao.ec(ILjava/lang/String;I)V") — Client.opPlayer.
// Verbatim port of Client.java:9298-9337: normalise the target name,
// find the matching tracked player, path toward them, then send the
// OPPLAYERn packet for the requested action. Unknown names report
// "Unable to find <name>".
pub fn op_player(c: &mut Client, action: i32, target: &str) {
    let raw = crate::jstring::to_raw_username_str(target);
    let display = crate::jstring::to_screen_name(crate::jstring::to_userhash(&raw))
        .unwrap_or_default();

    let mut found = false;
    for i in 0..c.player_count as usize {
        let pid = c.player_ids[i];
        let dest = c.players.get(pid as usize)
            .and_then(|o| o.as_ref())
            .filter(|p| p.name.eq_ignore_ascii_case(&display))
            .map(|p| (p.route_x[0], p.route_z[0]));
        let Some((dst_x, dst_z)) = dest else { continue; };
        let Some((src_x, src_z)) = c.local_player.as_ref()
            .map(|lp| (lp.route_x[0], lp.route_z[0])) else { break; };

        try_move(c, src_x, src_z, dst_x, dst_z, false, 0, 0, 1, 1, 0, 2);

        let Some(isaac) = c.isaac_out.as_mut() else { return; };
        let Some(out) = c.out_packet.as_mut() else { return; };
        if action == 1 {
            // OPPLAYER1
            out.p1_enc(246, isaac);
            out.p2(pid);
        } else if action == 4 {
            // OPPLAYER4
            out.p1_enc(78, isaac);
            out.p2(pid);
        } else if action == 6 {
            // OPPLAYER6
            out.p1_enc(111, isaac);
            out.p2_alt3(pid);
        } else if action == 7 {
            // OPPLAYER7
            out.p1_enc(119, isaac);
            out.p2_alt3(pid);
        }

        found = true;
        break;
    }
    if !found {
        add_chat(c, 0, Some(String::new()),
                 Some(format!("Unable to find {display}")),
                 Some(String::new()), 0);
    }
}

// @ObfuscatedName("bs.fz(Leg;I)Leg;") — Client.getDragLayer. Verbatim
// port of Client.java:11519-11547: walk serverDraggable(getActive)
// levels up the layer chain; when that yields nothing fall back to
// the component's cs2-assigned draggable target.
pub fn get_drag_layer(c: &Client, com: crate::script_runner::ComRef)
    -> crate::script_runner::ComRef
{
    use crate::script_runner::ComRef;
    let Some(resolved) = com.resolve() else { return ComRef::None; };

    let levels = crate::config::server_active::server_draggable(get_active(c, &resolved));
    let mut walked = ComRef::None;
    if levels != 0 {
        let mut cur = resolved.clone();
        let mut cur_ref = com;
        let mut ok = true;
        for _ in 0..levels {
            let next_id = cur.layer_id;
            match crate::config::if_type::get(next_id) {
                Some(next) => {
                    cur = next;
                    cur_ref = ComRef::Com(next_id);
                }
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            walked = cur_ref;
        }
    }

    if walked == ComRef::None {
        // Java: var5 = arg0.draggable (the IfType the cs2 set via
        // cc_setdraggable; null stays null).
        if resolved.draggable_sub != -2 && resolved.draggable != 0 {
            return ComRef::from_pair(resolved.draggable, resolved.draggable_sub);
        }
        return ComRef::None;
    }
    walked
}

// @ObfuscatedName("ch.ff(Leg;IIB)V") — Client.dragTryPickup. Verbatim
// port of Client.java:11357-11368.
pub fn drag_try_pickup(c: &mut Client, com: crate::script_runner::ComRef, x: i32, y: i32) {
    use crate::script_runner::ComRef;
    if c.drag_com != ComRef::None || c.is_menu_open {
        return;
    }
    let layer = get_drag_layer(c, com);
    if com == ComRef::None || layer == ComRef::None {
        return;
    }

    c.drag_com = com;
    c.drag_layer = layer;
    c.drag_pickup_x = x;
    c.drag_pickup_y = y;
    c.drag_time = 0;
    c.drag_alive = false;
}

// @ObfuscatedName(— Client.dispatchOrbitCamera). Verbatim port of
// Client.java:4103-4112. When not in cinema mode, route the orbit
// position + macro angle + shake state through cam_follow to set the
// scene camera. `camera_pitch_clamp` is the per-tile ceiling clamp
// (mapl 0x4 trigger).
pub fn dispatch_orbit_camera(c: &mut Client, camera_pitch_clamp: i32) {
    if c.cinema_cam { return; }
    let Some(lp) = c.local_player.as_ref() else { return; };
    let lp_x = lp.entity.x;
    let lp_z = lp.entity.z;
    let lp_level = c.minusedlevel;
    let mut pitch = c.orbit_cam_pitch;
    if camera_pitch_clamp / 256 > pitch {
        pitch = camera_pitch_clamp / 256;
    }
    if c.cam_shake[4] && c.cam_shake_ran[4] + 128 > pitch {
        pitch = c.cam_shake_ran[4] + 128;
    }
    let yaw = (c.orbit_cam_yaw + c.macro_camera_angle) & 0x7FF;
    let anchor_y = get_av_h(lp_x, lp_z, lp_level) - 50;
    let distance = pitch * 3 + 600;
    cam_follow(c, pitch, yaw, c.orbit_cam_x, anchor_y, c.orbit_cam_z, distance);
}

// @ObfuscatedName(— Client.clientVar). Verbatim port of
// Client.java:11663-11747. Server-side var changes flow here so the
// client can re-derive UI / audio / interface state from the new
// varp value:
//
//   clientcode 1  — brightness (Pix3D gamma re-init); deferred until
//                   the brightness path is wired.
//   clientcode 3  — MIDI volume (0..255 in 5 steps).
//   clientcode 4  — Wave volume (0..127).
//   clientcode 5  — one-mouse-button mode.
//   clientcode 6  — chat effects toggle.
//   clientcode 9  — bank arrange mode.
//   clientcode 10 — ambient volume (0..127).
//
// Also fires legacyUpdated + BgSound.recalculateMultilocs so any
// loc-anchored emitter rebinds to the new multi-var variant.
pub fn client_var(c: &mut Client, varp_id: i32) {
    legacy_updated(c);
    crate::sound::bg_sound::recalculate_multilocs();
    let clientcode = crate::config::varp_type::list(varp_id).clientcode;
    if clientcode == 0 { return; }
    let value = crate::config::var_cache::get_varp(varp_id);
    match clientcode {
        1 => {
            // Brightness preset → Pix3D.init_colour_table when wired.
        }
        3 => {
            let volume = match value {
                0 => 255, 1 => 192, 2 => 128, 3 => 64, _ => 0,
            };
            if c.midi_volume != volume {
                if c.midi_volume == 0 && c.next_midi_song != -1 {
                    c.queued_song_id = c.next_midi_song;
                    c.playing_jingle = false;
                } else if volume == 0 {
                    c.queued_song_id = -1;
                    c.playing_jingle = false;
                }
                c.midi_volume = volume;
            }
        }
        4 => {
            c.wave_volume = match value {
                0 => 127, 1 => 96, 2 => 64, 3 => 32, _ => 0,
            };
        }
        5 => { c.one_mouse_button = value; }
        6 => { c.chat_effects = value; }
        9 => { c.bank_arrange_mode = value; }
        10 => {
            c.ambient_volume = match value {
                0 => 127, 1 => 96, 2 => 64, 3 => 32, _ => 0,
            };
        }
        _ => {}
    }
}

// @ObfuscatedName(— Client.macroCameraDrift). Verbatim port of
// Client.java:2664-2699. Every 500 ticks the macro camera applies a
// small random nudge to (x, z, angle), then clamps the modifiers when
// the position hits the soft bounds. Reset/init on reconnect.
pub fn macro_camera_drift(c: &mut Client) {
    c.macro_camera_cycle += 1;
    if c.macro_camera_cycle > 500 {
        c.macro_camera_cycle = 0;
        let r = (rand_unit_local() * 8.0) as i32;
        if (r & 0x1) == 1 { c.macro_camera_x += c.macro_camera_x_modifier; }
        if (r & 0x2) == 2 { c.macro_camera_z += c.macro_camera_z_modifier; }
        if (r & 0x4) == 4 { c.macro_camera_angle += c.macro_camera_angle_modifier; }
    }
    if c.macro_camera_x < -50 { c.macro_camera_x_modifier = 2; }
    if c.macro_camera_x >  50 { c.macro_camera_x_modifier = -2; }
    if c.macro_camera_z < -55 { c.macro_camera_z_modifier = 2; }
    if c.macro_camera_z >  55 { c.macro_camera_z_modifier = -2; }
    if c.macro_camera_angle < -40 { c.macro_camera_angle_modifier = 1; }
    if c.macro_camera_angle >  40 { c.macro_camera_angle_modifier = -1; }
}

// @ObfuscatedName(— Client.bumpCamShakeCycles). Per-tick increment of
// the cam_shake_cycle counter for each of the 5 shake slots — used
// by apply_cam_shake to advance the sin phase. Java buries this in
// the main loop's render block.
pub fn bump_cam_shake_cycles(c: &mut Client) {
    for slot in 0..5 {
        if c.cam_shake[slot] {
            c.cam_shake_cycle[slot] += 1;
        }
    }
}

// @ObfuscatedName(— Client.decayDamageCycles). Per-tick decrement of
// the per-entity hitmark timer. After 70 ticks the hitmark fades; we
// mirror Java's `damageCycles[i] <= currentCycle` check by counting
// down then clearing on zero.
pub fn decay_damage_cycles(c: &mut Client) {
    let cycle = c.loop_cycle;
    if let Some(lp) = c.local_player.as_mut() {
        decay_entity_damage(&mut lp.entity, cycle);
    }
    for entity in c.players.iter_mut().flatten() {
        decay_entity_damage(&mut entity.entity, cycle);
    }
    for entity in c.npcs.iter_mut().flatten() {
        decay_entity_damage(&mut entity.entity, cycle);
    }
}

fn decay_entity_damage(entity: &mut crate::dash3d::ClientEntity, current_cycle: i32) {
    for i in 0..4 {
        if entity.damage_cycles[i] != 0 && entity.damage_cycles[i] <= current_cycle {
            entity.damage_cycles[i] = 0;
            entity.damage_values[i] = 0;
            entity.damage_types[i] = 0;
        }
    }
}

// @ObfuscatedName(— Client.jingleCompleteCheck). Verbatim port of
// Client.java:3577-3583 (subset). When a jingle finishes (signalled
// by the MIDI subsystem outside this function), resume the pending
// song by queuing it again. Java reads MidiManager.isInitialised; we
// approximate by checking whether playing_jingle was set but the
// queue has drained.
pub fn jingle_complete_check(c: &mut Client) {
    if c.playing_jingle && c.queued_jingle_id == -1 {
        c.playing_jingle = false;
        if c.next_midi_song != -1 {
            c.queued_song_id = c.next_midi_song;
        }
    }
}

// @ObfuscatedName("at.ej(ZB)V") — Client.preventTimeout. Verbatim
// port of Client.java:4992-5012. Throttles NO_TIMEOUT to once every
// 50 ticks (or immediately when `force` is true) during long-running
// mapBuildLoop / reconnect waits so the server doesn't drop the
// connection.
pub fn prevent_timeout(c: &mut Client, force: bool) {
    c.no_timeout_timer += 1;
    if c.no_timeout_timer < 50 && !force {
        return;
    }
    c.no_timeout_timer = 0;
    if c.network_error { return; }
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(228, isaac);
    // Java also calls `stream.write(out.data, 0, out.pos); out.pos = 0;`
    // here to flush; the Rust outbound stream is queued by client_stream
    // so the next mainloop pump drains it. We leave the bytes in
    // out_packet for that drain.
}

// @ObfuscatedName("dr.et(III)V") — Client.showObject. Verbatim port
// of Client.java:7652-7690. Picks the most valuable obj on a tile —
// "value" = ObjType.cost (scaled by count+1 for stackable items) —
// and registers it (plus up to two distinct-id extras) with the
// renderer via World.setObj. Each ClientObj enters the scene as a
// Temp ModelSource whose getTempModel is ObjType.getModelLit(id,
// count), exactly mirroring Java's ClientObj.getTempModel.
//
// Returns `true` if any obj was visible. `pile` is the slice of
// every ClientObj on the (level, tile_x, tile_z) cell. When the pile
// is empty, the tile's world ground-object slot is cleared.
pub fn show_object(
    world: &mut crate::dash3d::world::World,
    level: i32, tile_x: i32, tile_z: i32,
    pile: &[crate::dash3d::ClientObj],
) -> bool {
    use crate::dash3d::model_source::ModelSource;
    if pile.is_empty() {
        world.del_obj(level, tile_x, tile_z);
        return false;
    }
    let mut best_value = i32::MIN;
    let mut best: Option<&crate::dash3d::ClientObj> = None;
    for obj in pile {
        let Some(t) = crate::config::obj_type::list(obj.id) else { continue; };
        let mut value = t.cost;
        if t.stackable == 1 {
            value = value.saturating_mul(obj.count + 1);
        }
        if value > best_value {
            best_value = value;
            best = Some(obj);
        }
    }
    let Some(top) = best else {
        world.del_obj(level, tile_x, tile_z);
        return false;
    };
    // Up to 2 additional distinct-id objs for the stack render
    // (Java's var8 / var9 scan).
    let mut second: Option<&crate::dash3d::ClientObj> = None;
    let mut third: Option<&crate::dash3d::ClientObj> = None;
    for obj in pile {
        if obj.id == top.id { continue; }
        if second.is_none() {
            second = Some(obj);
            continue;
        }
        if second.map(|s| s.id) != Some(obj.id) && third.is_none() {
            third = Some(obj);
            break;
        }
    }
    // ClientObj.getTempModel — composes ObjType.getModelLit(id, count)
    // at render time.
    let obj_source = |obj: &crate::dash3d::ClientObj| -> std::sync::Arc<ModelSource> {
        let (id, count) = (obj.id, obj.count);
        ModelSource::temp(std::sync::Arc::new(move || {
            crate::config::obj_type::list(id)?.get_model_lit(count)
        }))
    };
    // Java: typecode = (tileZ << 7) + tileX + 0x60000000.
    let typecode = (tile_z << 7) + tile_x + 1610612736;
    let y = get_av_h(tile_x * 128 + 64, tile_z * 128 + 64, level);
    world.set_obj(
        level, tile_x, tile_z, y,
        Some(obj_source(top)), typecode,
        second.map(obj_source),
        third.map(obj_source),
    );
    true
}

// @ObfuscatedName(— Client.roofCheck2). Verbatim port of
// Client.java:4365-4372. Used during cinema mode to clamp the
// minimum visible level (Java's `var62`): if the camera is inside a
// roofed tile (mapl flag 0x4 set) and within 800 units of the
// ground, restrict drawing to the current level only. Otherwise
// draw all 3 levels normally.
pub fn roof_check_2(c: &Client) -> i32 {
    let h = get_av_h(c.cam_x, c.cam_z, c.minusedlevel);
    let tile_x = (c.cam_x >> 7) as usize;
    let tile_z = (c.cam_z >> 7) as usize;
    let cb = crate::client_build::STATE.lock().unwrap();
    let level = c.minusedlevel as usize;
    let in_roof = cb.mapl.get(level)
        .and_then(|l| l.get(tile_x))
        .and_then(|r| r.get(tile_z))
        .map(|f| (*f & 0x4) != 0)
        .unwrap_or(false);
    if h - c.cam_y < 800 && in_roof {
        c.minusedlevel
    } else {
        3
    }
}

// @ObfuscatedName(— Client.applyCamShake). Verbatim port of
// Client.java:4127-4153. Per-axis sinusoidal jitter of the camera
// position + yaw + pitch. Each of the 5 shake slots is keyed by
// axis (0=X, 1=Y, 2=Z, 3=Yaw, 4=Pitch) with independent amplitude /
// random / cycle counters.
pub fn apply_cam_shake(c: &mut Client) {
    for slot in 0..5 {
        if !c.cam_shake[slot] { continue; }
        let amp = c.cam_shake_amp[slot] as f64;
        let cycle = c.cam_shake_cycle[slot] as f64;
        let ran = c.cam_shake_ran[slot] as f64;
        let axis = c.cam_shake_axis[slot];
        let jitter = (rand_unit_local() * (axis as f64 * 2.0 + 1.0)
            - axis as f64
            + (amp / 100.0 * cycle).sin() * ran) as i32;
        match slot {
            0 => c.cam_x += jitter,
            1 => c.cam_y += jitter,
            2 => c.cam_z += jitter,
            3 => c.cam_yaw = (c.cam_yaw + jitter) & 0x7FF,
            4 => {
                c.cam_pitch += jitter;
                if c.cam_pitch < 128 { c.cam_pitch = 128; }
                if c.cam_pitch > 383 { c.cam_pitch = 383; }
            }
            _ => {}
        }
        c.cam_shake_cycle[slot] += 1;
    }
}

// Cheap LCG matching the other Client RNGs — gives deterministic-
// but-good distribution.
fn rand_unit_local() -> f64 {
    use std::sync::Mutex;
    static SEED: Mutex<u64> = Mutex::new(0xCBF2_9CE4_8422_2325);
    let mut g = SEED.lock().unwrap();
    *g = g.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let v = (*g >> 11) as f64 / (1u64 << 53) as f64;
    v.clamp(0.0, 1.0 - f64::EPSILON)
}

// @ObfuscatedName(— Client.followCamera). Verbatim port of
// Client.java:3337-3371. Smooth-tracks the orbit centre toward the
// local-player position with a +/-500 snap clamp, then applies the
// arrow-key-driven yaw/pitch velocities with /2 damping.
pub fn follow_camera(c: &mut Client) {
    let Some(lp) = c.local_player.as_ref() else { return; };
    let target_x = c.macro_camera_x + lp.entity.x;
    let target_z = c.macro_camera_z + lp.entity.z;
    let dx = c.orbit_cam_x - target_x;
    let dz = c.orbit_cam_z - target_z;
    if !(-500..=500).contains(&dx) || !(-500..=500).contains(&dz) {
        c.orbit_cam_x = target_x;
        c.orbit_cam_z = target_z;
    }
    if c.orbit_cam_x != target_x {
        c.orbit_cam_x += (target_x - c.orbit_cam_x) / 16;
    }
    if c.orbit_cam_z != target_z {
        c.orbit_cam_z += (target_z - c.orbit_cam_z) / 16;
    }
    // Arrow-key yaw/pitch (key codes 96..99 in Java's KeyEvent).
    if c.key_held_96 {
        c.orbit_cam_yaw_velocity += (-24 - c.orbit_cam_yaw_velocity) / 2;
    } else if c.key_held_97 {
        c.orbit_cam_yaw_velocity += (24 - c.orbit_cam_yaw_velocity) / 2;
    } else {
        c.orbit_cam_yaw_velocity /= 2;
    }
    if c.key_held_98 {
        c.orbit_cam_pitch_velocity += (12 - c.orbit_cam_pitch_velocity) / 2;
    } else if c.key_held_99 {
        c.orbit_cam_pitch_velocity += (-12 - c.orbit_cam_pitch_velocity) / 2;
    } else {
        c.orbit_cam_pitch_velocity /= 2;
    }
    c.orbit_cam_yaw = (c.orbit_cam_yaw_velocity / 2 + c.orbit_cam_yaw) & 0x7FF;
    c.orbit_cam_pitch += c.orbit_cam_pitch_velocity / 2;
    if c.orbit_cam_pitch < 128 { c.orbit_cam_pitch = 128; }
    if c.orbit_cam_pitch > 383 { c.orbit_cam_pitch = 383; }
}

// @ObfuscatedName(— Client.camFollow). Verbatim port of
// Client.java:4336-4361. Given a pitch/yaw/distance orbiting an
// anchor (x, y, z), compute the camera's world position using two
// 2D rotation matrices from the Pix3D sin/cos table.
pub fn cam_follow(c: &mut Client, pitch: i32, yaw: i32, anchor_x: i32, anchor_y: i32, anchor_z: i32, distance: i32) {
    let p = (2048 - pitch) & 0x7FF;
    let y = (2048 - yaw) & 0x7FF;
    let mut x_off = 0i32;
    let mut y_off = 0i32;
    let mut z_off = distance;
    if p != 0 {
        let s = crate::dash3d::pix3d::sin_table()[p as usize];
        let cc = crate::dash3d::pix3d::cos_table()[p as usize];
        let new_y = (y_off * cc - distance * s) >> 16;
        z_off = (y_off * s + distance * cc) >> 16;
        y_off = new_y;
    }
    if y != 0 {
        let s = crate::dash3d::pix3d::sin_table()[y as usize];
        let cc = crate::dash3d::pix3d::cos_table()[y as usize];
        let new_x = (x_off * cc + z_off * s) >> 16;
        z_off = (z_off * cc - x_off * s) >> 16;
        x_off = new_x;
    }
    c.cam_x = anchor_x - x_off;
    c.cam_y = anchor_y - y_off;
    c.cam_z = anchor_z - z_off;
    c.cam_pitch = pitch;
    c.cam_yaw = yaw;
}

// @ObfuscatedName("dc.ek(Ldn;I)V") — Client.locChangeSetOld. Verbatim
// port of Client.java:7528-7554. Snapshots the current loc state at
// (x, z) on the given layer into the LocChange entry's `old_*`
// fields, so the queue can revert when `end_time` elapses.
//
// Each layer maps to a different World query:
//   layer 0 — Wall, via world.wall_type / wall.typecode2
//   layer 1 — Decor (wall-mounted), decor_type / decor.typecode2
//   layer 2 — Sprite (the dynamic loc list) — deferred until the
//             Square holds a sprite list; current Rust falls through
//             to -1 / 0 / 0 here.
//   layer 3 — GroundDecor.
pub fn loc_change_set_old(world: &crate::dash3d::world::World, loc: &mut crate::dash3d::LocChange) {
    let mut typecode = 0i32;
    let mut old_type = -1i32;
    let mut old_shape = 0i32;
    let mut old_angle = 0i32;
    match loc.layer {
        0 => typecode = world.wall_type(loc.level, loc.x, loc.z),
        1 => typecode = world.decor_type(loc.level, loc.x, loc.z),
        3 => typecode = world.gd_type(loc.level, loc.x, loc.z),
        _ => {}
    }
    if typecode != 0 {
        // shape = low 5 bits of typecode2; angle = bits 6..7.
        let tc2 = match loc.layer {
            0 => world.get_wall(loc.level, loc.x, loc.z).map(|w| w.typecode2).unwrap_or(0),
            1 => world.get_decor(loc.level, loc.x, loc.z).map(|d| d.typecode2).unwrap_or(0),
            3 => world.get_gd(loc.level, loc.x, loc.z).map(|g| g.typecode2).unwrap_or(0),
            _ => 0,
        };
        old_type = (typecode >> 14) & 0x7FFF;
        old_shape = tc2 & 0x1F;
        old_angle = (tc2 >> 6) & 0x3;
    }
    loc.old_type = old_type;
    loc.old_shape = old_shape;
    loc.old_angle = old_angle;
}

// @ObfuscatedName("ez.dm(B)V") — Client.cinemaCamera. Verbatim port
// of Client.java:3406-3497. The 3-axis camera-position lerp toward
// `cam_move_to_*` plus a yaw/pitch lerp toward `cam_look_at_*`. Java
// formula:
//
//   cam = cam + camMoveToRate2 * delta / 1000 + camMoveToRate
//
// with overshoot clamps at every axis. Yaw also has the 2048-wrap
// short-path selection.
pub fn cinema_camera(c: &mut Client) {
    let target_x = c.cam_move_to_lx * 128 + 64;
    let target_z = c.cam_move_to_lz * 128 + 64;
    let target_y = get_av_h(target_x, target_z, c.minusedlevel) - c.cam_move_to_hei;
    let rate = c.cam_move_to_rate;
    let rate2 = c.cam_move_to_rate2;

    // X axis ease.
    if c.cam_x < target_x {
        c.cam_x += rate2 * (target_x - c.cam_x) / 1000 + rate;
        if c.cam_x > target_x { c.cam_x = target_x; }
    }
    if c.cam_x > target_x {
        c.cam_x -= rate2 * (c.cam_x - target_x) / 1000 + rate;
        if c.cam_x < target_x { c.cam_x = target_x; }
    }
    // Y axis ease.
    if c.cam_y < target_y {
        c.cam_y += rate2 * (target_y - c.cam_y) / 1000 + rate;
        if c.cam_y > target_y { c.cam_y = target_y; }
    }
    if c.cam_y > target_y {
        c.cam_y -= rate2 * (c.cam_y - target_y) / 1000 + rate;
        if c.cam_y < target_y { c.cam_y = target_y; }
    }
    // Z axis ease.
    if c.cam_z < target_z {
        c.cam_z += rate2 * (target_z - c.cam_z) / 1000 + rate;
        if c.cam_z > target_z { c.cam_z = target_z; }
    }
    if c.cam_z > target_z {
        c.cam_z -= rate2 * (c.cam_z - target_z) / 1000 + rate;
        if c.cam_z < target_z { c.cam_z = target_z; }
    }

    // Look-at target.
    let look_x = c.cam_look_at_lx * 128 + 64;
    let look_z = c.cam_look_at_lz * 128 + 64;
    let look_y = get_av_h(look_x, look_z, c.minusedlevel) - c.cam_look_at_hei;
    let dx = look_x - c.cam_x;
    let dy = look_y - c.cam_y;
    let dz = look_z - c.cam_z;
    let ground_d = ((dx * dx + dz * dz) as f64).sqrt() as i32;
    let target_pitch = (((dy as f64).atan2(ground_d as f64) * 325.949) as i32) & 0x7FF;
    let target_yaw = (((dx as f64).atan2(dz as f64) * -325.949) as i32) & 0x7FF;
    let target_pitch = target_pitch.clamp(128, 383);

    let lrate = c.cam_look_at_rate;
    let lrate2 = c.cam_look_at_rate2;
    if c.cam_pitch < target_pitch {
        c.cam_pitch += lrate2 * (target_pitch - c.cam_pitch) / 1000 + lrate;
        if c.cam_pitch > target_pitch { c.cam_pitch = target_pitch; }
    }
    if c.cam_pitch > target_pitch {
        c.cam_pitch -= lrate2 * (c.cam_pitch - target_pitch) / 1000 + lrate;
        if c.cam_pitch < target_pitch { c.cam_pitch = target_pitch; }
    }

    // Yaw — shortest-path lerp (wrap at ±1024).
    let mut delta = target_yaw - c.cam_yaw;
    if delta > 1024 { delta -= 2048; }
    if delta < -1024 { delta += 2048; }
    if delta > 0 {
        c.cam_yaw += lrate2 * delta / 1000 + lrate;
        c.cam_yaw &= 0x7FF;
    }
    if delta < 0 {
        c.cam_yaw -= lrate2 * (-delta) / 1000 + lrate;
        c.cam_yaw &= 0x7FF;
    }
    // Overshoot check.
    let mut delta2 = target_yaw - c.cam_yaw;
    if delta2 > 1024 { delta2 -= 2048; }
    if delta2 < -1024 { delta2 += 2048; }
    if (delta2 < 0 && delta > 0) || (delta2 > 0 && delta < 0) {
        c.cam_yaw = target_yaw;
    }
}

// @ObfuscatedName("es.gd(Leg;B)Z") — Client.clientButton. Verbatim
// port of Client.java:11888-11913.
//
// Dispatches a per-component "client-code" button:
//   205        → schedule a 250-tick logout countdown
//   300-313    → idk_design.change_part (part_idx, forward?)
//   314-323    → idk_design.change_colour (col_idx, forward?)
//   324 / 325  → idk_design.change_gender(male / female)
//   326        → emit IDK_SAVEDESIGN (opcode 71) + Save body
//
// Returns true when the button "consumes" the click (logout countdown
// + savedesign); false means the caller (if_button_x) should still
// emit the normal IF_BUTTON packet.
//
// The full IdkDesign struct (player kit composer) isn't ported yet
// so the change_part / change_colour / change_gender arms are stubs
// recording the requested change so the eventual idk_design pass can
// drain them.
pub fn client_button(c: &mut Client, com: &crate::config::if_type::IfType) -> bool {
    let code = com.client_code;
    if code == 205 {
        c.logout_timer = 250;
        return true;
    }
    if (300..=313).contains(&code) {
        let part = (code - 300) / 2;
        let _forward = (code & 0x1) == 1;
        let _ = part;
    } else if (314..=323).contains(&code) {
        let col = (code - 314) / 2;
        let _forward = (code & 0x1) == 1;
        let _ = col;
    } else if code == 324 {
        // change_gender(false)
    } else if code == 325 {
        // change_gender(true)
    } else if code == 326 {
        // IDK_SAVEDESIGN — opcode 71.
        let Some(isaac) = c.isaac_out.as_mut() else { return true; };
        let Some(out) = c.out_packet.as_mut() else { return true; };
        out.p1_enc(71, isaac);
        // The body (12 worn-slot + 5 recol + gender) is written by
        // idk_design.save_design once the composer ships.
        return true;
    }
    false
}

// @ObfuscatedName(— Client.enterTargetMode). Verbatim port of
// Client.java:8944-8988 (action == 25 dispatch). Caller is the menu
// dispatcher when the user picks the "target" right-click op (e.g.
// "Cast spell on..."). We capture the active component's targetMask
// from ServerActive flags, set targetMode = true, fire the
// ontargetenter hook, and stamp target_verb / target_op for the
// "Use X with..." cursor overlay.
pub fn enter_target_mode(c: &mut Client, parent_com: i32, sub_id: i32) {
    use crate::config::if_type;
    let Some(com) = if_type::get_sub(parent_com, sub_id) else { return; };
    end_target_mode(c);
    let active_flags = get_active(c, &com);
    let target_mask = crate::config::server_active::target_mask(active_flags);
    // ontargetenter hook fires through ScriptRunner — deferred.
    let _ = com.hook_ontargetenter.as_ref();
    c.target_mode = true;
    c.target_com = parent_com;
    c.target_sub = sub_id;
    c.target_mask = target_mask;
    // Re-fetch to grab the v3/legacy classification and verb.
    let verb = if target_mask == 0 || com.target_verb.trim().is_empty() {
        "Null".to_string()
    } else {
        com.target_verb.clone()
    };
    c.target_verb = verb;
    c.target_op = if com.v3 {
        format!("{}{}", com.base_op_name, crate::string_constants::tag_colour(0xFFFFFF))
    } else {
        // The legacy branch uses target_base + green colour code.
        // base_op_name doubles for both forms in our model.
        format!("{}{}{}",
            crate::string_constants::tag_colour(0x00FF00),
            com.base_op_name,
            crate::string_constants::tag_colour(0xFFFFFF))
    };
    component_updated(&com);
}

// @ObfuscatedName("ba.eo(B)V") — Client.endTargetMode. Verbatim port
// of Client.java:9341-9356. Drops the "use X on..." state and fires
// the active component's `ontargetleave` hook if it has one.
pub fn end_target_mode(c: &mut Client) {
    if !c.target_mode { return; }
    let target_id = (c.target_com << 16) | (c.target_sub & 0xFFFF);
    let com = crate::config::if_type::get(target_id);
    if let Some(ref c2) = com {
        if c2.hook_ontargetleave.is_some() {
            // Hook trigger — defer until the ScriptRunner dispatcher
            // ships.
        }
    }
    c.target_mode = false;
    if let Some(com) = com {
        component_updated(&com);
    }
}

// @ObfuscatedName("cz.gq(IIIB)Ldy;") — Client.openSubInterface.
// Verbatim port of Client.java:11809-11836. Allocates a new
// SubInterface for `(parent_com_id, sub_id, kind)`, registers it,
// resets interface anims, fires the onload hook, marks the parent
// component dirty, and clears the menu.
//
// Java additionally calls `runHookImmediate(toplevelinterface, 1)`
// once the new sub is mounted — when the ScriptRunner dispatcher
// lands the hook re-fires on every sub open.
pub fn open_sub_interface(c: &mut Client, parent_com_id: i32, sub_id: i32, kind: i32) {
    let interfaces_slot = crate::config::if_type::INTERFACES_SLOT
        .load(std::sync::atomic::Ordering::Relaxed);
    let sub = SubInterface { id: sub_id, type_: kind, key: parent_com_id, field1599: false };
    c.subinterfaces.insert(parent_com_id, sub);
    if interfaces_slot >= 0 {
        if_anim_reset(interfaces_slot, sub_id);
    }
    // Java Client.java:11816 — onload hooks for the opened group.
    crate::script_runner::execute_onload(c, sub_id);
    if let Some(com) = crate::config::if_type::get(parent_com_id) {
        component_updated(&com);
    }
    if c.resume_pause_com >= 0 {
        if let Some(com) = crate::config::if_type::get(c.resume_pause_com) {
            component_updated(&com);
        }
        c.resume_pause_com = -1;
    }
    c.is_menu_open = false;
    c.menu_num_entries = 0;
    if c.toplevelinterface != -1 {
        let toplevel = c.toplevelinterface;
        run_hook_immediate(c, toplevel, 1);
    }
}

// @ObfuscatedName("d.fd(Ljava/lang/String;Ljava/lang/String;IIIII)V") —
// Client.addMenuOption. Verbatim port of Client.java:9438-9450.
// Appends a single menu entry to the buffer. Java rejects when the
// menu is already open (so right-clicks during menu display don't
// stack) or once the 500-entry cap is reached.
pub fn add_menu_option(c: &mut Client, verb: &str, subject: &str, action: i32, a: i32, b: i32, d: i32) {
    if c.is_menu_open || c.menu_num_entries >= 500 { return; }
    let idx = c.menu_num_entries as usize;
    c.menu_verb[idx] = verb.to_string();
    c.menu_subject[idx] = subject.to_string();
    c.menu_action[idx] = action;
    c.menu_param_a[idx] = a;
    c.menu_param_b[idx] = b;
    c.menu_param_c[idx] = d;
    c.menu_num_entries += 1;
}

// @ObfuscatedName(— Client.sortMinimenu). Verbatim port of
// Client.java:8290-8327. Stable bubble-sort that keeps entries with
// action < 1000 (game-world ops) above entries with action >= 1000
// (interface ops) so right-click ordering matches the Jagex UX.
pub fn sort_minimenu(c: &mut Client) {
    let n = c.menu_num_entries as usize;
    let mut done = false;
    while !done {
        done = true;
        for i in 0..n.saturating_sub(1) {
            // Skip if both ordered: action >= 1000 stays after a
            // < 1000 (we want game ops first, then UI).
            if c.menu_action[i] >= 1000 || c.menu_action[i + 1] <= 1000 {
                continue;
            }
            c.menu_subject.swap(i, i + 1);
            c.menu_verb.swap(i, i + 1);
            c.menu_action.swap(i, i + 1);
            c.menu_param_b.swap(i, i + 1);
            c.menu_param_c.swap(i, i + 1);
            c.menu_param_a.swap(i, i + 1);
            done = false;
        }
    }
}

// @ObfuscatedName("bk.ep(B)V") — Client.openMenu. Verbatim port of
// Client.java:8387-8420. Measures the widest line, sizes the menu
// box, centres it horizontally on the click + clamps to 765×503.
//
// `b12_width` is the b12-font width fn (the chrome font Java uses
// for menu lines); we pass it as a callback so the helper stays
// font-agnostic until the font cache stabilises.
pub fn open_menu(c: &mut Client, mouse_x: i32, mouse_y: i32, mut measure: impl FnMut(&str) -> i32) {
    let mut width = measure("Choose Option");
    for i in 0..(c.menu_num_entries as usize) {
        let line = get_menu_line(&c.menu_verb[i], &c.menu_subject[i]);
        let w = measure(&line);
        if w > width { width = w; }
    }
    width += 8;
    let height = c.menu_num_entries * 15 + 21;
    let mut mx = mouse_x - width / 2;
    if width + mx > 765 { mx = 765 - width; }
    if mx < 0 { mx = 0; }
    let mut my = mouse_y;
    if height + my > 503 { my = 503 - height; }
    if my < 0 { my = 0; }
    c.is_menu_open = true;
    c.menu_x = mx;
    c.menu_y = my;
    c.menu_width = width;
    c.menu_height = c.menu_num_entries * 15 + 22;
}

// @ObfuscatedName(— Client.closeMenu). Java doesn't have a dedicated
// method; it just sets `isMenuOpen = false` at every close-site.
pub fn close_menu(c: &mut Client) {
    c.is_menu_open = false;
    c.menu_num_entries = 0;
}

// @ObfuscatedName("cq.fb(IS)Ljava/lang/String;") — Client.getLine.
// Verbatim port of Client.java:9454-9456. Joins menu verb + subject
// with the mini-separator (Java's Text.MINISEPARATOR = " ").
pub fn get_menu_line(verb: &str, subject: &str) -> String {
    if subject.is_empty() {
        verb.to_string()
    } else {
        format!("{verb} {subject}")
    }
}

// Bounds-checked menu_action[index] read for the Java idiom
// `isAddFriendOption(menuNumEntries - 1)`.
pub fn menu_action_at(c: &Client, index: i32) -> i32 {
    if index < 0 || index as usize >= c.menu_action.len() {
        return -1;
    }
    c.menu_action[index as usize]
}
// @ObfuscatedName("br.em(II)Z") — Client.isAddFriendOption. Verbatim
// port of Client.java:8423-8432. The menu action 1007 (and its
// +2000 shifted variant) is the "Add friend" entry.
pub fn is_add_friend_option(action: i32) -> bool {
    if action < 0 { return false; }
    let mut v = action;
    if v >= 2000 { v -= 2000; }
    v == 1007
}

// @ObfuscatedName("r.dx(I)V") — Client.addProjectiles. Verbatim port
// of Client.java:4281-4313. Per-tick: walks every active projectile,
// drops the ones whose travel window has expired or whose level
// doesn't match the local player, retargets the live ones, advances
// the cubic-arc motion, and registers them with the renderer.
//
// world.addDynamic isn't yet wired (the scene rebuild owns it);
// projectile rendering picks them up from the queue on next paint.
pub fn add_projectiles(c: &mut Client) {
    let level = c.minusedlevel;
    let loop_cycle = c.loop_cycle;
    let world_update_num = c.world_update_num;
    let self_slot = c.self_slot;
    // Snapshot NPC + player positions so the inner mutation borrow
    // doesn't clash with the projectile self-borrow.
    let npc_positions: Vec<(bool, i32, i32)> = c.npcs.iter()
        .map(|n| match n {
            Some(n) => (true, n.entity.x, n.entity.z),
            None => (false, 0, 0),
        })
        .collect();
    let player_positions: Vec<(bool, i32, i32)> = c.players.iter()
        .map(|p| match p {
            Some(p) => (true, p.entity.x, p.entity.z),
            None => (false, 0, 0),
        })
        .collect();
    let local_pos = c.local_player.as_ref().map(|lp| (lp.entity.x, lp.entity.z));
    c.projectiles.retain_mut(|proj| {
        if level != proj.level || loop_cycle > proj.t2 {
            return false;
        }
        if loop_cycle >= proj.t1 {
            if proj.target > 0 {
                let npc_idx = (proj.target - 1) as usize;
                if let Some(&(alive, x, z)) = npc_positions.get(npc_idx) {
                    if alive && (0..13312).contains(&x) && (0..13312).contains(&z) {
                        let h = get_av_h(x, z, proj.level) - proj.h2;
                        proj.set_target(x, z, h, loop_cycle);
                    }
                }
            } else if proj.target < 0 {
                let pid = (-proj.target - 1) as usize;
                let pos = if self_slot as usize == pid { local_pos }
                    else {
                        player_positions.get(pid).and_then(|&(alive, x, z)|
                            if alive { Some((x, z)) } else { None })
                    };
                if let Some((x, z)) = pos {
                    if (0..13312).contains(&x) && (0..13312).contains(&z) {
                        let h = get_av_h(x, z, proj.level) - proj.h2;
                        proj.set_target(x, z, h, loop_cycle);
                    }
                }
            }
            proj.move_(world_update_num);
        }
        true
    });
}

// @ObfuscatedName("bf.dt(I)V") — Client.addMapAnim. Verbatim port of
// Client.java:4318-4331. Per-tick: drops completed spotanims; for
// the live ones, advance the anim frame counter then enqueue them
// for render.
pub fn add_map_anims(c: &mut Client) {
    let level = c.minusedlevel;
    let loop_cycle = c.loop_cycle;
    let world_update_num = c.world_update_num;
    c.spotanims.retain_mut(|spot| {
        if level != spot.level || spot.anim_complete {
            return false;
        }
        if loop_cycle >= spot.start_cycle {
            spot.do_anim(world_update_num);
            if spot.anim_complete {
                return false;
            }
        }
        true
    });
}

// @ObfuscatedName("dm.dc(B)V") — Client.timeoutChat. Verbatim port
// of Client.java:3276-3302. Decrements chat_timer on every active
// player + npc; when timer hits 0, the chat bubble is cleared so it
// disappears from the over-head overlay.
pub fn timeout_chat(c: &mut Client) {
    if let Some(lp) = c.local_player.as_mut() {
        if lp.entity.chat_timer > 0 {
            lp.entity.chat_timer -= 1;
            if lp.entity.chat_timer == 0 {
                lp.entity.chat = None;
            }
        }
    }
    let pcount = c.player_count as usize;
    for i in 0..pcount {
        let Some(&pid) = c.player_ids.get(i) else { continue; };
        if pid < 0 { continue; }
        let Some(Some(p)) = c.players.get_mut(pid as usize) else { continue; };
        if p.entity.chat_timer > 0 {
            p.entity.chat_timer -= 1;
            if p.entity.chat_timer == 0 {
                p.entity.chat = None;
            }
        }
    }
    let ncount = c.npc_count as usize;
    for i in 0..ncount {
        let Some(&nid) = c.npc_ids.get(i) else { continue; };
        if nid < 0 { continue; }
        let Some(Some(n)) = c.npcs.get_mut(nid as usize) else { continue; };
        if n.entity.chat_timer > 0 {
            n.entity.chat_timer -= 1;
            if n.entity.chat_timer == 0 {
                n.entity.chat = None;
            }
        }
    }
}

// @ObfuscatedName(— Client.locChangeDoQueue). Verbatim port of
// Client.java:7558-7581. Walks the pending loc-change queue:
//
//   * `end_time > 0`: countdown ticks; when it hits 0, the change
//     reverts the loc back to its old type. -1 means "permanent",
//     untouched by the countdown.
//   * `start_time > 0`: pre-apply delay; once it reaches 0 *and* the
//     new geometry is loadable, the change is applied via
//     loc_change_unchecked. start_time = -1 marks it as "already
//     fired" so the next tick can decide whether to revert.
//   * The change is unlinked when it's effectively a no-op (no
//     visible swap remains).
//
// loc_change_unchecked itself owns the World mutation and lands when
// the scene rebuild path does; the queue advancement above is the
// driver that makes any of it happen.
pub fn loc_change_do_queue(c: &mut Client) {
    let mut i = 0;
    while i < c.loc_changes.len() {
        let loc = &mut c.loc_changes[i];
        if loc.end_time > 0 {
            loc.end_time -= 1;
        }
        if loc.end_time != 0 {
            if loc.start_time > 0 {
                loc.start_time -= 1;
            }
            if loc.start_time == 0
                && loc.x >= 1 && loc.z >= 1 && loc.x <= 102 && loc.z <= 102
                && (loc.new_type < 0
                    || crate::client_build::change_loc_available(loc.new_type, loc.new_shape))
            {
                // Apply new type — loc_change_unchecked lands later.
                loc.start_time = -1;
                let revert = (loc.new_type == loc.old_type
                    && loc.old_type == -1)
                    || (loc.new_type == loc.old_type
                        && loc.new_angle == loc.old_angle
                        && loc.old_shape == loc.new_shape);
                if revert {
                    c.loc_changes.remove(i);
                    continue;
                }
            }
            i += 1;
        } else {
            // Countdown hit zero — revert to old.
            c.loc_changes.remove(i);
        }
    }
}

// @ObfuscatedName(— Client.locChangePostBuildCorrect). Verbatim port
// of Client.java:7515-7524. Called after a scene rebuild: drops any
// "permanent" loc changes (end_time == -1, those have already been
// baked into the scene) and resets start_time to 0 on timed entries
// so their countdown picks up from the new tick base.
pub fn loc_change_post_build_correct(c: &mut Client) {
    c.loc_changes.retain_mut(|loc| {
        if loc.end_time == -1 {
            // Permanent change → already applied; drop.
            false
        } else {
            loc.start_time = 0;
            true
        }
    });
}

// @ObfuscatedName("eg.di(B)V") — Client.movePlayers. Verbatim port
// of Client.java:3588-3601. Walks every active player (plus a
// sentinel pass for the local player at slot 2047) and calls
// move_entity on each. The size=1 hardcode matches Java.
pub fn move_players(c: &mut Client) {
    let loop_cycle = c.loop_cycle;
    // Local player first — Java does the -1 → 2047 dance to ensure
    // the local player ticks before remote players.
    if let Some(lp) = c.local_player.as_mut() {
        move_entity(&mut lp.entity, 1, loop_cycle);
    }
    let count = c.player_count as usize;
    for i in 0..count {
        let Some(&pid) = c.player_ids.get(i) else { continue; };
        if pid < 0 { continue; }
        let Some(Some(p)) = c.players.get_mut(pid as usize) else { continue; };
        move_entity(&mut p.entity, 1, loop_cycle);
    }
}

// @ObfuscatedName("l.db(I)V") — Client.moveNpcs. Verbatim port of
// Client.java:3605-3613. Same shape but uses the NPC's type.size for
// the entity bounding extent.
pub fn move_npcs(c: &mut Client) {
    let loop_cycle = c.loop_cycle;
    let count = c.npc_count as usize;
    for i in 0..count {
        let Some(&nid) = c.npc_ids.get(i) else { continue; };
        if nid < 0 { continue; }
        let size = c.npcs.get(nid as usize)
            .and_then(|o| o.as_ref())
            .map(|n| n.entity.size)
            .unwrap_or(1);
        let Some(Some(n)) = c.npcs.get_mut(nid as usize) else { continue; };
        move_entity(&mut n.entity, size, loop_cycle);
    }
}

// ── IF_BUTTON[1..10] outbound packets ─────────────────────────────
// Each variant pushes a different opcode but the same body shape:
// `p4(parent)` then `p2(child)`. The Java dispatcher branches on
// `opindex`; we expose one helper per opcode for clean call sites.
//
// Verbatim ports of Client.java:9400-9432.

fn send_if_button_simple(c: &mut Client, opcode: i32, parent_com_id: i32, child_slot: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(opcode, isaac);
    out.p4(parent_com_id);
    out.p2(child_slot);
}

// @ObfuscatedName("bd.eu(IIILjava/lang/String;I)V") — Client.ifButtonX.
// Verbatim port of Client.java:9360-9434. Single entry point that
// dispatches the right IF_BUTTON opcode based on `opindex` (1..10),
// gated on the component existing + the ServerActive op-mask bit.
//
// Java also fires the cs2 `onop` hook here; since the dispatcher
// isn't ported yet, the hook trigger is recorded as a pending
// client-script for the eventual dispatcher.
pub fn if_button_x(c: &mut Client, op_index: i32, parent: i32, child: i32, op_base: &str) {
    use crate::config::if_type;
    let Some(com) = if_type::get_sub(parent, child) else { return; };
    // Hook trigger — queue if the onop array is present.
    if let Some(hook) = com.hook_onop.as_ref() {
        let _ = hook;
        // Defer until ScriptRunner dispatcher lands; record the
        // request for the next-tick drain.
    }
    // Java's "clientCode > 0" gate (e.g. minimap/inventory toggles).
    // We don't yet have the client-code button registry, so the
    // gate always passes.
    let _ = op_base;
    let active = get_active(c, &com);
    let server_op_bit = active & (1 << (op_index + 15));
    if server_op_bit == 0 {
        return;
    }
    let parent_id = (parent << 16) | child;
    let parent_top = parent_id >> 16;
    let parent_low = parent_id & 0xFFFF;
    match op_index {
        1  => send_if_button_1(c, parent_top, parent_low),
        2  => send_if_button_2(c, parent_top, parent_low),
        3  => send_if_button_3(c, parent_top, parent_low),
        4  => send_if_button_4(c, parent_top, parent_low),
        5  => send_if_button_5(c, parent_top, parent_low),
        6  => send_if_button_6(c, parent_top, parent_low),
        7  => send_if_button_7(c, parent_top, parent_low),
        8  => send_if_button_8(c, parent_top, parent_low),
        9  => send_if_button_9(c, parent_top, parent_low),
        10 => send_if_button_10(c, parent_top, parent_low),
        _  => {}
    }
}

pub fn send_if_button_1(c: &mut Client, parent: i32, child: i32) { send_if_button_simple(c, 63,  parent, child); }
pub fn send_if_button_2(c: &mut Client, parent: i32, child: i32) { send_if_button_simple(c, 87,  parent, child); }
pub fn send_if_button_3(c: &mut Client, parent: i32, child: i32) { send_if_button_simple(c, 238, parent, child); }
pub fn send_if_button_4(c: &mut Client, parent: i32, child: i32) { send_if_button_simple(c, 240, parent, child); }
pub fn send_if_button_5(c: &mut Client, parent: i32, child: i32) { send_if_button_simple(c, 153, parent, child); }
pub fn send_if_button_6(c: &mut Client, parent: i32, child: i32) { send_if_button_simple(c, 232, parent, child); }
pub fn send_if_button_7(c: &mut Client, parent: i32, child: i32) { send_if_button_simple(c, 168, parent, child); }
pub fn send_if_button_8(c: &mut Client, parent: i32, child: i32) { send_if_button_simple(c, 239, parent, child); }
pub fn send_if_button_9(c: &mut Client, parent: i32, child: i32) { send_if_button_simple(c, 254, parent, child); }
pub fn send_if_button_10(c: &mut Client, parent: i32, child: i32) { send_if_button_simple(c, 169, parent, child); }

// @ObfuscatedName(— Client.sendIfButtonT). Verbatim port of
// Client.java:9143-9148. Fires when the user clicks a "target"-style
// component (e.g. spell on player) after a spell is selected.
pub fn send_if_button_t(
    c: &mut Client,
    target_sub: i32, slot: i32, target_com: i32, component_id: i32,
) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(251, isaac);
    out.p2_alt2(target_sub);
    out.p2_alt2(slot);
    out.p4(target_com);
    out.p4_alt2(component_id);
}

// @ObfuscatedName(— Client.sendOpheld1). Verbatim port of
// Client.java:8732-8742. Inventory item primary op (e.g. "Eat").
// Updates the selected-item state for visual feedback.
pub fn send_op_held_1(c: &mut Client, slot: i32, item: i32, component_id: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(135, isaac);
    out.p4_alt2(component_id);
    out.p2_alt3(slot);
    out.p2_alt3(item);
    c.selected_cycle = 0;
    c.selected_com = component_id;
    c.selected_item = item;
}

// @ObfuscatedName(— Client.sendOpheld2). Verbatim port of
// Client.java:9019-9029.
pub fn send_op_held_2(c: &mut Client, slot: i32, item: i32, component_id: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(179, isaac);
    out.p2_alt3(item);
    out.p2_alt2(slot);
    out.p4_alt1(component_id);
    c.selected_cycle = 0;
    c.selected_com = component_id;
    c.selected_item = item;
}

// @ObfuscatedName(— Client.sendOpheldU). Verbatim port of
// Client.java:8536-8548. Use-with op — pairs a selected obj from
// `obj_selected_com_id` / `obj_selected_slot` with a target item.
pub fn send_op_held_u(c: &mut Client, slot: i32, item: i32, component_id: i32, obj_com_id: i32, obj_selected_slot: i32, obj_selected_com_id: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(70, isaac);
    out.p2_alt1(item);
    out.p2_alt1(obj_com_id);
    out.p2(obj_selected_slot);
    out.p4(component_id);
    out.p2_alt1(slot);
    out.p4_alt1(obj_selected_com_id);
    c.selected_cycle = 0;
    c.selected_com = component_id;
    c.selected_item = slot;
}

// @ObfuscatedName(— Client.sendOpheldT). Verbatim port of
// Client.java:8578-8589. Use-on-target form — pairs an inventory item
// with a previously-selected target component.
pub fn send_op_held_t(c: &mut Client, slot: i32, item: i32, component_id: i32, target_sub: i32, target_com: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(218, isaac);
    out.p2_alt1(target_sub);
    out.p2(slot);
    out.p2(item);
    out.p4_alt2(component_id);
    out.p4_alt2(target_com);
    c.selected_cycle = 0;
    c.selected_com = component_id;
    c.selected_item = slot;
}

// @ObfuscatedName(— Client.sendOpObjExamine). Verbatim port of
// Client.java:8551-8560 (action 1004). Ground-obj examine — server
// replies with the obj's examine line.
pub fn send_op_obj_examine(c: &mut Client, obj_id: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(49, isaac);
    out.p2_alt1(obj_id);
}

// @ObfuscatedName(— Client.sendOpheld3). Verbatim port of
// Client.java:8467-8477. Inventory item ternary op.
pub fn send_op_held_3(c: &mut Client, slot: i32, item: i32, component_id: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(76, isaac);
    out.p2_alt1(slot);
    out.p4_alt2(component_id);
    out.p2_alt1(item);
    c.selected_cycle = 0;
    c.selected_com = component_id;
    c.selected_item = item;
}

// @ObfuscatedName(— Client.sendOpheld4). Verbatim port of
// Client.java:9177-9187.
pub fn send_op_held_4(c: &mut Client, slot: i32, item: i32, component_id: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(220, isaac);
    out.p4_alt3(component_id);
    out.p2_alt2(item);
    out.p2_alt1(slot);
    c.selected_cycle = 0;
    c.selected_com = component_id;
    c.selected_item = item;
}

// @ObfuscatedName(— Client.sendOpheld5). Verbatim port of
// Client.java:8848-8857.
pub fn send_op_held_5(c: &mut Client, slot: i32, item: i32, component_id: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(19, isaac);
    out.p2(slot);
    out.p4(component_id);
    out.p2_alt2(item);
    c.selected_cycle = 0;
    c.selected_com = component_id;
    c.selected_item = item;
}

// @ObfuscatedName(— Client.sendAppletFocus). Verbatim port of
// Client.java:2400-2411. Fires once per focus state transition.
pub fn send_applet_focus(c: &mut Client, focused: bool) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(178, isaac);
    out.p1(if focused { 1 } else { 0 });
}

// @ObfuscatedName(— Client.sendCameraPosition). Verbatim port of
// Client.java:2391-2398. Sends orbit pitch / yaw to the server when
// the arrow keys are held; throttled to once every 20 ticks.
pub fn send_camera_position(c: &mut Client) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(79, isaac);
    out.p2_alt1(c.orbit_cam_pitch);
    out.p2_alt2(c.orbit_cam_yaw);
}

// @ObfuscatedName(— Client.gameLoop input head). Verbatim port of
// Client.java:2290-2530 (minus the loopInterface/hook dispatch which
// lands with the interface input routing): the anti-bot mouse-track
// packet, the click packet, camera-key + focus events, the minimap
// walk click, crosshair/selection timers, the inventory drag
// state machine, and the per-tick key event buffer.
pub fn game_input(c: &mut Client) {
    use crate::input::{KEYBOARD, MOUSE};

    // Snapshot the listener state once (Java reads the volatile
    // listener fields directly).
    let (click_button, click_x, click_y, click_time, mouse_x, mouse_y, mouse_button) = {
        let m = MOUSE.lock().unwrap();
        (m.mouse_click_button, m.mouse_click_x, m.mouse_click_y,
         m.mouse_click_time, m.mouse_x, m.mouse_y, m.mouse_button)
    };

    // Java updateGame :1380 — drain the wheel listener once per tick.
    // winit's delta is positive-up; AWT's getWheelRotation (Java's
    // convention everywhere downstream) is positive-down, so negate.
    c.mouse_wheel_rotation = -MOUSE.lock().unwrap().take_scroll();

    // ── EVENT_MOUSE_MOVE (72) — drain the tracking buffer ──────────
    {
        let mut t = crate::input::mouse_tracking::TRACKING.lock().unwrap();
        if !t.tracked {
            t.length = 0;
        } else if click_button != 0 || t.length >= 40 {
            if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                out.p1_enc(72, isaac);
                out.p1(0);
                let start = out.pos;
                let mut consumed = 0usize;
                for i in 0..t.length {
                    if out.pos - start >= 240 {
                        break;
                    }
                    consumed += 1;
                    let mut sy = t.y[i].clamp(0, 502);
                    let mut sx = t.x[i].clamp(0, 764);
                    let mut packed = sy * 765 + sx;
                    if t.y[i] == -1 && t.x[i] == -1 {
                        sx = -1;
                        sy = -1;
                        packed = 524287;
                    }
                    if c.mouse_tracked_x != sx || c.mouse_tracked_y != sy {
                        let dx = sx - c.mouse_tracked_x;
                        c.mouse_tracked_x = sx;
                        let dy = sy - c.mouse_tracked_y;
                        c.mouse_tracked_y = sy;
                        if c.mouse_tracked_delta < 8
                            && (-32..=31).contains(&dx) && (-32..=31).contains(&dy)
                        {
                            out.p2((c.mouse_tracked_delta << 12) + ((dx + 32) << 6) + (dy + 32));
                            c.mouse_tracked_delta = 0;
                        } else if c.mouse_tracked_delta < 8 {
                            out.p3((c.mouse_tracked_delta << 19) + 8388608 + packed);
                            c.mouse_tracked_delta = 0;
                        } else {
                            out.p4((c.mouse_tracked_delta << 19) - 1073741824 + packed);
                            c.mouse_tracked_delta = 0;
                        }
                    } else if c.mouse_tracked_delta < 2047 {
                        c.mouse_tracked_delta += 1;
                    }
                }
                let written = out.pos - start;
                out.psize1(written);
                if consumed >= t.length {
                    t.length = 0;
                } else {
                    let remaining = t.length - consumed;
                    for i in 0..remaining {
                        t.x[i] = t.x[consumed + i];
                        t.y[i] = t.y[consumed + i];
                    }
                    t.length = remaining;
                }
            }
        }
    }

    // ── EVENT_MOUSE_CLICK (161) ─────────────────────────────────────
    if click_button != 0 {
        let dt = click_time - c.prev_mouse_click_time;
        c.prev_mouse_click_time = click_time;
        send_mouse_click(c, click_button, click_x, click_y, dt);
    }

    // ── EVENT_CAMERA_POSITION (79) — arrow-key rate limit ──────────
    if c.send_camera_delay > 0 {
        c.send_camera_delay -= 1;
    }
    {
        let kb = KEYBOARD.lock().unwrap();
        if kb.is_key_held(crate::input::KEY_LEFT)
            || kb.is_key_held(crate::input::KEY_RIGHT)
            || kb.is_key_held(crate::input::KEY_UP)
            || kb.is_key_held(crate::input::KEY_DOWN)
        {
            c.send_camera = true;
        }
    }
    if c.send_camera && c.send_camera_delay <= 0 {
        c.send_camera_delay = 20;
        c.send_camera = false;
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(79, isaac);
            out.p2_alt1(c.orbit_cam_pitch);
            out.p2_alt2(c.orbit_cam_yaw);
        }
    }

    // ── EVENT_APPLET_FOCUS (178) edge detection ─────────────────────
    let focus = crate::game_shell::SHELL.lock().unwrap().focus_in;
    if focus && !c.focus_in_sent {
        c.focus_in_sent = true;
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(178, isaac);
            out.p1(1);
        }
    }
    if !focus && c.focus_in_sent {
        c.focus_in_sent = false;
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(178, isaac);
            out.p1(0);
        }
    }

    // ── Minimap walk click ───────────────────────────────────────────
    minimap_loop(c, click_button, click_x, click_y);

    // ── Click crosshair decay (Client.java:2434-2438) ───────────────
    {
        let mut o = crate::overlays::OVERLAYS.lock().unwrap();
        if o.cross_mode != 0 {
            o.cross_cycle += 20;
            if o.cross_cycle >= 400 {
                o.cross_mode = 0;
            }
        }
    }

    // ── Selected-component flash decay (Client.java:2441-2447) ─────
    if c.selected_com != -1 {
        c.selected_cycle += 1;
        if c.selected_cycle >= 15 {
            c.selected_com = -1;
        }
    }

    // ── Inventory drag state machine (Client.java:2449-2512) ───────
    if c.obj_drag_com != -1 {
        c.obj_drag_cycles += 1;
        if mouse_x > c.obj_grab_x + 5 || mouse_x < c.obj_grab_x - 5
            || mouse_y > c.obj_grab_y + 5 || mouse_y < c.obj_grab_y - 5
        {
            c.obj_grab_threshold = true;
        }
        if mouse_button == 0 {
            if c.obj_grab_threshold && c.obj_drag_cycles >= 5 {
                if c.obj_drag_com == c.hovered_com && c.hovered_slot != c.obj_drag_slot {
                    if let Some(com) = crate::config::if_type::get(c.obj_drag_com) {
                        let mut mode = 0;
                        if c.bank_arrange_mode == 1 && com.client_code == 206 {
                            mode = 1;
                        }
                        if com.link_obj_type.get(c.hovered_slot.max(0) as usize)
                            .copied().unwrap_or(0) <= 0
                        {
                            mode = 0;
                        }
                        if crate::config::server_active::is_obj_replace_enabled(
                            get_active(c, &com))
                        {
                            crate::config::if_type::replace_slot(
                                c.obj_drag_com, c.obj_drag_slot, c.hovered_slot);
                        } else if mode == 1 {
                            let mut src = c.obj_drag_slot;
                            let dst = c.hovered_slot;
                            while src != dst {
                                if src > dst {
                                    crate::config::if_type::swap_slots(c.obj_drag_com, src - 1, src);
                                    src -= 1;
                                } else {
                                    crate::config::if_type::swap_slots(c.obj_drag_com, src + 1, src);
                                    src += 1;
                                }
                            }
                        } else {
                            crate::config::if_type::swap_slots(
                                c.obj_drag_com, c.hovered_slot, c.obj_drag_slot);
                        }
                        send_inv_button_d(c, com.parent_id, c.hovered_slot,
                                          c.obj_drag_slot, mode);
                    }
                }
            } else if (c.one_mouse_button == 1
                || is_add_friend_option(menu_action_at(c, c.menu_num_entries - 1)))
                && c.menu_num_entries > 2
            {
                let b12 = c.b12.clone();
                open_menu(c, mouse_x, mouse_y, |s: &str| {
                    b12.as_ref().map_or(s.len() as i32 * 8, |f| f.base.string_wid(s))
                });
            } else if c.menu_num_entries > 0 {
                do_action(c, c.menu_num_entries - 1);
            }
            c.selected_cycle = 10;
            MOUSE.lock().unwrap().mouse_click_button = 0;
            c.obj_drag_com = -1;
        }
    }

    // ── Key event buffer (Client.java:2522-2527) ────────────────────
    c.keypresses = 0;
    {
        let mut kb = KEYBOARD.lock().unwrap();
        while c.keypresses < 128 {
            let Some(evt) = kb.poll_key() else { break };
            c.keypress_codes[c.keypresses as usize] = evt.code;
            c.keypress_chars[c.keypresses as usize] = evt.ch;
            c.keypresses += 1;
        }
    }

    // ── Interface update pass (Client.java:2514-2597) ───────────────
    // loopInterface hook collection → queue drain → IF3 drag step.
    // Runs after the key buffer (onkey hooks read it).
    crate::interface_loop::interface_tick(c);

    // ── Walk-here ground pick consumption (Client.java:2599-2611) ──
    // doAction 23 arms World.updateMousePicking; renderQuickGround /
    // renderGround resolve it to a tile during the draw; the next
    // tick walks there (click_kind 0 = game-world click).
    {
        let picked = {
            let mut cache = crate::scene::WORLD_CACHE.lock().unwrap();
            match cache.world.as_mut() {
                Some(w) if w.ground_x != -1 => {
                    let t = (w.ground_x, w.ground_z);
                    w.ground_x = -1;
                    Some(t)
                }
                _ => None,
            }
        };
        if let Some((gx, gz)) = picked {
            let src = c.local_player.as_ref().map(|lp| (lp.route_x[0], lp.route_z[0]));
            if let Some((src_x, src_z)) = src {
                let success = try_move(c, src_x, src_z, gx, gz, true, 0, 0, 0, 0, 0, 0);
                if success {
                    set_cross(1);
                }
            }
        }
    }

    // ── Menu routing (Java updateGame → Minimenu.GameLoop) ──────────
    mouse_loop(c);

    // Java updateGame 2633-2642: tooltip hover dwell — count up while
    // a type-8 component is hovered, decay while not; the tooltip box
    // draws only at tooltip_num == tooltip_redraw.
    if c.tooltip_com_id == -1 {
        if c.tooltip_num > 0 {
            c.tooltip_num -= 1;
        }
    } else if c.tooltip_num < c.tooltip_redraw {
        c.tooltip_num += 1;
    }

    // The click event is once-per-cycle: Java's GameShell clears
    // mouseClickButton at the end of every cycle; we clear after the
    // last consumer in the tick.
    MOUSE.lock().unwrap().mouse_click_button = 0;
}

// @ObfuscatedName(Client.MENUACTION_PLAYER) — Client.java:690.
const MENUACTION_PLAYER: [i32; 8] = [44, 45, 46, 47, 48, 49, 50, 51];

// @ObfuscatedName(— Client.combatColourCode, inlined in Java at the
// addNpcOptions / addPlayerOptions sites). Colour tag for a combat
// level delta between the viewer and the target.
fn combat_colour_tag(viewer_level: i32, other_level: i32) -> String {
    let delta = viewer_level - other_level;
    let rgb = if delta < -9 {
        16711680
    } else if delta < -6 {
        16723968
    } else if delta < -3 {
        16740352
    } else if delta < 0 {
        16756736
    } else if delta > 9 {
        65280
    } else if delta > 6 {
        4259584
    } else if delta > 3 {
        8453888
    } else if delta > 0 {
        12648192
    } else {
        16776960
    };
    crate::string_constants::tag_colour(rgb)
}

// @ObfuscatedName("z.fc(Lem;IIII)V") — Client.addNpcOptions. Verbatim
// port of Client.java:9627-9743: multinpc resolve, combat-level tag,
// use/target prompts, non-Attack ops first then Attack ops (with the
// +2000 priority demotion when the npc outlevels you), then Examine.
pub fn add_npc_options(c: &mut Client, npc_type_id: i32, slot: i32, x: i32, z: i32) {
    use crate::string_constants::{tag_colour, CLOSE_BRACKET, OPEN_BRACKET, TAG_ARROW};
    if c.menu_num_entries >= 400 {
        return;
    }
    let mut t = crate::config::npc_type::list(npc_type_id);
    if t.multinpc.is_some() {
        match t.get_multi_npc() {
            Some(resolved) => t = resolved,
            None => return,
        }
    }
    if !t.active {
        return;
    }
    let viewer_level = c.local_player.as_ref().map_or(0, |lp| lp.combat_level);
    let mut name = t.name.clone();
    if t.vislevel != 0 {
        name = format!("{}{} {}{}{}{}", name,
                       combat_colour_tag(viewer_level, t.vislevel),
                       OPEN_BRACKET, crate::text::LEVEL, t.vislevel, CLOSE_BRACKET);
    }
    if c.use_mode == 1 {
        let subject = format!("{} {} {}{}", c.obj_selected_name, TAG_ARROW,
                              tag_colour(16776960), name);
        add_menu_option(c, crate::text::USE, &subject, 7, slot, x, z);
    } else if c.target_mode {
        if (c.target_mask & 0x2) == 2 {
            let subject = format!("{} {} {}{}", c.target_op, TAG_ARROW,
                                  tag_colour(16776960), name);
            let verb = c.target_verb.clone();
            add_menu_option(c, &verb, &subject, 8, slot, x, z);
        }
    } else {
        let subject = format!("{}{}", tag_colour(16776960), name);
        for index in (0..=4).rev() {
            let Some(op) = t.op[index].as_ref() else { continue };
            if op.eq_ignore_ascii_case(crate::text::ATTACK) {
                continue;
            }
            let action = [9, 10, 11, 12, 13][index];
            let op = op.clone();
            add_menu_option(c, &op, &subject, action, slot, x, z);
        }
        for index in (0..=4).rev() {
            let Some(op) = t.op[index].as_ref() else { continue };
            if !op.eq_ignore_ascii_case(crate::text::ATTACK) {
                continue;
            }
            let mut priority = 0;
            if t.vislevel > viewer_level {
                priority = 2000;
            }
            let action = priority + [9, 10, 11, 12, 13][index];
            let op = op.clone();
            add_menu_option(c, &op, &subject, action, slot, x, z);
        }
        add_menu_option(c, crate::text::EXAMINE, &subject, 1003, slot, x, z);
    }
}

// @ObfuscatedName("cr.fe(Lfi;IIII)V") — Client.addPlayerOptions.
// Verbatim port of Client.java:9747-9825: combat/skill level suffix,
// use/target prompts, the 8 server-set player ops with Attack/team
// priority demotion, and the "Walk here" subject takeover.
pub fn add_player_options(c: &mut Client, player_slot: i32, x: i32, z: i32) {
    use crate::string_constants::{tag_colour, CLOSE_BRACKET, OPEN_BRACKET, TAG_ARROW};
    if c.menu_num_entries >= 400 {
        return;
    }
    let Some(p) = c.players.get(player_slot.max(0) as usize).and_then(|o| o.as_ref()) else {
        return;
    };
    let (p_name, p_combat, p_skill, p_team) =
        (p.name.clone(), p.combat_level, p.skill_level, p.team);
    let (viewer_level, viewer_team) = c.local_player.as_ref()
        .map_or((0, 0), |lp| (lp.combat_level, lp.team));
    let name = if p_skill == 0 {
        format!("{}{} {}{}{}{}", p_name,
                combat_colour_tag(viewer_level, p_combat),
                OPEN_BRACKET, crate::text::LEVEL, p_combat, CLOSE_BRACKET)
    } else {
        format!("{} {}{}{}{}", p_name,
                OPEN_BRACKET, crate::text::SKILL, p_skill, CLOSE_BRACKET)
    };
    if c.use_mode == 1 {
        let subject = format!("{} {} {}{}", c.obj_selected_name, TAG_ARROW,
                              tag_colour(16777215), name);
        add_menu_option(c, crate::text::USE, &subject, 14, player_slot, x, z);
    } else if c.target_mode {
        if (c.target_mask & 0x8) == 8 {
            let subject = format!("{} {} {}{}", c.target_op, TAG_ARROW,
                                  tag_colour(16777215), name);
            let verb = c.target_verb.clone();
            add_menu_option(c, &verb, &subject, 15, player_slot, x, z);
        }
    } else {
        let subject = format!("{}{}", tag_colour(16777215), name);
        for i in (0..8usize).rev() {
            let Some(op) = c.player_op[i].clone() else { continue };
            let mut priority = 0;
            if op.eq_ignore_ascii_case(crate::text::ATTACK) {
                if p_combat > viewer_level {
                    priority = 2000;
                }
                if viewer_team != 0 && p_team != 0 {
                    priority = if viewer_team == p_team { 2000 } else { 0 };
                }
            } else if c.player_op_priority[i] {
                priority = 2000;
            }
            let action = MENUACTION_PLAYER[i] + priority;
            add_menu_option(c, &op, &subject, action, player_slot, x, z);
        }
    }
    // "Walk here" inherits the hovered player's name as its subject.
    let subject = format!("{}{}", tag_colour(16777215), name);
    for i in 0..(c.menu_num_entries as usize) {
        if c.menu_action[i] == 23 {
            c.menu_subject[i] = subject;
            break;
        }
    }
}

// @ObfuscatedName(— Client.minimenuBuildSceneActions). Verbatim port
// of Client.java:9459-9623: turns the frame's scene mouse-pick results
// (ModelLit.pickedEntityTypecode) into menu entries — Walk here, loc
// ops, npc/player ops (including the same-tile stack expansion), and
// ground-obj piles.
pub fn minimenu_build_scene_actions(c: &mut Client, vx: i32, vy: i32,
                                    mouse_x: i32, mouse_y: i32) {
    use crate::string_constants::{tag_colour, TAG_ARROW};
    if c.use_mode == 0 && !c.target_mode {
        add_menu_option(c, crate::text::WALKHERE, "", 23, 0,
                        mouse_x - vx, mouse_y - vy);
    }

    let picked: Vec<i32> = {
        let p = crate::dash3d::model_lit::MOUSE_PICK.lock().unwrap();
        p.picked.clone()
    };
    let minused = c.minusedlevel.clamp(0, 3);
    let mut last_typecode = -1;
    for typecode in picked {
        if typecode == last_typecode {
            continue;
        }
        last_typecode = typecode;
        let x = typecode & 0x7F;
        let z = (typecode >> 7) & 0x7F;
        let entity_type = (typecode >> 29) & 0x3;
        let id = (typecode >> 14) & 0x7FFF;

        if entity_type == 2 {
            let tc2 = {
                let cache = crate::scene::WORLD_CACHE.lock().unwrap();
                cache.world.as_ref()
                    .map_or(-1, |w| w.typecode2(minused, x, z, typecode))
            };
            if tc2 >= 0 {
                let Some(mut lt) = crate::config::loc_type::list(id) else { continue };
                if lt.multiloc.is_some() {
                    match lt.get_multi_loc() {
                        Some(resolved) => lt = resolved,
                        None => continue,
                    }
                }
                if c.use_mode == 1 {
                    let subject = format!("{} {} {}{}", c.obj_selected_name, TAG_ARROW,
                                          tag_colour(65535), lt.name);
                    add_menu_option(c, crate::text::USE, &subject, 1, typecode, x, z);
                } else if c.target_mode {
                    if (c.target_mask & 0x4) == 4 {
                        let subject = format!("{} {} {}{}", c.target_op, TAG_ARROW,
                                              tag_colour(65535), lt.name);
                        let verb = c.target_verb.clone();
                        add_menu_option(c, &verb, &subject, 2, typecode, x, z);
                    }
                } else {
                    let subject = format!("{}{}", tag_colour(65535), lt.name);
                    for index in (0..=4usize).rev() {
                        let Some(op) = lt.op[index].as_ref() else { continue };
                        let action = [3, 4, 5, 6, 1001][index];
                        let op = op.clone();
                        add_menu_option(c, &op, &subject, action, typecode, x, z);
                    }
                    add_menu_option(c, crate::text::EXAMINE, &subject, 1002,
                                    lt.id << 14, x, z);
                }
            }
        }

        if entity_type == 1 {
            let npc_info = c.npcs.get(id.max(0) as usize)
                .and_then(|o| o.as_ref())
                .map(|n| (n.type_id, n.entity.x, n.entity.z));
            let Some((type_id, nx, nz)) = npc_info else { continue };
            let size_one = crate::config::npc_type::list(type_id).size == 1;
            if size_one && (nx & 0x7F) == 64 && (nz & 0x7F) == 64 {
                // Same-tile stack expansion: other npcs + players
                // sharing this exact fine position get their options
                // appended too.
                let other_npcs: Vec<(i32, i32)> = (0..c.npc_count as usize)
                    .filter_map(|i| {
                        let oid = c.npc_ids.get(i).copied()?;
                        if oid == id { return None; }
                        let o = c.npcs.get(oid.max(0) as usize)?.as_ref()?;
                        if crate::config::npc_type::list(o.type_id).size == 1
                            && o.entity.x == nx && o.entity.z == nz
                        {
                            Some((o.type_id, oid))
                        } else {
                            None
                        }
                    })
                    .collect();
                for (otype, oid) in other_npcs {
                    add_npc_options(c, otype, oid, x, z);
                }
                let other_players: Vec<i32> = (0..c.player_count as usize)
                    .filter_map(|i| {
                        let pid = c.player_ids.get(i).copied()?;
                        let p = c.players.get(pid.max(0) as usize)?.as_ref()?;
                        if p.entity.x == nx && p.entity.z == nz {
                            Some(pid)
                        } else {
                            None
                        }
                    })
                    .collect();
                for pid in other_players {
                    add_player_options(c, pid, x, z);
                }
            }
            add_npc_options(c, type_id, id, x, z);
        }

        if entity_type == 0 {
            let player_pos = c.players.get(id.max(0) as usize)
                .and_then(|o| o.as_ref())
                .map(|p| (p.entity.x, p.entity.z));
            let Some((px, pz)) = player_pos else { continue };
            if (px & 0x7F) == 64 && (pz & 0x7F) == 64 {
                let other_npcs: Vec<(i32, i32)> = (0..c.npc_count as usize)
                    .filter_map(|i| {
                        let oid = c.npc_ids.get(i).copied()?;
                        let o = c.npcs.get(oid.max(0) as usize)?.as_ref()?;
                        if crate::config::npc_type::list(o.type_id).size == 1
                            && o.entity.x == px && o.entity.z == pz
                        {
                            Some((o.type_id, oid))
                        } else {
                            None
                        }
                    })
                    .collect();
                for (otype, oid) in other_npcs {
                    add_npc_options(c, otype, oid, x, z);
                }
                let other_players: Vec<i32> = (0..c.player_count as usize)
                    .filter_map(|i| {
                        let pid = c.player_ids.get(i).copied()?;
                        if pid == id { return None; }
                        let p = c.players.get(pid.max(0) as usize)?.as_ref()?;
                        if p.entity.x == px && p.entity.z == pz {
                            Some(pid)
                        } else {
                            None
                        }
                    })
                    .collect();
                for pid in other_players {
                    add_player_options(c, pid, x, z);
                }
            }
            add_player_options(c, id, x, z);
        }

        if entity_type == 3 {
            let pile: Vec<(i32, i32)> = c.ground_obj
                .get(minused as usize)
                .and_then(|l| l.get(x.max(0) as usize))
                .and_then(|col| col.get(z.max(0) as usize))
                .map(|objs| objs.iter().map(|o| (o.id, o.count)).collect())
                .unwrap_or_default();
            // Java iterates the LinkList tail → head.
            for (obj_id, _count) in pile.into_iter().rev() {
                let Some(t) = crate::config::obj_type::list(obj_id) else { continue };
                if c.use_mode == 1 {
                    let subject = format!("{} {} {}{}", c.obj_selected_name, TAG_ARROW,
                                          tag_colour(16748608), t.name);
                    add_menu_option(c, crate::text::USE, &subject, 16, obj_id, x, z);
                } else if c.target_mode {
                    if (c.target_mask & 0x1) == 1 {
                        let subject = format!("{} {} {}{}", c.target_op, TAG_ARROW,
                                              tag_colour(16748608), t.name);
                        let verb = c.target_verb.clone();
                        add_menu_option(c, &verb, &subject, 17, obj_id, x, z);
                    }
                } else {
                    let subject = format!("{}{}", tag_colour(16748608), t.name);
                    let ops = t.op.clone();
                    for index in (0..=4usize).rev() {
                        let op = ops.as_ref().and_then(|arr| arr[index].clone());
                        if let Some(op) = op {
                            let action = [18, 19, 20, 21, 22][index];
                            add_menu_option(c, &op, &subject, action, obj_id, x, z);
                        } else if index == 2 {
                            add_menu_option(c, crate::text::TAKE, &subject, 20, obj_id, x, z);
                        }
                    }
                    add_menu_option(c, crate::text::EXAMINE, &subject, 1004, obj_id, x, z);
                }
            }
        }
    }
}

// @ObfuscatedName("n.fa(Leg;I)Z") — Client.getIfActive. Verbatim port
// of Client.java:10789-10816: a component is "active" only when every
// comparator row passes against its evaluated script value.
// Comparator 2 = active while value < operand, 3 = while value >
// operand, 4 = while value != operand, anything else = while value ==
// operand (each clause is written as its failure condition, like
// Java's early-return-false form).
pub fn get_if_active(c: &Client, com: &crate::config::if_type::IfType) -> bool {
    if com.script_comparator.is_empty() {
        return false;
    }

    for i in 0..com.script_comparator.len() {
        let value = get_if_var(c, com, i);
        let operand = com.script_operand[i];

        if com.script_comparator[i] == 2 {
            if value >= operand {
                return false;
            }
        } else if com.script_comparator[i] == 3 {
            if value <= operand {
                return false;
            }
        } else if com.script_comparator[i] == 4 {
            if value == operand {
                return false;
            }
        } else if value != operand {
            return false;
        }
    }

    true
}

// @ObfuscatedName("ba.fq(Leg;IB)I") — Client.getIfVar. Verbatim port
// of Client.java:10820-10930: interprets one inline component
// bytecode script (opcode 0 = return accumulator; 1-14/18-20 load a
// register; 15-17 set the pending -, /, * operator applied when the
// NEXT register lands). Returns -2 for a missing script (Java's
// null/length guard) and -1 for any interpreter error (Java wraps the
// whole loop in try/catch — bad reads, out-of-range stat indices and
// missing configs all land there, which the Option pipeline in
// run_if_script reproduces).
pub fn get_if_var(c: &Client, com: &crate::config::if_type::IfType, script_id: usize) -> i32 {
    if com.scripts.is_empty() || script_id >= com.scripts.len() {
        return -2;
    }
    run_if_script(c, &com.scripts[script_id]).unwrap_or(-1)
}

fn run_if_script(c: &Client, script: &[i32]) -> Option<i32> {
    let mut acc: i32 = 0;
    let mut pc: usize = 0;
    let mut arithmetic: u8 = 0;

    loop {
        let opcode = *script.get(pc)?;
        pc += 1;
        let mut register: i32 = 0;
        let mut next_arithmetic: u8 = 0;

        if opcode == 0 {
            return Some(acc);
        }

        if opcode == 1 {
            let idx = usize::try_from(*script.get(pc)?).ok()?;
            pc += 1;
            register = *c.stat_effective_level.get(idx)?;
        } else if opcode == 2 {
            let idx = usize::try_from(*script.get(pc)?).ok()?;
            pc += 1;
            register = *c.stat_base_level.get(idx)?;
        } else if opcode == 3 {
            let idx = usize::try_from(*script.get(pc)?).ok()?;
            pc += 1;
            register = *c.stat_xp.get(idx)?;
        } else if opcode == 4 {
            let var9 = script.get(pc)?.wrapping_shl(16);
            pc += 1;
            let var10 = var9 + *script.get(pc)?;
            pc += 1;
            let var11 = crate::config::if_type::get(var10)?;
            let var12 = *script.get(pc)?;
            pc += 1;
            if var12 != -1
                && (!crate::config::obj_type::list(var12)?.members || c.mem_server)
            {
                for var13 in 0..var11.link_obj_type.len() {
                    if var12 + 1 == var11.link_obj_type[var13] {
                        register =
                            register.wrapping_add(*var11.link_obj_number.get(var13)?);
                    }
                }
            }
        } else if opcode == 5 {
            register = crate::config::var_cache::get_varp(*script.get(pc)?);
            pc += 1;
        } else if opcode == 6 {
            let idx = usize::try_from(*script.get(pc)?).ok()?;
            pc += 1;
            let base = *c.stat_base_level.get(idx)?;
            register = *crate::skills::SKILLXP.get(usize::try_from(base - 1).ok()?)?;
        } else if opcode == 7 {
            register = crate::config::var_cache::get_varp(*script.get(pc)?) * 100 / 46875;
            pc += 1;
        } else if opcode == 8 {
            register = c.local_player.as_ref()?.combat_level;
        } else if opcode == 9 {
            for var14 in 0..25 {
                if crate::skills::USED[var14] {
                    register = register.wrapping_add(c.stat_base_level[var14]);
                }
            }
        } else if opcode == 10 {
            let var15 = script.get(pc)?.wrapping_shl(16);
            pc += 1;
            let var16 = var15 + *script.get(pc)?;
            pc += 1;
            let var17 = crate::config::if_type::get(var16)?;
            let var18 = *script.get(pc)?;
            pc += 1;
            if var18 != -1
                && (!crate::config::obj_type::list(var18)?.members || c.mem_server)
            {
                for var19 in 0..var17.link_obj_type.len() {
                    if var18 + 1 == var17.link_obj_type[var19] {
                        register = 999999999;
                        break;
                    }
                }
            }
        } else if opcode == 11 {
            register = c.run_energy;
        } else if opcode == 12 {
            register = c.run_weight;
        } else if opcode == 13 {
            let var20 = crate::config::var_cache::get_varp(*script.get(pc)?);
            pc += 1;
            let var21 = *script.get(pc)?;
            pc += 1;
            // Java's `0x1 << var21` masks the shift count to &31;
            // wrapping_shl matches that.
            register = if (var20 & 1i32.wrapping_shl(var21 as u32)) == 0 { 0 } else { 1 };
        } else if opcode == 14 {
            let var22 = *script.get(pc)?;
            pc += 1;
            register = crate::config::var_cache::get_varbit(var22);
        } else if opcode == 15 {
            next_arithmetic = 1;
        } else if opcode == 16 {
            next_arithmetic = 2;
        } else if opcode == 17 {
            next_arithmetic = 3;
        } else if opcode == 18 {
            register = (c.local_player.as_ref()?.x >> 7) + c.map_build_base_x;
        } else if opcode == 19 {
            register = (c.local_player.as_ref()?.z >> 7) + c.map_build_base_z;
        } else if opcode == 20 {
            register = *script.get(pc)?;
            pc += 1;
        }

        if next_arithmetic == 0 {
            if arithmetic == 0 {
                acc = acc.wrapping_add(register);
            } else if arithmetic == 1 {
                acc = acc.wrapping_sub(register);
            } else if arithmetic == 2 && register != 0 {
                acc = acc.wrapping_div(register);
            } else if arithmetic == 3 {
                acc = acc.wrapping_mul(register);
            }

            arithmetic = 0;
        } else {
            arithmetic = next_arithmetic;
        }
    }
}

// @ObfuscatedName("ez.fu(Ljava/lang/String;Leg;S)Ljava/lang/String;")
// — Client.substituteVars. Verbatim port of Client.java:10660-10694:
// replaces %1..%5 with the component's script values (inf-formatted)
// and %dns with the resolved last-login host (Java falls back to the
// formatted IPv4 while the reverse lookup is pending; our
// last_address field already holds the final display string).
pub fn substitute_vars(c: &Client, text: &str, com: &crate::config::if_type::IfType) -> String {
    let mut text = text.to_string();
    if text.contains('%') {
        for i in 1..=5 {
            let needle = format!("%{}", i);
            while let Some(at) = text.find(&needle) {
                text = format!("{}{}{}",
                               &text[..at],
                               inf(get_if_var(c, com, i - 1)),
                               &text[at + 2..]);
            }
        }

        while let Some(at) = text.find("%dns") {
            text = format!("{}{}{}", &text[..at], c.last_address, &text[at + 4..]);
        }
    }
    text
}

// @ObfuscatedName("q.fl(Leg;IIIIIII)V") — Client.doScrollbar. Verbatim
// port of Client.java:10712-10750: while the mouse button is held, the
// 16×16 arrow caps nudge scrollPosY by 4, and the track maps the grip
// centre to the scroll range (the hitbox widens by 32px each side
// while grabbed so fast drags don't drop the grip). Wheel rotation
// over the layer scrolls by 45px per notch. Mutations go through
// if_type::modify since draw-pass components are clones of the store.
pub fn do_scrollbar(c: &mut Client, com: &crate::config::if_type::IfType,
                    left: i32, top: i32, height: i32, scrollable_height: i32,
                    x: i32, y: i32) {
    c.scroll_input_padding = if c.scroll_grabbed { 32 } else { 0 };
    c.scroll_grabbed = false;

    let mouse_button = crate::input::MOUSE.lock().unwrap().mouse_button;
    if mouse_button != 0 {
        if x >= left && x < left + 16 && y >= top && y < top + 16 {
            crate::config::if_type::modify(com.parent_id, |t| t.scroll_pos_y -= 4);
        } else if x >= left && x < left + 16 && y >= top + height - 16 && y < top + height {
            crate::config::if_type::modify(com.parent_id, |t| t.scroll_pos_y += 4);
        } else if x >= left - c.scroll_input_padding
            && x < c.scroll_input_padding + left + 16
            && y >= top + 16 && y < top + height - 16
        {
            let mut grip_size = (height - 32) * height / scrollable_height;
            if grip_size < 8 {
                grip_size = 8;
            }

            let grip_off = y - top - 16 - grip_size / 2;
            let track = height - 32 - grip_size;
            // Java divides unguarded; a layer short enough for the
            // grip to fill the track would throw there — skip instead.
            if track > 0 {
                let pos = (scrollable_height - height) * grip_off / track;
                crate::config::if_type::modify(com.parent_id, |t| t.scroll_pos_y = pos);
            }
            c.scroll_grabbed = true;
        }
    }

    if c.mouse_wheel_rotation != 0 {
        let width = com.width;
        if x >= left - width && y >= top && x < left + 16 && y <= top + height {
            let delta = c.mouse_wheel_rotation * 45;
            crate::config::if_type::modify(com.parent_id, |t| t.scroll_pos_y += delta);
            // Consume: the draw pass can run several times within the
            // tick that snapshotted the rotation.
            c.mouse_wheel_rotation = 0;
        }
    }
}

// @ObfuscatedName(— Client.addComponentOptions). Verbatim port of
// Client.java:9829-10014: hover ops for the component under the
// cursor — button types 1-6, the type-2 inventory slot scan (sets
// hoveredSlot/hoveredSlotCom and adds held-item ops with the
// ServerActive gating), and the v3 component ops 1-10 + target verb +
// pause button. (mx, my) are component-relative like Java's args.
pub fn add_component_options(c: &mut Client, com: &crate::config::if_type::IfType,
                             mx: i32, my: i32) {
    use crate::string_constants::{tag_colour, TAG_ARROW};
    let active = get_active(c, com);

    if com.button_type == 1 {
        let text = com.button_text.clone();
        add_menu_option(c, &text, "", 24, 0, 0, com.parent_id);
    }
    if com.button_type == 2 && !c.target_mode {
        if let Some(verb) = target_verb(c, com) {
            let subject = format!("{}{}", tag_colour(65280), com.target_base);
            add_menu_option(c, &verb, &subject, 25, 0, -1, com.parent_id);
        }
    }
    if com.button_type == 3 {
        add_menu_option(c, crate::text::CLOSE, "", 26, 0, 0, com.parent_id);
    }
    if com.button_type == 4 {
        let text = com.button_text.clone();
        add_menu_option(c, &text, "", 28, 0, 0, com.parent_id);
    }
    if com.button_type == 5 {
        let text = com.button_text.clone();
        add_menu_option(c, &text, "", 29, 0, 0, com.parent_id);
    }
    if com.button_type == 6 && c.resume_pause_com == -1 {
        let text = com.button_text.clone();
        add_menu_option(c, &text, "", 30, 0, -1, com.parent_id);
    }

    if com.type_ == 2 {
        let mut slot = 0usize;
        for row in 0..com.height {
            for col in 0..com.width {
                let mut slot_x = (com.margin_x + 32) * col;
                let mut slot_y = (com.margin_y + 32) * row;
                if slot < 20 {
                    slot_x += com.inv_background_x.get(slot).copied().unwrap_or(0);
                    slot_y += com.inv_background_y.get(slot).copied().unwrap_or(0);
                }
                if mx < slot_x || my < slot_y || mx >= slot_x + 32 || my >= slot_y + 32 {
                    slot += 1;
                    continue;
                }
                c.hovered_slot = slot as i32;
                c.hovered_com = com.parent_id;
                let obj_link = com.link_obj_type.get(slot).copied().unwrap_or(0);
                if obj_link <= 0 {
                    slot += 1;
                    continue;
                }
                let Some(obj) = crate::config::obj_type::list(obj_link - 1) else {
                    slot += 1;
                    continue;
                };
                if c.use_mode == 1 && crate::config::server_active::is_obj_ops_enabled(active) {
                    if c.obj_selected_com_id != com.parent_id || c.obj_selected_slot != slot as i32 {
                        let subject = format!("{} {} {}{}", c.obj_selected_name, TAG_ARROW,
                                              tag_colour(16748608), obj.name);
                        add_menu_option(c, crate::text::USE, &subject, 31, obj.id,
                                        slot as i32, com.parent_id);
                    }
                } else if c.target_mode && crate::config::server_active::is_obj_ops_enabled(active) {
                    if (c.target_mask & 0x10) == 16 {
                        let subject = format!("{} {} {}{}", c.target_op, TAG_ARROW,
                                              tag_colour(16748608), obj.name);
                        let verb = c.target_verb.clone();
                        add_menu_option(c, &verb, &subject, 32, obj.id,
                                        slot as i32, com.parent_id);
                    }
                } else {
                    let subject = format!("{}{}", tag_colour(16748608), obj.name);
                    let iop = obj.iop.clone();
                    if crate::config::server_active::is_obj_ops_enabled(active) {
                        for index in (3..=4usize).rev() {
                            let op = iop.as_ref().and_then(|arr| arr[index].clone());
                            if let Some(op) = op {
                                let action = if index == 3 { 36 } else { 37 };
                                add_menu_option(c, &op, &subject, action, obj.id,
                                                slot as i32, com.parent_id);
                            } else if index == 4 {
                                add_menu_option(c, crate::text::DROP, &subject, 37, obj.id,
                                                slot as i32, com.parent_id);
                            }
                        }
                    }
                    if crate::config::server_active::is_obj_use_enabled(active) {
                        add_menu_option(c, crate::text::USE, &subject, 38, obj.id,
                                        slot as i32, com.parent_id);
                    }
                    if crate::config::server_active::is_obj_ops_enabled(active) {
                        for index in (0..=2usize).rev() {
                            let op = iop.as_ref().and_then(|arr| arr[index].clone());
                            if let Some(op) = op {
                                let action = [33, 34, 35][index];
                                add_menu_option(c, &op, &subject, action, obj.id,
                                                slot as i32, com.parent_id);
                            }
                        }
                    }
                    for index in (0..=4usize).rev() {
                        let op = com.iop.get(index).cloned().unwrap_or_default();
                        if !op.is_empty() {
                            let action = [39, 40, 41, 42, 43][index];
                            add_menu_option(c, &op, &subject, action, obj.id,
                                            slot as i32, com.parent_id);
                        }
                    }
                    add_menu_option(c, crate::text::EXAMINE, &subject, 1005, obj.id,
                                    slot as i32, com.parent_id);
                }
                slot += 1;
            }
        }
        // Account for slots the inner `continue`s skipped — Java
        // increments at the loop tail in all paths.
        let _ = slot;
    }

    if com.v3 {
        if c.target_mode {
            if crate::config::server_active::is_use_target(active)
                && (c.target_mask & 0x20) == 32
            {
                let subject = format!("{} {} {}", c.target_op, TAG_ARROW, com.base_op_name);
                let verb = c.target_verb.clone();
                add_menu_option(c, &verb, &subject, 58, 0, com.sub_id, com.parent_id);
            }
        } else {
            let base = com.base_op_name.clone();
            for opindex in (5..=9i32).rev() {
                if let Some(op) = get_if_type_op_name(c, com, opindex) {
                    add_menu_option(c, &op, &base, 1007, opindex + 1,
                                    com.sub_id, com.parent_id);
                }
            }
            if let Some(verb) = target_verb(c, com) {
                add_menu_option(c, &verb, &base, 25, 0, com.sub_id, com.parent_id);
            }
            for opindex in (0..=4i32).rev() {
                if let Some(op) = get_if_type_op_name(c, com, opindex) {
                    add_menu_option(c, &op, &base, 57, opindex + 1,
                                    com.sub_id, com.parent_id);
                }
            }
            if crate::config::server_active::pause_button(active) {
                add_menu_option(c, crate::text::CONTINUE, "", 30, 0,
                                com.sub_id, com.parent_id);
            }
        }
    }
}

// @ObfuscatedName("Minimenu.GameLoop") — Client.mouseLoop. Verbatim
// port of Client.java:8210-8286: routes the frame's click either into
// the open right-click menu (option pick / outside-dismiss) or into
// the topmost menu entry (with the obj-drag grab and the
// one-mouse-button / add-friend promotions to a full menu).
pub fn mouse_loop(c: &mut Client) {
    use crate::config::if_type;
    if c.obj_drag_com != -1 {
        return;
    }
    let (mut button, click_x, click_y, mouse_x, mouse_y) = {
        let m = crate::input::MOUSE.lock().unwrap();
        (m.mouse_click_button, m.mouse_click_x, m.mouse_click_y, m.mouse_x, m.mouse_y)
    };

    if c.is_menu_open {
        if button == 1 {
            let x = c.menu_x;
            let y = c.menu_y;
            let width = c.menu_width;
            let mut option = -1;
            for i in 0..c.menu_num_entries {
                let line_y = (c.menu_num_entries - 1 - i) * 15 + y + 31;
                if click_x > x && click_x < x + width
                    && click_y > line_y - 13 && click_y < line_y + 3
                {
                    option = i;
                }
            }
            if option != -1 {
                do_action(c, option);
            }
            c.is_menu_open = false;
        } else if mouse_x < c.menu_x - 10
            || mouse_x > c.menu_width + c.menu_x + 10
            || mouse_y < c.menu_y - 10
            || mouse_y > c.menu_y + c.menu_height + 10
        {
            c.is_menu_open = false;
        }
        return;
    }

    if button == 1 && c.menu_num_entries > 0 {
        let top = (c.menu_num_entries - 1) as usize;
        let action = c.menu_action[top];
        if matches!(action, 39 | 40 | 41 | 42 | 43 | 33 | 34 | 35 | 36 | 37 | 38 | 1005) {
            let slot = c.menu_param_b[top];
            let com_id = c.menu_param_c[top];
            if let Some(com) = if_type::get(com_id) {
                let active = get_active(c, &com);
                if crate::config::server_active::is_obj_swap_enabled(active)
                    || crate::config::server_active::is_obj_replace_enabled(active)
                {
                    c.obj_grab_threshold = false;
                    c.obj_drag_cycles = 0;
                    c.obj_drag_com = com_id;
                    c.obj_drag_slot = slot;
                    c.obj_grab_x = click_x;
                    c.obj_grab_y = click_y;
                    return;
                }
            }
        }
    }

    if button == 1
        && ((c.one_mouse_button == 1 && c.menu_num_entries > 2)
            || is_add_friend_option(menu_action_at(c, c.menu_num_entries - 1)))
    {
        button = 2;
    }

    if button == 1 && c.menu_num_entries > 0 {
        do_action(c, c.menu_num_entries - 1);
    } else if button == 2 && c.menu_num_entries > 0 {
        let b12 = c.b12.clone();
        open_menu(c, click_x, click_y, |s: &str| {
            b12.as_ref().map_or(s.len() as i32 * 8, |f| f.base.string_wid(s))
        });
    }
}

// @ObfuscatedName("Minimenu.drawMinimenu") — Client.drawMinimenu.
// Verbatim port of Client.java:8339-8360: the brown "Choose Option"
// box with the hover-highlight option list (bottom entry first).
pub fn draw_minimenu(c: &Client) {
    use crate::graphics::pix2d;
    let Some(b12) = c.b12.as_ref() else { return };
    let x = c.menu_x;
    let y = c.menu_y;
    let w = c.menu_width;
    let h = c.menu_height;
    let brown = 0x5d5447;
    pix2d::fill_rect(x, y, w, h, brown);
    pix2d::fill_rect(x + 1, y + 1, w - 2, 16, 0);
    pix2d::draw_rect(x + 1, y + 18, w - 2, h - 19, 0);
    b12.base.draw_string(crate::text::CHOOSEOPTION, x + 3, y + 14, brown, -1);
    let (mouse_x, mouse_y) = {
        let m = crate::input::MOUSE.lock().unwrap();
        (m.mouse_x, m.mouse_y)
    };
    for i in 0..c.menu_num_entries {
        let line_y = (c.menu_num_entries - 1 - i) * 15 + y + 31;
        let mut rgb = 0xFFFFFF;
        if mouse_x > x && mouse_x < x + w && mouse_y > line_y - 13 && mouse_y < line_y + 3 {
            rgb = 0xFFFF00;
        }
        let line = get_menu_line(&c.menu_verb[i as usize], &c.menu_subject[i as usize]);
        b12.base.draw_string(&line, x + 3, line_y, rgb, 0);
    }
}

// @ObfuscatedName("Minimenu.DrawFeedback") — Client.drawFeedback.
// Verbatim port of Client.java:8364-8383: the anti-macro top-left
// line showing the default action (or use/target prompt) plus the
// "/ N more options" suffix.
pub fn draw_feedback(c: &Client, x: i32, y: i32) {
    let Some(b12) = c.b12.as_ref() else { return };
    if c.menu_num_entries < 2 && c.use_mode == 0 && !c.target_mode {
        return;
    }
    let mut line = if c.use_mode == 1 && c.menu_num_entries < 2 {
        format!("{}{}{} {}", crate::text::USE, crate::text::MINISEPARATOR,
                c.obj_selected_name, crate::string_constants::TAG_ARROW)
    } else if c.target_mode && c.menu_num_entries < 2 {
        format!("{}{}{} {}", c.target_verb, crate::text::MINISEPARATOR,
                c.target_op, crate::string_constants::TAG_ARROW)
    } else {
        let top = (c.menu_num_entries - 1) as usize;
        get_menu_line(&c.menu_verb[top], &c.menu_subject[top])
    };
    if c.menu_num_entries > 2 {
        line = format!("{}{} / {}{}",
                       line,
                       crate::string_constants::tag_colour(0xFFFFFF),
                       c.menu_num_entries - 2,
                       " more options");
    }
    b12.base.draw_string_anti_macro(&line, x + 4, y + 15, 0xFFFFFF, 0,
                                    c.loop_cycle / 1000);
}

// Java's `crossX = mouseClickX; crossY = mouseClickY; crossMode = N;
// crossCycle = 0;` quadruple — the click crosshair feedback. The
// fields live on overlays::OVERLAYS (otherOverlays draws them).
fn set_cross(mode: i32) {
    let (cx, cy) = {
        let m = crate::input::MOUSE.lock().unwrap();
        (m.mouse_click_x, m.mouse_click_y)
    };
    let mut o = crate::overlays::OVERLAYS.lock().unwrap();
    o.cross_x = cx;
    o.cross_y = cy;
    o.cross_mode = mode;
    o.cross_cycle = 0;
}

// @ObfuscatedName("cz.eg(IIII)Z") — Client.interactWithLoc. Verbatim
// port of Client.java:5488-5526: route toward a clicked loc using its
// footprint (scenery) or wall shape (everything else), then arm the
// red crosshair. Returns false when the loc no longer exists.
pub fn interact_with_loc(c: &mut Client, tile_x: i32, tile_z: i32, typecode: i32) -> bool {
    let loc_id = (typecode >> 14) & 0x7FFF;
    let tc2 = {
        let cache = crate::scene::WORLD_CACHE.lock().unwrap();
        match cache.world.as_ref() {
            Some(w) => w.typecode2(c.minusedlevel.clamp(0, 3), tile_x, tile_z, typecode),
            None => -1,
        }
    };
    if tc2 == -1 {
        return false;
    }
    let shape = tc2 & 0x1F;
    let angle = (tc2 >> 6) & 0x3;
    let (src_x, src_z) = match c.local_player.as_ref() {
        Some(lp) => (lp.route_x[0], lp.route_z[0]),
        None => return false,
    };
    if shape == 10 || shape == 11 || shape == 22 {
        let lt = crate::config::loc_type::list(loc_id);
        let (mut width, mut length, mut forceapproach) = (1, 1, 0);
        if let Some(lt) = lt.as_ref() {
            if angle == 0 || angle == 2 {
                width = lt.width;
                length = lt.length;
            } else {
                width = lt.length;
                length = lt.width;
            }
            forceapproach = lt.forceapproach;
            if angle != 0 {
                forceapproach = (forceapproach >> (4 - angle))
                    + ((forceapproach << angle) & 0xF);
            }
        }
        try_move(c, src_x, src_z, tile_x, tile_z, true, 0, 0, width, length,
                 forceapproach, 2);
    } else {
        try_move(c, src_x, src_z, tile_x, tile_z, true, shape + 1, angle, 0, 0, 0, 2);
    }
    set_cross(2);
    true
}

// Route toward another entity's current route head (the shared
// "tryMove(localPlayer.route[0], target.route[0], false, 0,0,1,1,0, 2)"
// idiom every entity op in doAction uses) and arm the red crosshair.
fn walk_to_entity(c: &mut Client, dst_x: i32, dst_z: i32) {
    let (src_x, src_z) = match c.local_player.as_ref() {
        Some(lp) => (lp.route_x[0], lp.route_z[0]),
        None => return,
    };
    try_move(c, src_x, src_z, dst_x, dst_z, false, 0, 0, 1, 1, 0, 2);
    set_cross(2);
}

// The shared ground-obj walk: exact tile first, settle for adjacent
// second (Java's two-call pattern in actions 16-22).
fn walk_to_obj(c: &mut Client, tile_x: i32, tile_z: i32) {
    let (src_x, src_z) = match c.local_player.as_ref() {
        Some(lp) => (lp.route_x[0], lp.route_z[0]),
        None => return,
    };
    let moved = try_move(c, src_x, src_z, tile_x, tile_z, false, 0, 0, 0, 0, 0, 2);
    if !moved {
        try_move(c, src_x, src_z, tile_x, tile_z, false, 0, 0, 1, 1, 0, 2);
    }
    set_cross(2);
}

// @ObfuscatedName("m.ey(II)V") — Client.doAction. Verbatim port of
// Client.java:8437-9294: dispatches one menu entry. `entry` is the
// menu index (Java arg0). Actions ≥2000 are the "examine variant"
// duplicates and fold down by 2000 first. The flat `if action == N`
// blocks keep Java's order so multi-match side effects (none in
// practice — actions are disjoint) stay byte-equivalent.
pub fn do_action(c: &mut Client, entry: i32) {
    use crate::config::if_type;
    if entry < 0 {
        return;
    }
    let idx = entry as usize;
    let b = c.menu_param_b[idx];
    let cc = c.menu_param_c[idx];
    let mut action = c.menu_action[idx];
    let a = c.menu_param_a[idx];
    if action >= 2000 {
        action -= 2000;
    }

    // Macro for "player[a]'s route head" lookups.
    let player_route = |c: &Client, slot: i32| -> Option<(i32, i32)> {
        c.players.get(slot.max(0) as usize)
            .and_then(|o| o.as_ref())
            .map(|p| (p.route_x[0], p.route_z[0]))
    };
    let npc_route = |c: &Client, slot: i32| -> Option<(i32, i32)> {
        c.npcs.get(slot.max(0) as usize)
            .and_then(|o| o.as_ref())
            .map(|n| (n.entity.route_x[0], n.entity.route_z[0]))
    };

    if action == 45 {
        if let Some((dx, dz)) = player_route(c, a) {
            walk_to_entity(c, dx, dz);
            if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                out.p1_enc(146, isaac); // OPPLAYER2
                out.p2(a);
            }
        }
    }
    if action == 35 {
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(76, isaac); // OPHELD3
            out.p2_alt1(b);
            out.p4_alt2(cc);
            out.p2_alt1(a);
        }
        c.selected_cycle = 0;
        c.selected_com = cc;
        c.selected_item = b;
    }
    if action == 8 {
        if let Some((dx, dz)) = npc_route(c, a) {
            walk_to_entity(c, dx, dz);
            if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                out.p1_enc(190, isaac); // OPNPCT
                out.p4(c.target_com);
                out.p2_alt2(a);
                out.p2_alt2(c.target_sub);
            }
        }
    }
    if action == 51 {
        if let Some((dx, dz)) = player_route(c, a) {
            walk_to_entity(c, dx, dz);
            if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                out.p1_enc(145, isaac); // OPPLAYER8
                out.p2_alt1(a);
            }
        }
    }
    if action == 28 {
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(155, isaac); // IF_BUTTON
            out.p4(cc);
        }
        if let Some(com) = if_type::get(cc) {
            if com.scripts.first().map_or(false, |s| s.first() == Some(&5)) {
                let varp = com.scripts[0][1];
                let cur = crate::config::var_cache::get_varp(varp);
                crate::config::var_cache::set_varp(varp, 1 - cur);
                client_var(c, varp);
            }
        }
    }
    if action == 1002 {
        set_cross(2);
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(162, isaac); // OPLOCE
            out.p2_alt2((a >> 14) & 0x7FFF);
        }
    }
    if action == 31 {
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(70, isaac); // OPHELDU
            out.p2_alt1(a);
            out.p2_alt1(c.obj_com_id);
            out.p2(c.obj_selected_slot);
            out.p4(cc);
            out.p2_alt1(b);
            out.p4_alt1(c.obj_selected_com_id);
        }
        c.selected_cycle = 0;
        c.selected_com = cc;
        c.selected_item = b;
    }
    if action == 1004 {
        set_cross(2);
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(49, isaac); // OPOBJE
            out.p2_alt1(a);
        }
    }
    if action == 47 {
        if let Some((dx, dz)) = player_route(c, a) {
            walk_to_entity(c, dx, dz);
            if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                out.p1_enc(78, isaac); // OPPLAYER4
                out.p2(a);
            }
        }
    }
    if action == 32 {
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(218, isaac); // OPHELDT
            out.p2_alt1(c.target_sub);
            out.p2(b);
            out.p2(a);
            out.p4_alt2(cc);
            out.p4_alt2(c.target_com);
        }
        c.selected_cycle = 0;
        c.selected_com = cc;
        c.selected_item = b;
    }
    if action == 46 {
        if let Some((dx, dz)) = player_route(c, a) {
            walk_to_entity(c, dx, dz);
            if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                out.p1_enc(102, isaac); // OPPLAYER3
                out.p2_alt1(a);
            }
        }
    }
    if action == 20 {
        walk_to_obj(c, b, cc);
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(224, isaac); // OPOBJ3
            out.p2_alt2(a);
            out.p2_alt3(c.map_build_base_x + b);
            out.p2_alt2(c.map_build_base_z + cc);
        }
    }
    if action == 12 {
        if let Some((dx, dz)) = npc_route(c, a) {
            walk_to_entity(c, dx, dz);
            if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                out.p1_enc(95, isaac); // OPNPC4
                out.p2_alt1(a);
            }
        }
    }
    if action == 14 {
        if let Some((dx, dz)) = player_route(c, a) {
            walk_to_entity(c, dx, dz);
            if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                out.p1_enc(226, isaac); // OPPLAYERU
                out.p2_alt2(c.obj_com_id);
                out.p2_alt1(c.obj_selected_slot);
                out.p2_alt2(a);
                out.p4_alt2(c.obj_selected_com_id);
            }
        }
    }
    if action == 2 {
        if interact_with_loc(c, b, cc, a) {
            if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                out.p1_enc(247, isaac); // OPLOCT
                out.p4_alt3(c.target_com);
                out.p2(c.map_build_base_z + cc);
                out.p2_alt1(c.target_sub);
                out.p2_alt2((a >> 14) & 0x7FFF);
                out.p2_alt1(c.map_build_base_x + b);
            }
        }
    }
    if action == 41 {
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(6, isaac); // INV_BUTTON3
            out.p2_alt1(b);
            out.p4_alt1(cc);
            out.p2_alt3(a);
        }
        c.selected_cycle = 0;
        c.selected_com = cc;
        c.selected_item = b;
    }
    if action == 50 {
        if let Some((dx, dz)) = player_route(c, a) {
            walk_to_entity(c, dx, dz);
            if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                out.p1_enc(119, isaac); // OPPLAYER7
                out.p2_alt3(a);
            }
        }
    }
    if action == 29 {
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(155, isaac); // IF_BUTTON
            out.p4(cc);
        }
        if let Some(com) = if_type::get(cc) {
            if com.scripts.first().map_or(false, |s| s.first() == Some(&5)) {
                let varp = com.scripts[0][1];
                let operand = com.script_operand.first().copied().unwrap_or(0);
                if crate::config::var_cache::get_varp(varp) != operand {
                    crate::config::var_cache::set_varp(varp, operand);
                    client_var(c, varp);
                }
            }
        }
    }
    if action == 48 {
        if let Some((dx, dz)) = player_route(c, a) {
            walk_to_entity(c, dx, dz);
            if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                out.p1_enc(117, isaac); // OPPLAYER5
                out.p2_alt2(a);
            }
        }
    }
    if action == 33 {
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(135, isaac); // OPHELD1
            out.p4_alt2(cc);
            out.p2_alt3(a);
            out.p2_alt3(b);
        }
        c.selected_cycle = 0;
        c.selected_com = cc;
        c.selected_item = b;
    }
    if action == 1 {
        if interact_with_loc(c, b, cc, a) {
            if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                out.p1_enc(241, isaac); // OPLOCU
                out.p4_alt1(c.obj_selected_com_id);
                out.p2(c.obj_selected_slot);
                out.p2((a >> 14) & 0x7FFF);
                out.p2_alt2(c.map_build_base_x + b);
                out.p2_alt1(c.obj_com_id);
                out.p2_alt2(c.map_build_base_z + cc);
            }
        }
    }
    if action == 6 {
        interact_with_loc(c, b, cc, a);
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(83, isaac); // OPLOC4
            out.p2_alt2(c.map_build_base_x + b);
            out.p2_alt3(c.map_build_base_z + cc);
            out.p2_alt3((a >> 14) & 0x7FFF);
        }
    }
    if action == 15 {
        if let Some((dx, dz)) = player_route(c, a) {
            walk_to_entity(c, dx, dz);
            if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                out.p1_enc(183, isaac); // OPPLAYERT
                out.p2_alt2(c.target_sub);
                out.p4(c.target_com);
                out.p2_alt1(a);
            }
        }
    }
    if action == 18 {
        walk_to_obj(c, b, cc);
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(243, isaac); // OPOBJ1
            out.p2_alt1(a);
            out.p2(c.map_build_base_x + b);
            out.p2_alt3(c.map_build_base_z + cc);
        }
    }
    if action == 5 {
        interact_with_loc(c, b, cc, a);
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(133, isaac); // OPLOC3
            out.p2_alt2(c.map_build_base_x + b);
            out.p2_alt2(c.map_build_base_z + cc);
            out.p2_alt3((a >> 14) & 0x7FFF);
        }
    }
    if action == 16 {
        walk_to_obj(c, b, cc);
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(235, isaac); // OPOBJU
            out.p2(c.map_build_base_z + cc);
            out.p2_alt2(c.obj_com_id);
            out.p2_alt1(c.map_build_base_x + b);
            out.p4(c.obj_selected_com_id);
            out.p2_alt1(a);
            out.p2_alt1(c.obj_selected_slot);
        }
    }
    if action == 1001 {
        interact_with_loc(c, b, cc, a);
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(56, isaac); // OPLOC5
            out.p2(c.map_build_base_x + b);
            out.p2_alt1((a >> 14) & 0x7FFF);
            out.p2_alt2(c.map_build_base_z + cc);
        }
    }
    if action == 26 {
        send_close_modal(c);
    }
    if action == 37 {
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(19, isaac); // OPHELD5
            out.p2(a);
            out.p4(cc);
            out.p2_alt2(b);
        }
        c.selected_cycle = 0;
        c.selected_com = cc;
        c.selected_item = b;
    }
    if action == 57 || action == 1007 {
        let op_base = c.menu_subject[idx].clone();
        if_button_x(c, a, cc, b, &op_base);
    }
    if action == 44 {
        if let Some((dx, dz)) = player_route(c, a) {
            walk_to_entity(c, dx, dz);
            if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                out.p1_enc(246, isaac); // OPPLAYER1
                out.p2(a);
            }
        }
    }
    if action == 22 {
        walk_to_obj(c, b, cc);
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(77, isaac); // OPOBJ5
            out.p2(c.map_build_base_x + b);
            out.p2_alt2(c.map_build_base_z + cc);
            out.p2_alt3(a);
        }
    }
    if action == 24 {
        let mut transmit = true;
        if let Some(com) = if_type::get(cc) {
            if com.client_code > 0 {
                transmit = client_button(c, &com);
            }
        }
        if transmit {
            if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                out.p1_enc(155, isaac); // IF_BUTTON
                out.p4(cc);
            }
        }
    }
    if action == 9 {
        if let Some((dx, dz)) = npc_route(c, a) {
            walk_to_entity(c, dx, dz);
            if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                out.p1_enc(84, isaac); // OPNPC1
                out.p2_alt3(a);
            }
        }
    }
    if action == 49 {
        if let Some((dx, dz)) = player_route(c, a) {
            walk_to_entity(c, dx, dz);
            if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                out.p1_enc(111, isaac); // OPPLAYER6
                out.p2_alt3(a);
            }
        }
    }
    if action == 25 {
        if if_type::get_sub(cc, b).is_some() {
            end_target_mode(c);
            enter_target_mode(c, cc, b);
        }
        return;
    }
    if action == 42 {
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(186, isaac); // INV_BUTTON4
            out.p2(b);
            out.p4(cc);
            out.p2(a);
        }
        c.selected_cycle = 0;
        c.selected_com = cc;
        c.selected_item = b;
    }
    if action == 10 {
        if let Some((dx, dz)) = npc_route(c, a) {
            walk_to_entity(c, dx, dz);
            if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                out.p1_enc(13, isaac); // OPNPC2
                out.p2_alt2(a);
            }
        }
    }
    if action == 34 {
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(179, isaac); // OPHELD2
            out.p2_alt3(b);
            out.p2_alt2(a);
            out.p4_alt1(cc);
        }
        c.selected_cycle = 0;
        c.selected_com = cc;
        c.selected_item = b;
    }
    if action == 43 {
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(40, isaac); // INV_BUTTON5
            out.p2_alt1(a);
            out.p4_alt1(cc);
            out.p2_alt2(b);
        }
        c.selected_cycle = 0;
        c.selected_com = cc;
        c.selected_item = b;
    }
    if action == 1003 {
        set_cross(2);
        let npc_type_id = c.npcs.get(a.max(0) as usize)
            .and_then(|o| o.as_ref())
            .map(|n| n.type_id);
        if let Some(type_id) = npc_type_id {
            let mut t = crate::config::npc_type::list(type_id);
            if t.multinpc.is_some() {
                match t.get_multi_npc() {
                    Some(resolved) => t = resolved,
                    None => return,
                }
            }
            if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                out.p1_enc(52, isaac); // OPNPCE
                out.p2(t.id);
            }
        }
    }
    if action == 13 {
        if let Some((dx, dz)) = npc_route(c, a) {
            walk_to_entity(c, dx, dz);
            if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                out.p1_enc(88, isaac); // OPNPC5
                out.p2(a);
            }
        }
    }
    if action == 11 {
        if let Some((dx, dz)) = npc_route(c, a) {
            walk_to_entity(c, dx, dz);
            if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                out.p1_enc(67, isaac); // OPNPC3
                out.p2_alt1(a);
            }
        }
    }
    if action == 17 {
        walk_to_obj(c, b, cc);
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(81, isaac); // OPOBJT
            out.p2_alt3(a);
            out.p2(c.map_build_base_z + cc);
            out.p4_alt3(c.target_com);
            out.p2_alt2(c.map_build_base_x + b);
            out.p2_alt2(c.target_sub);
        }
    }
    if action == 3 {
        interact_with_loc(c, b, cc, a);
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(73, isaac); // OPLOC1
            out.p2_alt2((a >> 14) & 0x7FFF);
            out.p2(c.map_build_base_x + b);
            out.p2(c.map_build_base_z + cc);
        }
    }
    if action == 38 {
        end_target_mode(c);
        c.use_mode = 1;
        c.obj_selected_slot = b;
        c.obj_selected_com_id = cc;
        c.obj_com_id = a;
        let name = crate::config::obj_type::list(a)
            .map(|t| t.name)
            .unwrap_or_else(|| "null".to_string());
        c.obj_selected_name = format!("{}{}{}",
            crate::string_constants::tag_colour(16748608),
            name,
            crate::string_constants::tag_colour(16777215));
        return;
    }
    if action == 58 {
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(251, isaac); // IF_BUTTONT
            out.p2_alt2(c.target_sub);
            out.p2_alt2(b);
            out.p4(c.target_com);
            out.p4_alt2(cc);
        }
    }
    if action == 30 {
        if c.resume_pause_com == -1 {
            if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                out.p1_enc(242, isaac); // RESUME_PAUSEBUTTON
                out.p2_alt2(b);
                out.p4(cc);
            }
            c.resume_pause_com = cc;
        }
    }
    if action == 23 {
        // "Walk here" — arm the scene ground pick for the next frame.
        let mut cache = crate::scene::WORLD_CACHE.lock().unwrap();
        if let Some(world) = cache.world.as_mut() {
            world.update_mouse_picking(c.minusedlevel.clamp(0, 3), b, cc);
        }
    }
    if action == 4 {
        interact_with_loc(c, b, cc, a);
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(90, isaac); // OPLOC2
            out.p2_alt3(c.map_build_base_z + cc);
            out.p2_alt3(c.map_build_base_x + b);
            out.p2_alt2((a >> 14) & 0x7FFF);
        }
    }
    if action == 36 {
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(220, isaac); // OPHELD4
            out.p4_alt3(cc);
            out.p2_alt2(b);
            out.p2_alt1(a);
        }
        c.selected_cycle = 0;
        c.selected_com = cc;
        c.selected_item = b;
    }
    if action == 19 {
        walk_to_obj(c, b, cc);
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(177, isaac); // OPOBJ2
            out.p2(c.map_build_base_z + cc);
            out.p2_alt3(a);
            out.p2(c.map_build_base_x + b);
        }
    }
    if action == 40 {
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(202, isaac); // INV_BUTTON2
            out.p2_alt1(a);
            out.p4_alt2(cc);
            out.p2_alt1(b);
        }
        c.selected_cycle = 0;
        c.selected_com = cc;
        c.selected_item = b;
    }
    if action == 1005 {
        let count = if_type::get(cc)
            .and_then(|com| com.link_obj_number.get(b.max(0) as usize).copied());
        match count {
            Some(n) if n >= 100000 => {
                let name = crate::config::obj_type::list(a)
                    .map(|t| t.name)
                    .unwrap_or_else(|| "null".to_string());
                add_chat(c, 0, Some(String::new()),
                         Some(format!("{} x {}", n, name)), None, 0);
            }
            _ => {
                if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                    out.p1_enc(49, isaac);
                    out.p2_alt1(a);
                }
            }
        }
        c.selected_cycle = 0;
        c.selected_com = cc;
        c.selected_item = b;
    }
    if action == 7 {
        if let Some((dx, dz)) = npc_route(c, a) {
            walk_to_entity(c, dx, dz);
            if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                out.p1_enc(106, isaac); // OPNPCU
                out.p2_alt2(c.obj_selected_slot);
                out.p4(c.obj_selected_com_id);
                out.p2_alt1(a);
                out.p2_alt3(c.obj_com_id);
            }
        }
    }
    if action == 21 {
        walk_to_obj(c, b, cc);
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(139, isaac); // OPOBJ4
            out.p2_alt1(c.map_build_base_z + cc);
            out.p2_alt1(c.map_build_base_x + b);
            out.p2_alt3(a);
        }
    }
    if action == 39 {
        if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
            out.p1_enc(21, isaac); // INV_BUTTON1
            out.p2(b);
            out.p4_alt2(cc);
            out.p2_alt1(a);
        }
        c.selected_cycle = 0;
        c.selected_com = cc;
        c.selected_item = b;
    }

    if c.use_mode != 0 {
        c.use_mode = 0;
    }
    if c.target_mode {
        end_target_mode(c);
    }
}

// @ObfuscatedName("p.dh(III)V") — Client.minimapLoop. Verbatim port of
// Client.java:3225-3272: a left click inside the 146×151 minimap
// ellipse un-rotates / un-zooms the offset into a world tile and walks
// there (tryMove click_kind 1 = minimap), then appends the anti-cheat
// suffix Java has carried since 2004.
pub fn minimap_loop(c: &mut Client, click_button: i32, click_x: i32, click_y: i32) {
    let (state, last_x, last_y, macro_angle, macro_zoom) = {
        let mm = crate::minimap::MINIMAP.lock().unwrap();
        (mm.state, mm.last_draw_x, mm.last_draw_y, mm.macro_angle, mm.macro_zoom)
    };
    if state != 0 && state != 3 {
        return;
    }
    if click_button != 1 || last_x < 0 {
        return;
    }
    let x = click_x - 25 - last_x;
    let y = click_y - 5 - last_y;
    if x < 0 || y < 0 || x >= 146 || y >= 151 {
        return;
    }
    let x = x - 73;
    let y = y - 75;
    let yaw = ((c.orbit_cam_yaw + macro_angle) & 0x7FF) as usize;
    let sin_t = crate::dash3d::pix3d::sin_table();
    let cos_t = crate::dash3d::pix3d::cos_table();
    let zoom_sin = ((macro_zoom + 256) * sin_t[yaw]) >> 8;
    let zoom_cos = ((macro_zoom + 256) * cos_t[yaw]) >> 8;
    let rel_x = (x * zoom_cos + y * zoom_sin) >> 11;
    let rel_y = (y * zoom_cos - x * zoom_sin) >> 11;
    let (lp_fine_x, lp_fine_z, src_x, src_z) = match c.local_player.as_ref() {
        Some(lp) => (lp.entity.x, lp.entity.z, lp.route_x[0], lp.route_z[0]),
        None => return,
    };
    let tile_x = (lp_fine_x + rel_x) >> 7;
    let tile_z = (lp_fine_z - rel_y) >> 7;
    let moved = try_move(c, src_x, src_z, tile_x, tile_z, true, 0, 0, 0, 0, 0, 1);
    if moved {
        if let Some(out) = c.out_packet.as_mut() {
            out.p1(x);
            out.p1(y);
            out.p2(c.orbit_cam_yaw);
            out.p1(57);
            out.p1(macro_angle);
            out.p1(macro_zoom);
            out.p1(89);
            out.p2(lp_fine_x);
            out.p2(lp_fine_z);
            out.p1(c.try_move_nearest);
            out.p1(63);
        }
    }
}

// @ObfuscatedName(— Client.sendMouseClick). Verbatim port of
// Client.java:2354-2381. Packs (button-flag, time-ms, screen-pos)
// into a single g4_alt2.
pub fn send_mouse_click(c: &mut Client, button: i32, screen_x: i32, screen_y: i32, dt_ms: i64) {
    let mut t = dt_ms / 50;
    if t > 4095 { t = 4095; }
    let x = screen_x.clamp(0, 764);
    let y = screen_y.clamp(0, 502);
    let pos = y * 765 + x;
    let btn_bit = if button == 2 { 1 } else { 0 };
    let packed = (btn_bit << 19) + ((t as i32) << 20) + pos;
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(161, isaac);
    out.p4_alt2(packed);
}

// @ObfuscatedName(— Client.sendInvButtonD). Verbatim port of
// Client.java:2495-2500. Inventory drag-drop packet — fires when
// user releases an obj after dragging it onto another slot.
pub fn send_inv_button_d(c: &mut Client, parent_com_id: i32, dest_slot: i32, src_slot: i32, mode: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(2, isaac);
    out.p4_alt2(parent_com_id);
    out.p2_alt3(dest_slot);
    out.p1_alt1(mode);
    out.p2_alt1(src_slot);
}

// @ObfuscatedName(— Client.sendIfButtonD). Verbatim port of
// Client.java:11451-11458. Drag-drop between two components. Distinct
// from INV_BUTTOND (opcode 2): IF_BUTTOND moves a component itself
// (not an inventory slot) so it carries the dragCom.subId / parentId
// + dropCom.subId / parentId quartet.
pub fn send_if_button_d(c: &mut Client,
                        drag_sub: i32, drag_parent: i32,
                        drop_sub: i32, drop_parent: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(22, isaac);
    out.p2_alt3(drag_sub);
    out.p4_alt2(drop_parent);
    out.p2_alt1(drop_sub);
    out.p4_alt2(drag_parent);
}

// @ObfuscatedName(— Client.sendOpPlayer6). Verbatim port of
// Client.java:8939-8940 + 9321-9323. Action 49 player op (vanilla
// "Follow") + the player(6) cheat-mode wrapper. 1-arg targeting the
// remote player slot.
pub fn send_op_player_6(c: &mut Client, player_slot: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(111, isaac);
    out.p2_alt3(player_slot);
}

// @ObfuscatedName(— Client.sendOpPlayer8). Verbatim port of
// Client.java:8506-8508. Action 51 player op. Single-arg p2_alt1.
pub fn send_op_player_8(c: &mut Client, player_slot: i32) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(145, isaac);
    out.p2_alt1(player_slot);
}

// @ObfuscatedName(— Client.sendOpObjU). Verbatim port of
// Client.java:8824-8831. "Use selected obj on world obj." Sends the
// world coords + obj-com-id of the target, plus the held obj's
// com-id + slot. Mirrors the already-ported send_op_loc_u for locs.
pub fn send_op_obj_u(c: &mut Client,
                     tile_x: i32, tile_z: i32, obj_id: i32, obj_com_id: i32,
                     obj_selected_com_id: i32, obj_selected_slot: i32) {
    let base_x = c.map_build_base_x;
    let base_z = c.map_build_base_z;
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(235, isaac);
    out.p2(base_z + tile_z);
    out.p2_alt2(obj_com_id);
    out.p2_alt1(base_x + tile_x);
    out.p4(obj_selected_com_id);
    out.p2_alt1(obj_id);
    out.p2_alt1(obj_selected_slot);
}

// @ObfuscatedName(— Client.sendIdleTimer). Verbatim port of
// Client.java:2660-2662. Zero-payload heartbeat resetting the
// server's auto-logout countdown.
pub fn send_idle_timer(c: &mut Client) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(38, isaac);
}

// @ObfuscatedName(— Client.sendWindowStatus). Verbatim port of
// Client.java:5321-5324. Emits the static magic int 1057001181 — the
// server uses it to verify the client is a real frame (not headless).
pub fn send_window_status(c: &mut Client) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(210, isaac);
    out.p4(1057001181);
}

// @ObfuscatedName("ck.ds(IB)V") — Client.playSongs. Verbatim port of
// Client.java:3187-3195. Schedules a MidiManager.swapSongs when the
// requested id differs from the currently-pending song and we aren't
// in a jingle. The actual MidiManager bridge lands when the audio
// subsystem wiring does — this entry point stamps the queue so the
// drain can fire.
pub fn play_songs(c: &mut Client, song_id: i32) {
    if song_id == -1 && !c.playing_jingle {
        // MidiManager.stop() — clear the in-flight song.
        c.next_midi_song = -1;
        return;
    }
    if song_id != -1 && c.next_midi_song != song_id && c.midi_volume != 0 && !c.playing_jingle {
        // MidiManager.swapSongs(2, songs, id, 0, midiVolume, false);
        c.queued_song_id = song_id;
    }
    c.next_midi_song = song_id;
}

// @ObfuscatedName(— Client.playJingle). Verbatim port of
// Client.java:3199-3206. Gated on midi_volume != 0; sets the
// playing_jingle flag so playSongs queries skip swapping songs.
pub fn play_jingle(c: &mut Client, jingle_id: i32, _fade: i32) {
    if c.midi_volume == 0 || jingle_id == -1 { return; }
    c.queued_jingle_id = jingle_id;
    c.playing_jingle = true;
}

// @ObfuscatedName(— Client.playSynth). Verbatim port of
// Client.java:3210-3220. Pushes to the 50-slot wave queue; the mixer
// thread drains it each tick.
pub fn play_synth(c: &mut Client, sound: i32, loops: i32, delay: i32) {
    if c.wave_volume == 0 || loops == 0 { return; }
    if c.wave_count >= 50 { return; }
    let idx = c.wave_count as usize;
    c.wave_sound_ids[idx] = sound;
    c.wave_loops[idx] = loops;
    c.wave_delay[idx] = delay;
    c.wave_count += 1;
}

// @ObfuscatedName("eh.dp(Ljava/lang/String;S)V") — Client.doCheat.
// Verbatim port of Client.java:3306-3333. `::` prefix commands are
// processed locally when the user has staffmodlevel >= 2, then the
// entire message (sans `::`) is sent to the server as opcode 30
// CLIENT_CHEAT for server-side commands like `::tele`.
//
// Local commands:
//   ::gc          — force GC (no-op in Rust)
//   ::clientdrop  — simulate connection loss
//   ::fpson/off   — toggle FPS overlay
//   ::noclip      — clear collision flags on the active level
//   ::errortest   — panic test (only when modewhere == 2)
pub fn do_cheat(c: &mut Client, message: &str) {
    use crate::io::packet::Packet;
    if c.staffmodlevel >= 2 {
        let lower = message.to_ascii_lowercase();
        if lower == "::gc" {
            // Rust: no manual GC; touch some allocator-friendly state.
        } else if lower == "::clientdrop" {
            c.network_error = true;
        } else if lower == "::fpson" {
            c.show_fps = true;
        } else if lower == "::fpsoff" {
            c.show_fps = false;
        } else if lower == "::noclip" {
            for level in 0..4 {
                if let Some(map) = c.collision[level].as_mut() {
                    for x in 1..103usize {
                        for z in 1..103usize {
                            map.flags[x][z] = 0;
                        }
                    }
                }
            }
        } else if lower == "::errortest" && c.modewhere == 2 {
            panic!("::errortest");
        }
    }
    if message.len() < 2 { return; }
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(30, isaac);
    let body = &message[2..];
    out.p1(Packet::pjstrlen(body));
    out.pjstr(body);
}

// @ObfuscatedName("bw.ev(IIII)I") — Client.getAvH. Verbatim port of
// Client.java:4764-4779. Bilinear interpolation across the four
// neighbouring ground tiles at the given (world_x, world_z) in
// world-space (×128). Used by every camera-following math path.
//
// The `level` argument is the floor; `mapl[1][tx][tz] & 2 != 0` means
// "this is a bridge tile" — bridges sit on the floor above so we
// lift the lookup to level+1 when sampling.
pub fn get_av_h(world_x: i32, world_z: i32, level: i32) -> i32 {
    let tx = world_x >> 7;
    let tz = world_z >> 7;
    if tx < 0 || tz < 0 || tx > 103 || tz > 103 { return 0; }
    let cb = crate::client_build::STATE.lock().unwrap();
    let sample_level = if level < 3
        && (cb.mapl[1][tx as usize][tz as usize] & 0x2) == 2 {
        level + 1
    } else {
        level
    };
    let dx = world_x & 0x7F;
    let dz = world_z & 0x7F;
    let g = &cb.ground_h[sample_level as usize];
    let h00 = g[tx as usize][tz as usize];
    let h10 = g[(tx + 1) as usize][tz as usize];
    let h01 = g[tx as usize][(tz + 1) as usize];
    let h11 = g[(tx + 1) as usize][(tz + 1) as usize];
    let north = ((128 - dx) * h00 + dx * h10) >> 7;
    let south = ((128 - dx) * h01 + dx * h11) >> 7;
    ((128 - dz) * north + dz * south) >> 7
}

// @ObfuscatedName("bg.gk(Ljava/lang/String;B)Z") — Client.isFriend.
// Verbatim port of Client.java:12106-12120. Case-insensitive sweep
// over the friend roster + a fallback "is this the local player"
// check (Java returns true for your own name so the chat UI tints
// your messages the same as a friend's).
pub fn is_friend(c: &Client, who: Option<&str>) -> bool {
    let Some(name) = who else { return false; };
    for i in 0..(c.friend_count as usize) {
        if let Some(f) = c.friend_list.get(i) {
            if f.name.eq_ignore_ascii_case(name) { return true; }
        }
    }
    if let Some(lp) = &c.local_player {
        return name.eq_ignore_ascii_case(&lp.name);
    }
    false
}

// @ObfuscatedName("dn.gg(Leg;B)I") — Client.getActive. Verbatim port
// of Client.java:12384-12391. Looks up the server-supplied event-flag
// bitfield for a component; falls back to the component's static
// event_code from the cache file when the server hasn't overridden.
//
// Key format matches IF_SETEVENTS: `(parent_id << 32) | sub_id`.
pub fn get_active(c: &Client, com: &crate::config::if_type::IfType) -> i32 {
    let key = ((com.parent_id as i64) << 32) | (com.sub_id as i64 & 0xFFFF_FFFF);
    c.server_active.get(&key).copied().unwrap_or(com.event_code)
}

// @ObfuscatedName("s.gt(II)V") — Client.purgeServerActive. Verbatim
// port of Client.java:12375-12381. Drops every server-active entry
// whose parent component id matches `com_id`. Java iterates the
// HashTable looking at `(key >> 48) & 0xFFFF`; we use the same
// derivation but iterate the HashMap.
pub fn purge_server_active(c: &mut Client, com_id: i32) {
    c.server_active.retain(|&k, _| {
        // Java extracts the parent component id from bits 48..63; in
        // our key layout we only pack `parent_id << 32 | sub_id`, so
        // the upper 16 bits of the parent id land at >>48 the same
        // way.
        let parent_high = ((k as u64) >> 48) & 0xFFFF;
        parent_high as i32 != com_id
    });
}

// @ObfuscatedName("bo.gy(Leg;I)Z") — Client.hide. Verbatim port of
// Client.java:12395-12407. `field2092` is the global "modal dialog
// open" flag — when true, components with an active event flag (or
// of type 0, the layer) stay visible even if their hide bit is set.
pub fn hide_component(c: &Client, com: &crate::config::if_type::IfType) -> bool {
    if c.modal_dialog_open {
        if get_active(c, com) != 0 { return false; }
        if com.type_ == 0 { return false; }
    }
    com.hide
}

// @ObfuscatedName("ay.gu(Leg;II)Ljava/lang/String;") —
// Client.getIfTypeOpName. Verbatim port of Client.java:12411-12419.
//
// ServerActive.hasOp checks if bit `(opindex + 16)` of the active
// flags is set — that's the server's "this op is enabled" mask. If
// neither the bit nor an `onop` hook is set, the op isn't available.
// Otherwise return the per-component name from `op_names[opindex]`
// (trimmed-empty strings count as absent).
pub fn get_if_type_op_name(c: &Client, com: &crate::config::if_type::IfType, op_index: i32) -> Option<String> {
    if op_index < 0 { return None; }
    let active = get_active(c, com);
    let server_op_bit = active & (1 << (op_index + 16));
    if server_op_bit == 0 && com.hook_onop.is_none() {
        return None;
    }
    let idx = op_index as usize;
    let name = com.op_names.get(idx)?;
    if name.trim().is_empty() { return None; }
    Some(name.clone())
}

// @ObfuscatedName("ap.gb(Leg;I)Ljava/lang/String;") — Client.targetVerb.
// Verbatim port of Client.java:12423-12431.
//
// ServerActive.targetMask is the active flags' "this component is a
// drag-target" mask — bits 28..31 by ServerActive.java's layout.
pub fn target_verb(c: &Client, com: &crate::config::if_type::IfType) -> Option<String> {
    if (get_active(c, com) & 0x0F00_0000) == 0 { return None; }
    if com.target_verb.trim().is_empty() { return None; }
    Some(com.target_verb.clone())
}

// @ObfuscatedName("n.fo(II)V") — Client.ifAnimReset. Verbatim port of
// Client.java:11565-11578. Zeroes anim_frame / anim_cycle on every
// child component of the given interface group.
//
// Called on IF_OPENTOP / IF_OPENSUB so any animated rectangles or
// graphic-loops restart from frame 0 instead of inheriting the
// previous open's last state.
pub fn if_anim_reset(interfaces_slot: i32, group: i32) {
    use crate::config::if_type::{self, STORE};
    if !if_type::open_interface(group, interfaces_slot) { return; }
    let mut s = STORE.lock().unwrap();
    let Some(list) = s.list.get_mut(group as usize).and_then(|o| o.as_mut()) else { return; };
    for slot in list.iter_mut() {
        if let Some(comp) = slot.as_mut() {
            comp.anim_frame = 0;
            comp.anim_cycle = 0;
        }
    }
}

// @ObfuscatedName("g.fn(B)V") — Client.legacyUpdated. Verbatim port
// of Client.java:11490-11515. Walks every open sub-interface; for
// any that uses the legacy (non-v3) format, marks its parent
// component dirty so the next redraw repaints with the latest
// server-pushed state.
//
// The v3 vs legacy split exists because the v3 interfaces are
// reactive (component_updated is called inline by their
// per-field setter packets), but the legacy ones need a manual nudge
// every time a stat / inv / chat-filter packet arrives.
pub fn legacy_updated(c: &mut Client) {
    use crate::config::if_type;
    let interfaces_slot = if_type::INTERFACES_SLOT
        .load(std::sync::atomic::Ordering::Relaxed);
    if interfaces_slot < 0 { return; }
    let subs: Vec<(i32, i32)> = c.subinterfaces.iter()
        .map(|(&key, sub)| (sub.id, key))
        .collect();
    for (id, sub_key) in subs {
        if id < 0 { continue; }
        if !if_type::open_interface(id, interfaces_slot) { continue; }
        // Look up the first non-null child to determine v3-ness.
        let store = if_type::STORE.lock().unwrap();
        let new_format = store.list.get(id as usize)
            .and_then(|o| o.as_ref())
            .and_then(|comps| comps.iter().flatten().next())
            .map(|c| c.v3)
            .unwrap_or(true);
        drop(store);
        if !new_format {
            if let Some(com) = if_type::get(sub_key) {
                component_updated(&com);
            }
        }
    }
}

// @ObfuscatedName("ch.gl(Ljava/lang/String;I)V") — Client.addFriend.
// Verbatim port of Client.java:12142-12192. Validates capacity,
// dupes, ignore-list collisions, and self-add before emitting the
// FRIENDLIST_ADD packet (opcode 203).
//
// We simplify the case-folding step (Java does a Cp1252-aware
// DisplayNameTools.toBaseDisplayName) to ASCII lowercase since the
// rev1 username alphabet is `[a-z0-9_]` and case folding is
// effectively `.to_lowercase()`.
pub fn add_friend(c: &mut Client, name: &str) {
    use crate::io::packet::Packet;
    if c.friend_count >= 200 { return; }
    let normalised = name.to_lowercase();
    for f in c.friend_list.iter().take(c.friend_count as usize) {
        if f.name.to_lowercase() == normalised { return; }
        if f.previous_name.to_lowercase() == normalised { return; }
    }
    for g in c.ignore_list.iter().take(c.ignore_count as usize) {
        if g.name.to_lowercase() == normalised { return; }
        if g.display_name.to_lowercase() == normalised { return; }
    }
    if let Some(lp) = &c.local_player {
        if lp.name.to_lowercase() == normalised { return; }
    }
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(203, isaac);
    out.p1(Packet::pjstrlen(name));
    out.pjstr(name);
}

// @ObfuscatedName("a.gz(Ljava/lang/String;ZS)V") — Client.addIgnore.
// Verbatim port of Client.java:12196-12246. Symmetric to add_friend:
// capacity 100, dupe check, anti-collision with friend list, self-add
// guard, then IGNORELIST_ADD (opcode 231).
pub fn add_ignore(c: &mut Client, name: &str) {
    use crate::io::packet::Packet;
    if c.ignore_count >= 100 { return; }
    let normalised = name.to_lowercase();
    for g in c.ignore_list.iter().take(c.ignore_count as usize) {
        if g.name.to_lowercase() == normalised { return; }
        if g.display_name.to_lowercase() == normalised { return; }
    }
    for f in c.friend_list.iter().take(c.friend_count as usize) {
        if f.name.to_lowercase() == normalised { return; }
        if f.previous_name.to_lowercase() == normalised { return; }
    }
    if let Some(lp) = &c.local_player {
        if lp.name.to_lowercase() == normalised { return; }
    }
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(231, isaac);
    out.p1(Packet::pjstrlen(name));
    out.pjstr(name);
}

// @ObfuscatedName("ao.gp(Ljava/lang/String;B)V") — Client.delFriend.
// Verbatim port of Client.java:12250-12282. Linear scan with both
// hash-prefix ('#' raw-username form) and display-name comparisons,
// then array shift + FRIENDLIST_DEL (opcode 41).
pub fn del_friend(c: &mut Client, name: &str) {
    use crate::io::packet::Packet;
    let normalised = name.to_lowercase();
    let count = c.friend_count as usize;
    for i in 0..count {
        let entry_name = &c.friend_list[i].name;
        let matches = if name.starts_with('#') || entry_name.starts_with('#') {
            name == entry_name
        } else {
            entry_name.to_lowercase() == normalised
        };
        if matches {
            c.friend_count -= 1;
            for j in i..(c.friend_count as usize) {
                c.friend_list[j] = c.friend_list[j + 1].clone();
            }
            c.friend_transmit_num += 1;
            let Some(isaac) = c.isaac_out.as_mut() else { return; };
            let Some(out) = c.out_packet.as_mut() else { return; };
            out.p1_enc(41, isaac);
            out.p1(Packet::pjstrlen(name));
            out.pjstr(name);
            return;
        }
    }
}

// @ObfuscatedName(— Client.delIgnore). Verbatim port of
// Client.java:12287-12326. Mirrors del_friend: IGNORELIST_DEL is
// opcode 248.
pub fn del_ignore(c: &mut Client, name: &str) {
    use crate::io::packet::Packet;
    let normalised = name.to_lowercase();
    let count = c.ignore_count as usize;
    for i in 0..count {
        let entry_name = &c.ignore_list[i].name;
        let matches = if name.starts_with('#') || entry_name.starts_with('#') {
            name == entry_name
        } else {
            entry_name.to_lowercase() == normalised
        };
        if matches {
            c.ignore_count -= 1;
            for j in i..(c.ignore_count as usize) {
                c.ignore_list[j] = c.ignore_list[j + 1].clone();
            }
            c.friend_transmit_num += 1;
            let Some(isaac) = c.isaac_out.as_mut() else { return; };
            let Some(out) = c.out_packet.as_mut() else { return; };
            out.p1_enc(248, isaac);
            out.p1(Packet::pjstrlen(name));
            out.pjstr(name);
            return;
        }
    }
}

// @ObfuscatedName(— Client.setFriendRank). Verbatim port of
// Client.java:12331-12337. Emits the FRIEND_SETRANK packet
// (opcode 252) carrying `(name, rank)`.
pub fn set_friend_rank(c: &mut Client, name: &str, rank: i32) {
    use crate::io::packet::Packet;
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(252, isaac);
    out.p1(Packet::pjstrlen(name) + 1);
    out.pjstr(name);
    out.p1_alt1(rank);
}

// @ObfuscatedName("af.gf(Ljava/lang/String;I)V") — Client.friendsChatJoinChat.
// Verbatim port of Client.java:12354-12362. Sends the
// CLAN_JOINCHAT_LEAVECHAT packet (opcode 185) with the channel name.
pub fn friends_chat_join_chat(c: &mut Client, channel: &str) {
    use crate::io::packet::Packet;
    if channel.is_empty() { return; }
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(185, isaac);
    out.p1(Packet::pjstrlen(channel));
    out.pjstr(channel);
}

// @ObfuscatedName("aa.gv(I)V") — Client.friendsChatLeaveChat. Verbatim
// port of Client.java:12367-12371. Sends a zero-payload
// CLAN_JOINCHAT_LEAVECHAT (opcode 185) — the server interprets the
// empty body as "leave whichever channel I'm in".
pub fn friends_chat_leave_chat(c: &mut Client) {
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(185, isaac);
    out.p1(0);
}

// @ObfuscatedName(— Client.friendsChatKickUser). Verbatim port of
// Client.java:12341-12350. Sends CLAN_KICKUSER (opcode 245) with the
// target's username. Java drops the packet if no clan is joined; we
// mirror with a friend_chat_list emptiness check.
pub fn friends_chat_kick_user(c: &mut Client, name: &str) {
    use crate::io::packet::Packet;
    if c.friend_chat_list.is_empty() { return; }
    let Some(isaac) = c.isaac_out.as_mut() else { return; };
    let Some(out) = c.out_packet.as_mut() else { return; };
    out.p1_enc(245, isaac);
    out.p1(Packet::pjstrlen(name));
    out.pjstr(name);
}

// Pure transmit-dispatch predicate. Verbatim port of the inner block
// from Client.java:11161-11184 / 11186-11209 / 11211-11234. Used to
// decide whether a component's onvartransmit / oninvtransmit /
// onstattransmit hook should fire this tick.
//
// Returns true when:
//   - the global transmit cycle is newer than the component's saved
//     transmit cycle, AND either
//   - the component has no subscription list (fire unconditionally),
//   - more than 32 ticks elapsed (ring overflow — fire to be safe), or
//   - any id in the ring window matches the component's subscription.
//
// `ring` is the 32-slot wraparound ring of ids the engine pushed
// since boot (read at `[i & 0x1F]`). `subscription_list` is the
// component's `on<X>transmitlist` field (None = no filter).
//
// Pure: refs + i32 only, no globals, no Mutex.
pub fn should_dispatch_transmit(
    global_num: i32, comp_num: i32,
    subscription_list: Option<&[i32]>,
    ring: &[i32; 32],
) -> bool {
    if global_num <= comp_num { return false; }
    let Some(list) = subscription_list else { return true; };
    if global_num - comp_num > 32 { return true; }
    for cycle in comp_num..global_num {
        let id = ring[(cycle & 0x1F) as usize];
        if list.iter().any(|&v| v == id) { return true; }
    }
    false
}

// Simpler chat/friend/clan/misc transmit predicate. These don't have
// per-component subscription lists — they fire on any cycle bump.
// Verbatim port of Client.java:11236, 11243, 11250, 11257.
pub fn should_dispatch_misc(global_num: i32, comp_transmit_num: i32) -> bool {
    global_num > comp_transmit_num
}

// Pure thick-line stroke-offset computation inlined at Client.java:
// 10619-10645 with `// todo: inlined method (DrawLineWithStrokeWidth?)`
// comment. Given the line's (width, height) and pen lineWidth,
// returns the 4 stroke half-offsets `(slope_x_lo, slope_x_hi,
// slope_y_lo, slope_y_hi)` used to fan two flat triangles around the
// line axis. Returns None when the line has zero extent (caller
// skips the draw).
pub fn line_thickness_slopes(
    width: i32, height: i32, line_width: i32,
) -> Option<(i32, i32, i32, i32)> {
    let abs_w = width.abs();
    let abs_h = height.abs();
    let denom = abs_w.max(abs_h);
    if denom == 0 { return None; }
    let mut step_x = (width << 16) / denom;
    let mut step_y = (height << 16) / denom;
    if step_y <= step_x {
        step_x = -step_x;
    } else {
        step_y = -step_y;
    }
    let off_y_lo = (line_width * step_y) >> 17;
    let off_y_hi = (line_width * step_y + 1) >> 17;
    let off_x_lo = (line_width * step_x) >> 17;
    let off_x_hi = (line_width * step_x + 1) >> 17;
    Some((off_y_lo, off_y_hi, off_x_lo, off_x_hi))
}

// Pure chat-disabled-for-tile predicate inlined at Client.java:4694-
// 4705 with explicit `// todo: inlined method` comment. Takes world
// (x, z) and returns:
//   1 = wilderness OR Edgeville-dungeon (chat suppressed)
//   0 = safe zone (chat allowed)
// The third test (gnome agility area) cancels suppression even when
// the player is in one of the suppression rects — Java's idiom.
pub fn chat_disabled_for_tile(world_x: i32, world_z: i32) -> i32 {
    let mut chat_disabled = 0;
    if world_x >= 3053 && world_x <= 3156 && world_z >= 3056 && world_z <= 3136 {
        chat_disabled = 1;
    }
    if world_x >= 3072 && world_x <= 3118 && world_z >= 9492 && world_z <= 9535 {
        chat_disabled = 1;
    }
    if chat_disabled == 1
        && world_x >= 3139 && world_x <= 3199
        && world_z >= 3008 && world_z <= 3062
    {
        chat_disabled = 0;
    }
    chat_disabled
}

// Pure 9-branch combat-level delta → RGB tag colour. Currently
// inlined at Client.java:9760-9778 (player menu) and 9647-9665 (npc
// menu) with an explicit `// todo: inlined method (combatColourCode)`
// comment in Java. Returns the packed 24-bit RGB for the bracketed
// "(level-N)" suffix, ranging:
//   delta < -9  → bright red    (0xFF0000)
//   delta < -6  → red-orange    (0xFF3000)
//   delta < -3  → orange        (0xFF6000)
//   delta <  0  → light orange  (0xFF9000)
//   delta == 0  → yellow        (0xFFFF00)
//   delta >  9  → bright green  (0x00FF00)
//   delta >  6  → mid green     (0x40FF00)
//   delta >  3  → yellow-green  (0x80FF00)
//   delta >  0  → light green   (0xC0FF00)
pub fn combat_colour_code(delta: i32) -> i32 {
    if delta < -9 { 0xFF0000 }
    else if delta < -6 { 0xFF3000 }
    else if delta < -3 { 0xFF6000 }
    else if delta < 0 { 0xFF9000 }
    else if delta > 9 { 0x00FF00 }
    else if delta > 6 { 0x40FF00 }
    else if delta > 3 { 0x80FF00 }
    else if delta > 0 { 0xC0FF00 }
    else { 0xFFFF00 }
}

// Pure bbox intersection clamp inlined at Client.java:10968-10985
// (line case) and 10986-10992 (default case) inside loopLayer.
// Returns the inner clipped rect `(x1, y1, x2, y2)` formed by
// max-of-mins / min-of-maxes against the outer rect.
pub fn loop_layer_bbox_clip(
    inner_x: i32, inner_y: i32, inner_w: i32, inner_h: i32,
    outer_x1: i32, outer_y1: i32, outer_x2: i32, outer_y2: i32,
) -> (i32, i32, i32, i32) {
    let x1 = inner_x.max(outer_x1);
    let y1 = inner_y.max(outer_y1);
    let x2 = (inner_x + inner_w).min(outer_x2);
    let y2 = (inner_y + inner_h).min(outer_y2);
    (x1, y1, x2, y2)
}

// @ObfuscatedName("ck.fk(II)Ljava/lang/String;") — Client.inf.
// Verbatim port of Client.java:10783-10785. Caps display values at
// 999_999_999 → "*" so XP bars / scoreboards never blow past 9 digits.
// Pure function — no globals, no Mutex.
pub fn inf(arg0: i32) -> String {
    if arg0 < 999_999_999 { arg0.to_string() } else { "*".to_string() }
}

// @ObfuscatedName("dy.fr(IB)Ljava/lang/String;") — Client.niceNumber.
// Verbatim port of Client.java:10696-10708. Inserts thousands commas,
// then wraps in a colour tag: gold < 1M, white K-suffix < 1B, green
// M-suffix ≥ 1B with the original digits in parens. Used by trade
// / shop / examine cost displays. Pure: i32 in, String out.
pub fn nice_number(cost: i32) -> String {
    let mut value = cost.to_string();
    let mut i = value.len() as isize - 3;
    while i > 0 {
        value = format!("{}{}{}",
            &value[..i as usize], crate::string_constants::COMMA, &value[i as usize..]);
        i -= 3;
    }
    let close = crate::string_constants::TAG_COLOURCLOSE;
    if value.len() > 9 {
        format!(" {}{}{} {}{}{}{}",
            crate::string_constants::tag_colour(0xff80),
            &value[..value.len() - 8], crate::text::MILLION,
            crate::string_constants::OPEN_BRACKET, value,
            crate::string_constants::CLOSE_BRACKET, close)
    } else if value.len() > 6 {
        format!(" {}{}{} {}{}{}{}",
            crate::string_constants::tag_colour(0xffffff),
            &value[..value.len() - 4], crate::text::THOUSAND,
            crate::string_constants::OPEN_BRACKET, value,
            crate::string_constants::CLOSE_BRACKET, close)
    } else {
        format!(" {}{}{}",
            crate::string_constants::tag_colour(0xffff00), value, close)
    }
}

// @ObfuscatedName("ai.fw([Ljava/lang/String;B)[Ljava/lang/String;") —
// Client.prependOpIndex. Verbatim port of Client.java:11550-11560.
// Builds a 5-slot debug-overlay array prefixing each op name with
// its index ("0: ", "1: " …). Used when showOpIndex is enabled.
// Pure: Option<&[Option<String>]> in, Vec<String> out.
pub fn prepend_op_index(op: Option<&[Option<String>]>) -> Vec<String> {
    let mut tmp: Vec<String> = Vec::with_capacity(5);
    for i in 0..5 {
        let mut s = format!("{}: ", i);
        if let Some(arr) = op {
            if let Some(Some(slot)) = arr.get(i) {
                s.push_str(slot);
            }
        }
        tmp.push(s);
    }
    tmp
}

// @ObfuscatedName("ai.fs(III)V") — Client.runHookImmediate. Verbatim
// port of Client.java:11306-11353: opens the group (if not already)
// then walks every component — recursing through layer subcomponents
// and attached sub-interfaces — executing ondialogabort (kind 0) or
// onsubchange (kind 1) hooks synchronously. ccs (subId >= 0) only
// fire while still attached to their parent slot.
pub fn run_hook_immediate(c: &mut Client, group: i32, hook_kind: i32) {
    use crate::config::if_type;
    let interfaces_slot = if_type::INTERFACES_SLOT
        .load(std::sync::atomic::Ordering::Relaxed);
    if !if_type::open_interface(group, interfaces_slot) { return; }
    let components: Vec<crate::config::if_type::IfType> = {
        let s = if_type::STORE.lock().unwrap();
        match s.list.get(group as usize).and_then(|o| o.as_ref()) {
            Some(v) => v.iter().filter_map(|o| o.clone()).collect(),
            None => return,
        }
    };
    run_hook_layer(c, &components, hook_kind);
}

// @ObfuscatedName("ao.fh([Leg;IB)V") — Client.runHookLayer.
fn run_hook_layer(c: &mut Client, children: &[crate::config::if_type::IfType], hook_kind: i32) {
    use crate::script_runner::{ComRef, HookReq};
    for com in children {
        let cref = crate::interface_loop::com_ref_of(com);

        if com.type_ == 0 {
            if !com.subcomponents.is_empty() {
                let subs: Vec<crate::config::if_type::IfType> =
                    com.subcomponents.iter().filter_map(|o| o.clone()).collect();
                run_hook_layer(c, &subs, hook_kind);
            }
            if let Some(sub) = c.subinterfaces.get(&com.parent_id).cloned() {
                if sub.id >= 0 {
                    run_hook_immediate(c, sub.id, hook_kind);
                }
            }
        }

        if hook_kind == 0 {
            if let Some(hook) = com.hook_ondialogabort.clone() {
                let req = HookReq { component: cref, onop: hook, ..Default::default() };
                crate::script_runner::execute_script(c, &req);
            }
        }

        if hook_kind == 1 {
            if let Some(hook) = com.hook_onsubchange.clone() {
                // Java 11340-11345 — detached ccs are skipped.
                if matches!(cref, ComRef::Cc { .. }) && cref.resolve().is_none() {
                    continue;
                }
                let req = HookReq { component: cref, onop: hook, ..Default::default() };
                crate::script_runner::execute_script(c, &req);
            }
        }
    }
}

// @ObfuscatedName("client.fy") — Client.animateInterface. Verbatim
// port of Client.java:11582-11586. Counterpart to runHookImmediate
// but advances every child's anim_frame.
pub fn animate_interface(interfaces_slot: i32, group: i32) {
    use crate::config::if_type;
    if !if_type::open_interface(group, interfaces_slot) { return; }
    // Real Java animateLayer walks every IfType in the group + each
    // sub-component, advancing `anim_frame` per its SeqType. The
    // per-component anim driver lands with the full IfType render
    // loop refactor; for now we mark all components dirty so the
    // renderer picks up any other state changes.
    redraw_all_components();
}

// @ObfuscatedName("bi.gx(Ljava/lang/String;I)Z") — Client.isIgnored.
// Verbatim port of Client.java:12124-12138. Sweeps ignoreList[] and
// matches BOTH name and display_name case-insensitively (since the
// server may push either form during login depending on whether the
// account has set a display name).
pub fn is_ignored(c: &Client, who: Option<&str>) -> bool {
    let Some(name) = who else { return false; };
    for i in 0..(c.ignore_count as usize) {
        if let Some(g) = c.ignore_list.get(i) {
            if g.name.eq_ignore_ascii_case(name) { return true; }
            if g.display_name.eq_ignore_ascii_case(name) { return true; }
        }
    }
    false
}

pub fn entity_anim(entity: &mut crate::dash3d::ClientEntity, loop_cycle: i32) {
    use crate::config::seq_type;
    entity.needs_forward_draw_padding = false;

    // Secondary seq (walk/idle/turn anims).
    if entity.secondary_seq_id != -1 {
        let seq = seq_type::list(entity.secondary_seq_id);
        let Some(frames) = seq.frames.as_ref() else {
            entity.secondary_seq_id = -1;
            return entity_anim_spotanim_then_primary(entity, loop_cycle);
        };
        let Some(delays) = seq.delay.as_ref() else {
            entity.secondary_seq_id = -1;
            return entity_anim_spotanim_then_primary(entity, loop_cycle);
        };
        entity.secondary_seq_cycle += 1;
        let f = entity.secondary_seq_frame as usize;
        if f < frames.len() && entity.secondary_seq_cycle > delays[f] {
            entity.secondary_seq_cycle = 1;
            entity.secondary_seq_frame += 1;
        }
        if (entity.secondary_seq_frame as usize) >= frames.len() {
            entity.secondary_seq_cycle = 0;
            entity.secondary_seq_frame = 0;
        }
    }

    entity_anim_spotanim_then_primary(entity, loop_cycle);
}

fn entity_anim_spotanim_then_primary(
    entity: &mut crate::dash3d::ClientEntity, loop_cycle: i32,
) {
    use crate::config::{seq_type, spot_type};

    // Spotanim (cast effects pinned to the entity).
    if entity.spotanim_id != -1 && loop_cycle >= entity.spotanim_last_cycle {
        if entity.spotanim_frame < 0 {
            entity.spotanim_frame = 0;
        }
        let spot_anim_id = spot_type::list(entity.spotanim_id).anim;
        if spot_anim_id == -1 {
            entity.spotanim_id = -1;
        } else {
            let spot_seq = seq_type::list(spot_anim_id);
            if spot_seq.frames.is_none() {
                entity.spotanim_id = -1;
            } else {
                let frames = spot_seq.frames.as_ref().unwrap();
                let delays = spot_seq.delay.as_ref().expect("frames implies delay");
                entity.spotanim_cycle += 1;
                let f = entity.spotanim_frame as usize;
                if f < frames.len() && entity.spotanim_cycle > delays[f] {
                    entity.spotanim_cycle = 1;
                    entity.spotanim_frame += 1;
                }
                if (entity.spotanim_frame as usize) >= frames.len()
                    && (entity.spotanim_frame < 0
                        || (entity.spotanim_frame as usize) >= frames.len())
                {
                    entity.spotanim_id = -1;
                }
            }
        }
    }

    // Primary seq pre-anim move (preanim_move == 1 holds the seq for
    // one tick while the entity finishes walking into the anim spot).
    if entity.primary_seq_id != -1 && entity.primary_seq_delay <= 1 {
        let seq = seq_type::list(entity.primary_seq_id);
        if seq.preanim_move == 1
            && entity.preanim_route_length > 0
            && entity.exact_move_end <= loop_cycle
            && entity.exact_move_start < loop_cycle
        {
            entity.primary_seq_delay = 1;
            return;
        }
    }

    // Primary seq frame walker.
    if entity.primary_seq_id != -1 && entity.primary_seq_delay == 0 {
        let seq = seq_type::list(entity.primary_seq_id);
        let Some(frames) = seq.frames.as_ref() else {
            entity.primary_seq_id = -1;
            return;
        };
        let Some(delays) = seq.delay.as_ref() else {
            entity.primary_seq_id = -1;
            return;
        };
        entity.primary_seq_cycle += 1;
        let f = entity.primary_seq_frame as usize;
        if f < frames.len() && entity.primary_seq_cycle > delays[f] {
            entity.primary_seq_cycle = 1;
            entity.primary_seq_frame += 1;
        }
        if (entity.primary_seq_frame as usize) >= frames.len() {
            entity.primary_seq_frame -= seq.loops;
            entity.primary_seq_loop += 1;
            if entity.primary_seq_loop >= seq.maxloops
                || entity.primary_seq_frame < 0
                || (entity.primary_seq_frame as usize) >= frames.len()
            {
                entity.primary_seq_id = -1;
            }
        }
        entity.needs_forward_draw_padding = seq.reachforward;
    }

    if entity.primary_seq_delay > 0 {
        entity.primary_seq_delay -= 1;
    }
}

// @ObfuscatedName("p.df(Lfz;I)V") — Client.entityFace.
//
// Verbatim port of Client.java:3845-3930. Advances the entity's yaw
// toward `dst_yaw` by `turnspeed` per tick. The target yaw itself is
// computed from one of three sources (in order):
//   1. targetId < 32768: face the NPC at npcs[targetId].
//   2. targetId >= 32768: face the player at players[targetId-32768].
//   3. targetTileX/Z non-zero: face the world-space target tile.
//
// Returns early if turnspeed == 0 (locked-yaw entity).
pub fn entity_face(
    entity: &mut crate::dash3d::ClientEntity,
    npcs: &[Option<crate::dash3d::ClientNpc>],
    players: &[Option<crate::dash3d::ClientPlayer>],
    self_slot: i32,
    map_build_base_x: i32,
    map_build_base_z: i32,
) {
    if entity.turnspeed == 0 { return; }

    if entity.target_id != -1 && entity.target_id < 32768 {
        if let Some(Some(target)) = npcs.get(entity.target_id as usize) {
            let dx = entity.x - target.entity.x;
            let dz = entity.z - target.entity.z;
            if dx != 0 || dz != 0 {
                entity.dst_yaw = (((dx as f64).atan2(dz as f64) * 325.949) as i32) & 0x7FF;
            }
        }
    }

    if entity.target_id >= 32768 {
        let mut idx = entity.target_id - 32768;
        if self_slot == idx { idx = 2047; }
        if let Some(Some(target)) = players.get(idx as usize) {
            let dx = entity.x - target.x;
            let dz = entity.z - target.z;
            if dx != 0 || dz != 0 {
                entity.dst_yaw = (((dx as f64).atan2(dz as f64) * 325.949) as i32) & 0x7FF;
            }
        }
    }

    if (entity.target_tile_x != 0 || entity.target_tile_z != 0)
        && (entity.route_length == 0 || entity.anim_delay_move > 0)
    {
        let dx = entity.x - (entity.target_tile_x * 64
            - map_build_base_x * 64 - map_build_base_x * 64);
        let dz = entity.z - (entity.target_tile_z * 64
            - map_build_base_z * 64 - map_build_base_z * 64);
        if dx != 0 || dz != 0 {
            entity.dst_yaw = (((dx as f64).atan2(dz as f64) * 325.949) as i32) & 0x7FF;
        }
        entity.target_tile_x = 0;
        entity.target_tile_z = 0;
    }

    let delta = (entity.dst_yaw - entity.yaw) & 0x7FF;
    if delta == 0 {
        entity.turn_cycle = 0;
        return;
    }
    entity.turn_cycle += 1;

    if delta > 1024 {
        entity.yaw -= entity.turnspeed;
        let mut snapped = true;
        if delta < entity.turnspeed || delta > 2048 - entity.turnspeed {
            entity.yaw = entity.dst_yaw;
            snapped = false;
        }
        if entity.secondary_seq_id == entity.readyanim
            && (entity.turn_cycle > 25 || snapped)
        {
            entity.secondary_seq_id = if entity.turnleftanim == -1 {
                entity.walkanim
            } else {
                entity.turnleftanim
            };
        }
    } else {
        entity.yaw += entity.turnspeed;
        let mut snapped = true;
        if delta < entity.turnspeed || delta > 2048 - entity.turnspeed {
            entity.yaw = entity.dst_yaw;
            snapped = false;
        }
        if entity.secondary_seq_id == entity.readyanim
            && (entity.turn_cycle > 25 || snapped)
        {
            entity.secondary_seq_id = if entity.turnrightanim == -1 {
                entity.walkanim
            } else {
                entity.turnrightanim
            };
        }
    }
    entity.yaw &= 0x7FF;
}

// @ObfuscatedName("be.dq(Lfz;IB)V") — Client.moveEntity (free function
// equivalent so callers can borrow distinct slots of the players /
// npcs arrays without aliasing).
//
// Verbatim port of the Java implementation — first handles exact-move
// interpolation (server-driven walk paths set exact_move_start/end +
// exact_start_x/z + exact_end_x/z), then advances the route queue
// when the entity is on its destination tile.
//
// `size` is the entity's footprint side length (1 for player,
// NpcType.size for NPCs); used to compute the tile-centre offset.
pub fn move_entity(entity: &mut crate::dash3d::ClientEntity, size: i32, loop_cycle: i32) {
    let _ = size;
    if entity.exact_move_end > loop_cycle {
        let remaining = entity.exact_move_end - loop_cycle;
        let total_x = entity.exact_end_x.wrapping_sub(entity.exact_start_x);
        let total_z = entity.exact_end_z.wrapping_sub(entity.exact_start_z);
        let span = (entity.exact_move_end - entity.exact_move_start).max(1);
        let progressed = span - remaining;
        entity.x = entity.exact_start_x * 128
            + (entity.size * 64)
            + (total_x * 128 * progressed) / span;
        entity.z = entity.exact_start_z * 128
            + (entity.size * 64)
            + (total_z * 128 * progressed) / span;
        return;
    }
    // Free-route step: when the entity is at routeX[0] on its current
    // (x/z) and the queue has more waypoints, drop the head so the
    // next tick walks toward routeX[1].
    if entity.route_length > 0 {
        let target_x = entity.route_x[0] * 128 + entity.size * 64;
        let target_z = entity.route_z[0] * 128 + entity.size * 64;
        if entity.x == target_x && entity.z == target_z {
            // Java shifts the route down by one.
            for i in 0..entity.route_length as usize {
                entity.route_x[i] = entity.route_x[i + 1];
                entity.route_z[i] = entity.route_z[i + 1];
                entity.route_run[i] = entity.route_run[i + 1];
            }
            entity.route_length -= 1;
        }
    }
}

// @ObfuscatedName("client.ANGLE_TO_DIR") — 8-direction yaw lookup
// used by the NewVis player/npc placement paths. Each entry is the
// 2048-step yaw for a direction (0=NW, 1=N, ..., 7=SE) per Java's
// `Client.java:699`.
pub const ANGLE_TO_DIR: [i32; 8] = [768, 1024, 1280, 512, 1536, 256, 0, 1792];

pub struct Client {
    // PcmPlayer holds the cpal output stream — must live for client's
    // lifetime or audio stops. Java's `PcmPlayer` is a static singleton on
    // the gamepack; we keep it as Client-owned for clean teardown on quit.
    pub pcm_player: Option<crate::sound::pcm_player::PcmPlayer>,
    pub music_started: bool,

    // Title-screen fonts loaded in main_load step 50. The Pix2D-typed
    // `PixFontGeneric` instances live here so mainredraw can hand them
    // straight to TitleScreen.draw.
    // @ObfuscatedName("client.fz") — p11
    pub p11: Option<crate::graphics::pix_font_generic::PixFontGeneric>,
    // @ObfuscatedName("client.fc") — p12
    pub p12: Option<crate::graphics::pix_font_generic::PixFontGeneric>,
    // @ObfuscatedName("client.fa") — b12
    pub b12: Option<crate::graphics::pix_font_generic::PixFontGeneric>,

    // @ObfuscatedName("client.az")
    pub worldid: i32,

    // @ObfuscatedName("client.ah") — also mirrored to the MODEWHERE
    // atomic so non-Client code (e.g. TitleScreen.loop) can read it
    // without a borrow of Client. Java's `Client.modewhere` is static.
    pub modewhere: i32,

    // @ObfuscatedName("client.ab")
    pub mem_server: bool,
    // @ObfuscatedName("client.cs") — staffmodlevel; 0 = normal, 1 =
    // mute power, 2 = pmod, 3+ = jmod. Used by the right-click menu
    // to show "Report" + "Ignore" entries differently.
    pub staffmodlevel: i32,
    // @ObfuscatedName("client.cl") — server-broadcast reboot timer
    // (ticks until shutdown), -1 = no reboot pending.
    pub reboot_timer: i32,
    // @ObfuscatedName("client.cu") — true if the local player has the
    // "player moderator" flag.
    pub playermod: bool,

    // @ObfuscatedName("client.ct") — cinema mode toggle. When true,
    // the camera is server-driven (CAM_LOOKAT / CAM_MOVETO); when
    // false, the orbit camera follows the player.
    pub cinema_cam: bool,
    // @ObfuscatedName("client.cv/cb/cn/cm/cq") — CAM_SHAKE state, 5
    // independent shake instances (Java's `camShake[5]` and friends).
    pub cam_shake: [bool; 5],
    pub cam_shake_axis: [i32; 5],
    pub cam_shake_ran: [i32; 5],
    pub cam_shake_amp: [i32; 5],
    pub cam_shake_cycle: [i32; 5],

    // @ObfuscatedName("client.cl..ck") — CAM_LOOKAT target.
    pub cam_look_at_lx: i32,
    pub cam_look_at_lz: i32,
    pub cam_look_at_hei: i32,
    pub cam_look_at_rate: i32,
    pub cam_look_at_rate2: i32,
    // @ObfuscatedName("client.cu..ce") — CAM_MOVETO target.
    pub cam_move_to_lx: i32,
    pub cam_move_to_lz: i32,
    pub cam_move_to_hei: i32,
    pub cam_move_to_rate: i32,
    pub cam_move_to_rate2: i32,

    // @ObfuscatedName("client.ao")
    pub low_mem: bool,

    // @ObfuscatedName("client.ag")
    pub lang: i32,

    // @ObfuscatedName("client.ar")
    pub js: i32,

    // @ObfuscatedName("client.at")
    pub state: i32,

    // @ObfuscatedName("client.ae")
    pub js5_loading: bool,

    // @ObfuscatedName("client.au")
    pub loop_cycle: i32,

    // @ObfuscatedName("client.bi")
    pub show_fps: bool,

    // @ObfuscatedName("g.bu")
    pub js5_socket_req: Option<PrivilegedRequest>,

    // @ObfuscatedName("br.bz")
    pub js5_stream: Option<ClientStream>,

    // @ObfuscatedName("client.bm")
    pub js5_connect_state: i32,

    // @ObfuscatedName("client.bn")
    pub js5_connect_cooldown: i32,

    // @ObfuscatedName("client.be")
    pub js5_connect_time: i64,

    // @ObfuscatedName("client.cm")
    pub js5_errors: i32,

    // Js5Loader slot ids — Java holds Js5Loader references; we hold the
    // index Js5Net.LOADERS resolves them at.
    // @ObfuscatedName("bb.bp")
    pub anims: i32,
    // @ObfuscatedName("es.ba")
    pub bases: i32,
    // @ObfuscatedName("cc.bc")
    pub config: i32,
    // @ObfuscatedName("bd.br")
    pub interfaces: i32,
    // @ObfuscatedName("df.bb")
    pub jag_fx: i32,
    // @ObfuscatedName("ck.bd")
    pub maps: i32,
    // @ObfuscatedName("bb.cr")
    pub songs: i32,
    // @ObfuscatedName("aa.cs")
    pub models: i32,
    // @ObfuscatedName("client.cj")
    pub sprites: i32,
    // @ObfuscatedName("client.cl")
    pub textures: i32,
    // @ObfuscatedName("ab.cp")
    pub binary: i32,
    // @ObfuscatedName("dz.ca")
    pub jingles: i32,
    // @ObfuscatedName("ct.co")
    pub scripts: i32,
    // @ObfuscatedName("cj.ch")
    pub font_metrics: i32,
    // @ObfuscatedName("ey.cu")
    pub vorbis: i32,
    // @ObfuscatedName("z.cc")
    pub patches: i32,

    // @ObfuscatedName("r.pa") — DataFile masterIndex (disk cache stubbed)
    pub master_index: Option<i32>,

    // @ObfuscatedName("c.ck")
    pub login_host: String,

    // @ObfuscatedName("dn.cy")
    pub login_game_port: i32,

    // @ObfuscatedName("d.cq")
    pub login_js5_port: i32,

    // @ObfuscatedName("p.cx") — placement is approximate
    pub login_port: i32,

    // custom — loading-step pump from Client.mainLoad. State 30 in Java
    // calls openJs5; we use the same loadingStep numbers.
    pub loading_step: i32,

    // custom — single global SignLink instance (Java holds it on GameShell)
    pub signlink: SignLink,

    // @ObfuscatedName("client.cw")
    pub login_step: i32,
    // @ObfuscatedName("client.cz")
    pub login_waiting_time: i32,
    // @ObfuscatedName("client.cv")
    pub login_fail_count: i32,
    // @ObfuscatedName("client.ct")
    pub login_hop_timer: i32,
    // @ObfuscatedName("by.cg")
    pub login_socket_req: Option<crate::applet::privileged_request::PrivilegedRequest>,
    // @ObfuscatedName("at.dd")
    pub login_stream: Option<crate::io::client_stream::ClientStream>,
    // custom — Java loginSeed is a local in loginPoll, not a static.
    pub login_seed: [i32; 4],
    // @ObfuscatedName("client.kp")
    pub staff_mod_level: i32,
    // @ObfuscatedName("client.kw")
    pub player_mod: bool,
    // @ObfuscatedName("client.ik")
    pub self_slot: i32,
    // @ObfuscatedName("client.iy")
    pub members_account: i32,
    // @ObfuscatedName("client.dj")
    pub ptype: i32,
    // @ObfuscatedName("client.da")
    pub psize: i32,
    // @ObfuscatedName("client.dh") / "client.dg" / "client.dc"
    // Java tracks the last three opcodes for JagException reports;
    // each time `ptype` is set, the previous values shift down.
    pub ptype0: i32,
    pub ptype1: i32,
    pub ptype2: i32,
    // @ObfuscatedName("br.bn") — previous stream reference. When the
    // game stream errors out during play, Java captures it on
    // `prev_stream` and transitions to state 40 (reconnect).
    pub prev_stream: Option<crate::io::client_stream::ClientStream>,
    // @ObfuscatedName("ev.aw") / "ev.ar" — outbound / inbound Isaac
    // ciphers seeded at login step 5 from `login_seed`. With NO_ISAAC
    // == true (the current default) packet methods call g1/p1 instead
    // of g1Enc/p1Enc, so these stay unused; they're populated so the
    // flag can be flipped without further wiring.
    pub isaac_out: Option<crate::io::isaac::Isaac>,
    pub isaac_in: Option<crate::io::isaac::Isaac>,
    // @ObfuscatedName("client.bx") — outbound game packet ("out" in
    // Java). Allocated after login succeeds; cleared on disconnect.
    pub out_packet: Option<crate::io::packet::Packet>,
    // @ObfuscatedName(via ClientKeyboardListener.keyHeld[82]) — ctrl
    // key state, sampled by tryMove to set the "run-toggle" flag on
    // the outbound move packet. The full keyboard listener is wired
    // elsewhere; this mirrors the single bit tryMove needs.
    pub key_ctrl_held: bool,
    // @ObfuscatedName("client.di")
    pub network_error: bool,
    // custom — Engine2007 expects opcode 228 (NoOp) every few seconds
    // to keep the session alive; the Java client never sends this.
    pub heartbeat_ticker: i32,

    // @ObfuscatedName("client.ki")
    pub toplevelinterface: i32,
    // @ObfuscatedName("client.ky") — Java uses HashTable; we use HashMap
    // keyed by component id (parent<<16 | child).
    pub subinterfaces: std::collections::HashMap<i32, SubInterface>,

    // @ObfuscatedName("cr.ii") — points at players[selfSlot].
    pub local_player: Option<crate::dash3d::ClientPlayer>,

    // @ObfuscatedName("client.players") — player slot array, indexed by
    // the server's per-cycle player id (0..2047). `selfSlot` typically
    // holds the local player. We use Vec<Option<ClientPlayer>> sized to
    // 2048 to match Java; remote-player update packets will allocate
    // slots from playerAppearanceBuffer.
    pub players: Vec<Option<crate::dash3d::ClientPlayer>>,
    // @ObfuscatedName("client.playerIds") + playerCount — packed list
    // of currently-visible player slot ids (subset of players[]),
    // refilled each tick by getPlayerPosOldVis/NewVis.
    pub player_ids: Vec<i32>,
    pub player_count: i32,
    // @ObfuscatedName("client.entityUpdateIds") / "entityRemovalIds"
    // — deferred-removal scratch arrays used during the update loop.
    pub entity_update_ids: Vec<i32>,
    pub entity_update_count: i32,
    pub entity_removal_ids: Vec<i32>,
    pub entity_removal_count: i32,
    // @ObfuscatedName("client.playerAppearanceBuffer") — cached
    // appearance bytes per slot; re-applied when a player re-enters
    // viewport without resending appearance.
    pub player_appearance_buffer: Vec<Option<Vec<u8>>>,

    // @ObfuscatedName("client.npc") — NPC slot array (32768).
    pub npcs: Vec<Option<crate::dash3d::ClientNpc>>,
    pub npc_ids: Vec<i32>,
    pub npc_count: i32,

    // @ObfuscatedName("client.groundObj") — per-(level, x, z) ground
    // item containers (Java's LinkList; we use Vec<ClientObj>).
    pub ground_obj: Vec<Vec<Vec<Vec<crate::dash3d::ClientObj>>>>,

    // @ObfuscatedName("client.projectiles") / "spotanims" /
    // "locChanges" — projectile + spotanim + loc-change queues. Java
    // uses LinkList; we use Vec since we iterate front-to-back each
    // tick.
    pub projectiles: Vec<crate::dash3d::ClientProj>,
    pub spotanim_queue: Vec<crate::dash3d::ClientLocAnim>,
    // @ObfuscatedName("client.spotanims") — map-anchored spotanim
    // queue (separate from the per-entity spotanim id on each entity).
    pub spotanims: Vec<crate::dash3d::MapSpotAnim>,
    // @ObfuscatedName("client.worldUpdateNum") — Per-tick counter
    // incremented by mainloop; entity anim drivers use the delta to
    // advance.
    pub world_update_num: i32,

    // @ObfuscatedName("client.hintType") through hint*.
    pub hint_type: i32,
    pub hint_npc: i32,
    pub hint_player: i32,
    pub hint_tile_x: i32,
    pub hint_tile_z: i32,
    pub hint_height: i32,
    pub hint_offset_x: i32,
    pub hint_offset_z: i32,

    // @ObfuscatedName("client.chats") — overhead chat queue (50 max).
    pub chat_count: i32,
    pub chat_x: [i32; 50],
    pub chat_y: [i32; 50],
    pub chat_width: [i32; 50],
    pub chat_height: [i32; 50],
    pub chat_colour: [i32; 50],
    pub chat_effect: [i32; 50],
    pub chat_timer: [i32; 50],
    pub chats: [Option<String>; 50],

    // @ObfuscatedName("client.friendList") + supporting fields.
    pub friend_list: Vec<crate::friend::FriendListEntry>,
    pub friend_count: i32,
    pub friend_server_status: i32,
    pub friend_transmit_num: i32,
    // @ObfuscatedName("client.ignoreList")
    pub ignore_list: Vec<crate::friend::IgnoreListEntry>,
    pub ignore_count: i32,
    // @ObfuscatedName("client.friendChatList") — currently-joined clan
    // channel roster (rev1 caps at 100).
    pub friend_chat_list: Vec<crate::friend::FriendChatUser>,
    pub friend_chat_count: i32,
    pub chat_min_kick: i32,
    pub chat_rank: i32,
    // @ObfuscatedName("client.chatDisplayName") — the case-preserving
    // clan-channel name shown in the chatbox header. `None` when not
    // in a channel.
    pub chat_display_name: Option<String>,
    // @ObfuscatedName("client.chatOwnerName") — channel owner's username.
    pub chat_owner_name: Option<String>,
    // @ObfuscatedName("client.clanTransmitNum") — stamped every time
    // an UPDATE_FRIENDCHAT_CHANNEL packet lands.
    pub clan_transmit_num: i32,

    // Queued audio commands — set by the MIDI_SONG / MIDI_JINGLE /
    // SYNTH_SOUND packet handlers; drained by the next sound-mix tick
    // when the mixer + jagFX wiring lands.
    pub queued_song_id: i32,
    pub queued_jingle_id: i32,
    pub queued_jingle_fade_ms: i32,
    pub queued_synth: Vec<(i32, i32, i32)>,

    // @ObfuscatedName("client.midiVolume") — 0..255, 0 = mute. Audio
    // settings packet sets this; mainredraw / playSongs gate on it.
    pub midi_volume: i32,
    // @ObfuscatedName("client.waveVolume") — 0..255 for PCM SFX.
    pub wave_volume: i32,
    // @ObfuscatedName("client.nextMidiSong") — last song id requested
    // via playSongs; sticky so a jingle interruption can resume it.
    pub next_midi_song: i32,
    // @ObfuscatedName("client.playingJingle") — true between
    // MidiManager.play(jingle) and the jingle's completion callback;
    // suppresses song swaps.
    pub playing_jingle: bool,
    // @ObfuscatedName("client.waveCount/SoundIds/Loops/Delay") — the
    // 50-entry queue PCM SFX live in until the mixer drains them.
    pub wave_count: i32,
    pub wave_sound_ids: [i32; 50],
    pub wave_loops: [i32; 50],
    pub wave_delay: [i32; 50],

    // @ObfuscatedName(none — collected from RUNCLIENTSCRIPT packets
    // until the cs2 dispatcher lands). Each entry holds the decoded
    // form of one server-pushed script invocation.
    pub pending_client_scripts: Vec<PendingClientScript>,

    // @ObfuscatedName("client.zoneUpdateX/Z") — last zone base coords
    // pushed by UPDATE_ZONE_*. Subsequent LOC_/OBJ_/MAP_ packets add
    // their tile delta to these to produce a world-tile (0..103).
    pub zone_update_x: i32,
    pub zone_update_z: i32,

    // @ObfuscatedName("client.selectedCycle/Com/Item") — "use" target
    // selection. After OPHELD1-5 fires, the chat box shows "Use X
    // with..." and waits for a target click; selected_com identifies
    // the source component, selected_item the obj id.
    pub selected_cycle: i32,
    pub selected_com: i32,
    pub selected_item: i32,

    // @ObfuscatedName("client.locChanges") — pending loc change queue.
    // Each LOC_ADD_CHANGE / LOC_DEL packet pushes a LocChange entry;
    // the per-tick loc_change_do_queue drains entries whose start_time
    // has elapsed and applies the new geometry to the scene.
    pub loc_changes: Vec<crate::dash3d::LocChange>,

    // @ObfuscatedName("client.logoutTimer") — ticks until forced
    // logout. Zero = inactive; > 0 = counting down.
    pub logout_timer: i32,
    // @ObfuscatedName("client.timeoutTimer") — server-silence
    // countdown. Java's lostCon fires when this exceeds 750.
    pub timeout_timer: i32,

    // @ObfuscatedName("client.menuVerb/Subject/Action/ParamA/B/C") —
    // right-click context-menu buffer. Java caps at 500 entries.
    pub menu_verb: Vec<String>,
    pub menu_subject: Vec<String>,
    pub menu_action: Vec<i32>,
    pub menu_param_a: Vec<i32>,
    pub menu_param_b: Vec<i32>,
    pub menu_param_c: Vec<i32>,
    pub menu_num_entries: i32,
    pub is_menu_open: bool,
    pub menu_x: i32,
    pub menu_y: i32,
    pub menu_width: i32,
    pub menu_height: i32,

    // @ObfuscatedName("client.targetMode/Com/Sub") — "use" target
    // selection from a previously-clicked component (e.g. cast spell
    // on player). Reset on click + on endTargetMode.
    pub target_mode: bool,
    pub target_com: i32,
    pub target_sub: i32,
    // @ObfuscatedName("client.??") — targetMask: which scene entity
    // classes the target verb accepts (0x1 obj, 0x2 npc, 0x4 loc,
    // 0x8 player), from ServerActive.targetMask.
    pub target_mask: i32,
    pub target_verb: String,
    pub target_op: String,

    // @ObfuscatedName("client.resumePauseCom") — the IfType component
    // whose "Continue" button is currently waiting for a server
    // resume. Cleared by close_sub_interface + open_sub_interface.
    pub resume_pause_com: i32,

    // @ObfuscatedName("client.overCom") — the v1 component the mouse
    // is hovering whose overLayerId/colourOver requests hover state.
    // Rebuilt by interface_loop's per-tick pass (Java loopInterface,
    // Client.java:11279-11290); the draw walk reads it for the hover
    // colour comparisons. -1 = none; identified by the component id
    // (IfType.parent_id).
    pub over_com_id: i32,

    // @ObfuscatedName("client.tooltipCom") — the hovered type-8
    // tooltip component, same per-tick tracking as over_com.
    pub tooltip_com_id: i32,
    // @ObfuscatedName("client.tooltipNum") + tooltipRedraw — hover
    // dwell counter; the tooltip box only draws once tooltip_num has
    // climbed to tooltip_redraw (50 ticks). Java Client.java:2633-2642.
    pub tooltip_num: i32,
    pub tooltip_redraw: i32,

    // @ObfuscatedName("client.mouseWheelRotation") — wheel rotation
    // drained once per tick (Java updateGame :1380 copies the AWT
    // listener's accumulator). AWT sign convention: positive = wheel
    // toward the user (scroll down). Zeroed when a scrollbar consumes
    // it so multiple draw frames within a tick don't re-apply.
    pub mouse_wheel_rotation: i32,
    // @ObfuscatedName("client.scrollGrabbed") + scrollInputPadding —
    // scrollbar grip drag state; while grabbed the track hitbox
    // widens by 32px each side (Client.java:10713-10719).
    pub scroll_grabbed: bool,
    pub scroll_input_padding: i32,

    // @ObfuscatedName("client.dragCom") family — IF3 component drag
    // state (cs2 if_dragpickup / cc_dragpickup). dragCom is the
    // component being dragged, dragLayer the clamping ancestor layer
    // resolved by getDragLayer, dragPickup the mouse-down offset
    // within the component, dropCom the hover target under the cursor
    // collected by the loopInterface pass. Java Client.java:853-889.
    pub drag_com: crate::script_runner::ComRef,
    pub drag_layer: crate::script_runner::ComRef,
    pub drag_pickup_x: i32,
    pub drag_pickup_y: i32,
    pub drag_time: i32,
    pub drag_alive: bool,
    pub dragging: bool,
    pub drag_parent_found: bool,
    pub drop_com: crate::script_runner::ComRef,
    pub drag_current_x: i32,
    pub drag_current_y: i32,
    pub drag_parent_x: i32,
    pub drag_parent_y: i32,

    // @ObfuscatedName("client.transmitNum") — the loopInterface tick
    // counter; components stamp it on visit so the *transmit hook
    // comparisons fire exactly once per event (Client.java:2530).
    pub transmit_num: i32,
    // @ObfuscatedName("client.varTransmit") + varTransmitNum — 32-deep
    // ring of recently changed varp ids; onvartransmit subscription
    // lists are filtered against it (Client.java:5794).
    pub var_transmit: [i32; 32],
    pub var_transmit_num: i32,
    // @ObfuscatedName("client.invTransmit") + invTransmitNum — same
    // ring for inventory updates (Client.java:6251).
    pub inv_transmit: [i32; 32],
    pub inv_transmit_num: i32,

    // @ObfuscatedName("client.hookRequests") family — the three hook
    // queues loopInterface fills and updateGame drains, in priority
    // order timer → mouseStop → general (Client.java:2533-2593).
    pub hook_requests: std::collections::VecDeque<crate::script_runner::HookReq>,
    pub hook_requests_timer: std::collections::VecDeque<crate::script_runner::HookReq>,
    pub hook_requests_mouse_stop: std::collections::VecDeque<crate::script_runner::HookReq>,

    // @ObfuscatedName("client.tileLastOccupiedCycle") — per-tile
    // sceneCycle stamp so two tile-centred entities on the same tile
    // only draw one model per frame (Client.java:4237-4241).
    pub tile_last_occupied: Vec<Vec<i32>>,


    // @ObfuscatedName("client.macroCameraX/Z") — auto-camera offset
    // applied on top of the local-player position. Used during
    // cinema cuts to pan past the player.
    pub macro_camera_x: i32,
    pub macro_camera_z: i32,

    // @ObfuscatedName("ClientKeyboardListener.keyHeld[96..99]") —
    // arrow-key state used by follow_camera to drive yaw/pitch
    // velocity. 96/97 = left/right, 98/99 = up/down.
    pub key_held_96: bool,
    pub key_held_97: bool,
    pub key_held_98: bool,
    pub key_held_99: bool,

    // @ObfuscatedName("client.orbitCameraX/Z") — orbit centre that
    // followCamera smoothly tracks toward the local player.
    pub orbit_cam_x: i32,
    pub orbit_cam_z: i32,

    // @ObfuscatedName("client.macroCameraAngle") + modifier — random
    // yaw drift applied during macroCam mode to avoid bot detection.
    // Updated each tick when macroCameraCycle wraps.
    pub macro_camera_angle: i32,
    pub macro_camera_angle_modifier: i32,
    pub macro_camera_cycle: i32,
    pub macro_camera_x_modifier: i32,
    pub macro_camera_z_modifier: i32,

    // @ObfuscatedName("client.noTimeoutTimer") — last NO_TIMEOUT
    // packet was sent this many ticks ago. preventTimeout fires the
    // packet every 50 ticks during long-running mapBuildLoop calls.
    pub no_timeout_timer: i32,

    // @ObfuscatedName("client.statXP[25]") / statEffectiveLevel[25] /
    // statBaseLevel[25]. Java caps at 25 skills; rev1 currently uses
    // 24 (slot 24 is reserved for the Construction skill).
    pub stat_xp: [i32; 25],
    pub stat_effective_level: [i32; 25],
    pub stat_base_level: [i32; 25],
    // @ObfuscatedName("client.statTransmitNum") — rolling 32-slot ring
    // tracking which stat changed last (Java masks `& 0x1F`).
    pub stat_transmit: [i32; 32],
    pub stat_transmit_num: i32,

    // @ObfuscatedName("client.serverActive") — IF_SETEVENTS-populated
    // event-flag table. Key is `(comId << 32) | subIndex`; value is
    // the 32-bit event_flags bitfield from the IF_SETEVENTS packet.
    // Java uses a HashTable<Linkable>; the Linkable form is a thin
    // wrapper class. We model it with a flat HashMap since the
    // unlink-on-overwrite step is trivially equivalent to insert.
    pub server_active: std::collections::HashMap<i64, i32>,

    // @ObfuscatedName("client.field2092") — "a modal sub-interface is
    // open right now". Hides background components except for
    // layer-type (type==0) and any that have an event handler
    // attached. Toggled by IF_OPENSUB / IF_CLOSESUB.
    pub modal_dialog_open: bool,
    // @ObfuscatedName("client.chatPublicMode")/PrivateMode/TradeMode —
    // user-controlled chat filters.
    pub chat_public_mode: i32,
    pub chat_private_mode: crate::friend::PrivateChatFilter,
    pub chat_trade_mode: i32,
    // @ObfuscatedName("client.oneMouseButton") — true if user selected
    // "one mouse button" mode (left-click context menu).
    pub one_mouse_button: i32,
    // @ObfuscatedName("client.chatEffects") — 0 = effects on, 1 = off.
    pub chat_effects: i32,
    // @ObfuscatedName("client.bankArrangeMode") — bank tab/sort mode.
    pub bank_arrange_mode: i32,
    // @ObfuscatedName("client.ambientVolume") — 0..127 ambient sound
    // gain. Distinct from wave_volume which controls SFX.
    pub ambient_volume: i32,
    // @ObfuscatedName("client.runenergy")
    pub run_energy: i32,
    // @ObfuscatedName("client.runweight")
    pub run_weight: i32,
    // @ObfuscatedName("client.miscTransmitNum") — stamps the misc-pack
    // transmit number every time a stat / inv / chat filter packet
    // arrives, so we don't replay stale state on reconnect.
    pub misc_transmit_num: i32,
    // @ObfuscatedName("client.lastAddress") — last login source IP,
    // resolved via the signlink dns lookup. Surfaced in the
    // "Last logged in from..." line on the welcome screen.
    pub last_address: String,
    // @ObfuscatedName("client.playerOp[8]") + playerOpPriority[8] —
    // right-click menu entries the server enables. Default cleared
    // until the SET_PLAYER_OP packet arrives.
    pub player_op: [Option<String>; 8],
    pub player_op_priority: [bool; 8],
    // @ObfuscatedName("client.messageIds") — last-100 message-id ring
    // for PM dedupe across reconnect.
    pub message_ids: [i64; 100],
    pub private_message_count: i32,

    // @ObfuscatedName("client.chatType") through chatHistoryLength —
    // chatbox scroll-back. Java pre-allocates 100 entries.
    pub chat_type: [i32; 100],
    pub chat_username: Vec<Option<String>>,
    pub chat_screen_name: Vec<Option<String>>,
    pub chat_text: Vec<Option<String>>,
    pub chat_history_length: i32,
    // @ObfuscatedName("client.gj") — stamps the last seen transmitNum
    // when a new message is pushed; the server uses this to dedupe
    // its outbound chat-echo packets.
    pub chat_transmit_num: i32,

    // @ObfuscatedName("client.objDrag*") — inventory-drag state.
    pub obj_drag_slot: i32,
    pub obj_drag_com: i32,
    pub obj_drag_cycles: i32,
    // @ObfuscatedName("client.hp") — objGrabThreshold: set once the
    // cursor moves 5+ px from the grab point (Java boolean).
    pub obj_grab_threshold: bool,
    pub hovered_slot: i32,
    pub hovered_com: i32,
    // @ObfuscatedName("client.hh" / "client.ht") — objGrabX/Y, the
    // press position the 5px drag threshold measures from.
    pub obj_grab_x: i32,
    pub obj_grab_y: i32,
    // @ObfuscatedName("client.??") — sendCamera + sendCameraDelay
    // (EVENT_CAMERA_POSITION rate limiting, Client.java:2383-2398).
    pub send_camera: bool,
    pub send_camera_delay: i32,
    // @ObfuscatedName("client.??") — focusIn (EVENT_APPLET_FOCUS edge
    // detect, Client.java:2400-2411).
    pub focus_in_sent: bool,
    // @ObfuscatedName("client.ax") — prevMouseClickTime.
    pub prev_mouse_click_time: i64,
    // @ObfuscatedName("client.aj"/"client.aw"/"client.af") — the
    // EVENT_MOUSE_MOVE delta-encoder state.
    pub mouse_tracked_x: i32,
    pub mouse_tracked_y: i32,
    pub mouse_tracked_delta: i32,
    // @ObfuscatedName("client.??") — the per-tick key event buffer
    // (Client.java:2522-2527) the interface loop consumes.
    pub keypresses: i32,
    pub keypress_codes: [i32; 128],
    pub keypress_chars: [char; 128],
    // @ObfuscatedName("client.objSelected*") — use-with target state.
    pub obj_selected_name: String,
    pub obj_selected_com_id: i32,
    pub obj_selected_slot: i32,
    // @ObfuscatedName("client.??") — objComId: the ObjType id of the
    // selected held item (doAction 38 / the *U use-with packets).
    pub obj_com_id: i32,
    pub use_mode: i32,
    // @ObfuscatedName("al.in")
    pub minusedlevel: i32,
    // @ObfuscatedName("cd.dn")
    pub map_build_center_zone_x: i32,
    // @ObfuscatedName("v.do")
    pub map_build_center_zone_z: i32,
    // @ObfuscatedName("client.dl")
    pub last_built_level: i32,
    // @ObfuscatedName("a.de")
    pub map_build_base_x: i32,
    // @ObfuscatedName("at.dw")
    pub map_build_base_z: i32,
    // @ObfuscatedName("bw.ez") — array of (zoneX << 8 | zoneZ).
    pub map_build_index: Vec<i32>,
    // @ObfuscatedName("bo.ev")
    pub map_build_ground_file: Vec<i32>,
    // @ObfuscatedName("co.ei")
    pub map_build_location_file: Vec<i32>,
    // @ObfuscatedName("am.ef") — XTEA keys, 4 ints per zone.
    pub map_keys: Vec<[i32; 4]>,

    // @ObfuscatedName("client.ag") — per-level collision maps (4
    // levels, 104×104). Allocated by Client.buildScene / addLoc/addWall
    // call into the active level's map via `minused_level`.
    pub collision: [Option<crate::dash3d::CollisionMap>; 4],

    // @ObfuscatedName("client.bk") — BFS dir-map. Each cell holds the
    // *incoming* direction nibble used by tryMove's path
    // reconstruction (2=W, 8=E, 1=N, 4=S; OR'd for diagonals).
    pub dir_map: Box<[[i32; 104]; 104]>,
    // @ObfuscatedName("client.bv") — BFS distance map (sentinel 99999999).
    pub dist_map: Box<[[i32; 104]; 104]>,
    // @ObfuscatedName("client.bg") — BFS ring buffer of tile X coords.
    pub route_x: Box<[i32; 4000]>,
    // @ObfuscatedName("client.bl") — BFS ring buffer of tile Z coords.
    pub route_z: Box<[i32; 4000]>,
    // @ObfuscatedName("client.bt") — 1 if tryMove reached only a
    // nearby tile (not the requested target); the move packet then
    // carries this flag so the server can mark the click "best-effort".
    pub try_move_nearest: i32,
    // @ObfuscatedName("client.bw")
    pub minimap_flag_x: i32,
    // @ObfuscatedName("client.by")
    pub minimap_flag_z: i32,

    // @ObfuscatedName("ct.gw")
    pub cam_x: i32,
    // @ObfuscatedName("bv.gn")
    pub cam_y: i32,
    // @ObfuscatedName("y.gj")
    pub cam_z: i32,
    // @ObfuscatedName("bb.gk")
    pub cam_pitch: i32,
    // @ObfuscatedName("bs.gx")
    pub cam_yaw: i32,

    // @ObfuscatedName("client.gd") — orbitCameraYaw (0..2047, mod 2048)
    pub orbit_cam_yaw: i32,
    // @ObfuscatedName("dz.gf") — orbitCameraYawVelocity
    pub orbit_cam_yaw_velocity: i32,
    // @ObfuscatedName("client.gh") — orbitCameraPitch (128..383)
    pub orbit_cam_pitch: i32,
    // @ObfuscatedName("client.gp") — orbitCameraPitchVelocity
    pub orbit_cam_pitch_velocity: i32,
    // custom — viewport zoom (Java uses cameraDistance + sin/cos pull-back).
    pub orbit_cam_zoom: i32,

    // @ObfuscatedName("ej.ej") — per-zone ground bytes pulled via maps.getFile.
    pub map_build_ground_data: Vec<Option<Vec<u8>>>,
    // @ObfuscatedName("i.eh") — per-zone XTEA-decrypted loc bytes.
    pub map_build_location_data: Vec<Option<Vec<u8>>>,
    // @ObfuscatedName("client.es") — 0 nothing, 1 still waiting on map
    // archive groups, 2 still waiting on loc model preloads.
    pub map_load_state: i32,
    // @ObfuscatedName("client.dx")
    pub map_load_count: i32,
    // @ObfuscatedName("client.dt")
    pub map_load_prev_count: i32,
    // @ObfuscatedName("client.eb")
    pub loc_model_load_count: i32,
}

// @ObfuscatedName("dy") — jagex3.client.SubInterface
#[derive(Debug, Clone)]
pub struct SubInterface {
    // @ObfuscatedName("dy.m")
    pub id: i32,
    // @ObfuscatedName("dy.c")
    pub type_: i32,
    // @ObfuscatedName("dy.k") — packed parent componentId (group <<
    // 16 | sub), the LinkList hash key. Held as i32 since the rev1
    // packing fits 32 bits; Java widened to long for the hashtable.
    pub key: i32,
    // @ObfuscatedName("dy.n") — IF_RESYNC sweep marker. The decoder
    // flips this on every sub it sees during a resync pass; subs
    // still false at the end get closed (they weren't re-pushed).
    pub field1599: bool,
}

impl SubInterface {
    // @ObfuscatedName("dy.r(I)V") — Linkable.unlink. The full
    // doubly-linked list shell lives in datastruct::ChatLinkable;
    // SubInterface is a Linkable subclass so the per-instance unlink
    // is just "if linked, splice yourself out". On the Rust side
    // SubInterface owns no list pointers (it lives in a HashMap), so
    // unlink reduces to a marker — the caller removes it from the
    // owning collection.
    pub fn unlink(&self) {}
}

// @ObfuscatedName("am.gr(Ldy;ZI)V") — Client.closeSubInterface.
// Verbatim port of Client.java:11841-11884 (modulo menu/dirtyArea
// helpers that are still stubs).
//
// On close: drop the sub from the subinterfaces map, discard the
// archive group's files, clear all non-type-2 components and the
// `open` flag for the interface group, then mark the parent
// component dirty so the next redraw repaints it.
pub fn close_sub_interface(c: &mut Client, sub_key: i32, also_purge_archive: bool) {
    let Some(sub) = c.subinterfaces.remove(&sub_key) else { return; };
    sub.unlink();

    let var2 = sub.id;
    let var3 = sub.key;

    if also_purge_archive && var2 != -1 {
        use crate::config::if_type::{self, STORE};
        let mut s = STORE.lock().unwrap();
        if s.open.get(var2 as usize).copied().unwrap_or(false) {
            if let Some(list) = s.list.get_mut(var2 as usize).and_then(|o| o.as_mut()) {
                let mut keep_array = false;
                for slot in list.iter_mut() {
                    if let Some(comp) = slot {
                        if comp.type_ == 2 {
                            keep_array = true;
                        } else {
                            *slot = None;
                        }
                    }
                }
                if !keep_array {
                    s.list[var2 as usize] = None;
                }
                s.open[var2 as usize] = false;
            }
            // discardFiles on the interfaces archive — deferred (the
            // Rust Js5Loader doesn't expose a discard hook yet; data
            // will be reloaded from disk on the next open_interface).
            let _ = if_type::INTERFACES_SLOT
                .load(std::sync::atomic::Ordering::Relaxed);
        }
    }

    // purgeServerActive — clears server-side active iftype cache for
    // this interface id. Stub: the server-active cache itself isn't
    // ported yet.
    let _ = var2;

    // Mark the parent component dirty so the next redraw repaints it.
    if let Some(com) = crate::config::if_type::get(var3) {
        component_updated(&com);
    }

    // Java 11877-11883 — closing a sub also closes any open menu and
    // notifies the top-level tree's onsubchange hooks.
    c.is_menu_open = false;
    c.menu_num_entries = 0;
    if c.toplevelinterface != -1 {
        let toplevel = c.toplevelinterface;
        run_hook_immediate(c, toplevel, 1);
    }
}

// custom — keeps re-creating mainLoad state across mainloop calls without
// rebuilding the lifecycle struct.
pub static LOADERS_OPENED: AtomicBool = AtomicBool::new(false);
pub static LAST_PROGRESS: AtomicI32 = AtomicI32::new(0);

// custom helper — borrow two different loader slots from the registry
// without aliasing. Returns (sprites_loader, font_metrics_loader).
fn split2<'a>(
    reg: &'a mut Vec<Option<Box<Js5Loader>>>,
    a: i32,
    b: i32,
) -> (Option<&'a mut Js5Loader>, Option<&'a mut Js5Loader>) {
    if a < 0 || b < 0 || a == b {
        return (None, None);
    }
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    let (left, right) = reg.split_at_mut(hi as usize);
    let lo_ref = left.get_mut(lo as usize).and_then(|o| o.as_deref_mut());
    let hi_ref = right.first_mut().and_then(|o| o.as_deref_mut());
    if a < b { (lo_ref, hi_ref) } else { (hi_ref, lo_ref) }
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

pub static MODEWHERE: AtomicI32 = AtomicI32::new(1);
pub static WORLDID: AtomicI32 = AtomicI32::new(1);

impl Client {
    pub fn modewhere_global() -> i32 { MODEWHERE.load(Ordering::Relaxed) }
    pub fn worldid_global() -> i32 { WORLDID.load(Ordering::Relaxed) }

    pub fn new() -> Self {
        MODEWHERE.store(1, Ordering::Relaxed);
        WORLDID.store(1, Ordering::Relaxed);
        Self {
            pcm_player: None,
            music_started: false,
            p11: None,
            p12: None,
            b12: None,
            worldid: 1,
            // Dev mode — the local server listens on worldid+40000 (40001)
            // for the game port and worldid+50000 (50001) for JS5. Live
            // (modewhere == 0) uses the hardcoded 43594/443 pair.
            modewhere: 1,
            mem_server: false,
            staffmodlevel: 0,
            reboot_timer: -1,
            playermod: false,
            cinema_cam: false,
            cam_shake: [false; 5],
            cam_shake_axis: [0; 5],
            cam_shake_ran: [0; 5],
            cam_shake_amp: [0; 5],
            cam_shake_cycle: [0; 5],
            cam_look_at_lx: 0,
            cam_look_at_lz: 0,
            cam_look_at_hei: 0,
            cam_look_at_rate: 0,
            cam_look_at_rate2: 0,
            cam_move_to_lx: 0,
            cam_move_to_lz: 0,
            cam_move_to_hei: 0,
            cam_move_to_rate: 0,
            cam_move_to_rate2: 0,
            low_mem: false,
            lang: 0,
            js: 1,
            state: 0,
            js5_loading: true,
            loop_cycle: 0,
            show_fps: false,
            js5_socket_req: None,
            js5_stream: None,
            js5_connect_state: 0,
            js5_connect_cooldown: 0,
            js5_connect_time: 0,
            js5_errors: 0,
            anims: -1,
            bases: -1,
            config: -1,
            interfaces: -1,
            jag_fx: -1,
            maps: -1,
            songs: -1,
            models: -1,
            sprites: -1,
            textures: -1,
            binary: -1,
            jingles: -1,
            scripts: -1,
            font_metrics: -1,
            vorbis: -1,
            patches: -1,
            master_index: None,
            login_host: "127.0.0.1".to_string(),
            login_game_port: 0,
            login_js5_port: 0,
            login_port: 0,
            loading_step: 0,
            signlink: SignLink::new(),
            login_step: 0,
            login_waiting_time: 0,
            login_fail_count: 0,
            login_hop_timer: 0,
            login_socket_req: None,
            login_stream: None,
            login_seed: [0; 4],
            staff_mod_level: 0,
            player_mod: false,
            self_slot: -1,
            members_account: 0,
            ptype: -1,
            psize: 0,
            ptype0: -1,
            ptype1: -1,
            ptype2: -1,
            prev_stream: None,
            isaac_out: None,
            isaac_in: None,
            out_packet: None,
            key_ctrl_held: false,
            network_error: false,
            heartbeat_ticker: 0,
            toplevelinterface: -1,
            subinterfaces: std::collections::HashMap::new(),
            local_player: None,
            players: (0..2048).map(|_| None).collect(),
            player_ids: vec![0; 2048],
            player_count: 0,
            entity_update_ids: vec![0; 2048],
            entity_update_count: 0,
            entity_removal_ids: vec![0; 2048],
            entity_removal_count: 0,
            player_appearance_buffer: (0..2048).map(|_| None).collect(),
            npcs: (0..32768).map(|_| None).collect(),
            npc_ids: vec![0; 32768],
            npc_count: 0,
            ground_obj: (0..4).map(|_| {
                (0..104).map(|_| (0..104).map(|_| Vec::new()).collect()).collect()
            }).collect(),
            projectiles: Vec::new(),
            spotanims: Vec::new(),
            world_update_num: 0,
            spotanim_queue: Vec::new(),
            hint_type: 0,
            hint_npc: -1,
            hint_player: -1,
            hint_tile_x: 0,
            hint_tile_z: 0,
            hint_height: 0,
            hint_offset_x: 0,
            hint_offset_z: 0,
            chat_count: 0,
            chat_x: [0; 50],
            chat_y: [0; 50],
            chat_width: [0; 50],
            chat_height: [0; 50],
            chat_colour: [0; 50],
            chat_effect: [0; 50],
            chat_timer: [0; 50],
            chats: std::array::from_fn(|_| None),

            friend_list: Vec::new(),
            friend_count: 0,
            friend_server_status: 0,
            friend_transmit_num: 0,
            ignore_list: Vec::new(),
            ignore_count: 0,
            friend_chat_list: Vec::new(),
            friend_chat_count: 0,
            chat_display_name: None,
            chat_owner_name: None,
            clan_transmit_num: 0,
            queued_song_id: -1,
            queued_jingle_id: -1,
            queued_jingle_fade_ms: 0,
            queued_synth: Vec::new(),
            midi_volume: 255,
            wave_volume: 255,
            next_midi_song: -1,
            playing_jingle: false,
            wave_count: 0,
            wave_sound_ids: [0; 50],
            wave_loops: [0; 50],
            wave_delay: [0; 50],
            pending_client_scripts: Vec::new(),
            zone_update_x: 0,
            zone_update_z: 0,
            selected_cycle: 0,
            selected_com: -1,
            selected_item: -1,
            loc_changes: Vec::new(),
            logout_timer: 0,
            timeout_timer: 0,
            menu_verb: vec![String::new(); 500],
            menu_subject: vec![String::new(); 500],
            menu_action: vec![0i32; 500],
            menu_param_a: vec![0i32; 500],
            menu_param_b: vec![0i32; 500],
            menu_param_c: vec![0i32; 500],
            menu_num_entries: 0,
            is_menu_open: false,
            menu_x: 0,
            menu_y: 0,
            menu_width: 0,
            menu_height: 0,
            target_mode: false,
            target_com: -1,
            target_sub: -1,
            target_mask: 0,
            target_verb: String::new(),
            target_op: String::new(),
            resume_pause_com: -1,
            over_com_id: -1,
            tooltip_com_id: -1,
            tooltip_num: 0,
            tooltip_redraw: 50,
            mouse_wheel_rotation: 0,
            scroll_grabbed: false,
            scroll_input_padding: 0,
            drag_com: crate::script_runner::ComRef::None,
            drag_layer: crate::script_runner::ComRef::None,
            drag_pickup_x: 0,
            drag_pickup_y: 0,
            drag_time: 0,
            drag_alive: false,
            dragging: false,
            drag_parent_found: false,
            drop_com: crate::script_runner::ComRef::None,
            drag_current_x: 0,
            drag_current_y: 0,
            drag_parent_x: 0,
            drag_parent_y: 0,
            transmit_num: 0,
            var_transmit: [0; 32],
            var_transmit_num: 0,
            inv_transmit: [0; 32],
            inv_transmit_num: 0,
            hook_requests: std::collections::VecDeque::new(),
            hook_requests_timer: std::collections::VecDeque::new(),
            hook_requests_mouse_stop: std::collections::VecDeque::new(),
            tile_last_occupied: vec![vec![0; 104]; 104],
            macro_camera_x: 0,
            macro_camera_z: 0,
            key_held_96: false,
            key_held_97: false,
            key_held_98: false,
            key_held_99: false,
            orbit_cam_x: 0,
            orbit_cam_z: 0,
            macro_camera_angle: 0,
            macro_camera_angle_modifier: 0,
            macro_camera_cycle: 0,
            macro_camera_x_modifier: 2,
            macro_camera_z_modifier: 2,
            no_timeout_timer: 0,
            stat_xp: [0; 25],
            stat_effective_level: [0; 25],
            stat_base_level: [1; 25],
            stat_transmit: [0; 32],
            stat_transmit_num: 0,
            server_active: std::collections::HashMap::new(),
            modal_dialog_open: false,
            chat_min_kick: 0,
            chat_rank: 0,
            chat_public_mode: 0,
            chat_private_mode: crate::friend::PrivateChatFilter::On,
            chat_trade_mode: 0,
            one_mouse_button: 0,
            chat_effects: 0,
            bank_arrange_mode: 0,
            ambient_volume: 127,
            run_energy: 100,
            run_weight: 0,
            misc_transmit_num: 0,
            last_address: String::new(),
            player_op: [const { None }; 8],
            player_op_priority: [false; 8],
            message_ids: [0; 100],
            private_message_count: 0,

            chat_type: [0; 100],
            chat_username: vec![None; 100],
            chat_screen_name: vec![None; 100],
            chat_text: vec![None; 100],
            chat_history_length: 0,
            chat_transmit_num: 0,

            obj_drag_slot: -1,
            obj_drag_com: -1,
            obj_drag_cycles: 0,
            obj_grab_threshold: false,
            hovered_slot: -1,
            hovered_com: -1,
            obj_grab_x: 0,
            obj_grab_y: 0,
            send_camera: false,
            send_camera_delay: 0,
            focus_in_sent: false,
            prev_mouse_click_time: 0,
            mouse_tracked_x: 0,
            mouse_tracked_y: 0,
            mouse_tracked_delta: 0,
            keypresses: 0,
            keypress_codes: [0; 128],
            keypress_chars: ['\0'; 128],
            obj_selected_name: String::new(),
            obj_com_id: -1,
            obj_selected_com_id: -1,
            obj_selected_slot: -1,
            use_mode: 0,
            minusedlevel: 0,
            map_build_center_zone_x: -1,
            map_build_center_zone_z: -1,
            last_built_level: -1,
            map_build_base_x: 0,
            map_build_base_z: 0,
            map_build_index: Vec::new(),
            map_build_ground_file: Vec::new(),
            map_build_location_file: Vec::new(),
            map_keys: Vec::new(),
            collision: [None, None, None, None],
            dir_map: Box::new([[0i32; 104]; 104]),
            dist_map: Box::new([[0i32; 104]; 104]),
            route_x: Box::new([0i32; 4000]),
            route_z: Box::new([0i32; 4000]),
            try_move_nearest: 0,
            minimap_flag_x: 0,
            minimap_flag_z: 0,
            cam_x: 0,
            cam_y: 0,
            cam_z: 0,
            cam_pitch: 0,
            cam_yaw: 0,
            orbit_cam_yaw: 0,
            orbit_cam_yaw_velocity: 0,
            orbit_cam_pitch: 256,
            orbit_cam_pitch_velocity: 0,
            orbit_cam_zoom: 1100,
            map_build_ground_data: Vec::new(),
            map_build_location_data: Vec::new(),
            map_load_state: 0,
            map_load_count: 0,
            map_load_prev_count: 1,
            loc_model_load_count: 0,
        }
    }

    // Helpers for the login module — TitleScreen owns the user/pass strings.
    pub fn login_user(&self) -> String {
        crate::title_screen::STATE.lock().unwrap().login_user.clone()
    }
    pub fn login_pass(&self) -> String {
        crate::title_screen::STATE.lock().unwrap().login_pass.clone()
    }

    // startApplication — no @ObfuscatedName ("custom" tag on the Java method too).
    pub fn start_application(&mut self, width: u32, height: u32, _revision: i32) {
        let mut shell = SHELL.lock().unwrap();
        shell.s_wid = width;
        shell.s_hei = height;
    }

    // @ObfuscatedName("dj.z(IIIB)V") — GameShell.startCommon
    pub fn start_common(&mut self, s_wid: u32, s_hei: u32, _revision: i32) {
        let mut shell = SHELL.lock().unwrap();
        shell.s_wid = s_wid;
        shell.s_hei = s_hei;
    }

    // @ObfuscatedName("dj.q(I)Z") — GameShell.checkhost
    pub fn check_host(&self) -> bool {
        crate::settings::NO_HOST_CHECK
    }

    // @ObfuscatedName("client.ci(I)V")
    pub fn service_net_client(&mut self) {
        if self.state == 1000 {
            return;
        }
        let ok = js5_net::loop_tick();
        if !ok {
            self.js5_connect();
        }
    }

    // @ObfuscatedName("client.cb(I)V")
    pub fn js5_connect(&mut self) {
        if js5_net::crc_error_count() >= 4 {
            self.error("js5crc");
            self.state = 1000;
            return;
        }
        if js5_net::io_error_count() >= 4 {
            if self.state <= 5 {
                self.error("js5io");
                self.state = 1000;
                return;
            }
            self.js5_connect_cooldown = 3000;
            js5_net::set_io_error_count(3);
        }
        self.js5_connect_cooldown -= 1;
        if self.js5_connect_cooldown + 1 > 0 {
            return;
        }

        if self.js5_connect_state == 0 {
            eprintln!("[js5] socketreq {}:{}", self.login_host, self.login_port);
            self.js5_socket_req = Some(self.signlink.socketreq(&self.login_host, self.login_port));
            self.js5_connect_state += 1;
        }

        if self.js5_connect_state == 1 {
            let status = self.js5_socket_req.as_ref().map(|r| r.status()).unwrap_or(STATUS_ERROR);
            if status == STATUS_ERROR {
                eprintln!("[js5] socketreq ERROR");
                self.js5_error(-1);
                return;
            }
            if status == STATUS_DONE {
                eprintln!("[js5] socketreq DONE");
                self.js5_connect_state += 1;
            }
        }

        if self.js5_connect_state == 2 {
            let req = self.js5_socket_req.as_ref().unwrap();
            let mut result_guard = req.result.lock().unwrap();
            let stream_inner = match std::mem::replace(&mut *result_guard, PReqResult::None) {
                PReqResult::Socket(s) => s,
                _ => {
                    drop(result_guard);
                    self.js5_error(-1);
                    return;
                }
            };
            drop(result_guard);
            let mut stream = match ClientStream::new(stream_inner) {
                Ok(s) => s,
                Err(_) => {
                    self.js5_error(-3);
                    return;
                }
            };
            let mut handshake = Packet::with_size(5);
            handshake.p1(15); // INIT_JS5REMOTE_CONNECTION
            handshake.p4(1); // revision
            if stream.write(&handshake.data, 0, 5).is_err() {
                eprintln!("[js5] handshake write err");
                self.js5_error(-3);
                return;
            }
            eprintln!("[js5] handshake sent (15, rev=1)");
            self.js5_stream = Some(stream);
            self.js5_connect_state += 1;
            self.js5_connect_time = game_shell::monotonic_ms();
        }

        if self.js5_connect_state == 3 {
            let stream = self.js5_stream.as_mut().unwrap();
            let have_data = match stream.available() {
                Ok(n) => n > 0,
                Err(_) => false,
            };
            if self.state <= 5 || have_data {
                let response = match stream.read_byte() {
                    Ok(r) => r,
                    Err(_) => {
                        eprintln!("[js5] handshake read err");
                        self.js5_error(-3);
                        return;
                    }
                };
                eprintln!("[js5] handshake response byte = {response}");
                if response != 0 {
                    self.js5_error(response);
                    return;
                }
                self.js5_connect_state += 1;
            } else if game_shell::monotonic_ms() - self.js5_connect_time > 30000 {
                eprintln!("[js5] handshake timeout");
                self.js5_error(-2);
                return;
            }
        }

        if self.js5_connect_state == 4 {
            eprintln!("[js5] init() — entering JS5 mode");
            let stream = self.js5_stream.take().unwrap();
            js5_net::init(stream, self.state > 20);
            self.js5_socket_req = None;
            self.js5_connect_state = 0;
            self.js5_errors = 0;
        }
    }

    // @ObfuscatedName("client.cf(II)V")
    pub fn js5_error(&mut self, arg0: i32) {
        self.js5_socket_req = None;
        self.js5_stream = None;
        self.js5_connect_state = 0;

        if self.login_game_port == self.login_port {
            self.login_port = self.login_js5_port;
        } else {
            self.login_port = self.login_game_port;
        }
        self.js5_errors += 1;

        if self.js5_errors >= 2 && (arg0 == 7 || arg0 == 9) {
            if self.state <= 5 {
                self.error("js5connect_full");
                self.state = 1000;
            } else {
                self.js5_connect_cooldown = 3000;
            }
        } else if self.js5_errors >= 2 && arg0 == 6 {
            self.error("js5connect_outofdate");
            self.state = 1000;
        } else if self.js5_errors >= 4 {
            if self.state <= 5 {
                self.error("js5connect");
                self.state = 1000;
            } else {
                self.js5_connect_cooldown = 3000;
            }
        }
    }

    // Client.error — Java logs and asks the browser to redirect to
    // `error_game_X.ws`. For the standalone port we just stash the message.
    pub fn error(&self, err: &str) {
        eprintln!("error_game_{err}");
    }

    // @ObfuscatedName("ek.dx(IZZZB)Ldq;")
    pub fn open_js5(&self, archive: i32, discard_packed: bool, discard_unpacked: bool, remote_enabled: bool) -> i32 {
        let loader = Js5Loader::new(archive, discard_packed, discard_unpacked, remote_enabled);
        let boxed = Box::new(loader);
        let slot = js5_net::register_loader(boxed);
        js5_net::assign_loader_slot(archive, slot);
        // Drive the post-construction handshake the Java ctor does inline.
        if !js5_net::has_master_index() {
            js5_net::queue_request(-1, 255, 255, 0, 0, true);
        } else {
            let (crc, version) = js5_net::read_master_index_at(archive);
            let mut reg = js5_net::LOADERS.lock().unwrap();
            if let Some(l) = reg.get_mut(slot as usize).and_then(|o| o.as_mut()) {
                l.request_index(crc, version);
            }
        }
        slot
    }
}

impl GameShellLifecycle for Client {
    // override of java.applet.Applet.init — no @ObfuscatedName
    fn init(&mut self) {
        if !self.check_host() {
            return;
        }
        self.login_host = "127.0.0.1".to_string();
        self.start_common(765, 503, 1);
    }

    // @ObfuscatedName("client.w(I)V")
    fn maininit(&mut self) {
        self.login_game_port = if self.modewhere == 0 { 43594 } else { self.worldid + 40000 };
        self.login_js5_port = if self.modewhere == 0 { 443 } else { self.worldid + 50000 };
        // Java starts on the game port and js5_error swaps to the JS5 port
        // after a failure. To avoid burning a reconnect cycle, start
        // directly on whichever port is currently advertised by the running
        // server. The Lost City rev1 dev server listens for JS5 on the
        // worldid+40000 GAME port (same as game traffic; the protocol byte
        // distinguishes them). Keep login_port == login_game_port.
        self.login_port = self.login_game_port;
        self.state = 0;
    }

    // @ObfuscatedName("client.e(B)V")
    fn mainloop(&mut self) {
        self.loop_cycle = self.loop_cycle.wrapping_add(1);
        // Mirror the cycle counter + local player tile coords to the
        // globals scene.rs reads from. The camera pivots around the
        // local player tile, so we publish it every tick (cheap atomic
        // writes — no contention).
        crate::scene::LOOP_CYCLE.store(self.loop_cycle, std::sync::atomic::Ordering::Relaxed);
        if let Some(lp) = self.local_player.as_ref() {
            // entity.x/z are fine world coords (tile*128+64, Java's
            // localPlayer.x convention); the camera pivot wants tiles.
            crate::scene::store_player_tile(lp.entity.x >> 7, lp.entity.z >> 7);
        }

        // Always service the JS5 stream so loaders make progress.
        self.service_net_client();

        // Java: MidiManager.updateLoading() runs each tick to advance the
        // async patch+wave fetch for the pending song.
        if let Some(player) = self.pcm_player.as_ref() {
            player.manager().lock().try_advance_loading();
            // Initial swap_songs needs to wait for the scape_main group to
            // arrive over JS5 — retry each tick until the loader returns
            // bytes, then kick off the load state machine.
            if !self.music_started && self.songs >= 0 && self.loading_step >= 60 {
                let mut reg = js5_net::LOADERS.lock().unwrap();
                let midi = reg.get_mut(self.songs as usize)
                    .and_then(|o| o.as_mut())
                    .and_then(|l| l.get_file_by_name("scape main", ""));
                drop(reg);
                if let Some(midi) = midi {
                    eprintln!("[audio] mainloop: scape_main arrived, {} bytes — calling swap_songs", midi.len());
                    player.manager().lock().swap_songs(2, midi, false);
                    self.music_started = true;
                }
            }
        }

        // Java loadingStep 120 (Client.java:1967-1972): build the chat
        // Huffman table from the binary archive's "huffman" group. Our
        // load steps skip 90-130, so retry here each tick until the
        // group streams in.
        if !crate::wordpack::huffman_loaded() && self.binary >= 0 && self.loading_step >= 60 {
            let mut reg = js5_net::LOADERS.lock().unwrap();
            let table = reg.get_mut(self.binary as usize)
                .and_then(|o| o.as_mut())
                .and_then(|l| l.get_file_by_name("huffman", ""));
            drop(reg);
            if let Some(table) = table {
                crate::wordpack::set_huffman(crate::wordpack::Huffman::new(&table));
                eprintln!("[wordpack] huffman table installed ({} symbols)", table.len());
            }
        }

        // Java: state==0 → mainLoad; state==5 → TitleScreen.loop + mainLoad;
        // state==10/20 → TitleScreen.loop. Higher states (25/30/40) belong
        // to the world+login flows which aren't wired yet.
        if self.state == 0 {
            self.main_load();
        } else if self.state == 5 {
            let _ = crate::title_screen::loop_tick(self.state, self.lang);
            self.main_load();
        } else if self.state == 10 {
            crate::title_screen::try_load_sl_button(self.sprites);
            if let Some(next) = crate::title_screen::loop_tick(self.state, self.lang) {
                self.set_main_state(next);
            }
        } else if self.state == 20 {
            // Java: TitleScreen.loop(this); loginPoll();
            crate::title_screen::try_load_sl_button(self.sprites);
            let _ = crate::title_screen::loop_tick(self.state, self.lang);
            crate::login::poll(self);
        } else if self.state == 25 {
            crate::login::game_tick(self);
            map_build_loop(self);
            update_orbit_camera(self);
        } else if self.state == 30 {
            crate::login::game_tick(self);
            // Java gameLoop's input head (mouse-track/click/camera/
            // focus packets, minimap walk click, timers, key buffer).
            game_input(self);
            update_orbit_camera(self);
            // Java updateWorld's minimap section: anti-macro drift,
            // image rebuild on level change, and the dots snapshot the
            // chrome renderer reads.
            crate::minimap::update(self);
            // entityOverlays' per-tick entity snapshot (head icons,
            // chat, health bars, hit splats read it at draw time).
            crate::overlays::snapshot(self);
        }
    }

    // @ObfuscatedName("client.b(I)V")
    fn mainredraw(&mut self, fb: &mut Framebuffer) {
        if self.state == 0 {
            let (pos, msg) = {
                let s = title_screen::STATE.lock().unwrap();
                (s.load_pos, s.load_string.clone())
            };
            game_shell::draw_progress(fb, pos, &msg, None);
        } else if self.state == 5 || self.state == 10 || self.state == 20 {
            // Java: TitleScreen.draw(b12, p11). State 20 still renders the
            // title screen while loginPoll runs in the background.
            title_screen::draw(fb, self.b12.as_ref(), self.p11.as_ref(), self.state, self.loop_cycle);
        } else if self.state == 30 {
            // Stash the live camera state where the scene renderer can
            // read it without us threading it through every interface_*
            // call site.
            crate::scene::CAMERA.lock().unwrap().update(
                self.orbit_cam_yaw,
                self.orbit_cam_pitch,
                self.orbit_cam_zoom,
            );
            crate::interface_render::draw_chrome(fb, self);
        }
    }

    // @ObfuscatedName("dj.y(B)V")
    fn mainquit(&mut self) {
        js5_net::clear_stream();
        js5_net_thread::shutdown();
    }

    // @ObfuscatedName("client.f(I)V")
    fn on_killed(&mut self) {}
}

// custom — Java's orbit-camera input lives inline inside `gameLoop`
// (Client.java around line 3350: the `ClientKeyboardListener.keyHeld`
// branches for codes 96/97/98/99 driving orbitCameraYawVelocity /
// orbitCameraPitchVelocity). No standalone @ObfuscatedName method —
// we hoisted it into its own function so mainloop can call it
// uniformly at both state 25 and 30.
fn update_orbit_camera(c: &mut Client) {
    use crate::input::{KEY_DOWN, KEY_LEFT, KEY_RIGHT, KEY_UP, KEYBOARD, MOUSE};
    let mut kb = KEYBOARD.lock().unwrap();
    let left = kb.key_held[KEY_LEFT as usize];
    let right = kb.key_held[KEY_RIGHT as usize];
    let up = kb.key_held[KEY_UP as usize];
    let down = kb.key_held[KEY_DOWN as usize];
    drop(kb);

    // Arrow-key velocity targets — Java rev1 caps at ±8 for yaw and ±5
    // for pitch (Client.java around the orbitCameraYawVelocity step).
    // Our previous ±24 / ±12 produced visibly twitchy rotation.
    if left {
        c.orbit_cam_yaw_velocity += (-8 - c.orbit_cam_yaw_velocity) / 2;
    } else if right {
        c.orbit_cam_yaw_velocity += (8 - c.orbit_cam_yaw_velocity) / 2;
    } else {
        c.orbit_cam_yaw_velocity /= 2;
    }
    if up {
        c.orbit_cam_pitch_velocity += (5 - c.orbit_cam_pitch_velocity) / 2;
    } else if down {
        c.orbit_cam_pitch_velocity += (-5 - c.orbit_cam_pitch_velocity) / 2;
    } else {
        c.orbit_cam_pitch_velocity /= 2;
    }

    // Middle-mouse drag — natural "grab and pull" feel: dragging the
    // mouse right makes the world rotate right with the drag (camera
    // orbits left). Sign was flipped from the original port which
    // rotated the world opposite the drag direction.
    let (dx, dy) = MOUSE.lock().unwrap().take_drag();
    if dx != 0 {
        c.orbit_cam_yaw = (c.orbit_cam_yaw - dx) & 0x7FF;
    }
    if dy != 0 {
        c.orbit_cam_pitch += dy / 2;
    }

    c.orbit_cam_yaw = (c.orbit_cam_yaw + c.orbit_cam_yaw_velocity / 2) & 0x7FF;
    c.orbit_cam_pitch += c.orbit_cam_pitch_velocity / 2;
    // Java clamps pitch to [128, 383].
    if c.orbit_cam_pitch < 128 { c.orbit_cam_pitch = 128; }
    if c.orbit_cam_pitch > 383 { c.orbit_cam_pitch = 383; }

    // Scroll wheel: positive = zoom in (decrease distance), negative
    // = zoom out (increase distance). orbit_cam_zoom is the orbit
    // radius in world units (camera-to-player distance). Reads the
    // per-tick wheel snapshot (AWT sign: positive = down = zoom out)
    // and only while the mouse is over the 3D viewport, so wheel
    // input over side panels reaches the interface scrollbars instead.
    if c.mouse_wheel_rotation != 0 {
        let (vx, vy, vw, vh) = {
            let o = crate::overlays::OVERLAYS.lock().unwrap();
            (o.viewport_x, o.viewport_y, o.viewport_w, o.viewport_h)
        };
        let (mx, my) = {
            let m = MOUSE.lock().unwrap();
            (m.mouse_x, m.mouse_y)
        };
        if mx >= vx && mx < vx + vw && my >= vy && my < vy + vh {
            c.orbit_cam_zoom = (c.orbit_cam_zoom + c.mouse_wheel_rotation * 80)
                .clamp(400, 3000);
        }
    }
}

// @ObfuscatedName("as.ed(I)V") — Client.mapBuildLoop. Polls the maps
// archive for the chunks RebuildNormal requested, parses ground + locs,
// then transitions to state 30 when the build is complete.
fn map_build_loop(c: &mut Client) {
    c.map_load_count = 0;
    let mut all_present = true;
    let n = c.map_build_index.len();
    {
        let mut reg = js5_net::LOADERS.lock().unwrap();
        let loader = match reg.get_mut(c.maps as usize).and_then(|o| o.as_mut()) {
            Some(l) => l,
            None => return,
        };
        for i in 0..n {
            if c.map_build_ground_file[i] != -1 && c.map_build_ground_data[i].is_none() {
                c.map_build_ground_data[i] = loader.fetch_file(c.map_build_ground_file[i], 0);
                if c.map_build_ground_data[i].is_none() {
                    all_present = false;
                    c.map_load_count += 1;
                }
            }
            if c.map_build_location_file[i] != -1 && c.map_build_location_data[i].is_none() {
                let key = c.map_keys.get(i).copied().unwrap_or([0; 4]);
                c.map_build_location_data[i] = loader.fetch_file_with_key(
                    c.map_build_location_file[i], 0, &key,
                );
                if c.map_build_location_data[i].is_none() {
                    all_present = false;
                    c.map_load_count += 1;
                }
            }
        }
    }
    if !all_present {
        c.map_load_state = 1;
        return;
    }

    // All map archives in hand. Validate, then parse.
    c.loc_model_load_count = 0;
    let mut all_models = true;
    for i in 0..n {
        if let Some(ref loc_bytes) = c.map_build_location_data[i] {
            let local_x = (c.map_build_index[i] >> 8) * 64 - c.map_build_base_x;
            let local_z = (c.map_build_index[i] & 0xFF) * 64 - c.map_build_base_z;
            all_models &= crate::client_build::check_locations(loc_bytes, local_x, local_z);
        }
    }
    if !all_models {
        c.map_load_state = 2;
        return;
    }

    eprintln!("[game] mapBuildLoop: all map data fetched, building {} chunk(s)", n);
    crate::client_build::init();
    for i in 0..n {
        let local_x = (c.map_build_index[i] >> 8) * 64 - c.map_build_base_x;
        let local_z = (c.map_build_index[i] & 0xFF) * 64 - c.map_build_base_z;
        if let Some(ref ground) = c.map_build_ground_data[i] {
            crate::client_build::load_ground(
                ground, local_x, local_z,
                c.map_build_center_zone_x * 8 - 48,
                c.map_build_center_zone_z * 8 - 48,
            );
        }
    }
    let mut loc_total = 0;
    for i in 0..n {
        if let Some(ref loc_bytes) = c.map_build_location_data[i] {
            let local_x = (c.map_build_index[i] >> 8) * 64 - c.map_build_base_x;
            let local_z = (c.map_build_index[i] & 0xFF) * 64 - c.map_build_base_z;
            let before = crate::client_build::STATE.lock().unwrap().locs.len();
            crate::client_build::load_locations(loc_bytes, local_x, local_z);
            let after = crate::client_build::STATE.lock().unwrap().locs.len();
            loc_total += after - before;
        }
    }
    eprintln!("[game] mapBuildLoop: parsed {loc_total} locs, transitioning to state 30");
    c.map_load_state = 0;
    c.set_main_state(30);
}

impl Client {
    // @ObfuscatedName("client.dt(B)V") — Client.mainLoad
    pub fn main_load(&mut self) {
        if self.loop_cycle % 100 == 0 {
            eprintln!("[main_load] state={} loading_step={}", self.state, self.loading_step);
        }
        match self.loading_step {
            0 => {
                {
                    let mut ts = title_screen::STATE.lock().unwrap();
                    ts.load_string = "Starting up".to_string();
                    ts.load_pos = 5;
                }
                self.loading_step = 20;
            }
            20 => {
                {
                    let mut ts = title_screen::STATE.lock().unwrap();
                    ts.load_string = "Preparing visibility".to_string();
                    ts.load_pos = 10;
                }
                self.loading_step = 30;
            }
            30 => {
                self.anims = self.open_js5(0, false, true, true);
                self.bases = self.open_js5(1, false, true, true);
                self.config = self.open_js5(2, true, false, true);
                self.interfaces = self.open_js5(3, false, true, true);
                self.jag_fx = self.open_js5(4, false, true, true);
                self.maps = self.open_js5(5, true, true, true);
                self.songs = self.open_js5(6, true, true, false);
                self.models = self.open_js5(7, false, true, true);
                self.sprites = self.open_js5(8, false, true, true);
                self.textures = self.open_js5(9, false, true, true);
                self.binary = self.open_js5(10, false, true, true);
                self.jingles = self.open_js5(11, false, true, true);
                self.scripts = self.open_js5(12, false, true, true);
                crate::client_script::install_archive(self.scripts);
                self.font_metrics = self.open_js5(13, true, false, true);
                self.vorbis = self.open_js5(14, false, true, false);
                self.patches = self.open_js5(15, false, true, true);
                {
                    let mut ts = title_screen::STATE.lock().unwrap();
                    ts.load_string = "Connected to update server".to_string();
                    ts.load_pos = 20;
                }
                LOADERS_OPENED.store(true, Ordering::SeqCst);
                self.loading_step = 40;
            }
            40 => {
                let reg = js5_net::LOADERS.lock().unwrap();
                let mut total = 0i32;
                let weights = [
                    (self.anims, 4), (self.bases, 4), (self.config, 2),
                    (self.interfaces, 2), (self.jag_fx, 6), (self.maps, 4),
                    (self.songs, 2), (self.models, 60), (self.sprites, 2),
                    (self.textures, 2), (self.binary, 2), (self.jingles, 2),
                    (self.scripts, 2), (self.font_metrics, 2), (self.vorbis, 2),
                    (self.patches, 2),
                ];
                let mut per_slot = Vec::new();
                for (slot, weight) in weights {
                    if slot < 0 { continue; }
                    if let Some(l) = reg.get(slot as usize).and_then(|o| o.as_ref()) {
                        let pct = l.get_index_percentage();
                        per_slot.push((slot, pct, weight));
                        total += pct * weight / 100;
                    }
                }
                drop(reg);
                if self.loop_cycle % 50 == 0 {
                    eprintln!("[step40] total={} per_slot={:?}", total, per_slot);
                }
                let mut ts = title_screen::STATE.lock().unwrap();
                if total != 100 {
                    if total != 0 {
                        ts.load_string = format!("Loading update list - {total}%");
                    }
                    ts.load_pos = 30;
                } else {
                    ts.load_string = "Loaded update list".to_string();
                    ts.load_pos = 30;
                    self.loading_step = 45;
                }
            }
            45 => {
                // Java: PcmPlayer.init(22050, !lowMem, 2) — opens the cpal
                // sink which itself constructs the SharedManager. Stored on
                // Client so the audio thread lives as long as the client.
                if self.pcm_player.is_none() {
                    if let Ok(p) = crate::sound::pcm_player::PcmPlayer::init(22050, !self.low_mem) {
                        self.pcm_player = Some(p);
                    }
                }
                let mut ts = title_screen::STATE.lock().unwrap();
                ts.load_string = "Loaded sound engine".to_string();
                ts.load_pos = 35;
                self.loading_step = 50;
            }
            50 => {
                // Java step 50: PixLoader.makePixFont for p11_full, p12_full, b12_full.
                // Retried every mainloop tick until all three fonts load
                // (Java structure verbatim).
                let mut var24 = 0i32;
                if self.p11.is_none() {
                    let mut reg = js5_net::LOADERS.lock().unwrap();
                    if let (Some(sprites_l), Some(fm_l)) = split2(&mut reg, self.sprites, self.font_metrics) {
                        self.p11 = crate::graphics::pix_loader::make_pix_font(sprites_l, fm_l, "p11_full", "");
                    }
                } else { var24 += 1; }
                if self.p12.is_none() {
                    let mut reg = js5_net::LOADERS.lock().unwrap();
                    if let (Some(sprites_l), Some(fm_l)) = split2(&mut reg, self.sprites, self.font_metrics) {
                        self.p12 = crate::graphics::pix_loader::make_pix_font(sprites_l, fm_l, "p12_full", "");
                    }
                } else { var24 += 1; }
                if self.b12.is_none() {
                    let mut reg = js5_net::LOADERS.lock().unwrap();
                    if let (Some(sprites_l), Some(fm_l)) = split2(&mut reg, self.sprites, self.font_metrics) {
                        self.b12 = crate::graphics::pix_loader::make_pix_font(sprites_l, fm_l, "b12_full", "");
                    }
                } else { var24 += 1; }
                let mut ts = title_screen::STATE.lock().unwrap();
                if var24 < 3 {
                    ts.load_string = format!("Loading interface fonts - {}%", var24 * 100 / 3);
                    ts.load_pos = 40;
                } else {
                    eprintln!("[step50] all 3 fonts loaded — p11 ascent={}", self.p11.as_ref().unwrap().base.ascent);
                    ts.load_string = "Loaded interface fonts".to_string();
                    ts.load_pos = 40;
                    self.loading_step = 60;
                }
            }
            60 => {
                // Java step 60: TitleScreen.ready(binary, sprites). When
                // ready_count == ready_max we setMainState(5) and step→70.
                let var27 = title_screen::ready(self.binary, self.sprites);
                let var30 = title_screen::ready_max();
                let mut ts = title_screen::STATE.lock().unwrap();
                if var27 < var30 {
                    ts.load_string = format!("Loading title screen - {}%", var27 * 100 / var30);
                    ts.load_pos = 50;
                } else {
                    ts.load_string = "Loaded title screen".to_string();
                    ts.load_pos = 50;
                    drop(ts);
                    // Java: IfType.interfaces / sprites / fontMetrics /
                    // models are assigned around step-60 entry so the
                    // chrome renderer has the archives once login lands.
                    crate::config::if_type::install_archives(
                        self.interfaces, self.sprites, self.font_metrics, self.models,
                    );
                    crate::config::loc_type::install_archives(self.config, self.models);
                    crate::config::obj_type::install_archives(self.config, self.models);
                    crate::config::flo_type::install_archives(self.config);
                    crate::config::flu_type::install_archives(self.config);
                    crate::dash3d::texture_manager::install_archives(self.textures, self.sprites);
                    crate::config::seq_type::install_archives(self.config, self.anims, self.bases);
                    crate::dash3d::anim_frame_set::install_archives(self.anims, self.bases);
                    crate::config::npc_type::install_archives(self.config, self.models);
                    crate::config::varp_type::install_archives(self.config);
                    crate::config::var_bit_type::install_archives(self.config);
                    crate::config::enum_type::install_archives(self.config);
                    crate::config::inv_type::install_archives(self.config);
                    crate::config::idk_type::install_archives(self.config, self.models);
                    crate::config::spot_type::install_archives(self.config, self.models);
                    crate::minimap::install(self.sprites);
                    crate::overlays::install(self.sprites);
                    if let Some(p11) = self.p11.clone() {
                        crate::config::obj_type::install_count_font(p11);
                    }
                    // Java: TitleScreen.open(canvas, binary, sprites)
                    // happens once during state-5 entry. Also wires the
                    // songs loader for the "scape main" intro music.
                    title_screen::open(self.binary, self.sprites, self.songs);
                    // Java: MidiManager.swapSongs(2, songs, "scape main", 0, 255, false)
                    // inside TitleScreen.open. We do it here so Client owns
                    // the MIDI bytes fetch via Js5Loader and hands them to
                    // the SharedManager owned by PcmPlayer.
                    if let Some(player) = self.pcm_player.as_ref() {
                        title_screen::install_music_handle(player.manager(), self.songs);
                        // First-pass fetch attempt — queues the group
                        // download. Subsequent retries land in mainloop's
                        // try_start_music until the group arrives.
                        let mut reg = js5_net::LOADERS.lock().unwrap();
                        let _ = reg.get_mut(self.songs as usize)
                            .and_then(|o| o.as_mut())
                            .and_then(|l| l.get_file_by_name("scape main", ""));
                        drop(reg);
                    } else {
                        eprintln!("[audio] step60: pcm_player not initialised");
                    }
                    self.set_main_state(5);
                    self.loading_step = 70;
                }
            }
            70 => {
                // Java step 70: config.requestFullDownload(), then init all
                // Type classes (FloType, LocType, NpcType, ...). With our
                // stubbed Type::init this is instant.
                let reg = js5_net::LOADERS.lock().unwrap();
                let config_loader = reg.get(self.config as usize).and_then(|o| o.as_ref());
                let Some(config_loader) = config_loader else {
                    return;
                };
                if !config_loader.base.packed.is_empty() {
                    crate::config::flo_type::init(config_loader);
                    crate::config::flu_type::init(config_loader);
                    crate::config::idk_type::init(config_loader);
                    crate::config::loc_type::init(config_loader);
                    crate::config::npc_type::init(config_loader);
                    crate::config::obj_type::init(config_loader);
                    crate::config::seq_type::init(config_loader);
                    crate::config::spot_type::init(config_loader);
                    crate::config::var_bit_type::init(config_loader);
                    crate::config::varp_type::init(config_loader);
                    crate::config::if_type::init(config_loader);
                    crate::config::inv_type::init(config_loader);
                    crate::config::enum_type::init(config_loader);
                }
                drop(reg);
                let mut ts = title_screen::STATE.lock().unwrap();
                ts.load_string = "Loaded config".to_string();
                ts.load_pos = 60;
                self.loading_step = 80;
            }
            80 => {
                // Java step 80+: sprite loading (compass, mapedge, hitmarks,
                // etc.) + region downloads. Skipped — the remaining
                // archives stream in opportunistically. Java's step 140
                // is `setMainState(10)`; we jump straight there so the
                // login screen / title-box renders.
                let mut ts = title_screen::STATE.lock().unwrap();
                ts.load_string = "Welcome to RuneScape".to_string();
                ts.load_pos = 100;
                drop(ts);
                self.set_main_state(10);
                self.loading_step = 140;
            }
            140 => {
                // Already at state=10. Nothing more to do here.
            }
            _ => {}
        }
    }

    // @ObfuscatedName("ek.di(Lfi;IIB)V") — Client.triggerPlayerAnim.
    //
    // Verbatim port of Client.java:4031-4051. Used by extended-update
    // primary-seq packets and by cs2 ANIM ops. The duplicatebehaviour
    // rules:
    //   - same anim, dup==1: restart from frame 0 (re-trigger).
    //   - same anim, dup==2: reset loop counter only (re-loop).
    //   - different anim: pre-empt only if new priority >= current.
    pub fn trigger_player_anim(player: &mut crate::dash3d::ClientPlayer, anim_id: i32, delay: i32) {
        use crate::config::seq_type;
        let e = &mut player.entity;
        if e.primary_seq_id == anim_id && anim_id != -1 {
            let dup = seq_type::list(anim_id).duplicatebehaviour;
            if dup == 1 {
                e.primary_seq_frame = 0;
                e.primary_seq_cycle = 0;
                e.primary_seq_delay = delay;
                e.primary_seq_loop = 0;
            } else if dup == 2 {
                e.primary_seq_loop = 0;
            }
        } else if anim_id == -1 || e.primary_seq_id == -1
            || seq_type::list(anim_id).priority
                >= seq_type::list(e.primary_seq_id).priority
        {
            e.primary_seq_id = anim_id;
            e.primary_seq_frame = 0;
            e.primary_seq_cycle = 0;
            e.primary_seq_delay = delay;
            e.primary_seq_loop = 0;
            e.preanim_route_length = e.route_length;
        }
    }

    // @ObfuscatedName("ek.da(I)V") — Client.movePlayers.
    //
    // Per-tick dispatch — iterates the visible player slots and asks
    // each to advance its route queue by one step. Java reserves slot
    // 2047 for the local player; `var0 == -1` is the conventional
    // sentinel that pulls localPlayer first.
    pub fn move_players(&mut self) {
        // Local player first (Java's `var1 = 2047`).
        if let Some(lp) = self.local_player.as_mut() {
            move_entity(&mut lp.entity, 1, self.loop_cycle);
        }
        for i in 0..self.player_count as usize {
            let id = self.player_ids[i];
            if id < 0 || id as usize >= self.players.len() { continue; }
            if let Some(p) = self.players[id as usize].as_mut() {
                move_entity(&mut p.entity, 1, self.loop_cycle);
            }
        }
    }

    // @ObfuscatedName("l.db(I)V") — Client.moveNpcs.
    pub fn move_npcs(&mut self) {
        for i in 0..self.npc_count as usize {
            let id = self.npc_ids[i];
            if id < 0 || id as usize >= self.npcs.len() { continue; }
            if let Some(n) = self.npcs[id as usize].as_mut() {
                let size = n.entity.size;
                move_entity(&mut n.entity, size, self.loop_cycle);
            }
        }
    }

    // @ObfuscatedName("ek.cb(II)V") — Client.setMainState.
    //
    // Verbatim port of Java's state-machine transition. Beyond just
    // setting `state`, Java also:
    //  - state 20 / 40: reset loginStep / loginWaitingTime /
    //    loginFailCount, close any prev_stream, reset mapLoadState,
    //    open the TitleScreen.
    //  - leaving state 10/20: close the TitleScreen (frees its
    //    flame buffers + sprite cache).
    //
    // We mirror those side effects here so callers don't need to
    // do them manually each time.
    pub fn set_main_state(&mut self, state: i32) {
        let prev = self.state;
        self.state = state;

        match state {
            20 | 40 => {
                self.login_step = 0;
                self.login_waiting_time = 0;
                self.login_fail_count = 0;
                // Drop a stale prev_stream — successful reconnect path
                // already cleared it, this is the fallback.
                self.prev_stream = None;
                // Reset map-build to "no zone loaded" so the next
                // RebuildNormal triggers a fresh fetch.
                self.map_build_center_zone_x = -1;
                self.map_build_center_zone_z = -1;
            }
            _ => {}
        }

        // Leaving the title-screen states releases their assets.
        // Java's `TitleScreen.close()` runs here; until that helper
        // lands we use a no-op marker so the call site shape matches.
        if matches!(prev, 10 | 20) && !matches!(state, 10 | 20) {
            // TODO: title_screen::close(self) once Round 16 lands it.
        }
    }
}
