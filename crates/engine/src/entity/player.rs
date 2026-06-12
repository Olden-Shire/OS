//! Server-side player. Wire formats (the appearance buffer + the
//! PLAYER_INFO mask bits) match the rev1 client decode in
//! crates/client/src/login.rs and dash3d/client_player.rs.

use std::collections::HashMap;
use std::sync::Arc;

use protocol::ServerPacket;
use protocol::server as msg;

use crate::entity::PathingEntity;
use crate::script::file::ScriptFile;
use crate::script::state::ScriptArg;

/// The two player timer flavours (Engine-TS `PlayerTimerType`): a NORMAL timer
/// only fires when the player has protected access (not delayed / no modal), a
/// SOFT timer fires regardless of what the player is doing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimerKind {
    Normal,
    Soft,
}

/// A registered repeating timer (Engine-TS `EntityTimer`). Keyed in the
/// player's map by its script id, it re-runs `script` every `interval` ticks;
/// `clock` records the tick of its last fire (or registration).
#[derive(Clone)]
pub struct PlayerTimer {
    pub kind: TimerKind,
    pub script: Arc<ScriptFile>,
    pub args: Vec<ScriptArg>,
    pub interval: i32,
    pub clock: u32,
}

/// The player script-queue flavours (Engine-TS `PlayerQueueType`, minus ENGINE
/// — engine-trigger scripts use `World::enqueue_engine`). NORMAL/LONG/STRONG
/// share the main queue; WEAK has its own (cleared when a STRONG queues). LONG
/// carries a leading logout-action arg and accelerates during logout.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QueueKind {
    Normal,
    Long,
    Strong,
    Weak,
}

/// A queued script awaiting its delay (Engine-TS `PlayerQueueRequest`): runs
/// once `delay` ticks elapse and the player has protected access.
#[derive(Clone)]
pub struct PlayerQueueRequest {
    pub kind: QueueKind,
    pub script: Arc<ScriptFile>,
    pub args: Vec<ScriptArg>,
    pub delay: i32,
}

/// Skill indices (Engine-TS `PlayerStat`); OSRS rev1 has 23 stats.
pub const STAT_ATTACK: usize = 0;
pub const STAT_DEFENCE: usize = 1;
pub const STAT_STRENGTH: usize = 2;
pub const STAT_HITPOINTS: usize = 3;
pub const STAT_RANGED: usize = 4;
pub const STAT_PRAYER: usize = 5;
pub const STAT_MAGIC: usize = 6;
pub const STAT_AGILITY: usize = 16;
pub const STAT_COUNT: usize = 23;
/// rev1 raw-scale XP cap (200M).
pub const MAX_XP: i32 = 200_000_000;
/// Run energy is tracked 0..10000 (Engine-TS scale); the wire byte is /100.
pub const MAX_RUN_ENERGY: i32 = 10000;
/// VarPlayer that drives the client's run orb (Engine-TS `VarPlayerType.RUN`,
/// resolved by name; OSRS rev1 id is 173). Synced when energy hits 0.
pub const VARP_RUN: usize = 173;

// PLAYER_INFO extended-info mask bits (the client's
// getPlayerPosExtended flags).
pub const MASK_SAY: i32 = 0x1; // forced chat
pub const MASK_APPEARANCE: i32 = 0x2;
pub const MASK_PUBLIC_CHAT: i32 = 0x4;
pub const MASK_DAMAGE2: i32 = 0x8;
pub const MASK_ANIM: i32 = 0x10;
pub const MASK_FACE_ENTITY: i32 = 0x20;
pub const MASK_BIG: i32 = 0x40; // flags escape — second flags byte
pub const MASK_FACE_COORD: i32 = 0x80;
pub const MASK_EXACT_MOVE: i32 = 0x100;
pub const MASK_SPOTANIM: i32 = 0x200;
pub const MASK_DAMAGE: i32 = 0x400;

// Default human animation set.
pub const ANIM_READY: i32 = 808;
pub const ANIM_TURN: i32 = 823;
pub const ANIM_WALK: i32 = 819;
pub const ANIM_WALK_B: i32 = 820;
pub const ANIM_WALK_L: i32 = 821;
pub const ANIM_WALK_R: i32 = 822;
pub const ANIM_RUN: i32 = 824;

// Default appearance (idk part ids; stored +256 in the wire slots).
pub const DEFAULT_BODY: [i32; 7] = [0, 10, 18, 26, 33, 36, 42];

pub struct Player {
    pub pid: usize,
    pub username: String,
    pub display_name: String,

    pub entity: PathingEntity,

    // Appearance.
    pub gender: i32,
    /// idk part per body slot (hair, jaw, torso, arms, hands, legs,
    /// feet) — raw ids, +256 applied on the wire.
    pub body: [i32; 7],
    pub colours: [i32; 5],
    pub headicon_pk: i32,
    pub headicon_prayer: i32,
    pub combat_level: i32,
    /// Appearance "skill level" (Java client `ClientPlayer.skillLevel`, Engine-TS
    /// `skillLevel`). Sent as the 2-byte field after combat level. **0 means the
    /// right-click menu shows just the name**; any non-zero value makes the
    /// client render "Name (skill-N)". Static + script-settable (SET_SKILL_LEVEL),
    /// not auto-derived from stats.
    pub skill_level: i32,
    /// The player's base ("bas") stance animations sent in the appearance block
    /// (Engine-TS `readyanim`/`turnanim`/`walkanim`/…/`runanim`), set by the
    /// READYANIM/…/RUNANIM ops to match the worn weapon or active spellbook.
    /// Default to the unarmed human stance so a bare server still animates;
    /// `run_anim` may be -1 (no run animation → forced walk).
    pub ready_anim: i32,
    pub turn_anim: i32,
    pub walk_anim: i32,
    pub walk_anim_b: i32,
    pub walk_anim_l: i32,
    pub walk_anim_r: i32,
    pub run_anim: i32,
    /// Bumped whenever appearance changes so observers resend it.
    pub appearance_seq: u32,

    /// Run energy, 0..=10000. Drains while running, recovers while not.
    pub run_energy: i32,
    /// Worn-equipment weight in grams (negative for weight-reducing gear).
    /// 0 until the inventory/equipment system lands.
    pub run_weight: i32,
    /// Persistent run toggle (the run orb / RUN varp) — Engine-TS `run`. While
    /// set, *all* movement runs. Cleared automatically when energy hits 0.
    pub run: bool,
    /// One-path run from a ctrl-click (Engine-TS `tempRun`) — overrides walk
    /// for the current route only, then clears when the route ends.
    pub temp_run: bool,
    /// Base (un-boosted) skill levels, derived from `experience`.
    pub base_levels: [i32; STAT_COUNT],
    /// Current (boosted/drained) skill levels.
    pub levels: [i32; STAT_COUNT],
    /// Absolute experience per skill (rev1 raw scale).
    pub experience: [i32; STAT_COUNT],
    /// Last `(experience, levels)` flushed to the client per skill (Engine-TS
    /// `lastStats`/`lastLevels`), so `update_stats` emits an UPDATE_STAT only
    /// when one actually changed this tick. Seeded to -1 → the first flush sends
    /// every skill.
    last_stats: [i32; STAT_COUNT],
    last_levels: [i32; STAT_COUNT],
    pub varps: Vec<i32>,

