//! Content-shaped directory tree → cache. Inverse of [`crate::content::unpack::unpack`].
//!
//! Per group: read the decompressed payload (single .dat file or directory of per-file
//! .dats), reassemble multi-file groups via Jagex's chunk format (concat + delta-encoded
//! size table + 0x01 chunk count byte), recompress (`io::gzip::compress(level=6)` for
//! ctype=2, `io::bzip2::compress` for ctype=1, or none for ctype=0), prepend the JS5
//! header `[ctype | clen (u32 BE) | ulen (u32 BE if compressed)]`, append the 2-byte
//! version trailer, XTEA-encrypt `[5..len-2]` for map loc files, then sector-write into
//! `main_file_cache.dat2` and the per-archive idx files.

use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use io::xtea;

use crate::content::extensions;
use crate::content::manifest::{ArchiveManifest, GroupMeta, MasterManifest};
use crate::content::pack_file;
use crate::data_file::{IDX_ENTRY_SIZE, SECTOR_HEADER, SECTOR_PAYLOAD, SECTOR_SIZE};
use crate::{ARCHIVE_COUNT, ARCHIVE_NAMES, CONFIG_ARCHIVE, MASTER_ARCHIVE};

#[derive(Debug, Default)]
pub struct PackStats {
    pub master_entries: u32,
    pub total_groups: u64,
    pub total_bytes: u64,
}

/// Pack the directory at `src` into a cache at `dest`. `dest` is created if missing;
/// existing `main_file_cache.*` files in it are overwritten.
pub fn pack(src: &Path, dest: &Path) -> std::io::Result<PackStats> {
    fs::create_dir_all(dest)?;
    let dat_path = dest.join("main_file_cache.dat2");
    let mut writer = DatWriter::new(File::create(&dat_path)?);
    let mut stats = PackStats::default();

    // Load every .pack file at start; missing ones return an empty map (file renaming is
    // optional — manifest's `path` is the fallback).
    let pack_dir = src.join("pack");
    let packs = load_all_pack_files(&pack_dir)?;

    for archive in 0..ARCHIVE_COUNT {
        let archive_dir = src.join(ARCHIVE_NAMES[archive as usize]);
        let manifest: ArchiveManifest = read_manifest(&archive_dir.join("_meta.json"))?;
        let mut idx: BTreeMap<u32, (u32, u32)> = BTreeMap::new();
        for group in &manifest.groups {
            let group_bytes = build_group(
                &archive_dir,
                group,
                /* with_trailer = */ true,
                archive,
                &packs,
            )?;
            let first_sector = writer.write_group(archive, group.id, &group_bytes)?;
            idx.insert(group.id, (group_bytes.len() as u32, first_sector));
            stats.total_groups += 1;
            stats.total_bytes += group_bytes.len() as u64;
        }
        write_idx(&dest.join(format!("main_file_cache.idx{archive}")), &idx)?;
    }

    // Master archive (idx255) — entries have no version trailer and no pack-file scope.
    let master_dir = src.join("_master");
    let master_manifest: MasterManifest = read_manifest(&master_dir.join("_meta.json"))?;
    let mut master_idx: BTreeMap<u32, (u32, u32)> = BTreeMap::new();
    for entry in &master_manifest.entries {
        let group_bytes = build_group(
            &master_dir,
            entry,
            /* with_trailer = */ false,
            MASTER_ARCHIVE,
            &packs,
        )?;
        let first_sector = writer.write_group(MASTER_ARCHIVE, entry.id, &group_bytes)?;
        master_idx.insert(entry.id, (group_bytes.len() as u32, first_sector));
        stats.master_entries += 1;
    }
    write_idx(&dest.join("main_file_cache.idx255"), &master_idx)?;

    Ok(stats)
}

