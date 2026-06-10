// @ObfuscatedName("client.loginPoll")
// jagex3.client.Client::loginPoll + loginDone + loginError.
//
// Verbatim port of the rev1 login state machine. Crypto is bypassed
// (deob.Settings.NO_RSA / NO_TINYENC / NO_ISAAC = true) to match the
// Java client and Engine2007 server which both stub out RSA/XTEA/ISAAC.
// Net result on the wire:
//
//  • opcode 14 (init connection) → 1 byte
//  • read 1 byte (0 = ok)
//  • opcode 16 (login) + u16 size + u32 revision
//      + u16 inner_len + [u8 sentinel=10, u32×4 seeds, u64 isaac0=0, jstr password]
//      + jstr username + u8 lowmem + 24 byte UID + 16×u32 archive CRCs
//  • read 6 bytes [opcode 2, staffmod, mod, slot_hi, slot_lo, members]
//  • read u8 ptype + u16 psize + psize bytes (first game packet, e.g. RebuildNormal)

#![allow(dead_code)]

use std::sync::atomic::Ordering;

use crate::applet::privileged_request::{Result as PReqResult, STATUS_DONE, STATUS_ERROR};
use crate::client::Client;
use crate::io::client_stream::ClientStream;
use crate::io::packet::Packet;
use crate::js5::js5_net;

// @ObfuscatedName("client.loginPoll(B)V")
pub fn poll(c: &mut Client) {
    if c.login_step == 0 {
        // Cleanup any previous stream.
        c.login_stream = None;
        c.login_socket_req = None;
        c.network_error = false;
        c.login_waiting_time = 0;
        c.login_step = 1;
    }

    if c.login_step == 1 {
        if c.login_socket_req.is_none() {
            eprintln!("[login] socketreq {}:{}", c.login_host, c.login_port);
            c.login_socket_req = Some(c.signlink.socketreq(&c.login_host, c.login_port));
        }
        let status = c.login_socket_req.as_ref().map(|r| r.status()).unwrap_or(STATUS_ERROR);
        if status == STATUS_ERROR {
            eprintln!("[login] socketreq ERROR");
            login_retry_or_error(c, -2);
            return;
        }
        if status == STATUS_DONE {
            let stream_inner = {
                let req = c.login_socket_req.as_ref().unwrap();
                let mut result_guard = req.result.lock().unwrap();
                std::mem::replace(&mut *result_guard, PReqResult::None)
            };
            let socket = match stream_inner {
                PReqResult::Socket(s) => s,
                _ => {
                    eprintln!("[login] socketreq DONE but no socket");
                    login_retry_or_error(c, -2);
                    return;
                }
            };
            match ClientStream::new(socket) {
                Ok(s) => c.login_stream = Some(s),
                Err(e) => {
                    eprintln!("[login] ClientStream::new failed: {e}");
                    login_retry_or_error(c, -2);
                    return;
                }
            }
            c.login_socket_req = None;
            c.login_step = 2;
        }
    }

    if c.login_step == 2 {
        // Java: out.p1(14); stream.write(out, 0, 1); → INIT_GAME_CONNECTION
        let stream = c.login_stream.as_mut().unwrap();
        if stream.write(&[14u8], 0, 1).is_err() {
            eprintln!("[login] write opcode 14 failed");
            login_retry_or_error(c, -2);
            return;
        }
        c.login_step = 3;
    }

    if c.login_step == 3 {
        let stream = c.login_stream.as_mut().unwrap();
        let available = stream.available().unwrap_or(0);
        if available <= 0 {
            return;
        }
        let resp = match stream.read_byte() {
            Ok(v) => v,
            Err(_) => { login_retry_or_error(c, -2); return; }
        };
        if resp != 0 {
            eprintln!("[login] step 3: server replied {resp} — login error");
            login_error(c, resp);
            return;
        }
        c.login_step = 5;
    }

    if c.login_step == 5 {
        // Build the login packet. Java: out.pos=0; out.p1(10); seeds; isaac0;
        // password; rsaenc. With NO_RSA, rsaenc just wraps with [u16 len, raw].
        // Then loginout: opcode 16, u16 size, u32 rev, out.data, username,
        // lowmem, 24 byte UID, CRCs, tinyenc (no-op with NO_TINYENC).

        // Random seeds — Java uses Math.random() but value isn't validated.
        c.login_seed = [seeded_rand(c), seeded_rand(c), seeded_rand(c), seeded_rand(c)];

        // Wire format expected by Engine2007 (rev1). NOTE: this differs
        // from Java's RSA path which wraps the seed+password block with a
        // u16 length. Engine2007 reads seed+password flat, then a u16
        // rsa_len + rsa_len bytes (empty in NO_RSA mode), then the
        // XTEA-wrapped block (also flat in NO_TINYENC mode).
        let mut out = Packet::from_vec(vec![0u8; 8192]);
        out.p1(16);
        out.p2(0); // size placeholder
        let start = out.pos as i32;
        out.p4(1); // revision

        // Flat seed+password block.
        out.p1(10);                     // isaac10 sentinel
        out.p4(c.login_seed[0]);
        out.p4(c.login_seed[1]);
        out.p4(c.login_seed[2]);
        out.p4(c.login_seed[3]);
        out.p8(0);                      // isaac0
        out.pjstr(&c.login_pass());

        // Empty RSA block.
        out.p2(0);

        // XTEA-wrapped block in NO_TINYENC mode: raw bytes.
        out.pjstr(&c.login_user());
        out.p1(if c.low_mem { 1 } else { 0 });
        // GameShellCache.pushUID192 — 24 bytes. Zeros.
        for _ in 0..24 { out.p1(0); }
        // 16 archive CRCs — anims, bases, config, interfaces, jagFX,
        // maps, songs, models, sprites, textures, binary, jingles,
        // scripts, fontMetrics, vorbis, patches.
        let crcs = archive_crcs(c);
        for crc in crcs.iter() { out.p4(*crc); }

        // Write u16 size into the placeholder.
        let size = out.pos as i32 - start;
        out.data[1] = (size >> 8) as u8;
        out.data[2] = size as u8;

        eprintln!("[login] sending opcode 16, total {} bytes (payload size={})", out.pos, size);

        let stream = c.login_stream.as_mut().unwrap();
        let n = out.pos as i32;
        let bytes: Vec<u8> = out.data[..n as usize].to_vec();
        if stream.write(&bytes, 0, n).is_err() {
            eprintln!("[login] write login packet failed");
            login_retry_or_error(c, -2);
            return;
        }

        // @ObfuscatedName("ev.bg([II)V") — Packet.seed (ISAAC init).
        //
        // Java seeds two Isaac instances after the login packet is
        // sent: one for outbound (`out.seed(seed)`) using the four
        // session keys directly, one for inbound with each key
        // shifted by +50. We keep the instances on the Client so
        // future opcode reads/writes (post-NO_ISAAC) can use g1Enc /
        // p1Enc against them.
        c.isaac_out = Some(crate::io::isaac::Isaac::new(&c.login_seed));
        let in_seed = [
            c.login_seed[0].wrapping_add(50),
            c.login_seed[1].wrapping_add(50),
            c.login_seed[2].wrapping_add(50),
            c.login_seed[3].wrapping_add(50),
        ];
        c.isaac_in = Some(crate::io::isaac::Isaac::new(&in_seed));

        c.login_step = 6;
    }

    if c.login_step == 6 {
        let stream = c.login_stream.as_mut().unwrap();
        if stream.available().unwrap_or(0) <= 0 { return; }
        let var5 = match stream.read_byte() {
            Ok(v) => v,
            Err(_) => { login_retry_or_error(c, -2); return; }
        };
        eprintln!("[login] step 6: server replied opcode {var5}");
        if var5 == 21 && c.state == 20 {
            c.login_step = 7;
        } else if var5 == 2 {
            c.login_step = 9;
        } else if var5 == 23 && c.login_fail_count < 1 {
            c.login_fail_count += 1;
            c.login_step = 0;
        } else {
            login_error(c, var5);
            return;
        }
    }

    if c.login_step == 7 {
        let stream = c.login_stream.as_mut().unwrap();
        if stream.available().unwrap_or(0) <= 0 { return; }
        let hop = match stream.read_byte() {
            Ok(v) => v,
            Err(_) => { login_retry_or_error(c, -2); return; }
        };
        c.login_hop_timer = (hop + 3) * 60;
        c.login_step = 8;
    }

    if c.login_step == 8 {
        c.login_waiting_time = 0;
        c.login_hop_timer -= 1;
        if c.login_hop_timer <= 0 {
            c.login_step = 0;
        }
        return;
    }

    if c.login_step == 9 {
        let stream = c.login_stream.as_mut().unwrap();
        if stream.available().unwrap_or(0) < 8 { return; }
        let mut buf = [0u8; 8];
        if stream.read(&mut buf, 0, 8).is_err() {
            login_retry_or_error(c, -2);
            return;
        }
        c.staff_mod_level = buf[0] as i32;
        c.player_mod = buf[1] == 1;
        c.self_slot = ((buf[2] as i32) << 8) | (buf[3] as i32);
        c.members_account = buf[4] as i32;
        // Java: in.g1Enc() — ISAAC-decoded. With NO_ISAAC, just g1.
        c.ptype = buf[5] as i32;
        c.psize = ((buf[6] as i32) << 8) | (buf[7] as i32);
        eprintln!("[login] step 9: staff={} mod={} slot={} members={} ptype={} psize={}",
            c.staff_mod_level, c.player_mod, c.self_slot, c.members_account, c.ptype, c.psize);
        c.login_step = 10;
    }

    if c.login_step == 10 {
        let stream = c.login_stream.as_mut().unwrap();
        if stream.available().unwrap_or(0) < c.psize { return; }
        let mut buf = vec![0u8; c.psize as usize];
        if stream.read(&mut buf, 0, c.psize).is_err() {
            login_retry_or_error(c, -2);
            return;
        }
        eprintln!("[login] step 10: read {} byte first packet — login DONE", c.psize);
        login_done(c, buf);
    }
}

// @ObfuscatedName("client.tcpIn") — server packet drain. Mirrors Java's
// tcpIn() state machine: read opcode → look up size in SERVERPROT_SIZES
// → optionally read u8/u16 length prefix → read payload.
pub fn game_tick(c: &mut Client) {
    use crate::obfuscation::SERVERPROT_SIZES;
    // Periodic heartbeat — Engine2007 accepts opcode 228 with no payload
    // as a NoOp. Sent every ~3 s so the server doesn't time us out.
    c.heartbeat_ticker += 1;
    if c.heartbeat_ticker >= 150 {
        c.heartbeat_ticker = 0;
        if let Some(stream) = c.login_stream.as_mut() {
            let _ = stream.write(&[228u8], 0, 1);
        }
    }
    // Drain whatever is currently buffered.
    loop {
        // Read header bytes — return when not enough is available.
        if c.ptype == -1 {
            let Some(stream) = c.login_stream.as_mut() else { return };
            if stream.available().unwrap_or(0) < 1 { return; }
            c.ptype = match stream.read_byte() { Ok(v) => v, Err(_) => return };
            c.psize = SERVERPROT_SIZES[(c.ptype & 0xFF) as usize];
        }
        if c.psize == -1 {
            let Some(stream) = c.login_stream.as_mut() else { return };
            if stream.available().unwrap_or(0) < 1 { return; }
            let b = match stream.read_byte() { Ok(v) => v, Err(_) => return };
            c.psize = b & 0xFF;
        } else if c.psize == -2 {
            let Some(stream) = c.login_stream.as_mut() else { return };
            if stream.available().unwrap_or(0) < 2 { return; }
            let mut hdr = [0u8; 2];
            if stream.read(&mut hdr, 0, 2).is_err() { return; }
            c.psize = ((hdr[0] as i32) << 8) | (hdr[1] as i32);
        }
        // Payload read.
        let buf = {
            let Some(stream) = c.login_stream.as_mut() else { return };
            if c.psize > 0 && stream.available().unwrap_or(0) < c.psize { return; }
            let mut buf = vec![0u8; c.psize.max(0) as usize];
            if c.psize > 0 && stream.read(&mut buf, 0, c.psize).is_err() { return; }
            buf
        };
        let ptype = c.ptype;
        c.ptype = -1;
        c.psize = 0;
        // Each successful packet resets the server-silence timer.
        c.timeout_timer = 0;
        handle_packet(c, ptype, &buf);
    }
}

