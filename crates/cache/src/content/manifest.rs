//! Per-archive manifest written alongside the unpacked content tree. Records the
//! per-group metadata the pack pipeline needs to reconstruct CRC-identical groups
//! (compression type, the 2-byte version trailer, XTEA key for encrypted maps locs).
//!
//! File-id ordering for multi-file groups is *not* stored — pack reads the on-disk file
//! names, parses them as numbers, and uses the sorted order. The Jagex `Js5Index` always
//! stores file IDs via positive deltas so the on-disk sort matches the original
//! declaration order.

use serde::{Deserialize, Serialize};

/// Per-group metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMeta {
    /// Group id within its archive.
    pub id: u32,
    /// JS5 compression type: 0 = none, 1 = bzip2, 2 = gzip.
    pub ctype: u8,
    /// 2-byte version trailer appended after the (possibly-encrypted) payload.
    pub version: [u8; 2],
    /// Path on disk *relative to the archive directory*. Single-file groups point at a
    /// `.dat` file; multi-file groups point at a directory containing per-file `.dat`s.
    pub path: String,
    /// XTEA key for encrypted groups (rev1 = map loc files only). When present, the pack
    /// pipeline encrypts bytes `[5..len-2]` of the wrapped group with this key.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub xtea_key: Option<[i32; 4]>,
    /// File IDs for multi-file groups, in **declaration order** (matches the Js5Index's
    /// delta-encoded order, which is what `unpack_group` returns chunks in). Pack reads
    /// files from `path/{fid}.dat` and reassembles them in this exact order — critical
    /// because some groups have non-monotonic file IDs that wouldn't sort correctly by
    /// filename alone. `None` for single-file groups.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub file_ids: Option<Vec<i32>>,
    /// Chunk distribution for multi-file groups whose original `chunk_count > 1`.
    /// `chunks[chunk_idx][file_idx]` = bytes the chunk takes from file `file_idx`.
    /// `None` is shorthand for `chunk_count == 1` (each file is one contiguous chunk).
    /// Stored because the chunk split is data-dependent (e.g. anims/0 uses 3 chunks);
    /// without it, our recompressed body interleaves differently and the JS5 trailer
    /// shape changes, breaking byte-identity.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub chunks: Option<Vec<Vec<u32>>>,
    /// `true` when every file in this multi-file group is an empty config
    /// stub (a lone `0x00` terminator). Such groups (the rev1-unused
    /// config types — params/struct/etc., all stripped to empties in this
    /// cache) write NO files to disk: pack regenerates each `0x00` record
    /// from `file_ids` + `chunks`, reproducing the group byte-identically
    /// while saving ~1000 useless 1-byte files. `path` then has no dir.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub placeholder: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveManifest {
    pub archive_id: u8,
    pub archive_name: String,
    /// Sorted ascending by `id` so the pack pipeline writes sectors in a deterministic order.
    pub groups: Vec<GroupMeta>,
}

/// Manifest for the master archive (idx255). Each entry is the decompressed Js5Index for
/// one game archive (0..15); `path` is the decompressed `.dat` filename inside `_master/`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasterManifest {
    pub entries: Vec<GroupMeta>,
}
