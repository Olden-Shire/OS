//! End-to-end verification of map terrain + loc decoding against the real rev1 cache.
//!
//! Walks every region listed in `keys.json` (781 entries), decodes terrain + decrypts +
//! decodes locs, and accumulates basic stats. A panic in the decoders or a wrong XTEA key
//! would surface here as a cleanly-failing assertion.

use std::path::PathBuf;

use cache::Cache;
use cache::maps::{XteaKeys, mapsquare};

fn cache_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cache")
}

#[test]
fn loads_keys_json() {
    let keys = XteaKeys::load(&cache_dir().join("keys.json")).expect("load keys.json");
    eprintln!("  loaded {} XTEA keys", keys.by_mapsquare.len());
    assert!(keys.by_mapsquare.len() > 500, "too few keys");
    // Spot check: keys.json's first entry was l40_55 → mapsquare 10295.
    assert!(keys.by_mapsquare.contains_key(&mapsquare(40, 55)));
}

#[test]
fn decodes_every_keyed_region() {
    let dir = cache_dir();
    let keys = XteaKeys::load(&dir.join("keys.json")).expect("load keys.json");
    let mut c = Cache::open(&dir).expect("open cache");

    let mut total_locs = 0usize;
    let mut regions_loaded = 0usize;
    let mut regions_with_terrain = 0usize;
    let mut regions_with_locs = 0usize;
    let mut max_locs_in_region = 0usize;
    let mut max_locs_at = (0u32, 0u32);

    for &mapsquare_id in keys.by_mapsquare.keys() {
        let x = mapsquare_id >> 8;
        let y = mapsquare_id & 0xFF;
        let Some(region) = c.region(x, y, &keys).expect("region io") else {
            continue;
        };
        regions_loaded += 1;
        regions_with_terrain += 1;
        if !region.locs.is_empty() {
            regions_with_locs += 1;
            total_locs += region.locs.len();
            if region.locs.len() > max_locs_in_region {
                max_locs_in_region = region.locs.len();
                max_locs_at = (x, y);
            }
        }
    }

    eprintln!("  keyed regions:      {}", keys.by_mapsquare.len());
    eprintln!("  regions loaded:     {regions_loaded}");
    eprintln!("  with terrain:       {regions_with_terrain}");
    eprintln!("  with locs:          {regions_with_locs}");
    eprintln!("  total loc placements: {total_locs}");
    eprintln!(
        "  busiest region:     ({}, {}) with {} locs",
        max_locs_at.0, max_locs_at.1, max_locs_in_region
    );

    assert_eq!(regions_loaded, keys.by_mapsquare.len());
    assert!(total_locs > 1_000_000, "only {total_locs} loc placements");
}

#[test]
fn lumbridge_region_has_sensible_locs() {
    // Lumbridge is at region (50, 50). Should exist and have lots of locs.
    let dir = cache_dir();
    let keys = XteaKeys::load(&dir.join("keys.json")).expect("load keys.json");
    let mut c = Cache::open(&dir).expect("open cache");
    let region = c.region(50, 50, &keys).expect("io").expect("lumbridge missing");

    // Sanity: tile coords stay in bounds.
    for loc in &region.locs {
        assert!(loc.x < 64, "loc x out of bounds: {}", loc.x);
        assert!(loc.z < 64, "loc z out of bounds: {}", loc.z);
        assert!(loc.level < 4, "loc level out of bounds: {}", loc.level);
        assert!(loc.shape < 23, "loc shape out of range: {}", loc.shape);
        assert!(loc.rotation < 4, "loc rotation out of range: {}", loc.rotation);
        assert!(loc.id >= 0, "loc id negative: {}", loc.id);
    }
    eprintln!(
        "  region (50,50): {} locs, unique loc ids: {}",
        region.locs.len(),
        {
            let mut ids: Vec<i32> = region.locs.iter().map(|l| l.id).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.len()
        },
    );
    assert!(region.locs.len() > 500, "lumbridge has only {} locs", region.locs.len());
}
