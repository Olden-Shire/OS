// @ObfuscatedName("ch")
//
// jagex3.js5.Js5 — base class for an in-memory index of one cache archive.
// `decodeIndex` consumes the master-index payload the server hands back,
// then `requestGroupDownload`/`fetchFile` orchestrate per-group downloads.
//
// The Rust port keeps the same field layout; the abstract `requestGroupDownload2`
// is implemented by Js5Loader (the only concrete subclass).

#![allow(dead_code)]

use std::sync::LazyLock;

use crate::datastruct::int_hash_table::IntHashTable;
use crate::io::byte_array_wrapper;
use crate::io::gzip::GZip;
use crate::io::packet::{Packet, CRCTABLE};

// @ObfuscatedName("ch.u") — shared inflater. LazyLock-backed because the
// gamepack initialises `gzip` in the static block.
pub static GZIP: LazyLock<GZip> = LazyLock::new(GZip::new);

// @ObfuscatedName("ch.b") — soft-cap on uncompressed group size. 0 means no cap.
pub static MAXSIZE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

pub struct Js5 {
    // @ObfuscatedName("ch.r")
    pub size: i32,

    // @ObfuscatedName("ch.d")
    pub group_ids: Vec<i32>,

    // @ObfuscatedName("ch.l")
    pub group_name_hash: Option<Vec<i32>>,

    // @ObfuscatedName("ch.m")
    pub group_name_hash_table: Option<IntHashTable>,

    // @ObfuscatedName("ch.c")
    pub group_checksums: Vec<i32>,

    // @ObfuscatedName("ch.n")
    pub group_versions: Vec<i32>,

    // @ObfuscatedName("ch.j")
    pub group_sizes: Vec<i32>,

    // @ObfuscatedName("ch.z")
    pub file_ids: Vec<Option<Vec<i32>>>,

    // @ObfuscatedName("ch.g")
    pub file_name_hashes: Option<Vec<Option<Vec<i32>>>>,

    // @ObfuscatedName("ch.q")
    pub file_name_hash_tables: Option<Vec<Option<IntHashTable>>>,

    // @ObfuscatedName("ch.i")
    pub packed: Vec<Option<Vec<u8>>>,

    // @ObfuscatedName("ch.s")
    pub unpacked: Vec<Option<Vec<Option<Vec<u8>>>>>,

    // @ObfuscatedName("ch.v")
    pub crc: i32,

    // @ObfuscatedName("ch.w")
    pub discard_packed: bool,

    // @ObfuscatedName("ch.e")
    pub discard_unpacked: bool,
}

impl Js5 {
    pub fn new(discard_packed: bool, discard_unpacked: bool) -> Self {
        Self {
            size: 0,
            group_ids: Vec::new(),
            group_name_hash: None,
            group_name_hash_table: None,
            group_checksums: Vec::new(),
            group_versions: Vec::new(),
            group_sizes: Vec::new(),
            file_ids: Vec::new(),
            file_name_hashes: None,
            file_name_hash_tables: None,
            packed: Vec::new(),
            unpacked: Vec::new(),
            crc: 0,
            discard_packed,
            discard_unpacked,
        }
    }

