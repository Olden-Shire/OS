//! Typed ConfigType decoders — one module per type, mirroring `jagex3/config/` in the rev1
//! Java client. Each module defines the record's data fields and an `opcode → field`
//! decoder loop. Only the *data* side of the Java classes is ported; rendering / game-logic
//! methods (model loading, animation, etc.) stay on the client.
//!
//! Group IDs inside the config archive (archive 2) match the rev1 client's calls:
//!
//! | Group | Type        |
//! |-------|-------------|
//! | 1     | FluType     |
//! | 3     | IdkType     |
//! | 4     | FloType     |
//! | 5     | InvType     |
//! | 6     | LocType     |
//! | 8     | EnumType    |
//! | 9     | NpcType     |
//! | 10    | ObjType     |
//! | 12    | SeqType     |
//! | 13    | SpotType    |
//! | 14    | VarBitType  |
//! | 16    | VarpType    |

pub mod enum_;
pub mod flo;
pub mod flu;
pub mod idk;
pub mod inv;
pub mod loc;
pub mod npc;
pub mod obj;
pub mod seq;
pub mod spot;
pub mod varbit;
pub mod varp;

pub use enum_::EnumType;
pub use flo::FloType;
pub use flu::FluType;
pub use idk::IdkType;
pub use inv::InvType;
pub use loc::LocType;
pub use npc::NpcType;
pub use obj::ObjType;
pub use seq::SeqType;
pub use spot::SpotType;
pub use varbit::VarBitType;
pub use varp::VarpType;

/// Group ids inside the config archive (archive 2).
pub mod group {
    pub const FLU: u32 = 1;
    pub const IDK: u32 = 3;
    pub const FLO: u32 = 4;
    pub const INV: u32 = 5;
    pub const LOC: u32 = 6;
    pub const ENUM: u32 = 8;
    pub const NPC: u32 = 9;
    pub const OBJ: u32 = 10;
    pub const SEQ: u32 = 12;
    pub const SPOT: u32 = 13;
    pub const VARBIT: u32 = 14;
    pub const VARP: u32 = 16;
}

use io::Packet;

/// Read a 1-byte count then `n` (src, dst) pairs of u16 — shared by SpotType, IdkType,
/// NpcType, ObjType, and LocType for their recol/retex tables.
pub(super) fn read_pairs(p: &mut Packet, src: &mut Vec<i16>, dst: &mut Vec<i16>) {
    let n = p.g1() as usize;
    src.clear();
    dst.clear();
    src.reserve(n);
    dst.reserve(n);
    for _ in 0..n {
        src.push(p.g2() as i16);
        dst.push(p.g2() as i16);
    }
}
