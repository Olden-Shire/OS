//! Server→client messages. Each builder returns a [`ServerPacket`]
//! (opcode + body); the connection layer frames it (opcode byte +
//! optional size prefix) when flushing.
//!
//! Opcodes/sizes mirror Engine2007
//! src/network/os/server/codec/game/*Encoder.ts; the byte layouts
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

/// IF_SETTEXT (197) — set a component's text. Variable length: the client
/// reads jstr(text) then g4_alt3(component) (Client.java:7164-7172). The Java
/// client's Protocol.SERVERPROT_SIZES[197] is -2, i.e. a 2-byte length prefix
/// (Var2) — sending Var1 desyncs the read pump and disconnect-loops the client.
pub fn if_settext(component: i32, text: &str) -> ServerPacket {
    let mut p = body(text.len() + 6);
    p.pjstr(text);
    p.p4_alt3(component);
    finish(197, SizeKind::Var2, p)
}

/// IF_SETHIDE (84) — show or hide a component. Client reads g4_alt1(component)
/// then g1_alt3(hide) (Client.java:6324-6332).
pub fn if_sethide(component: i32, hide: bool) -> ServerPacket {
    let mut p = body(5);
    p.p4_alt1(component);
    p.p1_alt3(i32::from(hide));
    finish(84, SizeKind::Fixed(5), p)
}

/// IF_SETANIM (176) — play `seq` on a component's model. Client reads
/// g2b_alt3(seq) then g4(component) (Client.java:5826).
pub fn if_setanim(component: i32, seq: i32) -> ServerPacket {
    let mut p = body(6);
    p.p2_alt3(seq);
    p.p4(component);
    finish(176, SizeKind::Fixed(6), p)
}

/// IF_SETCOLOUR (234) — set a component's text colour (passed already in the
/// client's 15-bit form). Client reads g4_alt1(component) then g2(colour)
/// (Client.java:6107).
pub fn if_setcolour(component: i32, colour15: i32) -> ServerPacket {
    let mut p = body(6);
    p.p4_alt1(component);
    p.p2(colour15);
    finish(234, SizeKind::Fixed(6), p)
}

/// IF_SETMODEL (251) — set a component's model. Client reads g2(model) then
/// g4_alt2(component) (Client.java:7030).
pub fn if_setmodel(component: i32, model: i32) -> ServerPacket {
    let mut p = body(6);
    p.p2(model);
    p.p4_alt2(component);
    finish(251, SizeKind::Fixed(6), p)
}

/// UPDATE_INV_FULL (29) — set a component's whole inventory. Client reads
/// g4(component), g2(inv), g2(count), then per slot g1_alt3(qty) [g4_alt1 when
/// 255] and g2_alt1(obj+1) (0 = empty). Mirrors crates/client login.rs:941.
pub fn update_inv_full(component: i32, inv_id: i32, slots: &[(i32, i32)]) -> ServerPacket {
    let mut p = body(8 + slots.len() * 7);
    p.p4(component);
    p.p2(inv_id);
    p.p2(slots.len() as i32);
    for &(obj, count) in slots {
        let wire_id = if obj >= 0 { obj + 1 } else { 0 };
        if count >= 255 {
            p.p1_alt3(255);
            p.p4_alt1(count);
        } else {
            p.p1_alt3(count);
        }
        p.p2_alt1(wire_id);
    }
    finish(29, SizeKind::Var2, p)
}

/// LAST_LOGIN_INFO (241) — the previous-login IP shown on the welcome screen.
/// Client reads g4_alt1(ip) (login.rs:1189). (The rev1 packet is just the IP,
/// not the 377 multi-field block.)
pub fn last_login_info(ip: i32) -> ServerPacket {
    let mut p = body(4);
    p.p4_alt1(ip);
    finish(241, SizeKind::Fixed(4), p)
}

/// UPDATE_INV_STOPTRANSMIT (117) — clear a component's inventory display.
/// Client reads g4_alt1(component) (login.rs:1010).
pub fn update_inv_stop_transmit(component: i32) -> ServerPacket {
    let mut p = body(4);
    p.p4_alt1(component);
    finish(117, SizeKind::Fixed(4), p)
}

