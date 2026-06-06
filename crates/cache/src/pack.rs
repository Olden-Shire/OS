//! Content-shaped directory tree → cache.
//!
//! Reverses [`crate::unpack::unpack_to_dir`]. Reads each archive's `index.bin` (master
//! entry) plus per-group `.bin` files, writes a fresh `main_file_cache.dat2` plus
//! `idx0..idx15` and `idx255`, allocating sectors sequentially.
//!
//! The sector allocator starts at sector 1 (sector 0 is the "end of chain" sentinel) and
//! writes groups in archive-then-id order. The dat2 file layout differs from Jagex's
//! original (their allocator likely fragments as the cache evolves), but the per-group
//! raw bytes — and therefore the JS5 CRCs the client checks during cache sync — are
//! byte-identical.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use crate::{ARCHIVE_COUNT, ARCHIVE_NAMES, MASTER_ARCHIVE};
use crate::data_file::{IDX_ENTRY_SIZE, SECTOR_HEADER, SECTOR_PAYLOAD, SECTOR_SIZE};

/// Pack the directory at `src` into a cache at `dest`. `dest` is created if missing; any
/// existing `main_file_cache.*` files in it are overwritten.
pub fn pack_from_dir(src: &Path, dest: &Path) -> std::io::Result<PackStats> {
    fs::create_dir_all(dest)?;
    let dat_path = dest.join("main_file_cache.dat2");
    let mut writer = DatWriter::new(File::create(&dat_path)?);
    let mut stats = PackStats::default();

    // Game archives 0..15.
    for archive in 0..ARCHIVE_COUNT {
        let archive_dir = src.join(ARCHIVE_NAMES[archive as usize]);
        let entries = read_archive_groups(&archive_dir)?;
        let mut idx_entries: BTreeMap<u32, (u32, u32)> = BTreeMap::new();
        for (gid, bytes) in &entries {
            let first_sector = writer.write_group(archive, *gid, bytes)?;
            idx_entries.insert(*gid, (bytes.len() as u32, first_sector));
            stats.total_groups += 1;
            stats.total_bytes += bytes.len() as u64;
        }
        write_idx(&dest.join(format!("main_file_cache.idx{archive}")), &idx_entries)?;
    }

    // Master archive (255) — entries come from each archive's index.bin.
    let mut master_entries: BTreeMap<u32, (u32, u32)> = BTreeMap::new();
    for archive in 0..ARCHIVE_COUNT {
        let path = src.join(ARCHIVE_NAMES[archive as usize]).join("index.bin");
        let bytes = fs::read(&path)?;
        let first_sector = writer.write_group(MASTER_ARCHIVE, u32::from(archive), &bytes)?;
        master_entries.insert(u32::from(archive), (bytes.len() as u32, first_sector));
        stats.master_entries += 1;
    }
    write_idx(&dest.join("main_file_cache.idx255"), &master_entries)?;

    Ok(stats)
}

/// Read every `{group_id}.bin` file from an archive directory, sorted by id.
fn read_archive_groups(archive_dir: &Path) -> std::io::Result<Vec<(u32, Vec<u8>)>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(archive_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(stem) = name.strip_suffix(".bin") else { continue };
        if stem == "index" {
            continue;
        }
        let Ok(gid) = stem.parse::<u32>() else { continue };
        out.push((gid, fs::read(entry.path())?));
    }
    out.sort_by_key(|(g, _)| *g);
    Ok(out)
}

/// Write a 6-bytes-per-entry idx file. `entries` is keyed by group id; absent ids become
/// 6 zero bytes (size = 0, first sector = 0 → reads return None).
fn write_idx(path: &Path, entries: &BTreeMap<u32, (u32, u32)>) -> std::io::Result<()> {
    let max_id = entries.keys().copied().max().unwrap_or(0);
    let mut buf = vec![0u8; (max_id as usize + 1) * IDX_ENTRY_SIZE as usize];
    for (&gid, &(size, sector)) in entries {
        let off = gid as usize * IDX_ENTRY_SIZE as usize;
        buf[off] = (size >> 16) as u8;
        buf[off + 1] = (size >> 8) as u8;
        buf[off + 2] = size as u8;
        buf[off + 3] = (sector >> 16) as u8;
        buf[off + 4] = (sector >> 8) as u8;
        buf[off + 5] = sector as u8;
    }
    fs::write(path, buf)
}

struct DatWriter {
    dat: File,
    next_sector: u32,
}

impl DatWriter {
    fn new(dat: File) -> Self {
        Self { dat, next_sector: 1 }
    }

    /// Write a group's payload as a sector chain. Returns the first sector number, which
    /// goes into the idx entry. Empty groups still consume one sector with a 0-length
    /// payload (matches how Jagex's writeToFile handles a 0-size record).
    fn write_group(
        &mut self,
        archive: u8,
        group_id: u32,
        bytes: &[u8],
    ) -> std::io::Result<u32> {
        let first_sector = self.next_sector;
        let mut written = 0usize;
        let mut part: u16 = 0;

        loop {
            let sector_no = self.next_sector;
            let chunk = (bytes.len() - written).min(SECTOR_PAYLOAD);
            let is_last = written + chunk == bytes.len();
            let next_sector = if is_last { 0 } else { sector_no + 1 };

            let mut sector_buf = [0u8; SECTOR_SIZE];
            sector_buf[0] = (group_id >> 8) as u8;
            sector_buf[1] = group_id as u8;
            sector_buf[2] = (part >> 8) as u8;
            sector_buf[3] = part as u8;
            sector_buf[4] = (next_sector >> 16) as u8;
            sector_buf[5] = (next_sector >> 8) as u8;
            sector_buf[6] = next_sector as u8;
            sector_buf[7] = archive;
            sector_buf[SECTOR_HEADER..SECTOR_HEADER + chunk]
                .copy_from_slice(&bytes[written..written + chunk]);

            self.dat.seek(SeekFrom::Start(u64::from(sector_no) * SECTOR_SIZE as u64))?;
            self.dat.write_all(&sector_buf)?;

            written += chunk;
            part += 1;
            self.next_sector += 1;

            if is_last {
                return Ok(first_sector);
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct PackStats {
    pub master_entries: u32,
    pub total_groups: u64,
    pub total_bytes: u64,
}
