//! Server-side NPC. Mask bits match the rev1 client's
//! getNpcPosExtended flags.

use crate::entity::{EntityLifeCycle, MoveSpeed, PathingEntity};
use crate::script::trigger::Trigger;

/// A queued npc AI script awaiting its delay (Engine-TS `NpcQueueRequest`): the
/// [ai_queue<N>] `trigger` runs after `delay` ticks (counted down only while the
/// npc isn't action-locked), with `arg` exposed to it as the active value.
#[derive(Clone)]
pub struct NpcQueueRequest {
    pub trigger: Trigger,
    pub delay: i32,
    pub arg: i32,
}

pub const MASK_SPOTANIM: i32 = 0x1;
pub const MASK_FACE_COORD: i32 = 0x2;
pub const MASK_FACE_ENTITY: i32 = 0x4;
pub const MASK_ANIM: i32 = 0x8;
pub const MASK_DAMAGE2: i32 = 0x10;
pub const MASK_SAY: i32 = 0x20;
pub const MASK_CHANGE_TYPE: i32 = 0x40;
pub const MASK_DAMAGE: i32 = 0x80;

/// NpcStat indices (Engine-TS `NpcStat`) — six combat stats.
pub const NPC_STAT_STRENGTH: usize = 2;
pub const NPC_STAT_HITPOINTS: usize = 3;
pub const NPC_STAT_COUNT: usize = 6;
/// Default regen cadence (ticks) until the npc config (`regenrate`) loads.
pub const DEFAULT_REGEN_INTERVAL: i32 = 100;
/// Default respawn delay (ticks) until the npc config (`respawnrate`) loads.
pub const DEFAULT_RESPAWN_TIME: i32 = 100;
/// Disabled lifecycle countdown.
const LIFECYCLE_IDLE: i32 = -1;
/// Max distinct damage-dealers tracked for kill/loot ownership.
const MAX_HEROES: usize = 16;

/// What `Npc::process_lifecycle` did this tick, so the world can fire the
/// matching [ai_spawn] / [ai_despawn] trigger (Engine-TS npcEventQueue).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleEvent {
    None,
    Respawned,
    Despawned,
}

pub struct Npc {
    pub nid: usize,
    pub type_id: i32,
    pub entity: PathingEntity,
    /// Queued retype (npc_changetype) — sent via MASK_CHANGE_TYPE.
    pub new_type: i32,
    pub active: bool,

    /// Base (spawn) combat levels from the npc config; all 1 until it loads.
    pub base_levels: [i32; NPC_STAT_COUNT],
    /// Current combat levels — drift back toward `base_levels` via regen.
    pub levels: [i32; NPC_STAT_COUNT],
    /// Ticks between regen pulses + the counter toward the next one.
    pub regen_interval: i32,
    pub regen_clock: i32,

    /// Spawn/despawn behaviour and the countdown to the next lifecycle event
    /// (-1 = idle). RESPAWN map NPCs respawn at `spawn_*` after death.
    pub lifecycle: EntityLifeCycle,
    pub lifecycle_tick: i32,
    pub spawn_x: i32,
    pub spawn_z: i32,
    pub spawn_level: i32,
    pub respawn_time: i32,

    /// Damage-dealer tally for kill/loot ownership (Engine-TS `HeroPoints`):
    /// `(player pid, total damage)`. Whoever tops it wins the kill/drop.
    /// Cleared whenever stats reset (respawn).
    pub hero_points: Vec<(usize, i32)>,

    /// World tick until which the npc is action-locked (Engine-TS `delayedUntil`),
    /// set by NPC_DELAY. The suspended AI script resumes once it elapses.
    pub delayed_until: i32,

    /// AI timer (Engine-TS `timerInterval`/`timerClock`): fires the npc's
    /// [ai_timer] trigger every `timer_interval` ticks. 0 = no timer.
    pub timer_interval: i32,
    pub timer_clock: i32,

    /// Queued AI scripts (Engine-TS `Npc.queue`), set by NPC_QUEUE — processed
    /// each cycle, frozen while the npc is action-locked.
    pub queue: Vec<NpcQueueRequest>,