    // @ObfuscatedName("ch.r([BI)V")
    pub fn decode_index(&mut self, src: &[u8]) {
        // inlined getcrc
        let mut var3: i32 = -1;
        for &b in src {
            var3 = (var3 as u32 >> 8) as i32 ^ CRCTABLE[((var3 ^ b as i32) & 0xFF) as usize];
        }
        self.crc = !var3;

        let unc = Self::get_uncompressed_packet(src);
        let mut buf = Packet::from_vec(unc);
        let protocol = buf.g1();
        if !(5..=7).contains(&protocol) {
            panic!("Incorrect JS5 protocol number: {protocol}");
        }
        if protocol >= 6 {
            buf.g4();
        }
        let info = buf.g1();
        self.size = if protocol >= 7 { buf.gSmart2or4() } else { buf.g2() };

        let mut prev_group_id = 0i32;
        let mut max_group_id: i32 = -1;
        self.group_ids = vec![0; self.size as usize];
        if protocol >= 7 {
            for i in 0..self.size as usize {
                prev_group_id = prev_group_id.wrapping_add(buf.gSmart2or4());
                self.group_ids[i] = prev_group_id;
                if self.group_ids[i] > max_group_id {
                    max_group_id = self.group_ids[i];
                }
            }
        } else {
            for i in 0..self.size as usize {
                prev_group_id = prev_group_id.wrapping_add(buf.g2());
                self.group_ids[i] = prev_group_id;
                if self.group_ids[i] > max_group_id {
                    max_group_id = self.group_ids[i];
                }
            }
        }

        let len = (max_group_id + 1) as usize;
        self.group_checksums = vec![0; len];
        self.group_versions = vec![0; len];
        self.group_sizes = vec![0; len];
        self.file_ids = (0..len).map(|_| None).collect();
        self.packed = (0..len).map(|_| None).collect();
        self.unpacked = (0..len).map(|_| None).collect();

        if info != 0 {
            let mut group_name_hash = vec![0i32; len];
            for i in 0..self.size as usize {
                group_name_hash[self.group_ids[i] as usize] = buf.g4();
            }
            self.group_name_hash_table = Some(IntHashTable::new(&group_name_hash));
            self.group_name_hash = Some(group_name_hash);
        }

        for i in 0..self.size as usize {
            self.group_checksums[self.group_ids[i] as usize] = buf.g4();
        }
        for i in 0..self.size as usize {
            self.group_versions[self.group_ids[i] as usize] = buf.g4();
        }
        for i in 0..self.size as usize {
            self.group_sizes[self.group_ids[i] as usize] = buf.g2();
        }

        if protocol >= 7 {
            for i in 0..self.size as usize {
                let id = self.group_ids[i] as usize;
                let size = self.group_sizes[id] as usize;
                let mut prev_file_id = 0i32;
                let mut max_file_id: i32 = -1;
                let mut ids = vec![0i32; size];
                for j in 0..size {
                    prev_file_id = prev_file_id.wrapping_add(buf.gSmart2or4());
                    ids[j] = prev_file_id;
                    if ids[j] > max_file_id {
                        max_file_id = ids[j];
                    }
                }
                self.file_ids[id] = Some(ids);
                self.unpacked[id] = Some((0..(max_file_id + 1) as usize).map(|_| None).collect());
            }
        } else {
            for i in 0..self.size as usize {
                let id = self.group_ids[i] as usize;
                let size = self.group_sizes[id] as usize;
                let mut prev_file_id = 0i32;
                let mut max_file_id: i32 = -1;
                let mut ids = vec![0i32; size];
                for j in 0..size {
                    prev_file_id = prev_file_id.wrapping_add(buf.g2());
                    ids[j] = prev_file_id;
                    if ids[j] > max_file_id {
                        max_file_id = ids[j];
                    }
                }
                self.file_ids[id] = Some(ids);
                self.unpacked[id] = Some((0..(max_file_id + 1) as usize).map(|_| None).collect());
            }
        }

        if info != 0 {
            let mut fnh: Vec<Option<Vec<i32>>> = (0..len).map(|_| None).collect();
            let mut fnht: Vec<Option<IntHashTable>> = (0..len).map(|_| None).collect();
            for i in 0..self.size as usize {
                let id = self.group_ids[i] as usize;
                let size = self.group_sizes[id] as usize;
                let unpacked_len = self.unpacked[id].as_ref().unwrap().len();
                let mut hashes = vec![0i32; unpacked_len];
                for j in 0..size {
                    let fid = self.file_ids[id].as_ref().unwrap()[j] as usize;
                    hashes[fid] = buf.g4();
                }
                fnht[id] = Some(IntHashTable::new(&hashes));
                fnh[id] = Some(hashes);
            }
            self.file_name_hashes = Some(fnh);
            self.file_name_hash_tables = Some(fnht);
        }
    }

