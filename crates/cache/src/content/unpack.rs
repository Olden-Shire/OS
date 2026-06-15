//! Cache → Content-shaped directory tree.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

use io::{Packet, cp1252, xtea};

use crate::content::extensions;
use crate::content::hash_names::ArchiveNameMap;
use crate::content::manifest::{ArchiveManifest, GroupMeta, MasterManifest};
use crate::content::pack_file;
use crate::cs2::ClientScript;
use crate::cs2_asm::{self, NameMaps};
use crate::cs2_compile;
use crate::cs2_decompile;
use crate::cs2_sig::{self, ScriptSig};
use crate::cs2_source;
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
    unpack_with_names(cache, keys, dest, &ArchiveNameMap::new())
}

/// Like [`unpack`] but consults `name_map` for the on-disk stem of every group whose
/// `name_hash` has a match. Build the map from a reference pack directory via
/// [`crate::content::hash_names::load_from_pack_dir`].
pub fn unpack_with_names(
    cache: &mut Cache,
    keys: &XteaKeys,
    dest: &Path,
    name_map: &ArchiveNameMap,
) -> std::io::Result<UnpackStats> {
    fs::create_dir_all(dest)?;
    let mut stats = UnpackStats::default();
    let map_names = build_map_name_table();
    // Accumulate one BTreeMap per pack-file scope; written at end of unpack.
    let mut pack_data: HashMap<&'static str, BTreeMap<u32, String>> = HashMap::new();

    for archive in 0..ARCHIVE_COUNT {
        let archive_dir = dest.join(ARCHIVE_NAMES[archive as usize]);
        // Wipe stale files from prior unpacks (a re-run with a new name source will write
        // `scape_main.mid` but the previous run's `song_42.mid` would otherwise linger).
        if archive_dir.exists() {
            fs::remove_dir_all(&archive_dir)?;
        }
        fs::create_dir_all(&archive_dir)?;
        let group_ids: Vec<i32> = cache.index(archive).group_ids.clone();
        let mut manifest = ArchiveManifest {
            archive_id: archive,
            archive_name: ARCHIVE_NAMES[archive as usize].to_string(),
            groups: Vec::with_capacity(group_ids.len()),
        };

        let archive_name_lookup = name_map.get(&archive);
        // Clientscripts decompile to `.cs2` structured source; build the id→name
        // tables (own scripts + varp/varbit from the already-unpacked config archive)
        // and the cross-script signature table (gosub arity + returns) up front so
        // every reference resolves regardless of group order.
        let (names, cs2_sigs) = if extensions::is_clientscript_archive(archive) {
            (
                build_clientscript_names(archive, &group_ids, cache, &map_names, archive_name_lookup, &pack_data),
                build_clientscript_sigs(archive, &group_ids, cache),
            )
        } else {
            (NameMaps::new(), BTreeMap::new())
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
                archive_name_lookup,
                &archive_dir,
                &mut pack_data,
                &names,
                &cs2_sigs,
            )?;
            stats.total_groups += 1;
            manifest.groups.push(meta);
        }
        manifest.groups.sort_by_key(|g| g.id);
        write_manifest(&archive_dir.join("_meta.json"), &manifest)?;

        // Fold m/l map pairs into one editable .jm2 text per region (the
        // pass verifies each pair round-trips byte-exact before touching
        // it, and rewrites _meta.json itself).
        if archive == crate::MAPS_ARCHIVE {
            let s = crate::content::maps_jm2::convert_maps_dir(&archive_dir)?;
            crate::dbg_log!(
                "[unpack] maps → jm2: {} regions converted, {} kept as .dat",
                s.converted, s.kept_dat
            );
        }

        // Wrap vorbis samples as standard .ogg (group 0 — the shared
        // setup header — stays .dat). Same verify-then-convert contract.
        if archive == crate::VORBIS_ARCHIVE {
            let s = crate::vorbis_ogg::convert_vorbis_dir(&archive_dir)?;
            crate::dbg_log!(
                "[unpack] vorbis → ogg: {} samples converted, {} kept as .dat",
                s.converted, s.kept_dat
            );
        }
    }

    // Write accumulated .pack files.
    let pack_dir = dest.join("pack");
    fs::create_dir_all(&pack_dir)?;
    for (scope, map) in &pack_data {
        pack_file::write(&pack_dir.join(format!("{scope}.pack")), map)?;
    }

    // interface.order + interface.pack — the .rs2/.cs2 toolchain symbol
    // table for interfaces. The compiler resolves `if_549` →
    // `symbols.config("interface", name).id`, and the engine opens the
    // REAL interface id (549) — so the pack key MUST be the real id, not a
    // remapped flat-sequential one (Lost City uses flat ids because its
    // engine renumbers; ours doesn't). Roots key on the interface id;
    // components on the packed `(id << 16) | sub` the engine expects.
    // `interface.order` lists the interface ids (ascending). AUXILIARY —
    // not read back by pack (the cache rebuilds from .if + _meta.json).
    {
        let if_names = pack_data.get("interface");
        let mut group_ids: Vec<i32> = cache.index(crate::INTERFACES_ARCHIVE).group_ids.clone();
        group_ids.sort_unstable();
        let mut order = String::new();
        let mut pack = String::new();
        for gid in group_ids {
            let g = gid as u32;
            // Symbol name for .rs2/.cs2: a real rename if present, else
            // `if_{id}` — identifier-safe (a bare number isn't a valid
            // RuneScript ident, same reason clientscripts keep `script_`).
            // The on-disk FILENAME stays bare `{id}.if` (ext says the type).
            let name = match if_names.and_then(|m| m.get(&g).cloned()) {
                Some(n) if n != format!("{g}") => n,
                _ => format!("if_{g}"),
            };
            order.push_str(&format!("{gid}\n"));
            pack.push_str(&format!("{g}={name}\n"));
            let mut subs = cache.index(crate::INTERFACES_ARCHIVE).file_ids[g as usize].clone();
            subs.sort_unstable();
            for sub in subs {
                let packed = (g << 16) | (sub as u32);
                pack.push_str(&format!("{packed}={name}:com_{sub}\n"));
            }
        }
        fs::write(pack_dir.join("interface.order"), order)?;
        fs::write(pack_dir.join("interface.pack"), pack)?;
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
            placeholder: false,
        });
        stats.master_entries += 1;
        stats.total_payload_bytes += decompressed.len() as u64;
    }
    write_manifest(&master_dir.join("_meta.json"), &master_manifest)?;

    Ok(stats)
}

