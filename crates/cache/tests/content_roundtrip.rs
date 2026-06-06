//! CRC-identity round-trip through the Content-shaped tree:
//! cache → unpack (decompressed + decrypted) → pack → cache. Every group's raw bytes,
//! every idx255 master entry, every map XTEA-encrypted loc file must come back identical.

use std::path::PathBuf;

use cache::content::{pack, unpack};
use cache::maps::XteaKeys;
use cache::{ARCHIVE_COUNT, ARCHIVE_NAMES, Cache};

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
fn cache_to_content_to_cache_is_byte_identical() {
    let original_dir = cache_dir();
    let content_dir = scratch("ct_content");
    let repacked_dir = scratch("ct_repacked");

    // Unpack: cache → typed content tree.
    let mut original = Cache::open(&original_dir).expect("open original");
    let keys = XteaKeys::load(&original_dir.join("keys.json")).expect("load keys");
    let u_stats = unpack(&mut original, &keys, &content_dir).expect("unpack");
    eprintln!(
        "  unpacked: {} groups across {} archives, {} master entries, {} payload bytes",
        u_stats.total_groups, ARCHIVE_COUNT, u_stats.master_entries, u_stats.total_payload_bytes,
    );

    // Pack: typed content tree → cache.
    let p_stats = pack(&content_dir, &repacked_dir).expect("pack");
    eprintln!(
        "  repacked: {} groups, {} master entries, {} group bytes",
        p_stats.total_groups, p_stats.master_entries, p_stats.total_bytes,
    );

    // Verify byte-identical group reads, including XTEA-encrypted maps.
    let mut repacked = Cache::open(&repacked_dir).expect("open repacked");
    let mut compared = 0u64;
    for archive in 0..ARCHIVE_COUNT {
        let group_ids: Vec<i32> = original.index(archive).group_ids.clone();
        for gid in group_ids {
            let a = original.read_raw(archive, gid as u32).expect("io").expect("orig group");
            let b = repacked.read_raw(archive, gid as u32).expect("io").expect("repack group");
            assert_eq!(
                a, b,
                "{}/{gid} differs (orig {} bytes, repack {} bytes)",
                ARCHIVE_NAMES[archive as usize],
                a.len(),
                b.len(),
            );
            compared += 1;
        }
    }
    eprintln!("  byte-equal groups verified: {compared}");

    // Master index entries.
    for archive in 0..ARCHIVE_COUNT {
        let a = original.read_master_raw(archive).expect("io").expect("orig master");
        let b = repacked.read_master_raw(archive).expect("io").expect("repack master");
        assert_eq!(a, b, "master entry for archive {archive} differs");
    }
    eprintln!("  master entries round-trip cleanly");
}
