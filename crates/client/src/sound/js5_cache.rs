// custom — Js5Loader-backed shim that exposes the read_group / read_files
// surface the synth code expects. Replaces the `cache` crate's on-disk
// Cache so the music path runs entirely through Js5 streaming.

#![allow(dead_code)]

use crate::js5::js5_net;

pub struct Js5Cache;

impl Js5Cache {
    pub fn new() -> Self {
        Self
    }

    // Mirrors `cache::Cache::read_group(archive, group_id) -> Option<Vec<u8>>`.
    // archive 4 = jagFX, archive 14 = vorbis, archive 15 = patches.
    pub fn read_group(&mut self, archive: u8, group_id: u32) -> std::io::Result<Option<Vec<u8>>> {
        let slot = archive_to_loader_slot(archive);
        let mut reg = js5_net::LOADERS.lock().unwrap();
        let Some(loader) = reg.get_mut(slot as usize).and_then(|o| o.as_mut()) else {
            return Ok(None);
        };
        // Each group has one or more files. For single-file groups the
        // first file IS the group payload (file id 0).
        Ok(loader.fetch_file(group_id as i32, 0))
    }

    // Mirrors `cache::Cache::read_files(archive, group_id) -> Option<Vec<(u32, Vec<u8>)>>`.
    pub fn read_files(&mut self, archive: u8, group_id: u32) -> std::io::Result<Option<Vec<(u32, Vec<u8>)>>> {
        let slot = archive_to_loader_slot(archive);
        let mut reg = js5_net::LOADERS.lock().unwrap();
        let Some(loader) = reg.get_mut(slot as usize).and_then(|o| o.as_mut()) else {
            return Ok(None);
        };
        let group = group_id as i32;
        if group < 0 || group as usize >= loader.base.unpacked.len() {
            return Ok(None);
        }
        let Some(ids) = loader.base.file_ids.get(group as usize).cloned().flatten() else {
            return Ok(None);
        };
        let mut out: Vec<(u32, Vec<u8>)> = Vec::with_capacity(ids.len());
        for fid in ids {
            if let Some(bytes) = loader.fetch_file(group, fid) {
                out.push((fid as u32, bytes));
            }
        }
        Ok(Some(out))
    }
}

fn archive_to_loader_slot(archive: u8) -> i32 {
    // openJs5 ran with the same archive→slot mapping (anims=0, bases=1,
    // config=2, interfaces=3, jagFX=4, maps=5, songs=6, models=7,
    // sprites=8, textures=9, binary=10, jingles=11, scripts=12,
    // fontMetrics=13, vorbis=14, patches=15). Slots match archives.
    archive as i32
}