    // The 104x104 build-area origin the client currently holds
    // (set when REBUILD_NORMAL is sent).
    pub origin_x: i32,
    pub origin_z: i32,

    // Per-observer view state: tracked player pids in protocol order,
    // plus the appearance seq last sent per pid.
    pub view_players: Vec<usize>,
    pub seen_appearance: Vec<u32>,
    pub view_npcs: Vec<usize>,

    /// Outgoing packets, drained by the network layer each cycle.
    pub out: Vec<ServerPacket>,
    pub logging_out: bool,

    /// Last interface component the player clicked (Engine-TS `Player.lastCom`),
    /// set by the IF_BUTTON handler and read back by the LAST_COM script op so a
    /// shared button script can tell which component fired. -1 = none.
    pub last_com: i32,

    /// World tick until which the player is action-locked (Engine-TS
    /// `delayedUntil`), set by P_DELAY. While delayed the player can't walk and
    /// the suspended script that delayed them resumes once it elapses.
    pub delayed_until: i32,

    /// Pending walk-trigger script id (Engine-TS `Player.walktrigger`), set by
    /// the WALKTRIGGER op. Fired once — and then reset to -1 — when the player
    /// walks via a move-click (agility obstacles, traps, …). -1 = none.
    pub walk_trigger: i32,

    /// Public-chat state for this tick (Engine-TS `chatColour`/`chatEffect`/
    /// `chatRights`/`chatMessage`). Set by a MESSAGE_PUBLIC packet, broadcast in
    /// the PUBLIC_CHAT extended-info block, then cleared with the mask.
    pub chat_colour: i32,
    pub chat_effect: i32,
    pub chat_rights: i32,
    pub chat_message: Vec<u8>,
    /// One-public-chat-per-tick anti-spam latch (Engine-TS `socialProtect`),
    /// set when a chat is accepted and cleared at end of cycle.
    pub social_protect: bool,

    /// Deferred modal-close request (Engine-TS `requestModalClose`), set by the
    /// CLOSE_MODAL packet or a queued STRONG script and consumed at the top of
    /// the queue pass.
    pub request_modal_close: bool,

    /// Pending logout request (Engine-TS `requestLogout`), set by P_LOGOUT and
    /// granted by `process_logouts` once past `prevent_logout_until`.
    pub request_logout: bool,
    /// World tick before which a logout is refused (Engine-TS `preventLogoutUntil`,
    /// the combat logout delay), set by P_PREVENTLOGOUT. -1 = never prevented.
    pub prevent_logout_until: i32,
    /// Message shown when a logout is refused (Engine-TS `preventLogoutMessage`).
    pub prevent_logout_message: Option<String>,

    /// Components that resume a pause-button dialog when clicked (Engine-TS
    /// `resumeButtons`), set by IF_SETRESUMEBUTTONS. A click on one of these
    /// continues the paused script instead of firing the component's trigger.
    pub resume_buttons: [i32; 5],

    /// Animation-protection flag (Engine-TS `animProtect`), set by P_ANIMPROTECT.
    /// While non-zero the ANIM op can't change the player's animation (e.g. a
    /// special attack can't be interrupted by an emote). 0 = unprotected.
    pub anim_protect: i32,

    /// Registered repeating timers, keyed by timer script id (Engine-TS
    /// `Player.timers`). Processed each cycle by `World::process_timers`.
    pub timers: HashMap<i32, PlayerTimer>,

    /// Main script queue (Engine-TS `Player.queue`): NORMAL/LONG/STRONG entries.
    pub queue: Vec<PlayerQueueRequest>,
    /// Weak script queue (Engine-TS `Player.weakQueue`): cleared when a STRONG
    /// script is queued.
    pub weak_queue: Vec<PlayerQueueRequest>,

    /// PvP damage-dealer tally (Engine-TS `Player.heroPoints`): `(attacker pid,
    /// total damage)`. Whoever tops it on death earns the kill — the player
    /// parallel of the npc tally. Fed by BOTH_HEROPOINTS, read by FINDHERO.
    pub hero_points: Vec<(usize, i32)>,

    /// Random-event gate (Engine-TS `Player.afkEventReady`): the world sets it on
    /// the periodic AFK roll; content polls it via the AFK_EVENT op (which clears
    /// it) to decide whether to spawn a random event for this player.
    pub afk_event_ready: bool,
    /// Staff/moderator level (Engine-TS `Player.staffModLevel`). AFK_EVENT only
    /// fires for non-staff (level < 2); 0 until the account/rights system lands.
    pub staff_mod_level: i32,
    /// Low-detail client mode (Engine-TS `Player.lowMemory`), sent in the login
    /// block. Read by the LOWMEM op; the sound/music ops skip playback for it.
    pub low_memory: bool,
    /// Last two AFK "anchor" coords (Engine-TS `Player.afkZones`), each a packed
    /// `(x-10, z-10)` corner. The player counts as still in their zone while
    /// inside the 21×21 box around either anchor.
    pub afk_zones: [i32; 2],
    /// Ticks the player has stayed within their AFK zone, capped at 1000
    /// (Engine-TS `Player.lastAfkZone`). At 1000 `zones_afk()` is true and the
    /// AFK-event chance doubles.
    pub last_afk_zone: i32,
}

/// Cap on tracked PvP damage dealers (Engine-TS `HeroPoints(16)`).
const MAX_HEROES: usize = 16;

/// AABB overlap test, 1:1 with Engine-TS `CoordGrid.intersects` — true when the
/// `src` and `dest` rectangles share any tile.
fn intersects(sx: i32, sz: i32, sw: i32, sh: i32, dx: i32, dz: i32, dw: i32, dh: i32) -> bool {
    !(dx >= sx + sw || dx + dw <= sx || dz >= sz + sh || dz + dh <= sz)
}

impl Player {
    pub fn new(pid: usize, username: String, x: i32, z: i32, level: i32) -> Player {
        let mut entity = PathingEntity::at(x, z, level);
        entity.face_entity_mask = MASK_FACE_ENTITY;
        // Canonicalise the typed name to its account identity + display form, so
        // "Zezima"/"zezima"/"ZEZIMA" are one account (Engine-TS toSafeName).
        let display_name = crate::base37::to_display_name(&username);
        let username = crate::base37::to_safe_name(&username);
        Player {
            pid,
            display_name,
            username,
            entity,
            gender: 0,
            body: DEFAULT_BODY,
            colours: [0; 5],
            headicon_pk: -1,
            headicon_prayer: -1,
            combat_level: 3,
            skill_level: 0,
            ready_anim: ANIM_READY,
            turn_anim: ANIM_TURN,
            walk_anim: ANIM_WALK,
            walk_anim_b: ANIM_WALK_B,
            walk_anim_l: ANIM_WALK_L,
            walk_anim_r: ANIM_WALK_R,
            run_anim: ANIM_RUN,
            appearance_seq: 1,
            run_energy: MAX_RUN_ENERGY,
            run_weight: 0,
            run: false,
            temp_run: false,
            base_levels: Self::default_base_levels(),
            levels: Self::default_base_levels(),
            experience: {
                let mut xp = [0; STAT_COUNT];
                xp[STAT_HITPOINTS] = crate::skills::xp_for_level(10);
                xp
            },
            last_stats: [-1; STAT_COUNT],
            last_levels: [-1; STAT_COUNT],
            varps: vec![0; 4000],
            origin_x: 0,
            origin_z: 0,
            view_players: Vec::new(),
            seen_appearance: vec![0; 2048],
            view_npcs: Vec::new(),
            out: Vec::new(),
            logging_out: false,
            last_com: -1,
            delayed_until: -1,
            walk_trigger: -1,
            chat_colour: 0,
            chat_effect: 0,
            chat_rights: 0,
            chat_message: Vec::new(),
            social_protect: false,
            request_modal_close: false,
            request_logout: false,
            prevent_logout_until: -1,
            prevent_logout_message: None,
            resume_buttons: [-1; 5],
            anim_protect: 0,
            hero_points: Vec::new(),
            afk_event_ready: false,
            staff_mod_level: 0,
            low_memory: false,
            afk_zones: [0, 0],
            last_afk_zone: 0,
            timers: HashMap::new(),
            queue: Vec::new(),
            weak_queue: Vec::new(),
        }
    }

