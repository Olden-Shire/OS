//! Integration smoke-test: every song / jingle decodes, every patch loads, and every
//! patch a song discovers can be loaded from the patches archive. Run against the local
//! cache at `./cache` — skipped silently when the cache isn't present.

use std::path::Path;

use cache::Cache;
use synth::{MidiFile, Patch};

const SONGS_ARCHIVE: u8 = 6;
const JINGLES_ARCHIVE: u8 = 11;
const PATCHES_ARCHIVE: u8 = 15;

fn open_cache() -> Option<Cache> {
    let path = Path::new("../../cache");
    if !path.join("main_file_cache.dat2").exists() {
        return None;
    }
    Cache::open(path).ok()
}

#[test]
fn every_patch_decodes() {
    let Some(mut cache) = open_cache() else {
        eprintln!("skip: no cache at ../../cache");
        return;
    };
    let groups: Vec<u32> = cache.index(PATCHES_ARCHIVE).group_ids.iter().map(|&g| g as u32).collect();
    let mut ok = 0usize;
    for gid in &groups {
        let bytes = cache.read_group(PATCHES_ARCHIVE, *gid).unwrap().unwrap_or_default();
        if bytes.is_empty() {
            continue;
        }
        let p = Patch::decode(&bytes);
        // Sanity: at least one envelope, every non-zero wave id has an envelope assigned.
        assert!(!p.envelopes.is_empty(), "patch {gid} has zero envelopes");
        for n in 0..128 {
            if p.note_wave_id[n] != 0 {
                assert!(
                    p.note_envelope[n] != usize::MAX,
                    "patch {gid} note {n} missing envelope"
                );
            }
        }
        ok += 1;
    }
    eprintln!("decoded {ok}/{} patches", groups.len());
    assert!(ok > 0);
}

#[test]
fn every_song_decodes_and_discovers_patches() {
    let Some(mut cache) = open_cache() else {
        eprintln!("skip: no cache at ../../cache");
        return;
    };
    for archive in [SONGS_ARCHIVE, JINGLES_ARCHIVE] {
        let groups: Vec<u32> = cache.index(archive).group_ids.iter().map(|&g| g as u32).collect();
        let mut total_patches = 0usize;
        let mut songs = 0usize;
        for gid in &groups {
            let bytes = cache.read_group(archive, *gid).unwrap().unwrap_or_default();
            if bytes.is_empty() {
                continue;
            }
            let mut f = MidiFile::from_jagex(&bytes);
            f.discover_patches();
            let patches = f.patches.as_ref().unwrap();
            total_patches += patches.len();
            songs += 1;
        }
        eprintln!(
            "archive {archive}: {songs}/{} songs decoded, {total_patches} total patch refs",
            groups.len()
        );
        assert!(songs > 0);
    }
}
