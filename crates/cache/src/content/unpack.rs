//! Cache → Content-shaped directory tree.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

use io::{Packet, cp1252, xtea};

use crate::content::manifest::{ArchiveManifest, GroupMeta, MasterManifest};
use crate::content::pack_file;
use crate::maps::XteaKeys;
use crate::{ARCHIVE_COUNT, ARCHIVE_NAMES, Cache, MASTER_ARCHIVE, MAPS_ARCHIVE, decode_packet};
use crate::config::group as config_group;

#[derive(Debug, Default)]
pub struct UnpackStats {
    pub master_entries: u32,
    pub total_groups: u64,
    pub total_files: u64,
    pub total_payload_bytes: u64,
}

/// Unpack `cache` into a Content-shaped tree at `dest`. `keys` provides XTEA keys for
/// encrypted map loc files; pass `XteaKeys::default()` if you don't need to decrypt them
/// (the encrypted bytes will be written verbatim).
pub fn unpack(cache: &mut Cache, keys: &XteaKeys, dest: &Path) -> std::io::Result<UnpackStats> {
    fs::create_dir_all(dest)?;
    let mut stats = UnpackStats::default();
    let map_names = build_map_name_table();
    // Accumulate one BTreeMap per pack-file scope; written at end of unpack.
    let mut pack_data: HashMap<&'static str, BTreeMap<u32, String>> = HashMap::new();

    for archive in 0..ARCHIVE_COUNT {
        let archive_dir = dest.join(ARCHIVE_NAMES[archive as usize]);
        fs::create_dir_all(&archive_dir)?;
        let group_ids: Vec<i32> = cache.index(archive).group_ids.clone();
        let mut manifest = ArchiveManifest {
            archive_id: archive,
            archive_name: ARCHIVE_NAMES[archive as usize].to_string(),
            groups: Vec::with_capacity(group_ids.len()),
        };

        for gid in group_ids {
            let gid = gid as u32;
            let raw = cache
                .read_raw(archive, gid)?
                .unwrap_or_else(|| panic!("archive {archive} group {gid} missing"));
            let meta = unpack_one_group(
                cache,
                archive,
                gid,
                &raw,
                keys,
                &map_names,
                &archive_dir,
                &mut pack_data,
            )?;
            stats.total_groups += 1;
            manifest.groups.push(meta);
        }
        manifest.groups.sort_by_key(|g| g.id);
        write_manifest(&archive_dir.join("_meta.json"), &manifest)?;
    }

    // Write accumulated .pack files.
    let pack_dir = dest.join("pack");
    fs::create_dir_all(&pack_dir)?;
    for (scope, map) in &pack_data {
        pack_file::write(&pack_dir.join(format!("{scope}.pack")), map)?;
    }

    // Master archive (idx255) — entries have NO 2-byte version trailer (the per-archive
    // version is transported separately during JS5 sync, not in the archive bytes).
    let master_dir = dest.join("_master");
    fs::create_dir_all(&master_dir)?;
    let mut master_manifest = MasterManifest { entries: Vec::with_capacity(ARCHIVE_COUNT as usize) };
    for archive in 0..ARCHIVE_COUNT {
        let raw = cache
            .read_master_raw(archive)?
            .unwrap_or_else(|| panic!("master missing entry for archive {archive}"));
        let (decompressed, ctype) = split_group(&raw, /* has_trailer = */ false);
        let path = format!("{archive}.dat");
        fs::write(master_dir.join(&path), &decompressed)?;
        master_manifest.entries.push(GroupMeta {
            id: u32::from(archive),
            ctype,
            version: [0, 0],
            path,
            xtea_key: None,
            file_ids: None,
            chunks: None,
        });
        stats.master_entries += 1;
        stats.total_payload_bytes += decompressed.len() as u64;
    }
    write_manifest(&master_dir.join("_meta.json"), &master_manifest)?;

    Ok(stats)
}

