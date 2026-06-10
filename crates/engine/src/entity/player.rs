//! Server-side player. Wire formats (the appearance buffer + the
//! PLAYER_INFO mask bits) match the rev1 client decode in
//! crates/client/src/login.rs and dash3d/client_player.rs.

use protocol::ServerPacket;

use crate::entity::PathingEntity;

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
    pub total_level: i32,
    /// Bumped whenever appearance changes so observers resend it.
    pub appearance_seq: u32,

    pub run_energy: i32,
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
}

impl Player {
    pub fn new(pid: usize, username: String, x: i32, z: i32, level: i32) -> Player {
        Player {
            pid,
            display_name: username.clone(),
            username,
            entity: PathingEntity::at(x, z, level),
            gender: 0,
            body: DEFAULT_BODY,
            colours: [0; 5],
            headicon_pk: -1,
            headicon_prayer: -1,
            combat_level: 3,
            total_level: 32,
            appearance_seq: 1,
            run_energy: 100,
            varps: vec![0; 4000],
            origin_x: 0,
            origin_z: 0,
            view_players: Vec::new(),
            seen_appearance: vec![0; 2048],
            view_npcs: Vec::new(),
            out: Vec::new(),
            logging_out: false,
        }
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

    pub fn build_appearance(&mut self) {
        self.appearance_seq = self.appearance_seq.wrapping_add(1).max(1);
        self.entity.masks |= MASK_APPEARANCE;
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

        p.p2(ANIM_READY);
        p.p2(ANIM_TURN);
        p.p2(ANIM_WALK);
        p.p2(ANIM_WALK_B);
        p.p2(ANIM_WALK_L);
        p.p2(ANIM_WALK_R);
        p.p2(ANIM_RUN);

        p.pjstr(&self.display_name);
        p.p1(self.combat_level);
        p.p2(self.total_level);

        let mut data = p.data;
        data.truncate(p.pos as usize);
        data
    }
}