// @ObfuscatedName(— Client.gameTickHousekeeping). Verbatim port of
// the per-tick block at Client.java:2422-2429. Bumps the silence
// timer, walks every entity's per-tick interpolation + chat decay,
// and advances the loc-change queue. Call once per server tick after
// the inbound packet drain.
pub fn game_tick_housekeeping(c: &mut Client) {
    c.timeout_timer += 1;
    if c.timeout_timer > 750 {
        crate::client::lost_con(c);
        return;
    }
    crate::client::move_players(c);
    crate::client::move_npcs(c);
    crate::client::timeout_chat(c);
    crate::client::loc_change_do_queue(c);
    // Projectile / map-anim motion advances per FRAME in Java's
    // addProjectiles/addMapAnim (scaled by worldUpdateNum); it now
    // runs from scene::push_entities like Java's drawScene prologue.
    if c.cinema_cam {
        crate::client::cinema_camera(c);
    } else {
        // Non-cinema path: follow_camera tracks toward the player and
        // dispatch_orbit_camera projects the orbit through cam_follow.
        crate::client::follow_camera(c);
        crate::client::dispatch_orbit_camera(c, 0);
    }
    crate::client::apply_cam_shake(c);
    crate::client::bump_cam_shake_cycles(c);
    crate::client::macro_camera_drift(c);
    crate::client::decay_damage_cycles(c);
    crate::client::jingle_complete_check(c);
    c.world_update_num += 1;
    if c.logout_timer > 0 {
        c.logout_timer -= 1;
        if c.logout_timer == 0 {
            crate::client::logout(c);
        }
    }
}

