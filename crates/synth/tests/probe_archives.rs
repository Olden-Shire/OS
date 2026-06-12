//! One-shot diagnostic — print the layout of the jagfx/vorbis archives so we can verify
//! our wave_id → (group, file) mapping matches what's actually in the cache.

#[test]
fn probe() {
    let path = std::path::Path::new("../../cache");
    if !path.join("main_file_cache.dat2").exists() {
        return;
    }
    let mut c = cache::Cache::open(path).unwrap();
    for arch in [4u8, 14, 15] {
        let idx = c.index(arch);
        eprintln!(
            "archive {arch}: {} groups; group_ids first 16 = {:?}; group_ids last 5 = {:?}",
            idx.group_ids.len(),
            idx.group_ids.iter().take(16).collect::<Vec<_>>(),
            idx.group_ids.iter().rev().take(5).collect::<Vec<_>>()
        );
        for &gid in idx.group_ids.iter().take(5) {
            let fid_count = idx.file_ids.get(gid as usize).map_or(0, |f| f.len());
            let fid_sample: Vec<i32> = idx
                .file_ids
                .get(gid as usize)
                .map_or(Vec::new(), |f| f.iter().take(5).copied().collect());
            eprintln!("  group {gid}: {fid_count} files; first ids: {fid_sample:?}");
        }
    }
}
