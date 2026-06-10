//! World state + the 600ms game cycle — mirrors the Engine2007
//! reference World.ts shape (read input → process → write output)
//! with the entity/script layers the reference stubs out.

use protocol::client::ClientMessage;
use protocol::server as msg;

use crate::entity::npc::Npc;
use crate::entity::player::Player;
use crate::info;
use crate::script::provider::ScriptProvider;
use crate::script::runner;
use crate::script::state::{ScriptArg, ScriptState};
use crate::script::trigger;

pub const MAX_PLAYERS: usize = 2048;
pub const MAX_NPCS: usize = 8192;

/// Entities are mutually visible within this many tiles (the info
/// protocol's 5-bit signed deltas span -16..15).
pub const VIEW_DISTANCE: i32 = 15;

pub struct World {
    pub players: Vec<Option<Player>>,
    pub npcs: Vec<Option<Npc>>,
    pub tick: u32,
    pub vars: Vec<i32>,
    pub scripts: Option<ScriptProvider>,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    pub fn new() -> World {
        World {
            players: (0..MAX_PLAYERS).map(|_| None).collect(),
            npcs: (0..MAX_NPCS).map(|_| None).collect(),
            // Start with a minute of uptime in case scripts skip
            // testing 0-checks (reference World.ts does the same).
            tick: 100,
            vars: vec![0; 4000],
            scripts: None,
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
        // pid 2047 is reserved (the protocol's local-player sentinel).
        let pid = (0..MAX_PLAYERS - 1).find(|&i| self.players[i].is_none())?;
        let mut player = Player::new(pid, username, x, z, level);

        let (ox, oz) = protocol::server::build_area_origin(x, z);
        player.origin_x = ox;
        player.origin_z = oz;
        player.entity.tele = true;
        player.entity.jump = true;
        player.build_appearance();

        self.players[pid] = Some(player);
        self.on_login(pid);
        Some(pid)
    }

    fn on_login(&mut self, pid: usize) {
        // RuneScript [login,_] trigger when content is loaded;
        // otherwise the engine-level fallback so a bare server is
        // still usable.
        let script = self.scripts.as_ref()
            .and_then(|s| s.get_by_trigger(trigger::LOGIN, -1, -1));
        if let Some(script) = script {
            let mut state = ScriptState::new(script, &[]);
            state.active_player = Some(pid);
            state.pointer_add(crate::script::state::Pointer::ActivePlayer);
            state.pointer_add(crate::script::state::Pointer::ProtectedActivePlayer);
            runner::execute(&mut state, self);
            return;
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
        self.players[pid] = None;
    }

    // ── NPCs ──────────────────────────────────────────────────────

    pub fn add_npc(&mut self, type_id: i32, x: i32, z: i32, level: i32) -> Option<usize> {
        let nid = (0..MAX_NPCS).find(|&i| self.npcs[i].is_none())?;
        self.npcs[nid] = Some(Npc::new(nid, type_id, x, z, level));
        Some(nid)
    }

    pub fn remove_npc(&mut self, nid: usize) {
        self.npcs[nid] = None;
    }

    // ── Input ─────────────────────────────────────────────────────

    pub fn handle_message(&mut self, pid: usize, message: ClientMessage) {
        match message {
            ClientMessage::MoveClick { route, ctrl_held } => {
                if let Some(p) = self.players[pid].as_mut() {
                    p.entity.run = ctrl_held;
                    p.entity.queue_waypoints(&route);
                }
            }
            ClientMessage::ClientCheat { command } => {
                self.handle_cheat(pid, &command);
            }
            ClientMessage::IfButton { .. } => {
                // IF_BUTTON triggers route to RuneScript once the
                // content pack maps components → scripts.
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
                            runner::execute(&mut state, self);
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
        runner::execute(&mut state, self);
        true
    }

    // ── Cycle ─────────────────────────────────────────────────────

    /// One 600ms world tick. The network layer feeds decoded client
    /// messages in via [`World::handle_message`] before calling this,
    /// and drains every player's `out` queue after.
    pub fn cycle(&mut self) {
        // Process movement.
        for slot in self.npcs.iter_mut() {
            if let Some(npc) = slot {
                npc.entity.process_movement();
            }
        }
        for pid in 0..MAX_PLAYERS {
            let Some(player) = self.players[pid].as_mut() else { continue; };
            player.entity.process_movement();

            // Build-area edge → recentre the client's map.
            if player.needs_rebuild() {
                let (x, z) = (player.entity.x, player.entity.z);
                let (ox, oz) = protocol::server::build_area_origin(x, z);
                player.origin_x = ox;
                player.origin_z = oz;
                player.write(msg::rebuild_normal(x, z, |_, _| [0; 4]));
            }
        }

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

        // End-of-cycle transient reset.
        for slot in self.players.iter_mut() {
            if let Some(p) = slot {
                p.entity.reset_transient();
            }
        }
        for slot in self.npcs.iter_mut() {
            if let Some(n) = slot {
                n.entity.reset_transient();
            }
        }

        self.tick += 1;
    }
}
