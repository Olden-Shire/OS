// jagex3.dash3d — minimal placeholders for the world entities the
// network layer talks about. Only ClientPlayer is fleshed out enough to
// track the local player's position; ClientNpc / projectile / spotanim
// land later.

#![allow(dead_code)]

// @ObfuscatedName("fi") — jag::oldscape::ClientPlayer extends ClientEntity.
//
// Composes the full ClientEntity (movement, anim, hits, route queue)
// plus PlayerModel (appearance, worn-obj ids). Existing callers reach
// `player.x` / `player.z` / `player.route_x` / `route_len` / `cycle` /
// `level` directly — those fields stay at the top level as shims that
// mirror the entity's state, so we don't break consumers while the
// full migration to `player.entity.*` lands.
use crate::dash3d::client_entity::ClientEntity;
use crate::dash3d::player_model::PlayerModel;

#[derive(Debug, Clone, Default)]
pub struct ClientPlayer {
    // Shim fields — synced with `entity` by teleport / move_code.
    // @ObfuscatedName("fz.j")
    pub x: i32,
    // @ObfuscatedName("fz.z")
    pub z: i32,
    pub level: i32,
    // @ObfuscatedName("fz.bt")
    pub route_x: [i32; 10],
    // @ObfuscatedName("fz.bw")
    pub route_z: [i32; 10],
    pub route_len: i32,
    // @ObfuscatedName("fz.bi")
    pub cycle: i32,

    // Full entity state — the canonical movement/anim queue. Methods
    // that need the new fields (hitmark queue, primary/secondary seq,
    // chat overhead, etc.) reach into this directly.
    pub entity: ClientEntity,

    // @ObfuscatedName("fi.bf") — composed avatar model state. Stub
    // until the appearance opcode lands.
    pub model: PlayerModel,

    // @ObfuscatedName("fi.bu")
    pub name: String,
    // @ObfuscatedName("fi.bq")
    pub headicon_pk: i32,
    // @ObfuscatedName("fi.bj")
    pub headicon_prayer: i32,
    // @ObfuscatedName("fi.bz")
    pub combat_level: i32,
    // @ObfuscatedName("fi.bm")
    pub skill_level: i32,
    // @ObfuscatedName("fi.cp")
    pub team: i32,
}

impl ClientPlayer {
    // @ObfuscatedName("fi.f(I)Z") — ClientPlayer.ready. Java:
    // `return this.model != null;` — true once the appearance block
    // has been decoded.
    pub fn ready(&self) -> bool {
        self.model.applied
    }

    pub fn new() -> Self {
        Self {
            x: 0, z: 0, level: 0,
            route_x: [0; 10], route_z: [0; 10], route_len: 0,
            cycle: 0,
            entity: ClientEntity::default(),
            model: PlayerModel::default(),
            name: String::new(),
            headicon_pk: -1,
            headicon_prayer: -1,
            combat_level: 0,
            skill_level: 0,
            team: 0,
        }
    }

    // Mirror entity state into the legacy shim fields. Called at the
    // tail of teleport / move_code so existing readers stay in sync.
    fn sync_from_entity(&mut self) {
        self.x = self.entity.x;
        self.z = self.entity.z;
        self.route_x = self.entity.route_x;
        self.route_z = self.entity.route_z;
        self.route_len = self.entity.route_length;
        self.cycle = self.entity.cycle;
    }

    // @ObfuscatedName("fz.b(IIZB)V") — ClientEntity.teleport.
    // Verbatim port via ClientEntity. The legacy x/z shim fields
    // are updated to the new entity state.
    pub fn teleport(&mut self, tile_x: i32, tile_z: i32, jump: bool) {
        self.entity.teleport(tile_x, tile_z, jump);
        self.sync_from_entity();
    }

    // @ObfuscatedName("fz.y(IZI)V") — ClientEntity.moveCode.
    pub fn move_code(&mut self, direction: i32, run_step: bool) {
        self.entity.move_code(direction, run_step);
        self.sync_from_entity();
    }

    // @ObfuscatedName("fz.t(I)V") — ClientEntity.abortRoute.
    pub fn abort_route(&mut self) {
        self.entity.abort_route();
        self.sync_from_entity();
    }

    // @ObfuscatedName("fz.k(IIIB)V") — ClientEntity.addHitmark.
    pub fn add_hitmark(&mut self, value: i32, kind: i32, current_cycle: i32) {
        self.entity.add_hitmark(value, kind, current_cycle);
    }

    // @ObfuscatedName("fi.am(Lev;I)V") — ClientPlayer.setAppearance.
    // Verbatim port of ClientPlayer.java:73-148. Parses the appearance
    // opcode body (12 worn-slot ids with a packed g1/g2 prefix, 5
    // recol indices, 8 anim ids, the display name, combat level and
    // skill total).
    //
    // `is_local_player` is the Java check `Client.localPlayer == this`
    // — used to push the player's name into JagException so crash
    // reports identify the session.
    pub fn set_appearance(
        &mut self,
        packet: &mut crate::io::packet::Packet,
        is_local_player: bool,
    ) {
        use crate::config::obj_type;
        use crate::dash3d::recols_runescape;

        packet.pos = 0;
        let gender_flag = packet.g1();
        self.headicon_pk = packet.g1b() as i32;
        self.headicon_prayer = packet.g1b() as i32;
        let mut npc_override: i32 = -1;
        self.team = 0;

        let mut worn = [0i32; 12];
        for i in 0..12 {
            let lead = packet.g1();
            if lead == 0 {
                worn[i] = 0;
            } else {
                let lo = packet.g1();
                worn[i] = (lead << 8) + lo;
                if i == 0 && worn[0] == 65535 {
                    npc_override = packet.g2();
                    break;
                }
                if worn[i] >= 512 {
                    if let Some(obj) = obj_type::list(worn[i] - 512) {
                        if obj.team != 0 {
                            self.team = obj.team;
                        }
                    }
                }
            }
        }

        let mut recols = [0i32; 5];
        for i in 0..5 {
            let mut v = packet.g1();
            let palette = recols_runescape::recol1d(i);
            if v < 0 || (v as usize) >= palette.len() {
                v = 0;
            }
            recols[i] = v;
        }

        let read_anim = |p: &mut crate::io::packet::Packet| {
            let v = p.g2();
            if v == 65535 { -1 } else { v }
        };

        self.entity.readyanim = read_anim(packet);
        self.entity.turnleftanim = read_anim(packet);
        self.entity.turnrightanim = self.entity.turnleftanim;
        self.entity.walkanim = read_anim(packet);
        self.entity.walkanim_b = read_anim(packet);
        self.entity.walkanim_l = read_anim(packet);
        self.entity.walkanim_r = read_anim(packet);
        self.entity.runanim = read_anim(packet);

        let name = packet.gjstr();
        if is_local_player {
            // Java: JagException.username = this.name. Persist via the
            // shared crash-context hook when it lands.
        }
        self.combat_level = packet.g1();
        self.skill_level = packet.g2();
        self.model.apply_appearance(worn, recols, gender_flag == 1, npc_override);
        self.model.name = name.clone();
        self.model.combat_level = self.combat_level;
        self.name = name;
    }
}