    /// Queue a script — 1:1 with Engine-TS `Player.enqueueScript` for the
    /// player-script types. WEAK goes to the weak queue; NORMAL/LONG/STRONG to
    /// the main queue (ENGINE-type scripts use `World::enqueue_engine`).
    pub fn enqueue_script(&mut self, kind: QueueKind, script: Arc<ScriptFile>,
                          args: Vec<ScriptArg>, delay: i32) {
        let req = PlayerQueueRequest { kind, script, args, delay };
        match kind {
            QueueKind::Weak => self.weak_queue.push(req),
            _ => self.queue.push(req),
        }
    }

    /// Remove every queued script with `script_id` from both queues — 1:1 with
    /// Engine-TS `Player.unlinkQueuedScript` (the non-ENGINE path, used by
    /// CLEARQUEUE).
    pub fn clear_queued_script(&mut self, script_id: i32) {
        self.queue.retain(|r| r.script.id != script_id);
        self.weak_queue.retain(|r| r.script.id != script_id);
    }

    /// Count queued scripts with `script_id` across both queues (GETQUEUE).
    pub fn count_queued(&self, script_id: i32) -> i32 {
        let n = self.queue.iter().filter(|r| r.script.id == script_id).count()
            + self.weak_queue.iter().filter(|r| r.script.id == script_id).count();
        n as i32
    }

    /// Register (or replace) a repeating timer — 1:1 with Engine-TS
    /// `Player.setTimer`. The timer id is the script's own id, so re-setting the
    /// same script resets its interval/clock.
    pub fn set_timer(&mut self, kind: TimerKind, script: Arc<ScriptFile>,
                     args: Vec<ScriptArg>, interval: i32, now: u32) {
        let id = script.id;
        self.timers.insert(id, PlayerTimer { kind, script, args, interval, clock: now });
    }

    /// Remove a registered timer by its script id (Engine-TS `Player.clearTimer`).
    pub fn clear_timer(&mut self, timer_id: i32) {
        self.timers.remove(&timer_id);
    }

    /// Credit `points` of damage by attacker `pid` to this player's PvP tally —
    /// 1:1 with the npc `add_hero` (Engine-TS `HeroPoints.addHero`). Sub-1 hits
    /// are ignored; a new dealer is recorded up to [`MAX_HEROES`].
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

    /// The attacker who has dealt the most damage to this player (Engine-TS
    /// `HeroPoints.findHero`) — the kill earner; `None` if nobody qualifies.
    pub fn find_hero(&self) -> Option<usize> {
        self.hero_points
            .iter()
            .max_by_key(|(_, points)| *points)
            .map(|(pid, _)| *pid)
    }

    /// Advance the AFK-zone tracker once per cycle (Engine-TS
    /// `Player.updateAfkZones`). The idle counter climbs while the player stays
    /// inside the 21×21 box around either anchor; the moment they leave, the
    /// anchors re-centre on the current tile and the counter resets. A teleport
    /// (INSTANT + jump) keeps the previous anchor so a single hop can't
    /// instantly mark the player "afk" at the new spot.
    pub fn update_afk_zones(&mut self) {
        self.last_afk_zone = (self.last_afk_zone + 1).min(1000);
        if self.within_afk_zone() {
            return;
        }
        // packCoord(level=0, x-10, z-10): the SW corner of the box, level is
        // irrelevant to the 2-D intersection test.
        let coord = ((self.entity.x - 10) << 14) | (self.entity.z - 10);
        if self.entity.move_speed == crate::entity::MoveSpeed::Instant && self.entity.jump {
            self.afk_zones[1] = coord;
        } else {
            self.afk_zones[1] = self.afk_zones[0];
        }
        self.afk_zones[0] = coord;
        self.last_afk_zone = 0;
    }

    /// True once the player has sat in their AFK zone for the full 1000-tick
    /// window (Engine-TS `Player.zonesAfk`) — doubles the random-event chance.
    pub fn zones_afk(&self) -> bool {
        self.last_afk_zone == 1000
    }

    /// Whether the player's current tile falls inside the 21×21 box of either
    /// stored anchor (Engine-TS `Player.withinAfkZone`).
    fn within_afk_zone(&self) -> bool {
        const SIZE: i32 = 21;
        self.afk_zones.iter().any(|&packed| {
            let zx = (packed >> 14) & 0x3fff;
            let zz = packed & 0x3fff;
            intersects(self.entity.x, self.entity.z, 1, 1, zx, zz, SIZE, SIZE)
        })
    }

    /// Whether the player is action-locked at `tick` (Engine-TS `delayed`).
    pub fn is_delayed(&self, tick: i32) -> bool {
        tick < self.delayed_until
    }

    pub fn write(&mut self, packet: ServerPacket) {
        self.out.push(packet);
    }

    /// Local build-area coords (the 7-bit values PLAYER_INFO mode 3
    /// carries).
    pub fn local_x(&self) -> i32 {
        self.entity.x - self.origin_x
    }

    pub fn local_z(&self) -> i32 {
        self.entity.z - self.origin_z
    }

    /// Whether the player has drifted close enough to the build-area
    /// edge that the client needs a REBUILD_NORMAL.
    pub fn needs_rebuild(&self) -> bool {
        let lx = self.local_x();
        let lz = self.local_z();
        lx < 16 || lx >= 88 || lz < 16 || lz >= 88
    }

    fn default_base_levels() -> [i32; STAT_COUNT] {
        let mut lv = [1; STAT_COUNT];
        lv[STAT_HITPOINTS] = 10;
        lv
    }

