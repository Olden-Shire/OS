//! Pre-startup cache-integrity gate: prove that packing the Content tree reproduces the
//! vanilla cache bit-for-bit before the server is allowed to serve it.
//!
//! Two artifacts:
//!
//! * [`Baseline`] — a per-group CRC32 fingerprint of the known-good (vanilla) cache,
//!   generated once with [`Baseline::generate`] and stored as `crc_baseline.json`.
//! * [`verify_repack`] — packs a Content tree to a temp cache, recomputes every group's
//!   CRC, and diffs against the baseline. It also cross-checks each repacked group against
//!   the cache's own JS5 index `group_checksums` (an independent signal that doesn't rely
//!   on the baseline). Any mismatch means the unpack→edit→pack round-trip is not lossless
//!   — i.e. we would be shipping something other than the vanilla cache.
//!
//! While there is no custom CS2 yet, a clean report is the proof that the new `.cs2`
//! decompile/recompile path (and the rest of the pipeline) is byte-exact.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use io::crc32;
use serde::{Deserialize, Serialize};

use crate::content::pack;
use crate::{Cache, ARCHIVE_COUNT};

/// Per-group CRC fingerprint of a cache, plus per-archive master-index CRCs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Baseline {
    /// `archive → (group_id → crc32 of the raw compressed group)`.
    pub groups: BTreeMap<u8, BTreeMap<u32, u32>>,
    /// `archive → crc32 of the idx255 master entry` (the value the client validates the
    /// downloaded index against; equals `Js5Index::crc`).
    pub master: BTreeMap<u8, u32>,
}

impl Baseline {
    /// Fingerprint every group (and master entry) of the cache at `cache_dir`.
    pub fn generate(cache_dir: &Path) -> std::io::Result<Self> {
        let mut cache = Cache::open(cache_dir)?;
        let mut baseline = Baseline::default();
        for archive in 0..ARCHIVE_COUNT {
            let mut per_archive = BTreeMap::new();
            let group_ids: Vec<i32> = cache.index(archive).group_ids.clone();
            for gid in group_ids {
                let gid = gid as u32;
                if let Some(raw) = cache.read_raw(archive, gid)? {
                    per_archive.insert(gid, crc32::checksum(&raw, 0, raw.len()));
                }
            }
            baseline.groups.insert(archive, per_archive);
            if let Some(raw) = cache.read_master_raw(archive)? {
                baseline.master.insert(archive, crc32::checksum(&raw, 0, raw.len()));
            }
        }
        Ok(baseline)
    }

    /// Read a `crc_baseline.json`.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Write a `crc_baseline.json` (pretty-printed).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)
    }
}

/// One group whose repacked CRC didn't match what was expected.
#[derive(Debug, Clone)]
pub struct Mismatch {
    pub archive: u8,
    /// Group id, or [`MASTER_ENTRY`] when this is a master-index entry.
    pub group: u32,
    pub expected: u32,
    pub got: u32,
    /// `"baseline"` or `"index"` — which reference the repacked CRC disagreed with.
    pub source: &'static str,
}

/// Sentinel `group` value in a [`Mismatch`] that refers to an archive's master entry.
pub const MASTER_ENTRY: u32 = u32::MAX;

/// Result of a repack verification.
#[derive(Debug, Clone, Default)]
pub struct VerifyReport {
    pub groups_checked: u64,
    pub mismatches: Vec<Mismatch>,
}

impl VerifyReport {
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.mismatches.is_empty()
    }
}

impl fmt::Display for VerifyReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_ok() {
            return write!(f, "cache verified: {} groups CRC-identical to baseline", self.groups_checked);
        }
        writeln!(
            f,
            "cache verification FAILED: {} of {} groups diverged",
            self.mismatches.len(),
            self.groups_checked
        )?;
        for m in self.mismatches.iter().take(20) {
            if m.group == MASTER_ENTRY {
                writeln!(f, "  archive {} master: expected {:08x} got {:08x} ({})", m.archive, m.expected, m.got, m.source)?;
            } else {
                writeln!(f, "  {}/{}: expected {:08x} got {:08x} ({})", m.archive, m.group, m.expected, m.got, m.source)?;
            }
        }
        if self.mismatches.len() > 20 {
            writeln!(f, "  … and {} more", self.mismatches.len() - 20)?;
        }
        Ok(())
    }
}

