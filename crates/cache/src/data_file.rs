//! Low-level sector reader for one archive (`main_file_cache.dat2` + one `idx`).
//!
//! Mirrors `jagex3.io.DataFile` from the rev1 Java client. Only the *read* path is ported —
//! the write path exists in the client to receive JS5 downloads, which the server doesn't do.
//!
//! ## On-disk layout
//!
//! * **idx file** — packed array of 6-byte entries indexed by group id:
//!   `size:u24` (BE) `+ first_sector:u24` (BE).
//! * **dat2 file** — packed array of 520-byte sectors:
//!   `group_id:u16` `+ part:u16` `+ next_sector:u24` `+ archive_id:u8` `+ payload[512]`.
//!
//! A group's payload is reconstructed by chasing the sector chain. The header in each sector
//! is cross-checked against the requested `group_id`, expected `part` number, and `archive_id`
//! to catch corruption or wrong-archive reads.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

pub const SECTOR_SIZE: usize = 520;
pub const SECTOR_HEADER: usize = 8;
pub const SECTOR_PAYLOAD: usize = SECTOR_SIZE - SECTOR_HEADER;
pub const IDX_ENTRY_SIZE: u64 = 6;

pub struct DataFile {
    pub archive: u8,
    pub max_file_size: u32,
    dat: File,
    idx: File,
}

impl DataFile {
    /// Wrap pre-opened dat and idx file handles. The dat handle should be unique to this
    /// `DataFile` (seek positions are not shared); open the file once per archive.
    pub fn new(archive: u8, dat: File, idx: File, max_file_size: u32) -> Self {
        Self { archive, max_file_size, dat, idx }
    }

    /// Read the group with id `group_id` and return its raw bytes (still JS5-compressed).
    ///
    /// Returns `Ok(None)` for missing entries, out-of-range sizes, broken sector chains, or
    /// any header mismatch — matching the Java client's policy of returning null and letting
    /// the caller request the group from the network instead. Returns `Err` only for genuine
    /// IO failures.
    pub fn read(&mut self, group_id: u32) -> std::io::Result<Option<Vec<u8>>> {
        let idx_len = self.idx.metadata()?.len();
        let entry_offset = u64::from(group_id) * IDX_ENTRY_SIZE;
        if idx_len < entry_offset + IDX_ENTRY_SIZE {
            return Ok(None);
        }

        let mut entry = [0u8; 6];
        self.idx.seek(SeekFrom::Start(entry_offset))?;
        self.idx.read_exact(&mut entry)?;
        let size = (u32::from(entry[0]) << 16) | (u32::from(entry[1]) << 8) | u32::from(entry[2]);
        let mut sector =
            (u32::from(entry[3]) << 16) | (u32::from(entry[4]) << 8) | u32::from(entry[5]);

        if size > self.max_file_size {
            return Ok(None);
        }
        let dat_sectors = self.dat.metadata()?.len() / SECTOR_SIZE as u64;
        if sector == 0 || u64::from(sector) > dat_sectors {
            return Ok(None);
        }

        let mut out = vec![0u8; size as usize];
        let mut pos = 0usize;
        let mut part: u16 = 0;
        let mut sector_buf = [0u8; SECTOR_SIZE];

        while pos < size as usize {
            if sector == 0 {
                return Ok(None);
            }

            let chunk = (size as usize - pos).min(SECTOR_PAYLOAD);
            self.dat.seek(SeekFrom::Start(u64::from(sector) * SECTOR_SIZE as u64))?;
            self.dat.read_exact(&mut sector_buf[..SECTOR_HEADER + chunk])?;

            let hdr_group = (u32::from(sector_buf[0]) << 8) | u32::from(sector_buf[1]);
            let hdr_part = (u16::from(sector_buf[2]) << 8) | u16::from(sector_buf[3]);
            let next_sector = (u32::from(sector_buf[4]) << 16)
                | (u32::from(sector_buf[5]) << 8)
                | u32::from(sector_buf[6]);
            let hdr_archive = sector_buf[7];

            if hdr_group != group_id || hdr_part != part || hdr_archive != self.archive {
                return Ok(None);
            }
            if u64::from(next_sector) > dat_sectors {
                return Ok(None);
            }

            out[pos..pos + chunk]
                .copy_from_slice(&sector_buf[SECTOR_HEADER..SECTOR_HEADER + chunk]);
            pos += chunk;
            sector = next_sector;
            part += 1;
        }

        Ok(Some(out))
    }
}
