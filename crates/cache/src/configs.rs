//! Pre-loaded view of every ConfigType table in the cache.
//!
//! Production callers want all configs in memory at server startup (each record is a few
//! hundred bytes after decode; the whole set is a couple MB). `Configs::load` walks every
//! file in archive 2 once and yields a fully-typed snapshot.
//!
//! Records are stored sparsely keyed by id via `HashMap<i32, T>` since file IDs in a config
//! group are sparse (e.g. rev1's LocType has 26,210 records but max id is well above that).

use std::collections::HashMap;

use crate::config::{
    EnumType, FloType, FluType, IdkType, InvType, LocType, NpcType, ObjType, SeqType,
    SpotType, VarBitType, VarpType, group,
};
use crate::{CONFIG_ARCHIVE, Cache};

#[derive(Debug, Default, Clone)]
pub struct Configs {
    pub enums: HashMap<i32, EnumType>,
    pub flos: HashMap<i32, FloType>,
    pub flus: HashMap<i32, FluType>,
    pub idks: HashMap<i32, IdkType>,
    pub invs: HashMap<i32, InvType>,
    pub locs: HashMap<i32, LocType>,
    pub npcs: HashMap<i32, NpcType>,
    pub objs: HashMap<i32, ObjType>,
    pub seqs: HashMap<i32, SeqType>,
    pub spots: HashMap<i32, SpotType>,
    pub varbits: HashMap<i32, VarBitType>,
    pub varps: HashMap<i32, VarpType>,
}

impl Configs {
    /// Load every ConfigType record from the open cache. Panics if any record fails to
    /// decode — that means the cache isn't actually rev1 or our port has drifted.
    pub fn load(cache: &mut Cache) -> std::io::Result<Self> {
        Ok(Self {
            flus: load_group(cache, group::FLU, FluType::decode)?,
            idks: load_group(cache, group::IDK, IdkType::decode)?,
            flos: load_group(cache, group::FLO, FloType::decode)?,
            invs: load_group(cache, group::INV, InvType::decode)?,
            locs: load_group(cache, group::LOC, LocType::decode)?,
            enums: load_group(cache, group::ENUM, EnumType::decode)?,
            npcs: load_group(cache, group::NPC, NpcType::decode)?,
            objs: load_group(cache, group::OBJ, ObjType::decode)?,
            seqs: load_group(cache, group::SEQ, SeqType::decode)?,
            spots: load_group(cache, group::SPOT, SpotType::decode)?,
            varbits: load_group(cache, group::VARBIT, VarBitType::decode)?,
            varps: load_group(cache, group::VARP, VarpType::decode)?,
        })
    }

    /// Total record count across all 12 tables — handy for verification tests.
    #[must_use]
    pub fn total(&self) -> usize {
        self.enums.len()
            + self.flos.len()
            + self.flus.len()
            + self.idks.len()
            + self.invs.len()
            + self.locs.len()
            + self.npcs.len()
            + self.objs.len()
            + self.seqs.len()
            + self.spots.len()
            + self.varbits.len()
            + self.varps.len()
    }
}

fn load_group<T>(
    cache: &mut Cache,
    group_id: u32,
    decode: fn(i32, &[u8]) -> T,
) -> std::io::Result<HashMap<i32, T>> {
    let files = cache
        .read_files(CONFIG_ARCHIVE, group_id)?
        .unwrap_or_else(|| panic!("config archive missing group {group_id}"));
    Ok(files.into_iter().map(|(id, bytes)| (id, decode(id, &bytes))).collect())
}