    /// Add experience to a skill — 1:1 with Engine-TS `Player.addXp`.
    /// Recomputes the base level, tracks/replenishes the current level on a
    /// level-up, refreshes combat level, and emits UPDATE_STAT. Returns whether
    /// the base level advanced this call (the caller fires the ADVANCESTAT /
    /// CHANGESTAT engine-queue scripts — that needs World access).
    pub fn add_xp(&mut self, stat: usize, xp: i32) -> bool {
        if stat >= STAT_COUNT || xp < 0 {
            return false;
        }
        self.experience[stat] = (self.experience[stat] + xp).min(MAX_XP);

        let before = self.base_levels[stat];
        let new_base = crate::skills::level_for_xp(self.experience[stat]);
        // Only track the base into the current level when nothing is boosting it.
        if self.levels[stat] == self.base_levels[stat] {
            self.levels[stat] = new_base;
        }
        self.base_levels[stat] = new_base;
        if new_base > before && self.levels[stat] < before {
            self.levels[stat] += new_base - before; // replenish on level-up
        }
        // Engine-TS addXp tail: re-check the combat level on every xp gain and,
        // when it changes, rebuild the appearance block so nearby players see
        // the new combat level (1:1 with `buildAppearance`). `set_level` does
        // the same; add_xp previously skipped the rebuild, leaving observers on
        // a stale combat level after a level-up.
        let cb = self.compute_combat_level();
        if cb != self.combat_level {
            self.combat_level = cb;
            self.build_appearance();
        }
        // The skills-tab packet is flushed once per tick by `update_stats`.
        new_base > before
    }

    /// Combat level from base levels — 1:1 with Engine-TS `getCombatLevel`.
    pub fn compute_combat_level(&self) -> i32 {
        let b = &self.base_levels;
        let base = 0.25 * f64::from(b[STAT_DEFENCE] + b[STAT_HITPOINTS] + b[STAT_PRAYER] / 2);
        let melee = 0.325 * f64::from(b[STAT_ATTACK] + b[STAT_STRENGTH]);
        let range = 0.325 * f64::from(b[STAT_RANGED] / 2 + b[STAT_RANGED]);
        let magic = 0.325 * f64::from(b[STAT_MAGIC] / 2 + b[STAT_MAGIC]);
        (base + melee.max(range).max(magic)).floor() as i32
    }

    /// Send the full stat block + refresh combat level (login resync). Seeds the
    /// per-tick dedup trackers so the first `update_stats` doesn't redundantly
    /// resend the same block.
    pub fn sync_stats(&mut self) {
        for stat in 0..STAT_COUNT {
            self.write(msg::update_stat(stat as i32, self.levels[stat], self.experience[stat]));
            self.last_stats[stat] = self.experience[stat];
            self.last_levels[stat] = self.levels[stat];
        }
        self.combat_level = self.compute_combat_level();
    }

    /// Flush this tick's stat changes to the client — 1:1 with Engine-TS
    /// `NetworkPlayer.updateStats`. Emits UPDATE_STAT only for skills whose
    /// experience or current level changed since the last flush, so several
    /// mutations in one tick collapse to a single packet per skill.
    pub fn update_stats(&mut self) {
        for stat in 0..STAT_COUNT {
            if self.experience[stat] != self.last_stats[stat]
                || self.levels[stat] != self.last_levels[stat]
            {
                self.write(msg::update_stat(stat as i32, self.levels[stat], self.experience[stat]));
                self.last_stats[stat] = self.experience[stat];
                self.last_levels[stat] = self.levels[stat];
            }
        }
    }

    /// Per-tick run-energy update — 1:1 with Engine-TS `Player.updateEnergy`.
    /// Recovers (agility-scaled) when the player took fewer than 2 steps this
    /// tick, drains (weight-scaled) when it ran a full two. Emits
    /// UPDATE_RUNENERGY only when the displayed percent changes.
    pub fn update_energy(&mut self, tick: i32) {
        // An action-locked (P_DELAY) player neither recovers nor drains energy —
        // 1:1 with Engine-TS updateEnergy's leading `if (this.delayed) return`.
        if self.is_delayed(tick) {
            return;
        }
        let before_pct = self.run_energy / 100;

        if self.entity.steps_taken < 2 {
            let recovered = self.base_levels[STAT_AGILITY] / 6 + 8;
            self.run_energy = (self.run_energy + recovered).min(MAX_RUN_ENERGY);
        } else {
            // Drain in float (Engine-TS keeps `weightKg = runweight / 1000` and
            // the whole loss as a double, truncating only with the trailing
            // `| 0`). Truncating the kilograms first — as an integer divide would
            // — under-counts the drain by up to 1 for any fractional-kg weight.
            let weight_kg = self.run_weight as f64 / 1000.0;
            let clamp_weight = weight_kg.clamp(0.0, 64.0);
            let loss = (67.0 + (67.0 * clamp_weight) / 64.0) as i32;
            self.run_energy = (self.run_energy - loss).max(0);
        }

        // Out of energy turns the run orb off; low energy cancels a one-path
        // ctrl-run — 1:1 with Engine-TS updateEnergy. Syncing the RUN varp
        // turns the client's run orb off (Engine-TS `setVar(VarPlayerType.RUN,
        // run)`); guarding on the transition avoids resending the no-op every
        // tick energy sits at 0.
        if self.run_energy == 0 && self.run {
            self.run = false;
            self.set_var(VARP_RUN, 0);
        }
        if self.run_energy < 100 {
            self.temp_run = false;
        }

        let after_pct = self.run_energy / 100;
        if after_pct != before_pct {
            self.write(msg::update_runenergy(after_pct));
        }
    }

    /// Default move speed from the persistent run toggle (Engine-TS
    /// `defaultMoveSpeed`): run when the run orb is on, else walk.
    fn default_move_speed(&self) -> crate::entity::MoveSpeed {
        if self.run {
            crate::entity::MoveSpeed::Run
        } else {
            crate::entity::MoveSpeed::Walk
        }
    }

    /// Per-tick player movement — 1:1 with Engine-TS player `updateMovement`:
    /// choose the speed from the run / temp-run flags, advance, then drop the
    /// one-path temp run once the route is exhausted. Returns whether a step
    /// was taken.
    pub fn update_movement(&mut self) -> bool {
        use crate::entity::MoveSpeed;
        if self.entity.move_speed != MoveSpeed::Instant {
            self.entity.move_speed = self.default_move_speed();
            // A player with no run animation can't visually run, so the speed
            // clamps to WALK even with the run orb or a ctrl-click set — 1:1 with
            // Engine-TS updateMovement (`if runanim === -1 → WALK else if tempRun
            // → RUN`). RUNANIM sets run_anim to -1 for stances that lack a run.
            if self.run_anim == -1 {
                self.entity.move_speed = MoveSpeed::Walk;
            } else if self.temp_run {
                self.entity.move_speed = MoveSpeed::Run;
            }
        }
        let processed = self.entity.process_movement();
        if !processed {
            self.temp_run = false;
        }
        processed
    }

    /// End-of-cycle reset — the player half of Engine-TS `resetPathingEntity` /
    /// `Player.resetEntity`. Clears the shared transient state, restores the
    /// run-aware default move speed, and re-arms the one-chat-per-tick latch.
    /// Restoring the move speed is what releases a one-tick INSTANT (teleport):
    /// `update_movement` skips the recompute while INSTANT, so without this a
    /// teleported player would wedge as non-walking until a fresh route — the npc
    /// half already does the same in `Npc::reset_transient`.
    pub fn reset_transient(&mut self) {
        self.entity.reset_transient();
        self.entity.move_speed = self.default_move_speed();
        self.social_protect = false;
    }

