//! World entities. Movement + per-tick info-mask state shared by
//! players and NPCs lives in [`PathingEntity`]; the PLAYER_INFO /
//! NPC_INFO builders read the per-tick outputs and World resets them
//! at the end of each cycle.

pub mod npc;
pub mod player;

/// 8-way direction deltas, indexed by the 3-bit dir codes the rev1
/// client's ClientEntity.moveCode consumes (0=NW, 1=N, 2=NE, 3=W,
/// 4=E, 5=SW, 6=S, 7=SE).
pub const DIRECTION_DELTA: [(i32, i32); 8] = [
    (-1, 1),  // 0 NW
    (0, 1),   // 1 N
    (1, 1),   // 2 NE
    (-1, 0),  // 3 W
    (1, 0),   // 4 E
    (-1, -1), // 5 SW
    (0, -1),  // 6 S
    (1, -1),  // 7 SE
];

pub fn direction_from_delta(dx: i32, dz: i32) -> Option<usize> {
    DIRECTION_DELTA
        .iter()
        .position(|&(x, z)| x == dx.signum() && z == dz.signum() && (dx != 0 || dz != 0))
}

/// Entity spawn/despawn behaviour, mirroring Engine-TS `EntityLifeCycle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EntityLifeCycle {
    /// Never respawns or despawns — always in the world.
    Forever,
    /// Engine-spawned; respawns later after death (the default for map NPCs).
    #[default]
    Respawn,
    /// Script-spawned; despawns later.
    Despawn,
}

/// Per-tick movement rate, mirroring Engine-TS `MoveSpeed`. STATIONARY and
/// INSTANT take no walking steps (INSTANT = teleport-stepped); CRAWL advances
/// one tile every *other* tick; WALK one tile/tick; RUN two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MoveSpeed {
    Stationary,
    Crawl,
    #[default]
    Walk,
    Run,
    Instant,
}

/// Movement + transient update state shared by players and NPCs.
#[derive(Debug, Default, Clone)]
pub struct PathingEntity {
    /// Absolute world tile coords.
    pub x: i32,
    pub z: i32,
    pub level: i32,
    /// Packed id of the 8×8 zone this entity is currently registered in.
    pub zone_index: i32,

    /// Queued checkpoint waypoints (absolute tiles).
    pub waypoints: Vec<(i32, i32)>,
    pub move_speed: MoveSpeed,
    /// CRAWL advances on alternating ticks; this flips each tick.
    pub last_crawl: bool,

    // Per-tick movement outputs.
    pub walk_dir: i32,
    pub run_dir: i32,
    pub tele: bool,
    pub jump: bool,
    /// Steps walked this tick (1 for walk, 2 for a completed run) — read by
    /// the run-energy system, then reset before the next tick.
    pub steps_taken: i32,
    /// Tile occupied before the most recent step — the entity's facing
    /// orientation when it stops (Engine-TS `lastStepX`/`lastStepZ`).
    pub last_step_x: i32,
    pub last_step_z: i32,

    /// Extended-info mask bits queued this tick (protocol-level bits;
    /// player and npc use their own bit layouts).
    pub masks: i32,

    pub anim_id: i32,
    pub anim_delay: i32,

    pub chat: Option<String>,

    pub spotanim_id: i32,
    pub spotanim_height: i32,
    pub spotanim_delay: i32,

    pub face_entity: i32,
    pub face_x: i32,
    pub face_z: i32,
    /// Active interaction subject (Engine-TS `target`): the encoded id this
    /// entity is facing, or -1 when not interacting. While set, `face_entity`
    /// is sticky across ticks; once cleared, the next reset releases the face.
    pub target: i32,
    /// This entity's FACE_ENTITY info-mask bit — the player and npc info
    /// streams use different bit layouts, so each sets its own on construction
    /// (Engine-TS passes `entitymask` to the PathingEntity constructor).
    pub face_entity_mask: i32,

    /// Coord interaction target (Engine-TS `targetX`/`targetZ`): the tile of a
    /// *non-pathing* subject (a loc/obj/ground click) this entity walked to.
    /// Unlike `target` — which holds a player/npc id — this survives the
    /// interaction being cleared and is consumed by `reorient` once the entity
    /// runs out of steps, turning it to face what it walked up to. -1 = none.
    pub target_x: i32,
    pub target_z: i32,