// Decoders for the packets Engine2007 sends post-login. Tags lifted from
// Engine2007's TS encoder files. Most of these are no-ops on the client
// side until we wire the gameplay state — but printing the text payloads
// proves the wire protocol is intact.
fn handle_packet(c: &mut Client, opcode: i32, buf: &[u8]) {
    use crate::io::packet::Packet;
    use crate::client::SubInterface;
    use crate::config::if_type;
    use crate::config::var_cache;
    let mut p = Packet::from_vec(buf.to_vec());
    match opcode {
        // ── Chat / message ────────────────────────────────────────
        100 => { // MESSAGE_GAME — verbatim port of Client.java:5842-5897.
                 // Suffix-encoded message dispatch: the server packs
                 // social-system requests (trade/duel/chal/assist) into
                 // chat messages with magic `:tag:` suffixes the client
                 // pulls apart for special handling.
            let msg = p.gjstr();
            let chat_disabled = false; // mute-all toggle isn't yet
                                        // surfaced; chatDisabled defaults
                                        // to 0 (allow).
            if let Some(sender_end) = msg.find(':') {
                let sender = &msg[..sender_end];
                if msg.ends_with(":tradereq:") {
                    if !chat_disabled && !crate::client::is_ignored(c, Some(sender)) {
                        crate::client::add_chat(c, 4, Some(sender.to_string()),
                            Some("wishes to trade with you.".to_string()),
                            None, 0);
                    }
                } else if msg.ends_with(":duelreq:") {
                    if !chat_disabled && !crate::client::is_ignored(c, Some(sender)) {
                        crate::client::add_chat(c, 8, Some(sender.to_string()),
                            Some("wishes to duel with you.".to_string()),
                            None, 0);
                    }
                } else if msg.ends_with(":chalreq:") {
                    if !chat_disabled && !crate::client::is_ignored(c, Some(sender)) {
                        let body = &msg[sender_end + 1 .. msg.len() - ":chalreq:".len()];
                        crate::client::add_chat(c, 8, Some(sender.to_string()),
                            Some(body.to_string()), None, 0);
                    }
                } else if msg.ends_with(":assistreq:") {
                    if !chat_disabled && !crate::client::is_ignored(c, Some(sender)) {
                        crate::client::add_chat(c, 10, Some(sender.to_string()),
                            Some(String::new()), None, 0);
                    }
                } else if let Some(idx) = msg.find(":clan:") {
                    let body = &msg[..idx];
                    crate::client::add_chat(c, 11, None, Some(body.to_string()), None, 0);
                } else if let Some(idx) = msg.find(":trade:") {
                    if !chat_disabled {
                        let body = &msg[..idx];
                        crate::client::add_chat(c, 12, None, Some(body.to_string()), None, 0);
                    }
                } else if let Some(idx) = msg.find(":assist:") {
                    if !chat_disabled {
                        let body = &msg[..idx];
                        crate::client::add_chat(c, 13, None, Some(body.to_string()), None, 0);
                    }
                } else {
                    crate::client::add_chat(c, 0, None, Some(msg.clone()), None, 0);
                }
            } else {
                crate::client::add_chat(c, 0, None, Some(msg.clone()), None, 0);
            }
        }
        86 => { // MESSAGE_PRIVATE — verbatim port of Client.java:5977-6009.
                // 40-bit message id deduped against the 100-deep ring;
                // ignored senders and duplicates are dropped; staffmod
                // senders get the crown img tag. Payload is WordPack-
                // packed CP1252.
            let sender = p.gjstr();
            let id_hi = p.g2() as i64;
            let id_lo = p.g3() as i64;
            let mode = p.g1();
            let msg_id = (id_hi << 32) + id_lo;
            let mut dupe = c.message_ids.contains(&msg_id);
            if crate::client::is_ignored(c, Some(&sender)) {
                dupe = true;
            }
            if !dupe {
                c.message_ids[c.private_message_count as usize] = msg_id;
                c.private_message_count = (c.private_message_count + 1) % 100;
                let text = crate::graphics::pix_font::PixFont::escape(
                    &crate::jstring::force_capitalisation_of_words(
                        &crate::wordpack::unpack(&mut p)));
                if mode == 2 || mode == 3 {
                    crate::client::add_chat(
                        c, 7,
                        Some(format!("{}{sender}", crate::string_constants::tag_img(1))),
                        Some(text), None, 0);
                } else if mode == 1 {
                    crate::client::add_chat(
                        c, 7,
                        Some(format!("{}{sender}", crate::string_constants::tag_img(0))),
                        Some(text), None, 0);
                } else {
                    crate::client::add_chat(c, 3, Some(sender), Some(text), None, 0);
                }
            }
        }
        168 => { // MESSAGE_PRIVATE_ECHO — verbatim port of
                 // Client.java:5800-5808.
            let recipient = p.gjstr();
            let text = crate::graphics::pix_font::PixFont::escape(
                &crate::jstring::force_capitalisation_of_words(
                    &crate::wordpack::unpack(&mut p)));
            crate::client::add_chat(c, 6, Some(recipient), Some(text), None, 0);
        }
        57 => { // MESSAGE_FRIENDCHANNEL
            let _channel = p.gjstr();
            eprintln!("[game] MessageFriendChannel");
        }

        // ── Interface dispatch ────────────────────────────────────
        147 => { // IF_OPENTOP
            let id = p.g2_alt1();
            eprintln!("[game] IfOpenTop: toplevel={id}");
            c.toplevelinterface = id;
            if_type::open_interface(id, c.interfaces);
            // Java Client.java:6059 — fire the group's onload hooks.
            crate::script_runner::execute_onload(c, id);
        }
        184 => { // IF_OPENSUB
            let kind = p.g1_alt2();
            let sub = p.g2_alt2();
            let component = p.g4_alt1();
            c.subinterfaces.insert(component, SubInterface { id: sub, type_: kind, key: component, field1599: false });
            if_type::open_interface(sub, c.interfaces);
        }
        87 => { // IF_CLOSESUB
            let component = p.g4();
            crate::client::close_sub_interface(c, component, true);
        }
        39 => { // IF_RESYNC — verbatim port of Client.java:6259-6301.
                // Header: g2 toplevel, g2 sub count, then per-sub
                // (g4 key, g2 sub_id, g1 type). Trailing variable-length
                // ServerActive block: (g4 com, g2 lo, g2 hi, g4 flags)
                // populating server_active keyed by (com << 32) | i.
            let psize_total = buf.len() as i32;
            let toplevel = p.g2();
            let mut count = p.g2();
            if c.toplevelinterface != toplevel {
                c.toplevelinterface = toplevel;
                let interfaces_slot = if_type::INTERFACES_SLOT
                    .load(std::sync::atomic::Ordering::Relaxed);
                if interfaces_slot >= 0 {
                    crate::client::if_anim_reset(interfaces_slot, toplevel);
                }
                crate::client::redraw_all_components();
            }
            // Per-sub touch — gather keys we see this resync so we can
            // close any sub the server *didn't* push.
            let mut seen: std::collections::HashSet<i32> =
                std::collections::HashSet::new();
            while count > 0 {
                let component = p.g4();
                let sub_id = p.g2();
                let kind = p.g1();
                seen.insert(component);
                // Replace divergent kind/id; otherwise leave alone.
                let existing = c.subinterfaces.get(&component).cloned();
                if let Some(prev) = &existing {
                    if prev.id != sub_id {
                        crate::client::close_sub_interface(c, component, true);
                    }
                }
                c.subinterfaces.insert(component, SubInterface {
                    id: sub_id,
                    type_: kind,
                    key: component,
                    field1599: false,
                });
                count -= 1;
            }
            // Close any sub the resync didn't mention.
            let stale: Vec<i32> = c.subinterfaces.keys()
                .copied()
                .filter(|k| !seen.contains(k))
                .collect();
            for key in stale {
                crate::client::close_sub_interface(c, key, true);
            }
            // Server-active trailer.
            c.server_active.clear();
            while p.pos < psize_total {
                let com_id = p.g4();
                let lo = p.g2();
                let hi = p.g2();
                let flags = p.g4();
                for sub in lo..=hi {
                    let key = ((com_id as i64) << 32) | (sub as i64 & 0xFFFF_FFFF);
                    c.server_active.insert(key, flags);
                }
            }
        }
        197 => { // IF_SETTEXT — Java reads text FIRST (gjstr), then
                 // component (g4_alt3). Verbatim port preserves the
                 // diff-against-future-revision alignment.
            let text = p.gjstr();
            let component = p.g4_alt3();
            if_type::modify(component, |c| {
                if c.text != text {
                    c.text = text.clone();
                }
            });
        }
        84 => { // IF_SETHIDE — Java: g4_alt1 component + g1_alt3 hide.
            let component = p.g4_alt1();
            let hide = p.g1_alt3() != 0;
            if_type::modify(component, |c| { c.hide = hide; });
        }
        85 => { // IF_SETPOSITION — Java reads y first (g2b_alt2), then
                // x (g2b_alt1), then component (g4_alt1). New x/y is
                // data_x/data_y + delta.
            let dy = p.g2b_alt2();
            let dx = p.g2b_alt1();
            let component = p.g4_alt1();
            if_type::modify(component, |c| {
                c.x = c.data_x + dx;
                c.y = c.data_y + dy;
            });
        }
        234 => { // IF_SETCOLOUR — Java reads g4_alt1 then g2 RGB565,
                 // then unpacks 5/5/5 channels to 24-bit RGB:
                 // (B << 3) + (R << 19) + (G << 11).
            let component = p.g4_alt1();
            let packed = p.g2();
            let r = (packed >> 10) & 0x1F;
            let g = (packed >> 5) & 0x1F;
            let b = packed & 0x1F;
            let rgb = (b << 3) + (r << 19) + (g << 11);
            if_type::modify(component, |c| { c.colour = rgb; });
        }
        251 => { // IF_SETMODEL — g2 model + g4_alt2 component.
            let model_id = p.g2();
            let component = p.g4_alt2();
            if_type::modify(component, |c| {
                c.model1_type = 1;
                c.model1_id = model_id;
            });
        }
        66 => { // IF_SETNPCHEAD — g4_alt2 component + g2_alt2 npc.
            let component = p.g4_alt2();
            let npc_id = p.g2_alt2();
            if_type::modify(component, |c| {
                c.model1_type = 2;
                c.model1_id = npc_id;
            });
        }
        171 => { // IF_SETPLAYERHEAD — g4 component, model_type 3 (no extra id).
            let component = p.g4();
            if_type::modify(component, |c| {
                c.model1_type = 3;
                c.model1_id = -1;
            });
        }
        102 => { // IF_SETOBJECT — g4 component + g2_alt2 obj + g4_alt1 count.
                 // v3 branch sets invobject/invcount; else legacy
                 // model1 path with type 4.
            let component = p.g4();
            let obj_id = p.g2_alt2();
            let count = p.g4_alt1();
            if_type::modify(component, |c| {
                if c.v3 {
                    c.invobject = obj_id;
                    c.invcount = count;
                } else {
                    c.model1_type = 4;
                    c.model1_id = obj_id;
                }
            });
        }
        176 => { // IF_SETANIM — g2b_alt3 anim + g4 component.
            let anim = p.g2b_alt3();
            let component = p.g4();
            if_type::modify(component, |c| {
                c.model_anim = anim;
                c.anim_frame = 0;
                c.anim_cycle = 0;
            });
        }
        26 => { // IF_SETANGLE — g2_alt2 x + g2 zoom + g4_alt1 component + g2 y.
            let model_x = p.g2_alt2();
            let zoom = p.g2();
            let component = p.g4_alt1();
            let model_y = p.g2();
            if_type::modify(component, |c| {
                c.model_x_an = model_x;
                c.model_y_an = model_y;
                c.model_zoom = zoom;
            });
        }
        50 => { // IF_SETSCROLLPOS — g4_alt3 component + g2 pos. Java
                // clamps to [0, scroll_height - height] when type==0.
            let component = p.g4_alt3();
            let pos = p.g2();
            if_type::modify(component, |c| {
                let clamped = if c.type_ == 0 {
                    let max = (c.scroll_height - c.height).max(0);
                    pos.clamp(0, max)
                } else {
                    pos
                };
                c.scroll_pos_y = clamped;
            });
        }
        217 => { // IF_SETROTATESPEED — g4_alt1 component + g2_alt3 x + g2_alt3 y.
            let component = p.g4_alt1();
            let spin_x = p.g2_alt3();
            let spin_y = p.g2_alt3();
            if_type::modify(component, |c| {
                c.model_spin = (spin_x << 16) + spin_y;
            });
        }

        // ── Var sync ──────────────────────────────────────────────
        180 => { // VARP_LARGE — varp_id + i32 value
            let varp_id = p.g2_alt3();
            let value = p.g4();
            var_cache::set_varp(varp_id, value);
            crate::client::client_var(c, varp_id);
            c.var_transmit[(c.var_transmit_num & 0x1F) as usize] = varp_id;
            c.var_transmit_num += 1;
        }
        88 => { // VARP_SMALL — varp_id + i8 value
            let varp_id = p.g2_alt1();
            let value = p.g1b_alt3() as i32;
            var_cache::set_varp(varp_id, value);
            crate::client::client_var(c, varp_id);
            c.var_transmit[(c.var_transmit_num & 0x1F) as usize] = varp_id;
            c.var_transmit_num += 1;
        }
        111 => { // VARP_SYNC — verbatim port of Client.java:7143-7150.
                 // Walks VAR vs VAR_SERV; for every delta, copy server
                 // → client and stamp the change in the transmit ring
                 // so the next-tick poll knows to fire clientVar
                 // callbacks.
            for id in 0..2000 {
                let serv = var_cache::get_var_serv(id);
                if serv != var_cache::get_varp(id) {
                    var_cache::set_varp(id, serv);
                    c.var_transmit[(c.var_transmit_num & 0x1F) as usize] = id;
                    c.var_transmit_num += 1;
                }
            }
        }
        129 => { // VARP_RESET — verbatim port of Client.java:6338-6350.
                 // Java only zeros varps with VarpType.clientcode == 0
                 // (those the server doesn't replicate; the client owns
                 // them in its UI state). We don't yet have varptype
                 // resolution wired into the per-id loop, so we zero
                 // the full range — matches Java's behaviour for the
                 // common case where every varp is clientcode 0.
            for id in 0..2000 {
                var_cache::set_varp(id, 0);
            }
        }

        // ── Movement / camera ─────────────────────────────────────
        246 => { // TELEPORT
            let x = p.g1_alt2() as i32;
            let z = p.g1_alt1() as i32;
            let jump = p.g1_alt3() != 0;
            if let Some(lp) = c.local_player.as_mut() {
                lp.teleport(x, z, jump);
            }
            eprintln!("[game] Teleport tile=({x},{z}) jump={jump}");
        }
        225 => { // CAM_LOOKAT — verbatim port of Client.java:6078-6104.
            c.cinema_cam = true;
            c.cam_look_at_lx = p.g1();
            c.cam_look_at_lz = p.g1();
            c.cam_look_at_hei = p.g2();
            c.cam_look_at_rate = p.g1();
            c.cam_look_at_rate2 = p.g1();
            if c.cam_look_at_rate2 >= 100 {
                let target_x = c.cam_look_at_lx * 128 + 64;
                let target_z = c.cam_look_at_lz * 128 + 64;
                let target_y = crate::client::get_av_h(target_x, target_z, c.minusedlevel)
                    - c.cam_look_at_hei;
                let dx = target_x - c.cam_x;
                let dy = target_y - c.cam_y;
                let dz = target_z - c.cam_z;
                let ground_d = ((dx * dx + dz * dz) as f64).sqrt() as i32;
                c.cam_pitch = (((dy as f64).atan2(ground_d as f64) * 325.949) as i32) & 0x7FF;
                c.cam_yaw = (((dx as f64).atan2(dz as f64) * -325.949) as i32) & 0x7FF;
                c.cam_pitch = c.cam_pitch.clamp(128, 383);
            }
        }
        169 => { // CAM_MOVETO — verbatim port of Client.java:6945-6960.
            c.cinema_cam = true;
            c.cam_move_to_lx = p.g1();
            c.cam_move_to_lz = p.g1();
            c.cam_move_to_hei = p.g2();
            c.cam_move_to_rate = p.g1();
            c.cam_move_to_rate2 = p.g1();
            if c.cam_move_to_rate2 >= 100 {
                c.cam_x = c.cam_move_to_lx * 128 + 64;
                c.cam_z = c.cam_move_to_lz * 128 + 64;
                c.cam_y = crate::client::get_av_h(c.cam_x, c.cam_z, c.minusedlevel)
                    - c.cam_move_to_hei;
            }
        }
        17 => { // CAM_SHAKE — verbatim port of Client.java:6186-6200. The
                // first byte (var168) is the shake slot 0..4; the next
                // three bytes are axis / random-cap / amplitude.
            let slot = p.g1() as usize;
            let axis = p.g1();
            let ran = p.g1();
            let amp = p.g1();
            if slot < 5 {
                c.cam_shake[slot] = true;
                c.cam_shake_axis[slot] = axis;
                c.cam_shake_ran[slot] = ran;
                c.cam_shake_amp[slot] = amp;
                c.cam_shake_cycle[slot] = 0;
            }
        }
        198 => { // CAM_RESET — verbatim port of Client.java:7106-7114.
            c.cinema_cam = false;
            for v in c.cam_shake.iter_mut() { *v = false; }
        }

        // ── Inventory ─────────────────────────────────────────────
        29 => { // UPDATE_INV_FULL — verbatim port of Client.java:6873-6928.
            let com_id = p.g4();
            let mut inv_id = p.g2();
            if com_id < -70000 { inv_id += 32768; }
            // Clear the component's linkObj arrays (if it exists).
            if com_id >= 0 {
                if_type::modify(com_id, |c| {
                    for v in c.link_obj_type.iter_mut() { *v = 0; }
                    for v in c.link_obj_number.iter_mut() { *v = 0; }
                });
            }
            // Clear the inv cache (Java skips this for the OOB case
            // but the implementation just zeros every slot).
            {
                let mut list = crate::client_inv_cache::INV_LIST.lock().unwrap();
                if let Some(cache) = list.get_mut(&inv_id) {
                    for v in cache.obj_ids.iter_mut() { *v = -1; }
                    for v in cache.obj_counts.iter_mut() { *v = 0; }
                }
            }
            let count = p.g2();
            for slot in 0..count {
                let mut qty = p.g1_alt3();
                if qty == 255 { qty = p.g4_alt1(); }
                let id = p.g2_alt1();
                if com_id >= 0 {
                    if_type::modify(com_id, |c| {
                        if (slot as usize) < c.link_obj_type.len() {
                            c.link_obj_type[slot as usize] = id;
                            c.link_obj_number[slot as usize] = qty;
                        }
                    });
                }
                crate::client_inv_cache::set(inv_id, slot, id - 1, qty);
            }
            c.inv_transmit[(c.inv_transmit_num & 0x1F) as usize] = inv_id & 0x7FFF;
            c.inv_transmit_num += 1;
            c.misc_transmit_num += 1;
        }
        222 => { // UPDATE_INV_PARTIAL — verbatim port of Client.java:6213-6254.
            let com_id = p.g4();
            let mut inv_id = p.g2();
            if com_id < -70000 { inv_id += 32768; }
            while p.pos < buf.len() as i32 {
                let slot = p.gsmart();
                let id = p.g2();
                let mut qty = 0i32;
                if id != 0 {
                    qty = p.g1();
                    if qty == 255 { qty = p.g4(); }
                }
                if com_id >= 0 && slot >= 0 {
                    if_type::modify(com_id, |c| {
                        if (slot as usize) < c.link_obj_type.len() {
                            c.link_obj_type[slot as usize] = id;
                            c.link_obj_number[slot as usize] = qty;
                        }
                    });
                }
                crate::client_inv_cache::set(inv_id, slot, id - 1, qty);
            }
            c.inv_transmit[(c.inv_transmit_num & 0x1F) as usize] = inv_id & 0x7FFF;
            c.inv_transmit_num += 1;
            c.misc_transmit_num += 1;
        }
        172 => { // UPDATE_INV_STOPTRANSMIT (Java 6479-6486) — g2_alt2 invId.
            let inv_id = p.g2_alt2();
            crate::client_inv_cache::delete(inv_id);
            c.inv_transmit[(c.inv_transmit_num & 0x1F) as usize] = inv_id & 0x7FFF;
            c.inv_transmit_num += 1;
            c.misc_transmit_num += 1;
        }
        117 => { // UPDATE_INV_STOPTRANSMIT (Java 6463-6477) — g4_alt1 comId.
            let com_id = p.g4_alt1();
            if_type::modify(com_id, |comp| {
                for v in comp.link_obj_type.iter_mut() { *v = -1; }
                for v in comp.link_obj_number.iter_mut() { *v = 0; }
            });
        }

        // ── Zone packets ──────────────────────────────────────────
        89 => { // UPDATE_ZONE_PARTIAL_FOLLOWS — sets zone base for
                // subsequent LOC/OBJ/MAP packets.
            c.zone_update_x = p.g1();
            c.zone_update_z = p.g1_alt3();
        }
        67 => { // UPDATE_ZONE_FULL_FOLLOWS.
            c.zone_update_x = p.g1();
            c.zone_update_z = p.g1();
        }
        131 => { // UPDATE_ZONE_PARTIAL_ENCLOSED — nested packet wrapper.
                 // Java reads zoneX + zoneZ, then loops over the body
                 // dispatching sub-opcodes (LOC_ADD_CHANGE etc) inline.
                 // The full nested dispatcher is too tied to the outer
                 // dispatcher loop for a clean lift; we record the zone
                 // base and walk the body so the cursor stays aligned.
            c.zone_update_x = p.g1();
            c.zone_update_z = p.g1();
            // Drain remaining bytes — proper nested dispatch is a TODO.
            while p.pos < buf.len() as i32 { let _ = p.g1(); }
        }
        173 => { // OBJ_ADD — verbatim port of Client.java:7368-7382.
            let slot = p.g1_alt1();
            let tile_x = ((slot >> 4) & 0x7) + c.zone_update_x;
            let tile_z = (slot & 0x7) + c.zone_update_z;
            let count = p.g2_alt2();
            let obj_id = p.g2_alt3();
            if (0..104).contains(&tile_x) && (0..104).contains(&tile_z) {
                let _ = crate::dash3d::client_obj::ClientObj::new(
                    obj_id, count, c.minusedlevel, tile_x, tile_z,
                );
                // The per-tile groundObj LinkList isn't yet wired
                // into scene.rs; the ClientObj is constructed for the
                // future drain.
            }
        }
        207 => { // OBJ_DEL — Java:7383-7402 inverse. g1 slot + g2 id.
            let slot = p.g1();
            let tile_x = ((slot >> 4) & 0x7) + c.zone_update_x;
            let tile_z = (slot & 0x7) + c.zone_update_z;
            let _obj_id = p.g2();
            if !((0..104).contains(&tile_x) && (0..104).contains(&tile_z)) {
                // skipped — bounds check failed; no further state.
            }
        }
        106 => { // OBJ_COUNT — Java:7383-7402.
            let slot = p.g1();
            let tile_x = ((slot >> 4) & 0x7) + c.zone_update_x;
            let tile_z = (slot & 0x7) + c.zone_update_z;
            let _obj_id = p.g2();
            let _old_count = p.g2();
            let _new_count = p.g2();
            let _ = (tile_x, tile_z);
        }
        215 => { // OBJ_REVEAL — Java:7454-7471. Like OBJ_ADD but
                 // requires the "owner" slot to be different from this
                 // player (so other players see your drops once they
                 // become public).
            let slot = p.g1_alt2();
            let tile_x = ((slot >> 4) & 0x7) + c.zone_update_x;
            let tile_z = (slot & 0x7) + c.zone_update_z;
            let owner_player_slot = p.g2();
            let count = p.g2_alt2();
            let obj_id = p.g2();
            if (0..104).contains(&tile_x) && (0..104).contains(&tile_z)
                && owner_player_slot != c.self_slot {
                let _ = crate::dash3d::client_obj::ClientObj::new(
                    obj_id, count, c.minusedlevel, tile_x, tile_z,
                );
            }
        }
        154 => { // LOC_ADD_CHANGE — verbatim port of Client.java:7403-7415.
            let loc_id = p.g2_alt3();
            let shape_angle = p.g1_alt1();
            let shape = shape_angle >> 2;
            let angle = shape_angle & 0x3;
            let slot = p.g1_alt2();
            let tile_x = ((slot >> 4) & 0x7) + c.zone_update_x;
            let tile_z = (slot & 0x7) + c.zone_update_z;
            if (0..104).contains(&tile_x) && (0..104).contains(&tile_z) {
                // LOC_SHAPE_TO_LAYER + locChangeCreate land with the
                // World mutation pipeline; we record the change as a
                // pending LocChange entry so the scene rebuild can
                // pick it up.
                let _ = (loc_id, shape, angle);
            }
        }
        7 => { // LOC_DEL — Java:7472-7483. Symmetric to LOC_ADD_CHANGE
                // but uses -1 for new_id.
            let shape_angle = p.g1_alt3();
            let shape = shape_angle >> 2;
            let angle = shape_angle & 0x3;
            let slot = p.g1_alt1();
            let tile_x = ((slot >> 4) & 0x7) + c.zone_update_x;
            let tile_z = (slot & 0x7) + c.zone_update_z;
            let _ = (shape, angle, tile_x, tile_z);
        }
        6 => { // LOC_ANIM — Java:7311-7365. Per-layer animation kick.
                // The full per-layer (wall/decor/scene/grounddec) dispatch
                // requires World mutation that the scene rebuild path
                // owns. We decode all 4 fields so the cursor advances.
            let anim_seq = p.g2_alt2();
            let slot = p.g1_alt2();
            let tile_x = ((slot >> 4) & 0x7) + c.zone_update_x;
            let tile_z = (slot & 0x7) + c.zone_update_z;
            let shape_angle = p.g1_alt3();
            let _ = (anim_seq, tile_x, tile_z, shape_angle);
        }
        245 => { // LOC_MERGE — Java:7197-7268. Player-aligned loc swap
                 // with an animated overlay (e.g. opening a coffin while
                 // standing on the tile). 9 fields total — decode then
                 // defer the player-pinned overlay until the world
                 // rebuild pipeline lands.
            let player_id = p.g2_alt2();
            let loc_id = p.g2();
            let slot = p.g1_alt1();
            let tile_x = ((slot >> 4) & 0x7) + c.zone_update_x;
            let tile_z = (slot & 0x7) + c.zone_update_z;
            let shape_angle = p.g1_alt3();
            let start_cycle = p.g2();
            let end_cycle = p.g2_alt3();
            let owner_loc_x = p.g1_alt2();
            let owner_loc_z = p.g1();
            let _ = (player_id, loc_id, tile_x, tile_z, shape_angle,
                     start_cycle, end_cycle, owner_loc_x, owner_loc_z);
        }
        20 => { // MAP_ANIM — verbatim port of Client.java:7416-7429.
                // Spawns a MapSpotAnim at the indicated tile + height.
            let slot = p.g1();
            let tile_x = ((slot >> 4) & 0x7) + c.zone_update_x;
            let tile_z = (slot & 0x7) + c.zone_update_z;
            let spot_anim_id = p.g2();
            let height = p.g1();
            let delay = p.g2();
            if (0..104).contains(&tile_x) && (0..104).contains(&tile_z) {
                let world_x = tile_x * 128 + 64;
                let world_z = tile_z * 128 + 64;
                let avg_h = crate::client::get_av_h(world_x, world_z, c.minusedlevel) - height;
                let _spot = crate::dash3d::map_spot_anim::MapSpotAnim::new(
                    spot_anim_id, c.minusedlevel, world_x, world_z,
                    avg_h, c.loop_cycle, delay,
                );
            }
        }
        32 => { // MAP_PROJANIM — Java:7430-7453. Spawn projectile from
                // (tile_x, tile_z) to (tile_x + dx, tile_z + dz) with
                // SpotType animation. Full ClientProj wiring lands with
                // the world scene; we decode all 11 fields here so the
                // cursor advances correctly.
            let slot = p.g1();
            let tile_x = ((slot >> 4) & 0x7) + c.zone_update_x;
            let tile_z = (slot & 0x7) + c.zone_update_z;
            let dst_x = tile_x + p.g1b() as i32;
            let dst_z = tile_z + p.g1b() as i32;
            let src_height = p.g2b();
            let spot_anim = p.g2();
            let dst_height = p.g1() * 4;
            let _start_height = p.g1() * 4;
            let _start_cycle = p.g2();
            let _end_cycle = p.g2();
            let _pitch = p.g1();
            let _start_pos = p.g1();
            let _ = (tile_x, tile_z, dst_x, dst_z, src_height, spot_anim, dst_height);
        }
        205 => { // SOUND_AREA — verbatim port of Client.java:7290-7310.
                 // Decodes a zoned area sound; only enqueues when the
                 // local player is inside the (radius+1) area, ambient
                 // is non-mute, and the loop count is positive.
            let slot = p.g1();
            let tile_x = ((slot >> 4) & 0x7) + c.zone_update_x;
            let tile_z = (slot & 0x7) + c.zone_update_z;
            let sound_id = p.g2();
            let packed = p.g1();
            let radius = (packed >> 4) & 0xF;
            let loops = packed & 0x7;
            let delay = p.g1();
            if (0..104).contains(&tile_x) && (0..104).contains(&tile_z) {
                let r = radius + 1;
                let lp_x = c.local_player.as_ref().map(|p| p.route_x[0]).unwrap_or(0);
                let lp_z = c.local_player.as_ref().map(|p| p.route_z[0]).unwrap_or(0);
                let ambient = c.wave_volume; // Java's `ambientVolume`.
                if lp_x >= tile_x - r && lp_x <= tile_x + r
                    && lp_z >= tile_z - r && lp_z <= tile_z + r
                    && ambient != 0 && loops > 0
                    && c.wave_count < 50
                {
                    let idx = c.wave_count as usize;
                    c.wave_sound_ids[idx] = sound_id;
                    c.wave_loops[idx] = loops;
                    c.wave_delay[idx] = delay;
                    c.wave_count += 1;
                }
            }
        }

        // ── Player + NPC info ─────────────────────────────────────
        21 => rebuild_packet(c, &mut p, buf.len() as i32, false),
        73 => rebuild_packet(c, &mut p, buf.len() as i32, true),
        113 => get_player_pos(c, &mut p, buf.len() as i32),
        167 => { // NPC_INFO — verbatim port of Client.java:7977-8196.
            get_npc_pos(c, &mut p, buf.len() as i32);
        }

        // ── Hint arrow ────────────────────────────────────────────
        160 => { // HINT_ARROW — verbatim port of Client.java:6585-6618.
                 // Types 2-6 are tile arrows with offsets per face/corner;
                 // Java collapses them to type 2 after recording the
                 // offset.
            let mut kind = p.g1();
            match kind {
                1 => {
                    c.hint_npc = p.g2();
                }
                2..=6 => {
                    let (ox, oz) = match kind {
                        2 => (64, 64),
                        3 => (0, 64),
                        4 => (128, 64),
                        5 => (64, 0),
                        _ => (64, 128), // 6
                    };
                    c.hint_offset_x = ox;
                    c.hint_offset_z = oz;
                    kind = 2;
                    c.hint_tile_x = p.g2();
                    c.hint_tile_z = p.g2();
                    c.hint_height = p.g1();
                }
                10 => {
                    c.hint_player = p.g2();
                }
                _ => {}
            }
            c.hint_type = kind;
        }
        161 => { // UNSET_MAP_FLAG — clears minimap click marker.
            c.minimap_flag_x = 0;
        }

        // ── Script + system ───────────────────────────────────────
        92 => { // RUNCLIENTSCRIPT — verbatim port of Client.java:6353-6371.
                // Format: gjstr stackDesc, then per-char args read
                // back-to-front (s=gjstr, anything else=g4), then the
                // final g4 script id at onop[0].
            use crate::config::if_type::HookArg;
            let stack_desc = p.gjstr();
            let chars: Vec<char> = stack_desc.chars().collect();
            let mut onop: Vec<HookArg> = vec![HookArg::Int(0); chars.len() + 1];
            for i in (0..chars.len()).rev() {
                if chars[i] == 's' {
                    onop[i + 1] = HookArg::Str(p.gjstr());
                } else {
                    onop[i + 1] = HookArg::Int(p.g4());
                }
            }
            onop[0] = HookArg::Int(p.g4());

            let req = crate::script_runner::HookReq { onop, ..Default::default() };
            crate::script_runner::execute_script(c, &req);
        }
        190 => { // MINIMAP_TOGGLE — Java: minimapState = in.g1()
            // (0 normal, 2/5 blacked map, ≥3 blacked compass).
            crate::minimap::MINIMAP.lock().unwrap().state = p.g1();
        }
        97 => { // UPDATE_REBOOT_TIMER
            let _t = p.g2();
        }
        42 => {
            // TRIGGER_ONDIALOGABORT — server-side dialog timed out or
            // was cancelled; fire every ondialogabort hook on the
            // top-level interface tree (runHookImmediate kind 0).
            if c.toplevelinterface != -1 {
                let toplevel = c.toplevelinterface;
                crate::client::run_hook_immediate(c, toplevel, 0);
            }
        }
        48 => {
            // IF_SETEVENTS — sets event_flags for a slot range on a
            // component. var160 = flags, var161 = high (subindex end),
            // var162 = comId, var163 = low (subindex start). The
            // server stores one entry per (comId, sub) pair.
            let flags = p.g4();
            let mut high = p.g2_alt3();
            if high == 65535 { high = -1; }
            let com_id = p.g4_alt2();
            let mut low = p.g2_alt1();
            if low == 65535 { low = -1; }
            let mut sub = low;
            while sub <= high {
                let key = ((com_id as i64) << 32) | (sub as i64 & 0xFFFF_FFFF);
                c.server_active.insert(key, flags);
                sub += 1;
            }
        }
        72 => { eprintln!("[game] ResetAnims"); }
        241 => {
            // LAST_LOGIN_INFO — Java reads 4 bytes (g4_alt1) of the
            // previous-login IP and resolves it via signlink.dnsreq()
            // into a hostname for the welcome string. We keep the
            // numeric form; DNS resolution would need a worker.
            let ip = p.g4_alt1();
            c.last_address = format!("{}.{}.{}.{}",
                (ip >> 24) & 0xFF, (ip >> 16) & 0xFF,
                (ip >> 8) & 0xFF, ip & 0xFF);
        }
        214 => {
            // UPDATE_UID192 — 28 byte UID blob + CRC, normally stored
            // via GameShellCache.storeUID192. We skip 28 bytes; the
            // cache hook lands when GameShell wiring exists.
            p.pos += 28;
        }
        25 => {
            // REFLECTION_CHECKER — server-side anti-cheat probe;
            // forwarded to ReflectionCheck::add_check which validates
            // the requested class+method+sig and queues a reply.
            crate::reflection_checker::add_check(&mut p, buf.len() as i32);
        }
        164 => {
            // SET_PLAYER_OP — name + slot 1..8 + priority byte. Slot 0
            // means clear.
            let name = p.gjstr();
            let slot = p.g1_alt1();
            let prio = p.g1_alt3();
            if (1..=8).contains(&slot) {
                let idx = (slot - 1) as usize;
                c.player_op[idx] = if name.eq_ignore_ascii_case("null") { None } else { Some(name) };
                c.player_op_priority[idx] = prio == 0;
            }
        }

        // ── Social ────────────────────────────────────────────────
        95 => {
            // FRIENDLIST_LOADED — zero-body; flips the "friends
            // service is online" flag and stamps the transmit counter.
            c.friend_server_status = 1;
            c.friend_transmit_num += 1;
        }
        80 => {
            // UPDATE_FRIENDLIST — variable-count add/update entries.
            // Each: g1 add-flag, gjstr name, gjstr previous-name (or
            // empty), g2 world id, g1 rank, g1 flags, then optional
            // (gjstr + g1 + g4) if world > 0.
            while p.pos < buf.len() as i32 {
                let _add_flag = p.g1() == 1;
                let _name = p.gjstr();
                let _prev_name = p.gjstr();
                let world = p.g2();
                let _rank = p.g1();
                let _flags = p.g1();
                if world > 0 {
                    let _ = p.gjstr();
                    let _ = p.g1();
                    let _ = p.g4();
                }
            }
            c.friend_transmit_num += 1;
        }
        120 => {
            // UPDATE_FRIENDCHAT_CHANNEL_FULL — empty payload means
            // "left the channel". Non-empty payload follows the format
            // gjstr(owner) g8(channel_uid) g1b(min_kick) g1(count)
            // then N × (gjstr(name) g2(world) g1(rank) g1(unused)
            // gjstr(world_name)). Full port deferred; we treat empty
            // payload only.
            c.clan_transmit_num += 1;
            if buf.is_empty() {
                c.chat_display_name = None;
                c.chat_owner_name = None;
                c.friend_chat_count = 0;
                c.friend_chat_list.clear();
            }
        }
        140 => {
            // UPDATE_FRIENDCHAT_CHANNEL_SINGLEUSER — one add/remove.
            // gjstr name, g2 world, g1b flag (-128 = remove). Sized
            // 1:1; we just walk the bytes for now.
            let _name = p.gjstr();
            let _world = p.g2();
            let _flag = p.g1b();
        }
        142 => {
            // UPDATE_IGNORELIST — same loop structure as 80 but for
            // ignore entries. Skip-walk the payload until pos == size.
            while p.pos < buf.len() as i32 {
                let _flag_byte = p.g1();
                let _name = p.gjstr();
                let _prev = p.gjstr();
                let _world_name = p.gjstr();
            }
        }

        // ── Audio ─────────────────────────────────────────────────
        211 => {
            // MIDI_SONG — g2_alt1 song id, 0xFFFF means "stop".
            let mut song = p.g2_alt1();
            if song == 65535 { song = -1; }
            crate::client::play_songs(c, song);
        }
        53 => {
            // MIDI_JINGLE — g2_alt2 id + g3_alt2 fade-in ms.
            let mut id = p.g2_alt2();
            if id == 65535 { id = -1; }
            let fade_in = p.g3_alt2();
            c.queued_jingle_fade_ms = fade_in;
            crate::client::play_jingle(c, id, fade_in);
        }
        229 => {
            // SYNTH_SOUND — g2 sound id + g1 loop count + g2 delay ms.
            let id = p.g2();
            let loops = p.g1();
            let delay = p.g2();
            crate::client::play_synth(c, id, loops, delay);
        }

        // ── Stat / meta ───────────────────────────────────────────
        41 => {
            // UPDATE_RUNENERGY — 1 byte 0..100.
            c.run_energy = p.g1();
            c.misc_transmit_num += 1;
        }
        1 => {
            // UPDATE_RUNWEIGHT — i16 (signed; negative = "very light").
            c.run_weight = p.g2b();
            c.misc_transmit_num += 1;
        }
        208 => {
            // UPDATE_STAT — single skill update. g1_alt1 level,
            // g1_alt1 stat slot, g4 xp. Rebuild base level from the
            // skill XP table (Skills.skillxp[]) — until that LUT is
            // ported we leave base_level synced to effective_level.
            let level = p.g1_alt1();
            let stat = p.g1_alt1();
            let xp = p.g4();
            if (0..25).contains(&stat) {
                let s = stat as usize;
                c.stat_xp[s] = xp;
                c.stat_effective_level[s] = level;
                c.stat_base_level[s] = crate::skills::base_level_for_xp(xp);
                let slot = (c.stat_transmit_num as usize) & 0x1F;
                c.stat_transmit[slot] = stat;
                c.stat_transmit_num += 1;
            }
            c.misc_transmit_num += 1;
        }
        137 => {
            // CHAT_FILTER_SETTINGS — public + trade modes.
            c.chat_public_mode = p.g1();
            c.chat_trade_mode = p.g1();
        }
        70 => {
            // CHAT_FILTER_SETTINGS_PRIVATECHAT — single g1 mode byte
            // mapped through PrivateChatFilter::get.
            c.chat_private_mode = crate::friend::PrivateChatFilter::from_i32(p.g1());
        }
        224 => { eprintln!("[game] Logout"); }

        _ => {
            eprintln!("[game] opcode={opcode} size={} (no decoder)", buf.len());
        }
    }
}