    /// Pending walk-trigger (Engine-TS `walktrigger`/`walktriggerArg`): an
    /// [ai_queue<N>] queue id (0-based) that fires when the npc next walks, set
    /// by NPC_WALKTRIGGER. -1 = none.
    pub walk_trigger: i32,
    pub walktrigger_arg: i32,
    /// Current AI mode (Engine-TS `targetOp` / NpcMode) — read by NPC_GETMODE,
    /// 0 = NONE, set by NPC_SETMODE. Drives the npc AI turn (chase/face/escape).
    pub mode: i32,
    /// The npc's current AI target (Engine-TS `target`), set by NPC_SETMODE.
    /// Reuses the player interaction target enum.
    pub target: Option<crate::entity::player::InteractTarget>,
    /// Hunt search radius (Engine-TS `huntrange`) — set by NPC_SETHUNT.
    pub hunt_range: i32,
    /// Hunt type id (Engine-TS `huntMode`, -1 = none) — set by NPC_SETHUNTMODE.
    pub hunt_mode: i32,
    /// Ticks since the wandering npc was last at its spawn (Engine-TS
    /// `wanderCounter`): at 500 it teleports home. Reset by combat (aiMode).
    pub wander_counter: i32,
}

impl Npc {
    pub fn new(nid: usize, type_id: i32, x: i32, z: i32, level: i32) -> Npc {
        let mut entity = PathingEntity::at(x, z, level);
        entity.face_entity_mask = MASK_FACE_ENTITY;
        Npc {
            nid,
            type_id,
            entity,
            new_type: -1,
            active: true,
            base_levels: [1; NPC_STAT_COUNT],
            levels: [1; NPC_STAT_COUNT],
            regen_interval: DEFAULT_REGEN_INTERVAL,
            regen_clock: 0,
            lifecycle: EntityLifeCycle::Respawn,
            lifecycle_tick: LIFECYCLE_IDLE,
            spawn_x: x,
            spawn_z: z,
            spawn_level: level,
            respawn_time: DEFAULT_RESPAWN_TIME,
            hero_points: Vec::new(),
            delayed_until: -1,
            timer_interval: 0,
            timer_clock: 0,
            queue: Vec::new(),
            walk_trigger: -1,
            walktrigger_arg: 0,
            mode: 0,
            target: None,
            hunt_range: -1,
            hunt_mode: -1,
            wander_counter: 0,
        }
    }

    /// Credit `points` damage to player `pid` for kill/loot ownership — 1:1 with
    /// Engine-TS `HeroPoints.addHero`. Accumulates onto an existing dealer, else
    /// records a new one up to [`MAX_HEROES`]; sub-1 hits are ignored.
    pub fn add_hero(&mut self, pid: usize, points: i32) {
        if points < 1 {
            return;
        }
        if let Some(hero) = self.hero_points.iter_mut().find(|(p, _)| *p == pid) {
            hero.1 += points;
        } else if self.hero_points.len() < MAX_HEROES {
            self.hero_points.push((pid, points));
        }
    }

    /// The player who has dealt the most damage (Engine-TS `HeroPoints.findHero`),
    /// i.e. who earns the kill and private loot; `None` if nobody qualifies.
    pub fn find_hero(&self) -> Option<usize> {
        self.hero_points
            .iter()
            .max_by_key(|(_, points)| *points)
            .map(|(pid, _)| *pid)
    }

    /// Arm the lifecycle countdown (Engine-TS `setLifeCycle`).
    pub fn set_lifecycle(&mut self, duration: i32) {
        self.lifecycle_tick = duration;
    }

    /// Set the AI-timer interval (Engine-TS `Npc.setTimer`): -1 leaves it
    /// unchanged, any other value (re)arms the [ai_timer] firing cadence.
    pub fn set_timer(&mut self, interval: i32) {
        if interval != -1 {
            self.timer_interval = interval;
        }
    }

    /// Queue an [ai_queue<N>] script to run after `delay` ticks (Engine-TS
    /// `Npc.enqueueScript`); `arg` is exposed to it as the active value.
    pub fn enqueue_script(&mut self, trigger: Trigger, delay: i32, arg: i32) {
        self.queue.push(NpcQueueRequest { trigger, delay, arg });
    }

    /// Whether the npc is action-locked at `tick` (Engine-TS `delayed`).
    pub fn is_delayed(&self, tick: i32) -> bool {
        tick < self.delayed_until
    }

    /// Kill this NPC: deactivate and (if it's a respawning map NPC) start the
    /// respawn countdown.
    pub fn kill(&mut self) {
        self.active = false;
        if self.lifecycle == EntityLifeCycle::Respawn {
            self.set_lifecycle(self.respawn_time);
        }
    }

    /// Re-spawn at the original tile with full stats.
    fn respawn(&mut self) {
        self.active = true;
        self.new_type = -1;
        self.levels = self.base_levels; // restores HP (= levels[HITPOINTS]) too
        self.entity.teleport(self.spawn_x, self.spawn_z, self.spawn_level, false);
        // Engine-TS resetEntity(respawn) calls unfocus() → face south, regardless
        // of where the npc died; teleport's travel-direction focus doesn't apply
        // to a respawn.
        self.entity.unfocus();
        self.hero_points.clear(); // stats recovered -> drop kill credit
        self.queue.clear(); // drop any pre-death queued [ai_queue] scripts
        self.lifecycle_tick = LIFECYCLE_IDLE;
    }