    /// Forced-movement (Engine-TS `exactMove`) geometry: the visual glide runs
    /// from `(exact_start_x, exact_start_z)` to `(exact_end_x, exact_end_z)`
    /// (absolute tiles) between cycle deltas `exact_move_start`/`exact_move_end`,
    /// facing `exact_move_facing` (0=S,1=W,2=N,3=E). All -1 when idle.
    pub exact_start_x: i32,
    pub exact_start_z: i32,
    pub exact_end_x: i32,
    pub exact_end_z: i32,
    pub exact_move_start: i32,
    pub exact_move_end: i32,
    pub exact_move_facing: i32,

    /// Persistent server-side orientation (Engine-TS `faceAngleX`/`faceAngleZ`):
    /// the tile this entity is *looking at*. Updated on every step (it tracks
    /// the travel direction) and defaults to facing south. Unlike `face_x/z`
    /// (the one-shot FACE_COORD) this survives across ticks — it's what tells a
    /// freshly-observing client which way the entity is already turned.
    pub face_angle_x: i32,
    pub face_angle_z: i32,

    /// Position at the start of the current tick (Engine-TS `lastTickX`/
    /// `lastTickZ`), refreshed each `reset_transient`. Used to detect a move
    /// larger than a run, which must snap on the client instead of gliding.
    pub last_tick_x: i32,
    pub last_tick_z: i32,

    /// Tick on which this entity last took a step, stored as `currentTick + 1`
    /// (Engine-TS `lastMovement`, set in `updateMovement` when `stepsTaken > 0`).
    /// Read by P_ARRIVEDELAY / NPC_ARRIVEDELAY to wait for a step to settle.
    pub last_movement: i32,

    /// First hitsplat this tick (DAMAGE mask).
    pub damage_taken: i32,
    pub damage_type: i32,
    /// Second hitsplat this tick (DAMAGE2 mask) — OSRS shows up to two.
    pub damage_taken2: i32,
    pub damage_type2: i32,
    /// Alternates hitsplats between the two slots as hits land.
    pub hitmark_slot: i32,
    // Hitpoints live on the owning Npc/Player as `levels[HITPOINTS]` /
    // `base_levels[HITPOINTS]` (1:1 with Engine-TS, which keeps HP in the stat
    // array). The hitsplat HP bar in the info block reads those directly; the
    // entity only tracks the per-tick hitsplat slots above.
}

impl PathingEntity {
    pub fn at(x: i32, z: i32, level: i32) -> PathingEntity {
        PathingEntity {
            x,
            z,
            level,
            zone_index: crate::zone::zone_index(x, z, level),
            // Faces west initially, like Engine-TS (lastStep = x-1, z).
            last_step_x: x - 1,
            last_step_z: z,
            walk_dir: -1,
            run_dir: -1,
            anim_id: -1,
            anim_delay: -1,
            spotanim_id: -1,
            face_entity: -1,
            face_x: -1,
            face_z: -1,
            target: -1,
            target_x: -1,
            target_z: -1,
            exact_start_x: -1,
            exact_start_z: -1,
            exact_end_x: -1,
            exact_end_z: -1,
            exact_move_start: -1,
            exact_move_end: -1,
            exact_move_facing: -1,
            // Default orientation: facing south (the tile to the south), like
            // Engine-TS `unfocus`.
            face_angle_x: x,
            face_angle_z: z - 1,
            last_tick_x: x,
            last_tick_z: z,
            last_movement: 0,
            damage_taken: -1,
            damage_type: -1,
            damage_taken2: -1,
            damage_type2: -1,
            ..Default::default()
        }
    }

