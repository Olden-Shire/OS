//! Acid test for unpack/pack: unpack the real rev1 cache, pack it back, and verify every
//! group's raw bytes match the original byte-for-byte. Since CRC32 is computed over the
//! raw group bytes, byte equality implies CRC equality — meaning a real client doing JS5
//! cache sync against the repacked cache will accept our groups without re-downloading.

use std::path::PathBuf;

use cache::{ARCHIVE_COUNT, ARCHIVE_NAMES, Cache, pack, unpack};

fn cache_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cache")
}

/// Workspace-local scratch dir so we can inspect intermediate artifacts after a failure;
/// also keeps things outside the system temp dir.
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("clean scratch");
    }
    dir
}

#[test]
fn unpack_then_pack_gives_crc_identical_groups() {
    let original_dir = cache_dir();
    let content_dir = scratch("rt_content");
    let repacked_dir = scratch("rt_repacked");

    // Unpack.
    let mut original = Cache::open(&original_dir).expect("open original");
    let u_stats = unpack::unpack_to_dir(&mut original, &content_dir).expect("unpack");
    eprintln!(
        "  unpacked: {} archives × {} groups, {} master entries, {} bytes",
        ARCHIVE_COUNT, u_stats.total_groups, u_stats.master_entries, u_stats.total_bytes,
    );

    // Pack.
    let p_stats = pack::pack_from_dir(&content_dir, &repacked_dir).expect("pack");
    eprintln!(
        "  repacked: {} groups, {} master entries, {} bytes",
        p_stats.total_groups, p_stats.master_entries, p_stats.total_bytes,
    );

    // Verify byte-identical group reads across the full cache.
    let mut repacked = Cache::open(&repacked_dir).expect("open repacked");
    let mut total_compared = 0u64;
    for archive in 0..ARCHIVE_COUNT {
        let group_ids: Vec<i32> = original.index(archive).group_ids.clone();
        for gid in group_ids {
            let a = original
                .read_raw(archive, gid as u32)
                .expect("original io")
                .expect("original group missing");
            let b = repacked
                .read_raw(archive, gid as u32)
                .expect("repacked io")
                .expect("repacked group missing");
            assert_eq!(
                a, b,
                "{}/{gid} differs (orig {} bytes, repack {} bytes)",
                ARCHIVE_NAMES[archive as usize],
                a.len(),
                b.len(),
            );
            total_compared += 1;
        }
    }
    eprintln!("  byte-equal groups verified: {total_compared}");

    // Master index entries must also round-trip — the JS5 sync handshake fetches the
    // master index first; if our re-encoded master differs, the client re-pulls everything.
    for archive in 0..ARCHIVE_COUNT {
        let a = original.read_master_raw(archive).expect("io").expect("orig master");
        let b = repacked.read_master_raw(archive).expect("io").expect("repack master");
        assert_eq!(a, b, "master entry for archive {archive} differs");
    }
    eprintln!("  master entries round-trip cleanly");
}
