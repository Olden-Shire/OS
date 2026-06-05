//! Jagex cache reader (`main_file_cache.dat2` + idx) and ConfigType definitions (mirrors Engine-TS `src/cache`).
//!
//! Behavioral truth: the rev1 Java client at `src/main/java/jagex3/io/DataFile.java` and
//! `src/main/java/jagex3/js5/Js5.java`. Only the read path is ported (server doesn't write
//! to its cache the way the client does during JS5 downloads).

pub mod data_file;
pub mod js5;

use std::fs::File;
use std::path::Path;

pub use data_file::DataFile;
pub use js5::{Js5Index, decode_packet};

/// Index of the master directory archive on disk. Holds the JS5 index for each of the 16
/// game archives.
pub const MASTER_ARCHIVE: u8 = 255;

/// Number of game archives (idx0..idx15). The master archive (idx255) lives separately.
pub const ARCHIVE_COUNT: u8 = 16;

const MASTER_MAX_FILE_SIZE: u32 = 500_000;
const ARCHIVE_MAX_FILE_SIZE: u32 = 1_000_000;

/// Top-level cache reader. Opens the shared `dat2` file once per game archive (cheap — they're
/// just file handles, but each needs its own seek cursor) plus the master idx255.
pub struct Cache {
    archives: Vec<DataFile>, // 16 entries indexed by archive id (0..15)
    master: DataFile,
}

impl Cache {
    /// Open a cache directory containing `main_file_cache.dat2` + `main_file_cache.idx{0..15,255}`.
    pub fn open(dir: &Path) -> std::io::Result<Self> {
        let dat_path = dir.join("main_file_cache.dat2");
        let mut archives = Vec::with_capacity(ARCHIVE_COUNT as usize);
        for i in 0..ARCHIVE_COUNT {
            let dat = File::open(&dat_path)?;
            let idx = File::open(dir.join(format!("main_file_cache.idx{i}")))?;
            archives.push(DataFile::new(i, dat, idx, ARCHIVE_MAX_FILE_SIZE));
        }
        let master_dat = File::open(&dat_path)?;
        let master_idx = File::open(dir.join("main_file_cache.idx255"))?;
        let master =
            DataFile::new(MASTER_ARCHIVE, master_dat, master_idx, MASTER_MAX_FILE_SIZE);
        Ok(Self { archives, master })
    }

    /// Raw group bytes (still JS5-compressed) from the given archive.
    pub fn read_raw(&mut self, archive: u8, group_id: u32) -> std::io::Result<Option<Vec<u8>>> {
        self.archives[archive as usize].read(group_id)
    }

    /// Decompressed group bytes. Returns `None` if the group is absent on disk.
    pub fn read_group(
        &mut self,
        archive: u8,
        group_id: u32,
    ) -> std::io::Result<Option<Vec<u8>>> {
        Ok(self.read_raw(archive, group_id)?.map(|raw| decode_packet(&raw)))
    }

    /// Decoded JS5 directory for an archive (read from idx255 → group `archive`).
    pub fn read_index(&mut self, archive: u8) -> std::io::Result<Option<Js5Index>> {
        Ok(self.master.read(u32::from(archive))?.map(|raw| Js5Index::decode(&raw)))
    }
}
