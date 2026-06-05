//! End-to-end smoke test against the actual rev1 cache committed at `<repo>/cache/`.
//!
//! Verifies that we can open the cache, walk the master index, read every per-archive index,
//! and pull a representative group from each archive. If any of these byte-level mechanics
//! are wrong (sector chains, JS5 wrapper, BZip2/GZip dispatch, index decode), this fails.

use std::path::PathBuf;

use cache::{ARCHIVE_COUNT, Cache};

fn cache_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cache")
}

#[test]
fn opens_cache_directory() {
    let dir = cache_dir();
    assert!(dir.exists(), "rev1 cache not found at {dir:?}");
    Cache::open(&dir).expect("open cache");
}

#[test]
fn reads_master_index_for_every_archive() {
    let mut c = Cache::open(&cache_dir()).expect("open cache");
    let mut total_groups = 0u32;
    for archive in 0..ARCHIVE_COUNT {
        let idx = c
            .read_index(archive)
            .expect("read_index io")
            .unwrap_or_else(|| panic!("archive {archive} index missing"));
        assert!(idx.size > 0, "archive {archive} has 0 groups");
        assert!(matches!(idx.protocol, 5..=7), "archive {archive} unexpected protocol {}", idx.protocol);
        total_groups += idx.size;
        eprintln!(
            "  idx{archive}: proto={} rev={} size={} groups, max_id={}",
            idx.protocol,
            idx.revision,
            idx.size,
            idx.group_ids.iter().max().copied().unwrap_or(0),
        );
    }
    eprintln!("total groups across all archives: {total_groups}");
    assert!(total_groups > 0);
}

#[test]
fn reads_first_group_of_every_archive() {
    let mut c = Cache::open(&cache_dir()).expect("open cache");
    for archive in 0..ARCHIVE_COUNT {
        let idx = c
            .read_index(archive)
            .expect("read_index io")
            .expect("master index missing entry");
        let first_group = idx.group_ids[0] as u32;
        let bytes = c
            .read_group(archive, first_group)
            .expect("read_group io")
            .unwrap_or_else(|| panic!("archive {archive} group {first_group} missing"));
        assert!(
            !bytes.is_empty(),
            "archive {archive} group {first_group} decompressed to 0 bytes",
        );
        eprintln!(
            "  archive {archive} group {first_group}: {} bytes after decompression",
            bytes.len()
        );
    }
}
