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

/// Movement + transient update state shared by players and NPCs.
#[derive(Debug, Default, Clone)]
pub struct PathingEntity {
    /// Absolute world tile coords.
    pub x: i32,
    pub z: i32,
    pub level: i32,

    /// Queued checkpoint waypoints (absolute tiles).
    pub waypoints: Vec<(i32, i32)>,
    pub run: bool,

    // Per-tick movement outputs.
    pub walk_dir: i32,
    pub run_dir: i32,
    pub tele: bool,
    pub jump: bool,

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

    pub damage_taken: i32,
    pub damage_type: i32,
    pub health: i32,
    pub total_health: i32,
}

impl PathingEntity {
    pub fn at(x: i32, z: i32, level: i32) -> PathingEntity {
        PathingEntity {
            x,
            z,
            level,
            walk_dir: -1,
            run_dir: -1,
            anim_id: -1,
            spotanim_id: -1,
            face_entity: -1,
            face_x: -1,
            face_z: -1,
            damage_taken: -1,
            damage_type: -1,
            health: 10,
            total_health: 10,
            ..Default::default()
        }
    }

    pub fn teleport(&mut self, x: i32, z: i32, level: i32, jump: bool) {
        self.x = x;
        self.z = z;
        self.level = level;
        self.tele = true;
        self.jump = jump;
        self.waypoints.clear();
    }

    pub fn queue_waypoints(&mut self, route: &[(i32, i32)]) {
        self.waypoints = route.iter().copied().take(25).collect();
    }

    /// Advance one tick of movement: one step toward the current
    /// waypoint (two when running), producing the 3-bit dir codes the
    /// info packets carry.
    pub fn process_movement(&mut self) {
        self.walk_dir = -1;
        self.run_dir = -1;

        if self.tele || self.waypoints.is_empty() {
            return;
        }

        self.walk_dir = self.step_toward_waypoint();
        if self.walk_dir != -1 && self.run {
            self.run_dir = self.step_toward_waypoint();
        }
    }

    fn step_toward_waypoint(&mut self) -> i32 {
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
            self.x += DIRECTION_DELTA[dir].0;
            self.z += DIRECTION_DELTA[dir].1;
            if self.x == tx && self.z == tz {
                self.waypoints.remove(0);
            }
            return dir as i32;
        }
        -1
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
        self.anim_delay = 0;
        self.chat = None;
        self.spotanim_id = -1;
        self.spotanim_height = 0;
        self.spotanim_delay = 0;
        self.face_entity = -1;
        self.face_x = -1;
        self.face_z = -1;
        self.damage_taken = -1;
        self.damage_type = -1;
    }
}