    pub fn teleport(&mut self, x: i32, z: i32, level: i32, jump: bool) {
        // Clamp to a valid plane (0..=3), 1:1 with Engine-TS `teleport`. Script
        // callers currently mask the level, but a computed plane could land
        // out of range and the client only has four map levels.
        let level = level.clamp(0, 3);
        // A change of plane forces a hard snap regardless of the caller — you
        // can't glide between levels. This is exactly Engine-TS, where plain
        // `teleport` sets jump+INSTANT only `if (previousLevel != level)` while
        // `teleJump` always passes jump=true: p_teleport → jump=false (snap only
        // on a level change), p_telejump → jump=true (always snap).
        let jump = jump || level != self.level;
        let prev_x = self.x;
        let prev_z = self.z;
        self.x = x;
        self.z = z;
        self.level = level;
        self.tele = true;
        self.jump = jump;
        self.move_speed = MoveSpeed::Instant;
        self.waypoints.clear();
        // Face one tile past the destination in the direction of travel — 1:1
        // with Engine-TS teleport, which does `focus(moveX, moveZ)` where the dir
        // is `CoordGrid.face(prev, dst)` and moveX/Z step one tile further that
        // way. The Direction enum matches DIRECTION_DELTA's indices exactly, so
        // `direction_from_delta(dst - prev)` is that same `face`. A same-tile
        // teleport (no direction) keeps the default south facing — Engine-TS's
        // `face` returns -1 there, yielding an undefined offset.
        match direction_from_delta(x - prev_x, z - prev_z) {
            Some(d) => {
                self.face_angle_x = x + DIRECTION_DELTA[d].0;
                self.face_angle_z = z + DIRECTION_DELTA[d].1;
            }
            None => {
                self.face_angle_x = x;
                self.face_angle_z = z - 1;
            }
        }
    }

    pub fn queue_waypoints(&mut self, route: &[(i32, i32)]) {
        self.waypoints = route.iter().copied().take(25).collect();
    }

    /// Advance one tick of movement per [`MoveSpeed`], producing the 3-bit
    /// dir codes the info packets carry. Mirrors Engine-TS
    /// `PathingEntity.processMovement`. Returns whether movement was processed.
    pub fn process_movement(&mut self) -> bool {
        self.walk_dir = -1;
        self.run_dir = -1;
        self.steps_taken = 0;

        if self.waypoints.is_empty()
            || self.move_speed == MoveSpeed::Stationary
            || self.move_speed == MoveSpeed::Instant
        {
            return false;
        }

        if self.move_speed == MoveSpeed::Crawl {
            // Crawl only advances on alternating ticks.
            self.last_crawl = !self.last_crawl;
            if self.last_crawl {
                self.walk_dir = self.validate_and_advance_step();
            }
        } else {
            self.walk_dir = self.validate_and_advance_step();
            if self.move_speed == MoveSpeed::Run && self.walk_dir != -1 {
                self.run_dir = self.validate_and_advance_step();
            }
        }
        true
    }

    /// Take one validated step toward the head waypoint, recording the
    /// pre-step tile as the facing orientation and bumping `steps_taken`.
    /// Engine-TS `validateAndAdvanceStep` (sans collision, which OS1 lacks).
    fn validate_and_advance_step(&mut self) -> i32 {
        while let Some(&(tx, tz)) = self.waypoints.first() {
            let dx = tx - self.x;
            let dz = tz - self.z;
            if dx == 0 && dz == 0 {
                self.waypoints.remove(0);
                continue;
            }

            let Some(dir) = direction_from_delta(dx, dz) else {
                self.waypoints.clear();
                return -1;
            };
            self.last_step_x = self.x;
            self.last_step_z = self.z;
            self.x += DIRECTION_DELTA[dir].0;
            self.z += DIRECTION_DELTA[dir].1;
            // Orient toward the next tile in our travel direction — Engine-TS
            // focuses one tile ahead after each step, so the entity faces the
            // way it's heading.
            self.face_angle_x = self.x + DIRECTION_DELTA[dir].0;
            self.face_angle_z = self.z + DIRECTION_DELTA[dir].1;
            self.steps_taken += 1;
            if self.x == tx && self.z == tz {
                self.waypoints.remove(0);
            }
            return dir as i32;
        }
        -1
    }

    /// Record one hitsplat for this tick's info block — the hitsplat half of
    /// Engine-TS `applyDamage`. The owning Npc/Player has already applied the HP
    /// loss to its stat levels and passes the `dealt` amount (capped at the
    /// remaining HP). Alternates between the two hitmark slots so OSRS can show
    /// up to two splats; the caller passes its own DAMAGE / DAMAGE2 mask bits.
    pub fn record_hit(&mut self, dealt: i32, dtype: i32, mask_damage: i32, mask_damage2: i32) {
        if self.hitmark_slot % 2 == 1 {
            self.damage_taken2 = dealt;
            self.damage_type2 = dtype;
            self.masks |= mask_damage2;
        } else {
            self.damage_taken = dealt;
            self.damage_type = dtype;
            self.masks |= mask_damage;
        }
        self.hitmark_slot += 1;
    }

