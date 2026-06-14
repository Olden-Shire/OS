// @ObfuscatedName("dq")
// jag::oldscape::jagex3::Js5Loader
//
// jagex3.js5.Js5Loader extends Js5 — the per-archive provider Client holds
// references to (`anims`, `bases`, `config`, ...). Holds the disk-cache
// pointers + per-group "loaded" bitmap and dispatches actual download
// requests through Js5Net.

#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crc32fast::Hasher;

use crate::io::byte_array_wrapper;
use crate::io::packet::Packet;

use super::js5::Js5;
use super::js5_net;

pub struct Js5Loader {
    // Java extends Js5; we embed.
    pub base: Js5,

    // @ObfuscatedName("dq.f") — DataFile dataFile (disk cache or None)
    pub data_file: Option<i32>,

    // @ObfuscatedName("dq.k") — DataFile indexDataFile
    pub index_data_file: Option<i32>,

    // @ObfuscatedName("dq.o")
    pub archive: i32,

    // @ObfuscatedName("dq.a")
    pub load_status: AtomicBool,

    // @ObfuscatedName("dq.h")
    pub remote_enabled: bool,

    // @ObfuscatedName("dq.x")
    pub loaded_groups: Mutex<Option<Vec<bool>>>,

    // @ObfuscatedName("dq.ad")
    pub index_crc: i32,

    // @ObfuscatedName("dq.ac")
    pub index_version: i32,

    // @ObfuscatedName("dq.aa")
    pub field1581: i32,

    // custom — slot id assigned by js5_net at registration time so the
    // Js5Net loop can find this loader by integer index.
    pub slot: i32,
}

impl Js5Loader {
    // ctor — no @ObfuscatedName (custom on Js5Loader, not on dq)
    pub fn new(
        archive: i32,
        discard_packed: bool,
        discard_unpacked: bool,
        remote_enabled: bool,
    ) -> Self {
        Self {
            base: Js5::new(discard_packed, discard_unpacked),
            data_file: None,
            index_data_file: None,
            archive,
            load_status: AtomicBool::new(false),
            remote_enabled,
            loaded_groups: Mutex::new(None),
            index_crc: 0,
            index_version: 0,
            field1581: -1,
            slot: -1,
        }
    }

    // @ObfuscatedName("dq.bo(B)I")
    pub fn get_index_percentage(&self) -> i32 {
        if self.load_status.load(Ordering::SeqCst) {
            return 100;
        }
        if self.base.packed.is_empty() {
            let var1 = js5_net::transfer_progress(255, self.archive);
            if var1 >= 100 { 99 } else { var1 }
        } else {
            99
        }
    }

    // @ObfuscatedName("dq.d(IB)V")
    pub fn update_cache_hint(&self, group_id: i32) {
        js5_net::update_cache_hint(self.archive, group_id);
    }

    // @ObfuscatedName("dq.i(IB)V")
    pub fn request_group_download2(&self, group_id: i32) {
        let already_local = {
            let guard = self.loaded_groups.lock().unwrap();
            self.data_file.is_some() && guard.as_ref().is_some_and(|v| v.get(group_id as usize).copied().unwrap_or(false))
        };
        if already_local {
            // disk cache stubbed; nothing queued
        } else {
            js5_net::queue_request(
                self.slot,
                self.archive,
                group_id,
                self.base.group_checksums[group_id as usize],
                2,
                true,
            );
        }
    }

    // @ObfuscatedName("dq.bp(B)Z") — Js5.requestFullDownload, the concrete
    // Js5Loader form (Java's abstract Js5.requestFullDownload calls the
    // loader's requestGroupDownload). Returns true once every group's
    // packed payload is present locally. Used by the login loading
    // sequence (Client step 70 config, step 130 interfaces/scripts/
    // fontMetrics) to block until an archive is fully resident — the
    // config/interface/script clientscript hooks all assume this.
    pub fn request_full_download(&mut self) -> bool {
        let groups = self.base.group_ids.clone();
        let mut done = true;
        for group_id in groups {
            let gid = group_id as usize;
            if self.base.packed[gid].is_some() {
                continue;
            }
            self.request_group_download2(group_id);
            if self.base.packed[gid].is_none() {
                done = false;
            }
        }
        done
    }

    // Percentage of groups whose packed payload has arrived, for the
    // loading-bar text while request_full_download is still in flight.
    pub fn index_load_progress(&self) -> i32 {
        let groups = &self.base.group_ids;
        if groups.is_empty() {
            return 100;
        }
        let have = groups
            .iter()
            .filter(|&&g| self.base.packed[g as usize].is_some())
            .count();
        (have as i32) * 100 / (groups.len() as i32)
    }

    // @ObfuscatedName("dq.bq(III)V")
    pub fn request_index(&mut self, crc: i32, version: i32) {
        self.index_crc = crc;
        self.index_version = version;
        if self.index_data_file.is_none() {
            js5_net::queue_request(self.slot, 255, self.archive, self.index_crc, 0, true);
        }
    }

