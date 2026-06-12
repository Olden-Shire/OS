//! Diagnostic: for a sample of cache models, print the priority distribution so we can
//! verify (1) face_priority is being decoded correctly and (2) our priority pipeline is
//! actually engaged for character-like models.

use std::path::Path;

#[test]
fn print_priority_distribution() {
    let cache_path = Path::new("../../cache");
    if !cache_path.join("main_file_cache.dat2").exists() {
        eprintln!("skip: no cache");
        return;
    }
    let mut cache = cache::Cache::open(cache_path).unwrap();

    let mut total = 0;
    let mut with_priority = 0;
    let mut max_priority_seen = 0i8;
    let mut bucket_counts = [0u64; 16];
    let groups: Vec<u32> = cache
        .index(7)
        .group_ids
        .iter()
        .map(|&g| g as u32)
        .collect();
    for gid in groups.iter().take(2000) {
        let bytes = match cache.read_group(7, *gid) {
            Ok(Some(b)) if !b.is_empty() => b,
            _ => continue,
        };
        let m = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cache::model::Model::decode(&bytes)
        })) {
            Ok(m) => m,
            Err(_) => continue,
        };
        total += 1;
        if let Some(prios) = &m.face_priority {
            with_priority += 1;
            for &p in prios {
                if p > max_priority_seen { max_priority_seen = p; }
                let idx = p.clamp(0, 15) as usize;
                bucket_counts[idx] += 1;
            }
        }
    }
    eprintln!(
        "scanned {total} models — {with_priority} have per-face priority (max value seen = {max_priority_seen})"
    );
    for (i, &c) in bucket_counts.iter().enumerate() {
        if c > 0 {
            eprintln!("  priority {i:>2}: {c:>10} faces");
        }
    }
}
