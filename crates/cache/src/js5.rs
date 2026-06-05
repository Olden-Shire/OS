//! JS5 archive index decoder and JS5 packet (compression wrapper) helper.
//!
//! Mirrors `jagex3.js5.Js5.decodeIndex` and `Js5.getUncompressedPacket` from the rev1 Java
//! client. We only port the *decode* side — the client also implements upload/download
//! protocol, which the server doesn't need.

use io::{bzip2, gzip, Packet, crc32};

/// Decoded archive directory. `group_*` and `file_*` arrays are sparse — they're sized to
/// `max_group_id + 1` and zero-filled for absent entries; iterate `group_ids` for the
/// canonical list of present groups.
#[derive(Debug, Clone)]
pub struct Js5Index {
    /// CRC32 of the raw (compressed) index bytes as they sit in the master archive.
    pub crc: u32,
    pub protocol: u8,
    /// Index revision number (proto >= 6). 0 for proto 5.
    pub revision: i32,
    pub size: u32,
    /// Sparse list of present group IDs, in declaration order.
    pub group_ids: Vec<i32>,
    /// `[group_id] → cp1252 hash` of the group's name. `Some` iff the archive was packed with
    /// names (info bit 0 set).
    pub group_name_hashes: Option<Vec<i32>>,
    pub group_checksums: Vec<i32>,
    pub group_versions: Vec<i32>,
    /// `[group_id] → number of files in the group`.
    pub group_sizes: Vec<u32>,
    /// `[group_id] → list of file IDs present in that group`.
    pub file_ids: Vec<Vec<i32>>,
    /// `[group_id][file_id] → name hash`. `Some` iff the archive was packed with names.
    pub file_name_hashes: Option<Vec<Vec<i32>>>,
}

impl Js5Index {
    /// Look up a group id by its CP1252 name hash. Only returns `Some` for indices that
    /// were packed with names (info bit 0 set in the header).
    ///
    /// Linear scan over present groups — fine for occasional lookups. If callers need to
    /// resolve many names against the same index, build their own reverse map.
    #[must_use]
    pub fn find_group_by_hash(&self, hash: i32) -> Option<u32> {
        let table = self.group_name_hashes.as_ref()?;
        self.group_ids.iter().copied().find_map(|gid| {
            let g = gid as usize;
            if table.get(g).copied() == Some(hash) { Some(gid as u32) } else { None }
        })
    }

    /// Decode an index from its on-disk bytes (the JS5 wrapper is unwrapped internally).
    #[must_use]
    pub fn decode(raw: &[u8]) -> Self {
        let crc = crc32::checksum(raw, 0, raw.len());
        let uncompressed = decode_packet(raw);
        let mut p = Packet::from_vec(uncompressed);

        let protocol = p.g1() as u8;
        assert!(matches!(protocol, 5..=7), "unknown JS5 protocol {protocol}");
        let revision = if protocol >= 6 { p.g4() } else { 0 };
        let info = p.g1();
        let size = if protocol >= 7 { p.g_smart2or4() } else { p.g2() } as u32;

        let mut group_ids = Vec::with_capacity(size as usize);
        let mut prev = 0i32;
        let mut max_id = -1i32;
        for _ in 0..size {
            let delta = if protocol >= 7 { p.g_smart2or4() } else { p.g2() };
            prev += delta;
            group_ids.push(prev);
            if prev > max_id {
                max_id = prev;
            }
        }
        let limit = (max_id + 1) as usize;

        let group_name_hashes = if info & 1 != 0 {
            let mut h = vec![0i32; limit];
            for &gid in &group_ids {
                h[gid as usize] = p.g4();
            }
            Some(h)
        } else {
            None
        };

        let mut group_checksums = vec![0i32; limit];
        for &gid in &group_ids {
            group_checksums[gid as usize] = p.g4();
        }
        let mut group_versions = vec![0i32; limit];
        for &gid in &group_ids {
            group_versions[gid as usize] = p.g4();
        }
        let mut group_sizes = vec![0u32; limit];
        for &gid in &group_ids {
            group_sizes[gid as usize] = p.g2() as u32;
        }

        let mut file_ids: Vec<Vec<i32>> = vec![Vec::new(); limit];
        for &gid in &group_ids {
            let count = group_sizes[gid as usize] as usize;
            let mut ids = Vec::with_capacity(count);
            let mut prev = 0i32;
            for _ in 0..count {
                let delta = if protocol >= 7 { p.g_smart2or4() } else { p.g2() };
                prev += delta;
                ids.push(prev);
            }
            file_ids[gid as usize] = ids;
        }

        let file_name_hashes = if info & 1 != 0 {
            let mut all: Vec<Vec<i32>> = vec![Vec::new(); limit];
            for &gid in &group_ids {
                let max_fid = file_ids[gid as usize].iter().copied().max().unwrap_or(-1);
                let mut per_group = vec![0i32; (max_fid + 1) as usize];
                for &fid in &file_ids[gid as usize] {
                    per_group[fid as usize] = p.g4();
                }
                all[gid as usize] = per_group;
            }
            Some(all)
        } else {
            None
        };

        Self {
            crc,
            protocol,
            revision,
            size,
            group_ids,
            group_name_hashes,
            group_checksums,
            group_versions,
            group_sizes,
            file_ids,
            file_name_hashes,
        }
    }
}

/// Unwrap a JS5 compression packet: 1-byte type, 4-byte compressed length,
/// (4-byte uncompressed length if compressed,) then payload. Returns the decompressed bytes.
#[must_use]
pub fn decode_packet(src: &[u8]) -> Vec<u8> {
    let mut p = Packet::from_vec(src.to_vec());
    let ctype = p.g1();
    let clen = p.g4() as usize;

    if ctype == 0 {
        let mut out = vec![0u8; clen];
        p.gdata(&mut out, 0, clen);
        return out;
    }

    let _ulen = p.g4() as usize;
    // Compressed payload starts after the 9-byte JS5 header.
    let compressed = &src[9..9 + clen];
    match ctype {
        1 => bzip2::decompress(compressed),
        2 => gzip::decompress(compressed),
        _ => panic!("unknown JS5 compression type {ctype}"),
    }
}