/// IF_SETOBJECT (102) — show item `obj` at `scale` on a component. Client reads
/// g4(component), g2_alt2(obj), g4_alt1(scale) (Client.java:6633).
pub fn if_setobject(component: i32, obj: i32, scale: i32) -> ServerPacket {
    let mut p = body(10);
    p.p4(component);
    p.p2_alt2(obj);
    p.p4_alt1(scale);
    finish(102, SizeKind::Fixed(10), p)
}

/// IF_SETPOSITION (85) — move a component to (x, y). Client reads g2b_alt2(x),
/// g2b_alt1(y), g4_alt1(component) (Client.java:6125).
pub fn if_setposition(component: i32, x: i32, y: i32) -> ServerPacket {
    let mut p = body(8);
    p.p2_alt2(x);
    p.p2_alt1(y);
    p.p4_alt1(component);
    finish(85, SizeKind::Fixed(8), p)
}

/// IF_SETNPCHEAD (66) — show npc `npc`'s chathead on a component. Client reads
/// g4_alt2(component), g2_alt2(npc) (Client.java:6398).
pub fn if_setnpchead(component: i32, npc: i32) -> ServerPacket {
    let mut p = body(6);
    p.p4_alt2(component);
    p.p2_alt2(npc);
    finish(66, SizeKind::Fixed(6), p)
}

/// IF_SETPLAYERHEAD (171) — show the local player's chathead on a component.
/// Client reads g4_alt3(component) (Client.java:7094).
pub fn if_setplayerhead(component: i32) -> ServerPacket {
    let mut p = body(4);
    p.p4_alt3(component);
    finish(171, SizeKind::Fixed(4), p)
}

/// IF_SETSCROLLPOS (50) — scroll a component to `pos`. Client reads
/// g4_alt3(component), g2(pos) (Client.java:6981).
pub fn if_setscrollpos(component: i32, pos: i32) -> ServerPacket {
    let mut p = body(6);
    p.p4_alt3(component);
    p.p2(pos);
    finish(50, SizeKind::Fixed(6), p)
}

/// IF_SETANGLE (26) — set a component model's camera angle + zoom. The rev1
/// client reads g2_alt2(xan), g2(zoom), g4_alt1(component), g2(yan) — note it
/// uses a 4-byte component, unlike the newer-rev 2-byte form (Client.java:7003).
pub fn if_setangle(component: i32, xan: i32, yan: i32, zoom: i32) -> ServerPacket {
    let mut p = body(10);
    p.p2_alt2(xan);
    p.p2(zoom);
    p.p4_alt1(component);
    p.p2(yan);
    finish(26, SizeKind::Fixed(10), p)
}

/// SET_PLAYER_OP (164) — set one of the player's right-click menu options
/// (slot 1-8, e.g. "Follow"/"Trade"/"Attack"). Variable length: the rev1 client
/// reads jstr(label), g1_alt1(slot), g1_alt3(primary) (Client.java:6446); a
/// "null" label clears the slot.
pub fn set_player_op(slot: i32, label: &str, primary: i32) -> ServerPacket {
    let mut p = body(label.len() + 3);
    p.pjstr(label);
    p.p1_alt1(slot);
    p.p1_alt3(primary);
    finish(164, SizeKind::Var1, p)
}

/// IF_SETROTATION (217, "IF_SETROTATESPEED") — set a component model's auto-spin
/// speed. The rev1 client reads g4_alt1(com), g2_alt3(x), g2_alt3(y) and stores
/// modelSpin = (x << 16) + y (Client.java:6621).
pub fn if_setrotation(component: i32, x_angle: i32, y_angle: i32) -> ServerPacket {
    let mut p = body(8);
    p.p4_alt1(component);
    p.p2_alt3(x_angle);
    p.p2_alt3(y_angle);
    finish(217, SizeKind::Fixed(8), p)
}

/// RESET_CLIENT_VARCACHE (129) — zero every client varp. Zero body; sent on
/// login so a reconnecting client doesn't keep stale varps from a prior
/// session (Client.java:6337 "VARP_RESET").
pub fn reset_client_var_cache() -> ServerPacket {
    ServerPacket { opcode: 129, size: SizeKind::Fixed(0), body: Vec::new() }
}

/// RESET_ANIMS (72) — clear every entity's playing animation. Zero body; sent
/// on login to reset the client's animation state (Client.java:6963).
pub fn reset_anims() -> ServerPacket {
    ServerPacket { opcode: 72, size: SizeKind::Fixed(0), body: Vec::new() }
}

