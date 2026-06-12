// @ObfuscatedName("fr") — jag::oldscape::dash3d::AnimFrameSet
//
// Container of AnimFrames keyed by file id. Each animation sequence
// (SeqType) references frames as (frameset_id << 16) | frame_id pairs,
// so we cache the AnimFrameSet by frameset_id and the AnimFrames by
// frame_id within it.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::sync::atomic::{AtomicI32, Ordering};

use crate::dash3d::anim_base::AnimBase;
use crate::dash3d::anim_frame::AnimFrame;
use crate::js5::js5_net;

pub struct AnimFrameSet {
    pub list: HashMap<i32, Arc<AnimFrame>>,
}

// AnimBase shares across all frames in a frameset.
static BASE_CACHE: LazyLock<Mutex<HashMap<i32, Arc<AnimBase>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static FRAMESET_CACHE: LazyLock<Mutex<HashMap<i32, Arc<AnimFrameSet>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

// JS5 archive slots — populated at install_archives time.
pub static ANIMS_SLOT: AtomicI32 = AtomicI32::new(-1);
pub static BASES_SLOT: AtomicI32 = AtomicI32::new(-1);

pub fn install_archives(anims_slot: i32, bases_slot: i32) {
    ANIMS_SLOT.store(anims_slot, Ordering::Relaxed);
    BASES_SLOT.store(bases_slot, Ordering::Relaxed);
}

// custom — (base count, base bytes, frame count, frame bytes) estimate for
// the ::mem report; the cache statics are private to this module.
pub fn cache_stats() -> (usize, usize, usize, usize) {
    let bases = BASE_CACHE.lock().unwrap();
    let base_b: usize = bases.values()
        .map(|b| b.labels.iter().map(|l| l.len() * 4 + 24).sum::<usize>())
        .sum();
    let framesets = FRAMESET_CACHE.lock().unwrap();
    let mut frames = 0usize;
    let mut frame_b = 0usize;
    for fs in framesets.values() {
        for f in fs.list.values() {
            frames += 1;
            frame_b += (f.ti.len() + f.tx.len() + f.ty.len() + f.tz.len()) * 4 + 96;
        }
    }
    (bases.len(), base_b, frames, frame_b)
}

fn fetch_base(id: i32) -> Option<Arc<AnimBase>> {
    {
        let c = BASE_CACHE.lock().unwrap();
        if let Some(b) = c.get(&id) { return Some(Arc::clone(b)); }
    }
    let bases_slot = BASES_SLOT.load(Ordering::Relaxed);
    if bases_slot < 0 { return None; }
    let bytes = {
        let mut reg = js5_net::LOADERS.lock().unwrap();
        let loader = reg.get_mut(bases_slot as usize).and_then(|o| o.as_mut())?;
        loader.fetch_file(id, 0)?
    };
    let base = Arc::new(AnimBase::decode(id, &bytes));
    BASE_CACHE.lock().unwrap().insert(id, Arc::clone(&base));
    Some(base)
}

// @ObfuscatedName("fr") ctor — load a whole AnimFrameSet by id from
// the anims archive. Each file in the archive is one keyframe.
pub fn get(id: i32) -> Option<Arc<AnimFrameSet>> {
    {
        let c = FRAMESET_CACHE.lock().unwrap();
        if let Some(fs) = c.get(&id) { return Some(Arc::clone(fs)); }
    }
    let anims_slot = ANIMS_SLOT.load(Ordering::Relaxed);
    if anims_slot < 0 { return None; }
    // Pull the file id list straight out of Js5.file_ids — Java does
    // the same via Js5.getFileList(groupId).
    let file_ids: Vec<i32> = {
        let mut reg = js5_net::LOADERS.lock().unwrap();
        let loader = reg.get_mut(anims_slot as usize).and_then(|o| o.as_mut())?;
        loader.base.file_ids
            .get(id as usize)
            .and_then(|opt| opt.clone())
            .unwrap_or_default()
    };
    if file_ids.is_empty() { return None; }
    let mut list = HashMap::new();
    for f in file_ids {
        let bytes_opt = {
            let mut reg = js5_net::LOADERS.lock().unwrap();
            let loader = reg.get_mut(anims_slot as usize).and_then(|o| o.as_mut())?;
            loader.fetch_file(id, f)
        };
        if let Some(bytes) = bytes_opt {
            if bytes.len() < 2 { continue; }
            // First 2 bytes hold the base id (big-endian short).
            let base_id = ((bytes[0] as i32 & 0xFF) << 8) | (bytes[1] as i32 & 0xFF);
            if let Some(base) = fetch_base(base_id) {
                if let Some(frame) = AnimFrame::decode(&bytes, base) {
                    list.insert(f, Arc::new(frame));
                }
            }
        }
    }
    // If nothing decoded, the anims/bases groups haven't streamed in yet.
    // Do NOT cache an empty set — fetch_file self-queued the download, so
    // the next frame retries and picks them up once they land. Caching an
    // empty frameset here permanently strands the animation at frame 0.
    if list.is_empty() {
        return None;
    }
    let fs = Arc::new(AnimFrameSet { list });
    FRAMESET_CACHE.lock().unwrap().insert(id, Arc::clone(&fs));
    Some(fs)
}

// @ObfuscatedName("fr.z(IB)Z") — AnimFrameSet.getAnimateTransparencies
pub fn get_animate_transparencies(fs: &AnimFrameSet, frame: i32) -> bool {
    fs.list.get(&frame).map_or(false, |f| f.animate_transparencies)
}
