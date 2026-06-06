//! Cache → Content-shaped directory tree.
//!
//! This is the *lossless* unpack: each group's post-sector-reassembly bytes (still
//! compressed, still XTEA-encrypted for maps) are written verbatim to disk, so the
//! repack pass can produce a byte-identical cache with matching CRCs without having to
//! reproduce Jagex's exact compression parameters.
//!
//! Layout:
//!
//! ```text
//! dest/
//!   anims/        index.bin  0.bin  1.bin  ...     // archive 0
//!   bases/        index.bin  0.bin  ...            // archive 1
//!   config/       index.bin  ...                   // archive 2
//!   ...
//!   patches/      index.bin  ...                   // archive 15
//! ```
//!
//! `index.bin` inside each archive dir is the raw bytes of that archive's master-index
//! entry from `idx255`. On repack it gets re-packed into archive 255 verbatim, so the
//! decoded `Js5Index` doesn't need to be re-encoded (which would risk subtle byte
//! differences from smart-int width choices or trailing padding).

use std::fs;
use std::path::Path;

use crate::{ARCHIVE_COUNT, ARCHIVE_NAMES, Cache};

/// Unpack `cache` into `dest`, creating `dest` if it doesn't exist.
pub fn unpack_to_dir(cache: &mut Cache, dest: &Path) -> std::io::Result<UnpackStats> {
    fs::create_dir_all(dest)?;
    let mut stats = UnpackStats::default();

    for archive in 0..ARCHIVE_COUNT {
        let archive_dir = dest.join(ARCHIVE_NAMES[archive as usize]);
        fs::create_dir_all(&archive_dir)?;

        // Master index entry for this archive, written verbatim.
        let master = cache
            .read_master_raw(archive)?
            .unwrap_or_else(|| panic!("master missing entry for archive {archive}"));
        fs::write(archive_dir.join("index.bin"), &master)?;
        stats.master_entries += 1;

        // Every group's raw bytes.
        let group_ids: Vec<i32> = cache.index(archive).group_ids.clone();
        for gid in group_ids {
            let raw = cache
                .read_raw(archive, gid as u32)?
                .unwrap_or_else(|| panic!("archive {archive} group {gid} missing"));
            stats.total_groups += 1;
            stats.total_bytes += raw.len() as u64;
            fs::write(archive_dir.join(format!("{gid}.bin")), &raw)?;
        }
    }

    Ok(stats)
}

#[derive(Debug, Default)]
pub struct UnpackStats {
    pub master_entries: u32,
    pub total_groups: u64,
    pub total_bytes: u64,
}
