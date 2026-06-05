//! Jagex cache reader (`main_file_cache.dat2` + idx) and ConfigType definitions (mirrors Engine-TS `src/cache`).
//!
//! Behavioral truth: the rev1 Java client at `src/main/java/jagex3/io/DataFile.java` and
//! `src/main/java/jagex3/js5/Js5.java`. Only the read path is ported (server doesn't write
//! to its cache the way the client does during JS5 downloads).

pub mod config;
pub mod configs;
pub mod data_file;
pub mod js5;
pub mod maps;

use std::fs::File;
use std::path::Path;

use io::{Packet, cp1252, xtea};

use crate::maps::{Region, XteaKeys};

pub use configs::Configs;
pub use data_file::DataFile;
pub use js5::{Js5Index, decode_packet};

/// Index of the master directory archive on disk. Holds the JS5 index for each of the 16
/// game archives.
pub const MASTER_ARCHIVE: u8 = 255;

/// Number of game archives (idx0..idx15). The master archive (idx255) lives separately.
pub const ARCHIVE_COUNT: u8 = 16;

/// Archive 2 — holds all ConfigType records (NPCs, items, locs, seqs, etc.) grouped by type.
pub const CONFIG_ARCHIVE: u8 = 2;

/// Archive 5 — holds per-region terrain (`m{x}_{y}`) and loc placement (`l{x}_{y}`,
/// XTEA-encrypted) files, looked up by CP1252 name hash.
pub const MAPS_ARCHIVE: u8 = 5;

const MASTER_MAX_FILE_SIZE: u32 = 500_000;
const ARCHIVE_MAX_FILE_SIZE: u32 = 1_000_000;

/// Top-level cache reader. Opens each archive's `dat2` view + `idx` once, pre-loads every
/// JS5 directory from the master index, and exposes typed reads on top.
pub struct Cache {
    archives: Vec<DataFile>, // 16 entries indexed by archive id (0..15)
    master: DataFile,
    indices: Vec<Js5Index>, // 16 entries, archive -> decoded JS5 directory
}

impl Cache {
    /// Open a cache directory containing `main_file_cache.dat2` + `main_file_cache.idx{0..15,255}`.
    /// Eagerly decodes the 16 archive indices from idx255 — startup cost is a few ms.
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
        let mut master =
            DataFile::new(MASTER_ARCHIVE, master_dat, master_idx, MASTER_MAX_FILE_SIZE);

        let mut indices = Vec::with_capacity(ARCHIVE_COUNT as usize);
        for i in 0..ARCHIVE_COUNT {
            let raw = master.read(u32::from(i))?.unwrap_or_else(|| {
                panic!("master index missing entry for archive {i}")
            });
            indices.push(Js5Index::decode(&raw));
        }