    /// The script-facing handle for this player (Engine-TS player uid): the low
    /// 21 bits of its Base37 name hash packed above the 11-bit slot. A held uid
    /// goes stale if the slot is later reused by a different account, which
    /// [`World::get_player_by_uid`](crate::World) detects via the hash.
    pub fn uid(&self) -> i32 {
        let hash = (crate::base37::to_base37(&self.username) & 0x1f_ffff) as u32;
        ((hash << 11) | (self.pid as u32 & 0x7ff)) as i32
    }

    /// Read a player varp (Engine-TS `getVar`); 0 for an out-of-range id.
    pub fn get_var(&self, varp: usize) -> i32 {
        self.varps.get(varp).copied().unwrap_or(0)
    }

    /// Set a player varp and sync it to the client — 1:1 with Engine-TS
    /// `setVar`. A value that fits a signed byte rides VARP_SMALL, otherwise
    /// VARP_LARGE. (Once varp config loads, gate transmission on the type's
    /// `transmit` flag; for now every varp transmits.)
    pub fn set_var(&mut self, varp: usize, value: i32) {
        if varp >= self.varps.len() {
            return;
        }
        self.varps[varp] = value;
        if (-128..=127).contains(&value) {
            self.write(msg::varp_small(varp as i32, value));
        } else {
            self.write(msg::varp_large(varp as i32, value));
        }
    }

    /// Float a hint arrow over an npc (Engine-TS `hintNpc`).
    pub fn hint_npc(&mut self, nid: i32) {
        self.write(msg::hint_npc(nid));
    }

    /// Float a hint arrow over a player (Engine-TS `hintPlayer`).
    pub fn hint_player(&mut self, slot: i32) {
        self.write(msg::hint_player(slot));
    }

    /// Float a hint arrow over an absolute tile (Engine-TS `hintTile`). `offset`
    /// (2..6) picks the arrow's position on the tile.
    pub fn hint_tile(&mut self, offset: i32, x: i32, z: i32, height: i32) {
        self.write(msg::hint_tile(offset, x, z, height));
    }

    /// Clear any active hint arrow (Engine-TS `stopHint`).
    pub fn hint_stop(&mut self) {
        self.write(msg::hint_stop());
    }

    /// Aim the cutscene camera at an absolute tile (Engine-TS CAM_LOOKAT) —
    /// converted to this player's scene-local frame. `rate`/`rate2` pace the
    /// rotation; the client only commits the move when `rate2 >= 100`.
    pub fn cam_lookat(&mut self, x: i32, z: i32, height: i32, rate: i32, rate2: i32) {
        self.write(msg::cam_lookat(x - self.origin_x, z - self.origin_z, height, rate, rate2));
    }

    /// Move the cutscene camera to an absolute tile (Engine-TS CAM_MOVETO).
    pub fn cam_moveto(&mut self, x: i32, z: i32, height: i32, rate: i32, rate2: i32) {
        self.write(msg::cam_moveto(x - self.origin_x, z - self.origin_z, height, rate, rate2));
    }

    /// Shake a camera component (Engine-TS CAM_SHAKE).
    pub fn cam_shake(&mut self, slot: i32, axis: i32, random: i32, amplitude: i32) {
        self.write(msg::cam_shake(slot, axis, random, amplitude));
    }

    /// Restore the default orbit camera (Engine-TS CAM_RESET).
    pub fn cam_reset(&mut self) {
        self.write(msg::cam_reset());
    }

    /// Set a stat's *current* level (the boosted/drained value — base xp is
    /// untouched) and sync it to the skills tab. Clamps to [0, 255]. Callers
    /// pass a validated `stat`.
    fn set_stat_level(&mut self, stat: usize, level: i32) {
        self.levels[stat] = level.clamp(0, 255);
        // Flushed once per tick by `update_stats`.
    }

    /// Set a skill's level directly — 1:1 with Engine-TS `setLevel`. Clamps to
    /// [1, 99], resets base + current level and the xp to that level's exact
    /// threshold, recomputes the combat level (rebuilding appearance if it
    /// changed), and syncs the skills tab. The inverse of [`add_xp`](Self::add_xp).
    pub fn set_level(&mut self, stat: usize, level: i32) {
        if stat >= STAT_COUNT {
            return;
        }
        let level = level.clamp(1, 99);
        self.base_levels[stat] = level;
        self.levels[stat] = level;
        self.experience[stat] = crate::skills::xp_for_level(level);
        let cb = self.compute_combat_level();
        if cb != self.combat_level {
            self.combat_level = cb;
            self.build_appearance();
        }
        // Flushed once per tick by `update_stats`.
    }

    // The STAT_* helpers each return whether the value they computed differs
    // from the current level (Engine-TS compares the pre-255-clamp value vs
    // current to decide whether to fire CHANGESTAT — so a boost on an already
    // maxed stat still counts as a change). The caller fires CHANGESTAT (it
    // needs World access).

    /// STAT_ADD — add `constant + base*percent/100` to the current level
    /// (capped at 255). 1:1 with Engine-TS.
    pub fn stat_add(&mut self, stat: usize, constant: i32, percent: i32) -> bool {
        let (base, current) = (self.base_levels[stat], self.levels[stat]);
        let added = current + (constant + base * percent / 100);
        self.set_stat_level(stat, added);
        added != current
    }

    /// STAT_SUB — subtract `constant + base*percent/100` from the current level
    /// (floored at 0).
    pub fn stat_sub(&mut self, stat: usize, constant: i32, percent: i32) -> bool {
        let (base, current) = (self.base_levels[stat], self.levels[stat]);
        let subbed = current - (constant + base * percent / 100);
        self.set_stat_level(stat, subbed);
        subbed != current
    }

    /// STAT_BOOST — temporary boost (potions): raise by `constant +
    /// base*percent/100` but never past `base + boost`, and never below the
    /// current level.
    pub fn stat_boost(&mut self, stat: usize, constant: i32, percent: i32) -> bool {
        let (base, current) = (self.base_levels[stat], self.levels[stat]);
        let boost = constant + base * percent / 100;
        let boosted = (current + boost).min(base + boost).max(current);
        self.set_stat_level(stat, boosted);
        boosted != current
    }

    /// STAT_DRAIN — drain by `constant + current*percent/100` (note: percent is
    /// of the *current* level, not base), floored at 0.
    pub fn stat_drain(&mut self, stat: usize, constant: i32, percent: i32) -> bool {
        let current = self.levels[stat];
        let subbed = current - (constant + current * percent / 100);
        self.set_stat_level(stat, subbed);
        subbed != current
    }

    /// STAT_HEAL — restore toward base by `constant + base*percent/100`, capped
    /// at base and never lowering the current level.
    pub fn stat_heal(&mut self, stat: usize, constant: i32, percent: i32) -> bool {
        let (base, current) = (self.base_levels[stat], self.levels[stat]);
        let healed = (current + (constant + base * percent / 100)).min(base).max(current);
        self.set_stat_level(stat, healed);
        healed != current
    }

    /// HEALENERGY — restore run energy (100 = 1%, 10000 = 100%), clamped to the
    /// 0..=10000 range. The value reaches the client via the periodic energy sync.
    pub fn heal_energy(&mut self, amount: i32) {
        self.run_energy = (self.run_energy + amount).clamp(0, MAX_RUN_ENERGY);
    }