    // @ObfuscatedName("dq.bj(I[BZZB)V")
    pub fn write(&mut self, arg0: i32, arg1: Vec<u8>, arg2: bool, arg3: bool) {
        dbg_log!("[loader#{} archive={}] write group={} is_index={} urgent={} bytes={}", self.slot, self.archive, arg0, arg2, arg3, arg1.len());
        if !arg2 {
            // Group payload write.
            let mut data = arg1;
            let n = data.len();
            data[n - 2] = (self.base.group_versions[arg0 as usize] >> 8) as u8;
            data[n - 1] = self.base.group_versions[arg0 as usize] as u8;
            if self.data_file.is_some() {
                // disk cache stubbed
                let mut guard = self.loaded_groups.lock().unwrap();
                if let Some(v) = guard.as_mut() {
                    v[arg0 as usize] = true;
                }
            }
            if arg3 {
                self.base.packed[arg0 as usize] = Some(byte_array_wrapper::wrap(&data, false));
            }
            return;
        }
        if self.load_status.load(Ordering::SeqCst) {
            panic!("");
        }
        self.base.decode_index(&arg1);
        self.load_all_local();
        dbg_log!("[loader#{} archive={}] post-write load_status={}", self.slot, self.archive, self.load_status.load(Ordering::SeqCst));
    }

    // @ObfuscatedName("dq.bz(Lap;I[BZI)V")
    pub fn load_index(&mut self, idx_is_self: bool, group_id: i32, src: Option<Vec<u8>>, urgent: bool) {
        if !idx_is_self {
            if !urgent && self.field1581 == group_id {
                self.load_status.store(true, Ordering::SeqCst);
            }
            match src.as_ref() {
                None => {
                    let mut guard = self.loaded_groups.lock().unwrap();
                    if let Some(v) = guard.as_mut() {
                        v[group_id as usize] = false;
                    }
                    if self.remote_enabled || urgent {
                        js5_net::queue_request(
                            self.slot,
                            self.archive,
                            group_id,
                            self.base.group_checksums[group_id as usize],
                            2,
                            urgent,
                        );
                    }
                }
                Some(s) if s.len() <= 2 => {
                    let mut guard = self.loaded_groups.lock().unwrap();
                    if let Some(v) = guard.as_mut() {
                        v[group_id as usize] = false;
                    }
                    if self.remote_enabled || urgent {
                        js5_net::queue_request(
                            self.slot,
                            self.archive,
                            group_id,
                            self.base.group_checksums[group_id as usize],
                            2,
                            urgent,
                        );
                    }
                }
                Some(s) => {
                    let mut hasher = Hasher::new();
                    hasher.update(&s[..s.len() - 2]);
                    let var9 = hasher.finalize() as i32;
                    let var10 = ((s[s.len() - 2] as i32 & 0xFF) << 8) + (s[s.len() - 1] as i32 & 0xFF);
                    let mut guard = self.loaded_groups.lock().unwrap();
                    if self.base.group_checksums[group_id as usize] != var9
                        || self.base.group_versions[group_id as usize] != var10
                    {
                        if let Some(v) = guard.as_mut() {
                            v[group_id as usize] = false;
                        }
                        drop(guard);
                        if self.remote_enabled || urgent {
                            js5_net::queue_request(
                                self.slot,
                                self.archive,
                                group_id,
                                self.base.group_checksums[group_id as usize],
                                2,
                                urgent,
                            );
                        }
                    } else {
                        if let Some(v) = guard.as_mut() {
                            v[group_id as usize] = true;
                        }
                        if urgent {
                            self.base.packed[group_id as usize] =
                                Some(byte_array_wrapper::wrap(s, false));
                        }
                    }
                }
            }
            return;
        }

        if self.load_status.load(Ordering::SeqCst) {
            panic!("");
        }
        let Some(src) = src else {
            js5_net::queue_request(self.slot, 255, self.archive, self.index_crc, 0, true);
            return;
        };

        let mut hasher = Hasher::new();
        hasher.update(&src);
        let crc = hasher.finalize() as i32;
        let unc = Js5::get_uncompressed_packet(&src);
        let mut buf = Packet::from_vec(unc);
        let protocol = buf.g1();
        if protocol != 5 && protocol != 6 {
            panic!("Incorrect JS5 protocol number: {protocol}");
        }
        let mut version = 0;
        if protocol >= 6 {
            version = buf.g4();
        }
        if self.index_crc != crc || self.index_version != version {
            js5_net::queue_request(self.slot, 255, self.archive, self.index_crc, 0, true);
            return;
        }
        self.base.decode_index(&src);
        self.load_all_local();
    }