    /// Per-tick lifecycle countdown — 1:1 with Engine-TS `Npc` respawn/despawn
    /// handling. When the countdown elapses: a dead RESPAWN npc respawns (an
    /// active one reverts a pending changetype); a DESPAWN npc deactivates.
    pub fn process_lifecycle(&mut self) -> LifecycleEvent {
        if self.lifecycle_tick <= 0 {
            return LifecycleEvent::None; // idle / not counting
        }
        self.lifecycle_tick -= 1;
        if self.lifecycle_tick != 0 {
            return LifecycleEvent::None;
        }
        match self.lifecycle {
            EntityLifeCycle::Respawn => {
                if !self.active {
                    self.respawn();
                    LifecycleEvent::Respawned
                } else if self.new_type != -1 {
                    self.entity.masks |= MASK_CHANGE_TYPE; // revert queued
                    self.lifecycle_tick = LIFECYCLE_IDLE;
                    LifecycleEvent::None
                } else {
                    self.lifecycle_tick = LIFECYCLE_IDLE;
                    LifecycleEvent::None
                }
            }
            EntityLifeCycle::Despawn => {
                self.active = false;
                LifecycleEvent::Despawned
            }
            EntityLifeCycle::Forever => {
                self.lifecycle_tick = LIFECYCLE_IDLE;
                LifecycleEvent::None
            }
        }
    }

    pub fn change_type(&mut self, type_id: i32) {
        self.new_type = type_id;
        self.entity.masks |= MASK_CHANGE_TYPE;
    }

    /// Per-tick NPC movement — 1:1 with Engine-TS `Npc.updateMovement`. Resets
    /// the move speed to the NPC default (WALK) unless mid-teleport, advances
    /// along any queued waypoints, and reports whether the NPC changed tile this
    /// tick. Without this per-tick reset a one-off INSTANT (teleport) would
    /// wedge the NPC as non-walking forever, since `process_movement` skips
    /// INSTANT. (The NOMOVE move-restrict gate and the AI walktrigger script
    /// hook need NPC config / AI scripts and are deferred until those land.)
    pub fn update_movement(&mut self) -> bool {
        if self.entity.move_speed != MoveSpeed::Instant {
            self.entity.move_speed = MoveSpeed::Walk;
        }
        self.entity.process_movement();
        self.entity.x != self.entity.last_tick_x || self.entity.z != self.entity.last_tick_z
    }

    /// End-of-cycle reset — the NPC half of Engine-TS `resetPathingEntity`.
    /// Clears the shared transient state, then restores the move speed to the
    /// NPC default (WALK, `defaultMoveSpeed`). This is what releases a one-tick
    /// INSTANT (teleport) so the NPC resumes walking on the following tick.
    pub fn reset_transient(&mut self) {
        self.entity.reset_transient();
        self.entity.move_speed = MoveSpeed::Walk;
    }

    /// NPC_STATADD — raise the current level by `constant + base*percent/100`,
    /// capped at 255. 1:1 with Engine-TS NPC_STATADD.
    pub fn stat_add(&mut self, stat: usize, constant: i32, percent: i32) {
        let (base, current) = (self.base_levels[stat], self.levels[stat]);
        self.levels[stat] = (current + (constant + base * percent / 100)).min(255);
    }

    /// NPC_STATSUB — lower the current level by `constant + base*percent/100`,
    /// floored at 0. 1:1 with Engine-TS NPC_STATSUB.
    pub fn stat_sub(&mut self, stat: usize, constant: i32, percent: i32) {
        let (base, current) = (self.base_levels[stat], self.levels[stat]);
        self.levels[stat] = (current - (constant + base * percent / 100)).max(0);
    }

    /// NPC_STATHEAL — restore toward base by `constant + base*percent/100`,
    /// capped at base. Unlike the player heal this has no `max(.., current)`
    /// floor, so it pulls a boosted stat back down to base. Clears the hero
    /// tally once HITPOINTS reaches base — 1:1 with Engine-TS NPC_STATHEAL.
    pub fn stat_heal(&mut self, stat: usize, constant: i32, percent: i32) {
        let (base, current) = (self.base_levels[stat], self.levels[stat]);
        self.levels[stat] = (current + (constant + base * percent / 100)).min(base);
        if stat == NPC_STAT_HITPOINTS && self.levels[stat] >= self.base_levels[stat] {
            self.hero_points.clear();
        }
    }

