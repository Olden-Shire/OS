//! Hash-match a reference content tree (e.g. Lost City's `Content-old/`) against ours and
//! rename files where the bytes are identical. Lets us recover real Jagex-era names for
//! the chunk of rev1 assets that survived unchanged from the era Content-old captures.
//!
//! Only applies to scopes where Content-old stores **raw cache bytes** (post-decompression):
//!
//! | Scope   | Content-old layout         | Extension |
//! |---------|----------------------------|-----------|
//! | models  | `models/**/*.ob2`          | `.ob2`    |
//! | songs   | `songs/*.mid`              | `.mid`    |
//! | jingles | `jingles/*.mid`            | `.mid`    |
//!
//! Scopes where Content-old stores *decoded* / authoring-friendly forms (sprites as PNGs,
//! scripts as `.rs2` source, configs as text) can't be matched this way — the bytes diverge
//! regardless of whether the underlying content is the same.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use io::crc32;

use crate::content::pack_file;

/// Per-scope match counts.
#[derive(Debug, Default)]
pub struct ImportStats {
    pub models: ScopeStats,
    pub songs: ScopeStats,
    pub jingles: ScopeStats,
}

#[derive(Debug, Default)]
pub struct ScopeStats {
    pub source_files: usize,    // files found in Content-old
    pub target_files: usize,    // files iterated in Content
    pub renamed: usize,         // matches that resulted in a rename
    pub already_named: usize,   // matches where current name already equals target
    pub conflicts: usize,       // matches where target name was already taken
}

/// Apply the matcher to all supported scopes.
pub fn import(content_dir: &Path, old_content_dir: &Path) -> std::io::Result<ImportStats> {
    Ok(ImportStats {
        models: apply_scope(content_dir, old_content_dir, ScopeSpec::MODELS)?,
        songs: apply_scope(content_dir, old_content_dir, ScopeSpec::SONGS)?,
        jingles: apply_scope(content_dir, old_content_dir, ScopeSpec::JINGLES)?,
    })
}

struct ScopeSpec {
    content_subdir: &'static str,  // e.g. "models"
    pack_filename: &'static str,    // e.g. "model.pack"
    old_subdir: &'static str,       // e.g. "models" (under Content-old)
    extension: &'static str,        // file ext to match in old dir, no dot
}

impl ScopeSpec {
    const MODELS: Self = Self {
        content_subdir: "models",
        pack_filename: "model.pack",
        old_subdir: "models",
        extension: "ob2",
    };
    const SONGS: Self = Self {
        content_subdir: "songs",
        pack_filename: "song.pack",
        old_subdir: "songs",
        extension: "mid",
    };
    const JINGLES: Self = Self {
        content_subdir: "jingles",
        pack_filename: "jingle.pack",
        old_subdir: "jingles",
        extension: "mid",
    };
}

fn apply_scope(
    content_dir: &Path,
    old_dir: &Path,
    spec: ScopeSpec,
) -> std::io::Result<ScopeStats> {
    let mut stats = ScopeStats::default();
    let scope_dir = content_dir.join(spec.content_subdir);
    let pack_path = content_dir.join("pack").join(spec.pack_filename);
    let old_root = old_dir.join(spec.old_subdir);
    if !old_root.exists() || !scope_dir.exists() || !pack_path.exists() {
        return Ok(stats);
    }

    let index = OldContentIndex::build(&old_root, spec.extension)?;
    stats.source_files = index.total_files;

    let mut pack_map = pack_file::read(&pack_path)?;
    // Build reverse map (current_stem → id) to detect conflicts when renaming.
    let mut taken: std::collections::HashSet<String> = pack_map.values().cloned().collect();

    let entries: Vec<(u32, String)> =
        pack_map.iter().map(|(k, v)| (*k, v.clone())).collect();

    for (id, current_stem) in entries {
        let path = scope_dir.join(format!("{current_stem}.dat"));
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        stats.target_files += 1;
        let Some(matched_name) = index.lookup(&bytes) else { continue };
        if matched_name == current_stem {
            stats.already_named += 1;
            continue;
        }
        if taken.contains(matched_name) {
            stats.conflicts += 1;
            continue;
        }
        let new_path = scope_dir.join(format!("{matched_name}.dat"));
        std::fs::rename(&path, &new_path)?;
        taken.remove(&current_stem);
        taken.insert(matched_name.to_string());
        pack_map.insert(id, matched_name.to_string());
        stats.renamed += 1;
    }

    pack_file::write(&pack_path, &pack_map)?;
    Ok(stats)
}

/// Hash-indexed view of Content-old files for a given scope. Stores full bytes alongside
/// each hash so we can disambiguate CRC32 collisions exactly.
struct OldContentIndex {
    by_hash: HashMap<u32, Vec<(String, Vec<u8>)>>,
    total_files: usize,
}

impl OldContentIndex {
    fn build(dir: &Path, ext: &str) -> std::io::Result<Self> {
        let want_ext = OsStr::new(ext);
        let mut paths: Vec<PathBuf> = Vec::new();
        walk_files(dir, want_ext, &mut paths)?;
        let mut by_hash: HashMap<u32, Vec<(String, Vec<u8>)>> = HashMap::new();
        for path in &paths {
            let bytes = std::fs::read(path)?;
            let h = crc32::checksum(&bytes, 0, bytes.len());
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            by_hash.entry(h).or_default().push((name, bytes));
        }
        Ok(Self { by_hash, total_files: paths.len() })
    }

    fn lookup(&self, bytes: &[u8]) -> Option<&str> {
        let h = crc32::checksum(bytes, 0, bytes.len());
        self.by_hash
            .get(&h)?
            .iter()
            .find(|(_, b)| b == bytes)
            .map(|(n, _)| n.as_str())
    }
}

fn walk_files(dir: &Path, want_ext: &OsStr, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            walk_files(&path, want_ext, out)?;
        } else if path.extension() == Some(want_ext) {
            out.push(path);
        }
    }
    Ok(())
}
