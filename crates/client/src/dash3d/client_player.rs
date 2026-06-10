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

    // @ObfuscatedName("fi.bn") — render height (getAvH at the fine
    // coords), stamped by addPlayers each frame.
    pub y: i32,
    // @ObfuscatedName("fi.cl") — crowd LOD: drop the secondary anim
    // when too many players are on screen.
    pub low_mem: bool,
    // @ObfuscatedName("fi.bb") family — the carried/animated loc model
    // (agility obstacles etc.) merged into the avatar between
    // locStartCycle and locEndCycle.
    pub loc_model: Option<crate::dash3d::model_lit::ModelLit>,
    pub loc_start_cycle: i32,
    pub loc_end_cycle: i32,
    pub loc_offset_x: i32,
    pub loc_offset_y: i32,
    pub loc_offset_z: i32,
    // @ObfuscatedName("fi.bd") family — the loc's tile span, used by
    // the addDynamic span variant while the loc anim runs.
    pub min_tile_x: i32,
    pub min_tile_z: i32,
    pub max_tile_x: i32,
    pub max_tile_z: i32,
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
            y: 0,
            low_mem: false,
            loc_model: None,
            loc_start_cycle: 0,
            loc_end_cycle: 0,
            loc_offset_x: 0,
            loc_offset_y: 0,
            loc_offset_z: 0,
            min_tile_x: 0,
            min_tile_z: 0,
            max_tile_x: 0,
            max_tile_z: 0,
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

    // @ObfuscatedName("fi.g(I)Lfo;") — ClientPlayer.getTempModel.
    // Verbatim port of ClientPlayer.java:152-206: resolve the avatar
    // model through PlayerModel with the entity's primary/secondary
    // seq state, stamp height from the bounding cylinder, stack the
    // spotanim (translated up by its height), merge the carried loc
    // model while its cycle window is live (rotated to the entity's
    // destination yaw), and flag AABB mouse picking.
    pub fn get_temp_model(&mut self, loop_cycle: i32) -> Option<crate::dash3d::model_lit::ModelLit> {
        use crate::dash3d::model_lit::ModelLit;
        if !self.model.applied {
            return None;
        }
        let primary = if self.entity.primary_seq_id != -1 && self.entity.primary_seq_delay == 0 {
            Some(crate::config::seq_type::list(self.entity.primary_seq_id))
        } else {
            None
        };
        let secondary = if self.entity.secondary_seq_id == -1
            || self.low_mem
            || (self.entity.secondary_seq_id == self.entity.readyanim && primary.is_some())
        {
            None
        } else {
            Some(crate::config::seq_type::list(self.entity.secondary_seq_id))
        };

        let mut model = self.model.get_temp_model(
            primary.as_ref(), self.entity.primary_seq_frame,
            secondary.as_ref(), self.entity.secondary_seq_frame)?;

        model.calc_bounding_cylinder();
        self.entity.height = model.min_y;

        if !self.low_mem
            && self.entity.spotanim_id != -1
            && self.entity.spotanim_frame != -1
        {
            let spot = crate::config::spot_type::list(self.entity.spotanim_id)
                .get_temp_model2(self.entity.spotanim_frame);
            if let Some(mut spot) = spot {
                spot.translate(0, -self.entity.spotanim_height, 0);
                model = ModelLit::merge(&[&model, &spot]);
            }
        }

        if !self.low_mem && self.loc_model.is_some() {
            if loop_cycle >= self.loc_end_cycle {
                self.loc_model = None;
            }
            if loop_cycle >= self.loc_start_cycle && loop_cycle < self.loc_end_cycle {
                if let Some(loc) = self.loc_model.as_mut() {
                    loc.translate(self.loc_offset_x - self.entity.x,
                                  self.loc_offset_y - self.y,
                                  self.loc_offset_z - self.entity.z);
                    if self.entity.dst_yaw == 512 {
                        loc.rotate90();
                        loc.rotate90();
                        loc.rotate90();
                    } else if self.entity.dst_yaw == 1024 {
                        loc.rotate90();
                        loc.rotate90();
                    } else if self.entity.dst_yaw == 1536 {
                        loc.rotate90();
                    }
                    model = ModelLit::merge(&[&model, loc]);
                    if self.entity.dst_yaw == 512 {
                        loc.rotate90();
                    } else if self.entity.dst_yaw == 1024 {
                        loc.rotate90();
                        loc.rotate90();
                    } else if self.entity.dst_yaw == 1536 {
                        loc.rotate90();
                        loc.rotate90();
                        loc.rotate90();
                    }
                    loc.translate(self.entity.x - self.loc_offset_x,
                                  self.y - self.loc_offset_y,
                                  self.entity.z - self.loc_offset_z);
                }
            }
        }

        model.use_aabb_mouse_check = true;
        Some(model)
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