/// Build one group's full on-the-wire bytes. `with_trailer` controls whether the 2-byte
/// version trailer is appended after the payload (true for game-archive groups, false for
/// master-archive entries). `packs` carries the loaded .pack files for filename overrides.
fn build_group(
    archive_dir: &Path,
    meta: &GroupMeta,
    with_trailer: bool,
    archive: u8,
    packs: &PackFiles,
) -> std::io::Result<Vec<u8>> {
    let payload = read_group_payload(archive_dir, meta, archive, packs)?;
    let compressed = match meta.ctype {
        0 => payload.clone(),
        1 => io::bzip2::compress(&payload),
        2 => io::gzip::compress(&payload, 6),
        c => panic!("unknown JS5 ctype {c} for group {} in {}", meta.id, meta.path),
    };

    let cap = 9 + compressed.len() + if with_trailer { 2 } else { 0 };
    let mut out = Vec::with_capacity(cap);
    out.push(meta.ctype);
    out.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
    if meta.ctype != 0 {
        out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    }
    out.extend_from_slice(&compressed);
    if with_trailer {
        out.extend_from_slice(&meta.version);
    }

    if let Some(key) = &meta.xtea_key {
        let len = out.len();
        // XTEA range is `[5..len-2]` — see Cache::read_group_with_key for why we stop
        // 2 bytes before the end (the version trailer was appended *after* encryption at
        // pack time, so it stays plain).
        xtea::encrypt(&mut out, key, 5, len - 2);
    }

    Ok(out)
}