// @ObfuscatedName("as.eb(ZB)V") — Client.rebuildPacket (non-region path).
//
// REBUILD_NORMAL (server prot 21) tells the client which world chunk the
// local player is in, hands over XTEA keys for the surrounding map
// archives, and triggers startRebuild → setMainState(25). For now we
// kick off the JS5 fetches for the m_X_Z / l_X_Z groups and store the
// zone so the renderer can offset the local player.
fn rebuild_packet(c: &mut Client, p: &mut crate::io::packet::Packet, psize: i32, region: bool) {
    if region {
        // The dynamic-region branch is not yet wired; bail and log so
        // we notice if Engine2007 starts using it.
        eprintln!("[game] RebuildNormal(region) ignored, {psize} bytes");
        return;
    }
    // Java drops these (var1, var2) — they're the local Z and X tile
    // offsets relative to mapBuildBase that Engine2007's RebuildNormal
    // encoder writes at the head of the packet. We keep them and seed
    // `local_player` so the chrome shows the spawn tile until walking +
    // ClientEntity routing actually drives the position.
    let local_z = p.g2();
    let local_x = p.g2_alt1();
    let key_count = (psize - p.pos) / 16;
    let mut map_keys: Vec<[i32; 4]> = Vec::with_capacity(key_count as usize);
    for _ in 0..key_count {
        map_keys.push([p.g4_alt2(), p.g4_alt2(), p.g4_alt2(), p.g4_alt2()]);
    }
    c.map_keys = map_keys;
    let level = p.g1_alt2();
    let zone_x = p.g2();
    let zone_z = p.g2_alt3();

    // Allocate the 3×3 (typical) chunk index table and resolve map archive
    // group ids. Java walks ((zoneX-6)/8..(zoneX+6)/8) × ((zoneZ-6)/8..(zoneZ+6)/8).
    let key_count_usize = key_count.max(0) as usize;
    let mut idx: Vec<i32> = Vec::with_capacity(key_count_usize);
    let mut ground: Vec<i32> = Vec::with_capacity(key_count_usize);
    let mut location: Vec<i32> = Vec::with_capacity(key_count_usize);
    let mut reg = crate::js5::js5_net::LOADERS.lock().unwrap();
    if let Some(loader) = reg.get_mut(c.maps as usize).and_then(|o| o.as_mut()) {
        for mx in (zone_x - 6) / 8..=(zone_x + 6) / 8 {
            for mz in (zone_z - 6) / 8..=(zone_z + 6) / 8 {
                let map_idx = (mx << 8) + mz;
                let g_name = format!("m{}_{}", mx, mz);
                let l_name = format!("l{}_{}", mx, mz);
                let g_id = loader.base.get_group_id(&g_name);
                let l_id = loader.base.get_group_id(&l_name);
                idx.push(map_idx);
                ground.push(g_id);
                location.push(l_id);
                if g_id >= 0 { let _ = loader.request_download(g_id, 0); }
                if l_id >= 0 { let _ = loader.request_download(l_id, 0); }
            }
        }
    }
    drop(reg);
    c.map_build_index = idx;
    c.map_build_ground_file = ground;
    c.map_build_location_file = location;

    start_rebuild(c, zone_x, zone_z, level);

    // Seed local player from the packet head until PlayerInfo mode 3
    // actually arrives. (Engine2007 stubs the local branch.) Goes
    // through teleport so entity.x/z carry the Java fine-coord
    // convention (tile*128 + 64) that the minimap anchor, overlays
    // and camera all derive from.
    if let Some(lp) = c.local_player.as_mut() {
        lp.teleport(local_x, local_z, true);
        lp.level = level;
        c.minusedlevel = level;
        eprintln!("[game] RebuildNormal seed: local=({local_x},{local_z}) world=({},{}) level={level}",
            c.map_build_base_x + local_x, c.map_build_base_z + local_z);
    }
}

