// @ObfuscatedName("am") — jag::oldscape::io::CacheUtil.
//
// Cache directory locator. Java walks a list of plausible OS paths
// (`%USERPROFILE%/jagexcache`, `~/.jagex_cache_32`, etc.) and probes
// each by trying to open a sentinel file. We use the platform's home
// dir + a canonical "jagexcache" subdir.

#![allow(dead_code)]

use std::path::PathBuf;

// @ObfuscatedName("am.r(I)Ljava/lang/String;") — CacheUtil.getCacheDir.
pub fn get_cache_dir() -> PathBuf {
    let mut path = if let Ok(p) = std::env::var("JAGEX_CACHE_DIR") {
        PathBuf::from(p)
    } else if let Some(home) = dirs_home() {
        home.join(".jagex_cache")
    } else {
        PathBuf::from(".jagex_cache")
    };
    path.push("oldschool");
    let _ = std::fs::create_dir_all(&path);
    path
}

fn dirs_home() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("HOME") {
        return Some(PathBuf::from(home));
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        return Some(PathBuf::from(profile));
    }
    None
}
