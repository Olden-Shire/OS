//! `jagex3.sound.WaveCache` — load + cache waves from the JS5 cache.
//!
//! Two backing archives: `jagfx` (4) for synthesized FX, `vorbis` (14) for sampled
//! instruments. Vorbis is decoded via the `lewton` crate — equivalent output to the Java
//! client's hand-rolled `JagVorbis` decoder, which is 3 k+ LoC and not worth porting when
//! a well-tested decoder exists.

use std::collections::HashMap;
use std::sync::Arc;

use cache::Cache;

use crate::jagfx::JagFX;
use crate::jagvorbis::{JagVorbis, VorbisHeaders};
use crate::wave::Wave;

const JAGFX_ARCHIVE: u8 = 4;
const VORBIS_ARCHIVE: u8 = 14;

#[derive(Default)]
pub struct WaveCache {
    waves: HashMap<u64, Arc<Wave>>,
    vorbis_headers: Option<Arc<VorbisHeaders>>,
}

impl WaveCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a JagFX-synthesized wave. `group` and `file` index into archive 4.
    pub fn get_jagfx(&mut self, cache: &mut Cache, group: u32, file: i32) -> Option<Arc<Wave>> {
        let key = wave_key(group, file, false);
        if let Some(w) = self.waves.get(&key) {
            return Some(Arc::clone(w));
        }
        let bytes = read_file(cache, JAGFX_ARCHIVE, group, file)?;
        let wave = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut fx = JagFX::decode(&bytes);
            fx.to_wave()
        }))
        .ok()?;
        let arc = Arc::new(wave);
        self.waves.insert(key, Arc::clone(&arc));
        Some(arc)
    }

    /// Load a Vorbis-encoded sample. The cache's vorbis archive stores raw Vorbis I audio
    /// packets in a small custom container (see [`crate::jagvorbis`]). Headers are shared
    /// across all samples and loaded lazily from group 0 / file 0.
    pub fn get_vorbis(&mut self, cache: &mut Cache, group: u32, file: i32) -> Option<Arc<Wave>> {
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

    fn ensure_vorbis_headers(&mut self, cache: &mut Cache) -> Option<Arc<VorbisHeaders>> {
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

    /// Look up a wave by Jagex's combined "audio id" — used by Patch.load_waves where
    /// `id & 1 == 0` selects JagFX, else Vorbis; id >> 2 is the archive group.
    pub fn get_by_wave_id(&mut self, cache: &mut Cache, wave_id: i32) -> Option<Arc<Wave>> {
        let id_minus_one = wave_id - 1;
        let archive_group = (id_minus_one >> 2) as u32;
        let is_vorbis = (id_minus_one & 0x1) != 0;
        // Jagex's WaveCache.getJagFx(int, int[]) handles two layouts: single-group archives
        // (group=0, file=archive_group) or single-file groups (group=archive_group, file=0).
        // We try both and use whichever has data.
        let loader = if is_vorbis {
            WaveCache::get_vorbis as fn(&mut WaveCache, &mut Cache, u32, i32) -> Option<Arc<Wave>>
        } else {
            WaveCache::get_jagfx
        };
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

fn read_file(cache: &mut Cache, archive: u8, group: u32, file: i32) -> Option<Vec<u8>> {
    if file == 0 {
        // single-file group: return the group payload directly.
        return cache.read_group(archive, group).ok().flatten().filter(|b| !b.is_empty());
    }
    let files = cache.read_files(archive, group).ok().flatten()?;
    files.into_iter().find(|(id, _)| *id == file).map(|(_, b)| b).filter(|b| !b.is_empty())
}