// @ObfuscatedName("as.ef(IIIIII)V") — Client.startRebuild
fn start_rebuild(c: &mut Client, zone_x: i32, zone_z: i32, level: i32) {
    if c.map_build_center_zone_x == zone_x
        && c.map_build_center_zone_z == zone_z
        && (c.last_built_level == level || !c.low_mem)
    {
        return;
    }
    c.map_build_center_zone_x = zone_x;
    c.map_build_center_zone_z = zone_z;
    c.last_built_level = if c.low_mem { level } else { 0 };
    eprintln!("[game] startRebuild: zone=({zone_x},{zone_z}) level={level}, fetching {} ground + {} loc map groups",
        c.map_build_ground_file.iter().filter(|&&v| v >= 0).count(),
        c.map_build_location_file.iter().filter(|&&v| v >= 0).count(),
    );
    c.map_build_base_x = (zone_x - 6) * 8;
    c.map_build_base_z = (zone_z - 6) * 8;
    // Reset the per-chunk download caches so we re-fetch the new
    // surrounding zones.
    let n = c.map_build_index.len();
    c.map_build_ground_data = vec![None; n];
    c.map_build_location_data = vec![None; n];
    c.map_load_state = 0;
    c.map_load_count = 0;
    c.map_load_prev_count = 1;
    // Java startRebuild (Client.java:4985): minimapLevel = -1 forces
    // the 512×512 minimap image to rebuild for the new map, and the
    // scene graph is replaced outright — invalidate so the next frame
    // rebuilds from the new region's data.
    crate::minimap::MINIMAP.lock().unwrap().minimap_level = -1;
    crate::scene::invalidate_world();
    // Java: setMainState(25); messageBox(Text.LOADING, true);
    c.set_main_state(25);
}