    #[cfg(test)]
    fn route_to(&mut self, x: i32, z: i32, speed: MoveSpeed) {
        self.move_speed = speed;
        self.queue_waypoints(&[(x, z)]);
    }

    /// Anti-desync check — 1:1 with Engine-TS `validateDistanceWalked`. If this
    /// entity moved more than a run (>2 tiles, Chebyshev) since the tick began,
    /// flag a `jump` so the client snaps to the new tile instead of trying to
    /// glide a walk/run it can't represent. Callers skip it during an exact-move
    /// (that path already snaps).
    pub fn validate_distance_walked(&mut self) {
        let moved = (self.x - self.last_tick_x)
            .abs()
            .max((self.z - self.last_tick_z).abs());
        if moved > 2 {
            self.jump = true;
        }
    }

    /// Orient toward a tile without notifying the client (Engine-TS `focus`
    /// with `client=false`) — updates the persistent orientation only. The
    /// one-shot FACE_COORD (`face_x/z` + its mask) stays a separate path.
    pub fn face_toward(&mut self, tile_x: i32, tile_z: i32) {
        self.face_angle_x = tile_x;
        self.face_angle_z = tile_z;
    }

    /// Reset the persistent orientation to the default south — 1:1 with
    /// Engine-TS `unfocus` (`faceAngle = fine(x), fine(z - 1)`). Used on respawn,
    /// where the facing must snap south rather than follow `teleport`'s
    /// travel-direction focus.
    pub fn unfocus(&mut self) {
        self.face_angle_x = self.x;
        self.face_angle_z = self.z - 1;
    }

    /// The 3-bit direction code a new observer needs to render this entity at
    /// its current orientation — the index into the client's `ANGLE_TO_DIR`
    /// yaw table. Derived from the look vector (`face_angle` − position) via the
    /// shared 8-way table; defaults to south (6) when not oriented.
    pub fn orientation_dir(&self) -> i32 {
        direction_from_delta(self.face_angle_x - self.x, self.face_angle_z - self.z)
            .map_or(6, |d| d as i32)
    }

    /// Begin facing an interaction subject — `target_id` is the encoded
    /// faceEntity id (player slot + 32768, or an npc nid). Mirrors the facing
    /// block of Engine-TS `setInteraction`: the mask is flagged only when the
    /// faced entity actually changes, so a held interaction sends it just once.
    pub fn set_face_entity(&mut self, target_id: i32) {
        self.target = target_id;
        if self.face_entity != target_id {
            self.face_entity = target_id;
            self.masks |= self.face_entity_mask;
        }
    }

    /// Begin facing a non-pathing interaction subject at `(tile_x, tile_z)` —
    /// the `else` branch of Engine-TS `setInteraction` (a loc/obj/ground click).
    /// The subject has no entity id, so instead of `face_entity` we stash the
    /// tile in `target_x/target_z` (Engine-TS `targetX`/`targetZ`) and orient
    /// toward it now; `reorient` re-faces it once the walk to it completes.
    pub fn set_face_coord_target(&mut self, tile_x: i32, tile_z: i32) {
        self.target_x = tile_x;
        self.target_z = tile_z;
        self.face_toward(tile_x, tile_z);
    }

    /// End the current interaction (Engine-TS `clearInteraction`). The held
    /// face isn't dropped here — `reset_transient` releases it on the next tick
    /// so the client is told exactly once to stop facing. `target_x/target_z`
    /// are deliberately *not* cleared: Engine-TS `clearInteraction` leaves them
    /// for `reorient` to consume, so the entity still turns to face the tile it
    /// walked up to even after the interaction itself is over.
    pub fn clear_interaction(&mut self) {
        self.target = -1;
    }

