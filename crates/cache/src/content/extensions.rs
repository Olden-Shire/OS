//! Per-scope file extensions for the on-disk Content tree.
//!
//! Only scopes where the raw cache bytes ARE the conventional format get a typed
//! extension. Things that need format-level decode/encode (Jagex MIDI → standard MIDI,
//! Jagex sprites → PNG, custom vorbis variant, etc.) stay `.dat` until those codecs land
//! — using a misleading extension would imply the file is in a standard format when it
//! isn't.

use std::path::{Path, PathBuf};

/// Subdirectory (under the archive dir) a group's files live in, sharding
/// the otherwise-flat huge archives into navigable 1000-id buckets:
/// `models/07000/model_7123.ob2`, `anims/01000/anim_1042/…`. Only the
/// archives the user reorganises (anims=0, bases=1, models=7) shard;
/// everything else returns `None` (files stay directly under the archive
/// dir). Derived purely from the group id so unpack and pack agree
/// without storing the shard in `_meta.json`.
#[must_use]
pub fn shard_subdir(archive: u8, group_id: u32) -> Option<String> {
    match archive {
        // anims(0) bases(1) jagfx(4) models(7) sprites(8): big single-/
        // multi-file archives sharded by group id.
        0 | 1 | 4 | 7 | 8 => Some(format!("{:05}", group_id / 1000 * 1000)),
        _ => None,
    }
}

/// Bucket subdir for ONE file *inside* a multi-file group dir, sharding
/// the huge config types (loc 26k, obj 12k, …) by file id. Only groups
/// with more than `SHARD_THRESHOLD` files shard; smaller ones stay flat.
/// Derived from file count + id so unpack and pack agree from the
/// manifest alone.
pub const SHARD_THRESHOLD: usize = 1000;

#[must_use]
pub fn intra_group_shard(file_count: usize, file_id: i32) -> Option<String> {
    if file_count > SHARD_THRESHOLD && file_id >= 0 {
        Some(format!("{:05}", file_id / 1000 * 1000))
    } else {
        None
    }
}

/// The base directory a group's file(s) live in: the archive dir, plus the
/// shard subdir for sharded archives.
#[must_use]
pub fn group_base(archive_dir: &Path, archive: u8, group_id: u32) -> PathBuf {
    match shard_subdir(archive, group_id) {
        Some(s) => archive_dir.join(s),
        None => archive_dir.to_path_buf(),
    }
}

/// Extension for a single-file group's payload, given its archive and a peek at the bytes.
/// `payload` is the *raw cache bytes* (post-decompression, pre-codec).
///
/// Archives whose raw bytes need a format-aware codec to produce a standard file (songs,
/// jingles, sprites, …) carry their *standardized* extension here so the on-disk file
/// reflects what's actually written. The unpack pipeline applies the codec; pack reverses
/// it for CRC-identical repack.
#[must_use]
pub fn single_file_ext(archive: u8, payload: &[u8]) -> &'static str {
    match archive {
        6 => "mid",     // songs — decoded via io::midi to standard MIDI
        7 => "ob2",     // models — raw cache bytes are the .ob2 format
        10 => sniff_binary(payload),
        11 => "mid",    // jingles — same codec as songs
        12 => "cs2",    // clientscripts — structured source (or .cs2asm fallback), chosen in unpack
        _ => "dat",
    }
}

/// `true` if a single-file scope requires the Jagex MIDI codec to convert between raw
/// cache bytes and on-disk standard MIDI.
#[must_use]
pub const fn is_midi_archive(archive: u8) -> bool {
    matches!(archive, 6 | 11)
}

/// `true` if a single-file scope holds CS2 clientscript bytecode, which unpacks to
/// structured `.cs2` source (`cs2_decompile`/`cs2_source`, verified byte-exact per
/// script) or a `.cs2asm` assembly fallback, and packs back to byte-identical
/// bytecode either way.
#[must_use]
pub const fn is_clientscript_archive(archive: u8) -> bool {
    archive == 12
}

/// Extension for a file *inside* a multi-file group directory (e.g. anim frames).
#[must_use]
pub fn multi_file_inner_ext(archive: u8, _group_id: u32) -> &'static str {
    match archive {
        0 => "anim",
        _ => "dat",
    }
}

fn sniff_binary(bytes: &[u8]) -> &'static str {
    // JFIF / EXIF JPEG: starts with FF D8 FF E0 (JFIF) or FF D8 FF E1 (EXIF).
    if bytes.len() >= 4 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return "jpg";
    }
    "dat"
}
