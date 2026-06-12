//! World state + the 600ms game cycle — mirrors the Engine2007
//! reference World.ts shape (read input → process → write output)
//! with the entity/script layers the reference stubs out.

use protocol::client::ClientMessage;
use protocol::server as msg;

use crate::entity::npc::Npc;
use crate::entity::player;
use crate::entity::player::Player;
use crate::info;
use crate::script::file::ScriptFile;
use crate::script::provider::ScriptProvider;
use crate::script::runner;
use crate::script::state::{ScriptArg, ScriptState};
use crate::script::trigger;
use std::sync::Arc;

/// A script queued to run on the player after `delay` ticks — the engine half
/// of Engine-TS `Player.engineQueue` (used for ADVANCESTAT, CHANGESTAT and other
/// engine-triggered delayed actions). The script runs once its delay counts
/// down to 0, then is dropped.
struct EngineQueueEntry {
    script: Arc<ScriptFile>,
    args: Vec<ScriptArg>,
    delay: i32,
}

/// A world-level script awaiting its delay — the engine half of Engine-TS
/// `World.queue` (the [world] trigger / WORLD_DELAY). Unlike the player queue it
/// holds the live `ScriptState` so a WORLD_DELAY-suspended script resumes from
/// its program counter.
struct WorldQueueEntry {
    state: ScriptState,
    delay: i32,
}

/// When a P_DELAY/P_PAUSEBUTTON-suspended script should resume.
enum ResumeOn {
    /// After a tick deadline (P_DELAY).
    Tick(u32),
    /// When the client sends RESUME_PAUSEBUTTON (P_PAUSEBUTTON dialog).
    PauseButton,
    /// When the client sends RESUME_P_COUNTDIALOG (P_COUNTDIALOG "enter amount"
    /// dialog) — the entered value resumes the script via LAST_INT.
    CountDialog,
}

pub const MAX_PLAYERS: usize = 2048;
pub const MAX_NPCS: usize = 8192;

/// Entities are mutually visible within this many tiles (the info
/// protocol's 5-bit signed deltas span -16..15).
pub const VIEW_DISTANCE: i32 = 15;

/// For an observer, the build-area-local zone base + packed in-zone slot of an
/// absolute tile — `None` if the tile is on another level or outside the
/// player's 104-tile build area. Shared by the obj/loc zone broadcasts.
fn local_zone_slot(p: &Player, x: i32, z: i32, level: i32) -> Option<(i32, i32, i32)> {
    if p.entity.level != level {
        return None;
    }
    let lx = x - p.origin_x;
    let lz = z - p.origin_z;
    if !(0..104).contains(&lx) || !(0..104).contains(&lz) {
        return None;
    }
    let slot = ((lx & 7) << 4) | (lz & 7);
    Some((lx & !7, lz & !7, slot))
}

pub struct World {
    pub players: Vec<Option<Player>>,
    pub npcs: Vec<Option<Npc>>,
    /// Spatial index of which entities occupy each 8×8 zone.
    pub zones: crate::zone::ZoneMap,
    pub tick: u32,
    pub vars: Vec<i32>,
    pub scripts: Option<ScriptProvider>,
    /// Per-player engine queues (parallel to `players`) — delayed scripts that
    /// fire on the owning player. Engine-TS stores this on the Player; OS1 keeps
    /// it World-side so `player.rs` stays decoupled from the script types.
    engine_queues: Vec<Vec<EngineQueueEntry>>,
    /// Per-player suspended script (Engine-TS `Player.activeScript`): a script
    /// that hit P_DELAY / P_PAUSEBUTTON, paused mid-run, and resumes on the
    /// stored condition.
    suspended: Vec<Option<(ScriptState, ResumeOn)>>,
    /// World-level script queue (Engine-TS `World.queue`): [world] / WORLD_DELAY
    /// scripts with no owning player, processed at the head of every cycle.
    world_queue: Vec<WorldQueueEntry>,
    /// Per-npc suspended AI script (Engine-TS `Npc.activeScript` after NPC_DELAY):
    /// the parked `ScriptState` and the tick it resumes on.
    npc_suspended: Vec<Option<(ScriptState, u32)>>,
    /// Engine-local PRNG state (xorshift64*). Engine-TS reaches for `Math.random`
    /// in spots the game logic needs entropy — currently only the AFK-event roll.
    /// Seeded from wall-clock at construction so each boot differs.
    rng_state: u64,
}

/// How often (in ticks) the world rolls each player's AFK-event flag — Engine-TS
/// `World.AFK_EVENTRATE` (500 ticks ≈ 5 minutes).
const AFK_EVENTRATE: u32 = 500;
/// Per-roll chance while the player is still moving around — Engine-TS
/// `World.AFK_CHANCE1` (1/24 ≈ 4%, averaging one event every ~2 hours).
const AFK_CHANCE1: f64 = 1.0 / (120.0 / 5.0);
/// Per-roll chance once the player has gone "afk" in one spot — Engine-TS
/// `World.AFK_CHANCE2` (1/12 ≈ 8%, averaging one event every ~1 hour).
const AFK_CHANCE2: f64 = 1.0 / (60.0 / 5.0);

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

/// A nonzero xorshift seed derived from wall-clock nanos (xorshift requires a
/// nonzero state). Determinism doesn't matter here — only the AFK roll consumes
/// it, and tests don't run the 500-tick window.
fn seed_rng() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    nanos ^ 0x9E37_79B9_7F4A_7C15
}

impl World {
    pub fn new() -> World {
        World {
            players: (0..MAX_PLAYERS).map(|_| None).collect(),
            npcs: (0..MAX_NPCS).map(|_| None).collect(),
            zones: crate::zone::ZoneMap::default(),
            // Start with a minute of uptime in case scripts skip
            // testing 0-checks (reference World.ts does the same).
            tick: 100,
            vars: vec![0; 4000],
            scripts: None,
            engine_queues: (0..MAX_PLAYERS).map(|_| Vec::new()).collect(),
            suspended: (0..MAX_PLAYERS).map(|_| None).collect(),
            world_queue: Vec::new(),
            npc_suspended: (0..MAX_NPCS).map(|_| None).collect(),
            rng_state: seed_rng(),
        }
    }

