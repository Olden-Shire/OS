// @ObfuscatedName("fz") — jag::oldscape::ClientEntity. Abstract base
// class in Java; both ClientPlayer (`fi`) and ClientNpc (`fa`) extend
// it. We expose it as a plain struct + impl, and the two concrete
// classes embed one as their first field. Rust doesn't have inheritance
// so callers reach `player.entity.x` rather than `player.x`, but the
// rest of the field semantics matches Java 1:1.

#![allow(dead_code)]

#[derive(Debug, Clone)]
pub struct ClientEntity {
    // @ObfuscatedName("fz.j") — current interpolated world-space X
    // (tile_x * 128 + size * 64 after teleport / route update).
    pub x: i32,
    // @ObfuscatedName("fz.z") — current interpolated world-space Z.
    pub z: i32,
    // @ObfuscatedName("fz.g") — render yaw (0..2047).
    pub yaw: i32,
    // @ObfuscatedName("fz.q") — extra forward-draw padding flag for
    // visible-edge clamp (used during the head-icon overlay pass).
    pub needs_forward_draw_padding: bool,
    // @ObfuscatedName("fz.i") — footprint side length in tiles. NPC
    // size comes from NpcType.size; ClientPlayer always 1.
    pub size: i32,
    // @ObfuscatedName("fz.s")
    pub readyanim: i32,
    // @ObfuscatedName("fz.u")
    pub turnleftanim: i32,
    // @ObfuscatedName("fz.v")
    pub turnrightanim: i32,
    // @ObfuscatedName("fz.w")
    pub walkanim: i32,
    // @ObfuscatedName("fz.e")
    pub walkanim_b: i32,
    // @ObfuscatedName("fz.b")
    pub walkanim_l: i32,
    // @ObfuscatedName("fz.y")
    pub walkanim_r: i32,
    // @ObfuscatedName("fz.t")
    pub runanim: i32,
    // @ObfuscatedName("fz.f")
    pub chat: Option<String>,
    // @ObfuscatedName("fz.k")
    pub chat_timer: i32,
    // @ObfuscatedName("fz.o")
    pub chat_colour: i32,
    // @ObfuscatedName("fz.a")
    pub chat_effect: i32,
    // @ObfuscatedName("fz.x")
    pub damage_values: [i32; 4],
    // @ObfuscatedName("fz.p")
    pub damage_types: [i32; 4],
    // @ObfuscatedName("fz.ad")
    pub damage_cycles: [i32; 4],
    // @ObfuscatedName("fz.ac")
    pub combat_cycle: i32,
    // @ObfuscatedName("fz.aa")
    pub health: i32,
    // @ObfuscatedName("fz.as")
    pub total_health: i32,
    // @ObfuscatedName("fz.am")
    pub target_id: i32,
    // @ObfuscatedName("fz.ap")
    pub target_tile_x: i32,
    // @ObfuscatedName("fz.av")
    pub target_tile_z: i32,
    // @ObfuscatedName("fz.ak")
    pub secondary_seq_id: i32,
    // @ObfuscatedName("fz.az")
    pub secondary_seq_frame: i32,
    // @ObfuscatedName("fz.an")
    pub secondary_seq_cycle: i32,
    // @ObfuscatedName("fz.ah")
    pub primary_seq_id: i32,
    // @ObfuscatedName("fz.ay")
    pub primary_seq_frame: i32,
    // @ObfuscatedName("fz.al")
    pub primary_seq_cycle: i32,
    // @ObfuscatedName("fz.ab")
    pub primary_seq_delay: i32,
    // @ObfuscatedName("fz.ao")
    pub primary_seq_loop: i32,
    // @ObfuscatedName("fz.ag")
    pub spotanim_id: i32,
    // @ObfuscatedName("fz.ar")
    pub spotanim_frame: i32,
    // @ObfuscatedName("fz.aq")
    pub spotanim_cycle: i32,
    // @ObfuscatedName("fz.at")
    pub spotanim_last_cycle: i32,
    // @ObfuscatedName("fz.ae")
    pub spotanim_height: i32,
    // @ObfuscatedName("fz.au")
    pub exact_start_x: i32,
    // @ObfuscatedName("fz.ax")
    pub exact_end_x: i32,
    // @ObfuscatedName("fz.ai")
    pub exact_start_z: i32,
    // @ObfuscatedName("fz.aj")
    pub exact_end_z: i32,
    // @ObfuscatedName("fz.aw")
    pub exact_move_end: i32,
    // @ObfuscatedName("fz.af")
    pub exact_move_start: i32,
    // @ObfuscatedName("fz.bh")
    pub exact_move_facing: i32,
    // @ObfuscatedName("fz.bi")
    pub cycle: i32,
    // @ObfuscatedName("fz.bs") — render height (200 default).
    pub height: i32,
    // @ObfuscatedName("fz.bk") — target yaw, lerps toward by turnspeed.
    pub dst_yaw: i32,
    // @ObfuscatedName("fz.bv")
    pub turn_cycle: i32,
    // @ObfuscatedName("fz.bg") — yaw lerp speed (32 default).
    pub turnspeed: i32,
    // @ObfuscatedName("fz.bl")
    pub route_length: i32,
    // @ObfuscatedName("fz.bt")
    pub route_x: [i32; 10],
    // @ObfuscatedName("fz.bw")
    pub route_z: [i32; 10],
    // @ObfuscatedName("fz.by")
    pub route_run: [bool; 10],
    // @ObfuscatedName("fz.bx")
    pub anim_delay_move: i32,
    // @ObfuscatedName("fz.bf")
    pub preanim_route_length: i32,
}