    /// Cancel any queued walk and clear the client's minimap move-to flag —
    /// 1:1 with Engine-TS `Player.unsetMapFlag`. Called when the engine takes
    /// over movement (e.g. an exact-move) so the client stops trying to walk
    /// the now-discarded path.
    pub fn unset_map_flag(&mut self) {
        self.entity.waypoints.clear();
        self.write(msg::unset_map_flag());
    }

    pub fn build_appearance(&mut self) {
        self.appearance_seq = self.appearance_seq.wrapping_add(1).max(1);
        self.entity.masks |= MASK_APPEARANCE;
    }

    /// Take a hit — 1:1 with Engine-TS `Player.applyDamage`. Subtracts from HP
    /// (`levels[HITPOINTS]`, clamped at 0) and files the hitsplat (capped at the
    /// remaining HP) under the player DAMAGE/DAMAGE2 masks.
    pub fn apply_damage(&mut self, damage: i32, dtype: i32) {
        let current = self.levels[STAT_HITPOINTS];
        let dealt = if current - damage <= 0 {
            self.levels[STAT_HITPOINTS] = 0;
            current
        } else {
            self.levels[STAT_HITPOINTS] = current - damage;
            damage
        };
        self.entity.record_hit(dealt, dtype, MASK_DAMAGE, MASK_DAMAGE2);
    }

    /// Whether this player has been reduced to 0 hitpoints.
    pub fn is_dead(&self) -> bool {
        self.levels[STAT_HITPOINTS] <= 0
    }