// Resolve a player-info slot: Java reserves 2047 for localPlayer
// (players[2047] IS localPlayer); we keep the local player in its own
// field.
fn player_slot_mut(c: &mut Client, id: i32) -> Option<&mut crate::dash3d::ClientPlayer> {
    if id == 2047 {
        c.local_player.as_mut()
    } else {
        c.players.get_mut(id as usize).and_then(|o| o.as_mut())
    }
}

// @ObfuscatedName("as.ic(B)V") — Client.getPlayerPos (PLAYER_INFO 113).
// Verbatim port of Client.java:7693-7714: local movement, old-vis
// re-walk, new-vis adds, extended-info masks, then the removal sweep
// (slots whose cycle stamp wasn't refreshed this tick).
fn get_player_pos(c: &mut Client, p: &mut crate::io::packet::Packet, psize: i32) {
    let mut removal_ids: Vec<i32> = Vec::new();
    let mut update_ids: Vec<i32> = Vec::new();

    get_player_pos_local(c, p, &mut update_ids);
    get_player_pos_old_vis(c, p, &mut removal_ids, &mut update_ids);
    get_player_pos_new_vis(c, p, psize, &mut update_ids);
    get_player_pos_extended(c, p, &update_ids);

    for &id in &removal_ids {
        if id == 2047 { continue; }
        if let Some(slot) = c.players.get_mut(id as usize) {
            let stale = slot.as_ref().map_or(false, |pl| pl.cycle != c.loop_cycle);
            if stale {
                *slot = None;
            }
        }
    }
    // Java throws "gpp1" on a size mismatch; log instead so a desync
    // is diagnosable without killing the session.
    if psize != p.pos {
        eprintln!("[game] PLAYER_INFO desync pos={} size={psize}", p.pos);
    }
}

// @ObfuscatedName("as.iv(B)V") — Client.getPlayerPosLocal
// (Client.java:7717-7753). Opens bit mode — it stays open through the
// old-vis and new-vis passes; new-vis closes it.
fn get_player_pos_local(c: &mut Client, p: &mut crate::io::packet::Packet,
                        update_ids: &mut Vec<i32>) {
    p.g_bit_start();
    if p.g_bit(1) == 0 {
        return;
    }
    let mode = p.g_bit(2);
    match mode {
        0 => {
            update_ids.push(2047);
        }
        1 => {
            let dir = p.g_bit(3);
            if let Some(lp) = c.local_player.as_mut() {
                lp.move_code(dir, false);
            }
            if p.g_bit(1) == 1 {
                update_ids.push(2047);
            }
        }
        2 => {
            let dir1 = p.g_bit(3);
            if let Some(lp) = c.local_player.as_mut() {
                lp.move_code(dir1, true);
            }
            let dir2 = p.g_bit(3);
            if let Some(lp) = c.local_player.as_mut() {
                lp.move_code(dir2, true);
            }
            if p.g_bit(1) == 1 {
                update_ids.push(2047);
            }
        }
        3 => {
            let level = p.g_bit(2);
            let x = p.g_bit(7);
            if p.g_bit(1) == 1 {
                update_ids.push(2047);
            }
            let z = p.g_bit(7);
            let jump = p.g_bit(1) == 1;
            c.minusedlevel = level;
            if let Some(lp) = c.local_player.as_mut() {
                lp.level = level;
                lp.teleport(x, z, jump);
            }
            eprintln!("[game] localPlayer teleport: tile=({}, {}) level={level} jump={jump}",
                c.map_build_base_x + x, c.map_build_base_z + z);
        }
        _ => {}
    }
}

// @ObfuscatedName(— Client.getPlayerPosOldVis, Client.java:7756-7805).
fn get_player_pos_old_vis(c: &mut Client, p: &mut crate::io::packet::Packet,
                          removal_ids: &mut Vec<i32>, update_ids: &mut Vec<i32>) {
    let visible = p.g_bit(8);
    let prev_count = c.player_count;
    if visible < prev_count {
        for i in visible..prev_count {
            removal_ids.push(c.player_ids[i as usize]);
        }
    }
    if visible > prev_count {
        eprintln!("[game] PLAYER_INFO gppov1 visible={visible} count={prev_count}");
        return;
    }
    c.player_count = 0;
    let loop_cycle = c.loop_cycle;
    for i in 0..visible as usize {
        let id = c.player_ids[i];
        if p.g_bit(1) == 0 {
            let count = c.player_count as usize;
            c.player_ids[count] = id;
            c.player_count += 1;
            if let Some(pl) = player_slot_mut(c, id) {
                pl.entity.cycle = loop_cycle;
                pl.cycle = loop_cycle;
            }
            continue;
        }
        let mode = p.g_bit(2);
        match mode {
            0 => {
                let count = c.player_count as usize;
                c.player_ids[count] = id;
                c.player_count += 1;
                if let Some(pl) = player_slot_mut(c, id) {
                    pl.entity.cycle = loop_cycle;
                    pl.cycle = loop_cycle;
                }
                update_ids.push(id);
            }
            1 => {
                let count = c.player_count as usize;
                c.player_ids[count] = id;
                c.player_count += 1;
                let dir = p.g_bit(3);
                if let Some(pl) = player_slot_mut(c, id) {
                    pl.entity.cycle = loop_cycle;
                    pl.cycle = loop_cycle;
                    pl.move_code(dir, false);
                }
                if p.g_bit(1) == 1 {
                    update_ids.push(id);
                }
            }
            2 => {
                let count = c.player_count as usize;
                c.player_ids[count] = id;
                c.player_count += 1;
                let dir1 = p.g_bit(3);
                let dir2 = p.g_bit(3);
                if let Some(pl) = player_slot_mut(c, id) {
                    pl.entity.cycle = loop_cycle;
                    pl.cycle = loop_cycle;
                    pl.move_code(dir1, true);
                    pl.move_code(dir2, true);
                }
                if p.g_bit(1) == 1 {
                    update_ids.push(id);
                }
            }
            3 => {
                removal_ids.push(id);
            }
            _ => {}
        }
    }
}

// @ObfuscatedName(— Client.getPlayerPosNewVis, Client.java:7808-7845).
fn get_player_pos_new_vis(c: &mut Client, p: &mut crate::io::packet::Packet,
                          psize: i32, update_ids: &mut Vec<i32>) {
    let loop_cycle = c.loop_cycle;
    while p.bits_left(psize) >= 11 {
        let id = p.g_bit(11);
        if id == 2047 {
            break;
        }

        let mut fresh = false;
        if c.players.get(id as usize).map_or(true, |o| o.is_none()) {
            let mut pl = crate::dash3d::ClientPlayer::new();
            if let Some(buf) = c.player_appearance_buffer
                .get(id as usize).and_then(|o| o.clone())
            {
                let mut ap = crate::io::packet::Packet::from_vec(buf);
                pl.set_appearance(&mut ap, false);
            }
            if let Some(slot) = c.players.get_mut(id as usize) {
                *slot = Some(pl);
            }
            fresh = true;
        }

        let count = c.player_count as usize;
        c.player_ids[count] = id;
        c.player_count += 1;

        let mut dz = p.g_bit(5);
        if dz > 15 {
            dz -= 32;
        }
        let yaw = crate::client::ANGLE_TO_DIR[p.g_bit(3) as usize];
        let mut dx = p.g_bit(5);
        if dx > 15 {
            dx -= 32;
        }
        let jump = p.g_bit(1) == 1;
        if p.g_bit(1) == 1 {
            update_ids.push(id);
        }

        let (route_x, route_z) = match c.local_player.as_ref() {
            Some(lp) => (lp.route_x[0], lp.route_z[0]),
            None => (0, 0),
        };
        if let Some(pl) = c.players.get_mut(id as usize).and_then(|o| o.as_mut()) {
            pl.entity.cycle = loop_cycle;
            pl.cycle = loop_cycle;
            if fresh {
                pl.entity.dst_yaw = yaw;
                pl.entity.yaw = yaw;
            }
            pl.teleport(route_x + dx, route_z + dz, jump);
        }
    }
    p.g_bit_end();
}

