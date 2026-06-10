//! Server-side NPC. Mask bits match the rev1 client's
//! getNpcPosExtended flags.

use crate::entity::PathingEntity;

pub const MASK_SPOTANIM: i32 = 0x1;
pub const MASK_FACE_COORD: i32 = 0x2;
pub const MASK_FACE_ENTITY: i32 = 0x4;
pub const MASK_ANIM: i32 = 0x8;
pub const MASK_DAMAGE2: i32 = 0x10;
pub const MASK_SAY: i32 = 0x20;
pub const MASK_CHANGE_TYPE: i32 = 0x40;
pub const MASK_DAMAGE: i32 = 0x80;

pub struct Npc {
    pub nid: usize,
    pub type_id: i32,
    pub entity: PathingEntity,
    /// Queued retype (npc_changetype) — sent via MASK_CHANGE_TYPE.
    pub new_type: i32,
    pub active: bool,
}

impl Npc {
    pub fn new(nid: usize, type_id: i32, x: i32, z: i32, level: i32) -> Npc {
        Npc {
            nid,
            type_id,
            entity: PathingEntity::at(x, z, level),
            new_type: -1,
            active: true,
        }
    }

    pub fn change_type(&mut self, type_id: i32) {
        self.new_type = type_id;
        self.entity.masks |= MASK_CHANGE_TYPE;
    }
}
