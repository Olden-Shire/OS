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
use crate::cs2_asm::{self, NameMaps};
use crate::cs2_compile;
use crate::cs2_sig::ScriptSig;
use crate::cs2_source;
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

    // Name tables for symbolic clientscript operands — the exact inverse of the maps the
    // unpack side rendered with, so `gosub login`/`push_varp world_var` resolve back to ids.
    let mut names = NameMaps::new();
    if let Some(m) = packs.get("script") {
        names.set_scripts(m);
    }
    if let Some(m) = packs.get("varp") {
        names.set_varps(m);
    }
    if let Some(m) = packs.get("varbit") {
        names.set_varbits(m);
    }

    // Cross-script signature table for structured `.cs2` sources — every header is
    // scanned before any body compiles, so gosub callees resolve regardless of order.
    let cs2_sigs = scan_cs2_signatures(src, &names)?;

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
                &names,
                &cs2_sigs,
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
            &names,
            &cs2_sigs,
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
#[allow(clippy::too_many_arguments)]
fn build_group(
    archive_dir: &Path,
    meta: &GroupMeta,
    with_trailer: bool,
    archive: u8,
    packs: &PackFiles,
    names: &NameMaps,
    cs2_sigs: &BTreeMap<u32, ScriptSig>,
) -> std::io::Result<Vec<u8>> {
    let payload = read_group_payload(archive_dir, meta, archive, packs, names, cs2_sigs)?;
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
    names: &NameMaps,
    cs2_sigs: &BTreeMap<u32, ScriptSig>,
) -> std::io::Result<Vec<u8>> {
    // Sharded archives (models/anims/bases) store files in a 1000-id
    // bucket subdir; group_base resolves to the archive dir otherwise.
    let base_dir = extensions::group_base(archive_dir, archive, meta.id);
    if let Some(file_ids) = &meta.file_ids {
        // All-empty placeholder group (unpack wrote no files): regenerate
        // a lone 0x00 record per file and reassemble via the stored chunk
        // layout — byte-identical to the original, zero disk reads.
        if meta.placeholder {
            let files: Vec<Vec<u8>> = file_ids.iter().map(|_| vec![0u8]).collect();
            let (body, trailer) = match &meta.chunks {
                None => single_chunk_layout(&files),
                Some(chunks) => multi_chunk_layout(&files, chunks),
            };
            let mut out = Vec::with_capacity(body.len() + trailer.len());
            out.extend_from_slice(&body);
            out.extend_from_slice(&trailer);
            return Ok(out);
        }
        // Interface group stored as one `.if` text: re-encode every
        // component and reassemble in file-id order via the chunk layout.
        if archive == crate::INTERFACES_ARCHIVE
            && std::path::Path::new(&meta.path).extension().is_some_and(|e| e.eq_ignore_ascii_case("if"))
        {
            let p = base_dir.join(&meta.path);
            let text = fs::read_to_string(&p)
                .map_err(|e| std::io::Error::new(e.kind(), format!("read {p:?}: {e}")))?;
            let parsed = crate::content::interface_text::encode_group(meta.id, &text).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{p:?}: .if re-encode failed"))
            })?;
            let map: std::collections::HashMap<i32, Vec<u8>> = parsed.into_iter().collect();
            let mut files: Vec<Vec<u8>> = file_ids.iter()
                .map(|f| map.get(f).cloned())
                .collect::<Option<_>>()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData,
                    format!("{p:?}: .if missing a component")))?;
            // A single-component group has NO multi-file chunk trailer —
            // its payload is the component bytes verbatim.
            if files.len() == 1 {
                return Ok(files.pop().unwrap());
            }
            let (body, trailer) = match &meta.chunks {
                None => single_chunk_layout(&files),
                Some(chunks) => multi_chunk_layout(&files, chunks),
            };
            let mut out = Vec::with_capacity(body.len() + trailer.len());
            out.extend_from_slice(&body);
            out.extend_from_slice(&trailer);
            return Ok(out);
        }
        // Multi-file group.
        let group_dir = base_dir.join(group_dir_name(meta, archive, packs));
        let inner_ext = extensions::multi_file_inner_ext(archive, meta.id);
        // Files within: pack file (if config-type scope) overrides the default "{fid}" stem.
        let file_pack = if archive == CONFIG_ARCHIVE {
            pack_file::pack_name_for_config_group(meta.id).and_then(|s| packs.get(s))
        } else {
            None
        };
        // Config-archive groups may store records as readable text
        // (`.obj`, `.loc`, …) — re-encode those to the exact bytes; a
        // `.dat` sibling is the verbatim fallback (and all other archives).
        let config_codec = if archive == CONFIG_ARCHIVE {
            crate::content::config_text::schema_for_group(meta.id)
        } else {
            None
        };
        let nfiles = file_ids.len();
        let mut files: Vec<Vec<u8>> = Vec::with_capacity(nfiles);
        for &fid in file_ids {
            let stem = file_pack
                .and_then(|m| m.get(&(fid as u32)).map(String::as_str))
                .map_or_else(|| fid.to_string(), str::to_string);
            // Mirror unpack's intra-group id-bucket sharding for huge types.
            let fdir = match extensions::intra_group_shard(nfiles, fid) {
                Some(b) => group_dir.join(b),
                None => group_dir.clone(),
            };
            if let Some((schema, kind)) = config_codec {
                let text_path = fdir.join(format!("{stem}.{kind}"));
                if text_path.exists() {
                    let text = fs::read_to_string(&text_path).map_err(|e| {
                        std::io::Error::new(e.kind(), format!("read {text_path:?}: {e}"))
                    })?;
                    let bytes = crate::content::config_text::encode(schema, &text).ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("{text_path:?}: config text re-encode failed"),
                        )
                    })?;
                    files.push(bytes);
                    continue;
                }
            }
            let p = fdir.join(format!("{stem}.{inner_ext}"));
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
        // Maps land/loc pair sharing one region text: meta.path is
        // "m{r}.jm2" or "l{r}.jm2", the physical file is "{r}.jm2"
        // (content::maps_jm2). Re-encode the half this group owns; the
        // caller then compresses + appends the trailer + XTEA-encrypts
        // exactly as for a .dat payload.
        let meta_ext = std::path::Path::new(&meta.path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if meta_ext.eq_ignore_ascii_case("jm2") {
            let stem = std::path::Path::new(&meta.path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let (half, region) = stem.split_at(1.min(stem.len()));
            let file = archive_dir.join(format!("{region}.jm2"));
            let text = fs::read_to_string(&file)
                .map_err(|e| std::io::Error::new(e.kind(), format!("read {file:?}: {e}")))?;
            let raw = crate::maps::text::RawRegion::from_text(&text).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{file:?}: {e}"))
            })?;
            return Ok(match half {
                "m" => raw.encode_land(),
                "l" => raw.encode_locs(),
                _ => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("jm2 group path {:?} must start with m or l", meta.path),
                    ));
                }
            });
        }

        // Single-file empty config stub — regenerate the lone 0x00 record.
        if meta.placeholder {
            return Ok(vec![0u8]);
        }
        // Single-file group.
        let stem = single_file_stem(meta, archive, packs);
        let dat = match stem {
            Some(s) => {
                let ext = std::path::Path::new(&meta.path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("dat");
                base_dir.join(format!("{s}.{ext}"))
            }
            None => base_dir.join(&meta.path),
        };
        // Vorbis sample groups live as standard .ogg on disk
        // (crate::vorbis_ogg) — rebuild the Jagex container from the ogg
        // packets + comment fields. Group 0 (shared setup header) stays
        // .dat and falls through as raw bytes.
        if archive == crate::VORBIS_ARCHIVE
            && dat.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("ogg"))
        {
            let bytes = fs::read(&dat)
                .map_err(|e| std::io::Error::new(e.kind(), format!("read {dat:?}: {e}")))?;
            let sample = crate::vorbis_ogg::from_ogg(&bytes).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{dat:?}: {e}"))
            })?;
            return Ok(sample.encode());
        }
        let bytes = fs::read(&dat)
            .map_err(|e| std::io::Error::new(e.kind(), format!("read {dat:?}: {e}")))?;
        // Reverse the unpack-side codec. A `.cs2` file is structured clientscript
        // source (parse → compile → encode); a `.cs2asm` is the faithful assembly
        // fallback (assemble → encode); the decode-failure fallback stays `.dat` and
        // falls through as raw bytes. MIDI files need re-encoding to Jagex.
        let ext = std::path::Path::new(&meta.path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if ext.eq_ignore_ascii_case("cs2") {
            let text = String::from_utf8(bytes).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{dat:?}: not UTF-8: {e}"))
            })?;
            let ir = cs2_source::parse(&text, names, cs2_sigs).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{dat:?}: {e}"))
            })?;
            let script = cs2_compile::compile(&ir).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{dat:?}: {e}"))
            })?;
            Ok(script.encode())
        } else if ext.eq_ignore_ascii_case("cs2asm") {
            let text = String::from_utf8(bytes).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{dat:?}: not UTF-8: {e}"))
            })?;
            let script = cs2_asm::assemble(&text, names).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{dat:?}: {e}"))
            })?;
            Ok(script.encode())
        } else if extensions::is_midi_archive(archive) {
            Ok(io::midi::encode(&bytes))
        } else {
            Ok(bytes)
        }
    }
}

/// Scan every structured `.cs2` source in the clientscript archives and collect the
/// id → signature table needed to compile gosub call sites. `.cs2asm` fallbacks don't
/// contribute (their callers, if any, would themselves be `.cs2asm` — the unpack side
/// only emits structured source when the whole verification passed against this same
/// table). Public so external tools (the IntelliJ plugin's preview CLI) can compile a
/// single source file against the same table.
pub fn scan_cs2_signatures(src: &Path, names: &NameMaps) -> std::io::Result<BTreeMap<u32, ScriptSig>> {
    let mut sigs = BTreeMap::new();
    for archive in 0..ARCHIVE_COUNT {
        if !extensions::is_clientscript_archive(archive) {
            continue;
        }
        let archive_dir = src.join(ARCHIVE_NAMES[archive as usize]);
        if !archive_dir.exists() {
            continue;
        }
        for entry in fs::read_dir(&archive_dir)? {
            let path = entry?.path();
            if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("cs2")) {
                continue;
            }
            let text = fs::read_to_string(&path)?;
            let (id, sig) = cs2_source::parse_signature(&text, names).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{path:?}: {e}"))
            })?;
            sigs.insert(id, sig);
        }
    }
    Ok(sigs)
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