    // @ObfuscatedName("c.a([BI)[B")
    pub fn get_uncompressed_packet(src: &[u8]) -> Vec<u8> {
        let mut buf = Packet::from_vec(src.to_vec());
        let ctype = buf.g1();
        let clen = buf.g4();

        let maxsize = MAXSIZE.load(std::sync::atomic::Ordering::Relaxed);
        if clen < 0 || (maxsize != 0 && clen > maxsize) {
            panic!("");
        }

        if ctype == 0 {
            let mut data = vec![0u8; clen as usize];
            buf.gdata(&mut data, 0, clen);
            return data;
        }

        let ulen = buf.g4();
        if ulen < 0 || (maxsize != 0 && ulen > maxsize) {
            panic!("");
        }

        let mut data = vec![0u8; ulen as usize];
        if ctype == 1 {
            // BZip2.decompress(data, ulen, src, clen, 9)
            crate::io::bzip2::decompress(&mut data, ulen, src, clen, 9).expect("bzip2");
        } else {
            GZIP.decompress(&mut buf, &mut data).expect("gzip");
        }
        data
    }

    // @ObfuscatedName("ch.b(I[II)Z")
    pub fn unpack_group_data(&mut self, group_id: i32, key: Option<&[i32; 4]>) -> bool {
        let gid = group_id as usize;
        if self.packed[gid].is_none() {
            return false;
        }
        let group_size = self.group_sizes[gid];
        let ids = self.file_ids[gid].clone().unwrap();
        let mut all_present = true;
        if let Some(slot) = &self.unpacked[gid] {
            for k in 0..group_size as usize {
                if slot[ids[k] as usize].is_none() {
                    all_present = false;
                    break;
                }
            }
        }
        if all_present {
            return true;
        }

        let packed = self.packed[gid].as_ref().unwrap().clone();
        // Java: if key == null || key all zero → unwrap(false). Else
        // unwrap(true) then tinydec(key, 5, len). Our stub byte_array
        // wrapper ignores the bool, but we still need to decrypt.
        let key_present = key.map_or(false, |k| k[0] != 0 || k[1] != 0 || k[2] != 0 || k[3] != 0);
        let var8 = if key_present {
            let mut buf = byte_array_wrapper::unwrap(&packed, true);
            let mut p = Packet::from_vec(std::mem::take(&mut buf));
            let len = p.data.len() as i32;
            p.tinydec(key.unwrap(), 5, len);
            p.data
        } else {
            byte_array_wrapper::unwrap(&packed, false)
        };

        let var10 = Self::get_uncompressed_packet(&var8);

        if self.discard_packed {
            self.packed[gid] = None;
        }

        let slot = self.unpacked[gid].as_mut().unwrap();
        if group_size > 1 {
            let var28 = var10.len() as i32;
            let var44 = var28 - 1;
            let var29 = var10[var44 as usize] as i32 & 0xFF;
            let var30 = var44 - group_size * var29 * 4;
            let mut var31 = Packet::from_vec(var10.clone());
            let mut var32 = vec![0i32; group_size as usize];
            var31.pos = var30;
            for _ in 0..var29 {
                let mut var34: i32 = 0;
                for var35 in 0..group_size as usize {
                    var34 = var34.wrapping_add(var31.g4());
                    var32[var35] = var32[var35].wrapping_add(var34);
                }
            }
            let mut var36: Vec<Vec<u8>> =
                (0..group_size as usize).map(|i| vec![0u8; var32[i] as usize]).collect();
            for i in 0..group_size as usize {
                var32[i] = 0;
            }
            var31.pos = var30;
            let mut var38 = 0usize;
            for _ in 0..var29 {
                let mut var40: i32 = 0;
                for var41 in 0..group_size as usize {
                    var40 = var40.wrapping_add(var31.g4());
                    let dest = &mut var36[var41];
                    let dst_off = var32[var41] as usize;
                    dest[dst_off..dst_off + var40 as usize]
                        .copy_from_slice(&var10[var38..var38 + var40 as usize]);
                    var32[var41] = var32[var41].wrapping_add(var40);
                    var38 += var40 as usize;
                }
            }
            for i in 0..group_size as usize {
                slot[ids[i] as usize] = Some(var36[i].clone());
            }
        } else {
            slot[ids[0] as usize] = Some(var10);
        }
        true
    }