fn unpack_one_group(
    cache: &mut Cache,
    archive: u8,
    group_id: u32,
    raw: &[u8],
    keys: &XteaKeys,
    map_names: &HashMap<i32, String>,
    archive_dir: &Path,
    pack_data: &mut HashMap<&'static str, BTreeMap<u32, String>>,
) -> std::io::Result<GroupMeta> {
    // For map loc files (archive 5, l*_* name hash), decrypt before decompressing.
    let mut owned: Vec<u8>;
    let bytes: &[u8];
    let mut xtea_key: Option<[i32; 4]> = None;

    if archive == MAPS_ARCHIVE {
        let name_hash = cache.index(archive).group_name_hashes.as_ref().unwrap()[group_id as usize];
        if let Some(name) = map_names.get(&name_hash)
            && name.starts_with('l')
        {
            // Parse "lX_Y" → mapsquare → key
            let (x, y) = parse_map_xy(name);
            if let Some(key) = keys.get(x, y) {
                owned = raw.to_vec();
                let len = owned.len();
                xtea::decrypt(&mut owned, key, 5, len - 2);
                xtea_key = Some(*key);
                bytes = &owned;
            } else {
                bytes = raw;
            }
        } else {
            bytes = raw;
        }
    } else {
        bytes = raw;
    }

    let (payload, ctype) = split_group(bytes, /* has_trailer = */ true);
    let version = [bytes[bytes.len() - 2], bytes[bytes.len() - 1]];

    // Resolve on-disk name for this group.
    let group_name = group_path(archive, group_id, cache.index(archive).group_name_hashes.as_deref(), map_names);
    let file_ids = cache.index(archive).file_ids[group_id as usize].clone();

    let (path, stored_file_ids, chunks) = if file_ids.len() > 1 {
        let group_dir = archive_dir.join(&group_name);
        fs::create_dir_all(&group_dir)?;
        let files = crate::unpack_group(&payload, file_ids.len());
        for (i, file_bytes) in files.iter().enumerate() {
            let fid = file_ids[i];
            fs::write(group_dir.join(format!("{fid}.dat")), file_bytes)?;
        }
        let chunks = extract_chunks(&payload, file_ids.len());

        // Config-archive groups (npc, obj, loc, …) put per-file entries into their
        // type-specific .pack — default file stem is just the file id.
        if archive == crate::CONFIG_ARCHIVE
            && let Some(scope) = pack_file::pack_name_for_config_group(group_id)
        {
            let entry = pack_data.entry(scope).or_default();
            for &fid in &file_ids {
                entry.insert(fid as u32, fid.to_string());
            }
        }

        // Non-config multi-file archives (anim_*, interface_*) get per-group entries
        // mapping group_id → dir stem.
        if archive != crate::CONFIG_ARCHIVE
            && let Some(scope) = pack_file::pack_name_for_archive(archive)
        {
            pack_data.entry(scope).or_default().insert(group_id, group_name.clone());
        }

        (group_name, Some(file_ids), chunks)
    } else {
        let path = format!("{group_name}.dat");
        fs::write(archive_dir.join(&path), &payload)?;

        // Single-file group: pack entry maps group_id → file stem (no .dat).
        if archive != crate::CONFIG_ARCHIVE
            && let Some(scope) = pack_file::pack_name_for_archive(archive)
        {
            pack_data.entry(scope).or_default().insert(group_id, group_name.clone());
        }

        (path, None, None)
    };

    Ok(GroupMeta { id: group_id, ctype, version, path, xtea_key, file_ids: stored_file_ids, chunks })
}

/// Parse the multi-file group trailer to recover per-chunk per-file byte sizes. Returns
/// `None` when `chunk_count == 1` (the trivial case — each file's chunk size equals its
/// total length, derivable on pack from file sizes alone).
fn extract_chunks(bytes: &[u8], file_count: usize) -> Option<Vec<Vec<u32>>> {
    if file_count <= 1 {
        return None;
    }
    let total = bytes.len();
    let chunk_count = bytes[total - 1] as usize;
    if chunk_count == 1 {
        return None;
    }
    let table_start = total - 1 - file_count * chunk_count * 4;
    let mut p = Packet::from_vec(bytes.to_vec());
    p.pos = table_start;
    let mut chunks: Vec<Vec<u32>> = Vec::with_capacity(chunk_count);
    for _ in 0..chunk_count {
        let mut chunk = vec![0u32; file_count];
        let mut running: i32 = 0;
        for slot in chunk.iter_mut() {
            running = running.wrapping_add(p.g4());
            *slot = running as u32;
        }
        chunks.push(chunk);
    }
    Some(chunks)
}

