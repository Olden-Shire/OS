//! Confirms 100% round-trip match: every compressed group in the rev1 cache (both bzip2
//! and gzip) recompresses byte-identically with our io::bzip2::compress and
//! io::gzip::compress(level=6) — no fallbacks needed.

use std::path::PathBuf;

use cache::{Cache, ARCHIVE_NAMES, ARCHIVE_COUNT};

fn cache_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cache")
}

#[test]
fn all_compressed_groups_round_trip_byte_identical() {
    let mut c = Cache::open(&cache_dir()).expect("open cache");
    let mut totals = (0u32, 0u32, 0u32, 0u32); // (gzip_match, gzip_total, bzip_match, bzip_total)
    let mut per_archive_gzip = vec![(0u32, 0u32); ARCHIVE_COUNT as usize];

    let keys = cache::maps::XteaKeys::load(&cache_dir().join("keys.json")).expect("keys");
    for archive in 0..ARCHIVE_COUNT {
        let gids: Vec<i32> = c.index(archive).group_ids.clone();
        for gid in gids {
            let mut raw = c.read_raw(archive, gid as u32).unwrap().unwrap();
            // Maps archive: decrypt before testing the compression layer.
            if archive == 5 {
                // Look up by mapsquare encoded in this archive's name hash. The loc
                // file (encrypted) is named "l{x}_{y}"; if we can't find a key we
                // assume it's an unencrypted terrain file.
                let name_hashes = c.index(archive).group_name_hashes.as_ref();
                if let Some(table) = name_hashes {
                    let hash = table[gid as usize];
                    // Brute-force find the matching keys.json entry.
                    let mut applied = false;
                    for (&ms, key) in &keys.by_mapsquare {
                        let x = ms >> 8;
                        let y = ms & 0xFF;
                        if io::cp1252::name_hash(&format!("l{x}_{y}")) == hash {
                            let len = raw.len();
                            io::xtea::decrypt(&mut raw, key, 5, len - 2);
                            applied = true;
                            break;
                        }
                    }
                    let _ = applied; // terrain files have no key — that's fine
                }
            }
            let ctype = raw[0];
            if ctype != 1 && ctype != 2 {
                continue;
            }
            let clen = u32::from_be_bytes(raw[1..5].try_into().unwrap()) as usize;
            let payload = &raw[9..9 + clen];
            match ctype {
                1 => {
                    let decompressed = io::bzip2::decompress(payload);
                    let recompressed = io::bzip2::compress(&decompressed);
                    totals.3 += 1;
                    if recompressed[..] == payload[..] {
                        totals.2 += 1;
                    } else {
                        panic!(
                            "bzip2 mismatch: archive {} ({}), group {gid}",
                            archive, ARCHIVE_NAMES[archive as usize],
                        );
                    }
                }
                2 => {
                    let decompressed = io::gzip::decompress(payload);
                    let recompressed = io::gzip::compress(&decompressed, 6);
                    totals.1 += 1;
                    per_archive_gzip[archive as usize].1 += 1;
                    if recompressed[..] == payload[..] {
                        totals.0 += 1;
                        per_archive_gzip[archive as usize].0 += 1;
                    } else {
                        panic!(
                            "gzip mismatch: archive {} ({}), group {gid}",
                            archive, ARCHIVE_NAMES[archive as usize],
                        );
                    }
                }
                _ => unreachable!(),
            }
        }
    }

    eprintln!("\nPer-archive gzip match (level 6):");
    for archive in 0..ARCHIVE_COUNT {
        let (m, t) = per_archive_gzip[archive as usize];
        if t > 0 {
            eprintln!("  {:<14}  {:>6}/{:<6}", ARCHIVE_NAMES[archive as usize], m, t);
        }
    }
    eprintln!("\nTotals:");
    eprintln!("  bzip2: {}/{}", totals.2, totals.3);
    eprintln!("  gzip:  {}/{}", totals.0, totals.1);
    assert_eq!(totals.0, totals.1);
    assert_eq!(totals.2, totals.3);
}