    // @ObfuscatedName("ch.y(Ljava/lang/String;I)I") — Js5.getGroupId
    pub fn get_group_id(&self, group: &str) -> i32 {
        let lower = group.to_lowercase();
        let hash = crate::jstring::computeCp1252HashFromUtf8(&lower);
        match self.group_name_hash_table.as_ref() {
            Some(t) => t.find(hash),
            None => -1,
        }
    }

    // @ObfuscatedName("ch.t(ILjava/lang/String;B)I") — Js5.getFileId
    pub fn get_file_id(&self, group_id: i32, file: &str) -> i32 {
        let lower = file.to_lowercase();
        let hash = crate::jstring::computeCp1252HashFromUtf8(&lower);
        match self.file_name_hash_tables.as_ref().and_then(|v| v.get(group_id as usize)).and_then(|o| o.as_ref()) {
            Some(t) => t.find(hash),
            None => -1,
        }
    }

    // @ObfuscatedName("ch.m(II[IS)[B") — Js5.fetchFile
    //
    // Returns None when the group hasn't been downloaded/unpacked yet —
    // caller is expected to re-poll once the group arrives.
    pub fn fetch_file(&mut self, group_id: i32, file_id: i32) -> Option<Vec<u8>> {
        self.fetch_file_with_key(group_id, file_id, None)
    }

    // @ObfuscatedName("ch.m(II[IS)[B") — Js5.fetchFile with XTEA key.
    pub fn fetch_file_with_key(&mut self, group_id: i32, file_id: i32, key: Option<&[i32; 4]>) -> Option<Vec<u8>> {
        if group_id < 0 || group_id as usize >= self.unpacked.len() {
            return None;
        }
        if self.unpacked[group_id as usize].is_none() {
            return None;
        }
        let fid = file_id as usize;
        let len = self.unpacked[group_id as usize].as_ref().unwrap().len();
        if file_id < 0 || fid >= len {
            return None;
        }
        let need_unpack = self.unpacked[group_id as usize].as_ref().unwrap()[fid].is_none();
        if need_unpack {
            let ok = self.unpack_group_data(group_id, key);
            if !ok {
                return None;
            }
        }
        let data = self.unpacked[group_id as usize].as_ref().unwrap()[fid].clone();
        if self.discard_unpacked {
            if let Some(slot) = self.unpacked[group_id as usize].as_mut() {
                slot[fid] = None;
            }
        }
        data
    }

    // @ObfuscatedName("ch.u(IS)I") — Js5.getFileIdLimit. Verbatim port
    // of Js5.java:349-352. Returns `unpacked[group_id].length` i.e.
    // maxFileId + 1, NOT the encoded file count. Sparse archives
    // diverge from `group_sizes`; callers sizing tables by this
    // need the slot count not the entry count.
    pub fn get_file_id_limit(&self, group_id: i32) -> i32 {
        self.unpacked.get(group_id as usize)
            .and_then(|o| o.as_ref())
            .map(|v| v.len() as i32)
            .unwrap_or(0)
    }

    // @ObfuscatedName("ch.v(I)I") — Js5.getGroupCount. Verbatim port
    // of Js5.java:355-358. Returns `unpacked.length` i.e. maxGroupId
    // + 1, NOT the encoded group count. Sparse archives diverge.
    pub fn get_group_count(&self) -> i32 {
        self.unpacked.len() as i32
    }