/// Split a raw group blob into `(decompressed_payload, ctype)`. `has_trailer` is true for
/// game-archive groups (which carry a 2-byte version trailer) and false for master-archive
/// entries (which don't — the per-archive version travels separately during JS5 sync).
/// `bytes` may have already been XTEA-decrypted for maps loc groups.
fn split_group(bytes: &[u8], has_trailer: bool) -> (Vec<u8>, u8) {
    let ctype = bytes[0];
    let end = if has_trailer { bytes.len() - 2 } else { bytes.len() };
    let payload = match ctype {
        0 => {
            let clen = u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize;
            bytes[5..5 + clen].to_vec()
        }
        1 | 2 => decode_packet(&bytes[..end]),
        _ => panic!("unknown JS5 ctype {ctype}"),
    };
    (payload, ctype)
}

fn write_manifest<T: serde::Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    fs::write(path, json)
}

/// Build a hash → "mX_Y" / "lX_Y" lookup table by brute force (256² × 2 = 131,072 entries).
fn build_map_name_table() -> HashMap<i32, String> {
    let mut table = HashMap::with_capacity(131_072);
    for x in 0..256u32 {
        for y in 0..256u32 {
            let m = format!("m{x}_{y}");
            let l = format!("l{x}_{y}");
            table.insert(cp1252::name_hash(&m), m);
            table.insert(cp1252::name_hash(&l), l);
        }
    }
    table
}

fn parse_map_xy(name: &str) -> (u32, u32) {
    // "l40_55" → (40, 55)
    let rest = &name[1..];
    let mut parts = rest.split('_');
    let x: u32 = parts.next().unwrap().parse().unwrap();
    let y: u32 = parts.next().unwrap().parse().unwrap();
    (x, y)
}

/// Compute the on-disk path stem (no extension) for a group.
fn group_path(
    archive: u8,
    group_id: u32,
    name_hashes: Option<&[i32]>,
    map_names: &HashMap<i32, String>,
) -> String {
    match archive {
        0 => format!("anim_{group_id}"),
        1 => format!("base_{group_id}"),
        2 => match group_id {
            config_group::FLU => "flu".to_string(),
            config_group::IDK => "idk".to_string(),
            config_group::FLO => "flo".to_string(),
            config_group::INV => "inv".to_string(),
            config_group::LOC => "loc".to_string(),
            config_group::ENUM => "enum".to_string(),
            config_group::NPC => "npc".to_string(),
            config_group::OBJ => "obj".to_string(),
            config_group::SEQ => "seq".to_string(),
            config_group::SPOT => "spot".to_string(),
            config_group::VARBIT => "varbit".to_string(),
            config_group::VARP => "varp".to_string(),
            _ => format!("group_{group_id}"),
        },
        3 => format!("if_{group_id}"),
        4 => format!("jagfx_{group_id}"),
        MAPS_ARCHIVE => {
            let hash = name_hashes
                .expect("maps archive must have name hashes")
                .get(group_id as usize)
                .copied()
                .unwrap_or(0);
            map_names
                .get(&hash)
                .cloned()
                .unwrap_or_else(|| format!("group_{group_id}"))
        }
        6 => format!("song_{group_id}"),
        7 => format!("model_{group_id}"),
        8 => format!("sprite_{group_id}"),
        9 => format!("texture_{group_id}"),
        10 => format!("binary_{group_id}"),
        11 => format!("jingle_{group_id}"),
        12 => format!("script_{group_id}"),
        13 => format!("font_{group_id}"),
        14 => format!("vorbis_{group_id}"),
        15 => format!("patch_{group_id}"),
        _ => format!("group_{group_id}"),
    }
}

// Re-export to silence unused-import warning (MASTER_ARCHIVE used in tests only).
const _: u8 = MASTER_ARCHIVE;
