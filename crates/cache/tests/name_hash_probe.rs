//! Diagnostic: which archives carry group_name_hashes in their JS5 index, and how many
//! of those hashes have a match in Content-old's corresponding pack file.

use std::path::Path;

use io::cp1252;

const ARCHIVES: &[(&str, &str)] = &[
    ("anims", "anim.pack"),
    ("bases", "base.pack"),
    ("config", "type.pack"),
    ("interfaces", "interface.pack"),
    ("jagfx", "synth.pack"),
    ("maps", "map.pack"),
    ("songs", "midi.pack"),
    ("models", "model.pack"),
    ("sprites", "sprite.pack"),
    ("textures", "texture.pack"),
    ("binary", "binary.pack"),
    ("jingles", "midi.pack"),
    ("clientscripts", "script.pack"),
    ("fonts", "font.pack"),
    ("vorbis", "vorbis.pack"),
    ("patches", "synth.pack"),
];

#[test]
fn probe_name_hash_coverage() {
    let cache_path = Path::new("../../cache");
    if !cache_path.join("main_file_cache.dat2").exists() {
        eprintln!("skip: no cache");
        return;
    }
    let pack_root = Path::new("../../Content-old/pack");
    if !pack_root.exists() {
        eprintln!("skip: no Content-old/pack");
        return;
    }
    let cache = cache::Cache::open(cache_path).unwrap();

    for archive in 0u8..16 {
        let idx = cache.index(archive);
        let has_hashes = idx.group_name_hashes.is_some();
        let group_count = idx.group_ids.len();
        let (scope, pack_filename) = ARCHIVES
            .iter()
            .find(|(s, _)| *s == cache::ARCHIVE_NAMES[archive as usize])
            .copied()
            .unwrap_or(("?", "?"));
        if !has_hashes {
            eprintln!(
                "archive {archive} ({scope:10}): {group_count} groups, NO name hashes"
            );
            continue;
        }
        // Load pack file if present, build name_hash → name map, count matches.
        let pack_path = pack_root.join(pack_filename);
        if !pack_path.exists() {
            eprintln!(
                "archive {archive} ({scope:10}): {group_count} groups, hashes present, NO pack file ({pack_filename})"
            );
            continue;
        }
        let pack_map = match cache::content::pack_file::read(&pack_path) {
            Ok(m) => m,
            Err(_) => {
                eprintln!("archive {archive} ({scope:10}): pack read failed");
                continue;
            }
        };
        let hash_to_name: std::collections::HashMap<i32, &String> = pack_map
            .values()
            .map(|name| (cp1252::name_hash(name), name))
            .collect();
        let hashes = idx.group_name_hashes.as_ref().unwrap();
        let matched = hashes.iter().filter(|h| hash_to_name.contains_key(h)).count();
        let pct = matched as f64 * 100.0 / group_count as f64;
        eprintln!(
            "archive {archive} ({scope:10}): {group_count} groups, {} pack names, {matched} matched ({pct:.1}%)",
            pack_map.len()
        );
    }
}
