//! Lost City–style `.pack` files: one `id=name` line per record, `//` line comments.
//!
//! These let users rename files in the Content tree (e.g. `npc/0.dat` → `npc/hans.dat`)
//! without losing the ID mapping the pack pipeline needs to reconstruct CRC-identical
//! groups. Pack files live at `Content/pack/{type}.pack` and are the authoritative source
//! for file naming wherever a scope is defined; the per-archive manifest's `path` field
//! is the fallback for things without a pack scope (maps, master, unknown config groups).
//!
//! ## Scopes
//!
//! * Per-archive (`anim.pack`, `base.pack`, `model.pack`, …): keys = group ids.
//! * Per-config-type (`npc.pack`, `obj.pack`, …): keys = file ids within that group of
//!   the config archive (archive 2).
//!
//! Maps (archive 5) and the master archive don't have pack files — their on-disk names
//! come from name-hash resolution or the trivial archive-id mapping.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use crate::CONFIG_ARCHIVE;
use crate::config::group as config_group;

/// Read a `.pack` file. Returns an empty map if the file doesn't exist.
pub fn read(path: &Path) -> std::io::Result<BTreeMap<u32, String>> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let text = std::fs::read_to_string(path)?;
    let mut out = BTreeMap::new();
    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.split("//").next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (k, v) = line.split_once('=').ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}:{}: missing '=' separator: {raw}", path.display(), lineno + 1),
            )
        })?;
        let id: u32 = k.trim().parse().map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}:{}: invalid id: {e}", path.display(), lineno + 1),
            )
        })?;
        out.insert(id, v.trim().to_string());
    }
    Ok(out)
}

/// Write a `.pack` file (sorted by id).
pub fn write(path: &Path, map: &BTreeMap<u32, String>) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = std::fs::File::create(path)?;
    for (id, name) in map {
        writeln!(f, "{id}={name}")?;
    }
    Ok(())
}

/// Pack-file name (stem, no extension) for a non-config archive. `None` means the archive
/// has no pack scope — maps use name-hash resolution, master entries are name-by-id.
#[must_use]
pub fn pack_name_for_archive(archive: u8) -> Option<&'static str> {
    match archive {
        0 => Some("anim"),
        1 => Some("base"),
        // 2 (config) is handled per-group via `pack_name_for_config_group`.
        3 => Some("interface"),
        4 => Some("jagfx"),
        // 5 (maps) — no pack file (names derived from name_hash)
        6 => Some("song"),
        7 => Some("model"),
        8 => Some("sprite"),
        9 => Some("texture"),
        10 => Some("binary"),
        11 => Some("jingle"),
        12 => Some("script"),
        13 => Some("font"),
        14 => Some("vorbis"),
        15 => Some("patch"),
        _ => None,
    }
}

/// Pack-file name (stem) for one of the typed groups inside the config archive (archive 2).
/// `None` for unknown / unrecognised group ids (group_11, group_15, etc.).
#[must_use]
pub fn pack_name_for_config_group(group_id: u32) -> Option<&'static str> {
    match group_id {
        config_group::FLU => Some("flu"),
        config_group::IDK => Some("idk"),
        config_group::FLO => Some("flo"),
        config_group::INV => Some("inv"),
        config_group::LOC => Some("loc"),
        config_group::ENUM => Some("enum"),
        config_group::NPC => Some("npc"),
        config_group::OBJ => Some("obj"),
        config_group::SEQ => Some("seq"),
        config_group::SPOT => Some("spot"),
        config_group::VARBIT => Some("varbit"),
        config_group::VARP => Some("varp"),
        _ => None,
    }
}

/// Resolve "which pack file governs renaming for this manifest entry, if any?"
/// Returns the pack stem (e.g., "model") to look up under `Content/pack/{stem}.pack`.
#[must_use]
pub fn scope_for(archive: u8, group_id: u32) -> Option<&'static str> {
    if archive == CONFIG_ARCHIVE {
        pack_name_for_config_group(group_id)
    } else {
        pack_name_for_archive(archive)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let mut map = BTreeMap::new();
        map.insert(0, "hans".to_string());
        map.insert(995, "coins".to_string());
        map.insert(2, "man2".to_string());
        let tmp = std::env::temp_dir().join("os_pack_file_test.pack");
        write(&tmp, &map).unwrap();
        let back = read(&tmp).unwrap();
        assert_eq!(back, map);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn ignores_comments_and_blank_lines() {
        let tmp = std::env::temp_dir().join("os_pack_file_comments.pack");
        std::fs::write(&tmp, "// header\n0=hans\n\n  // indented comment\n1=man  // trailing\n").unwrap();
        let map = read(&tmp).unwrap();
        assert_eq!(map.get(&0), Some(&"hans".to_string()));
        assert_eq!(map.get(&1), Some(&"man".to_string()));
        assert_eq!(map.len(), 2);
        let _ = std::fs::remove_file(&tmp);
    }
}