    // @ObfuscatedName("dq.bm(S)V")
    pub fn load_all_local(&mut self) {
        let n = self.base.packed.len();
        {
            let mut guard = self.loaded_groups.lock().unwrap();
            *guard = Some(vec![false; n]);
        }
        if self.data_file.is_none() {
            self.load_status.store(true, Ordering::SeqCst);
            return;
        }
        self.field1581 = -1;
        for var2 in 0..n {
            if self.base.group_sizes[var2] > 0 {
                // disk cache stubbed: nothing to queue
                self.field1581 = var2 as i32;
            }
        }
        if self.field1581 == -1 {
            self.load_status.store(true, Ordering::SeqCst);
        }
    }

    // @ObfuscatedName("ch.u(IS)I") — Js5.getFileIdLimit (Java does NOT
    // override this in Js5Loader). Returns unpacked[group].length =
    // maxFileId + 1, the sparse SLOT count — not the file count.
    // A previous version returned group_sizes[gid], which undersizes
    // *Type::install tables for any group with file-id gaps.
    pub fn get_file_id_limit(&self, group_id: i32) -> i32 {
        self.base.get_file_id_limit(group_id)
    }

    // @ObfuscatedName("dq.bn(II)I")
    pub fn get_group_load_progress(&self, arg0: i32) -> i32 {
        let gid = arg0 as usize;
        if self.base.packed[gid].is_none() {
            let g = self.loaded_groups.lock().unwrap();
            if g.as_ref().is_some_and(|v| v[gid]) {
                100
            } else {
                js5_net::transfer_progress(self.archive, arg0)
            }
        } else {
            100
        }
    }

    // @ObfuscatedName("ch.m(II[IS)[B") — Js5.fetchFile via Js5Loader so we
    // can call request_group_download2 on the miss path (Java's abstract
    // method on Js5 is concrete here).
    pub fn fetch_file(&mut self, group_id: i32, file_id: i32) -> Option<Vec<u8>> {
        let first = self.base.fetch_file(group_id, file_id);
        if first.is_some() {
            return first;
        }
        // Java: if unpack fails, trigger network request and try once more.
        self.request_group_download2(group_id);
        self.base.fetch_file(group_id, file_id)
    }

    // @ObfuscatedName("ch.m(II[IS)[B") — Js5.fetchFile with XTEA key.
    pub fn fetch_file_with_key(&mut self, group_id: i32, file_id: i32, key: &[i32; 4]) -> Option<Vec<u8>> {
        let first = self.base.fetch_file_with_key(group_id, file_id, Some(key));
        if first.is_some() {
            return first;
        }
        self.request_group_download2(group_id);
        self.base.fetch_file_with_key(group_id, file_id, Some(key))
    }

    // @ObfuscatedName("ch.c(III)Z") — Js5.requestDownload(groupId, fileId)
    pub fn request_download(&mut self, group_id: i32, file_id: i32) -> bool {
        if group_id < 0
            || group_id as usize >= self.base.unpacked.len()
            || self.base.unpacked[group_id as usize].is_none()
        {
            return false;
        }
        let len = self.base.unpacked[group_id as usize].as_ref().unwrap().len();
        if file_id < 0 || file_id as usize >= len {
            return false;
        }
        if self.base.unpacked[group_id as usize].as_ref().unwrap()[file_id as usize].is_some() {
            return true;
        }
        if self.base.packed[group_id as usize].is_some() {
            return true;
        }
        self.request_group_download2(group_id);
        self.base.packed[group_id as usize].is_some()
    }

    // @ObfuscatedName("ch.k(Ljava/lang/String;Ljava/lang/String;B)Z") — name-based
    pub fn request_download_by_name(&mut self, group: &str, file: &str) -> bool {
        let group_id = self.base.get_group_id(group);
        if group_id < 0 {
            return false;
        }
        let file_id = self.base.get_file_id(group_id, file);
        if file_id < 0 {
            return false;
        }
        self.request_download(group_id, file_id)
    }

    // @ObfuscatedName("ch.f(...)[B") — Js5.getFile(name, name)
    pub fn get_file_by_name(&mut self, group: &str, file: &str) -> Option<Vec<u8>> {
        let group_id = self.base.get_group_id(group);
        if group_id < 0 {
            return None;
        }
        let file_id = self.base.get_file_id(group_id, file);
        if file_id < 0 {
            return None;
        }
        self.fetch_file(group_id, file_id)
    }

    // @ObfuscatedName("dq.be(I)I")
    pub fn get_index_load_progress(&self) -> i32 {
        let mut var1 = 0;
        let mut var2 = 0;
        for var3 in 0..self.base.packed.len() {
            if self.base.group_sizes[var3] > 0 {
                var1 += 100;
                var2 += self.get_group_load_progress(var3 as i32);
            }
        }
        if var1 == 0 { 100 } else { var2 * 100 / var1 }
    }
}