    /// Per-tick stat regen — 1:1 with Engine-TS `Npc.processRegen`. Every
    /// `regen_interval` ticks each current stat steps one toward its base
    /// (recovering drained stats, decaying boosted ones).
    pub fn process_regen(&mut self) {
        self.regen_clock += 1;
        if self.regen_clock < self.regen_interval {
            return;
        }
        // (When npc config loads, reload regen_interval from the type here —
        // Engine-TS reloads it only on a regen pulse.)
        self.regen_clock = 0;
        for i in 0..NPC_STAT_COUNT {
            if self.levels[i] < self.base_levels[i] {
                self.levels[i] += 1;
            } else if self.levels[i] > self.base_levels[i] {
                self.levels[i] -= 1;
            }
        }
    }

    /// Current hitpoints (NpcStat.HITPOINTS).
    pub fn hitpoints(&self) -> i32 {
        self.levels[NPC_STAT_HITPOINTS]
    }

    /// Take a hit — 1:1 with Engine-TS `Npc.applyDamage`. Subtracts from HP
    /// (`levels[HITPOINTS]`, clamped at 0), files the hitsplat (capped at the
    /// remaining HP) under this NPC's DAMAGE/DAMAGE2 masks, and credits `source`
    /// (the attacker) the *dealt* amount for kill/loot ownership. Pass `None`
    /// for sourceless damage (poison, scripted).
    pub fn apply_damage(&mut self, damage: i32, dtype: i32, source: Option<usize>) {
        let current = self.levels[NPC_STAT_HITPOINTS];
        let dealt = if current - damage <= 0 {
            self.levels[NPC_STAT_HITPOINTS] = 0;
            current
        } else {
            self.levels[NPC_STAT_HITPOINTS] = current - damage;
            damage
        };
        self.entity.record_hit(dealt, dtype, MASK_DAMAGE, MASK_DAMAGE2);
        if let Some(pid) = source {
            self.add_hero(pid, dealt);
        }
    }