    /// The appearance block body — matches the rev1 client's
    /// ClientPlayer.setAppearance read order exactly.
    pub fn appearance_bytes(&self) -> Vec<u8> {
        let mut p = io::packet::Packet::new(128);
        p.p1(self.gender);
        p.p1(self.headicon_pk);
        p.p1(self.headicon_prayer);

        // 12 wire slots: 0 head, 1 cape, 2 amulet, 3 weapon, 4 torso,
        // 5 shield, 6 arms, 7 legs, 8 hair, 9 hands, 10 feet, 11 jaw.
        // No worn equipment yet — idk parts fill their body slots.
        let slot_part: [Option<usize>; 12] = [
            None,    // head
            None,    // cape
            None,    // amulet
            None,    // weapon
            Some(2), // torso
            None,    // shield
            Some(3), // arms
            Some(5), // legs
            Some(0), // hair
            Some(4), // hands
            Some(6), // feet
            Some(1), // jaw
        ];
        for part in slot_part {
            match part {
                Some(i) => p.p2(256 + self.body[i]),
                None => p.p1(0),
            }
        }

        for colour in self.colours {
            p.p1(colour);
        }

        p.p2(self.ready_anim);
        p.p2(self.turn_anim);
        p.p2(self.walk_anim);
        p.p2(self.walk_anim_b);
        p.p2(self.walk_anim_l);
        p.p2(self.walk_anim_r);
        p.p2(self.run_anim);

        p.pjstr(&self.display_name);
        p.p1(self.combat_level);
        p.p2(self.skill_level);

        let mut data = p.data;
        data.truncate(p.pos as usize);
        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UPDATE_RUNENERGY: u8 = 41;

    fn player() -> Player {
        Player::new(0, "t".into(), 3200, 3200, 0)
    }

    #[test]
    fn running_drains_energy_with_fractional_kg_precision() {
        // A 21.5 kg load running this tick: Engine-TS drains
        // (67 + 67*21.5/64) | 0 = 89. The old integer `runweight/1000` truncated
        // to 21 kg first and drained only 88 — this guards the float path.
        let mut p = player();
        p.entity.steps_taken = 2; // two steps in a tick = running
        p.run_weight = 21500;
        p.run_energy = MAX_RUN_ENERGY;
        p.out.clear();
        p.update_energy(100); // delayed_until = -1, so not delayed
        assert_eq!(p.run_energy, MAX_RUN_ENERGY - 89, "fractional-kg drain matches the float formula");
    }

    #[test]
    fn teleport_instant_is_released_at_end_of_cycle() {
        use crate::entity::MoveSpeed;
        let mut p = player();
        p.run = false;
        // Teleport snaps the player and flags INSTANT for this tick.
        p.entity.teleport(3225, 3225, 0, false);
        assert_eq!(p.entity.move_speed, MoveSpeed::Instant);
        // The end-of-cycle reset releases INSTANT back to the run-aware default.
        p.reset_transient();
        assert_eq!(p.entity.move_speed, MoveSpeed::Walk, "INSTANT released after the cycle");
        // ...so a route queued next tick actually walks (no longer wedged).
        p.entity.queue_waypoints(&[(3226, 3225)]);
        p.update_movement();
        assert_eq!(p.entity.x, 3226, "walks the tile after teleport instead of staying INSTANT");
    }

    #[test]
    fn no_run_animation_forces_walk() {
        use crate::entity::MoveSpeed;
        // Run orb on but the stance lacks a run animation → clamps to walk.
        let mut p = player();
        p.run = true;
        p.run_anim = -1;
        p.entity.queue_waypoints(&[(3205, 3200)]);
        p.update_movement();
        assert_eq!(p.entity.move_speed, MoveSpeed::Walk, "no run anim clamps to walk");
        assert_eq!(p.entity.x, 3201, "walked one tile, not two");

        // A present run animation lets the orb run (two tiles).
        let mut p = player();
        p.run = true;
        p.run_anim = ANIM_RUN;
        p.entity.queue_waypoints(&[(3205, 3200)]);
        p.update_movement();
        assert_eq!(p.entity.move_speed, MoveSpeed::Run);
        assert_eq!(p.entity.x, 3202, "ran two tiles");

        // A ctrl-click temp-run is likewise ignored without a run animation.
        let mut p = player();
        p.run_anim = -1;
        p.temp_run = true;
        p.entity.queue_waypoints(&[(3205, 3200)]);
        p.update_movement();
        assert_eq!(p.entity.move_speed, MoveSpeed::Walk, "temp-run ignored without a run anim");
    }

    #[test]
    fn run_flags_drive_move_speed() {
        use crate::entity::MoveSpeed;

        // Persistent run orb: movement runs without a ctrl-click.
        let mut p = player();
        p.run = true;
        p.entity.queue_waypoints(&[(3204, 3200)]);
        p.update_movement();
        assert_eq!(p.entity.move_speed, MoveSpeed::Run, "run orb → run");
        assert_eq!(p.entity.x, 3202, "ran two tiles");

        // Ctrl-click (temp run) on a walker: runs this route, then clears.
        let mut p = player();
        p.temp_run = true;
        p.entity.queue_waypoints(&[(3201, 3200)]);
        p.update_movement();
        assert_eq!(p.entity.move_speed, MoveSpeed::Run, "temp run → run");
        assert!(p.temp_run, "temp run holds while the route is live");
        p.update_movement(); // route exhausted
        assert!(!p.temp_run, "temp run drops when the route ends");
    }

    #[test]
    fn energy_depletion_clears_run_flags() {
        let mut p = player();
        p.run = true;
        p.temp_run = true;

        // Drain to a low-but-nonzero level: temp run cancels (<100), orb holds.
        p.run_energy = 133;
        p.entity.steps_taken = 2; // ran a full step → drains 67
        p.update_energy(0);
        assert_eq!(p.run_energy, 66);
        assert!(!p.temp_run, "ctrl-run cancels under 100 energy");
        assert!(p.run, "the run orb stays on until empty");

        // Drain to empty: the orb turns off too.
        p.run_energy = 50;
        p.entity.steps_taken = 2;
        p.update_energy(0);
        assert_eq!(p.run_energy, 0);
        assert!(!p.run, "empty energy turns the run orb off");
    }

    #[test]
    fn a_delayed_player_neither_recovers_nor_drains_energy() {
        let mut p = player();
        // Action-lock the player a few ticks ahead.
        p.delayed_until = 10;

        // Idle (would normally recover) while delayed: energy is untouched.
        p.run_energy = 5000;
        p.entity.steps_taken = 0;
        p.update_energy(5);
        assert_eq!(p.run_energy, 5000, "no recovery while action-locked");

        // Running a full step while delayed: still no drain.
        p.entity.steps_taken = 2;
        p.update_energy(5);
        assert_eq!(p.run_energy, 5000, "no drain while action-locked");

        // Once the lock lapses, the normal agility recovery resumes.
        p.entity.steps_taken = 0;
        p.update_energy(10);
        assert!(p.run_energy > 5000, "recovery resumes after the lock lifts");
    }

    #[test]
    fn stat_boost_drain_heal_match_engine_formulas() {
        let mut p = player();
        let s = STAT_ATTACK;
        p.base_levels[s] = 50;
        p.levels[s] = 50;
        p.out.clear();

        // Boost: +5 flat + 10% of base(50)=5 → +10, to 60. Flushed by update_stats.
        p.stat_boost(s, 5, 10);
        assert_eq!(p.levels[s], 60);
        p.update_stats();
        assert!(p.out.iter().any(|m| m.opcode == UPDATE_STAT));
        // Re-boosting caps at base+boost (60) — no stacking.
        p.stat_boost(s, 5, 10);
        assert_eq!(p.levels[s], 60);

        // Drain takes a % of the *current* level: -20% of 60 = -12 → 48.
        p.stat_drain(s, 0, 20);
        assert_eq!(p.levels[s], 48);

        // Heal toward base: +3 + 10% of base(5) = +8 → 56, capped at base 50.
        p.stat_heal(s, 3, 10);
        assert_eq!(p.levels[s], 50);

        // The base level / xp never moved — only the live level.
        assert_eq!(p.base_levels[s], 50);
    }

    #[test]
    fn stat_add_sub_and_heal_energy() {
        let mut p = player();
        let s = STAT_STRENGTH;
        p.base_levels[s] = 40;
        p.levels[s] = 40;

        p.stat_add(s, 10, 0); // +10 flat → 50
        assert_eq!(p.levels[s], 50);
        p.stat_sub(s, 0, 25); // -25% of base(40) = 10 → 40
        assert_eq!(p.levels[s], 40);

        p.run_energy = 5000;
        p.heal_energy(3000); // +30%
        assert_eq!(p.run_energy, 8000);
        p.heal_energy(5000); // clamps at the maximum
        assert_eq!(p.run_energy, MAX_RUN_ENERGY);
    }

    #[test]
    fn hint_arrows_are_fixed_six_byte_packets() {
        use io::packet::Packet;
        let mut p = player();
        p.out.clear();

        // NPC hint: type 1, then the nid, zero-padded to 6 bytes.
        p.hint_npc(42);
        let pkt = p.out.iter().find(|m| m.opcode == 160).expect("HINT_ARROW");
        assert_eq!(pkt.body.len(), 6);
        let mut r = Packet::from_vec(pkt.body.clone());
        assert_eq!(r.g1(), 1, "type 1 = npc");
        assert_eq!(r.g2(), 42, "nid");
        assert_eq!((r.g2(), r.g1()), (0, 0), "zero padding");

        // Tile hint: absolute coords (no origin conversion) + offset + height.
        p.out.clear();
        p.hint_tile(2, 3210, 3250, 64);
        let mut r = Packet::from_vec(p.out.iter().find(|m| m.opcode == 160).unwrap().body.clone());
        assert_eq!(r.g1(), 2, "offset/type 2 = centred tile");
        assert_eq!((r.g2(), r.g2()), (3210, 3250), "absolute tile coords");
        assert_eq!(r.g1(), 64, "height");

        // Player hint (type 10) and clear (type -1 → 255 unsigned).
        p.out.clear();
        p.hint_player(7);
        let mut r = Packet::from_vec(p.out.iter().find(|m| m.opcode == 160).unwrap().body.clone());
        assert_eq!((r.g1(), r.g2()), (10, 7), "type 10 = player slot");

        p.out.clear();
        p.hint_stop();
        let mut r = Packet::from_vec(p.out.iter().find(|m| m.opcode == 160).unwrap().body.clone());
        assert_eq!(r.g1(), 255, "type -1 clears (0xff)");
    }

    #[test]
    fn camera_packets_use_scene_local_coords() {
        use io::packet::Packet;
        let mut p = player();
        p.origin_x = 3200;
        p.origin_z = 3200;
        p.out.clear();

        // Absolute (3210,3215) → local (10,15) against the build-area origin.
        p.cam_lookat(3210, 3215, 100, 8, 200);
        let pkt = p.out.iter().find(|m| m.opcode == 225).expect("CAM_LOOKAT");
        assert_eq!(pkt.body.len(), 6);
        let mut r = Packet::from_vec(pkt.body.clone());
        assert_eq!((r.g1(), r.g1()), (10, 15), "scene-local x,z");
        assert_eq!(r.g2(), 100, "height");
        assert_eq!((r.g1(), r.g1()), (8, 200), "rate, rate2");

        // CAM_MOVETO shares the layout under opcode 169.
        p.out.clear();
        p.cam_moveto(3208, 3203, 50, 4, 150);
        let pkt = p.out.iter().find(|m| m.opcode == 169).expect("CAM_MOVETO");
        let mut r = Packet::from_vec(pkt.body.clone());
        assert_eq!((r.g1(), r.g1()), (8, 3), "scene-local x,z");

        // Shake (4 bytes) and reset (empty) are fixed control packets.
        p.out.clear();
        p.cam_shake(4, 1, 2, 3);
        assert_eq!(p.out.iter().find(|m| m.opcode == 17).expect("CAM_SHAKE").body.len(), 4);
        p.out.clear();
        p.cam_reset();
        assert_eq!(p.out.iter().find(|m| m.opcode == 198).expect("CAM_RESET").body.len(), 0);
    }

    #[test]
    fn set_var_stores_and_syncs_small_then_large() {
        use io::packet::Packet;
        let mut p = player();
        p.out.clear();

        // A byte-sized value rides VARP_SMALL (88): g2_alt1 id, g1b_alt3 value.
        p.set_var(173, -42);
        assert_eq!(p.get_var(173), -42);
        let pkt = p.out.iter().find(|m| m.opcode == 88).expect("VARP_SMALL");
        assert_eq!(pkt.body.len(), 3);
        let mut r = Packet::from_vec(pkt.body.clone());
        assert_eq!(r.g2_alt1(), 173, "varp id");
        assert_eq!(r.g1b_alt3() as i32, -42, "signed byte value");

        // A value beyond a byte rides VARP_LARGE (180): g2_alt3 id, g4 value.
        p.out.clear();
        p.set_var(174, 100_000);
        assert_eq!(p.get_var(174), 100_000);
        let pkt = p.out.iter().find(|m| m.opcode == 180).expect("VARP_LARGE");
        assert_eq!(pkt.body.len(), 6);
        let mut r = Packet::from_vec(pkt.body.clone());
        assert_eq!(r.g2_alt3(), 174, "varp id");
        assert_eq!(r.g4(), 100_000, "32-bit value");
    }

    #[test]
    fn zero_energy_turns_run_off_and_syncs_the_run_varp() {
        use io::packet::Packet;
        let mut p = player();
        p.run = true;
        p.run_energy = 67;        // one full run-step (weight 0) drains exactly to 0
        p.entity.steps_taken = 2; // ran this tick
        p.out.clear();
        p.update_energy(0);
        assert_eq!(p.run_energy, 0);
        assert!(!p.run, "run orb forced off at 0 energy");
        let pkt = p.out.iter().find(|m| m.opcode == 88).expect("VARP_SMALL for RUN");
        let mut r = Packet::from_vec(pkt.body.clone());
        assert_eq!(r.g2_alt1(), VARP_RUN as i32, "RUN varp id");
        assert_eq!(r.g1b_alt3() as i32, 0, "run varp set to 0 — client orb off");
    }

    #[test]
    fn zero_energy_while_run_already_off_sends_no_run_varp() {
        let mut p = player();
        p.run = false;            // run already off
        p.run_energy = 67;
        p.entity.steps_taken = 2;
        p.out.clear();
        p.update_energy(0);
        assert_eq!(p.run_energy, 0);
        assert!(p.out.iter().all(|m| m.opcode != 88),
                "no redundant RUN varp when the orb is already off");
    }

    #[test]
    fn drains_67_per_tick_unencumbered_when_running() {
        let mut p = player();
        p.entity.steps_taken = 2; // a full run step
        p.update_energy(0);
        assert_eq!(p.run_energy, MAX_RUN_ENERGY - 67); // weight 0 -> loss 67
    }

    #[test]
    fn recovers_by_agility_formula_when_idle() {
        let mut p = player();
        p.run_energy = 5000;
        p.entity.steps_taken = 0;
        p.update_energy(0);
        // base agility 1 -> (1/6)|0 + 8 = 8
        assert_eq!(p.run_energy, 5008);
    }

    #[test]
    fn drain_clamps_at_zero() {
        let mut p = player();
        p.run_energy = 40;
        p.entity.steps_taken = 2;
        p.update_energy(0);
        assert_eq!(p.run_energy, 0);
    }

    const UPDATE_STAT: u8 = 208;

    #[test]
    fn add_xp_levels_up_and_emits_stat() {
        let mut p = player();
        p.out.clear();
        p.add_xp(STAT_ATTACK, 83); // exactly level 2
        assert_eq!(p.base_levels[STAT_ATTACK], 2);
        assert_eq!(p.levels[STAT_ATTACK], 2);
        assert_eq!(p.experience[STAT_ATTACK], 83);
        p.update_stats();
        assert!(p.out.iter().any(|m| m.opcode == UPDATE_STAT));
    }

    #[test]
    fn update_stats_batches_and_dedups_per_tick() {
        let mut p = player();
        let s = STAT_ATTACK;
        p.base_levels[s] = 50;
        p.levels[s] = 50;
        // Seed the trackers so the test only sees changes to this skill.
        p.update_stats();
        p.out.clear();

        // Two mutations to the same skill in one "tick", net level 50 → 48...
        p.stat_boost(s, 5, 10); // → 60
        p.stat_drain(s, 0, 20); // → 48
        // ...flush to exactly one UPDATE_STAT, carrying the final value.
        p.update_stats();
        let stat_packets = p.out.iter().filter(|m| m.opcode == UPDATE_STAT).count();
        assert_eq!(stat_packets, 1, "several changes in a tick collapse to one packet");
        assert_eq!(p.levels[s], 48, "the flushed value is the final one");

        // A follow-up flush with no change emits nothing.
        p.out.clear();
        p.update_stats();
        assert!(p.out.iter().all(|m| m.opcode != UPDATE_STAT), "no packet when nothing changed");
    }

    #[test]
    fn add_xp_rebuilds_appearance_on_combat_level_change() {
        // Engine-TS addXp tail: a combat-stat gain that raises the combat
        // level must rebuild the appearance so observers resend it.
        let mut p = player();
        let seq_before = p.appearance_seq;
        p.entity.masks = 0;
        p.add_xp(STAT_STRENGTH, 50_000); // well past level 40
        assert!(p.combat_level > 3, "combat level rose from the strength gain");
        assert_ne!(p.appearance_seq, seq_before, "appearance buffer bumped");
        assert_ne!(p.entity.masks & MASK_APPEARANCE, 0,
                   "appearance mask flagged so observers resend it");
    }

    #[test]
    fn add_xp_non_combat_skill_leaves_appearance_untouched() {
        // Agility isn't in the combat formula, so a level-up there must not
        // touch the combat level or rebuild the appearance.
        let mut p = player();
        let seq_before = p.appearance_seq;
        p.entity.masks = 0;
        p.add_xp(STAT_AGILITY, 50_000);
        assert_eq!(p.combat_level, 3, "non-combat skill leaves combat level");
        assert_eq!(p.appearance_seq, seq_before, "no appearance rebuild");
        assert_eq!(p.entity.masks & MASK_APPEARANCE, 0, "no appearance mask");
    }

    #[test]
    fn add_xp_caps_xp_and_level() {
        let mut p = player();
        p.add_xp(STAT_MAGIC, 500_000_000); // beyond the 200M cap
        assert_eq!(p.experience[STAT_MAGIC], MAX_XP);
        assert_eq!(p.base_levels[STAT_MAGIC], 99);
    }

    #[test]
    fn combat_level_starts_at_three() {
        assert_eq!(player().compute_combat_level(), 3);
    }

    #[test]
    fn combat_level_from_base_levels() {
        let mut p = player();
        for s in [STAT_ATTACK, STAT_STRENGTH, STAT_DEFENCE, STAT_HITPOINTS] {
            p.base_levels[s] = 40;
        }
        // base 0.25*(40+40+0)=20, melee 0.325*(40+40)=26 -> floor(46)
        assert_eq!(p.compute_combat_level(), 46);
    }

    #[test]
    fn emits_packet_only_when_displayed_percent_changes() {
        let mut p = player();
        // 50 -> 58: still 0% displayed, no packet.
        p.run_energy = 50;
        p.entity.steps_taken = 0;
        p.out.clear();
        p.update_energy(0);
        assert!(p.out.iter().all(|m| m.opcode != UPDATE_RUNENERGY));

        // 95 -> 103: crosses 0% -> 1%, packet emitted.
        p.run_energy = 95;
        p.out.clear();
        p.update_energy(0);
        assert!(p.out.iter().any(|m| m.opcode == UPDATE_RUNENERGY));
    }
}
