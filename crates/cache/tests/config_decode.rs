//! Decode every entry of every ported ConfigType from the rev1 cache.
//!
//! Panics in opcode dispatch (unknown opcode) will surface here as test failures, which is
//! the whole point: cleanly decoding 100% of records is the "verification" the user asked
//! for. Counts are printed so we can sanity-check against expected rev1 sizes.

use std::path::PathBuf;

use cache::config::{
    EnumType, FloType, FluType, IdkType, InvType, LocType, NpcType, ObjType, SeqType,
    SpotType, VarBitType, VarpType, group,
};
use cache::{CONFIG_ARCHIVE, Cache};

fn cache_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cache")
}

fn decode_all<T>(
    label: &str,
    c: &mut Cache,
    group_id: u32,
    decode: fn(i32, &[u8]) -> T,
) -> Vec<T> {
    let files = c
        .read_files(CONFIG_ARCHIVE, group_id)
        .expect("read_files io")
        .unwrap_or_else(|| panic!("{label}: group {group_id} missing"));
    let decoded: Vec<T> =
        files.into_iter().map(|(id, bytes)| decode(id, &bytes)).collect();
    eprintln!("  {label:<10} group {group_id:>2}: {} entries", decoded.len());
    decoded
}

#[test]
fn small_config_types_decode_cleanly() {
    let mut c = Cache::open(&cache_dir()).expect("open cache");
    decode_all("VarpType", &mut c, group::VARP, VarpType::decode);
    decode_all("VarBitType", &mut c, group::VARBIT, VarBitType::decode);
    decode_all("InvType", &mut c, group::INV, InvType::decode);
    decode_all("EnumType", &mut c, group::ENUM, EnumType::decode);
    decode_all("FloType", &mut c, group::FLO, FloType::decode);
    decode_all("FluType", &mut c, group::FLU, FluType::decode);
    decode_all("SpotType", &mut c, group::SPOT, SpotType::decode);
    decode_all("IdkType", &mut c, group::IDK, IdkType::decode);
}

#[test]
fn npc_type_decodes_cleanly() {
    let mut c = Cache::open(&cache_dir()).expect("open cache");
    let npcs = decode_all("NpcType", &mut c, group::NPC, NpcType::decode);
    // Sanity: rev1 should have >1000 NPCs.
    assert!(npcs.len() > 1000, "only {} npcs decoded", npcs.len());
}

#[test]
fn seq_type_decodes_cleanly() {
    let mut c = Cache::open(&cache_dir()).expect("open cache");
    decode_all("SeqType", &mut c, group::SEQ, SeqType::decode);
}

#[test]
fn obj_type_decodes_cleanly() {
    let mut c = Cache::open(&cache_dir()).expect("open cache");
    let objs = decode_all("ObjType", &mut c, group::OBJ, ObjType::decode);
    // Rev1 has many thousands of items.
    assert!(objs.len() > 5000, "only {} objs decoded", objs.len());
}

#[test]
fn loc_type_decodes_cleanly() {
    let mut c = Cache::open(&cache_dir()).expect("open cache");
    let locs = decode_all("LocType", &mut c, group::LOC, LocType::decode);
    assert!(locs.len() > 5000, "only {} locs decoded", locs.len());
}