    /// Force a non-walking movement — 1:1 with Engine-TS `exactMove`. The true
    /// tile jumps to the destination immediately (the glide is purely a visual
    /// replay on the client): `start`/`end` are cycle deltas, `facing` is the
    /// client direction code (0=S,1=W,2=N,3=E). `mask` is the caller's
    /// EXACT_MOVE info bit (player and npc streams differ).
    #[allow(clippy::too_many_arguments)]
    pub fn set_exact_move(
        &mut self,
        start_x: i32,
        start_z: i32,
        end_x: i32,
        end_z: i32,
        start: i32,
        end: i32,
        facing: i32,
        mask: i32,
    ) {
        self.exact_start_x = start_x;
        self.exact_start_z = start_z;
        self.exact_end_x = end_x;
        self.exact_end_z = end_z;
        self.exact_move_start = start;
        self.exact_move_end = end;
        self.exact_move_facing = facing;
        self.masks |= mask;
        // True tile snaps to the destination now; lastStep faces back west.
        self.x = end_x;
        self.z = end_z;
        self.last_step_x = self.x - 1;
        self.last_step_z = self.z;
        self.tele = true;
    }

    /// End-of-cycle transient reset (World calls this after the info
    /// packets have been built for every observer).
    pub fn reset_transient(&mut self) {
        self.masks = 0;
        self.walk_dir = -1;
        self.run_dir = -1;
        self.tele = false;
        self.jump = false;
        self.anim_id = -1;
        self.anim_delay = -1; // the "no animation" sentinel, 1:1 with Engine-TS
        self.chat = None;
        self.spotanim_id = -1;
        self.spotanim_height = 0;
        self.spotanim_delay = 0;
        // faceSquare (FACE_COORD) is a one-shot each tick.
        self.face_x = -1;
        self.face_z = -1;
        // faceEntity is sticky: while an interaction target is held it persists
        // (and its mask is sent only on change), so the client keeps facing.
        // Once the target is gone, release it here and re-flag the mask once so
        // the client stops facing — 1:1 with Engine-TS resetPathingEntity.
        if self.target == -1 && self.face_entity != -1 {
            self.masks |= self.face_entity_mask;
            self.face_entity = -1;
        }
        // Exact-move is a one-shot, like anim/spotanim (resetPathingEntity).
        self.exact_start_x = -1;
        self.exact_start_z = -1;
        self.exact_end_x = -1;
        self.exact_end_z = -1;
        self.exact_move_start = -1;
        self.exact_move_end = -1;
        self.exact_move_facing = -1;
        self.damage_taken = -1;
        self.damage_type = -1;
        self.damage_taken2 = -1;
        self.damage_type2 = -1;
        self.hitmark_slot = 0;
        // Snapshot where this tick ended — next tick measures movement from here.
        self.last_tick_x = self.x;
        self.last_tick_z = self.z;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn teleport_faces_the_travel_direction() {
        // East teleport → face one tile past the destination, eastward.
        let mut e = PathingEntity::at(3222, 3222, 0);
        e.teleport(3225, 3222, 0, false);
        assert_eq!((e.face_angle_x, e.face_angle_z), (3226, 3222), "faces one past the dest");
        assert_eq!(e.orientation_dir(), 4, "orientation resolves to east");

        // Diagonal (north-west) teleport.
        let mut e = PathingEntity::at(3222, 3222, 0);
        e.teleport(3220, 3224, 0, false);
        assert_eq!((e.face_angle_x, e.face_angle_z), (3219, 3225), "faces one past the dest, NW");
        assert_eq!(e.orientation_dir(), 0, "orientation resolves to north-west");

        // A same-tile teleport has no travel direction → keep the south default.
        let mut e = PathingEntity::at(3222, 3222, 0);
        e.teleport(3222, 3222, 0, false);
        assert_eq!((e.face_angle_x, e.face_angle_z), (3222, 3221), "same-tile keeps south");
        assert_eq!(e.orientation_dir(), 6);
    }

    #[test]
    fn walk_takes_one_tile_per_tick() {
        let mut e = PathingEntity::at(3200, 3200, 0);
        e.route_to(3203, 3200, MoveSpeed::Walk);
        assert!(e.process_movement());
        assert_eq!((e.x, e.z, e.steps_taken), (3201, 3200, 1));
        assert_eq!(e.walk_dir, 4); // east
        assert_eq!(e.run_dir, -1);
        assert_eq!((e.last_step_x, e.last_step_z), (3200, 3200));
    }

    #[test]
    fn run_takes_two_tiles_and_reports_run_dir() {
        let mut e = PathingEntity::at(3200, 3200, 0);
        e.route_to(3204, 3200, MoveSpeed::Run);
        e.process_movement();
        assert_eq!((e.x, e.steps_taken), (3202, 2));
        assert_ne!(e.walk_dir, -1);
        assert_ne!(e.run_dir, -1);
    }

    #[test]
    fn run_with_single_step_left_walks() {
        let mut e = PathingEntity::at(3200, 3200, 0);
        e.route_to(3201, 3200, MoveSpeed::Run);
        e.process_movement();
        assert_eq!((e.x, e.steps_taken), (3201, 1));
        assert_eq!(e.run_dir, -1, "no second step available -> walk");
    }

    #[test]
    fn crawl_advances_every_other_tick() {
        let mut e = PathingEntity::at(3200, 3200, 0);
        e.route_to(3205, 3200, MoveSpeed::Crawl);
        e.process_movement();
        assert_eq!(e.x, 3201, "first tick crawls");
        e.process_movement();
        assert_eq!(e.x, 3201, "second tick rests");
        e.process_movement();
        assert_eq!(e.x, 3202, "third tick crawls");
    }

    #[test]
    fn instant_and_stationary_take_no_steps() {
        let mut e = PathingEntity::at(3200, 3200, 0);
        e.route_to(3205, 3200, MoveSpeed::Instant);
        assert!(!e.process_movement());
        assert_eq!((e.x, e.steps_taken), (3200, 0));

        e.move_speed = MoveSpeed::Stationary;
        assert!(!e.process_movement());
        assert_eq!(e.x, 3200);
    }

    #[test]
    fn record_hit_alternates_hitmark_slots() {
        // HP clamping now lives on the Npc/Player (see those modules' tests);
        // the entity just files the dealt amount into alternating slots.
        let mut e = PathingEntity::at(3200, 3200, 0);
        const D: i32 = 0x400;
        const D2: i32 = 0x8;

        e.record_hit(3, 0, D, D2); // hit 1 -> slot 0 (DAMAGE)
        assert_eq!(e.damage_taken, 3);
        assert_ne!(e.masks & D, 0);

        e.record_hit(4, 1, D, D2); // hit 2 -> slot 1 (DAMAGE2)
        assert_eq!((e.damage_taken2, e.damage_type2), (4, 1));
        assert_ne!(e.masks & D2, 0);

        e.record_hit(5, 0, D, D2); // hit 3 -> back to slot 0
        assert_eq!(e.damage_taken, 5);
    }

    #[test]
    fn face_entity_is_sticky_until_interaction_ends() {
        const FACE_MASK: i32 = 0x20;
        let mut e = PathingEntity::at(3200, 3200, 0);
        e.face_entity_mask = FACE_MASK;

        // Start facing target 50: faced + mask flagged this tick.
        e.set_face_entity(50);
        assert_eq!(e.face_entity, 50);
        assert_ne!(e.masks & FACE_MASK, 0);

        // Across ticks while the interaction holds, the face persists and the
        // mask is NOT re-sent.
        e.reset_transient();
        assert_eq!(e.face_entity, 50, "face persists while interacting");
        assert_eq!(e.masks & FACE_MASK, 0, "mask only sent on change");
        e.set_face_entity(50); // re-asserting the same target is a no-op
        assert_eq!(e.masks & FACE_MASK, 0);

        // Ending the interaction releases the face on the next reset, flagging
        // the mask once so the client stops facing.
        e.clear_interaction();
        e.reset_transient();
        assert_eq!(e.face_entity, -1, "face released after interaction ends");
        assert_ne!(e.masks & FACE_MASK, 0, "client told to stop facing");

        // Steady state: nothing more to send.
        e.reset_transient();
        assert_eq!(e.masks & FACE_MASK, 0);
    }

    #[test]
    fn far_move_forces_a_snap() {
        let mut e = PathingEntity::at(3200, 3200, 0);
        e.last_tick_x = 3200;
        e.last_tick_z = 3200;

        // A diagonal run (2 tiles) still glides.
        e.x = 3202;
        e.z = 3202;
        e.jump = false;
        e.validate_distance_walked();
        assert!(!e.jump, "a 2-tile run does not snap");

        // A 5-tile jump (e.g. a scripted reposition) snaps.
        e.x = 3205;
        e.z = 3200;
        e.jump = false;
        e.validate_distance_walked();
        assert!(e.jump, "moving more than a run snaps the client");
    }

    #[test]
    fn orientation_tracks_travel_direction() {
        // Fresh entity faces south (6) — the OSRS default.
        let mut e = PathingEntity::at(3200, 3200, 0);
        assert_eq!(e.orientation_dir(), 6);

        // Walk east → faces east (DIRECTION_DELTA/ANGLE_TO_DIR index 4).
        e.route_to(3203, 3200, MoveSpeed::Walk);
        e.process_movement();
        assert_eq!(e.orientation_dir(), 4, "faces the way it walked (east)");

        // Then straight north from the new tile → faces north (1).
        let (cx, cz) = (e.x, e.z);
        e.route_to(cx, cz + 3, MoveSpeed::Walk);
        e.process_movement();
        assert_eq!(e.orientation_dir(), 1, "re-orients north");

        // Teleport re-faces toward the destination — here (3300,3300) is NE of
        // the current tile, so the entity faces north-east (1:1 with Engine-TS's
        // travel-direction focus, replacing the old south shortcut).
        e.teleport(3300, 3300, 0, false);
        assert_eq!(e.orientation_dir(), 2, "teleport faces the travel direction (north-east)");
    }

    #[test]
    fn exact_move_snaps_tile_and_is_one_shot() {
        const EXACT_MASK: i32 = 0x100;
        let mut e = PathingEntity::at(3200, 3200, 0);
        // Glide visually from (3200,3200) to (3203,3200) over 5 cycles, face east.
        e.set_exact_move(3200, 3200, 3203, 3200, 0, 5, 3, EXACT_MASK);

        // True tile jumps to the end immediately; the move is flagged + tele.
        assert_eq!((e.x, e.z), (3203, 3200), "true tile snaps to destination");
        assert!(e.tele);
        assert_ne!(e.masks & EXACT_MASK, 0);
        assert_eq!(e.exact_start_x, 3200);
        assert_eq!(e.exact_move_end, 5);
        assert_eq!(e.exact_move_facing, 3);

        // One-shot: cleared on the next reset.
        e.reset_transient();
        assert_eq!(e.exact_start_x, -1);
        assert_eq!(e.exact_move_end, -1);
        assert_eq!(e.exact_move_facing, -1);
        assert_eq!(e.masks & EXACT_MASK, 0);
    }

    #[test]
    fn teleport_sets_instant_and_clears_route() {
        let mut e = PathingEntity::at(3200, 3200, 0);
        e.route_to(3205, 3200, MoveSpeed::Walk);
        e.teleport(3300, 3300, 0, true);
        assert_eq!(e.move_speed, MoveSpeed::Instant);
        assert!(e.waypoints.is_empty());
        assert!(!e.process_movement());
    }

    #[test]
    fn teleport_clamps_level_to_valid_plane() {
        let mut e = PathingEntity::at(3200, 3200, 0);
        e.teleport(3300, 3300, 9, false); // out-of-range plane
        assert_eq!(e.level, 3, "clamped to the top map level");
        e.teleport(3300, 3300, -2, false);
        assert_eq!(e.level, 0, "clamped to ground level");
    }

    #[test]
    fn cross_plane_teleport_forces_a_snap_even_without_telejump() {
        let mut e = PathingEntity::at(3200, 3200, 0);
        // Same-plane p_teleport (jump=false): the client glides, no hard snap.
        e.teleport(3300, 3300, 0, false);
        assert!(!e.jump, "a same-plane p_teleport does not force a snap");
        // A p_teleport that changes plane must snap — you can't glide between
        // levels, so jump is forced on regardless of the caller.
        e.teleport(3300, 3300, 2, false);
        assert!(e.jump, "a cross-plane p_teleport snaps");
        // p_telejump always snaps, same plane or not.
        let mut e2 = PathingEntity::at(3200, 3200, 1);
        e2.teleport(3205, 3205, 1, true);
        assert!(e2.jump, "p_telejump always snaps");
    }
}
