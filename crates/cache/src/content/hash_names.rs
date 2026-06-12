//! Recover original asset names by hashing every candidate name from a reference
//! `pack/` directory (typically Lost City's `Content-old/pack/`) and matching against the
//! `group_name_hashes` stored in our rev1 cache's JS5 indices.
//!
//! Different from [`super::import_names`], which does CRC32 byte-matching of raw asset
//! bodies — this approach is much more powerful for scopes where the byte format may have
//! diverged but the *name* survived (e.g. songs whose container changed but whose name
//! is still `scape_main`).
//!
//! Only useful for archives whose JS5 index actually carries name hashes. From the rev1
//! cache: 5 (maps), 6 (songs), 8 (sprites), 10 (binary), 12 (clientscripts), 13 (fonts).

use std::collections::HashMap;
use std::path::Path;

use io::cp1252;

use crate::content::pack_file;

/// `archive_id → (name_hash → name)` lookup. Populated entries override the default
/// numeric stem in [`super::unpack::group_path`].
pub type ArchiveNameMap = HashMap<u8, HashMap<i32, String>>;

/// Per-archive (pack_filename, archive_id) bindings. Only archives with viable name hashes
/// and a corresponding pack file in `Content-old/pack/` are listed.
const BINDINGS: &[(u8, &str)] = &[
    (6, "midi.pack"),     // songs
    // Maps already get 100% coverage from the brute-force `m{x}_{y}` / `l{x}_{y}` table.
    // Scripts use a different hash algorithm in rev1; no matches when tested.
    // Sprites/binary/fonts have hashes but no Content-old pack file to match against.
];

/// Load name maps for every supported archive. Missing files / unreadable packs are
/// silently skipped — the loader returns whatever it could build.
#[must_use]
pub fn load_from_pack_dir(pack_dir: &Path) -> ArchiveNameMap {
    let mut out = ArchiveNameMap::new();
    for &(archive, pack_filename) in BINDINGS {
        let path = pack_dir.join(pack_filename);
        let Ok(pack) = pack_file::read(&path) else { continue };
        let mut by_hash = HashMap::with_capacity(pack.len());
        for name in pack.values() {
            by_hash.insert(cp1252::name_hash(name), name.clone());
        }
        if !by_hash.is_empty() {
            out.insert(archive, by_hash);
        }
    }
    out
}