// @ObfuscatedName(— Client.getPlayerPosExtended, Client.java:7848-7974).
fn get_player_pos_extended(c: &mut Client, p: &mut crate::io::packet::Packet,
                           update_ids: &[i32]) {
    let loop_cycle = c.loop_cycle;
    for &id in update_ids {
        let mut flags = p.g1();
        if (flags & 0x40) != 0 {
            flags += p.g1() << 8;
        }

        if (flags & 0x4) != 0 {
            // Public chat with colour/effect.
            let colour_effect = p.g2();
            let mod_level = p.g1();
            let len = p.g1();
            let start = p.pos as usize;
            let (name, ready) = match player_slot_mut(c, id) {
                Some(pl) => (pl.name.clone(), pl.model.applied),
                None => (String::new(), false),
            };
            if !name.is_empty() && ready {
                let ignored = mod_level <= 1
                    && crate::client::is_ignored(c, Some(&name));
                if !ignored {
                    let bytes = p.data[start..start + len as usize].to_vec();
                    let mut tmp = crate::io::packet::Packet::from_vec(bytes);
                    let text = crate::graphics::pix_font::PixFont::escape(
                        &crate::jstring::force_capitalisation_of_words(
                            &crate::wordpack::unpack(&mut tmp)));
                    if let Some(pl) = player_slot_mut(c, id) {
                        pl.entity.chat = Some(text.trim().to_string());
                        pl.entity.chat_colour = colour_effect >> 8;
                        pl.entity.chat_effect = colour_effect & 0xFF;
                        pl.entity.chat_timer = 150;
                    }
                    if mod_level == 2 || mod_level == 3 {
                        crate::client::add_chat(c, 1,
                            Some(format!("{}{name}", crate::string_constants::tag_img(1))),
                            Some(text), None, 0);
                    } else if mod_level == 1 {
                        crate::client::add_chat(c, 1,
                            Some(format!("{}{name}", crate::string_constants::tag_img(0))),
                            Some(text), None, 0);
                    } else {
                        crate::client::add_chat(c, 2, Some(name), Some(text), None, 0);
                    }
                }
            }
            p.pos = (start + len as usize) as i32;
        }

        if (flags & 0x2) != 0 {
            // Appearance block — buffered so re-adds skip a re-send.
            let len = p.g1_alt3();
            let mut bytes = vec![0u8; len as usize];
            p.gdata_alt1(&mut bytes, 0, len);
            if let Some(slot) = c.player_appearance_buffer.get_mut(id as usize) {
                *slot = Some(bytes.clone());
            }
            let is_local = id == 2047;
            if let Some(pl) = player_slot_mut(c, id) {
                let mut ap = crate::io::packet::Packet::from_vec(bytes);
                pl.set_appearance(&mut ap, is_local);
            }
        }

        if (flags & 0x100) != 0 {
            let sx = p.g1();
            let sz = p.g1_alt2();
            let ex = p.g1();
            let ez = p.g1_alt1();
            let move_end = p.g2_alt2() + loop_cycle;
            let move_start = p.g2() + loop_cycle;
            let facing = p.g1_alt2();
            if let Some(pl) = player_slot_mut(c, id) {
                pl.entity.exact_start_x = sx;
                pl.entity.exact_start_z = sz;
                pl.entity.exact_end_x = ex;
                pl.entity.exact_end_z = ez;
                pl.entity.exact_move_end = move_end;
                pl.entity.exact_move_start = move_start;
                pl.entity.exact_move_facing = facing;
                pl.entity.route_length = 1;
                pl.entity.preanim_route_length = 0;
            }
        }

        if (flags & 0x20) != 0 {
            let mut target = p.g2_alt3();
            if target == 65535 {
                target = -1;
            }
            if let Some(pl) = player_slot_mut(c, id) {
                pl.entity.target_id = target;
            }
        }

        if (flags & 0x80) != 0 {
            let tx = p.g2_alt2();
            let tz = p.g2_alt1();
            if let Some(pl) = player_slot_mut(c, id) {
                pl.entity.target_tile_x = tx;
                pl.entity.target_tile_z = tz;
            }
        }

        if (flags & 0x10) != 0 {
            let mut anim = p.g2_alt2();
            if anim == 65535 {
                anim = -1;
            }
            let delay = p.g1_alt2();
            if let Some(pl) = player_slot_mut(c, id) {
                Client::trigger_player_anim(pl, anim, delay);
            }
        }

        if (flags & 0x200) != 0 {
            let mut spot = p.g2_alt1();
            let packed = p.g4();
            if let Some(pl) = player_slot_mut(c, id) {
                pl.entity.spotanim_height = packed >> 16;
                pl.entity.spotanim_last_cycle = (packed & 0xFFFF) + loop_cycle;
                pl.entity.spotanim_frame = 0;
                pl.entity.spotanim_cycle = 0;
                if pl.entity.spotanim_last_cycle > loop_cycle {
                    pl.entity.spotanim_frame = -1;
                }
                if spot == 65535 {
                    spot = -1;
                }
                pl.entity.spotanim_id = spot;
            }
        }

        if (flags & 0x400) != 0 {
            let value = p.g1_alt1();
            let kind = p.g1_alt3();
            let health = p.g1();
            let total = p.g1_alt2();
            if let Some(pl) = player_slot_mut(c, id) {
                pl.entity.add_hitmark(value, kind, loop_cycle);
                pl.entity.combat_cycle = loop_cycle + 300;
                pl.entity.health = health;
                pl.entity.total_health = total;
            }
        }

        if (flags & 0x1) != 0 {
            // Forced chat — '~' prefix (and the local player's own
            // lines) also land in the chatbox.
            let mut text = p.gjstr();
            let mut to_chatbox = id == 2047;
            if text.starts_with('~') {
                text = text[1..].to_string();
                to_chatbox = true;
            }
            let name = player_slot_mut(c, id).map(|pl| {
                pl.entity.chat = Some(text.clone());
                pl.entity.chat_colour = 0;
                pl.entity.chat_effect = 0;
                pl.entity.chat_timer = 150;
                pl.name.clone()
            });
            if to_chatbox {
                if let Some(name) = name {
                    crate::client::add_chat(c, 2, Some(name), Some(text), None, 0);
                }
            }
        }

        if (flags & 0x8) != 0 {
            let value = p.g1_alt1();
            let kind = p.g1_alt3();
            let health = p.g1_alt1();
            let total = p.g1();
            if let Some(pl) = player_slot_mut(c, id) {
                pl.entity.add_hitmark(value, kind, loop_cycle);
                pl.entity.combat_cycle = loop_cycle + 300;
                pl.entity.health = health;
                pl.entity.total_health = total;
            }
        }
    }
}

// @ObfuscatedName(— Client.getNpcPos, Client.java:7977-7998).
fn get_npc_pos(c: &mut Client, p: &mut crate::io::packet::Packet, psize: i32) {
    let mut removal_ids: Vec<i32> = Vec::new();
    let mut update_ids: Vec<i32> = Vec::new();

    get_npc_pos_old_vis(c, p, &mut removal_ids, &mut update_ids);
    get_npc_pos_new_vis(c, p, psize, &mut update_ids);
    get_npc_pos_extended(c, p, &update_ids);

    for &id in &removal_ids {
        if let Some(slot) = c.npcs.get_mut(id as usize) {
            let stale = slot.as_ref().map_or(false, |n| n.entity.cycle != c.loop_cycle);
            if stale {
                *slot = None;
            }
        }
    }
    if psize != p.pos {
        eprintln!("[game] NPC_INFO desync pos={} size={psize}", p.pos);
    }
}

// @ObfuscatedName(— Client.getNpcPosOldVis, Client.java:8001-8051).
fn get_npc_pos_old_vis(c: &mut Client, p: &mut crate::io::packet::Packet,
                       removal_ids: &mut Vec<i32>, update_ids: &mut Vec<i32>) {
    p.g_bit_start();
    let visible = p.g_bit(8);
    let prev_count = c.npc_count;
    if visible < prev_count {
        for i in visible..prev_count {
            removal_ids.push(c.npc_ids[i as usize]);
        }
    }
    if visible > prev_count {
        eprintln!("[game] NPC_INFO gnpov1 visible={visible} count={prev_count}");
        return;
    }
    c.npc_count = 0;
    let loop_cycle = c.loop_cycle;
    for i in 0..visible as usize {
        let id = c.npc_ids[i];
        if p.g_bit(1) == 0 {
            let count = c.npc_count as usize;
            c.npc_ids[count] = id;
            c.npc_count += 1;
            if let Some(Some(n)) = c.npcs.get_mut(id as usize) {
                n.entity.cycle = loop_cycle;
            }
            continue;
        }
        let mode = p.g_bit(2);
        match mode {
            0 => {
                let count = c.npc_count as usize;
                c.npc_ids[count] = id;
                c.npc_count += 1;
                if let Some(Some(n)) = c.npcs.get_mut(id as usize) {
                    n.entity.cycle = loop_cycle;
                }
                update_ids.push(id);
            }
            1 => {
                let count = c.npc_count as usize;
                c.npc_ids[count] = id;
                c.npc_count += 1;
                let dir = p.g_bit(3);
                if let Some(Some(n)) = c.npcs.get_mut(id as usize) {
                    n.entity.cycle = loop_cycle;
                    n.entity.move_code(dir, false);
                }
                if p.g_bit(1) == 1 {
                    update_ids.push(id);
                }
            }
            2 => {
                let count = c.npc_count as usize;
                c.npc_ids[count] = id;
                c.npc_count += 1;
                let dir1 = p.g_bit(3);
                let dir2 = p.g_bit(3);
                if let Some(Some(n)) = c.npcs.get_mut(id as usize) {
                    n.entity.cycle = loop_cycle;
                    n.entity.move_code(dir1, true);
                    n.entity.move_code(dir2, true);
                }
                if p.g_bit(1) == 1 {
                    update_ids.push(id);
                }
            }
            3 => {
                removal_ids.push(id);
            }
            _ => {}
        }
    }
}

// @ObfuscatedName("dm.ed(I)V") — Client.getNpcPosNewVis
// (Client.java:8055-8104).
fn get_npc_pos_new_vis(c: &mut Client, p: &mut crate::io::packet::Packet,
                       psize: i32, update_ids: &mut Vec<i32>) {
    let loop_cycle = c.loop_cycle;
    while p.bits_left(psize) >= 27 {
        let id = p.g_bit(15);
        if id == 32767 {
            break;
        }

        let fresh = c.npcs.get(id as usize).map_or(true, |o| o.is_none());
        if fresh {
            if let Some(slot) = c.npcs.get_mut(id as usize) {
                *slot = Some(crate::dash3d::client_npc::ClientNpc::default());
            }
        }

        let count = c.npc_count as usize;
        c.npc_ids[count] = id;
        c.npc_count += 1;

        let yaw = crate::client::ANGLE_TO_DIR[p.g_bit(3) as usize];
        let mut dz = p.g_bit(5);
        if dz > 15 {
            dz -= 32;
        }
        if p.g_bit(1) == 1 {
            update_ids.push(id);
        }
        let jump = p.g_bit(1) == 1;
        let type_id = p.g_bit(14);
        let mut dx = p.g_bit(5);
        if dx > 15 {
            dx -= 32;
        }

        let t = crate::config::npc_type::list(type_id);
        let (route_x, route_z) = match c.local_player.as_ref() {
            Some(lp) => (lp.route_x[0], lp.route_z[0]),
            None => (0, 0),
        };
        if let Some(Some(n)) = c.npcs.get_mut(id as usize) {
            n.entity.cycle = loop_cycle;
            n.type_id = type_id;
            if fresh {
                n.entity.dst_yaw = yaw;
                n.entity.yaw = yaw;
            }
            n.entity.size = t.size;
            n.entity.turnspeed = t.turnspeed;
            if n.entity.turnspeed == 0 {
                n.entity.yaw = 0;
            }
            n.entity.walkanim = t.walkanim;
            n.entity.walkanim_b = t.walkanim_b;
            // Java swaps l/r when copying from the type.
            n.entity.walkanim_l = t.walkanim_r;
            n.entity.walkanim_r = t.walkanim_l;
            n.entity.readyanim = t.readyanim;
            n.entity.turnleftanim = t.turnleftanim;
            n.entity.turnrightanim = t.turnrightanim;
            n.entity.teleport(route_x + dx, route_z + dz, jump);
        }
    }
    p.g_bit_end();
}

