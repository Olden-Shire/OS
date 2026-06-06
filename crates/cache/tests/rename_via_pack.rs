//! Rename a file in the Content tree, update the corresponding `.pack` entry, and verify
//! the repacked cache is still byte-identical to the original. This is the acceptance
//! criterion for the .pack rename mechanism — without it, renaming would either lose the
//! ID mapping or change the resulting CRC.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cache::content::{pack, pack_file, unpack};
use cache::maps::XteaKeys;
use cache::{ARCHIVE_COUNT, Cache};

fn cache_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cache")
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("clean scratch");
    }
    dir
}

#[test]
fn renaming_via_pack_file_preserves_crc() {
    let original_dir = cache_dir();
    let content_dir = scratch("rn_content");
    let repacked_dir = scratch("rn_repacked");

    // 1. Unpack so we have a fresh tree with default names.
    let mut original = Cache::open(&original_dir).expect("open original");
    let keys = XteaKeys::load(&original_dir.join("keys.json")).expect("load keys");
    unpack(&mut original, &keys, &content_dir).expect("unpack");

    // 2. Rename two files across different namespaces:
    //    - models/model_995.dat → models/coins.dat (single-file archive scope)
    //    - config/npc/0.dat     → config/npc/hans.dat (config-type file scope)
    rename_via_pack(&content_dir, "models", "model.pack", 995, "model_995", "coins");
    rename_via_pack(&content_dir, "config/npc", "npc.pack", 0, "0", "hans");

    // 3. Pack and verify byte-identity per group.
    pack(&content_dir, &repacked_dir).expect("pack");
    let mut repacked = Cache::open(&repacked_dir).expect("open repacked");
    let mut compared = 0u64;
    for archive in 0..ARCHIVE_COUNT {
        let gids: Vec<i32> = original.index(archive).group_ids.clone();
        for gid in gids {
            let a = original.read_raw(archive, gid as u32).unwrap().unwrap();
            let b = repacked.read_raw(archive, gid as u32).unwrap().unwrap();
            assert_eq!(a, b, "archive {archive} group {gid} differs after rename");
            compared += 1;
        }
    }
    for archive in 0..ARCHIVE_COUNT {
        let a = original.read_master_raw(archive).unwrap().unwrap();
        let b = repacked.read_master_raw(archive).unwrap().unwrap();
        assert_eq!(a, b, "master entry for archive {archive} differs after rename");
    }
    eprintln!("  {compared} groups byte-identical after renames");
}

/// Rename `{stem_old}.dat` → `{stem_new}.dat` inside `{content_dir}/{rel_dir}`,
/// and update the `id=name` line in `{content_dir}/pack/{pack_file}` to point at the new
/// stem.
fn rename_via_pack(
    content_dir: &PathBuf,
    rel_dir: &str,
    pack_file_name: &str,
    id: u32,
    stem_old: &str,
    stem_new: &str,
) {
    let dir = content_dir.join(rel_dir);
    let old_path = dir.join(format!("{stem_old}.dat"));
    let new_path = dir.join(format!("{stem_new}.dat"));
    assert!(
        old_path.exists(),
        "expected {old_path:?} to exist before rename",
    );
    std::fs::rename(&old_path, &new_path).expect("rename");

    let pack_path = content_dir.join("pack").join(pack_file_name);
    let mut map: BTreeMap<u32, String> = pack_file::read(&pack_path).expect("read pack");
    assert_eq!(
        map.get(&id).map(String::as_str),
        Some(stem_old),
        "pack file {pack_file_name} should have id={id} → {stem_old}",
    );
    map.insert(id, stem_new.to_string());
    pack_file::write(&pack_path, &map).expect("write pack");
}