#[allow(clippy::too_many_arguments)]
fn unpack_one_group(
    cache: &mut Cache,
    archive: u8,
    group_id: u32,
    raw: &[u8],
    keys: &XteaKeys,
    map_names: &HashMap<i32, String>,
    name_lookup: Option<&HashMap<i32, String>>,
    archive_dir: &Path,
    pack_data: &mut HashMap<&'static str, BTreeMap<u32, String>>,
    names: &NameMaps,
    cs2_sigs: &BTreeMap<u32, ScriptSig>,
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
    let group_name = group_path(
        archive,
        group_id,
        cache.index(archive).group_name_hashes.as_deref(),
        map_names,
        name_lookup,
    );
    let file_ids = cache.index(archive).file_ids[group_id as usize].clone();

    // Sharded archives (models/anims/bases) write into a 1000-id bucket
    // subdir; everything else writes directly under the archive dir.
    let base_dir = extensions::group_base(archive_dir, archive, group_id);

    let mut placeholder = false;
    // Interfaces (archive 3): collapse the whole group's components into
    // ONE readable `.if` file (Content-old style) when every component
    // re-encodes byte-exact; otherwise fall through to the per-file
    // `if_*/N.dat` layout. The `.if` extension on `path` tells pack to
    // re-encode via interface_text.
    if archive == crate::INTERFACES_ARCHIVE {
        let files = crate::unpack_group(&payload, file_ids.len());
        let chunks = extract_chunks(&payload, file_ids.len());
        let comps: Vec<(i32, Vec<u8>)> =
            file_ids.iter().copied().zip(files.iter().cloned()).collect();
        if let Some(text) = crate::content::interface_text::decode_group(group_id, &comps) {
            let path = format!("{group_name}.if");
            fs::write(archive_dir.join(&path), text)?;
            if let Some(scope) = pack_file::pack_name_for_archive(archive) {
                pack_data.entry(scope).or_default().insert(group_id, group_name.clone());
            }
            return Ok(GroupMeta {
                id: group_id, ctype, version, path, xtea_key,
                file_ids: Some(file_ids), chunks, placeholder: false,
            });
        }
        // else: fall through to the generic per-file layout below.
    }

    let (path, stored_file_ids, chunks) = if file_ids.len() > 1 {
        let inner_ext = extensions::multi_file_inner_ext(archive, group_id);
        let files = crate::unpack_group(&payload, file_ids.len());
        let chunks = extract_chunks(&payload, file_ids.len());

        // All-empty-stub groups (rev1-unused config types stripped to a
        // lone 0x00 terminator each) write NO files — pack regenerates
        // them from file_ids + chunks. Saves ~1000 useless 1-byte files.
        if archive == crate::CONFIG_ARCHIVE && files.iter().all(|f| f.as_slice() == [0u8]) {
            placeholder = true;
            (group_name, Some(file_ids), chunks)
        } else {
            let group_dir = base_dir.join(&group_name);
            fs::create_dir_all(&group_dir)?;
            // Config-archive groups with a text codec write readable `.obj`,
            // `.loc`, … per record (verified byte-exact); records the codec
            // can't reproduce stay `.dat`. Pack probes both extensions.
            let config_codec = if archive == crate::CONFIG_ARCHIVE {
                crate::content::config_text::schema_for_group(group_id)
            } else {
                None
            };
            let nfiles = files.len();
            for (i, file_bytes) in files.iter().enumerate() {
                let fid = file_ids[i];
                // Huge config types shard their files into id buckets.
                let dir = match extensions::intra_group_shard(nfiles, fid) {
                    Some(b) => {
                        let d = group_dir.join(b);
                        fs::create_dir_all(&d)?;
                        d
                    }
                    None => group_dir.clone(),
                };
                let wrote_text = if let Some((schema, kind)) = config_codec {
                    // Model names aren't known yet mid-unpack (model.pack is written
                    // at the end), so configs unpack with raw model ids; the
                    // post-unpack rename tools rewrite them to names.
                    let model_refs = crate::content::config_text::ConfigRefs::default();
                    crate::content::config_text::decode(schema, kind, fid as u32, file_bytes, &model_refs)
                        .map(|text| fs::write(dir.join(format!("{fid}.{kind}")), text))
                        .transpose()?
                        .is_some()
                } else {
                    false
                };
                if !wrote_text {
                    fs::write(dir.join(format!("{fid}.{inner_ext}")), file_bytes)?;
                }
            }

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
        }
    } else if archive == crate::CONFIG_ARCHIVE && payload.as_slice() == [0u8] {
        // Single-file empty config stub (e.g. group 25): write nothing,
        // flag placeholder; pack regenerates the lone 0x00 record.
        placeholder = true;
        (group_name, None, None)
    } else {
        // Clientscripts decompile to structured `.cs2` source when the full
        // decompile → recompile → byte-compare verification passes; otherwise to
        // faithful `.cs2asm` assembly. A script that fails to decode (never for
        // vanilla) falls back to raw `.dat` so nothing is lost. Other archives keep
        // their MIDI/raw handling.
        let (ext, on_disk): (&str, Vec<u8>) = if extensions::is_clientscript_archive(archive) {
            match ClientScript::decode(&payload) {
                Some(script) => match try_decompile_source(group_id, &script, names, cs2_sigs, &payload) {
                    Some(text) => ("cs2", text.into_bytes()),
                    None => ("cs2asm", cs2_asm::disassemble(&script, names).into_bytes()),
                },
                None => ("dat", payload.clone()),
            }
        } else if extensions::is_midi_archive(archive) {
            (extensions::single_file_ext(archive, &payload), io::midi::decode(&payload))
        } else {
            (extensions::single_file_ext(archive, &payload), payload.clone())
        };
        let path = format!("{group_name}.{ext}");
        fs::create_dir_all(&base_dir)?;
        fs::write(base_dir.join(&path), &on_disk)?;

        // Single-file group: pack entry maps group_id → file stem (no .dat).
        if archive != crate::CONFIG_ARCHIVE
            && let Some(scope) = pack_file::pack_name_for_archive(archive)
        {
            pack_data.entry(scope).or_default().insert(group_id, group_name.clone());
        }

        (path, None, None)
    };

    Ok(GroupMeta { id: group_id, ctype, version, path, xtea_key, file_ids: stored_file_ids, chunks, placeholder })
}