impl Default for ClientEntity {
    fn default() -> Self {
        Self {
            x: 0, z: 0, yaw: 0,
            needs_forward_draw_padding: false,
            size: 1,
            readyanim: -1, turnleftanim: -1, turnrightanim: -1,
            walkanim: -1, walkanim_b: -1, walkanim_l: -1, walkanim_r: -1,
            runanim: -1,
            chat: None, chat_timer: 100, chat_colour: 0, chat_effect: 0,
            damage_values: [0; 4], damage_types: [0; 4], damage_cycles: [0; 4],
            combat_cycle: -1000, health: 0, total_health: 0,
            target_id: -1, target_tile_x: 0, target_tile_z: 0,
            secondary_seq_id: -1, secondary_seq_frame: 0, secondary_seq_cycle: 0,
            primary_seq_id: -1, primary_seq_frame: 0, primary_seq_cycle: 0,
            primary_seq_delay: 0, primary_seq_loop: 0,
            spotanim_id: -1, spotanim_frame: 0, spotanim_cycle: 0,
            spotanim_last_cycle: 0, spotanim_height: 0,
            exact_start_x: 0, exact_end_x: 0, exact_start_z: 0, exact_end_z: 0,
            exact_move_end: 0, exact_move_start: 0, exact_move_facing: 0,
            cycle: 0, height: 200,
            dst_yaw: 0, turn_cycle: 0, turnspeed: 32,
            route_length: 0, route_x: [0; 10], route_z: [0; 10], route_run: [false; 10],
            anim_delay_move: 0, preanim_route_length: 0,
        }
    }
}

impl ClientEntity {
    // @ObfuscatedName("fz.f(I)Z") — ClientEntity.ready. Verbatim port
    // of ClientEntity.java:271-274. Base default returns false; the
    // ClientNpc and ClientPlayer subclasses override (NpcType.list-
    // backed for npc, model-population-backed for player). Used by
    // mixed-entity iterators that hold a generic &ClientEntity ref.
    pub fn ready(&self) -> bool { false }