    /// Next uniform `f64` in `[0, 1)` from the engine PRNG (xorshift64*), the
    /// `Math.random()` analogue used by the AFK-event roll.
    fn next_rand_unit(&mut self) -> f64 {
        let mut x = self.rng_state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng_state = x;
        let bits = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        // Top 53 bits → a double in [0, 1), matching JS `Math.random()` precision.
        (bits >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Run a freshly-built script and, if it suspended on P_DELAY, park it to
    /// resume once its delay elapses — 1:1 with how Engine-TS keeps a player's
    /// `activeScript` across ticks. Returns the execution result.
    fn dispatch(&mut self, mut state: ScriptState) -> crate::script::state::Execution {
        use crate::script::state::Execution;
        let result = runner::execute(&mut state, self);
        // An NPC_DELAY-suspended script parks on its npc instead of a player.
        if result == Execution::NpcSuspended {
            let resume = self.tick + 1 + state.delay.max(0) as u32;
            if let Some(nid) = state.active_npc {
                if let Some(slot) = self.npc_suspended.get_mut(nid) {
                    *slot = Some((state, resume));
                }
            }
            return result;
        }
        let resume = match result {
            Execution::Suspended => ResumeOn::Tick(self.tick + 1 + state.delay.max(0) as u32),
            Execution::PauseButton => ResumeOn::PauseButton,
            Execution::CountDialog => ResumeOn::CountDialog,
            _ => return result,
        };
        if let Some(pid) = state.active_player {
            if let Some(slot) = self.suspended.get_mut(pid) {
                *slot = Some((state, resume));
            }
        }
        result
    }

    /// Resume any npc's NPC_DELAY-suspended AI script whose lock has elapsed —
    /// 1:1 with the resume block at the head of Engine-TS `Npc.turn`. Runs before
    /// npc movement so a resumed script can set up the npc's actions this tick.
    fn resume_suspended_npc_scripts(&mut self) {
        for nid in 0..MAX_NPCS {
            let due = matches!(self.npc_suspended.get(nid).and_then(|s| s.as_ref()),
                Some((_, rt)) if self.tick >= *rt);
            if !due {
                continue;
            }
            if let Some((state, _)) = self.npc_suspended[nid].take() {
                if self.npcs[nid].is_some() {
                    self.dispatch(state);
                }
            }
        }
    }

    /// Fire due npc AI timers — 1:1 with Engine-TS `Npc.processTimers`. Each
    /// armed npc's clock ticks up; once it reaches the interval the npc's
    /// [ai_timer] trigger runs (and the clock resets only when a script exists,
    /// matching the reference). Runs before npc movement so the AI can act.
    fn process_npc_timers(&mut self) {
        let mut due: Vec<(usize, Arc<ScriptFile>)> = Vec::new();
        for nid in 0..MAX_NPCS {
            let type_id = match self.npcs[nid].as_mut() {
                Some(npc) if npc.timer_interval > 0 => {
                    npc.timer_clock += 1;
                    if npc.timer_clock < npc.timer_interval {
                        continue;
                    }
                    npc.type_id
                }
                _ => continue,
            };
            if let Some(script) = self.scripts.as_ref()
                .and_then(|s| s.get_by_trigger(trigger::AI_TIMER, type_id, -1))
            {
                if let Some(npc) = self.npcs[nid].as_mut() {
                    npc.timer_clock = 0;
                }
                due.push((nid, script));
            }
        }
        for (nid, script) in due {
            let mut state = ScriptState::new(script, &[]);
            state.active_npc = Some(nid);
            state.pointer_add(crate::script::state::Pointer::ActiveNpc);
            self.dispatch(state);
        }
    }

    /// Run due npc queue entries — 1:1 with Engine-TS `Npc.processQueue`. Each
    /// entry counts down (and fires) only while the npc isn't action-locked, so
    /// a queue script that NPC_DELAYs the npc freezes the rest of its queue; a
    /// due entry runs its [ai_queue<N>] trigger with the queued `arg` as the
    /// active value. Entries a script enqueues mid-pass are picked up next tick.
    fn process_npc_queues(&mut self) {
        for nid in 0..MAX_NPCS {
            let entries = match self.npcs[nid].as_mut() {
                Some(npc) if npc.active && !npc.queue.is_empty() => std::mem::take(&mut npc.queue),
                _ => continue,
            };
            let mut kept = Vec::with_capacity(entries.len());
            for mut req in entries {
                let delayed = self.npcs[nid].as_ref()
                    .map_or(true, |n| n.is_delayed(self.tick as i32));
                if delayed {
                    kept.push(req);
                    continue;
                }
                req.delay -= 1;
                if req.delay > 0 {
                    kept.push(req);
                    continue;
                }
                let Some(type_id) = self.npcs[nid].as_ref().map(|n| n.type_id) else { continue; };
                if let Some(script) = self.scripts.as_ref()
                    .and_then(|s| s.get_by_trigger(req.trigger, type_id, -1))
                {
                    let mut state = ScriptState::new(script, &[]);
                    state.active_npc = Some(nid);
                    state.last_int = req.arg;
                    state.pointer_add(crate::script::state::Pointer::ActiveNpc);
                    self.dispatch(state);
                }
            }
            if let Some(npc) = self.npcs[nid].as_mut() {
                kept.append(&mut npc.queue);
                npc.queue = kept;
            }
        }
    }

    /// Fire armed npc walk-triggers — 1:1 with the walktrigger check in Engine-TS
    /// `Npc.processMovementInteraction`: an npc that is about to walk (has
    /// waypoints, not action-locked) runs its armed [ai_queue<N>] script once
    /// (with the stored arg as the script's first int local), then clears it.
    /// Runs after the npc queue and before npc movement.
    fn process_npc_walktriggers(&mut self) {
        let mut due: Vec<(usize, crate::script::trigger::Trigger, i32)> = Vec::new();
        for nid in 0..MAX_NPCS {
            let Some(npc) = self.npcs[nid].as_mut() else { continue; };
            if npc.walk_trigger == -1
                || npc.entity.waypoints.is_empty()
                || npc.is_delayed(self.tick as i32)
            {
                continue;
            }
            let trig = (trigger::AI_QUEUE1 as i32 + npc.walk_trigger) as u16;
            let arg = npc.walktrigger_arg;
            npc.walk_trigger = -1;
            due.push((nid, trig, arg));
        }
        for (nid, trig, arg) in due {
            let Some(type_id) = self.npcs[nid].as_ref().map(|n| n.type_id) else { continue; };
            if let Some(script) = self.scripts.as_ref()
                .and_then(|s| s.get_by_trigger(trig, type_id, -1))
            {
                let mut state = ScriptState::new(script, &[ScriptArg::Int(arg)]);
                state.active_npc = Some(nid);
                state.pointer_add(crate::script::state::Pointer::ActiveNpc);
                self.dispatch(state);
            }
        }
    }

    /// Resume any player's suspended script whose delay has elapsed (Engine-TS
    /// resumes the active script in processPlayers). A script that suspends
    /// again is re-parked; one whose player has logged out is dropped.
    fn resume_suspended_scripts(&mut self) {
        for pid in 0..MAX_PLAYERS {
            let due = matches!(self.suspended.get(pid).and_then(|s| s.as_ref()),
                Some((_, ResumeOn::Tick(rt))) if self.tick >= *rt);
            if !due {
                continue;
            }
            if let Some((state, _)) = self.suspended[pid].take() {
                if self.players[pid].is_some() {
                    self.dispatch(state);
                }
            }
        }
    }

    /// Resume a player's script that's paused on a continue button — invoked by
    /// the RESUME_PAUSEBUTTON packet (Engine-TS ResumePauseButtonHandler).
    fn resume_pausebutton(&mut self, pid: usize) {
        let waiting = matches!(self.suspended.get(pid).and_then(|s| s.as_ref()),
            Some((_, ResumeOn::PauseButton)));
        if waiting {
            if let Some((state, _)) = self.suspended[pid].take() {
                if self.players[pid].is_some() {
                    self.dispatch(state);
                }
            }
        }
    }

    /// Resume a player's script paused on a count dialog — invoked by the
    /// RESUME_P_COUNTDIALOG packet (Engine-TS ResumePCountDialogHandler). The
    /// entered amount is exposed to the resumed script as LAST_INT.
    fn resume_countdialog(&mut self, pid: usize, value: i32) {
        let waiting = matches!(self.suspended.get(pid).and_then(|s| s.as_ref()),
            Some((_, ResumeOn::CountDialog)));
        if waiting {
            if let Some((mut state, _)) = self.suspended[pid].take() {
                if self.players[pid].is_some() {
                    state.last_int = value;
                    self.dispatch(state);
                }
            }
        }
    }

    /// Award experience and, on a level-up, fire the stat's [changestat,<stat>]
    /// then [advancestat,<stat>] engine-queue scripts — 1:1 with Engine-TS
    /// `Player.addXp` (which the Player can't do itself without World access).
    pub fn give_xp(&mut self, pid: usize, stat: i32, xp: i32) {
        if stat < 0 || stat as usize >= crate::entity::player::STAT_COUNT {
            return;
        }
        let leveled = match self.players[pid].as_mut() {
            Some(p) => p.add_xp(stat as usize, xp),
            None => return,
        };
        if !leveled {
            return;
        }
        // changeStat before advancestat, matching the order in Engine-TS addXp.
        self.fire_changestat(pid, stat);
        if let Some(s) = self.scripts.as_ref()
            .and_then(|s| s.get_by_trigger_specific(trigger::ADVANCESTAT, stat, -1))
        {
            self.enqueue_engine(pid, s, Vec::new(), 0);
        }
    }

    /// Queue the stat's [changestat,<stat>] script — 1:1 with Engine-TS
    /// `Player.changeStat`, fired whenever a stat's level changes (xp level-up
    /// or a boost/drain). No-op when no script is registered for the stat.
    pub fn fire_changestat(&mut self, pid: usize, stat: i32) {
        if let Some(s) = self.scripts.as_ref()
            .and_then(|s| s.get_by_trigger(trigger::CHANGESTAT, stat, -1))
        {
            self.enqueue_engine(pid, s, Vec::new(), 0);
        }
    }

    /// Queue `script` to run on player `pid` after `delay` ticks — 1:1 with
    /// Engine-TS `enqueueScript(.., PlayerQueueType.ENGINE)`. `delay <= 0` runs
    /// it on the next queue pass.
    pub fn enqueue_engine(&mut self, pid: usize, script: Arc<ScriptFile>,
                          args: Vec<ScriptArg>, delay: i32) {
        if let Some(q) = self.engine_queues.get_mut(pid) {
            q.push(EngineQueueEntry { script, args, delay });
        }
    }

    /// Run the player's due engine-queue scripts — 1:1 with Engine-TS
    /// `processEngineQueue`: each entry's delay post-decrements, and an entry
    /// whose (pre-decrement) delay has reached 0 runs once and is dropped.
    /// Scripts a run enqueues are appended and picked up next pass.
    fn process_engine_queue(&mut self, pid: usize) {
        // Engine-TS processEngineQueue fires only when `canAccess() && delay <= 0`:
        // a due entry blocked by an action-lock / modal stays queued and retries
        // (its delay keeps counting down) rather than firing through the lock —
        // the same protected-access gate the NORMAL/WEAK queues use.
        let can_access = self.player_can_access(pid);
        let ready: Vec<(Arc<ScriptFile>, Vec<ScriptArg>)> = {
            let Some(queue) = self.engine_queues.get_mut(pid) else { return; };
            if queue.is_empty() {
                return;
            }
            let mut ready = Vec::new();
            let mut kept = Vec::with_capacity(queue.len());
            for mut e in queue.drain(..) {
                let due = e.delay <= 0;
                e.delay -= 1;
                if due && can_access {
                    ready.push((e.script, e.args));
                } else {
                    kept.push(e);
                }
            }
            *queue = kept;
            ready
        };
        for (script, args) in ready {
            let mut state = ScriptState::new(script, &args);
            state.active_player = Some(pid);
            state.pointer_add(crate::script::state::Pointer::ActivePlayer);
            state.pointer_add(crate::script::state::Pointer::ProtectedActivePlayer);
            self.dispatch(state);
        }
    }

    /// Run a player's NORMAL/WEAK/STRONG/LONG script queues — 1:1 with Engine-TS
    /// `Player.processQueues`. A queued STRONG script first clears the weak queue
    /// (the modal-interface close it also drives awaits OS1's modal tracking),
    /// then the main queue and the weak queue each count down and fire their due
    /// entries while the player has protected access.
    fn process_queues(&mut self, pid: usize) {
        // A queued STRONG script requests a modal close before either queue runs
        // (Engine-TS processQueues sets requestModalClose on the first STRONG).
        let has_strong = self.players[pid].as_ref()
            .map_or(false, |p| p.queue.iter().any(|r| r.kind == player::QueueKind::Strong));
        if has_strong {
            if let Some(p) = self.players[pid].as_mut() {
                p.request_modal_close = true;
            }
        }
        // Consume a pending modal-close request (from a STRONG script or the
        // CLOSE_MODAL packet) — clears the latch, then closes the modal.
        let close = self.players[pid].as_ref().map_or(false, |p| p.request_modal_close);
        if close {
            if let Some(p) = self.players[pid].as_mut() {
                p.request_modal_close = false;
            }
            self.close_modal(pid);
        }
        self.process_queue(pid, false);
        self.process_queue(pid, true);
    }

    /// Grant pending logout requests — 1:1 with Engine-TS `processLogouts` (the
    /// request path; the connection/response timeouts that can *force* a logout
    /// live in the socket layer). A requested logout is granted once the world
    /// tick reaches `prevent_logout_until`; while still prevented, the player is
    /// shown the prevention message instead. The server layer reaps the player
    /// (running [logout]) once `logging_out` is set.
    fn process_logouts(&mut self) {
        let tick = self.tick as i32;
        for pid in 0..MAX_PLAYERS {
            let Some(p) = self.players[pid].as_ref() else { continue; };
            if !p.request_logout {
                continue;
            }
            // Antilog window: refuse the logout, show the message, and drop the
            // request — the player must click again once the timer elapses (1:1
            // with Engine-TS, which clears requestLogout either way).
            if tick < p.prevent_logout_until {
                if let Some(p) = self.players[pid].as_mut() {
                    if let Some(message) = p.prevent_logout_message.take() {
                        p.write(msg::message_game(&message));
                    }
                    p.request_logout = false;
                }
                continue;
            }
            // Not prevented: only log out once the player is free — Engine-TS
            // gates the logout on canAccess() and an empty engine queue, so a
            // delayed / mid-dialog player, or one with a pending engine-queued
            // action, stays until it resolves (the request persists and retries).
            // (The main-queue "discardable" gate — a LONG ^finish script blocking
            // logout — awaits the logout-acceleration interplay and isn't applied
            // here, to avoid deadlocking against it.)
            if !self.player_can_access(pid) || !self.engine_queues[pid].is_empty() {
                continue;
            }
            if let Some(p) = self.players[pid].as_mut() {
                p.logging_out = true;
                p.write(msg::logout());
                p.request_logout = false;
            }
        }
    }

    /// Close the player's open modal — 1:1 with Engine-TS `Player.closeModal`
    /// for the parts OS1 can express: it always clears the weak queue, and — the
    /// "a modal was open" branch — abandons a dialog-suspended script (a player
    /// who dismisses a dialog client-side or is pre-empted by a STRONG script
    /// drops the pause-button wait; Engine-TS nulls activeScript when its
    /// execution is PAUSEBUTTON/COUNTDIALOG). The modalState bitfield, the
    /// [if_close] triggers, and the interface-close transmission await OS1's
    /// modal-interface tracking.
    pub(crate) fn close_modal(&mut self, pid: usize) {
        if let Some(p) = self.players[pid].as_mut() {
            p.weak_queue.clear();
        }
        if matches!(self.suspended.get(pid).and_then(|s| s.as_ref()),
                    Some((_, ResumeOn::PauseButton | ResumeOn::CountDialog))) {
            if let Some(slot) = self.suspended.get_mut(pid) {
                *slot = None;
            }
        }
    }

    /// Process one of a player's queues (the weak queue when `weak`). Each entry
    /// counts down once; a due entry whose player can be accessed runs protected
    /// and is dropped — a running script that delays the player blocks the rest
    /// this tick (their delays still tick down), exactly as Engine-TS `protect`
    /// gates the loop. Entries a script enqueues mid-pass are picked up next tick
    /// (the reference's LinkList re-entrancy quirk is intentionally not copied).
    fn process_queue(&mut self, pid: usize, weak: bool) {
        let logging_out = self.players[pid].as_ref().map_or(false, |p| p.logging_out);
        // Drain the queue so scripts can enqueue fresh entries while it runs.
        let entries: Vec<player::PlayerQueueRequest> = match self.players[pid].as_mut() {
            Some(p) if weak => std::mem::take(&mut p.weak_queue),
            Some(p) => std::mem::take(&mut p.queue),
            None => return,
        };
        let mut kept = Vec::new();
        for mut req in entries {
            // Logout accelerates a LONG script flagged with logout-action 0.
            if logging_out && req.kind == player::QueueKind::Long
                && matches!(req.args.first(), Some(ScriptArg::Int(0))) {
                req.delay = 0;
            }
            let due = req.delay <= 0;
            req.delay -= 1;
            if due && self.player_can_access(pid) {
                // LONG scripts carry a leading logout-action arg the script never sees.
                let args = if req.kind == player::QueueKind::Long && !req.args.is_empty() {
                    req.args[1..].to_vec()
                } else {
                    req.args.clone()
                };
                let mut state = ScriptState::new(Arc::clone(&req.script), &args);
                state.active_player = Some(pid);
                state.pointer_add(crate::script::state::Pointer::ActivePlayer);
                state.pointer_add(crate::script::state::Pointer::ProtectedActivePlayer);
                self.dispatch(state);
            } else {
                kept.push(req);
            }
        }
        // Restore unfired entries ahead of anything enqueued during this pass.
        if let Some(p) = self.players[pid].as_mut() {
            let q = if weak { &mut p.weak_queue } else { &mut p.queue };
            if !kept.is_empty() {
                kept.append(q);
                *q = kept;
            }
        }
    }

    /// Fire the player's armed walk-trigger script — 1:1 with Engine-TS
    /// `Player.processWalktrigger`. Gated on protected access (its `!protect &&
    /// !delayed`); the trigger is cleared whether or not its script resolves, and
    /// runs protected. Called from the move-click handler once a click produces a
    /// path (the authentic PLAYERPACKET walk-trigger setting).
    fn process_walktrigger(&mut self, pid: usize) {
        let wt = match self.players[pid].as_ref() {
            Some(p) if p.walk_trigger != -1 => p.walk_trigger,
            _ => return,
        };
        if !self.player_can_access(pid) {
            return;
        }
        let script = self.scripts.as_ref().and_then(|s| s.get(wt));
        if let Some(p) = self.players[pid].as_mut() {
            p.walk_trigger = -1;
        }
        if let Some(script) = script {
            let mut state = ScriptState::new(script, &[]);
            state.active_player = Some(pid);
            state.pointer_add(crate::script::state::Pointer::ActivePlayer);
            state.pointer_add(crate::script::state::Pointer::ProtectedActivePlayer);
            self.dispatch(state);
        }
    }

    /// Whether the player has protected access this tick — 1:1 with Engine-TS
    /// `Player.canAccess` (`!protect && !busy()`). OS1 expresses "busy" as being
    /// delayed (P_DELAY) or having a script parked in `suspended` (a protected
    /// script mid-flight); the modal-interface half of `busy()` awaits the modal
    /// tracking system. NORMAL timers gate on this; SOFT timers ignore it.
    /// Whether the player is "busy" — 1:1 with Engine-TS `Player.busy()` plus the
    /// `loggingOut` BUSY check: action-locked (P_DELAY), mid-dialog (a parked
    /// pause-button / count-dialog script — OS1's stand-in for an open CHAT/MAIN
    /// modal until modal tracking lands), or logging out.
    pub(crate) fn player_busy(&self, pid: usize) -> bool {
        let Some(p) = self.players.get(pid).and_then(|o| o.as_ref()) else {
            return false;
        };
        p.is_delayed(self.tick as i32)
            || p.logging_out
            || matches!(self.suspended.get(pid).and_then(|s| s.as_ref()),
                        Some((_, ResumeOn::PauseButton | ResumeOn::CountDialog)))
    }

    pub(crate) fn player_can_access(&self, pid: usize) -> bool {
        let not_delayed = self.players[pid].as_ref()
            .map_or(false, |p| !p.is_delayed(self.tick as i32));
        not_delayed && self.suspended.get(pid).map_or(true, |s| s.is_none())
    }

    /// Run a player's due timers of one kind — 1:1 with Engine-TS
    /// `Player.processTimers`. A timer whose `clock + interval` has elapsed
    /// fires (its clock resets to now) when it's a SOFT timer or the player has
    /// protected access; NORMAL timers run protected, SOFT timers don't.
    fn process_timers(&mut self, pid: usize, kind: player::TimerKind) {
        let now = self.tick;
        let can_access = self.player_can_access(pid);
        let due: Vec<(Arc<ScriptFile>, Vec<ScriptArg>)> = {
            let Some(p) = self.players[pid].as_mut() else { return; };
            let able = kind == player::TimerKind::Soft || can_access;
            let mut due = Vec::new();
            for timer in p.timers.values_mut() {
                if timer.kind != kind {
                    continue;
                }
                if able && now as i64 >= timer.clock as i64 + timer.interval as i64 {
                    timer.clock = now;
                    due.push((Arc::clone(&timer.script), timer.args.clone()));
                }
            }
            due
        };
        for (script, args) in due {
            let mut state = ScriptState::new(script, &args);
            state.active_player = Some(pid);
            state.pointer_add(crate::script::state::Pointer::ActivePlayer);
            // NORMAL timers get protected access; SOFT timers run unprotected.
            if kind == player::TimerKind::Normal {
                state.pointer_add(crate::script::state::Pointer::ProtectedActivePlayer);
            }
            self.dispatch(state);
        }
    }

    /// Enqueue a world-level script (Engine-TS `World.enqueueScript`): it runs
    /// `delay` ticks from now at the head of a cycle. The `+1` mirrors the
    /// reference — a delay-0 script runs next cycle, not the current one.
    pub fn enqueue_world(&mut self, state: ScriptState, delay: i32) {
        self.world_queue.push(WorldQueueEntry { state, delay: delay + 1 });
    }

    /// Run the world-level script queue — 1:1 with Engine-TS `processWorld`.
    /// Each entry's delay post-decrements; one whose (pre-decrement) delay has
    /// reached 0 runs once and is dropped, unless it suspends on WORLD_DELAY,
    /// which re-queues it with the freshly-popped delay so it resumes from its
    /// program counter.
    fn process_world_queue(&mut self) {
        if self.world_queue.is_empty() {
            return;
        }
        for mut entry in std::mem::take(&mut self.world_queue) {
            let due = entry.delay <= 0;
            entry.delay -= 1;
            if !due {
                self.world_queue.push(entry);
                continue;
            }
            let mut state = entry.state;
            if runner::execute(&mut state, self)
                == crate::script::state::Execution::WorldSuspended
            {
                let delay = state.pop_int();
                self.enqueue_world(state, delay);
            }
        }
    }

    pub fn load_scripts(&mut self, dir: &str) {
        match ScriptProvider::load(dir) {
            Ok(p) => self.scripts = Some(p),
            Err(e) => eprintln!("[world] scripts not loaded: {e}"),
        }
    }

    // ── Players ───────────────────────────────────────────────────

    pub fn add_player(&mut self, username: String, x: i32, z: i32, level: i32)
        -> Option<usize>
    {
        // pid 0 and pid 2047 are reserved: 2047 is the protocol's local-player
        // sentinel, and 0 is left unused so a real player never collides with
        // the "offline / null" pid that friend/clan/uid lookups treat as empty
        // — 1:1 with Engine-TS, which allocates slots 1..2046.
        let pid = (1..MAX_PLAYERS - 1).find(|&i| self.players[i].is_none())?;
        let mut player = Player::new(pid, username, x, z, level);

        let (ox, oz) = protocol::server::build_area_origin(x, z);
        player.origin_x = ox;
        player.origin_z = oz;
        player.entity.tele = true;
        player.entity.jump = true;
        player.build_appearance();

        self.zones.enter_player(player.entity.zone_index, pid);
        self.players[pid] = Some(player);
        self.on_login(pid);
        Some(pid)
    }

    fn on_login(&mut self, pid: usize) {
        // Engine-init packets, sent unconditionally before any UI — 1:1 with
        // the head of Engine-TS `Player.onLogin`. RESET_CLIENT_VARCACHE clears
        // varps a reconnecting client may still hold from a prior session;
        // RESET_ANIMS clears stale playing animations.
        if let Some(p) = self.players[pid].as_mut() {
            p.write(msg::reset_client_var_cache());
            p.write(msg::reset_anims());
            // Default chat filters: public on (0), trade on (0).
            p.write(msg::chat_filter_settings(0, 0));
        }

        // Resync the stat block (UPDATE_STAT per skill) + combat level, then
        // the run energy — Engine-TS `onLogin` sends UpdateRunEnergy explicitly.
        // `update_energy` only emits on a percent change, so without this a
        // freshly-logged-in player (energy at max) never tells the client and
        // the run orb shows 0% instead of 100%.
        if let Some(p) = self.players[pid].as_mut() {
            p.sync_stats();
            p.write(msg::update_runenergy(p.run_energy / 100));
        }

        // RuneScript [login,_] trigger when content is loaded. The
        // reference 2005 content pack's login script hits ops this
        // runtime doesn't implement yet, so fall through to the
        // engine-level welcome/UI when the script is missing OR
        // aborts — a bare server stays usable either way.
        let script = self.scripts.as_ref()
            .and_then(|s| s.get_by_trigger(trigger::LOGIN, -1, -1));
        if let Some(script) = script {
            let mut state = ScriptState::new(script, &[]);
            state.active_player = Some(pid);
            state.pointer_add(crate::script::state::Pointer::ActivePlayer);
            state.pointer_add(crate::script::state::Pointer::ProtectedActivePlayer);
            let result = self.dispatch(state);
            if result != crate::script::state::Execution::Aborted {
                return;
            }
            eprintln!("[world] login script aborted; using engine fallback UI");
        }

        let Some(p) = self.players[pid].as_mut() else { return; };
        p.write(msg::message_game("Welcome to RuneScape."));
        p.write(msg::if_opentop(548));
        // toplevel tab/chat overlay set (Engine2007 reference layout).
        const SUBS: [(i32, i32); 15] = [
            (90, 137),  // chat
            (99, 92),   // combat
            (100, 320), // stats
            (101, 274), // quest journal
            (102, 149), // inventory
            (103, 387), // worn
            (104, 271), // prayer
            (105, 192), // magic
            (106, 589), // clan
            (107, 550), // friends
            (108, 551), // ignore
            (109, 182), // logout
            (110, 261), // options
            (111, 464), // emotes
            (112, 239), // music
        ];
        for (child, sub) in SUBS {
            p.write(msg::if_opensub((548 << 16) | child, sub, 1));
        }
    }

    pub fn remove_player(&mut self, pid: usize) {
        // Run the [logout] script while the player is still present — 1:1 with
        // Engine-TS processLogouts, which fires it before removal so it can do
        // cleanup (the mirror of the [login] trigger run in on_login).
        if self.players[pid].is_some() {
            self.run_trigger(trigger::LOGOUT, -1, -1, Some(pid), &[]);
        }
        if let Some(p) = self.players[pid].as_ref() {
            self.zones.leave_player(p.entity.zone_index, pid);
        }
        self.players[pid] = None;
        if let Some(q) = self.engine_queues.get_mut(pid) {
            q.clear();
        }
        if let Some(s) = self.suspended.get_mut(pid) {
            *s = None;
        }
    }

    // ── NPCs ──────────────────────────────────────────────────────

    pub fn add_npc(&mut self, type_id: i32, x: i32, z: i32, level: i32) -> Option<usize> {
        let nid = (0..MAX_NPCS).find(|&i| self.npcs[i].is_none())?;
        let npc = Npc::new(nid, type_id, x, z, level);
        self.zones.enter_npc(npc.entity.zone_index, nid);
        self.npcs[nid] = Some(npc);
        Some(nid)
    }

    pub fn remove_npc(&mut self, nid: usize) {
        if let Some(n) = self.npcs[nid].as_ref() {
            self.zones.leave_npc(n.entity.zone_index, nid);
        }
        self.npcs[nid] = None;
        if let Some(slot) = self.npc_suspended.get_mut(nid) {
            *slot = None;
        }
    }

    /// Player slot ids in/near the view of `(x, z, level)`, gathered from the
    /// zone index (the OSRS build-area zones) rather than a full slot scan.
    /// Sorted by id so the info-packet add order stays stable; the exact
    /// `within_view` test still narrows the candidates. Same-level only (zones
    /// are per-level).
    pub fn nearby_player_ids(&self, x: i32, z: i32, level: i32) -> Vec<usize> {
        let zr = (VIEW_DISTANCE >> 3) + 1; // tiles -> zones either side
        let mut out = Vec::new();
        for dz in -zr..=zr {
            for dx in -zr..=zr {
                let idx = crate::zone::zone_index(x + dx * 8, z + dz * 8, level);
                out.extend_from_slice(self.zones.players_in(idx));
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Npc slot ids whose zone is within `distance` tiles of `(x, z, level)` —
    /// the spatial candidate set for NPC_FINDALL (callers still apply the exact
    /// per-tile distance + type filter).
    pub fn npcs_within(&self, x: i32, z: i32, level: i32, distance: i32) -> Vec<usize> {
        let zr = (distance.max(0) >> 3) + 1;
        let mut out = Vec::new();
        for dz in -zr..=zr {
            for dx in -zr..=zr {
                let idx = crate::zone::zone_index(x + dx * 8, z + dz * 8, level);
                out.extend_from_slice(self.zones.npcs_in(idx));
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The `(shape, angle)` of a spawned/changed map object of `id` whose origin
    /// is at `(x, z, level)` (Engine-TS `World.getLoc`), or `None`. Finds dynamic
    /// loc changes; base-map locs await map loading. Backs LOC_FIND.
    pub fn find_loc(&self, x: i32, z: i32, level: i32, id: i32) -> Option<(i32, i32)> {
        let zidx = crate::zone::zone_index(x, z, level);
        self.zones.locs_in(zidx).iter()
            .find(|l| l.id == id && l.x == x && l.z == z && l.level == level)
            .map(|l| (l.shape, l.angle))
    }

    /// The count of a ground item of `id` at `(x, z, level)` visible to player
    /// `pid` (Engine-TS `World.getObj` + the visibility check), or `None` if no
    /// such item is there. Backs OBJ_FIND / OBJ_COUNT.
    pub fn find_obj(&self, x: i32, z: i32, level: i32, id: i32, pid: usize) -> Option<i32> {
        let zidx = crate::zone::zone_index(x, z, level);
        self.zones.objs_in(zidx).iter()
            .find(|o| o.id == id && o.x == x && o.z == z && o.level == level && o.visible_to(pid))
            .map(|o| o.count)
    }

    /// Remove a ground item and broadcast its disappearance to every nearby
    /// player who could see it — the OBJ_DEL path (and the same broadcast the
    /// despawn timer uses).
    pub fn remove_obj_broadcast(&mut self, x: i32, z: i32, level: i32, id: i32) {
        let Some(o) = self.zones.remove_obj(id, x, z, level) else { return; };
        for pid in self.nearby_player_ids(x, z, level) {
            if !o.visible_to(pid) {
                continue;
            }
            let Some(p) = self.players[pid].as_mut() else { continue; };
            if let Some((zx, zz, slot)) = local_zone_slot(p, x, z, level) {
                p.write(protocol::server::update_zone_partial_follows(zx, zz));
                p.write(protocol::server::obj_del(slot, id));
            }
        }
    }

    /// `(x, z, level, id, shape, angle)` of every spawned map object in the
    /// coord's zone — the candidate set for LOC_FINDALLZONE.
    pub fn locs_in_zone(&self, x: i32, z: i32, level: i32) -> Vec<(i32, i32, i32, i32, i32, i32)> {
        let zidx = crate::zone::zone_index(x, z, level);
        self.zones.locs_in(zidx).iter()
            .map(|l| (l.x, l.z, l.level, l.id, l.shape, l.angle))
            .collect()
    }

    /// `(x, z, level, id)` locators of every ground item in the coord's zone
    /// visible to player `pid` — the candidate set for OBJ_FINDALLZONE.
    pub fn objs_in_zone(&self, x: i32, z: i32, level: i32, pid: usize) -> Vec<(i32, i32, i32, i32)> {
        let zidx = crate::zone::zone_index(x, z, level);
        self.zones.objs_in(zidx).iter()
            .filter(|o| o.visible_to(pid))
            .map(|o| (o.x, o.z, o.level, o.id))
            .collect()
    }

    /// Active npc slot ids in the single 8×8 zone containing `(x, z, level)` —
    /// the candidate set for NPC_FINDALLZONE.
    pub fn npcs_in_zone(&self, x: i32, z: i32, level: i32) -> Vec<usize> {
        let zidx = crate::zone::zone_index(x, z, level);
        self.zones.npcs_in(zidx).iter()
            .filter(|&&nid| self.npcs[nid].as_ref().is_some_and(|n| n.active))
            .copied()
            .collect()
    }

    pub fn nearby_npc_ids(&self, x: i32, z: i32, level: i32) -> Vec<usize> {
        let zr = (VIEW_DISTANCE >> 3) + 1;
        let mut out = Vec::new();
        for dz in -zr..=zr {
            for dx in -zr..=zr {
                let idx = crate::zone::zone_index(x + dx * 8, z + dz * 8, level);
                out.extend_from_slice(self.zones.npcs_in(idx));
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Drop a ground item: record it in its zone and send OBJ_ADD (with the
    /// zone-base prefix) to every nearby player whose build area covers the
    /// tile. Mirrors Engine-TS's per-zone obj broadcast (immediate path; a
    /// zone rebuild re-sends existing objs to players who walk in later).
    /// Spawn a public ground item (no owner; everyone nearby sees it now).
    /// Current tile of an interaction target encoded the way `set_face_entity`
    /// stores it: `slot + 32768` for a player, a raw nid for an npc.
    fn target_tile(&self, target: i32) -> Option<(i32, i32)> {
        if target >= 32768 {
            let slot = (target - 32768) as usize;
            self.players.get(slot)?.as_ref().map(|p| (p.entity.x, p.entity.z))
        } else if target >= 0 {
            let n = self.npcs.get(target as usize)?.as_ref()?;
            n.active.then_some((n.entity.x, n.entity.z))
        } else {
            None
        }
    }

    /// Turn every entity holding an interaction target toward that target's
    /// *current* tile (Engine-TS `reorient`). Only the persistent orientation
    /// (`face_angle`) is updated — the FACE_ENTITY mask was already sent when
    /// the interaction began, so no packet is re-emitted; this just keeps a
    /// new observer's view of the facing correct as the target moves.
    fn reorient_entities(&mut self) {
        // (is_player, index, face_x, face_z, clear_coord) — resolved first to
        // avoid borrowing two entities at once. `clear_coord` marks branch 2
        // (coord target), which consumes `target_x/target_z` after facing.
        let mut faces: Vec<(bool, usize, i32, i32, bool)> = Vec::new();
        // Engine-TS `reorient`: branch 1 re-faces a pathing-entity target's
        // *current* tile (it moves); branch 2 re-faces the stored tile of a
        // non-pathing target (a loc/obj/ground click) once the entity has
        // stopped (`stepsTaken === 0`), then forgets it. The two are mutually
        // exclusive — a held entity target suppresses the coord refocus.
        for pid in 0..self.players.len() {
            if let Some(p) = self.players[pid].as_ref() {
                let e = &p.entity;
                if e.target != -1 {
                    if let Some((x, z)) = self.target_tile(e.target) {
                        faces.push((true, pid, x, z, false));
                    }
                } else if e.target_x != -1 && e.steps_taken == 0 {
                    faces.push((true, pid, e.target_x, e.target_z, true));
                }
            }
        }
        for nid in 0..self.npcs.len() {
            if let Some(n) = self.npcs[nid].as_ref() {
                let e = &n.entity;
                if e.target != -1 {
                    if let Some((x, z)) = self.target_tile(e.target) {
                        faces.push((false, nid, x, z, false));
                    }
                } else if e.target_x != -1 && e.steps_taken == 0 {
                    faces.push((false, nid, e.target_x, e.target_z, true));
                }
            }
        }
        for (is_player, idx, x, z, clear_coord) in faces {
            let entity = if is_player {
                self.players[idx].as_mut().map(|p| &mut p.entity)
            } else {
                self.npcs[idx].as_mut().map(|n| &mut n.entity)
            };
            if let Some(e) = entity {
                e.face_toward(x, z);
                if clear_coord {
                    e.target_x = -1;
                    e.target_z = -1;
                }
            }
        }
    }

    /// Resolve a player uid to its slot, rejecting a stale uid whose slot now
    /// holds a different account — 1:1 with Engine-TS `getPlayerByUid` (the
    /// 21-bit Base37 name hash must still match).
    pub fn get_player_by_uid(&self, uid: i32) -> Option<usize> {
        let slot = (uid & 0x7ff) as usize;
        let hash = (uid >> 11) & 0x1f_ffff;
        let p = self.players.get(slot)?.as_ref()?;
        let phash = (crate::base37::to_base37(&p.username) & 0x1f_ffff) as i32;
        (phash == hash).then_some(slot)
    }

    /// Resolve an npc uid (`(type << 16) | slot`) to its slot — 1:1 with
    /// Engine-TS `getNpcByUid`. The type in the high bits must match the npc
    /// currently in that slot, so a stale uid (the npc despawned and the slot
    /// was reused by a different type, or the npc changed type) resolves to
    /// `None` instead of silently homing on the new occupant.
    pub fn get_npc_by_uid(&self, uid: i32) -> Option<usize> {
        let slot = (uid & 0xffff) as usize;
        let type_id = (uid >> 16) & 0xffff;
        let n = self.npcs.get(slot)?.as_ref()?;
        (n.active && n.type_id == type_id).then_some(slot)
    }

    pub fn drop_obj(&mut self, id: i32, count: i32, x: i32, z: i32, level: i32) {
        self.spawn_obj(id, count, x, z, level, -1, crate::zone::OBJ_DESPAWN_TICKS);
    }

    /// Drop an item owned by `receiver` (a player drop): private to that player
    /// for OBJ_REVEAL_TICKS, then it goes public. `receiver = -1` is public now.
    pub fn drop_obj_owned(&mut self, id: i32, count: i32, x: i32, z: i32, level: i32, receiver: i32) {
        self.spawn_obj(id, count, x, z, level, receiver, crate::zone::OBJ_DESPAWN_TICKS);
    }

    /// Drop a ground item with an explicit despawn duration — the OBJ_ADD path
    /// (owned by `receiver`, or public if -1).
    pub fn add_ground_obj(&mut self, id: i32, count: i32, x: i32, z: i32, level: i32,
                          receiver: i32, despawn: i32) {
        self.spawn_obj(id, count, x, z, level, receiver, despawn);
    }

    fn spawn_obj(&mut self, id: i32, count: i32, x: i32, z: i32, level: i32, receiver: i32,
                 despawn: i32) {
        let reveal = if receiver >= 0 { crate::zone::OBJ_REVEAL_TICKS } else { -1 };
        self.zones.add_obj(crate::zone::Obj {
            id, count, x, z, level, receiver, reveal, despawn,
        });
        for pid in self.nearby_player_ids(x, z, level) {
            // Private drop: only the owner gets the immediate OBJ_ADD.
            if receiver >= 0 && receiver != pid as i32 {
                continue;
            }
            let Some(p) = self.players[pid].as_mut() else { continue; };
            if let Some((zx, zz, slot)) = local_zone_slot(p, x, z, level) {
                p.write(protocol::server::update_zone_partial_follows(zx, zz));
                p.write(protocol::server::obj_add(slot, count, id));
            }
        }
    }

    /// Restack a ground item in place and broadcast OBJ_COUNT — 1:1 with
    /// Engine-TS's stack-change zone event. Matches the pile by id + current
    /// count; the update reaches only players who can see the item (a private
    /// drop stays private). Returns whether a matching pile was found.
    pub fn change_obj_count(
        &mut self,
        id: i32,
        x: i32,
        z: i32,
        level: i32,
        old_count: i32,
        new_count: i32,
    ) -> bool {
        let Some((receiver, reveal)) =
            self.zones.update_obj_count(id, x, z, level, old_count, new_count)
        else {
            return false;
        };
        let public = reveal < 0;
        for pid in self.nearby_player_ids(x, z, level) {
            if !public && receiver != pid as i32 {
                continue;
            }
            let Some(p) = self.players[pid].as_mut() else { continue; };
            if let Some((zx, zz, slot)) = local_zone_slot(p, x, z, level) {
                p.write(protocol::server::update_zone_partial_follows(zx, zz));
                p.write(protocol::server::obj_count(slot, id, old_count, new_count));
            }
        }
        true
    }

    /// Broadcast a loc animation (LOC_ANIM) to nearby players — plays `seq` on
    /// the loc already standing at the tile (shape/angle identify which).
    pub fn loc_anim(&mut self, seq: i32, shape: i32, angle: i32, x: i32, z: i32, level: i32) {
        for pid in self.nearby_player_ids(x, z, level) {
            let Some(p) = self.players[pid].as_mut() else { continue; };
            if let Some((zx, zz, slot)) = local_zone_slot(p, x, z, level) {
                p.write(protocol::server::update_zone_partial_follows(zx, zz));
                p.write(protocol::server::loc_anim(slot, seq, shape, angle));
            }
        }
    }

    /// Broadcast a one-shot tile spotanim (MAP_ANIM) to every nearby player —
    /// a transient FX (e.g. a spell splash), not stored in the zone.
    pub fn map_anim(&mut self, spotanim: i32, height: i32, delay: i32, x: i32, z: i32, level: i32) {
        for pid in self.nearby_player_ids(x, z, level) {
            let Some(p) = self.players[pid].as_mut() else { continue; };
            if let Some((zx, zz, slot)) = local_zone_slot(p, x, z, level) {
                p.write(protocol::server::update_zone_partial_follows(zx, zz));
                p.write(protocol::server::map_anim(slot, spotanim, height, delay));
            }
        }
    }

    /// Broadcast a projectile (MAP_PROJANIM) from `(src_*)` to `(dst_*)` — or
    /// homing on entity `target` (-1 = none) — to every nearby player. The
    /// packed slot and zone base derive from the source tile; the destination
    /// rides as a signed tile delta.
    #[allow(clippy::too_many_arguments)]
    pub fn map_projanim(
        &mut self,
        spotanim: i32,
        src_x: i32,
        src_z: i32,
        dst_x: i32,
        dst_z: i32,
        target: i32,
        src_height: i32,
        dst_height: i32,
        start_delay: i32,
        end_delay: i32,
        peak: i32,
        arc: i32,
        level: i32,
    ) {
        let (dx, dz) = (dst_x - src_x, dst_z - src_z);
        for pid in self.nearby_player_ids(src_x, src_z, level) {
            let Some(p) = self.players[pid].as_mut() else { continue; };
            if let Some((zx, zz, slot)) = local_zone_slot(p, src_x, src_z, level) {
                p.write(protocol::server::update_zone_partial_follows(zx, zz));
                p.write(protocol::server::map_projanim(
                    slot, dx, dz, target, spotanim, src_height, dst_height, start_delay,
                    end_delay, peak, arc,
                ));
            }
        }
    }

    /// Pick up / remove a ground item: drop it from its zone and broadcast
    /// OBJ_DEL to nearby players. Returns the removed item (None if absent).
    pub fn take_obj(&mut self, id: i32, x: i32, z: i32, level: i32) -> Option<crate::zone::Obj> {
        let obj = self.zones.remove_obj(id, x, z, level)?;
        for pid in self.nearby_player_ids(x, z, level) {
            let Some(p) = self.players[pid].as_mut() else { continue; };
            if let Some((zx, zz, slot)) = local_zone_slot(p, x, z, level) {
                p.write(protocol::server::update_zone_partial_follows(zx, zz));
                p.write(protocol::server::obj_del(slot, id));
            }
        }
        Some(obj)
    }

    /// Re-send every ground item in a player's build area — the OSRS
    /// zone-rebuild path, used after a REBUILD_NORMAL so items already on the
    /// ground appear for a player who has just (re)loaded the area.
    pub fn send_zone_objs(&mut self, pid: usize) {
        let Some(p) = self.players[pid].as_ref() else { return; };
        let (x, z, level) = (p.entity.x, p.entity.z, p.entity.level);
        let zr = (VIEW_DISTANCE >> 3) + 1;
        let mut objs: Vec<crate::zone::Obj> = Vec::new();
        for dz in -zr..=zr {
            for dx in -zr..=zr {
                let idx = crate::zone::zone_index(x + dx * 8, z + dz * 8, level);
                objs.extend_from_slice(self.zones.objs_in(idx));
            }
        }
        let Some(p) = self.players[pid].as_mut() else { return; };
        for o in objs {
            if !o.visible_to(pid) {
                continue; // someone else's still-private drop
            }
            if let Some((zx, zz, slot)) = local_zone_slot(p, o.x, o.z, o.level) {
                p.write(protocol::server::update_zone_partial_follows(zx, zz));
                p.write(protocol::server::obj_add(slot, o.count, o.id));
            }
        }
    }

    /// Advance ground-item lifecycle timers: reveal private drops (OBJ_REVEAL
    /// to non-owners) and despawn expired ones (OBJ_DEL). Mirrors Engine-TS
    /// `Obj.turn`.
    pub fn process_zone_objs(&mut self) {
        let (revealed, despawned) = self.zones.tick_objs();
        for o in revealed {
            for pid in self.nearby_player_ids(o.x, o.z, o.level) {
                let Some(p) = self.players[pid].as_mut() else { continue; };
                if let Some((zx, zz, slot)) = local_zone_slot(p, o.x, o.z, o.level) {
                    p.write(protocol::server::update_zone_partial_follows(zx, zz));
                    p.write(protocol::server::obj_reveal(slot, o.receiver, o.count, o.id));
                }
            }
        }
        for o in despawned {
            for pid in self.nearby_player_ids(o.x, o.z, o.level) {
                if !o.visible_to(pid) {
                    continue;
                }
                let Some(p) = self.players[pid].as_mut() else { continue; };
                if let Some((zx, zz, slot)) = local_zone_slot(p, o.x, o.z, o.level) {
                    p.write(protocol::server::update_zone_partial_follows(zx, zz));
                    p.write(protocol::server::obj_del(slot, o.id));
                }
            }
        }
    }

    /// Spawn or retype a map object permanently (until explicitly removed).
    pub fn add_loc(&mut self, id: i32, shape: i32, angle: i32, x: i32, z: i32, level: i32) {
        self.spawn_loc(id, shape, angle, x, z, level, -1);
    }

    /// Spawn/retype a map object that reverts (LOC_DEL) after `duration` ticks.
    pub fn add_loc_timed(&mut self, id: i32, shape: i32, angle: i32, x: i32, z: i32, level: i32, duration: i32) {
        self.spawn_loc(id, shape, angle, x, z, level, duration);
    }

    fn spawn_loc(&mut self, id: i32, shape: i32, angle: i32, x: i32, z: i32, level: i32, despawn: i32) {
        self.zones.add_loc(crate::zone::Loc { id, shape, angle, x, z, level, despawn });
        for pid in self.nearby_player_ids(x, z, level) {
            let Some(p) = self.players[pid].as_mut() else { continue; };
            if let Some((zx, zz, slot)) = local_zone_slot(p, x, z, level) {
                p.write(protocol::server::update_zone_partial_follows(zx, zz));
                p.write(protocol::server::loc_add_change(slot, id, shape, angle));
            }
        }
    }

    /// Advance timed loc changes: expire ones whose duration elapsed (LOC_DEL).
    pub fn process_zone_locs(&mut self) {
        for l in self.zones.tick_locs() {
            for pid in self.nearby_player_ids(l.x, l.z, l.level) {
                let Some(p) = self.players[pid].as_mut() else { continue; };
                if let Some((zx, zz, slot)) = local_zone_slot(p, l.x, l.z, l.level) {
                    p.write(protocol::server::update_zone_partial_follows(zx, zz));
                    p.write(protocol::server::loc_del(slot, l.shape, l.angle));
                }
            }
        }
    }

    /// Remove a map object (the loc change on a tile/shape) and broadcast
    /// LOC_DEL to nearby players. Returns the removed change.
    pub fn del_loc(&mut self, x: i32, z: i32, level: i32, shape: i32) -> Option<crate::zone::Loc> {
        let loc = self.zones.remove_loc(x, z, level, shape)?;
        for pid in self.nearby_player_ids(x, z, level) {
            let Some(p) = self.players[pid].as_mut() else { continue; };
            if let Some((zx, zz, slot)) = local_zone_slot(p, x, z, level) {
                p.write(protocol::server::update_zone_partial_follows(zx, zz));
                p.write(protocol::server::loc_del(slot, loc.shape, loc.angle));
            }
        }
        Some(loc)
    }

    /// Retype the map object on a tile/shape, keeping its angle, and broadcast
    /// the change (LOC_CHANGE). Reverts after `despawn` ticks (-1 = permanent).
    pub fn change_loc(&mut self, x: i32, z: i32, level: i32, shape: i32, new_id: i32, despawn: i32) {
        let Some(old) = self.zones.remove_loc(x, z, level, shape) else { return; };
        self.spawn_loc(new_id, shape, old.angle, x, z, level, despawn);
    }

    /// Play animation `seq` on the map object at a tile/shape (LOC_ANIM).
    pub fn anim_loc(&mut self, x: i32, z: i32, level: i32, shape: i32, angle: i32, seq: i32) {
        for pid in self.nearby_player_ids(x, z, level) {
            let Some(p) = self.players[pid].as_mut() else { continue; };
            if let Some((zx, zz, slot)) = local_zone_slot(p, x, z, level) {
                p.write(protocol::server::update_zone_partial_follows(zx, zz));
                p.write(protocol::server::loc_anim(slot, seq, shape, angle));
            }
        }
    }

    /// Re-send every loc change in a player's build area (the rebuild path).
    pub fn send_zone_locs(&mut self, pid: usize) {
        let Some(p) = self.players[pid].as_ref() else { return; };
        let (x, z, level) = (p.entity.x, p.entity.z, p.entity.level);
        let zr = (VIEW_DISTANCE >> 3) + 1;
        let mut locs: Vec<crate::zone::Loc> = Vec::new();
        for dz in -zr..=zr {
            for dx in -zr..=zr {
                let idx = crate::zone::zone_index(x + dx * 8, z + dz * 8, level);
                locs.extend_from_slice(self.zones.locs_in(idx));
            }
        }
        let Some(p) = self.players[pid].as_mut() else { return; };
        for l in locs {
            if let Some((zx, zz, slot)) = local_zone_slot(p, l.x, l.z, l.level) {
                p.write(protocol::server::update_zone_partial_follows(zx, zz));
                p.write(protocol::server::loc_add_change(slot, l.id, l.shape, l.angle));
            }
        }
    }

    // ── Input ─────────────────────────────────────────────────────

    pub fn handle_message(&mut self, pid: usize, message: ClientMessage) {
        match message {
            ClientMessage::MoveClick { route, ctrl_held } => {
                let tick = self.tick as i32;
                let mut walked = false;
                if let Some(p) = self.players[pid].as_mut() {
                    // An action-locked (P_DELAY) player can't walk — Engine-TS
                    // just clears the client's map flag and ignores the click.
                    if p.is_delayed(tick) {
                        p.unset_map_flag();
                        return;
                    }
                    // Don't trust the client's coords: reject a click whose
                    // first waypoint is outside the loaded scene (Engine-TS
                    // rejects distanceToSW(player, start) > 104). On reject,
                    // clear any queued walk and the client's minimap flag.
                    let in_scene = route.first().is_some_and(|&(sx, sz)| {
                        (sx - p.entity.x).abs().max((sz - p.entity.z).abs()) <= 104
                    });
                    if !in_scene {
                        p.unset_map_flag();
                    } else {
                        // Ctrl-click runs this one route; ctrl-run needs at least
                        // 1% energy (>= 100 on the 0..10000 scale) — 1:1 with
                        // Engine-TS's `if (runenergy < 100 && ctrlHeld) tempRun =
                        // 0`, consistent with `update_energy`'s own < 100 cancel.
                        p.temp_run = ctrl_held && p.run_energy >= 100;
                        p.entity.queue_waypoints(&route);
                        walked = !p.entity.waypoints.is_empty();
                    }
                }
                // PLAYERPACKET walk-trigger: a move-click that produced a path
                // fires the player's armed walktrigger (Engine-TS MoveClickHandler
                // `if (hasWaypoints()) processWalktrigger()`).
                if walked {
                    self.process_walktrigger(pid);
                }
            }
            ClientMessage::ClientCheat { command } => {
                self.handle_cheat(pid, &command);
            }
            ClientMessage::IfButton { component, .. } => {
                // Record the clicked component (Engine-TS sets player.lastCom in
                // its IfButton handler) so the script can read it back via
                // LAST_COM, then route the click to its [if_button, <component>]
                // script. The component is the packed (interface<<16)|child the
                // script is keyed on (e.g. the welcome screen's "click to play").
                if let Some(p) = self.players[pid].as_mut() {
                    p.last_com = component;
                }
                // A click on a registered resume button continues a paused
                // pause-button dialog instead of firing the component's
                // [if_button] trigger — 1:1 with Engine-TS IfButtonHandler.
                let is_resume = self.players[pid].as_ref()
                    .map_or(false, |p| p.resume_buttons.contains(&component));
                if is_resume {
                    self.resume_pausebutton(pid);
                } else {
                    self.run_trigger(trigger::IF_BUTTON, component, -1, Some(pid), &[]);
                }
            }
            ClientMessage::MessagePublic { colour, effect, message } => {
                // Validate against the rev1 client's ranges + the 100-byte cap +
                // the one-per-tick latch — 1:1 with Engine-TS MessagePublicHandler.
                // (The WordEnc profanity filter awaits the cache wordenc data; the
                // already-packed bytes are re-broadcast verbatim.)
                if let Some(p) = self.players[pid].as_mut() {
                    if p.social_protect
                        || !(0..=11).contains(&colour)
                        || !(0..=5).contains(&effect)
                        || message.len() > 100
                    {
                        return;
                    }
                    p.chat_colour = colour;
                    p.chat_effect = effect;
                    // chatRights = min(staffModLevel, 2) — the mod/admin crown shown
                    // beside the message (1:1 with Engine-TS MessagePublicHandler).
                    p.chat_rights = p.staff_mod_level.min(2);
                    p.chat_message = message;
                    p.entity.masks |= crate::entity::player::MASK_PUBLIC_CHAT;
                    p.social_protect = true;
                }
            }
            ClientMessage::CloseModal => {
                // The client dismissed the interface — request a deferred modal
                // close (Engine-TS CloseModalHandler sets requestModalClose, not
                // an immediate close, so same-tick ordering stays authentic).
                if let Some(p) = self.players[pid].as_mut() {
                    p.request_modal_close = true;
                }
            }
            ClientMessage::ResumeCountDialog { value } => {
                // The player submitted an amount — resume the count-dialog script.
                self.resume_countdialog(pid, value);
            }
            ClientMessage::ResumePauseButton { .. } => {
                // Continue a dialog paused on P_PAUSEBUTTON.
                self.resume_pausebutton(pid);
            }
            ClientMessage::NoOp => {}
        }
    }

    fn handle_cheat(&mut self, pid: usize, command: &str) {
        let parts: Vec<&str> = command.split_whitespace().collect();
        let Some(&name) = parts.first() else { return; };
        let arg = |i: usize| parts.get(i).and_then(|s| s.parse::<i32>().ok());

        match name {
            "tele" => {
                if let (Some(x), Some(z)) = (arg(1), arg(2)) {
                    let level = arg(3).unwrap_or(0);
                    if let Some(p) = self.players[pid].as_mut() {
                        p.entity.teleport(x, z, level, true);
                    }
                }
            }
            "anim" => {
                if let (Some(id), Some(p)) = (arg(1), self.players[pid].as_mut()) {
                    p.entity.anim_id = id;
                    p.entity.anim_delay = 0;
                    p.entity.masks |= crate::entity::player::MASK_ANIM;
                }
            }
            "spot" => {
                if let (Some(id), Some(p)) = (arg(1), self.players[pid].as_mut()) {
                    p.entity.spotanim_id = id;
                    p.entity.spotanim_height = arg(2).unwrap_or(0);
                    p.entity.spotanim_delay = 0;
                    p.entity.masks |= crate::entity::player::MASK_SPOTANIM;
                }
            }
            "npc" => {
                if let Some(id) = arg(1) {
                    let pos = self.players[pid].as_ref()
                        .map(|p| (p.entity.x, p.entity.z, p.entity.level));
                    if let Some((x, z, level)) = pos {
                        self.add_npc(id, x, z, level);
                    }
                }
            }
            "proc" => {
                // ::proc <script_name> — run a named proc on self.
                if let Some(&script_name) = parts.get(1) {
                    let script = self.scripts.as_ref()
                        .and_then(|s| s.get_by_name(script_name));
                    match script {
                        Some(script) => {
                            let mut state = ScriptState::new(script, &[]);
                            state.active_player = Some(pid);
                            state.pointer_add(crate::script::state::Pointer::ActivePlayer);
                            state.pointer_add(
                                crate::script::state::Pointer::ProtectedActivePlayer);
                            self.dispatch(state);
                        }
                        None => {
                            if let Some(p) = self.players[pid].as_mut() {
                                p.write(msg::message_game(
                                    &format!("no such script: {script_name}")));
                            }
                        }
                    }
                }
            }
            _ => {
                if let Some(p) = self.players[pid].as_mut() {
                    p.write(msg::message_game(&format!("unknown command: {name}")));
                }
            }
        }
    }

    /// Run a script by trigger against a player (op handlers etc.).
    pub fn run_trigger(&mut self, t: trigger::Trigger, type_id: i32, category: i32,
                       pid: Option<usize>, args: &[ScriptArg]) -> bool {
        let script = self.scripts.as_ref()
            .and_then(|s| s.get_by_trigger(t, type_id, category));
        let Some(script) = script else { return false; };
        let mut state = ScriptState::new(script, args);
        if let Some(pid) = pid {
            state.active_player = Some(pid);
            state.pointer_add(crate::script::state::Pointer::ActivePlayer);
            state.pointer_add(crate::script::state::Pointer::ProtectedActivePlayer);
        }
        self.dispatch(state);
        true
    }

    // ── Cycle ─────────────────────────────────────────────────────

    /// One 600ms world tick. The network layer feeds decoded client
    /// messages in via [`World::handle_message`] before calling this,
    /// and drains every player's `out` queue after.
    pub fn cycle(&mut self) {
        // Client-input head (Engine-TS processClientsIn): every AFK_EVENTRATE
        // ticks, re-roll each player's random-event flag — the odds double once
        // they've gone "afk" in one spot. Content reads the flag via AFK_EVENT.
        if self.tick % AFK_EVENTRATE == 0 {
            for pid in 0..MAX_PLAYERS {
                let afk = match self.players[pid].as_ref() {
                    Some(p) => p.zones_afk(),
                    None => continue,
                };
                let chance = if afk { AFK_CHANCE2 } else { AFK_CHANCE1 };
                let ready = self.next_rand_unit() < chance;
                if let Some(p) = self.players[pid].as_mut() {
                    p.afk_event_ready = ready;
                }
            }
        }

        // World-level script queue runs first (Engine-TS processWorld is the
        // head of the cycle, before client input / npcs / players).
        self.process_world_queue();

        // Zone lifecycle: ground-item reveal/despawn + timed loc reverts.
        self.process_zone_objs();
        self.process_zone_locs();

        // Resume any NPC_DELAY-suspended AI scripts whose lock elapsed (the head
        // of Engine-TS Npc.turn), then fire due AI timers — both before npc
        // movement so a resumed/fired script can set up the npc's actions. (The
        // reference runs timers just after regen; OS1 runs them a step earlier,
        // before the movement loop's regen — a negligible one-pulse stat lag.)
        self.resume_suspended_npc_scripts();
        self.process_npc_timers();
        self.process_npc_queues();
        self.process_npc_walktriggers();

        // Process movement, collecting zone-boundary crossings to apply to
        // the spatial index after (can't mutate self.zones mid-iteration).
        let npc_tick = self.tick as i32;
        let mut npc_moves: Vec<(usize, i32, i32)> = Vec::new();
        for nid in 0..self.npcs.len() {
            let Some(npc) = self.npcs[nid].as_mut() else { continue; };
            npc.process_lifecycle();
            npc.process_regen();
            // Movement is gated exactly as Engine-TS `processMovementInteraction`
            // (early-returns on `delayed || !isActive`): a NPC_DELAY-locked or
            // inactive npc holds still even with waypoints queued. Regen/timers/
            // queues above still run while delayed.
            if npc.active && !npc.is_delayed(npc_tick) {
                npc.update_movement();
                // Stamp the arrival tick on a step (Engine-TS Npc.turn sets
                // `lastMovement = currentTick + 1` for NPC_ARRIVEDELAY).
                if npc.entity.steps_taken > 0 {
                    npc.entity.last_movement = npc_tick + 1;
                }
            }
            npc.entity.validate_distance_walked();
            let new_idx = crate::zone::zone_index(npc.entity.x, npc.entity.z, npc.entity.level);
            if new_idx != npc.entity.zone_index {
                npc_moves.push((nid, npc.entity.zone_index, new_idx));
                npc.entity.zone_index = new_idx;
            }
        }
        for (nid, from, to) in npc_moves {
            self.zones.move_npc(nid, from, to);
        }

        // Resume P_DELAY-suspended scripts whose lock elapsed, then fire the
        // delayed engine-queue scripts — both before movement, matching
        // Engine-TS processPlayers.
        self.resume_suspended_scripts();
        for pid in 0..MAX_PLAYERS {
            if self.players[pid].is_some() {
                // Engine-TS processPlayers order: queues, then timers (NORMAL
                // then SOFT), then the engine queue — after the resumed script.
                self.process_queues(pid);
                self.process_timers(pid, player::TimerKind::Normal);
                self.process_timers(pid, player::TimerKind::Soft);
                self.process_engine_queue(pid);
            }
        }

        let mut player_moves: Vec<(usize, i32, i32)> = Vec::new();
        let mut rebuilt: Vec<usize> = Vec::new();
        let tick = self.tick as i32;
        for pid in 0..MAX_PLAYERS {
            let Some(player) = self.players[pid].as_mut() else { continue; };
            // Pick walk/run from the run + temp-run flags, then step.
            player.update_movement();
            // Stamp the arrival tick when a step was taken (Engine-TS updateMovement
            // sets `lastMovement = currentTick + 1` for P_ARRIVEDELAY).
            if player.entity.steps_taken > 0 {
                player.entity.last_movement = tick + 1;
            }

            // Run energy drains/recovers from this tick's step count (a delayed
            // player is skipped inside update_energy).
            player.update_energy(tick);

            // Flush this tick's stat changes in one batch (Engine-TS runs
            // updateStats just before updateAfkZones in the same player loop), so
            // multiple stat mutations from this tick's scripts collapse to one
            // packet per skill instead of one per mutation.
            player.update_stats();

            // Re-anchor the AFK zone from this tick's final position — Engine-TS
            // runs updateAfkZones in the post-movement player loop, so the next
            // AFK roll sees whether the player has held still.
            player.update_afk_zones();

            // A move larger than a run snaps on the client (exact-move already
            // handles its own snap, so skip the check there).
            if player.entity.masks & crate::entity::player::MASK_EXACT_MOVE == 0 {
                player.entity.validate_distance_walked();
            }

            let new_idx = crate::zone::zone_index(player.entity.x, player.entity.z, player.entity.level);
            if new_idx != player.entity.zone_index {
                player_moves.push((pid, player.entity.zone_index, new_idx));
                player.entity.zone_index = new_idx;
            }

            // Build-area edge → recentre the client's map.
            if player.needs_rebuild() {
                let (x, z) = (player.entity.x, player.entity.z);
                let (ox, oz) = protocol::server::build_area_origin(x, z);
                player.origin_x = ox;
                player.origin_z = oz;
                player.write(msg::rebuild_normal(x, z, |_, _| [0; 4]));
                rebuilt.push(pid);
            }
        }
        for (pid, from, to) in player_moves {
            self.zones.move_player(pid, from, to);
        }
        // After a rebuild the client's map is fresh — re-send the ground items
        // and loc changes in the new build area so existing world state appears.
        for pid in rebuilt {
            self.send_zone_objs(pid);
            self.send_zone_locs(pid);
        }

        // Grant any pending logout requests — 1:1 with Engine-TS processLogouts,
        // run after the player phase and before info is built.
        self.process_logouts();

        // Re-face interaction targets that moved this tick — 1:1 with Engine-TS
        // `reorient` (run in processInfo, before the info packets) so a newly
        // observing client sees each entity turned toward its (moving) target.
        self.reorient_entities();

        // Build per-observer info packets. The builders only read
        // world state, so snapshot-free split borrows work per pid.
        for pid in 0..MAX_PLAYERS {
            if self.players[pid].is_none() {
                continue;
            }
            let player_info = info::build_player_info(self, pid);
            let npc_info = info::build_npc_info(self, pid);
            let p = self.players[pid].as_mut().unwrap();
            p.write(player_info);
            p.write(npc_info);
        }

        // End-of-cycle transient reset (also releases a teleport's INSTANT and
        // re-arms the one-chat-per-tick latch — see Player::reset_transient).
        for slot in self.players.iter_mut() {
            if let Some(p) = slot {
                p.reset_transient();
            }
        }
        for slot in self.npcs.iter_mut() {
            if let Some(n) = slot {
                n.reset_transient();
            }
        }

        self.tick += 1;
    }
}

#[cfg(test)]
mod reorient_tests {
    use super::*;

    fn midi_script(song: i32) -> Arc<crate::script::file::ScriptFile> {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::PUSH_CONSTANT_INT, op::MIDI_SONG, op::RETURN],
            int_operands: vec![song, 0, 0],
            string_operands: vec![None; 3],
        })
    }

    fn npc_add_script(coord: i32, type_id: i32) -> Arc<crate::script::file::ScriptFile> {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT,
                          op::PUSH_CONSTANT_INT, op::NPC_ADD, op::RETURN],
            int_operands: vec![coord, type_id, 0, 0, 0],
            string_operands: vec![None; 5],
        })
    }

    #[test]
    fn engine_queue_waits_for_protected_access() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        // PUSH 55, MIDI_SONG, RETURN — emits packet 211 when it runs.
        let script = Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::PUSH_CONSTANT_INT, op::MIDI_SONG, op::RETURN],
            int_operands: vec![55, 0, 0],
            string_operands: vec![None; 3],
        });
        world.enqueue_engine(pid, script, vec![], 0);

        // Action-lock the player so they can't access this tick.
        world.players[pid].as_mut().unwrap().delayed_until = world.tick as i32 + 2;
        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle();
        assert!(world.players[pid].as_ref().unwrap().out.iter().all(|m| m.opcode != 211),
                "an engine-queue script doesn't fire through an action-lock");

        // Release the lock — the still-queued entry fires next tick.
        world.players[pid].as_mut().unwrap().delayed_until = -1;
        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle();
        assert!(world.players[pid].as_ref().unwrap().out.iter().any(|m| m.opcode == 211),
                "once unlocked the queued script runs");
    }

    #[test]
    fn p_delay_suspends_then_resumes_the_script() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        // PUSH 0, P_DELAY, PUSH 55, MIDI_SONG, RETURN — suspends at P_DELAY, so
        // MIDI_SONG (packet 211) only fires after the script resumes.
        let script = Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::PUSH_CONSTANT_INT, op::P_DELAY, op::PUSH_CONSTANT_INT,
                          op::MIDI_SONG, op::RETURN],
            int_operands: vec![0, 0, 55, 0, 0],
            string_operands: vec![None; 5],
        });
        world.enqueue_engine(pid, script, vec![], 0);

        let t0 = world.tick as i32;
        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle(); // runs the script -> suspends at P_DELAY
        assert!(world.players[pid].as_ref().unwrap().out.iter().all(|m| m.opcode != 211),
                "script paused before MIDI_SONG");
        assert!(world.players[pid].as_ref().unwrap().is_delayed(t0),
                "player action-locked during the delay");

        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle(); // resume -> MIDI_SONG runs
        assert!(world.players[pid].as_ref().unwrap().out.iter().any(|m| m.opcode == 211),
                "script resumed and finished after the delay");
    }

    #[test]
    fn p_pausebutton_waits_for_the_continue_click() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        use protocol::client::ClientMessage;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        // P_PAUSEBUTTON, PUSH 55, MIDI_SONG, RETURN — pauses, then plays midi
        // (packet 211) only after the continue click resumes it.
        let script = Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::P_PAUSEBUTTON, op::PUSH_CONSTANT_INT, op::MIDI_SONG, op::RETURN],
            int_operands: vec![0, 55, 0, 0],
            string_operands: vec![None; 4],
        });
        world.enqueue_engine(pid, script, vec![], 0);

        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle(); // runs -> pauses on the button
        assert!(world.players[pid].as_ref().unwrap().out.iter().all(|m| m.opcode != 211),
                "paused before MIDI_SONG");
        // Ticks alone must NOT resume a pause-button wait.
        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle();
        world.cycle();
        assert!(world.players[pid].as_ref().unwrap().out.iter().all(|m| m.opcode != 211),
                "a pause-button wait doesn't resume on ticks");
        // The continue click resumes it.
        world.players[pid].as_mut().unwrap().out.clear();
        world.handle_message(pid, ClientMessage::ResumePauseButton { component: 1, sub: -1 });
        assert!(world.players[pid].as_ref().unwrap().out.iter().any(|m| m.opcode == 211),
                "resumed on the continue click");
    }

    #[test]
    fn delayed_player_cannot_move() {
        use protocol::client::ClientMessage;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        // Lock the player a few ticks ahead.
        world.players[pid].as_mut().unwrap().delayed_until = world.tick as i32 + 5;
        world.handle_message(pid, ClientMessage::MoveClick {
            route: vec![(3225, 3222)], ctrl_held: false,
        });
        assert!(world.players[pid].as_ref().unwrap().entity.waypoints.is_empty(),
                "an action-locked player's walk is ignored");
    }

    #[test]
    fn logout_runs_the_logout_script() {
        use crate::script::provider::ScriptProvider;
        let mut world = World::new();
        let mut provider = ScriptProvider::test_empty();
        // [logout,_] spawns npc type 99 at (3222,3222) — observable after the
        // player is gone since npcs persist in the world.
        let coord = (3222i32 << 14) | 3222;
        provider.test_insert_global(trigger::LOGOUT, npc_add_script(coord, 99));
        world.scripts = Some(provider);
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        assert!(world.npcs.iter().flatten().all(|n| n.type_id != 99), "no npc before logout");
        world.remove_player(pid);
        assert!(world.npcs.iter().flatten().any(|n| n.type_id == 99),
                "logout script ran before the player was removed");
        assert!(world.players[pid].is_none(), "player removed after the script");
    }

    #[test]
    fn give_xp_fires_advancestat_on_level_up() {
        use crate::entity::player::STAT_ATTACK;
        use crate::script::provider::ScriptProvider;
        let mut world = World::new();
        // Register an [advancestat, attack] script that plays midi 77 (op 211).
        let mut provider = ScriptProvider::test_empty();
        provider.test_insert_specific(trigger::ADVANCESTAT, STAT_ATTACK as i32, midi_script(77));
        world.scripts = Some(provider);
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();

        // Award enough XP to advance attack from 1 — fires the queue entry.
        world.give_xp(pid, STAT_ATTACK as i32, 5000);
        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle(); // queue pass runs the advancestat script
        assert!(world.players[pid].as_ref().unwrap().out.iter().any(|m| m.opcode == 211),
                "advancestat script ran after the level-up");
    }

    #[test]
    fn give_xp_without_level_up_enqueues_nothing() {
        use crate::entity::player::STAT_ATTACK;
        use crate::script::provider::ScriptProvider;
        let mut world = World::new();
        let mut provider = ScriptProvider::test_empty();
        provider.test_insert_specific(trigger::ADVANCESTAT, STAT_ATTACK as i32, midi_script(77));
        world.scripts = Some(provider);
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        // 1 xp doesn't change the level → no advancestat.
        world.give_xp(pid, STAT_ATTACK as i32, 1);
        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle();
        assert!(world.players[pid].as_ref().unwrap().out.iter().all(|m| m.opcode != 211),
                "no advancestat without a level-up");
    }

    #[test]
    fn engine_queue_runs_script_after_its_delay() {
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        // A script that plays midi 42 (writes packet opcode 211).
        world.enqueue_engine(pid, midi_script(42), vec![], 1); // fires after 1 tick

        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle(); // delay 1 -> 0, not yet due
        assert!(world.players[pid].as_ref().unwrap().out.iter().all(|m| m.opcode != 211),
                "delayed script does not fire early");

        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle(); // delay 0, due -> runs once
        assert!(world.players[pid].as_ref().unwrap().out.iter().any(|m| m.opcode == 211),
                "delayed script fires after its delay");

        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle(); // already dropped -> never again
        assert!(world.players[pid].as_ref().unwrap().out.iter().all(|m| m.opcode != 211),
                "queued script runs exactly once");
    }

    fn midi_script_id(id: i32, song: i32) -> Arc<crate::script::file::ScriptFile> {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        Arc::new(ScriptFile {
            id,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::PUSH_CONSTANT_INT, op::MIDI_SONG, op::RETURN],
            int_operands: vec![song, 0, 0],
            string_operands: vec![None; 3],
        })
    }

    #[test]
    fn normal_timer_fires_each_interval_then_clears() {
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        // A NORMAL timer (interval 3) re-runs a midi script (packet 211) every
        // 3 ticks: first fire when the clock+interval is reached.
        let now = world.tick;
        world.players[pid].as_mut().unwrap()
            .set_timer(player::TimerKind::Normal, midi_script_id(1, 55), vec![], 3, now);

        // Ticks 1..3: clock(0)+interval(3) not reached -> no fire.
        for _ in 0..3 {
            world.players[pid].as_mut().unwrap().out.clear();
            world.cycle();
            assert!(world.players[pid].as_ref().unwrap().out.iter().all(|m| m.opcode != 211),
                    "timer silent before its interval elapses");
        }
        // 4th cycle (now == 3): fires.
        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle();
        assert!(world.players[pid].as_ref().unwrap().out.iter().any(|m| m.opcode == 211),
                "timer fires once its interval elapses");

        // It keeps firing every 3 ticks (next at now == 6).
        for _ in 0..2 {
            world.players[pid].as_mut().unwrap().out.clear();
            world.cycle();
            assert!(world.players[pid].as_ref().unwrap().out.iter().all(|m| m.opcode != 211),
                    "timer waits a full interval before re-firing");
        }
        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle();
        assert!(world.players[pid].as_ref().unwrap().out.iter().any(|m| m.opcode == 211),
                "timer re-fires on each interval");

        // Clearing it stops all further fires.
        world.players[pid].as_mut().unwrap().clear_timer(1);
        for _ in 0..6 {
            world.players[pid].as_mut().unwrap().out.clear();
            world.cycle();
            assert!(world.players[pid].as_ref().unwrap().out.iter().all(|m| m.opcode != 211),
                    "a cleared timer never fires again");
        }
    }

    #[test]
    fn soft_timer_runs_while_delayed_normal_timer_waits() {
        let mut world = World::new();
        // p1 has a NORMAL timer, p2 a SOFT timer — both interval 1, both delayed.
        let p1 = world.add_player("a".into(), 3222, 3222, 0).unwrap();
        let p2 = world.add_player("b".into(), 3230, 3230, 0).unwrap();
        let now = world.tick;
        world.players[p1].as_mut().unwrap()
            .set_timer(player::TimerKind::Normal, midi_script_id(1, 55), vec![], 1, now);
        world.players[p2].as_mut().unwrap()
            .set_timer(player::TimerKind::Soft, midi_script_id(2, 55), vec![], 1, now);
        // Action-lock both players well past the interval.
        let lock = world.tick as i32 + 10;
        world.players[p1].as_mut().unwrap().delayed_until = lock;
        world.players[p2].as_mut().unwrap().delayed_until = lock;

        world.players[p1].as_mut().unwrap().out.clear();
        world.players[p2].as_mut().unwrap().out.clear();
        // interval 1 is due one tick after registration (now == clock + 1).
        world.cycle(); // now == registration tick: not yet due
        world.cycle(); // now == clock + 1: SOFT fires, NORMAL stays locked
        assert!(world.players[p1].as_ref().unwrap().out.iter().all(|m| m.opcode != 211),
                "a NORMAL timer waits while the player is action-locked");
        assert!(world.players[p2].as_ref().unwrap().out.iter().any(|m| m.opcode == 211),
                "a SOFT timer fires regardless of the action-lock");

        // Once the lock lapses the NORMAL timer fires (its clock never advanced).
        world.players[p1].as_mut().unwrap().delayed_until = -1;
        world.players[p1].as_mut().unwrap().out.clear();
        world.cycle();
        assert!(world.players[p1].as_ref().unwrap().out.iter().any(|m| m.opcode == 211),
                "the NORMAL timer fires once the lock lifts");
    }

    #[test]
    fn settimer_op_registers_timer_and_gettimer_reads_clock() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        use crate::script::provider::ScriptProvider;
        let mut world = World::new();
        let mut provider = ScriptProvider::test_empty();
        provider.test_insert_by_id(7, midi_script_id(7, 55));
        provider.test_insert_by_id(8, midi_script_id(8, 55));
        world.scripts = Some(provider);
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();

        // [setup] PUSH 7 (timer id), PUSH 4 (interval), PUSH "" (no args),
        // SETTIMER — registers a NORMAL timer keyed by script id 7.
        let setup = Arc::new(ScriptFile {
            id: 99,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT,
                          op::PUSH_CONSTANT_STRING, op::SETTIMER, op::RETURN],
            int_operands: vec![7, 4, 0, 0, 0],
            string_operands: vec![None, None, Some(String::new()), None, None],
        });
        let t0 = world.tick;
        world.enqueue_engine(pid, setup, vec![], 0);
        world.cycle(); // engine queue runs SETTIMER this tick -> clock == t0

        // The timer is now registered with the right interval/clock.
        let t = world.players[pid].as_ref().unwrap().timers.get(&7)
            .expect("SETTIMER registered a timer keyed by script id");
        assert_eq!(t.interval, 4, "interval stored");
        assert_eq!(t.clock, t0, "clock set to the tick it was registered");

        // GETTIMER reads the clock for a live timer, -1 for an absent one.
        let gettimer = |timer_id: i32| Arc::new(ScriptFile {
            id: 98,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::PUSH_CONSTANT_INT, op::GETTIMER, op::RETURN],
            int_operands: vec![timer_id, 0, 0],
            string_operands: vec![None; 3],
        });
        let run = |world: &mut World, timer_id: i32| {
            let mut st = ScriptState::new(gettimer(timer_id), &[]);
            st.active_player = Some(pid);
            runner::execute(&mut st, world);
            st.int_stack.last().copied()
        };
        assert_eq!(run(&mut world, 7), Some(t0 as i32), "GETTIMER returns the clock for a live timer");
        assert_eq!(run(&mut world, 8), Some(-1), "GETTIMER returns -1 for an unset timer");
    }

    #[test]
    fn queue_script_fires_after_its_delay_once() {
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        world.players[pid].as_mut().unwrap().enqueue_script(
            player::QueueKind::Normal, midi_script_id(5, 55), vec![ScriptArg::Int(0)], 1);

        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle(); // delay 1 -> 0, not yet due
        assert!(world.players[pid].as_ref().unwrap().out.iter().all(|m| m.opcode != 211),
                "queued script waits out its delay");
        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle(); // delay 0, due -> fires
        assert!(world.players[pid].as_ref().unwrap().out.iter().any(|m| m.opcode == 211),
                "queued script fires when its delay elapses");
        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle(); // already unlinked -> never again
        assert!(world.players[pid].as_ref().unwrap().out.iter().all(|m| m.opcode != 211),
                "a fired queue entry runs exactly once");
    }

    #[test]
    fn strong_queue_clears_the_weak_queue() {
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        let p = world.players[pid].as_mut().unwrap();
        // A weak entry parked a few ticks out, plus a strong entry queued after.
        p.enqueue_script(player::QueueKind::Weak, midi_script_id(6, 55), vec![ScriptArg::Int(0)], 3);
        assert_eq!(p.weak_queue.len(), 1, "weak entry queued");
        p.enqueue_script(player::QueueKind::Strong, midi_script_id(7, 55), vec![ScriptArg::Int(0)], 3);

        world.cycle();
        assert!(world.players[pid].as_ref().unwrap().weak_queue.is_empty(),
                "queuing a STRONG script clears the weak queue");
    }

    #[test]
    fn a_delaying_queue_script_blocks_later_entries_until_it_resumes() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        // script A: P_DELAY 0 then MIDI 55 — suspends before its midi.
        let a = Arc::new(ScriptFile {
            id: 10,
            info: ScriptInfo {
                script_name: "a".into(), source_file_path: "a".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::PUSH_CONSTANT_INT, op::P_DELAY, op::PUSH_CONSTANT_INT,
                          op::MIDI_SONG, op::RETURN],
            int_operands: vec![0, 0, 55, 0, 0],
            string_operands: vec![None; 5],
        });
        // Both queued at delay 0 so both are due the first cycle.
        world.players[pid].as_mut().unwrap()
            .enqueue_script(player::QueueKind::Normal, a, vec![], 0);
        world.players[pid].as_mut().unwrap()
            .enqueue_script(player::QueueKind::Normal, midi_script_id(11, 66), vec![ScriptArg::Int(0)], 0);

        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle(); // A runs, suspends at P_DELAY; B is due but blocked (no access)
        assert!(world.players[pid].as_ref().unwrap().out.iter().all(|m| m.opcode != 211),
                "the delaying script suspends and blocks the next entry");
        assert_eq!(world.players[pid].as_ref().unwrap().queue.len(), 1,
                "the blocked entry stays queued");

        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle(); // A resumes (-> midi 55), then B gets access (-> midi 66)
        assert!(world.players[pid].as_ref().unwrap().out.iter().filter(|m| m.opcode == 211).count() >= 2,
                "both scripts run once the lock lifts");
        assert!(world.players[pid].as_ref().unwrap().queue.is_empty(),
                "the queue drains after both run");
    }

    #[test]
    fn long_queue_hides_the_logout_action_arg_from_the_script() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        // script: if local0 == 99 -> play midi 55. local0 is the script's first
        // arg, which after the LONG shift must be `arg` (99), not the logout flag.
        let s = Arc::new(ScriptFile {
            id: 12,
            info: ScriptInfo {
                script_name: "s".into(), source_file_path: "s".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 1, string_local_count: 0, int_arg_count: 1, string_arg_count: 0,
            switch_tables: vec![],
            // PUSH local0, PUSH 99, BRANCH_NOT +2, PUSH 55, MIDI, RETURN
            opcodes: vec![op::PUSH_INT_LOCAL, op::PUSH_CONSTANT_INT, op::BRANCH_NOT,
                          op::PUSH_CONSTANT_INT, op::MIDI_SONG, op::RETURN],
            int_operands: vec![0, 99, 2, 55, 0, 0],
            string_operands: vec![None; 6],
        });
        // LONG args = [logout_action(0), arg(99)]; the shift drops logout_action.
        world.players[pid].as_mut().unwrap().enqueue_script(
            player::QueueKind::Long, s, vec![ScriptArg::Int(0), ScriptArg::Int(99)], 0);

        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle();
        assert!(world.players[pid].as_ref().unwrap().out.iter().any(|m| m.opcode == 211),
                "the LONG script saw arg 99, proving the logout-action arg was shifted off");
    }

    #[test]
    fn getqueue_counts_and_clearqueue_removes() {
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        let p = world.players[pid].as_mut().unwrap();
        // Two normal + one weak of script id 5, plus one of id 6.
        p.enqueue_script(player::QueueKind::Normal, midi_script_id(5, 55), vec![ScriptArg::Int(0)], 9);
        p.enqueue_script(player::QueueKind::Normal, midi_script_id(5, 55), vec![ScriptArg::Int(0)], 9);
        p.enqueue_script(player::QueueKind::Weak, midi_script_id(5, 55), vec![ScriptArg::Int(0)], 9);
        p.enqueue_script(player::QueueKind::Normal, midi_script_id(6, 55), vec![ScriptArg::Int(0)], 9);
        assert_eq!(p.count_queued(5), 3, "GETQUEUE counts both queues");
        assert_eq!(p.count_queued(6), 1);
        p.clear_queued_script(5);
        assert_eq!(p.count_queued(5), 0, "CLEARQUEUE drops every matching entry");
        assert_eq!(p.count_queued(6), 1, "CLEARQUEUE leaves other scripts alone");
    }

    #[test]
    fn walktrigger_op_arms_and_fires_on_move_click_then_clears() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        use crate::script::provider::ScriptProvider;
        use protocol::client::ClientMessage;
        let mut world = World::new();
        let mut provider = ScriptProvider::test_empty();
        provider.test_insert_by_id(20, midi_script_id(20, 55)); // the walktrigger script
        world.scripts = Some(provider);
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();

        // [setup] PUSH 20, WALKTRIGGER — arm script 20 as the walk trigger.
        let setup = Arc::new(ScriptFile {
            id: 21,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::PUSH_CONSTANT_INT, op::WALKTRIGGER, op::RETURN],
            int_operands: vec![20, 0, 0],
            string_operands: vec![None; 3],
        });
        world.enqueue_engine(pid, setup, vec![], 0);
        world.cycle(); // runs setup -> walk_trigger = 20
        assert_eq!(world.players[pid].as_ref().unwrap().walk_trigger, 20,
                   "WALKTRIGGER op armed the trigger");

        // GETWALKTRIGGER reads it back.
        let mut st = ScriptState::new(Arc::new(ScriptFile {
            id: 22,
            info: ScriptInfo {
                script_name: "g".into(), source_file_path: "g".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::GETWALKTRIGGER, op::RETURN],
            int_operands: vec![0, 0],
            string_operands: vec![None; 2],
        }), &[]);
        st.active_player = Some(pid);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack.last().copied(), Some(20), "GETWALKTRIGGER reads the armed id");

        // A move-click that produces a path fires the trigger once, then clears.
        world.players[pid].as_mut().unwrap().out.clear();
        world.handle_message(pid, ClientMessage::MoveClick {
            route: vec![(3225, 3222)], ctrl_held: false,
        });
        let p = world.players[pid].as_ref().unwrap();
        assert!(p.out.iter().any(|m| m.opcode == 211), "the walk fired the walktrigger script");
        assert_eq!(p.walk_trigger, -1, "the walktrigger is one-shot");

        // A subsequent walk does not re-fire it.
        world.players[pid].as_mut().unwrap().out.clear();
        world.handle_message(pid, ClientMessage::MoveClick {
            route: vec![(3226, 3222)], ctrl_held: false,
        });
        assert!(world.players[pid].as_ref().unwrap().out.iter().all(|m| m.opcode != 211),
                "a consumed walktrigger never fires again");
    }

    #[test]
    fn walktrigger_does_not_fire_without_a_resulting_path() {
        use crate::script::provider::ScriptProvider;
        use protocol::client::ClientMessage;
        let mut world = World::new();
        let mut provider = ScriptProvider::test_empty();
        provider.test_insert_by_id(20, midi_script_id(20, 55));
        world.scripts = Some(provider);
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        world.players[pid].as_mut().unwrap().walk_trigger = 20;

        // An out-of-scene click is rejected, yields no path -> no walktrigger.
        world.players[pid].as_mut().unwrap().out.clear();
        world.handle_message(pid, ClientMessage::MoveClick {
            route: vec![(3500, 3222)], ctrl_held: false,
        });
        let p = world.players[pid].as_ref().unwrap();
        assert!(p.out.iter().all(|m| m.opcode != 211), "no path -> no walktrigger");
        assert_eq!(p.walk_trigger, 20, "an unfired walktrigger stays armed");
    }

    #[test]
    fn public_chat_stores_state_sets_mask_and_rate_limits() {
        use protocol::client::ClientMessage;
        use crate::entity::player::MASK_PUBLIC_CHAT;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();

        world.handle_message(pid, ClientMessage::MessagePublic {
            colour: 3, effect: 1, message: vec![0xAB, 0xCD],
        });
        let p = world.players[pid].as_ref().unwrap();
        assert_eq!(p.chat_colour, 3);
        assert_eq!(p.chat_effect, 1);
        assert_eq!(p.chat_message, vec![0xAB, 0xCD]);
        assert_eq!(p.chat_rights, 0);
        assert!(p.entity.masks & MASK_PUBLIC_CHAT != 0, "PUBLIC_CHAT mask set");
        assert!(p.social_protect, "anti-spam latch set after a chat");

        // A second chat the same tick is rate-limited (latch set) and ignored.
        world.handle_message(pid, ClientMessage::MessagePublic {
            colour: 5, effect: 2, message: vec![0x11],
        });
        assert_eq!(world.players[pid].as_ref().unwrap().chat_colour, 3,
                   "the second same-tick chat is dropped");

        // The cycle broadcasts the chat (exercises the encoder — no panic where
        // the old `unreachable!` was), then clears the mask and re-arms the latch.
        world.cycle();
        let p = world.players[pid].as_ref().unwrap();
        assert!(p.entity.masks & MASK_PUBLIC_CHAT == 0, "chat mask cleared after broadcast");
        assert!(!p.social_protect, "latch re-armed for the next tick");
    }

    #[test]
    fn public_chat_rejects_out_of_range_values() {
        use protocol::client::ClientMessage;
        use crate::entity::player::MASK_PUBLIC_CHAT;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        let chatted = |w: &World, pid: usize|
            w.players[pid].as_ref().unwrap().entity.masks & MASK_PUBLIC_CHAT != 0;

        // colour > 11 rejected.
        world.handle_message(pid, ClientMessage::MessagePublic { colour: 12, effect: 0, message: vec![1] });
        assert!(!chatted(&world, pid), "colour 12 rejected");
        // effect > 5 rejected.
        world.handle_message(pid, ClientMessage::MessagePublic { colour: 0, effect: 6, message: vec![1] });
        assert!(!chatted(&world, pid), "effect 6 rejected");
        // over-long packed message (>100 bytes) rejected.
        world.handle_message(pid, ClientMessage::MessagePublic { colour: 0, effect: 0, message: vec![0; 101] });
        assert!(!chatted(&world, pid), "over-long message rejected");
        // a valid one then goes through.
        world.handle_message(pid, ClientMessage::MessagePublic { colour: 0, effect: 0, message: vec![0; 100] });
        assert!(chatted(&world, pid), "an in-range chat is accepted");
    }

    #[test]
    fn public_chat_rights_track_staff_mod_level() {
        use protocol::client::ClientMessage;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        let chat = |w: &mut World, level: i32| -> i32 {
            let p = w.players[pid].as_mut().unwrap();
            p.staff_mod_level = level;
            p.social_protect = false; // re-arm the one-chat-per-tick latch
            w.handle_message(pid, ClientMessage::MessagePublic { colour: 0, effect: 0, message: vec![1] });
            w.players[pid].as_ref().unwrap().chat_rights
        };
        // chatRights = min(staffModLevel, 2): the message's mod/admin crown.
        assert_eq!(chat(&mut world, 0), 0, "non-staff → no crown");
        assert_eq!(chat(&mut world, 1), 1, "mod → 1");
        assert_eq!(chat(&mut world, 2), 2, "admin → 2");
        assert_eq!(chat(&mut world, 5), 2, "higher levels cap at 2");
    }

    #[test]
    fn p_countdialog_resumes_with_the_entered_amount() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        use protocol::client::ClientMessage;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        let coord = (3225i32 << 14) | 3225;
        // P_COUNTDIALOG, then spawn an npc whose type is the entered amount
        // (LAST_INT) — proving the resume value flows through.
        let script = Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::P_COUNTDIALOG, op::PUSH_CONSTANT_INT, op::LAST_INT,
                          op::PUSH_CONSTANT_INT, op::NPC_ADD, op::RETURN],
            int_operands: vec![0, coord, 0, 0, 0, 0],
            string_operands: vec![None; 6],
        });
        world.enqueue_engine(pid, script, vec![], 0);
        world.cycle(); // runs → suspends on P_COUNTDIALOG
        let count = |w: &World| w.npcs.iter().flatten().count();
        assert_eq!(count(&world), 0, "suspended before spawning");

        // Ticks alone don't resume a count dialog.
        world.cycle();
        world.cycle();
        assert_eq!(count(&world), 0, "a count dialog doesn't resume on ticks");

        // The entered amount (77) resumes the script → spawns a type-77 npc.
        world.handle_message(pid, ClientMessage::ResumeCountDialog { value: 77 });
        assert_eq!(count(&world), 1, "resumed on the entered amount");
        assert!(world.npcs.iter().flatten().any(|n| n.type_id == 77),
                "the resumed script read the entered value (77) via LAST_INT");
    }

    #[test]
    fn close_modal_abandons_a_pausebutton_suspended_script() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        use protocol::client::ClientMessage;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        // P_PAUSEBUTTON, PUSH 55, MIDI_SONG, RETURN — pauses before the midi.
        let script = Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::P_PAUSEBUTTON, op::PUSH_CONSTANT_INT, op::MIDI_SONG, op::RETURN],
            int_operands: vec![0, 55, 0, 0],
            string_operands: vec![None; 4],
        });
        world.enqueue_engine(pid, script, vec![], 0);
        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle(); // runs -> pauses on the button
        assert!(world.players[pid].as_ref().unwrap().out.iter().all(|m| m.opcode != 211),
                "paused before the midi");

        // The player dismisses the dialog instead of clicking continue.
        world.handle_message(pid, ClientMessage::CloseModal);
        world.cycle(); // process_queues consumes the request -> close_modal drops the wait

        // The continue click now does nothing — the script was abandoned.
        world.players[pid].as_mut().unwrap().out.clear();
        world.handle_message(pid, ClientMessage::ResumePauseButton { component: 1, sub: -1 });
        assert!(world.players[pid].as_ref().unwrap().out.iter().all(|m| m.opcode != 211),
                "CLOSE_MODAL abandons the paused script; the continue click is inert");
    }

    #[test]
    fn p_stopaction_clears_interaction_modal_and_walk_flag() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        // An active interaction target + a parked weak-queue entry.
        world.players[pid].as_mut().unwrap().entity.target = 5;
        world.players[pid].as_mut().unwrap().enqueue_script(
            player::QueueKind::Weak, midi_script_id(6, 55), vec![ScriptArg::Int(0)], 9);
        world.players[pid].as_mut().unwrap().out.clear();

        // P_STOPACTION via the engine queue (runs protected).
        let script = Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::P_STOPACTION, op::RETURN],
            int_operands: vec![0, 0],
            string_operands: vec![None; 2],
        });
        world.enqueue_engine(pid, script, vec![], 0);
        world.cycle();

        let p = world.players[pid].as_ref().unwrap();
        assert_eq!(p.entity.target, -1, "the interaction is cleared");
        assert!(p.weak_queue.is_empty(), "closing the modal cleared the weak queue");
        assert!(p.out.iter().any(|m| m.opcode == 161), "UNSET_MAP_FLAG was sent");
    }

    #[test]
    fn p_animprotect_blocks_then_allows_the_anim_op() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        use crate::entity::player::MASK_ANIM;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();

        // [PUSH p, P_ANIMPROTECT, PUSH seq, PUSH 0, ANIM] — protect=p, then animate.
        let run = |world: &mut World, protect: i32, seq: i32| {
            let script = Arc::new(ScriptFile {
                id: 0,
                info: ScriptInfo {
                    script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                    parameter_types: vec![], pcs: vec![], lines: vec![],
                },
                int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
                switch_tables: vec![],
                opcodes: vec![op::PUSH_CONSTANT_INT, op::P_ANIMPROTECT,
                              op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT, op::ANIM, op::RETURN],
                int_operands: vec![protect, 0, seq, 0, 0, 0],
                string_operands: vec![None; 6],
            });
            let mut st = ScriptState::new(script, &[]);
            st.active_player = Some(pid);
            st.pointer_add(crate::script::state::Pointer::ActivePlayer);
            st.pointer_add(crate::script::state::Pointer::ProtectedActivePlayer);
            crate::script::runner::execute(&mut st, world);
        };

        world.players[pid].as_mut().unwrap().entity.anim_id = -1;
        world.players[pid].as_mut().unwrap().entity.masks = 0;
        run(&mut world, 1, 5); // protect, then try to animate (5) → blocked
        {
            let p = world.players[pid].as_ref().unwrap();
            assert_eq!(p.entity.anim_id, -1, "a protected animation isn't changed");
            assert_eq!(p.entity.masks & MASK_ANIM, 0, "the ANIM mask is not set");
        }

        run(&mut world, 0, 7); // release, then animate (7) → works
        {
            let p = world.players[pid].as_ref().unwrap();
            assert_eq!(p.entity.anim_id, 7, "a released animation plays");
            assert_ne!(p.entity.masks & MASK_ANIM, 0, "the ANIM mask is set");
        }
    }

    #[test]
    fn pvp_hero_points_credit_and_findhero() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let a = world.add_player("attacker".into(), 3222, 3222, 0).unwrap();
        let b = world.add_player("victim".into(), 3223, 3222, 0).unwrap();
        let mk = |opcodes: Vec<u16>, ints: Vec<i32>| {
            let n = opcodes.len();
            Arc::new(ScriptFile {
                id: 0,
                info: ScriptInfo {
                    script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                    parameter_types: vec![], pcs: vec![], lines: vec![],
                },
                int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
                switch_tables: vec![],
                opcodes, int_operands: ints, string_operands: vec![None; n],
            })
        };

        // BOTH_HEROPOINTS(50): credit attacker A's damage to victim B's tally.
        let mut st = ScriptState::new(
            mk(vec![op::PUSH_CONSTANT_INT, op::BOTH_HEROPOINTS, op::RETURN], vec![50, 0, 0]), &[]);
        st.active_player = Some(a);
        st.active_player2 = Some(b);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(world.players[b].as_ref().unwrap().hero_points, vec![(a, 50)],
                   "the attacker is credited to the victim's tally");

        // FINDHERO on the victim resolves the attacker as the secondary player.
        let mut st = ScriptState::new(mk(vec![op::FINDHERO, op::RETURN], vec![0, 0]), &[]);
        st.active_player = Some(b);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack.last().copied(), Some(1), "found the hero");
        assert_eq!(st.active_player2, Some(a), "the attacker is set as active_player2");

        // A player with no damage dealers → FINDHERO pushes 0.
        let c = world.add_player("bystander".into(), 3224, 3222, 0).unwrap();
        let mut st = ScriptState::new(mk(vec![op::FINDHERO, op::RETURN], vec![0, 0]), &[]);
        st.active_player = Some(c);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack.last().copied(), Some(0), "no hero → 0");
    }

    #[test]
    fn busy_and_busy2_report_player_state() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        let run = |world: &mut World, opcode: u16| -> i32 {
            let script = Arc::new(ScriptFile {
                id: 0,
                info: ScriptInfo {
                    script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                    parameter_types: vec![], pcs: vec![], lines: vec![],
                },
                int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
                switch_tables: vec![],
                opcodes: vec![opcode, op::RETURN],
                int_operands: vec![0, 0],
                string_operands: vec![None; 2],
            });
            let mut st = ScriptState::new(script, &[]);
            st.active_player = Some(pid);
            crate::script::runner::execute(&mut st, world);
            st.int_stack.last().copied().unwrap()
        };

        // A fresh player is neither busy nor interacting/walking.
        assert_eq!(run(&mut world, op::BUSY), 0, "idle player is not busy");
        assert_eq!(run(&mut world, op::BUSY2), 0, "no interaction or walk");

        // Action-locked → BUSY.
        world.players[pid].as_mut().unwrap().delayed_until = world.tick as i32 + 5;
        assert_eq!(run(&mut world, op::BUSY), 1, "a delayed player is busy");
        world.players[pid].as_mut().unwrap().delayed_until = -1;

        // Logging out → BUSY.
        world.players[pid].as_mut().unwrap().logging_out = true;
        assert_eq!(run(&mut world, op::BUSY), 1, "a logging-out player is busy");
        world.players[pid].as_mut().unwrap().logging_out = false;

        // A pending interaction → BUSY2.
        world.players[pid].as_mut().unwrap().entity.target = 5;
        assert_eq!(run(&mut world, op::BUSY2), 1, "a pending interaction is busy2");
        world.players[pid].as_mut().unwrap().entity.target = -1;

        // Walking → BUSY2.
        world.players[pid].as_mut().unwrap().entity.queue_waypoints(&[(3225, 3222)]);
        assert_eq!(run(&mut world, op::BUSY2), 1, "still walking is busy2");
    }

    #[test]
    fn p_clearpendingaction_clears_interaction_and_modal_but_keeps_the_walk() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        world.players[pid].as_mut().unwrap().entity.target = 5;
        world.players[pid].as_mut().unwrap().enqueue_script(
            player::QueueKind::Weak, midi_script_id(6, 55), vec![ScriptArg::Int(0)], 9);
        world.players[pid].as_mut().unwrap().entity.queue_waypoints(&[(3225, 3222)]);
        world.players[pid].as_mut().unwrap().out.clear();

        let script = Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::P_CLEARPENDINGACTION, op::RETURN],
            int_operands: vec![0, 0],
            string_operands: vec![None; 2],
        });
        world.enqueue_engine(pid, script, vec![], 0);
        world.cycle();

        let p = world.players[pid].as_ref().unwrap();
        assert_eq!(p.entity.target, -1, "the interaction is cleared");
        assert!(p.weak_queue.is_empty(), "closing the modal cleared the weak queue");
        assert!(!p.entity.waypoints.is_empty(), "the walk queue is kept (unlike P_STOPACTION)");
        assert!(p.out.iter().all(|m| m.opcode != 161), "no UNSET_MAP_FLAG is sent");
    }

    #[test]
    fn weight_op_pushes_run_weight() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        world.players[pid].as_mut().unwrap().run_weight = 2500;
        let script = Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::WEIGHT, op::RETURN],
            int_operands: vec![0, 0],
            string_operands: vec![None; 2],
        });
        let mut st = ScriptState::new(script, &[]);
        st.active_player = Some(pid);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack.last().copied(), Some(2500), "WEIGHT pushes the run weight");
    }

    #[test]
    fn facesquare_sets_one_shot_and_persistent_facing() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        const MASK_FACE_COORD: i32 = crate::entity::player::MASK_FACE_COORD;
        let script = Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::FACESQUARE, op::RETURN],
            int_operands: vec![0, 0],
            string_operands: vec![None; 2],
        });
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        let coord = (3225 << 14) | 3222; // three tiles east
        let mut st = ScriptState::new(script, &[]);
        st.active_player = Some(pid);
        st.push_int(coord);
        crate::script::runner::execute(&mut st, &mut world);

        let e = &world.players[pid].as_ref().unwrap().entity;
        assert_eq!((e.face_x, e.face_z), (3225, 3222), "one-shot FACE_COORD target set");
        assert_ne!(e.masks & MASK_FACE_COORD, 0, "FACE_COORD mask flagged");
        // The persistent orientation turned east too, so a newly-observing client
        // sees the entity already facing the coord (not the default south).
        assert_eq!(e.orientation_dir(), 4, "persistent facing turned east");
    }

    #[test]
    fn npc_say_ignores_empty_text() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        use crate::script::state::Pointer;
        const MASK_SAY: i32 = crate::entity::npc::MASK_SAY;
        let mk = || Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::NPC_SAY, op::RETURN],
            int_operands: vec![0, 0],
            string_operands: vec![None; 2],
        });
        let mut world = World::new();
        let nid = world.add_npc(11, 3222, 3222, 0).unwrap();
        let run = |world: &mut World, text: &str| {
            let mut st = ScriptState::new(mk(), &[]);
            st.active_npc = Some(nid);
            st.pointer_add(Pointer::ActiveNpc);
            st.push_string(text.to_string());
            crate::script::runner::execute(&mut st, world);
        };

        // Non-empty say sets the overhead message and the SAY mask.
        run(&mut world, "hello");
        {
            let n = world.npcs[nid].as_ref().unwrap();
            assert_eq!(n.entity.chat.as_deref(), Some("hello"));
            assert_ne!(n.entity.masks & MASK_SAY, 0, "non-empty say flags the mask");
        }

        // Reset, then an empty say is a no-op (no message, no mask).
        {
            let n = world.npcs[nid].as_mut().unwrap();
            n.entity.chat = None;
            n.entity.masks = 0;
        }
        run(&mut world, "");
        let n = world.npcs[nid].as_ref().unwrap();
        assert_eq!(n.entity.chat, None, "empty say sets no overhead message");
        assert_eq!(n.entity.masks & MASK_SAY, 0, "empty say flags no mask");
    }

    #[test]
    fn p_arrivedelay_suspends_only_after_a_step() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        use crate::script::state::{Execution, Pointer};
        let mk = || Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::P_ARRIVEDELAY, op::RETURN],
            int_operands: vec![0, 0],
            string_operands: vec![None; 2],
        });
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        let tick = world.tick as i32;
        let run = |world: &mut World| {
            let mut st = ScriptState::new(mk(), &[]);
            st.active_player = Some(pid);
            st.pointer_add(Pointer::ProtectedActivePlayer);
            crate::script::runner::execute(&mut st, world)
        };

        // No recent step → the op falls through and the script finishes.
        world.players[pid].as_mut().unwrap().entity.last_movement = 0;
        assert_eq!(run(&mut world), Execution::Finished, "no step → no arrive delay");

        // Stepped this tick → suspend and action-lock for one tick.
        world.players[pid].as_mut().unwrap().entity.last_movement = tick + 1;
        assert_eq!(run(&mut world), Execution::Suspended, "a fresh step arms the arrive delay");
        assert_eq!(world.players[pid].as_ref().unwrap().delayed_until, tick + 1);
    }

    #[test]
    fn npc_arrivedelay_scales_with_step_recency() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        use crate::script::state::{Execution, Pointer};
        let mk = || Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::NPC_ARRIVEDELAY, op::RETURN],
            int_operands: vec![0, 0],
            string_operands: vec![None; 2],
        });
        let mut world = World::new();
        let nid = world.add_npc(11, 3222, 3222, 0).unwrap();
        let tick = world.tick as i32;
        let run = |world: &mut World| {
            let mut st = ScriptState::new(mk(), &[]);
            st.active_npc = Some(nid);
            st.pointer_add(Pointer::ActiveNpc);
            crate::script::runner::execute(&mut st, world)
        };

        // Stamped this tick (lastMovement = tick+1) → 2-tick settle.
        world.npcs[nid].as_mut().unwrap().entity.last_movement = tick + 1;
        assert_eq!(run(&mut world), Execution::NpcSuspended);
        assert_eq!(world.npcs[nid].as_ref().unwrap().delayed_until, tick + 2);

        // Stamp one tick older (lastMovement = tick-1) → 1-tick settle.
        world.npcs[nid].as_mut().unwrap().entity.last_movement = tick - 1;
        assert_eq!(run(&mut world), Execution::NpcSuspended);
        assert_eq!(world.npcs[nid].as_ref().unwrap().delayed_until, tick + 1);

        // Older than that → no delay, the script finishes.
        world.npcs[nid].as_mut().unwrap().entity.last_movement = tick - 2;
        assert_eq!(run(&mut world), Execution::Finished, "stale movement → no delay");
    }

    #[test]
    fn afk_event_op_reports_and_clears_the_ready_flag() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mk = || Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::AFK_EVENT, op::RETURN],
            int_operands: vec![0, 0],
            string_operands: vec![None; 2],
        });
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        world.players[pid].as_mut().unwrap().afk_event_ready = true;

        // First poll fires and consumes the flag.
        let mut st = ScriptState::new(mk(), &[]);
        st.active_player = Some(pid);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack.last().copied(), Some(1), "AFK_EVENT reports the armed event");
        assert!(!world.players[pid].as_ref().unwrap().afk_event_ready, "the flag is cleared");

        // Second poll (flag now clear) reports nothing.
        let mut st = ScriptState::new(mk(), &[]);
        st.active_player = Some(pid);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack.last().copied(), Some(0), "a consumed event doesn't re-fire");

        // Staff (level >= 2) are exempt even with the flag armed.
        world.players[pid].as_mut().unwrap().afk_event_ready = true;
        world.players[pid].as_mut().unwrap().staff_mod_level = 2;
        let mut st = ScriptState::new(mk(), &[]);
        st.active_player = Some(pid);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack.last().copied(), Some(0), "staff are exempt from AFK events");
    }

    #[test]
    fn afk_zone_tracker_marks_a_stationary_player_after_the_window() {
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        let p = world.players[pid].as_mut().unwrap();
        // The first call anchors the zone; each subsequent stationary call ticks
        // the idle counter toward the 1000-tick "afk" threshold.
        for _ in 0..1002 {
            p.update_afk_zones();
        }
        assert!(p.zones_afk(), "a player who never leaves their tile reads as afk");

        // Walking out of the 21×21 box re-anchors and resets the counter.
        p.entity.x += 100;
        p.update_afk_zones();
        assert!(!p.zones_afk(), "leaving the zone clears the afk state");
    }

    #[test]
    fn if_close_op_closes_the_modal() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        // Park a weak-queue entry (a dialog continuation).
        world.players[pid].as_mut().unwrap().enqueue_script(
            player::QueueKind::Weak, midi_script_id(6, 55), vec![ScriptArg::Int(0)], 9);

        let script = Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::IF_CLOSE, op::RETURN],
            int_operands: vec![0, 0],
            string_operands: vec![None; 2],
        });
        let mut st = ScriptState::new(script, &[]);
        st.active_player = Some(pid);
        st.pointer_add(crate::script::state::Pointer::ActivePlayer);
        crate::script::runner::execute(&mut st, &mut world);

        assert!(world.players[pid].as_ref().unwrap().weak_queue.is_empty(),
            "IF_CLOSE closes the modal server-side — clears the weak queue");
    }

    #[test]
    fn close_modal_clears_the_weak_queue() {
        use protocol::client::ClientMessage;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        world.players[pid].as_mut().unwrap().enqueue_script(
            player::QueueKind::Weak, midi_script_id(6, 55), vec![ScriptArg::Int(0)], 5);
        world.handle_message(pid, ClientMessage::CloseModal);
        world.cycle(); // process_queues -> close_modal clears the weak queue
        assert!(world.players[pid].as_ref().unwrap().weak_queue.is_empty(),
                "CLOSE_MODAL clears the weak queue");
    }

    #[test]
    fn logout_waits_until_the_player_is_free() {
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        // Action-lock the player, then request a logout.
        world.players[pid].as_mut().unwrap().delayed_until = world.tick as i32 + 3;
        world.players[pid].as_mut().unwrap().request_logout = true;

        world.cycle(); // busy → logout deferred, request kept
        assert!(!world.players[pid].as_ref().unwrap().logging_out,
            "an action-locked player isn't logged out");
        assert!(world.players[pid].as_ref().unwrap().request_logout,
            "the logout request persists while busy");

        // Release the lock; the next cycle completes the logout.
        world.players[pid].as_mut().unwrap().delayed_until = -1;
        world.cycle();
        assert!(world.players[pid].as_ref().unwrap().logging_out,
            "once free, the player logs out");
    }

    #[test]
    fn p_logout_request_is_granted_by_process_logouts() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        // P_LOGOUT, RETURN — requests a logout (runs protected via the queue).
        let script = Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::P_LOGOUT, op::RETURN],
            int_operands: vec![0, 0],
            string_operands: vec![None; 2],
        });
        world.enqueue_engine(pid, script, vec![], 0);
        assert!(!world.players[pid].as_ref().unwrap().logging_out, "not logging out yet");
        world.cycle(); // engine queue requests logout; process_logouts grants it
        assert!(world.players[pid].as_ref().unwrap().logging_out,
                "an unprevented logout request is granted this tick");
    }

    #[test]
    fn p_preventlogout_refuses_then_allows_logout() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        let t0 = world.tick as i32;

        // [setup] PUSH 10, PUSH "busy", P_PREVENTLOGOUT — antilog for 10 ticks.
        let setup = Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_STRING,
                          op::P_PREVENTLOGOUT, op::RETURN],
            int_operands: vec![10, 0, 0, 0],
            string_operands: vec![None, Some("busy".into()), None, None],
        });
        world.enqueue_engine(pid, setup, vec![], 0);
        world.cycle(); // runs P_PREVENTLOGOUT at tick t0
        {
            let p = world.players[pid].as_ref().unwrap();
            assert_eq!(p.prevent_logout_until, t0 + 10, "antilog set to now + delay");
            assert_eq!(p.prevent_logout_message.as_deref(), Some("busy"));
        }

        // Request a logout while prevented → refused, message consumed.
        {
            let p = world.players[pid].as_mut().unwrap();
            p.request_logout = true;
        }
        world.cycle();
        {
            let p = world.players[pid].as_ref().unwrap();
            assert!(!p.logging_out, "logout refused while prevented");
            assert!(p.prevent_logout_message.is_none(), "the prevention message is shown once");
            assert!(!p.request_logout, "the request is consumed either way");
        }

        // Clear the prevention and re-request → granted.
        {
            let p = world.players[pid].as_mut().unwrap();
            p.prevent_logout_until = -1;
            p.request_logout = true;
        }
        world.cycle();
        assert!(world.players[pid].as_ref().unwrap().logging_out,
                "logout granted once no longer prevented");
    }

    #[test]
    fn if_setrotation_sends_packet_and_setresumebuttons_stores_state() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        let mk = |opcodes: Vec<u16>, ints: Vec<i32>| {
            let n = opcodes.len();
            Arc::new(ScriptFile {
                id: 0,
                info: ScriptInfo {
                    script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                    parameter_types: vec![], pcs: vec![], lines: vec![],
                },
                int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
                switch_tables: vec![],
                opcodes, int_operands: ints, string_operands: vec![None; n],
            })
        };
        // IF_SETROTATION(40, 16, 32)
        world.enqueue_engine(pid, mk(
            vec![op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT,
                 op::IF_SETROTATION, op::RETURN],
            vec![40, 16, 32, 0, 0]), vec![], 0);
        // IF_SETRESUMEBUTTONS(1, 2, 3, 4, 5)
        world.enqueue_engine(pid, mk(
            vec![op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT,
                 op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT, op::IF_SETRESUMEBUTTONS, op::RETURN],
            vec![1, 2, 3, 4, 5, 0, 0]), vec![], 0);

        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle();
        assert!(world.players[pid].as_ref().unwrap().out.iter().any(|m| m.opcode == 217),
                "IF_SETROTATION sent the rev1 IF_SETROTATESPEED packet (217)");
        assert_eq!(world.players[pid].as_ref().unwrap().resume_buttons, [1, 2, 3, 4, 5],
                   "IF_SETRESUMEBUTTONS stored the five components");
    }

    #[test]
    fn set_player_op_sends_packet_and_rejects_a_bad_index() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        // PUSH index, PUSH primary(0), PUSH "label", SET_PLAYER_OP, RETURN.
        let mk = |index: i32, label: &str| Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT,
                          op::PUSH_CONSTANT_STRING, op::SET_PLAYER_OP, op::RETURN],
            int_operands: vec![index, 0, 0, 0, 0],
            string_operands: vec![None, None, Some(label.to_string()), None, None],
        });

        // A valid slot (2 = "Follow") sends the SET_PLAYER_OP packet (164).
        world.enqueue_engine(pid, mk(2, "Follow"), vec![], 0);
        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle();
        assert!(world.players[pid].as_ref().unwrap().out.iter().any(|m| m.opcode == 164),
                "a valid slot sends SET_PLAYER_OP");

        // An out-of-range slot (9) aborts the op — no packet.
        world.enqueue_engine(pid, mk(9, "Bad"), vec![], 0);
        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle();
        assert!(world.players[pid].as_ref().unwrap().out.iter().all(|m| m.opcode != 164),
                "an out-of-range slot sends no SET_PLAYER_OP");
    }

    fn finduid_script(uid: i32, op_code: u16) -> Arc<crate::script::file::ScriptFile> {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::PUSH_CONSTANT_INT, op_code, op::RETURN],
            int_operands: vec![uid, 0, 0],
            string_operands: vec![None; 3],
        })
    }

    #[test]
    fn npc_range_measures_chebyshev_distance() {
        use crate::script::opcode as op;
        let mut world = World::new();
        let nid = world.add_npc(11, 3222, 3222, 0).unwrap();
        let run = |world: &mut World, coord: i32| -> i32 {
            let mut st = ScriptState::new(finduid_script(coord, op::NPC_RANGE), &[]);
            st.active_npc = Some(nid);
            crate::script::runner::execute(&mut st, world);
            st.int_stack.last().copied().unwrap()
        };
        // (3225,3222) is three tiles east → distance 3.
        assert_eq!(run(&mut world, (3225 << 14) | 3222), 3);
        // Same tile → 0.
        assert_eq!(run(&mut world, (3222 << 14) | 3222), 0);
        // A different plane → -1.
        assert_eq!(run(&mut world, (1 << 28) | (3222 << 14) | 3222), -1);
    }

    #[test]
    fn a_delayed_npc_holds_still_until_the_lock_elapses() {
        let mut world = World::new();
        let nid = world.add_npc(11, 3222, 3222, 0).unwrap();
        let start_x = world.npcs[nid].as_ref().unwrap().entity.x;
        // Queue a walk east, then action-lock the npc (as NPC_DELAY would).
        let lock = world.tick as i32 + 2;
        {
            let npc = world.npcs[nid].as_mut().unwrap();
            npc.entity.queue_waypoints(&[(3225, 3222)]);
            npc.delayed_until = lock;
        }
        world.cycle();
        assert_eq!(world.npcs[nid].as_ref().unwrap().entity.x, start_x,
            "a NPC_DELAY-locked npc doesn't walk its route");

        // Once the lock is released the npc resumes walking the queued route.
        world.npcs[nid].as_mut().unwrap().delayed_until = -1;
        world.cycle();
        assert!(world.npcs[nid].as_ref().unwrap().entity.x > start_x,
            "an unlocked npc advances along its waypoints");
    }

    #[test]
    fn npc_findexact_sets_the_active_npc_at_a_tile() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let nid = world.add_npc(11, 3222, 3222, 0).unwrap();
        let run = |world: &mut World, coord: i32, type_id: i32| -> (i32, Option<usize>) {
            let script = Arc::new(ScriptFile {
                id: 0,
                info: ScriptInfo {
                    script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                    parameter_types: vec![], pcs: vec![], lines: vec![],
                },
                int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
                switch_tables: vec![],
                opcodes: vec![op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT,
                              op::NPC_FINDEXACT, op::RETURN],
                int_operands: vec![coord, type_id, 0, 0],
                string_operands: vec![None; 4],
            });
            let mut st = ScriptState::new(script, &[]);
            crate::script::runner::execute(&mut st, world);
            (st.int_stack.last().copied().unwrap(), st.active_npc)
        };
        let coord = (3222 << 14) | 3222;
        assert_eq!(run(&mut world, coord, 11), (1, Some(nid)), "exact tile + type resolves the npc");
        assert_eq!(run(&mut world, coord, 99).0, 0, "wrong type → not found");
        assert_eq!(run(&mut world, (3300 << 14) | 3300, 11).0, 0, "wrong tile → not found");
    }

    #[test]
    fn npc_findall_iterates_matching_npcs_nearest_first() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let c = world.add_npc(11, 3222, 3222, 0).unwrap();         // dist 0
        let _m = world.add_npc(11, 3224, 3222, 0).unwrap();        // dist 2
        let _f = world.add_npc(11, 3225, 3222, 0).unwrap();        // dist 3
        let _wrong = world.add_npc(99, 3223, 3222, 0).unwrap();    // dist 1, wrong type
        let _far = world.add_npc(11, 3240, 3222, 0).unwrap();      // dist 18 > 5
        let coord = (3222i32 << 14) | 3222;

        // PUSH coord, PUSH 11, PUSH 5, PUSH 0, NPC_FINDALL, <find_then…>, RETURN.
        let mk = |find_then: Vec<u16>| {
            let mut opcodes = vec![op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT,
                                   op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT, op::NPC_FINDALL];
            let mut ints = vec![coord, 11, 5, 0, 0];
            for o in &find_then { opcodes.push(*o); ints.push(0); }
            opcodes.push(op::RETURN);
            ints.push(0);
            let n = opcodes.len();
            Arc::new(ScriptFile {
                id: 0,
                info: ScriptInfo {
                    script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                    parameter_types: vec![], pcs: vec![], lines: vec![],
                },
                int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
                switch_tables: vec![],
                opcodes, int_operands: ints, string_operands: vec![None; n],
            })
        };

        // One FINDNEXT yields the nearest match (the centre npc), two left.
        let mut st = ScriptState::new(mk(vec![op::NPC_FINDNEXT]), &[]);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.active_npc, Some(c), "first FINDNEXT yields the nearest npc");
        assert_eq!(st.int_stack.last().copied(), Some(1));
        assert_eq!(st.npc_iterator.len(), 2, "two matches remain");

        // Four FINDNEXTs → three matches then exhausted (wrong type + far excluded).
        let mut st = ScriptState::new(mk(vec![op::NPC_FINDNEXT; 4]), &[]);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack, vec![1, 1, 1, 0], "exactly three matches, then 0");
    }

    fn iter_script(push_ints: &[i32], find_op: u16, nexts: usize)
        -> Arc<crate::script::file::ScriptFile> {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut opcodes = vec![op::PUSH_CONSTANT_INT; push_ints.len()];
        let mut ints = push_ints.to_vec();
        opcodes.push(find_op);
        ints.push(0);
        for _ in 0..nexts {
            opcodes.push(op::NPC_FINDNEXT);
            ints.push(0);
        }
        opcodes.push(op::RETURN);
        ints.push(0);
        let n = opcodes.len();
        Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes, int_operands: ints, string_operands: vec![None; n],
        })
    }

    #[test]
    fn obj_find_and_read_an_active_ground_item() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        world.drop_obj(1163, 7, 3222, 3222, 0); // 7 of obj 1163, public
        let coord = (3222i32 << 14) | 3222;
        let mk = |opcodes: Vec<u16>, ints: Vec<i32>| {
            let n = opcodes.len();
            Arc::new(ScriptFile {
                id: 0,
                info: ScriptInfo {
                    script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                    parameter_types: vec![], pcs: vec![], lines: vec![],
                },
                int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
                switch_tables: vec![],
                opcodes, int_operands: ints, string_operands: vec![None; n],
            })
        };

        // OBJ_FIND(coord, 1163) → 1, then OBJ_COORD / OBJ_TYPE / OBJ_COUNT.
        let mut st = ScriptState::new(mk(
            vec![op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT, op::OBJ_FIND,
                 op::OBJ_COORD, op::OBJ_TYPE, op::OBJ_COUNT, op::RETURN],
            vec![coord, 1163, 0, 0, 0, 0, 0]), &[]);
        st.active_player = Some(pid);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack, vec![1, coord, 1163, 7],
                   "found + packed coord + type + count");

        // A different type at the tile → not found.
        let mut st = ScriptState::new(mk(
            vec![op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT, op::OBJ_FIND, op::RETURN],
            vec![coord, 9999, 0, 0]), &[]);
        st.active_player = Some(pid);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack.last().copied(), Some(0), "missing obj → 0");
    }

    #[test]
    fn loc_findallzone_iterates_the_zone() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        world.add_loc(100, 0, 0, 3220, 3220, 0);  // zone block 402
        world.add_loc(200, 0, 0, 3223, 3223, 0);  // same zone
        world.add_loc(300, 0, 0, 3225, 3225, 0);  // adjacent zone (403)
        let coord = (3220i32 << 14) | 3220;
        let mk = |opcodes: Vec<u16>, ints: Vec<i32>| {
            let n = opcodes.len();
            Arc::new(ScriptFile {
                id: 0,
                info: ScriptInfo {
                    script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                    parameter_types: vec![], pcs: vec![], lines: vec![],
                },
                int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
                switch_tables: vec![],
                opcodes, int_operands: ints, string_operands: vec![None; n],
            })
        };

        // FINDALLZONE + 3 FINDNEXT → only the two same-zone locs, then 0.
        let mut st = ScriptState::new(mk(
            vec![op::PUSH_CONSTANT_INT, op::LOC_FINDALLZONE,
                 op::LOC_FINDNEXT, op::LOC_FINDNEXT, op::LOC_FINDNEXT, op::RETURN],
            vec![coord, 0, 0, 0, 0, 0]), &[]);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack, vec![1, 1, 0], "two same-zone locs, then exhausted");

        // FINDNEXT sets the active loc — LOC_TYPE reads the first one (id 100).
        let mut st = ScriptState::new(mk(
            vec![op::PUSH_CONSTANT_INT, op::LOC_FINDALLZONE,
                 op::LOC_FINDNEXT, op::LOC_TYPE, op::RETURN],
            vec![coord, 0, 0, 0, 0]), &[]);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack, vec![1, 100], "the iterator set the active loc for LOC_TYPE");
    }

    #[test]
    fn loc_change_retypes_and_loc_anim_broadcasts() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3225, 3225, 0).unwrap();
        let coord = (3225i32 << 14) | 3225;
        world.players[pid].as_mut().unwrap().out.clear();

        // LOC_ADD(1530, angle 2, shape 0, -1), LOC_CHANGE(1531, -1), LOC_ANIM(285).
        let opcodes = vec![
            op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT,
            op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT, op::LOC_ADD,
            op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT, op::LOC_CHANGE,
            op::PUSH_CONSTANT_INT, op::LOC_ANIM, op::RETURN,
        ];
        let ints = vec![coord, 1530, 2, 0, -1, 0, 1531, -1, 0, 285, 0, 0];
        let n = opcodes.len();
        let script = Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes, int_operands: ints, string_operands: vec![None; n],
        });
        let mut st = ScriptState::new(script, &[]);
        crate::script::runner::execute(&mut st, &mut world);

        assert_eq!(world.find_loc(3225, 3225, 0, 1531), Some((0, 2)), "retyped to 1531 (shape/angle kept)");
        assert!(world.find_loc(3225, 3225, 0, 1530).is_none(), "old type gone");
        assert!(world.players[pid].as_ref().unwrap().out.iter().any(|m| m.opcode == 6),
                "a LOC_ANIM packet was broadcast");
    }

    #[test]
    fn loc_add_and_del_a_map_object() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let _pid = world.add_player("p".into(), 3225, 3225, 0).unwrap();
        let coord = (3225i32 << 14) | 3225;
        let mk = |opcodes: Vec<u16>, ints: Vec<i32>| {
            let n = opcodes.len();
            Arc::new(ScriptFile {
                id: 0,
                info: ScriptInfo {
                    script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                    parameter_types: vec![], pcs: vec![], lines: vec![],
                },
                int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
                switch_tables: vec![],
                opcodes, int_operands: ints, string_operands: vec![None; n],
            })
        };

        // LOC_ADD(coord, type 1530, angle 2, shape 0, duration -1) — permanent.
        let mut st = ScriptState::new(mk(
            vec![op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT,
                 op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT, op::LOC_ADD, op::RETURN],
            vec![coord, 1530, 2, 0, -1, 0, 0]), &[]);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(world.find_loc(3225, 3225, 0, 1530), Some((0, 2)), "loc spawned (shape 0, angle 2)");

        // LOC_FIND the spawned loc, then LOC_DEL it.
        let mut st = ScriptState::new(mk(
            vec![op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT, op::LOC_FIND,
                 op::PUSH_CONSTANT_INT, op::LOC_DEL, op::RETURN],
            vec![coord, 1530, 0, 0, 0, 0]), &[]);
        crate::script::runner::execute(&mut st, &mut world);
        assert!(world.find_loc(3225, 3225, 0, 1530).is_none(), "loc removed by LOC_DEL");
    }

    #[test]
    fn loc_find_and_read_a_spawned_object() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        // Spawn loc 1530 (shape 0, angle 2) at (3225, 3225, 0).
        world.add_loc(1530, 0, 2, 3225, 3225, 0);
        let coord = (3225i32 << 14) | 3225;
        let mk = |opcodes: Vec<u16>, ints: Vec<i32>| {
            let n = opcodes.len();
            Arc::new(ScriptFile {
                id: 0,
                info: ScriptInfo {
                    script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                    parameter_types: vec![], pcs: vec![], lines: vec![],
                },
                int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
                switch_tables: vec![],
                opcodes, int_operands: ints, string_operands: vec![None; n],
            })
        };

        // LOC_FIND(coord, 1530) → 1, then LOC_COORD / LOC_TYPE / LOC_SHAPE / LOC_ANGLE.
        let mut st = ScriptState::new(mk(
            vec![op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT, op::LOC_FIND,
                 op::LOC_COORD, op::LOC_TYPE, op::LOC_SHAPE, op::LOC_ANGLE, op::RETURN],
            vec![coord, 1530, 0, 0, 0, 0, 0, 0]), &[]);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack, vec![1, coord, 1530, 0, 2],
                   "found + coord + type + shape + angle");

        // A different id at the tile → not found.
        let mut st = ScriptState::new(mk(
            vec![op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT, op::LOC_FIND, op::RETURN],
            vec![coord, 9999, 0, 0]), &[]);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack.last().copied(), Some(0), "missing loc → 0");
    }

    #[test]
    fn obj_add_drops_a_ground_item_owned_by_the_player() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        let coord = (3222i32 << 14) | 3222;

        // OBJ_ADD(coord, 995, 100, duration 50), then OBJ_COUNT.
        let script = Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT,
                          op::PUSH_CONSTANT_INT, op::OBJ_ADD, op::OBJ_COUNT, op::RETURN],
            int_operands: vec![coord, 995, 100, 50, 0, 0, 0],
            string_operands: vec![None; 7],
        });
        let mut st = ScriptState::new(script, &[]);
        st.active_player = Some(pid);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack, vec![100], "OBJ_ADD set the active obj; OBJ_COUNT reads its count");
        assert_eq!(world.find_obj(3222, 3222, 0, 995, pid), Some(100), "added, visible to the owner");

        // A private drop is hidden from other players until it reveals.
        let other = world.add_player("o".into(), 3222, 3222, 0).unwrap();
        assert_eq!(world.find_obj(3222, 3222, 0, 995, other), None, "private to the owner");
    }

    #[test]
    fn obj_del_removes_the_active_ground_item() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        world.drop_obj(1163, 7, 3222, 3222, 0);
        let coord = (3222i32 << 14) | 3222;
        world.players[pid].as_mut().unwrap().out.clear();

        // OBJ_FIND(coord, 1163), OBJ_DEL, OBJ_COUNT.
        let script = Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT, op::OBJ_FIND,
                          op::OBJ_DEL, op::OBJ_COUNT, op::RETURN],
            int_operands: vec![coord, 1163, 0, 0, 0, 0],
            string_operands: vec![None; 6],
        });
        let mut st = ScriptState::new(script, &[]);
        st.active_player = Some(pid);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack, vec![1, 0], "found, then deleted → count 0");
        assert!(world.find_obj(3222, 3222, 0, 1163, pid).is_none(), "removed from the zone");
        assert!(world.players[pid].as_ref().unwrap().out.iter().any(|m| m.opcode == 207),
                "an OBJ_DEL packet was broadcast");
    }

    #[test]
    fn obj_findallzone_iterates_the_zone() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3220, 3220, 0).unwrap();
        world.drop_obj(100, 1, 3220, 3220, 0);   // zone block 402
        world.drop_obj(200, 5, 3223, 3223, 0);   // same zone
        world.drop_obj(300, 1, 3225, 3225, 0);   // adjacent zone (403)
        let coord = (3220i32 << 14) | 3220;
        let mk = |opcodes: Vec<u16>, ints: Vec<i32>| {
            let n = opcodes.len();
            Arc::new(ScriptFile {
                id: 0,
                info: ScriptInfo {
                    script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                    parameter_types: vec![], pcs: vec![], lines: vec![],
                },
                int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
                switch_tables: vec![],
                opcodes, int_operands: ints, string_operands: vec![None; n],
            })
        };

        // FINDALLZONE + 3 FINDNEXT → only the two same-zone objs, then 0.
        let mut st = ScriptState::new(mk(
            vec![op::PUSH_CONSTANT_INT, op::OBJ_FINDALLZONE,
                 op::OBJ_FINDNEXT, op::OBJ_FINDNEXT, op::OBJ_FINDNEXT, op::RETURN],
            vec![coord, 0, 0, 0, 0, 0]), &[]);
        st.active_player = Some(pid);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack, vec![1, 1, 0], "two same-zone objs, then exhausted");

        // FINDNEXT sets the active obj — OBJ_TYPE reads the first one (id 100).
        let mut st = ScriptState::new(mk(
            vec![op::PUSH_CONSTANT_INT, op::OBJ_FINDALLZONE,
                 op::OBJ_FINDNEXT, op::OBJ_TYPE, op::RETURN],
            vec![coord, 0, 0, 0, 0]), &[]);
        st.active_player = Some(pid);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack, vec![1, 100], "the iterator set the active obj for OBJ_TYPE");
    }

    #[test]
    fn npc_findallany_matches_any_type_within_range() {
        use crate::script::opcode as op;
        let mut world = World::new();
        let _c = world.add_npc(11, 3222, 3222, 0).unwrap();    // dist 0
        let _d = world.add_npc(99, 3224, 3222, 0).unwrap();    // dist 2, other type
        let _far = world.add_npc(11, 3240, 3222, 0).unwrap();  // dist 18 > 5
        let coord = (3222i32 << 14) | 3222;
        // NPC_FINDALLANY(coord, distance 5, checkvis 0) + 3 FINDNEXT.
        let mut st = ScriptState::new(iter_script(&[coord, 5, 0], op::NPC_FINDALLANY, 3), &[]);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack, vec![1, 1, 0], "two npcs of any type in range, then 0");
    }

    #[test]
    fn npc_findallzone_iterates_only_the_zone() {
        use crate::script::opcode as op;
        let mut world = World::new();
        let _a = world.add_npc(11, 3220, 3220, 0).unwrap();     // zone block 402
        let _b = world.add_npc(11, 3223, 3223, 0).unwrap();     // same zone
        let _other = world.add_npc(11, 3225, 3225, 0).unwrap(); // adjacent zone (403)
        let coord = (3220i32 << 14) | 3220;
        // NPC_FINDALLZONE(coord) + 3 FINDNEXT.
        let mut st = ScriptState::new(iter_script(&[coord], op::NPC_FINDALLZONE, 3), &[]);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack, vec![1, 1, 0], "only the two same-zone npcs, then 0");
    }

    #[test]
    fn npc_finduid_sets_the_active_npc() {
        use crate::script::opcode as op;
        let mut world = World::new();
        let nid = world.add_npc(42, 3222, 3222, 0).unwrap(); // type 42
        let uid = (42 << 16) | nid as i32;

        let mut st = ScriptState::new(finduid_script(uid, op::NPC_FINDUID), &[]);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack.last().copied(), Some(1), "NPC_FINDUID found the npc");
        assert_eq!(st.active_npc, Some(nid), "active npc set to the resolved slot");

        // A uid with the wrong type does not resolve.
        let mut st = ScriptState::new(finduid_script((99 << 16) | nid as i32, op::NPC_FINDUID), &[]);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack.last().copied(), Some(0), "wrong type → not found");
        assert_eq!(st.active_npc, None, "active npc left unset on a miss");
    }

    #[test]
    fn p_finduid_sets_the_active_player_and_respects_access() {
        use crate::script::opcode as op;
        let mut world = World::new();
        let pid = world.add_player("zezima".into(), 3222, 3222, 0).unwrap();
        let uid = world.players[pid].as_ref().unwrap().uid();

        let mut st = ScriptState::new(finduid_script(uid, op::P_FINDUID), &[]);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack.last().copied(), Some(1), "P_FINDUID found the player");
        assert_eq!(st.active_player, Some(pid), "active player set to the resolved slot");

        // An unknown uid fails.
        let mut st = ScriptState::new(finduid_script(0x7FFF_FFF0, op::P_FINDUID), &[]);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack.last().copied(), Some(0), "unknown uid → not found");

        // A busy (action-locked) target can't be acted on.
        world.players[pid].as_mut().unwrap().delayed_until = world.tick as i32 + 5;
        let mut st = ScriptState::new(finduid_script(uid, op::P_FINDUID), &[]);
        crate::script::runner::execute(&mut st, &mut world);
        assert_eq!(st.int_stack.last().copied(), Some(0), "a busy player can't be found");
    }

    #[test]
    fn bas_anim_ops_override_the_appearance_stance() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        // The bare-server default is the unarmed human stance (run anim 824).
        assert_eq!(world.players[pid].as_ref().unwrap().run_anim, 824, "default run stance");

        // WALKANIM(1234), RUNANIM(-1) via the ops.
        let s = Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::PUSH_CONSTANT_INT, op::WALKANIM,
                          op::PUSH_CONSTANT_INT, op::RUNANIM, op::RETURN],
            int_operands: vec![1234, 0, -1, 0, 0],
            string_operands: vec![None; 5],
        });
        world.enqueue_engine(pid, s, vec![], 0);
        world.cycle();
        let p = world.players[pid].as_ref().unwrap();
        assert_eq!(p.walk_anim, 1234, "WALKANIM op set the walk stance");
        assert_eq!(p.run_anim, -1, "RUNANIM op accepts -1 (no run animation)");

        // The appearance block now carries the overridden walk anim (1234 = 0x04D2),
        // not the hardcoded default — proving the buffer reads the fields.
        let app = p.appearance_bytes();
        assert!(app.windows(2).any(|w| w == [0x04, 0xD2]),
                "appearance block carries the overridden walk anim");
    }

    #[test]
    fn resume_button_click_continues_a_paused_dialog() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        use protocol::client::ClientMessage;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        // P_PAUSEBUTTON, PUSH 55, MIDI_SONG, RETURN — pauses before the midi.
        let script = Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "t".into(), source_file_path: "t".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::P_PAUSEBUTTON, op::PUSH_CONSTANT_INT, op::MIDI_SONG, op::RETURN],
            int_operands: vec![0, 55, 0, 0],
            string_operands: vec![None; 4],
        });
        world.enqueue_engine(pid, script, vec![], 0);
        world.players[pid].as_mut().unwrap().out.clear();
        world.cycle(); // pauses on the button
        assert!(world.players[pid].as_ref().unwrap().out.iter().all(|m| m.opcode != 211),
                "paused before the midi");

        // Register component 5000 as a resume button.
        world.players[pid].as_mut().unwrap().resume_buttons = [5000, -1, -1, -1, -1];

        // A click on a non-resume button does not continue the dialog.
        world.players[pid].as_mut().unwrap().out.clear();
        world.handle_message(pid, ClientMessage::IfButton { op: 1, component: 9999, sub: -1 });
        assert!(world.players[pid].as_ref().unwrap().out.iter().all(|m| m.opcode != 211),
                "a non-resume button leaves the dialog paused");

        // A click on the resume button continues the script → the midi plays.
        world.players[pid].as_mut().unwrap().out.clear();
        world.handle_message(pid, ClientMessage::IfButton { op: 1, component: 5000, sub: -1 });
        assert!(world.players[pid].as_ref().unwrap().out.iter().any(|m| m.opcode == 211),
                "the resume button continues the paused dialog");
    }

    #[test]
    fn npc_walktrigger_fires_when_the_npc_walks() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        use crate::script::provider::ScriptProvider;
        let mut world = World::new();
        let coord = (3225i32 << 14) | 3225;
        // [ai_queue1] spawns an npc whose type is its int arg (the walktrigger arg).
        let ai = Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "q".into(), source_file_path: "q".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 1, string_local_count: 0, int_arg_count: 1, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::PUSH_CONSTANT_INT, op::PUSH_INT_LOCAL,
                          op::PUSH_CONSTANT_INT, op::NPC_ADD, op::RETURN],
            int_operands: vec![coord, 0, 0, 0, 0],
            string_operands: vec![None; 5],
        });
        let mut provider = ScriptProvider::test_empty();
        provider.test_insert_global(trigger::AI_QUEUE1, ai);
        world.scripts = Some(provider);
        let nid = world.add_npc(11, 3222, 3222, 0).unwrap();

        // Arm the walktrigger (queue id 1, arg 99) via the op.
        let setup = Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "s".into(), source_file_path: "s".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT,
                          op::NPC_WALKTRIGGER, op::RETURN],
            int_operands: vec![1, 99, 0, 0],
            string_operands: vec![None; 4],
        });
        let mut st = ScriptState::new(setup, &[]);
        st.active_npc = Some(nid);
        st.pointer_add(crate::script::state::Pointer::ActiveNpc);
        world.dispatch(st);
        assert_eq!(world.npcs[nid].as_ref().unwrap().walk_trigger, 0,
                   "armed (queue id 1 → 0-based 0)");

        let count = |w: &World| w.npcs.iter().flatten().count();
        world.cycle(); // no waypoints → doesn't fire
        assert_eq!(count(&world), 1, "no walk, no trigger");
        assert_eq!(world.npcs[nid].as_ref().unwrap().walk_trigger, 0, "still armed");

        // Give the npc a route → walking fires the trigger once (spawning a
        // type-99 npc — proving the arg was passed) and clears it.
        world.npcs[nid].as_mut().unwrap().entity.queue_waypoints(&[(3226, 3222)]);
        world.cycle();
        assert_eq!(count(&world), 2, "walking fired the walktrigger");
        assert!(world.npcs.iter().flatten().any(|n| n.type_id == 99),
                "the [ai_queue1] script ran with the walktrigger arg (99)");
        assert_eq!(world.npcs[nid].as_ref().unwrap().walk_trigger, -1,
                   "one-shot: cleared after firing");
    }

    #[test]
    fn npc_queue_op_enqueues_and_fires_after_its_delay() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        use crate::script::provider::ScriptProvider;
        let mut world = World::new();
        let coord = (3225i32 << 14) | 3225;
        let mut provider = ScriptProvider::test_empty();
        provider.test_insert_global(trigger::AI_QUEUE1, npc_add_script(coord, 22));
        world.scripts = Some(provider);
        let nid = world.add_npc(11, 3222, 3222, 0).unwrap();

        // NPC_QUEUE(queue_id 1, arg 0, delay 2) via the op (stack: id, arg, delay).
        let setup = Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "s".into(), source_file_path: "s".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT,
                          op::PUSH_CONSTANT_INT, op::NPC_QUEUE, op::RETURN],
            int_operands: vec![1, 0, 2, 0, 0],
            string_operands: vec![None; 5],
        });
        let mut st = ScriptState::new(setup, &[]);
        st.active_npc = Some(nid);
        st.pointer_add(crate::script::state::Pointer::ActiveNpc);
        world.dispatch(st);
        assert_eq!(world.npcs[nid].as_ref().unwrap().queue.len(), 1, "NPC_QUEUE op queued one entry");

        let count = |w: &World| w.npcs.iter().flatten().count();
        world.cycle(); // delay 2 -> 1, not due
        assert_eq!(count(&world), 1);
        world.cycle(); // delay 1 -> 0, fires [ai_queue1]
        assert_eq!(count(&world), 2, "ai_queue fired after its delay");
        world.cycle();
        assert_eq!(count(&world), 2, "a queue entry runs exactly once");
    }

    #[test]
    fn npc_queue_is_frozen_while_action_locked() {
        use crate::script::provider::ScriptProvider;
        let mut world = World::new();
        let coord = (3225i32 << 14) | 3225;
        let mut provider = ScriptProvider::test_empty();
        provider.test_insert_global(trigger::AI_QUEUE1, npc_add_script(coord, 22));
        world.scripts = Some(provider);
        let nid = world.add_npc(11, 3222, 3222, 0).unwrap();
        // Action-lock the npc well ahead, then queue a delay-1 entry.
        world.npcs[nid].as_mut().unwrap().delayed_until = world.tick as i32 + 10;
        world.npcs[nid].as_mut().unwrap().enqueue_script(trigger::AI_QUEUE1, 1, 0);

        let count = |w: &World| w.npcs.iter().flatten().count();
        world.cycle();
        world.cycle();
        assert_eq!(count(&world), 1, "the queue is frozen while the npc is action-locked");

        // Lift the lock — the entry's delay (never decremented) now counts down.
        world.npcs[nid].as_mut().unwrap().delayed_until = -1;
        world.cycle(); // delay 1 -> 0, fires
        assert_eq!(count(&world), 2, "the queue resumes once the lock lifts");
    }

    #[test]
    fn npc_timer_op_arms_and_fires_the_ai_timer_trigger() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        use crate::script::provider::ScriptProvider;
        let mut world = World::new();
        let coord = (3225i32 << 14) | 3225;
        // [ai_timer] spawns a type-22 npc each time it fires.
        let mut provider = ScriptProvider::test_empty();
        provider.test_insert_global(trigger::AI_TIMER, npc_add_script(coord, 22));
        world.scripts = Some(provider);
        let nid = world.add_npc(11, 3222, 3222, 0).unwrap();

        // NPC_SETTIMER(3) via the op (dispatched as an AI script on the npc).
        let setup = Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "s".into(), source_file_path: "s".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::PUSH_CONSTANT_INT, op::NPC_SETTIMER, op::RETURN],
            int_operands: vec![3, 0, 0],
            string_operands: vec![None; 3],
        });
        let mut st = ScriptState::new(setup, &[]);
        st.active_npc = Some(nid);
        st.pointer_add(crate::script::state::Pointer::ActiveNpc);
        world.dispatch(st);
        assert_eq!(world.npcs[nid].as_ref().unwrap().timer_interval, 3,
                   "NPC_SETTIMER op armed the AI timer");

        let count = |w: &World| w.npcs.iter().flatten().count();
        world.cycle();
        world.cycle(); // clocks 1, 2 — not yet due
        assert_eq!(count(&world), 1, "timer hasn't reached its interval");
        world.cycle(); // clock 3 — fires [ai_timer], spawning one
        assert_eq!(count(&world), 2, "ai_timer fired on its interval");
        world.cycle();
        world.cycle();
        world.cycle(); // next interval
        assert_eq!(count(&world), 3, "ai_timer re-fires every interval");
    }

    #[test]
    fn npc_delay_suspends_and_resumes_the_npc_script() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let nid = world.add_npc(11, 3222, 3222, 0).unwrap();
        let coord = (3225i32 << 14) | 3225;
        // NPC_DELAY(1), NPC_ADD(type 22), RETURN — the active npc action-locks,
        // then on resume spawns a second npc (the observable).
        let script = Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "ai".into(), source_file_path: "ai".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![op::PUSH_CONSTANT_INT, op::NPC_DELAY,
                          op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT,
                          op::NPC_ADD, op::RETURN],
            int_operands: vec![1, 0, coord, 22, 0, 0, 0],
            string_operands: vec![None; 7],
        });
        let mut state = ScriptState::new(script, &[]);
        state.active_npc = Some(nid);
        state.pointer_add(crate::script::state::Pointer::ActiveNpc);
        let t0 = world.tick as i32;
        world.dispatch(state); // runs up to NPC_DELAY, then suspends

        let count = |w: &World| w.npcs.iter().flatten().count();
        assert_eq!(count(&world), 1, "suspended on NPC_DELAY (no second npc yet)");
        assert_eq!(world.npcs[nid].as_ref().unwrap().delayed_until, t0 + 2,
                   "npc action-locked until now + 1 + delay");

        world.cycle();
        world.cycle();
        assert_eq!(count(&world), 1, "still action-locked through the delay");
        world.cycle();
        assert_eq!(count(&world), 2, "resumed once the lock elapsed, spawning the second npc");
    }

    #[test]
    fn world_delay_suspends_and_resumes_a_world_script() {
        use crate::script::file::{ScriptFile, ScriptInfo};
        use crate::script::opcode as op;
        let mut world = World::new();
        let coord = (3222i32 << 14) | 3222;
        // NPC_ADD(type 11), WORLD_DELAY(0), NPC_ADD(type 22), RETURN — a world
        // script (no owning player) that spawns one npc, suspends, then spawns
        // a second when it resumes.
        let script = Arc::new(ScriptFile {
            id: 0,
            info: ScriptInfo {
                script_name: "w".into(), source_file_path: "w".into(), lookup_key: -1,
                parameter_types: vec![], pcs: vec![], lines: vec![],
            },
            int_local_count: 0, string_local_count: 0, int_arg_count: 0, string_arg_count: 0,
            switch_tables: vec![],
            opcodes: vec![
                op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT, op::NPC_ADD,
                op::PUSH_CONSTANT_INT, op::WORLD_DELAY,
                op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT, op::PUSH_CONSTANT_INT, op::NPC_ADD,
                op::RETURN,
            ],
            int_operands: vec![coord, 11, 0, 0, 0, 0, coord, 22, 0, 0, 0],
            string_operands: vec![None; 11],
        });
        world.enqueue_world(ScriptState::new(script, &[]), 0);

        let count = |w: &World| w.npcs.iter().flatten().count();
        world.cycle(); // queued with +1 → not due yet
        assert_eq!(count(&world), 0, "world script not due the first cycle");
        world.cycle(); // first half runs, suspends on WORLD_DELAY
        assert_eq!(count(&world), 1, "ran up to WORLD_DELAY, spawning the first npc");
        world.cycle(); // still inside the delay
        assert_eq!(count(&world), 1, "stays suspended through the delay");
        world.cycle(); // resumes from the program counter, runs the rest
        assert_eq!(count(&world), 2, "resumed and spawned the second npc");
    }

    #[test]
    fn move_click_outside_scene_is_rejected() {
        use protocol::client::ClientMessage;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        world.players[pid].as_mut().unwrap().out.clear();
        // A click 200 tiles east (outside the 104-tile scene) is rejected.
        world.handle_message(pid, ClientMessage::MoveClick {
            route: vec![(3422, 3222)], ctrl_held: false,
        });
        let p = world.players[pid].as_ref().unwrap();
        assert!(p.entity.waypoints.is_empty(), "out-of-scene click queues no walk");
        assert!(p.out.iter().any(|m| m.opcode == 161), "UNSET_MAP_FLAG sent on reject");
        // A nearby click queues the walk normally.
        world.handle_message(pid, ClientMessage::MoveClick {
            route: vec![(3225, 3222)], ctrl_held: false,
        });
        assert!(!world.players[pid].as_ref().unwrap().entity.waypoints.is_empty(),
                "in-scene click queues the walk");
    }

    #[test]
    fn ctrl_run_needs_one_percent_energy() {
        use protocol::client::ClientMessage;
        let mut world = World::new();
        let pid = world.add_player("p".into(), 3222, 3222, 0).unwrap();
        // 99/10000 = 0% displayed: ctrl-click must NOT enable a temp run.
        world.players[pid].as_mut().unwrap().run_energy = 99;
        world.handle_message(pid, ClientMessage::MoveClick {
            route: vec![(3225, 3222)], ctrl_held: true,
        });
        assert!(!world.players[pid].as_ref().unwrap().temp_run,
                "no ctrl-run below 1% energy");
        // Exactly 100 (1%) is enough.
        world.players[pid].as_mut().unwrap().run_energy = 100;
        world.handle_message(pid, ClientMessage::MoveClick {
            route: vec![(3225, 3222)], ctrl_held: true,
        });
        assert!(world.players[pid].as_ref().unwrap().temp_run, "ctrl-run at 1% energy");
    }

    #[test]
    fn npc_uid_validates_type() {
        let mut world = World::new();
        let nid = world.add_npc(7, 3222, 3222, 0).unwrap();
        // Correct type in the high bits resolves.
        let uid = (7 << 16) | nid as i32;
        assert_eq!(world.get_npc_by_uid(uid), Some(nid));
        // A stale uid (slot reused by a different type) does not resolve.
        let stale = (4 << 16) | nid as i32;
        assert_eq!(world.get_npc_by_uid(stale), None,
                   "type mismatch -> stale uid rejected");
    }

    #[test]
    fn player_slots_start_at_one() {
        // Slot 0 is reserved (OSRS pids are 1-indexed; 0 = the offline/null
        // sentinel), 1:1 with Engine-TS allocating 1..2046.
        let mut world = World::new();
        let first = world.add_player("a".into(), 3222, 3222, 0).unwrap();
        assert_eq!(first, 1, "first player gets pid 1, not 0");
        assert!(world.players[0].is_none(), "slot 0 stays empty");
        let second = world.add_player("b".into(), 3223, 3222, 0).unwrap();
        assert_eq!(second, 2);
    }

    #[test]
    fn login_sends_engine_init_packets() {
        // 1:1 with the head of Engine-TS Player.onLogin: a fresh login must
        // clear the client's varp cache and animation state before any UI, so a
        // reconnecting player never sees stale varps/animations.
        let mut world = World::new();
        let pid = world.add_player("t".into(), 3222, 3222, 0).unwrap();
        let out = &world.players[pid].as_ref().unwrap().out;
        let ops: Vec<u8> = out.iter().map(|m| m.opcode).collect();
        let cache = ops.iter().position(|&o| o == 129).expect("RESET_CLIENT_VARCACHE");
        let anims = ops.iter().position(|&o| o == 72).expect("RESET_ANIMS");
        assert!(ops.contains(&137), "CHAT_FILTER_SETTINGS sent");
        // The run orb must be initialised on login (UPDATE_RUNENERGY = 41) to
        // 100% (energy starts at max).
        let energy = out.iter().find(|m| m.opcode == 41).expect("UPDATE_RUNENERGY");
        assert_eq!(energy.body, vec![100], "run orb initialised to 100%");
        // Varp cache reset must precede the stat block (opcode 208).
        if let Some(stat) = ops.iter().position(|&o| o == 208) {
            assert!(cache < stat, "varcache reset before stats");
            assert!(anims < stat, "anims reset before stats");
        }
    }

    // Branch 2 of Engine-TS `reorient`: after walking up to a non-pathing
    // subject (a loc/obj/ground click stored as a coord target), an idle
    // entity turns to face that tile, and the coord target is consumed so it
    // only fires once.
    #[test]
    fn coord_target_faced_once_when_stopped() {
        let mut world = World::new();
        let pid = world.add_player("t".into(), 3222, 3222, 0).unwrap();

        {
            let e = &mut world.players[pid].as_mut().unwrap().entity;
            // Subject is three tiles east; pretend the walk to it is done (no
            // waypoints → steps_taken stays 0 this cycle).
            e.set_face_coord_target(3225, 3222);
            // Scribble the persistent orientation south so a no-op reorient
            // would be visible as "still facing south".
            e.face_angle_x = e.x;
            e.face_angle_z = e.z - 1;
        }

        world.cycle();

        let e = &world.players[pid].as_ref().unwrap().entity;
        assert_eq!((e.face_angle_x, e.face_angle_z), (3225, 3222),
                   "idle entity re-faces its coord target");
        assert_eq!(e.orientation_dir(), 4, "orientation resolves to east");
        assert_eq!((e.target_x, e.target_z), (-1, -1),
                   "coord target consumed after one refocus");

        // A second cycle must not re-touch the (now cleared) target.
        world.players[pid].as_mut().unwrap().entity.face_toward(3222, 3221);
        world.cycle();
        let e = &world.players[pid].as_ref().unwrap().entity;
        assert_eq!((e.face_angle_x, e.face_angle_z), (3222, 3221),
                   "no refocus once the coord target is gone");
    }

    // Branch 1 still wins: a live entity target suppresses the coord refocus.
    #[test]
    fn entity_target_suppresses_coord_refocus() {
        let mut world = World::new();
        let pid = world.add_player("a".into(), 3222, 3222, 0).unwrap();
        let other = world.add_player("b".into(), 3224, 3222, 0).unwrap();

        {
            let e = &mut world.players[pid].as_mut().unwrap().entity;
            e.set_face_coord_target(3220, 3222);   // would face west…
            e.set_face_entity((other as i32) + 32768); // …but a player target wins
        }

        world.cycle();

        let e = &world.players[pid].as_ref().unwrap().entity;
        assert_eq!((e.face_angle_x, e.face_angle_z), (3224, 3222),
                   "faces the entity target, not the coord");
        assert_eq!((e.target_x, e.target_z), (3220, 3222),
                   "coord target left intact while an entity target is held");
    }
}