        Ok(Self { archives, master, indices })
    }

    /// Decoded JS5 directory for an archive.
    #[must_use]
    pub fn index(&self, archive: u8) -> &Js5Index {
        &self.indices[archive as usize]
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

    /// Decompressed group bytes, applying XTEA decryption to bytes `[5..]` before
    /// decompression (mirrors `Js5.unpackGroupData`'s `tinydec(key, 5, len)` step). Used
    /// for OSRS map loc files in archive 5, which are XTEA-encrypted with a per-region key.
    pub fn read_group_with_key(
        &mut self,
        archive: u8,
        group_id: u32,
        key: Option<&[i32; 4]>,
    ) -> std::io::Result<Option<Vec<u8>>> {
        let Some(mut raw) = self.read_raw(archive, group_id)? else {
            return Ok(None);
        };
        // Skip XTEA if the key is null or all-zero (unencrypted, per Js5.unpackGroupData).
        //
        // Encrypted range is `5..len - 2`, NOT `5..len` as the Java client's
        // `tinydec(key, 5, data.length)` suggests. The 2-byte version trailer at the end
        // of compressed groups was appended *after* encryption at pack time, so for files
        // where `(len - 5) % 8 ∈ {0, 1}` the Java-style range produces one extra phantom
        // decrypt block that corrupts the gzip CRC. About 19% of rev1 maps failed before
        // this fix; the rest happened to round to a block boundary where it didn't matter.
        if let Some(k) = key
            && (k[0] | k[1] | k[2] | k[3]) != 0
        {
            const VERSION_TRAILER: usize = 2;
            let len = raw.len();
            let end = len.saturating_sub(VERSION_TRAILER);
            xtea::decrypt(&mut raw, k, 5, end);
        }
        Ok(Some(decode_packet(&raw)))
    }

    /// Re-read the index for an archive from `idx255`. Useful if the cache was just rebuilt.
    pub fn reload_index(&mut self, archive: u8) -> std::io::Result<()> {
        let raw = self
            .master
            .read(u32::from(archive))?
            .unwrap_or_else(|| panic!("master missing index for archive {archive}"));
        self.indices[archive as usize] = Js5Index::decode(&raw);
        Ok(())
    }

    /// Decode one map region. Looks up the terrain group `"m{x}_{y}"` and the (optionally
    /// XTEA-encrypted) loc group `"l{x}_{y}"` by CP1252 name hash in the maps Js5 index.
    ///
    /// Returns `None` if the terrain group is missing (i.e. the region doesn't exist in
    /// this cache). A missing loc file is silently treated as "no locs in this region".
    pub fn region(&mut self, x: u32, y: u32, keys: &XteaKeys) -> std::io::Result<Option<Region>> {
        let m_name = format!("m{x}_{y}");
        let l_name = format!("l{x}_{y}");
        let index = self.index(MAPS_ARCHIVE);
        let Some(m_group) = index.find_group_by_hash(cp1252::name_hash(&m_name)) else {
            return Ok(None);
        };
        let l_group = index.find_group_by_hash(cp1252::name_hash(&l_name));

        let Some(terrain) = self.read_group(MAPS_ARCHIVE, m_group)? else {
            return Ok(None);
        };
        let locs = if let Some(g) = l_group {
            self.read_group_with_key(MAPS_ARCHIVE, g, keys.get(x, y))?
        } else {
            None
        };
        Ok(Some(Region::decode(&terrain, locs.as_deref())))
    }

    /// Read every file inside a group and pair each with its file id. The order matches
    /// `Js5Index::file_ids[group]` (declaration order, not file_id-sorted).
    pub fn read_files(
        &mut self,
        archive: u8,
        group_id: u32,
    ) -> std::io::Result<Option<Vec<(i32, Vec<u8>)>>> {
        let file_ids = self.indices[archive as usize].file_ids[group_id as usize].clone();
        let Some(bytes) = self.read_group(archive, group_id)? else {
            return Ok(None);
        };
        let raw_files = unpack_group(&bytes, file_ids.len());
        Ok(Some(file_ids.into_iter().zip(raw_files).collect()))
    }
}

/// Split a multi-file group's uncompressed bytes back into its constituent files.
///
/// Mirrors the second half of `jagex3.js5.Js5.unpackGroupData`. Single-file groups have no
/// trailer; multi-file groups have a chunked size table appended:
///
/// ```text
/// [file_0_chunk_0 || file_1_chunk_0 || ... || file_N_chunk_0
///  || file_0_chunk_1 || ... ||
///  delta_table[ chunks × files × 4 bytes ]
///  chunk_count : u8 ]
/// ```
///
/// Within a chunk, each file's size is the running sum of its delta and all prior deltas
/// in that chunk (not the bare delta).
#[must_use]
pub fn unpack_group(bytes: &[u8], file_count: usize) -> Vec<Vec<u8>> {
    if file_count == 1 {
        return vec![bytes.to_vec()];
    }

    let total = bytes.len();
    let chunk_count = bytes[total - 1] as usize;
    let table_start = total - 1 - file_count * chunk_count * 4;

    // Pass 1: total bytes per file across all chunks.
    let mut sizes = vec![0usize; file_count];
    {
        let mut p = Packet::from_vec(bytes.to_vec());
        p.pos = table_start;
        for _ in 0..chunk_count {
            let mut running: i32 = 0;
            for size in sizes.iter_mut().take(file_count) {
                running = running.wrapping_add(p.g4());
                *size = size.saturating_add(running as usize);
            }
        }
    }

    // Pass 2: copy each chunk's bytes into the right file.
    let mut files: Vec<Vec<u8>> =
        sizes.iter().map(|&s| Vec::with_capacity(s)).collect();
    let mut p = Packet::from_vec(bytes.to_vec());
    p.pos = table_start;
    let mut read_pos = 0usize;
    for _ in 0..chunk_count {
        let mut running: i32 = 0;
        for file in files.iter_mut().take(file_count) {
            running = running.wrapping_add(p.g4());
            let len = running as usize;
            file.extend_from_slice(&bytes[read_pos..read_pos + len]);
            read_pos += len;
        }
    }
    files
}