    // @ObfuscatedName("fz.b(IIZB)V") — ClientEntity.teleport. Verbatim
    // port of ClientEntity.java:183-212.
    //
    // For small-delta moves (within ±8 tiles) and non-`jump` calls,
    // shift the route history forward and push the new (x, z) on the
    // front so the renderer can interpolate. For jumps or larger
    // distances, clear the route and snap to the new tile.
    pub fn teleport(&mut self, tile_x: i32, tile_z: i32, jump: bool) {
        // Java consults SeqType.list(primarySeqId).postanim_move at
        // ClientEntity.java:184 — when the active primary anim has the
        // post-anim-move flag set (postanim_move == 1), teleporting
        // mid-sequence cancels the anim. Mirrors player-driven walk
        // chains where the server interrupts an attack with a
        // forced-move (knockback / teleport tile).
        if self.primary_seq_id != -1 {
            use crate::config::seq_type;
            let seq = seq_type::list(self.primary_seq_id);
            if seq.postanim_move == 1 {
                self.primary_seq_id = -1;
            }
        }
        if !jump {
            let dx = tile_x - self.route_x[0];
            let dz = tile_z - self.route_z[0];
            if (-8..=8).contains(&dx) && (-8..=8).contains(&dz) {
                if self.route_length < 9 {
                    self.route_length += 1;
                }
                let mut i = self.route_length as usize;
                while i > 0 {
                    self.route_x[i] = self.route_x[i - 1];
                    self.route_z[i] = self.route_z[i - 1];
                    self.route_run[i] = self.route_run[i - 1];
                    i -= 1;
                }
                self.route_x[0] = tile_x;
                self.route_z[0] = tile_z;
                self.route_run[0] = false;
                return;
            }
        }
        self.route_length = 0;
        self.preanim_route_length = 0;
        self.anim_delay_move = 0;
        self.route_x[0] = tile_x;
        self.route_z[0] = tile_z;
        // Java line 210-211: x = tile_x * 128 + size * 64.
        self.x = tile_x * 128 + self.size * 64;
        self.z = tile_z * 128 + self.size * 64;
    }

    // @ObfuscatedName("fz.y(IZI)V") — ClientEntity.moveCode. Verbatim
    // port of ClientEntity.java:216-261. `direction` is 0..7 with the
    // OSRS standard 8-way encoding (NW=0, N=1, NE=2, W=3, E=4, SW=5,
    // S=6, SE=7). The new tile is pushed on the route queue; the
    // renderer interpolates toward it next tick.
    pub fn move_code(&mut self, direction: i32, run_step: bool) {
        // Same primarySeq.postanim_move check as teleport — a
        // move-step also cancels a flagged primary anim.
        if self.primary_seq_id != -1 {
            use crate::config::seq_type;
            let seq = seq_type::list(self.primary_seq_id);
            if seq.postanim_move == 1 {
                self.primary_seq_id = -1;
            }
        }
        let mut tx = self.route_x[0];
        let mut tz = self.route_z[0];
        match direction {
            0 => { tx -= 1; tz += 1; }
            1 => { tz += 1; }
            2 => { tx += 1; tz += 1; }
            3 => { tx -= 1; }
            4 => { tx += 1; }
            5 => { tx -= 1; tz -= 1; }
            6 => { tz -= 1; }
            7 => { tx += 1; tz -= 1; }
            _ => {}
        }
        if self.route_length < 9 {
            self.route_length += 1;
        }
        let mut i = self.route_length as usize;
        while i > 0 {
            self.route_x[i] = self.route_x[i - 1];
            self.route_z[i] = self.route_z[i - 1];
            self.route_run[i] = self.route_run[i - 1];
            i -= 1;
        }
        self.route_x[0] = tx;
        self.route_z[0] = tz;
        self.route_run[0] = run_step;
    }

    // @ObfuscatedName("fz.t(I)V") — ClientEntity.abortRoute.
    pub fn abort_route(&mut self) {
        self.route_length = 0;
        self.preanim_route_length = 0;
    }

    // @ObfuscatedName("fz.k(IIIB)V") — ClientEntity.addHitmark.
    // Inserts a new (value, type, cycle) tuple into the first
    // damage slot whose existing cycle has passed.
    pub fn add_hitmark(&mut self, value: i32, kind: i32, current_cycle: i32) {
        for i in 0..4 {
            if self.damage_cycles[i] <= current_cycle {
                self.damage_values[i] = value;
                self.damage_types[i] = kind;
                self.damage_cycles[i] = current_cycle + 70;
                return;
            }
        }
    }
}