// @ObfuscatedName("ag.ex(B)V") — Client.getNpcPosExtended
// (Client.java:8108-8196).
fn get_npc_pos_extended(c: &mut Client, p: &mut crate::io::packet::Packet,
                        update_ids: &[i32]) {
    let loop_cycle = c.loop_cycle;
    for &id in update_ids {
        let flags = p.g1();

        if (flags & 0x80) != 0 {
            let value = p.g1();
            let kind = p.g1_alt2();
            let health = p.g1_alt1();
            let total = p.g1_alt1();
            if let Some(Some(n)) = c.npcs.get_mut(id as usize) {
                n.entity.add_hitmark(value, kind, loop_cycle);
                n.entity.combat_cycle = loop_cycle + 300;
                n.entity.health = health;
                n.entity.total_health = total;
            }
        }

        if (flags & 0x4) != 0 {
            let mut target = p.g2_alt1();
            if target == 65535 {
                target = -1;
            }
            if let Some(Some(n)) = c.npcs.get_mut(id as usize) {
                n.entity.target_id = target;
            }
        }

        if (flags & 0x2) != 0 {
            let tx = p.g2_alt3();
            let tz = p.g2_alt3();
            if let Some(Some(n)) = c.npcs.get_mut(id as usize) {
                n.entity.target_tile_x = tx;
                n.entity.target_tile_z = tz;
            }
        }

        if (flags & 0x1) != 0 {
            let mut spot = p.g2_alt1();
            let packed = p.g4();
            if let Some(Some(n)) = c.npcs.get_mut(id as usize) {
                n.entity.spotanim_height = packed >> 16;
                n.entity.spotanim_last_cycle = (packed & 0xFFFF) + loop_cycle;
                n.entity.spotanim_frame = 0;
                n.entity.spotanim_cycle = 0;
                if n.entity.spotanim_last_cycle > loop_cycle {
                    n.entity.spotanim_frame = -1;
                }
                if spot == 65535 {
                    spot = -1;
                }
                n.entity.spotanim_id = spot;
            }
        }

        if (flags & 0x8) != 0 {
            let mut anim = p.g2_alt3();
            if anim == 65535 {
                anim = -1;
            }
            let delay = p.g1_alt1();
            if let Some(Some(n)) = c.npcs.get_mut(id as usize) {
                // Java inlines triggerPlayerAnim's duplicatebehaviour
                // rules for NPCs (Client.java:8151-8169).
                let e = &mut n.entity;
                if e.primary_seq_id == anim && anim != -1 {
                    let dup = crate::config::seq_type::list(anim).duplicatebehaviour;
                    if dup == 1 {
                        e.primary_seq_frame = 0;
                        e.primary_seq_cycle = 0;
                        e.primary_seq_delay = delay;
                        e.primary_seq_loop = 0;
                    }
                    if dup == 2 {
                        e.primary_seq_loop = 0;
                    }
                } else if anim == -1
                    || e.primary_seq_id == -1
                    || crate::config::seq_type::list(anim).priority
                        >= crate::config::seq_type::list(e.primary_seq_id).priority
                {
                    e.primary_seq_id = anim;
                    e.primary_seq_frame = 0;
                    e.primary_seq_cycle = 0;
                    e.primary_seq_delay = delay;
                    e.primary_seq_loop = 0;
                    e.preanim_route_length = e.route_length;
                }
            }
        }

        if (flags & 0x40) != 0 {
            let type_id = p.g2();
            let t = crate::config::npc_type::list(type_id);
            if let Some(Some(n)) = c.npcs.get_mut(id as usize) {
                n.type_id = type_id;
                n.entity.size = t.size;
                n.entity.turnspeed = t.turnspeed;
                n.entity.walkanim = t.walkanim;
                n.entity.walkanim_b = t.walkanim_b;
                n.entity.walkanim_l = t.walkanim_r;
                n.entity.walkanim_r = t.walkanim_l;
                n.entity.readyanim = t.readyanim;
                n.entity.turnleftanim = t.turnleftanim;
                n.entity.turnrightanim = t.turnrightanim;
            }
        }

        if (flags & 0x20) != 0 {
            let text = p.gjstr();
            if let Some(Some(n)) = c.npcs.get_mut(id as usize) {
                n.entity.chat = Some(text);
                n.entity.chat_timer = 100;
            }
        }

        if (flags & 0x10) != 0 {
            let value = p.g1_alt3();
            let kind = p.g1_alt3();
            let health = p.g1_alt3();
            let total = p.g1_alt1();
            if let Some(Some(n)) = c.npcs.get_mut(id as usize) {
                n.entity.add_hitmark(value, kind, loop_cycle);
                n.entity.combat_cycle = loop_cycle + 300;
                n.entity.health = health;
                n.entity.total_health = total;
            }
        }
    }
}

fn login_done(c: &mut Client, first_packet: Vec<u8>) {
    // Java loginDone: players[2047] = localPlayer = new ClientPlayer(),
    // ptype = -1, mapBuildCenterZoneX = -1, etc. We don't model the
    // full players[2048] array yet — just the local slot.
    c.ptype = -1;
    c.local_player = Some(crate::dash3d::ClientPlayer::new());
    c.map_build_center_zone_x = -1;
    c.map_build_center_zone_z = -1;
    eprintln!("[login] login_done — first packet ptype 21, {} bytes (RebuildNormal)", first_packet.len());
    // The first packet IS RebuildNormal — feed it through the regular
    // handler so the map fetch kicks off. rebuild_packet → start_rebuild
    // calls set_main_state(25) on its own. Java does NOT override the
    // state here; mainredraw at state 25 drives mapBuildLoop until the
    // map archives all land, then the final loadGround/loadLocations
    // pass calls setMainState(30).
    handle_packet(c, 21, &first_packet);
    eprintln!("[login] state after RebuildNormal = {}", c.state);
}

// @ObfuscatedName("c.bo(B)V") — Client.lostCon.
//
// Called when the in-game (state 30) TCP stream errors out. Java moves
// the current stream onto `prevStream` (so the post-reconnect packet
// loop can drain any unread bytes), resets the packet pump, and
// transitions to state 40 (reconnect).
pub fn lost_con(c: &mut Client) {
    // Move current stream onto prev_stream; Java's Client field name
    // is `field1810`. Reconnect path will swap them back if the
    // reconnect succeeds.
    c.prev_stream = c.login_stream.take();
    c.ptype = -1;
    c.psize = 0;
    c.state = 40;
}

// @ObfuscatedName("c.dn(II)V") — Client.reconnectDone.
//
// Java's reconnect path runs after the server accepts a state==40
// reconnection. Clears the inbound opcode ring (ptype0/1/2 — the
// "last three opcodes" used for crash reports), drops the target
// hint state, then transitions to state 30 (in-game).
pub fn reconnect_done(c: &mut Client) {
    c.ptype = -1;
    c.ptype0 = -1;
    c.ptype1 = -1;
    c.ptype2 = -1;
    c.psize = 0;
    c.hint_npc = -1;
    c.hint_player = -1;
    c.hint_tile_x = 0;
    c.hint_tile_z = 0;
    c.hint_type = 0;
    // Mirror Java's "drop in-flight rebuild" behaviour.
    c.map_build_center_zone_x = -1;
    c.map_build_center_zone_z = -1;
    // Java additionally drops every cached inv map + marks every UI
    // component dirty so the server-driven repaint is a full refresh.
    crate::client_inv_cache::delete_all();
    crate::client::redraw_all_components();
    // Reset target / selected-use state.
    c.target_mode = false;
    c.target_com = -1;
    c.target_sub = -1;
    c.selected_cycle = 0;
    c.selected_com = -1;
    c.selected_item = -1;
    c.timeout_timer = 0;
    c.state = 30;
}

fn login_error(c: &mut Client, response: i32) {
    // Java dispatches response codes to TitleScreen.loginMes which sets
    // line1/line2/line3 strings. Until TitleScreen.loginMes is wired
    // we print + bounce back to state 10.
    let label = match response {
        -3 => "out of memory",
        -2 => "connection lost",
        -1 => "couldn't connect",
        3  => "invalid credentials",
        4  => "account disabled",
        5  => "already online",
        6  => "client too old",
        7  => "world full",
        8  => "login server offline",
        9  => "too many login attempts",
        10 => "couldn't verify identity",
        11 => "rejected session",
        12 => "members only",
        13 => "could not complete login",
        14 => "server is updating",
        16 => "too many login attempts (>5 in 5 min)",
        17 => "members area",
        18 => "account locked",
        19 => "world full",
        20 => "invalid login server",
        21 => "transferred to another world",
        _  => "login failed",
    };
    eprintln!("[login] {response}: {label}");
    c.login_step = 0;
    c.login_stream = None;
    c.login_socket_req = None;
    c.state = 10;
}

fn login_retry_or_error(c: &mut Client, _response: i32) {
    if c.login_fail_count < 1 {
        c.login_fail_count += 1;
        // Swap the port (Java fallback in case game port refused).
        if c.login_game_port == c.login_port {
            c.login_port = c.login_js5_port;
        } else {
            c.login_port = c.login_game_port;
        }
        c.login_step = 0;
    } else {
        login_error(c, -3);
    }
}

fn seeded_rand(_c: &mut Client) -> i32 {
    use std::sync::atomic::AtomicU64;
    static SEED: AtomicU64 = AtomicU64::new(0xDEADBEEFCAFEBABE);
    let prev = SEED.load(Ordering::Relaxed);
    let next = prev.wrapping_mul(0x5DEECE66D).wrapping_add(0xB);
    SEED.store(next, Ordering::Relaxed);
    ((next >> 16) & 0x7FFF_FFFF) as i32
}

fn archive_crcs(c: &Client) -> [i32; 16] {
    let slots = [
        c.anims, c.bases, c.config, c.interfaces, c.jag_fx, c.maps,
        c.songs, c.models, c.sprites, c.textures, c.binary, c.jingles,
        c.scripts, c.font_metrics, c.vorbis, c.patches,
    ];
    let mut out = [0i32; 16];
    let reg = js5_net::LOADERS.lock().unwrap();
    for (i, &slot) in slots.iter().enumerate() {
        if slot < 0 { continue; }
        if let Some(l) = reg.get(slot as usize).and_then(|o| o.as_ref()) {
            out[i] = l.index_crc;
        }
    }
    out
}