/// Pack the Content tree at `content_dir` into `tmp_dir`, then verify every repacked
/// group's CRC against `baseline` and against the repacked cache's own JS5 index
/// checksums. `tmp_dir` is created/overwritten by the pack step.
pub fn verify_repack(
    content_dir: &Path,
    baseline: &Baseline,
    tmp_dir: &Path,
) -> std::io::Result<VerifyReport> {
    pack::pack(content_dir, tmp_dir)?;
    verify_cache(tmp_dir, baseline)
}

/// Verify an already-packed cache directory (`main_file_cache.*`) against the
/// vanilla `baseline`, group by group, plus an independent cross-check against
/// the cache's own JS5 index checksums. Use this when the cache has already
/// been packed from Content (e.g. the served/browsed cache) so it isn't packed
/// twice.
pub fn verify_cache(cache_dir: &Path, baseline: &Baseline) -> std::io::Result<VerifyReport> {
    verify_cache_with_progress(cache_dir, baseline, &mut |_, _| {})
}

/// Like [`verify_cache`], but reports `(groups_done, groups_total)` as it goes —
/// used by the control-panel splash for a verify progress bar.
pub fn verify_cache_with_progress(
    cache_dir: &Path,
    baseline: &Baseline,
    progress: &mut dyn FnMut(usize, usize),
) -> std::io::Result<VerifyReport> {
    let mut cache = Cache::open(cache_dir)?;
    let mut report = VerifyReport::default();

    // Pre-count groups across all archives for the progress total.
    let total: usize = (0..ARCHIVE_COUNT).map(|a| cache.index(a).group_ids.len()).sum();
    let mut done = 0usize;

    for archive in 0..ARCHIVE_COUNT {
        // Snapshot the index checksums once (avoids borrowing `cache` across read_raw).
        let index_checksums = cache.index(archive).group_checksums.clone();
        let group_ids: Vec<i32> = cache.index(archive).group_ids.clone();
        let empty = BTreeMap::new();
        let baseline_groups = baseline.groups.get(&archive).unwrap_or(&empty);

        for gid in group_ids {
            done += 1;
            if done % 512 == 0 || done == total {
                progress(done, total);
            }
            let gid = gid as u32;
            let Some(raw) = cache.read_raw(archive, gid)? else { continue };
            // Full-bytes CRC is the baseline fingerprint (catches any change, trailer
            // included). The JS5 index stores the CRC of the group *without* its 2-byte
            // version trailer (the trailer is appended after CRCing at pack time), so the
            // independent index cross-check uses that range.
            let got = crc32::checksum(&raw, 0, raw.len());
            let got_no_trailer = crc32::checksum(&raw, 0, raw.len().saturating_sub(2));
            report.groups_checked += 1;

            if let Some(&expected) = baseline_groups.get(&gid)
                && got != expected
            {
                report.mismatches.push(Mismatch { archive, group: gid, expected, got, source: "baseline" });
                continue; // one report per group is enough
            }
            // Independent cross-check against the index's stored checksum.
            if let Some(&stored) = index_checksums.get(gid as usize) {
                let stored = stored as u32;
                if stored != 0 && got_no_trailer != stored {
                    report.mismatches.push(Mismatch { archive, group: gid, expected: stored, got: got_no_trailer, source: "index" });
                }
            }
        }

        // Master entry for this archive.
        if let (Some(raw), Some(&expected)) =
            (cache.read_master_raw(archive)?, baseline.master.get(&archive))
        {
            let got = crc32::checksum(&raw, 0, raw.len());
            if got != expected {
                report.mismatches.push(Mismatch { archive, group: MASTER_ENTRY, expected, got, source: "baseline" });
            }
        }
    }

    Ok(report)
}