    /// Whether this NPC has been reduced to 0 hitpoints.
    pub fn is_dead(&self) -> bool {
        self.levels[NPC_STAT_HITPOINTS] <= 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn respawn_faces_south_and_clears_queue() {
        // Spawns at (3222,3222); wandered east to (3230,3222) and "died" there
        // with a queued script still pending.
        let mut n = Npc::new(0, 1, 3222, 3222, 0);
        n.entity.x = 3230;
        n.entity.z = 3222;
        n.active = false;
        n.queue.push(NpcQueueRequest { trigger: 0, delay: 1, arg: 0 });
        n.lifecycle = EntityLifeCycle::Respawn;
        n.lifecycle_tick = 1; // elapses → respawn fires this call

        n.process_lifecycle();

        assert!(n.active, "respawned");
        assert_eq!((n.entity.x, n.entity.z), (3222, 3222), "snaps back to the spawn tile");
        assert_eq!(n.entity.orientation_dir(), 6,
            "faces south via unfocus, not the death→spawn travel direction");
        assert!(n.queue.is_empty(), "pre-death queue is cleared on respawn");
    }

    #[test]
    fn regen_steps_toward_base_on_interval() {
        let mut n = Npc::new(0, 1, 3200, 3200, 0);
        n.base_levels[NPC_STAT_HITPOINTS] = 10;
        n.levels[NPC_STAT_HITPOINTS] = 4; // drained
        n.regen_interval = 3;

        n.process_regen();
        n.process_regen();
        assert_eq!(n.hitpoints(), 4, "no regen before the interval elapses");
        n.process_regen();
        assert_eq!(n.hitpoints(), 5, "+1 on the interval tick");
        assert_eq!(n.regen_clock, 0, "clock resets after a pulse");
    }

    #[test]
    fn update_movement_walks_queued_waypoints() {
        let mut n = Npc::new(0, 1, 3200, 3200, 0);
        n.entity.route_to(3202, 3200, MoveSpeed::Walk);
        // Pretend a prior tick ended here so `moved` measures this step.
        n.entity.last_tick_x = n.entity.x;
        n.entity.last_tick_z = n.entity.z;
        assert!(n.update_movement(), "stepped toward the waypoint");
        assert_eq!((n.entity.x, n.entity.z), (3201, 3200), "advanced one tile east");
    }

    #[test]
    fn instant_is_preserved_in_tick_then_cleared_by_reset() {
        // Engine-TS semantics: updateMovement keeps INSTANT (so the teleport
        // tick takes no walking step), and resetPathingEntity clears it back to
        // WALK at cycle end — without that reset the NPC would never walk again.
        let mut n = Npc::new(0, 1, 3200, 3200, 0);
        n.entity.route_to(3202, 3200, MoveSpeed::Walk); // queue the path…
        n.entity.move_speed = MoveSpeed::Instant; // …then a teleport overrides the speed
        n.entity.last_tick_x = n.entity.x;
        n.entity.last_tick_z = n.entity.z;

        assert!(!n.update_movement(), "INSTANT tick takes no step");
        assert_eq!(n.entity.move_speed, MoveSpeed::Instant,
                   "updateMovement preserves INSTANT during its tick");

        n.reset_transient(); // end of cycle
        assert_eq!(n.entity.move_speed, MoveSpeed::Walk,
                   "reset restores the WALK default");

        // Next tick: now WALK, the queued route advances.
        n.entity.last_tick_x = n.entity.x;
        n.entity.last_tick_z = n.entity.z;
        assert!(n.update_movement(), "walks now that INSTANT cleared");
        assert_eq!((n.entity.x, n.entity.z), (3201, 3200));
    }

    #[test]
    fn update_movement_resets_a_stale_run_to_walk() {
        // A non-INSTANT speed left over from a prior tick is reset to the NPC
        // default each tick (Engine-TS Npc.updateMovement).
        let mut n = Npc::new(0, 1, 3200, 3200, 0);
        n.entity.move_speed = MoveSpeed::Run;
        n.update_movement();
        assert_eq!(n.entity.move_speed, MoveSpeed::Walk);
    }

    #[test]
    fn dead_respawn_npc_returns_after_its_timer() {
        let mut n = Npc::new(0, 1, 3200, 3200, 0);
        n.base_levels[NPC_STAT_HITPOINTS] = 8;
        n.levels[NPC_STAT_HITPOINTS] = 8;
        n.respawn_time = 3;

        n.entity.x = 3210; // wandered off, then killed
        n.kill();
        assert!(!n.active);
        assert_eq!(n.lifecycle_tick, 3);

        n.process_lifecycle();
        n.process_lifecycle();
        assert!(!n.active, "still dead before the timer elapses");
        n.process_lifecycle();
        assert!(n.active, "respawned on the timer tick");
        assert_eq!((n.entity.x, n.entity.z), (3200, 3200), "back at spawn tile");
        assert_eq!(n.hitpoints(), 8, "full HP on respawn");
        assert_eq!(n.lifecycle_tick, -1, "countdown idle again");
    }

    #[test]
    fn idle_lifecycle_never_fires() {
        let mut n = Npc::new(0, 1, 3200, 3200, 0);
        for _ in 0..1000 {
            n.process_lifecycle();
        }
        assert!(n.active);
        assert_eq!(n.lifecycle_tick, -1);
    }

    #[test]
    fn hero_points_track_top_damage_dealer() {
        let mut n = Npc::new(0, 1, 3200, 3200, 0);
        n.base_levels[NPC_STAT_HITPOINTS] = 30;
        n.levels[NPC_STAT_HITPOINTS] = 30;

        // Two players whittle it down; player 5 deals the most overall.
        n.apply_damage(8, 0, Some(5));
        n.apply_damage(10, 0, Some(7));
        n.apply_damage(9, 0, Some(5)); // 5 now on 17 vs 7's 10
        assert_eq!(n.find_hero(), Some(5), "highest cumulative damage wins the kill");

        // Sourceless damage (poison) credits nobody.
        n.apply_damage(3, 0, None);
        assert_eq!(n.hero_points.len(), 2);
    }

    #[test]
    fn respawn_clears_kill_credit() {
        let mut n = Npc::new(0, 1, 3200, 3200, 0);
        n.base_levels[NPC_STAT_HITPOINTS] = 10;
        n.levels[NPC_STAT_HITPOINTS] = 10;
        n.respawn_time = 1;
        n.apply_damage(10, 0, Some(2));
        assert_eq!(n.find_hero(), Some(2));

        n.kill();
        n.process_lifecycle(); // respawns
        assert!(n.active);
        assert_eq!(n.find_hero(), None, "kill credit reset on respawn");
    }

    #[test]
    fn regen_decays_boosted_stats() {
        let mut n = Npc::new(0, 1, 3200, 3200, 0);
        n.base_levels[NPC_STAT_STRENGTH] = 5;
        n.levels[NPC_STAT_STRENGTH] = 9; // boosted
        n.regen_interval = 1;
        n.process_regen();
        assert_eq!(n.levels[NPC_STAT_STRENGTH], 8, "boosted stat decays toward base");
    }
}