/// CHAT_FILTER_SETTINGS (137) — set the public / trade chat filter modes the
/// client shows. Client reads g1(public), g1(trade) (Client.java:6037).
pub fn chat_filter_settings(public_mode: i32, trade_mode: i32) -> ServerPacket {
    let mut p = body(2);
    p.p1(public_mode);
    p.p1(trade_mode);
    finish(137, SizeKind::Fixed(2), p)
}

/// IF_CLOSESUB (87) — close the sub-interface attached at `component`. Client
/// reads g4(component) (Client.java:5810).
pub fn if_close_sub(component: i32) -> ServerPacket {
    let mut p = body(4);
    p.p4(component);
    finish(87, SizeKind::Fixed(4), p)
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

/// UNSET_MAP_FLAG (161) — clear the client's minimap move-to flag. Zero
/// body; the client just sets `minimapFlagX = 0` (Client.java:6577-6582).
/// Sent when the engine cancels a queued walk (e.g. an exact-move).
pub fn unset_map_flag() -> ServerPacket {
    ServerPacket { opcode: 161, size: SizeKind::Fixed(0), body: Vec::new() }
}

/// MINIMAP_TOGGLE (190) — set the client's minimap visibility state
/// (0 normal, 1 hidden, 2 hidden + compass, 3 whole map gone, …). The
/// client reads a single byte into `minimapState` (Client.java:6315-6320).
pub fn minimap_toggle(state: i32) -> ServerPacket {
    let mut p = body(1);
    p.p1(state);
    finish(190, SizeKind::Fixed(1), p)
}

/// UPDATE_RUNWEIGHT (1).
pub fn update_runweight(value: i32) -> ServerPacket {
    let mut p = body(2);
    p.p2(value);
    finish(1, SizeKind::Fixed(2), p)
}

/// VARP_SMALL (88) — set a player varp whose value fits a signed byte.
/// Client read order: varp = g2_alt1, value = g1b_alt3.
pub fn varp_small(varp: i32, value: i32) -> ServerPacket {
    let mut p = body(3);
    p.p2_alt1(varp);
    p.p1_alt3(value);
    finish(88, SizeKind::Fixed(3), p)
}

/// VARP_LARGE (180) — set a player varp needing the full 32-bit value.
/// Client read order: varp = g2_alt3, value = g4.
pub fn varp_large(varp: i32, value: i32) -> ServerPacket {
    let mut p = body(6);
    p.p2_alt3(varp);
    p.p4(value);
    finish(180, SizeKind::Fixed(6), p)
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

    // Tutorial-island map skip (Client.rebuildPacket, Client.java:4805-4822).
    // Around tutorial island the client OMITS the empty build-area mapsquares
    // from its region list, then assigns the sequentially-read `mapKeys[i]` to
    // the i-th *non-skipped* region. We must skip the SAME mapsquares when
    // emitting keys here, or every key after the first skip shifts by one and
    // the loc file decrypts to garbage ("Invalid GZIP header!" on entry). The
    // flag keys off the centre mapsquare (zx/8, zz/8). For non-tutorial regions
    // `tutorial` is false and every mapsquare is emitted, as before.
    let tutorial = ((zx / 8 == 48 || zx / 8 == 49) && zz / 8 == 48)
        || (zx / 8 == 48 && zz / 8 == 148);

    let mut mx = (zx - 6) >> 3; // == Java var11
    while mx <= (zx + 6) >> 3 {
        let mut mz = (zz - 6) >> 3; // == Java var12
        while mz <= (zz + 6) >> 3 {
            let included = !tutorial
                || (mz != 49 && mz != 149 && mz != 147 && mx != 50
                    && !(mx == 49 && mz == 47));
            if included {
                let key = xtea_keys(mx, mz);
                for k in key {
                    p.p4_alt2(k);
                }
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

// ── Zone updates ──────────────────────────────────────────────────

/// UPDATE_ZONE_PARTIAL_FOLLOWS (89) — sets the build-area-local zone base for
/// the OBJ/LOC packets that follow. Coords are the zone's SW corner in the
/// observer's 104-tile build area.
pub fn update_zone_partial_follows(local_zone_x: i32, local_zone_z: i32) -> ServerPacket {
    let mut p = body(2);
    p.p1(local_zone_x);
    p.p1_alt3(local_zone_z);
    finish(89, SizeKind::Fixed(2), p)
}

/// OBJ_ADD (173) — a ground item. `slot` packs the tile within the current
/// zone as `(x_off << 4) | z_off` (3 bits each).
pub fn obj_add(slot: i32, count: i32, obj_id: i32) -> ServerPacket {
    let mut p = body(5);
    p.p1_alt1(slot);
    p.p2_alt2(count);
    p.p2_alt3(obj_id);
    finish(173, SizeKind::Fixed(5), p)
}

/// OBJ_DEL (207) — removes a ground item. Field order mirrors the client:
/// obj id first (`p2_alt3`), then the in-zone `slot` (`p1`).
pub fn obj_del(slot: i32, obj_id: i32) -> ServerPacket {
    let mut p = body(3);
    p.p2_alt3(obj_id);
    p.p1(slot);
    finish(207, SizeKind::Fixed(3), p)
}

/// LOC_ADD_CHANGE (154) — spawn or retype a map object in the current zone.
/// `shape`/`angle` pack into one byte as `(shape << 2) | angle`.
pub fn loc_add_change(slot: i32, loc_id: i32, shape: i32, angle: i32) -> ServerPacket {
    let mut p = body(4);
    p.p2_alt3(loc_id);
    p.p1_alt1((shape << 2) | (angle & 0x3));
    p.p1_alt2(slot);
    finish(154, SizeKind::Fixed(4), p)
}

/// LOC_DEL (7) — remove a map object (door closed, tree felled, …).
pub fn loc_del(slot: i32, shape: i32, angle: i32) -> ServerPacket {
    let mut p = body(2);
    p.p1_alt3((shape << 2) | (angle & 0x3));
    p.p1_alt1(slot);
    finish(7, SizeKind::Fixed(2), p)
}

/// OBJ_REVEAL (215) — a previously private (just-dropped) ground item becomes
/// public. `owner` is the dropper's player slot; the client only renders it
/// for non-owners (the owner already saw the private OBJ_ADD).
pub fn obj_reveal(slot: i32, owner: i32, count: i32, obj_id: i32) -> ServerPacket {
    let mut p = body(7);
    p.p1_alt2(slot);
    p.p2(owner);
    p.p2_alt2(count);
    p.p2(obj_id);
    finish(215, SizeKind::Fixed(7), p)
}

/// HINT_ARROW (160) — float an arrow over npc `nid` (type 1). Fixed 6 bytes:
/// the trailing fields are zero-padded for entity hints.
pub fn hint_npc(nid: i32) -> ServerPacket {
    let mut p = body(6);
    p.p1(1);
    p.p2(nid);
    p.p2(0);
    p.p1(0);
    finish(160, SizeKind::Fixed(6), p)
}

/// HINT_ARROW (160) — float an arrow over player `slot` (type 10).
pub fn hint_player(slot: i32) -> ServerPacket {
    let mut p = body(6);
    p.p1(10);
    p.p2(slot);
    p.p2(0);
    p.p1(0);
    finish(160, SizeKind::Fixed(6), p)
}

/// HINT_ARROW (160) — float an arrow over an absolute tile. `offset` (2..6)
/// selects the arrow's position on the tile (centre/edges); `height` lifts it.
pub fn hint_tile(offset: i32, x: i32, z: i32, height: i32) -> ServerPacket {
    let mut p = body(6);
    p.p1(offset);
    p.p2(x);
    p.p2(z);
    p.p1(height);
    finish(160, SizeKind::Fixed(6), p)
}

/// HINT_ARROW (160) — clear any active hint (type -1).
pub fn hint_stop() -> ServerPacket {
    let mut p = body(6);
    p.p1(-1);
    p.p2(0);
    p.p2(0);
    p.p1(0);
    finish(160, SizeKind::Fixed(6), p)
}

/// CAM_LOOKAT (225) — aim the cutscene camera at a scene-local tile.
/// Client read order: x = g1, z = g1, height = g2, rate = g1, rate2 = g1.
pub fn cam_lookat(local_x: i32, local_z: i32, height: i32, rate: i32, rate2: i32) -> ServerPacket {
    let mut p = body(6);
    p.p1(local_x);
    p.p1(local_z);
    p.p2(height);
    p.p1(rate);
    p.p1(rate2);
    finish(225, SizeKind::Fixed(6), p)
}

/// CAM_MOVETO (169) — move the cutscene camera to a scene-local tile. Same
/// layout as CAM_LOOKAT.
pub fn cam_moveto(local_x: i32, local_z: i32, height: i32, rate: i32, rate2: i32) -> ServerPacket {
    let mut p = body(6);
    p.p1(local_x);
    p.p1(local_z);
    p.p2(height);
    p.p1(rate);
    p.p1(rate2);
    finish(169, SizeKind::Fixed(6), p)
}

/// CAM_SHAKE (17) — shake camera component `slot` (0..4). The remaining three
/// bytes are pass-through jitter parameters (axis / random cap / amplitude).
pub fn cam_shake(slot: i32, axis: i32, random: i32, amplitude: i32) -> ServerPacket {
    let mut p = body(4);
    p.p1(slot);
    p.p1(axis);
    p.p1(random);
    p.p1(amplitude);
    finish(17, SizeKind::Fixed(4), p)
}

/// CAM_RESET (198) — drop the cutscene camera back to the default orbit. No body.
pub fn cam_reset() -> ServerPacket {
    ServerPacket { opcode: 198, size: SizeKind::Fixed(0), body: Vec::new() }
}

/// OBJ_COUNT (106) — restack a ground item in place (e.g. a pile grew/shrank).
/// Client read order: slot = g1, obj = g2, oldCount = g2, newCount = g2.
pub fn obj_count(slot: i32, obj_id: i32, old_count: i32, new_count: i32) -> ServerPacket {
    let mut p = body(7);
    p.p1(slot);
    p.p2(obj_id);
    p.p2(old_count);
    p.p2(new_count);
    finish(106, SizeKind::Fixed(7), p)
}

/// LOC_ANIM (6) — play an animation `seq` on the loc at `slot` with the given
/// shape/angle (packed `(shape<<2)|angle`). Client read order:
/// seq = g2_alt2, slot = g1_alt2, shape/angle = g1_alt3.
pub fn loc_anim(slot: i32, seq: i32, shape: i32, angle: i32) -> ServerPacket {
    let mut p = body(4);
    p.p2_alt2(seq);
    p.p1_alt2(slot);
    p.p1_alt3((shape << 2) | angle);
    finish(6, SizeKind::Fixed(4), p)
}

/// MAP_ANIM (20) — a one-shot spotanim played on a tile. `slot` is the packed
/// tile-in-zone `(localX&7)<<4 | (localZ&7)`. Client read order:
/// slot = g1, spotanim = g2, height = g1, delay = g2.
pub fn map_anim(slot: i32, spotanim: i32, height: i32, delay: i32) -> ServerPacket {
    let mut p = body(6);
    p.p1(slot);
    p.p2(spotanim);
    p.p1(height);
    p.p2(delay);
    finish(20, SizeKind::Fixed(6), p)
}

/// MAP_PROJANIM (32) — a projectile from a source tile (the packed `slot`)
/// toward `(dx,dz)` tiles away, optionally homing on entity `target`
/// (-1 = none). Heights are sent at quarter resolution. Client read order:
/// slot = g1, dx = g1b, dz = g1b, target = g2b, spotanim = g2,
/// srcHeight/4 = g1, dstHeight/4 = g1, startDelay = g2, endDelay = g2,
/// peak = g1, arc = g1.
#[allow(clippy::too_many_arguments)]
pub fn map_projanim(
    slot: i32,
    dx: i32,
    dz: i32,
    target: i32,
    spotanim: i32,
    src_height: i32,
    dst_height: i32,
    start_delay: i32,
    end_delay: i32,
    peak: i32,
    arc: i32,
) -> ServerPacket {
    let mut p = body(15);
    p.p1(slot);
    p.p1(dx); // signed byte: dstX - srcX
    p.p1(dz); // signed byte: dstZ - srcZ
    p.p2(target); // signed 2-byte
    p.p2(spotanim);
    p.p1(src_height / 4);
    p.p1(dst_height / 4);
    p.p2(start_delay);
    p.p2(end_delay);
    p.p1(peak);
    p.p1(arc);
    finish(32, SizeKind::Fixed(15), p)
}
