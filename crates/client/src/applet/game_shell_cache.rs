// @ObfuscatedName("ay") — jag::oldscape::client::applet::GameShellCache.
//
// Disk cache layout helper. Java's class:
//   - locates the cache dir via CacheUtil
//   - opens / creates `main_file_cache.idx`, `main_file_cache.idx255`,
//     and `main_file_cache.dat2` via FileOnDisk
//   - manages the UID192 (random-ish player identity stored in a tiny
//     file used by the anti-bot fingerprint)

#![allow(dead_code)]

use std::path::PathBuf;

pub struct GameShellCache {
    pub cache_dir: PathBuf,
    pub uid192: Vec<u8>,
}

impl Default for GameShellCache {
    fn default() -> Self {
        Self {
            cache_dir: crate::io::cache_util::get_cache_dir(),
            uid192: Vec::new(),
        }
    }
}

impl GameShellCache {
    // @ObfuscatedName("ay.r(I)V") — GameShellCache.openUID. Reads
    // the cache_dir/random.dat file if present.
    pub fn open_uid(&mut self) {
        let p = self.cache_dir.join("random.dat");
        self.uid192 = std::fs::read(&p).unwrap_or_default();
    }

    // @ObfuscatedName("ay.s([BI)V") — GameShellCache.pushUID192.
    pub fn push_uid192(&mut self, bytes: Vec<u8>) {
        let p = self.cache_dir.join("random.dat");
        let _ = std::fs::write(&p, &bytes);
        self.uid192 = bytes;
    }
}