/// Decode every script of the clientscript archive and infer the cross-script
/// signature table (gosub arg/return counts) the structured decompiler needs.
/// Scripts that fail analysis simply stay out of the table — anything that calls them
/// then falls back to `.cs2asm` via [`try_decompile_source`].
fn build_clientscript_sigs(
    archive: u8,
    group_ids: &[i32],
    cache: &mut Cache,
) -> BTreeMap<u32, ScriptSig> {
    let mut scripts: BTreeMap<u32, ClientScript> = BTreeMap::new();
    for &gid in group_ids {
        let gid = gid as u32;
        if let Ok(Some(bytes)) = cache.read_group(archive, gid)
            && let Some(script) = ClientScript::decode(&bytes)
        {
            scripts.insert(gid, script);
        }
    }
    cs2_sig::analyze_all(&scripts).sigs
}

/// Structured decompile with full verification: the lifted IR must recompile to the
/// exact original payload, and the printed source must reparse to the identical IR.
/// Any failure returns `None` and the caller writes `.cs2asm` instead — the cache
/// round-trips either way.
fn try_decompile_source(
    id: u32,
    script: &ClientScript,
    names: &NameMaps,
    sigs: &BTreeMap<u32, ScriptSig>,
    payload: &[u8],
) -> Option<String> {
    let ir = cs2_decompile::lift(id, script, sigs).ok()?;
    let back = cs2_compile::compile(&ir).ok()?;
    if back.encode() != payload {
        return None;
    }
    let text = cs2_source::print(&ir, names);
    match cs2_source::parse(&text, names, sigs) {
        Ok(reparsed) if reparsed == ir => Some(text),
        _ => None,
    }
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

/// Build the [`NameMaps`] used to render clientscript operands symbolically. Script names
/// are the on-disk stems of every archive-12 group (same resolution as the file names, so
/// pack reads them back via `script.pack`); varp/varbit names come from the config
/// archive's already-populated `.pack` data.
fn build_clientscript_names(
    archive: u8,
    group_ids: &[i32],
    cache: &Cache,
    map_names: &HashMap<i32, String>,
    name_lookup: Option<&HashMap<i32, String>>,
    pack_data: &HashMap<&'static str, BTreeMap<u32, String>>,
) -> NameMaps {
    let name_hashes = cache.index(archive).group_name_hashes.as_deref();
    let mut scripts: BTreeMap<u32, String> = BTreeMap::new();
    for &gid in group_ids {
        let gid = gid as u32;
        scripts.insert(gid, group_path(archive, gid, name_hashes, map_names, name_lookup));
    }
    let mut names = NameMaps::new();
    names.set_scripts(&scripts);
    if let Some(m) = pack_data.get("varp") {
        names.set_varps(m);
    }
    if let Some(m) = pack_data.get("varbit") {
        names.set_varbits(m);
    }
    names
}

/// Compute the on-disk path stem (no extension) for a group.
fn group_path(
    archive: u8,
    group_id: u32,
    name_hashes: Option<&[i32]>,
    map_names: &HashMap<i32, String>,
    name_lookup: Option<&HashMap<i32, String>>,
) -> String {
    // For archives that carry name hashes AND we have a reference name map, try the hash
    // match first. Lowercased to match Lost City convention. Falls through to the numeric
    // default if no match.
    if let (Some(hashes), Some(lookup)) = (name_hashes, name_lookup) {
        if let Some(hash) = hashes.get(group_id as usize) {
            if let Some(name) = lookup.get(hash) {
                return name.to_lowercase();
            }
        }
    }
    // Pure-asset archives use BARE ids ({id}.ext): the parent dir already
    // names the type, so a `model_`/`sprite_`/… prefix is redundant (and
    // jagfx/vorbis were already bare). The stem is the rename surface —
    // a real name from the .pack replaces the bare id. Clientscripts (12)
    // KEEP `script_` because the stem doubles as the symbolic name cs2
    // sources reference via `gosub <name>` (a bare number isn't an ident).
    match archive {
        // 3 (interfaces) joins the bare-id set: the `.if` extension already
        // says it's an interface, so an `if_` prefix is redundant.
        0 | 1 | 3 | 6 | 7 | 8 | 9 | 10 | 11 | 13 | 15 => format!("{group_id}"),
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
        // Sound-effect synth records — bare ids so jagfx.pack can give
        // them real names (same rationale as vorbis archive 14).
        4 => format!("{group_id}"),
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
        12 => format!("script_{group_id}"),
        // jagfx (4) and vorbis (14) were already bare; everything else.
        _ => format!("{group_id}"),
    }
}

// Re-export to silence unused-import warning (MASTER_ARCHIVE used in tests only).
const _: u8 = MASTER_ARCHIVE;
