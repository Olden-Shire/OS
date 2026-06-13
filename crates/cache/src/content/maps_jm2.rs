//! Maps archive ↔ `.jm2` text sync.
//!
//! The unpack pipeline first writes each maps-archive group as a decrypted,
//! decompressed `.dat` payload (`m{x}_{y}.dat` = land stream, `l{x}_{y}.dat` =
//! loc stream). This pass then folds each m/l pair into ONE editable text file
//! per region — `{x}_{y}.jm2` — via [`RawRegion`], whose round-trip is
//! byte-exact for every vanilla region (see `tests/maps_load.rs`).
//!
//! Manifest convention: the two group entries keep their per-group identity in
//! `path` ("m{x}_{y}.jm2" / "l{x}_{y}.jm2"); the PHYSICAL file both resolve to
//! is the prefix-stripped "{x}_{y}.jm2". The pack side re-encodes the matching
//! half from the shared text (see `read_group_payload`'s jm2 branch), after
//! which the normal compress + version-trailer + XTEA flow reproduces the
//! original group bytes.
//!
//! A pair is only converted when the decode→encode round-trip reproduces both
//! `.dat` payloads exactly; otherwise it is left as `.dat` and reported. The
//! pass is idempotent — already-converted entries are skipped.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::content::manifest::ArchiveManifest;
use crate::maps::text::RawRegion;

#[derive(Debug, Default)]
pub struct Jm2Stats {
    /// Regions folded into a `.jm2` this run.
    pub converted: u32,
    /// Entries already in `.jm2` form.
    pub already: u32,
    /// Pairs left as `.dat` (round-trip mismatch or missing half).
    pub kept_dat: u32,
}

/// Fold every `m{r}.dat` + `l{r}.dat` pair under `maps_dir` into `{r}.jm2`,
/// updating `_meta.json` in place. Safe to re-run.
pub fn convert_maps_dir(maps_dir: &Path) -> std::io::Result<Jm2Stats> {
    let meta_path = maps_dir.join("_meta.json");
    let mut manifest: ArchiveManifest = serde_json::from_slice(&fs::read(&meta_path)?)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{meta_path:?}: {e}")))?;

    // region → (index of m entry, index of l entry) for unconverted pairs.
    let mut by_region: BTreeMap<String, (Option<usize>, Option<usize>)> = BTreeMap::new();
    let mut stats = Jm2Stats::default();
    for (i, g) in manifest.groups.iter().enumerate() {
        let p = Path::new(&g.path);
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext.eq_ignore_ascii_case("jm2") {
            stats.already += 1;
            continue;
        }
        if !ext.eq_ignore_ascii_case("dat") {
            continue;
        }
        let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else { continue };
        let (half, region) = stem.split_at(1.min(stem.len()));
        if !matches!(half, "m" | "l")
            || region.is_empty()
            || !region.chars().all(|c| c.is_ascii_digit() || c == '_')
        {
            continue;
        }
        let slot = by_region.entry(region.to_string()).or_default();
        if half == "m" {
            slot.0 = Some(i);
        } else {
            slot.1 = Some(i);
        }
    }

    for (region, (mi, li)) in by_region {
        let (Some(mi), Some(li)) = (mi, li) else {
            stats.kept_dat += 1;
            continue;
        };
        let m_path = maps_dir.join(&manifest.groups[mi].path);
        let l_path = maps_dir.join(&manifest.groups[li].path);
        let land = fs::read(&m_path)?;
        let locs = fs::read(&l_path)?;

        let raw = RawRegion::decode(&land, Some(&locs));
        if raw.encode_land() != land || raw.encode_locs() != locs {
            eprintln!("[maps-jm2] {region}: binary round-trip mismatch — keeping .dat");
            stats.kept_dat += 1;
            continue;
        }
        let text = raw.to_text();
        // Belt and braces: the text form must restore the same streams too.
        let back = match RawRegion::from_text(&text) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[maps-jm2] {region}: text re-parse failed ({e}) — keeping .dat");
                stats.kept_dat += 1;
                continue;
            }
        };
        if back.encode_land() != land || back.encode_locs() != locs {
            eprintln!("[maps-jm2] {region}: text round-trip mismatch — keeping .dat");
            stats.kept_dat += 1;
            continue;
        }

        fs::write(maps_dir.join(format!("{region}.jm2")), &text)?;
        fs::remove_file(&m_path)?;
        fs::remove_file(&l_path)?;
        manifest.groups[mi].path = format!("m{region}.jm2");
        manifest.groups[li].path = format!("l{region}.jm2");
        stats.converted += 1;
    }

    let json = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    fs::write(&meta_path, json)?;
    Ok(stats)
}