/// Read the decompressed payload for a group. Single-file groups: read the .dat (name
/// from pack file if scope exists, else manifest). Multi-file groups: read each per-file
/// .dat in the **manifest's declaration order**, concat, then append the chunk-trailer.
fn read_group_payload(
    archive_dir: &Path,
    meta: &GroupMeta,
    archive: u8,
    packs: &PackFiles,
) -> std::io::Result<Vec<u8>> {
    if let Some(file_ids) = &meta.file_ids {
        // Multi-file group.
        let group_dir = archive_dir.join(group_dir_name(meta, archive, packs));
        let inner_ext = extensions::multi_file_inner_ext(archive, meta.id);
        // Files within: pack file (if config-type scope) overrides the default "{fid}" stem.
        let file_pack = if archive == CONFIG_ARCHIVE {
            pack_file::pack_name_for_config_group(meta.id).and_then(|s| packs.get(s))
        } else {
            None
        };
        let mut files: Vec<Vec<u8>> = Vec::with_capacity(file_ids.len());
        for &fid in file_ids {
            let stem = file_pack
                .and_then(|m| m.get(&(fid as u32)).map(String::as_str))
                .map_or_else(|| fid.to_string(), str::to_string);
            let p = group_dir.join(format!("{stem}.{inner_ext}"));
            files.push(fs::read(&p).map_err(|e| {
                std::io::Error::new(e.kind(), format!("read {p:?}: {e}"))
            })?);
        }

        let (body, trailer) = match &meta.chunks {
            None => single_chunk_layout(&files),
            Some(chunks) => multi_chunk_layout(&files, chunks),
        };
        let mut out = Vec::with_capacity(body.len() + trailer.len());
        out.extend_from_slice(&body);
        out.extend_from_slice(&trailer);
        Ok(out)
    } else {
        // Single-file group.
        let stem = single_file_stem(meta, archive, packs);
        let dat = match stem {
            Some(s) => {
                let ext = std::path::Path::new(&meta.path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("dat");
                archive_dir.join(format!("{s}.{ext}"))
            }
            None => archive_dir.join(&meta.path),
        };
        let bytes = fs::read(&dat)
            .map_err(|e| std::io::Error::new(e.kind(), format!("read {dat:?}: {e}")))?;
        // Reverse the unpack-side codec: MIDI files on disk need re-encoding to Jagex.
        if extensions::is_midi_archive(archive) {
            Ok(io::midi::encode(&bytes))
        } else {
            Ok(bytes)
        }
    }
}

/// Map of pack-file stem (e.g. "model") → its loaded id→name table.
type PackFiles = HashMap<&'static str, BTreeMap<u32, String>>;

fn load_all_pack_files(pack_dir: &Path) -> std::io::Result<PackFiles> {
    let mut out = HashMap::new();
    let scopes = [
        "anim", "base", "interface", "jagfx", "song", "model", "sprite", "texture",
        "binary", "jingle", "script", "font", "vorbis", "patch",
        "flu", "idk", "flo", "inv", "loc", "enum", "npc", "obj", "seq", "spot",
        "varbit", "varp",
    ];
    for scope in scopes {
        let path = pack_dir.join(format!("{scope}.pack"));
        let map = pack_file::read(&path)?;
        if !map.is_empty() {
            out.insert(scope, map);
        }
    }
    Ok(out)
}

/// Resolve the dir name for a multi-file group: pack-file entry if scope exists, else
/// the manifest's declared path.
fn group_dir_name<'a>(meta: &'a GroupMeta, archive: u8, packs: &'a PackFiles) -> &'a str {
    if archive != CONFIG_ARCHIVE
        && let Some(scope) = pack_file::pack_name_for_archive(archive)
        && let Some(name) = packs.get(scope).and_then(|m| m.get(&meta.id))
    {
        return name.as_str();
    }
    meta.path.as_str()
}

/// Resolve the file stem (no .dat) for a single-file group. Returns `None` to mean
/// "use manifest.path verbatim".
fn single_file_stem<'a>(meta: &'a GroupMeta, archive: u8, packs: &'a PackFiles) -> Option<&'a str> {
    if archive != CONFIG_ARCHIVE
        && let Some(scope) = pack_file::pack_name_for_archive(archive)
        && let Some(name) = packs.get(scope).and_then(|m| m.get(&meta.id))
    {
        return Some(name.as_str());
    }
    None
}

/// Chunk_count=1 layout: concat all files, append delta-encoded sizes (one per file),
/// then a 0x01 chunk_count byte.
fn single_chunk_layout(files: &[Vec<u8>]) -> (Vec<u8>, Vec<u8>) {
    let total: usize = files.iter().map(Vec::len).sum();
    let mut body = Vec::with_capacity(total);
    for f in files {
        body.extend_from_slice(f);
    }
    let mut trailer = Vec::with_capacity(4 * files.len() + 1);
    let mut prev: i32 = 0;
    for f in files {
        let delta = (f.len() as i32).wrapping_sub(prev);
        trailer.extend_from_slice(&delta.to_be_bytes());
        prev = f.len() as i32;
    }
    trailer.push(1);
    (body, trailer)
}

/// chunk_count > 1 layout: body is chunk-interleaved
/// (`[c0_f0 | c0_f1 | ... | c0_fN] [c1_f0 | ...] ...`), trailer is per-chunk delta
/// tables (matching the encoding `unpack_group` decodes), then the chunk_count byte.
fn multi_chunk_layout(files: &[Vec<u8>], chunks: &[Vec<u32>]) -> (Vec<u8>, Vec<u8>) {
    let total: usize = chunks.iter().flat_map(|c| c.iter()).map(|&n| n as usize).sum();
    let mut body = Vec::with_capacity(total);
    let mut file_offsets = vec![0usize; files.len()];
    for chunk in chunks {
        for (i, &n) in chunk.iter().enumerate() {
            let n = n as usize;
            body.extend_from_slice(&files[i][file_offsets[i]..file_offsets[i] + n]);
            file_offsets[i] += n;
        }
    }
    let mut trailer = Vec::with_capacity(4 * files.len() * chunks.len() + 1);
    for chunk in chunks {
        let mut prev: i32 = 0;
        for &n in chunk {
            let delta = (n as i32).wrapping_sub(prev);
            trailer.extend_from_slice(&delta.to_be_bytes());
            prev = n as i32;
        }
    }
    trailer.push(chunks.len() as u8);
    (body, trailer)
}

fn read_manifest<T: serde::de::DeserializeOwned>(path: &Path) -> std::io::Result<T> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

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

    fn write_group(&mut self, archive: u8, group_id: u32, bytes: &[u8]) -> std::io::Result<u32> {
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