    // @ObfuscatedName("ch.f(I)[I") — Js5.getFileList. Returns the
    // file ids for a group, in encoded order. None when the group
    // hasn't been decoded.
    pub fn get_file_list(&self, group_id: i32) -> Option<Vec<i32>> {
        self.file_ids.get(group_id as usize).and_then(|o| o.clone())
    }

    // @ObfuscatedName("ch.k(II)[B") — Js5.peekFile.
    //
    // Like fetch_file but never triggers a network request and never
    // unpacks. Returns None if the group isn't already resident in
    // `unpacked`. Used by AnimFrameSet which doesn't want to block on
    // misses (bases stream in over multiple frames).
    pub fn peek_file(&self, group_id: i32, file_id: i32) -> Option<Vec<u8>> {
        let gid = group_id as usize;
        let fid = file_id as usize;
        let group = self.unpacked.get(gid)?.as_ref()?;
        group.get(fid).and_then(|o| o.clone())
    }

    // @ObfuscatedName("ch.z(II)[B") — Js5.getFile(int id). Verbatim
    // port of Js5.java:293-301. One-arg overload: dispatches to
    // (0, id) if the archive has a single group, or (id, 0) if every
    // group holds a single file. Panics otherwise — Java throws.
    pub fn fetch_file_one_arg(&mut self, id: i32) -> Option<Vec<u8>> {
        if self.unpacked.len() == 1 {
            return self.fetch_file(0, id);
        }
        let uid = id as usize;
        if let Some(Some(group)) = self.unpacked.get(uid) {
            if group.len() == 1 {
                return self.fetch_file(id, 0);
            }
        }
        // Java throws; we return None to keep the caller side ergonomic.
        None
    }

    // @ObfuscatedName("ch.q(II)[B") — Js5.peekFile(int id). Verbatim
    // port of Js5.java:327-335. One-arg peek mirror — dispatches by
    // archive shape just like fetch_file_one_arg.
    pub fn peek_file_one_arg(&self, id: i32) -> Option<Vec<u8>> {
        if self.unpacked.len() == 1 {
            return self.peek_file(0, id);
        }
        let uid = id as usize;
        if let Some(Some(group)) = self.unpacked.get(uid) {
            if group.len() == 1 {
                return self.peek_file(id, 0);
            }
        }
        None
    }

    // @ObfuscatedName("ch.w(II)V") — Js5.discardFiles. Verbatim port
    // of Js5.java:362-366. Java nulls each per-file slot but KEEPS the
    // outer group array; nulling the whole group breaks fetch_file's
    // bounds check (it returns None forever instead of re-unpacking
    // from `packed`). Fixed: clear inner Vec, keep outer slot Some.
    pub fn discard_files(&mut self, group_id: i32) {
        let gid = group_id as usize;
        if let Some(Some(slot)) = self.unpacked.get_mut(gid) {
            for entry in slot.iter_mut() {
                *entry = None;
            }
        }
    }

    // @ObfuscatedName("ch.e(I)V") — Js5.discardAllFiles. Verbatim
    // port of Js5.java:370-380. Same fix as discard_files: null the
    // INNER file slots, keep outer group arrays intact.
    pub fn discard_all_files(&mut self) {
        for slot in self.unpacked.iter_mut() {
            if let Some(group) = slot.as_mut() {
                for entry in group.iter_mut() {
                    *entry = None;
                }
            }
        }
    }

    // @ObfuscatedName("ch.j(B)Z")
    pub fn request_full_download(
        &mut self,
        mut request: impl FnMut(i32),
    ) -> bool {
        let mut done = true;
        let groups = self.group_ids.clone();
        for &group_id in &groups {
            if self.packed[group_id as usize].is_some() {
                continue;
            }
            request(group_id);
            if self.packed[group_id as usize].is_none() {
                done = false;
            }
        }
        done
    }
}
