// @ObfuscatedName("a")
// jag::oldscape::sound::WaveCache
//
// Loads + caches Waves. JagFX (archive 4) for synthesised FX, JagVorbis
// (archive 14) for sampled instruments. Both decoders are verbatim ports
// of jagex3.sound.{JagFX,JagVorbis}. The vorbis archive's group 0 / file 0
// holds a shared Vorbis I header used by every sample.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use crate::sound::jagvorbis::{JagVorbis, VorbisHeaders};
use crate::sound::js5_cache::Js5Cache;
use crate::sound::wave::Wave;

const JAGFX_ARCHIVE: u8 = 4;
const VORBIS_ARCHIVE: u8 = 14;

#[derive(Default)]
pub struct WaveCache {
    // @ObfuscatedName("a.m") — cached decoded waves keyed by archive+group+file.
    waves: HashMap<u64, Arc<Wave>>,
    // Shared codebooks/floors/etc. for the vorbis archive. Loaded once
    // from archive 14 group 0 / file 0.
    vorbis_headers: Option<Arc<VorbisHeaders>>,
}

impl WaveCache {
    pub fn new() -> Self { Self::default() }

    // @ObfuscatedName("a.r(II[II)Leq;") — WaveCache.getJagFx
    pub fn get_jagfx(&mut self, cache: &mut Js5Cache, group: u32, file: i32) -> Option<Arc<Wave>> {
        let key = wave_key(group, file, false);
        if let Some(w) = self.waves.get(&key) {
            return Some(Arc::clone(w));
        }
        let bytes = read_file(cache, JAGFX_ARCHIVE, group, file)?;
        let wave = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut fx = crate::sound::jagfx::JagFX::decode(&bytes);
            fx.to_wave()
        }))
        .ok()?;
        let arc = Arc::new(wave);
        self.waves.insert(key, Arc::clone(&arc));
        Some(arc)
    }

    // @ObfuscatedName("a.d(II[II)Leq;") — WaveCache.getJagVorbis
    pub fn get_vorbis(&mut self, cache: &mut Js5Cache, group: u32, file: i32) -> Option<Arc<Wave>> {
        let key = wave_key(group, file, true);
        if let Some(w) = self.waves.get(&key) {
            return Some(Arc::clone(w));
        }
        let headers = self.ensure_vorbis_headers(cache)?;
        let bytes = read_file(cache, VORBIS_ARCHIVE, group, file)?;
        let wave = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            JagVorbis::decode(&bytes).to_wave(&headers)
        }))
        .ok()?;
        let arc = Arc::new(wave);
        self.waves.insert(key, Arc::clone(&arc));
        Some(arc)
    }

    fn ensure_vorbis_headers(&mut self, cache: &mut Js5Cache) -> Option<Arc<VorbisHeaders>> {
        if let Some(h) = &self.vorbis_headers {
            return Some(Arc::clone(h));
        }
        let bytes = read_file(cache, VORBIS_ARCHIVE, 0, 0)?;
        let parsed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            VorbisHeaders::parse(&bytes)
        }))
        .ok()?;
        let arc = Arc::new(parsed);
        self.vorbis_headers = Some(Arc::clone(&arc));
        Some(arc)
    }

    pub fn get_by_wave_id(&mut self, cache: &mut Js5Cache, wave_id: i32) -> Option<Arc<Wave>> {
        let id_minus_one = wave_id - 1;
        let archive_group = (id_minus_one >> 2) as u32;
        let is_vorbis = (id_minus_one & 0x1) != 0;
        let loader: fn(&mut WaveCache, &mut Js5Cache, u32, i32) -> Option<Arc<Wave>> =
            if is_vorbis { WaveCache::get_vorbis } else { WaveCache::get_jagfx };
        loader(self, cache, archive_group, 0)
            .or_else(|| loader(self, cache, 0, archive_group as i32))
    }
}

fn wave_key(group: u32, file: i32, vorbis: bool) -> u64 {
    let g = group as u64;
    let f = file as u32 as u64;
    let base = (g << 16) | f;
    if vorbis { base | (1 << 32) } else { base }
}

fn read_file(cache: &mut Js5Cache, archive: u8, group: u32, file: i32) -> Option<Vec<u8>> {
    if file == 0 {
        return cache.read_group(archive, group).ok().flatten().filter(|b| !b.is_empty());
    }
    let files = cache.read_files(archive, group).ok().flatten()?;
    files.into_iter().find(|(id, _)| *id == file as u32).map(|(_, b)| b).filter(|b| !b.is_empty())
}
