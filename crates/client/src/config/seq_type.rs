// @ObfuscatedName("eo") — jag::oldscape::configdecoder::SeqType
//
// Animation sequence type. Loaded from config archive group 12.
// Stores the frame indices, per-frame delays, optional iframes,
// sound triggers, walk merge, priority, and various movement /
// duplicate behaviour flags. Decoder + post-decode ported verbatim.
// AnimFrameSet loading + animateModel methods land once dash3d.anim
// is wired.

#![allow(dead_code)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::io::packet::Packet;
use crate::js5::js5_loader::Js5Loader;
use crate::js5::js5_net;

// @ObfuscatedName("dz.n") / "ag.j" / "eo.z"
pub static CONFIG_SLOT: AtomicI32 = AtomicI32::new(-1);
pub static ANIMS_SLOT: AtomicI32 = AtomicI32::new(-1);
pub static BASES_SLOT: AtomicI32 = AtomicI32::new(-1);

#[derive(Debug, Clone, Default)]
pub struct SeqType {
    // @ObfuscatedName("eo.i") / "eo.s"
    pub frames: Option<Vec<i32>>,
    pub iframes: Option<Vec<i32>>,
    // @ObfuscatedName("eo.u")
    pub delay: Option<Vec<i32>>,
    // @ObfuscatedName("eo.v")
    pub sound: Option<Vec<i32>>,
    // @ObfuscatedName("eo.w")
    pub loops: i32,
    // @ObfuscatedName("eo.e")
    pub walkmerge: Option<Vec<i32>>,
    // @ObfuscatedName("eo.b")
    pub reachforward: bool,
    // @ObfuscatedName("eo.y")
    pub priority: i32,
    // @ObfuscatedName("eo.t") / "eo.f"
    pub replaceheldleft: i32,
    pub replaceheldright: i32,
    // @ObfuscatedName("eo.k")
    pub maxloops: i32,
    // @ObfuscatedName("eo.o") / "eo.a"
    pub preanim_move: i32,
    pub postanim_move: i32,
    // @ObfuscatedName("eo.h")
    pub duplicatebehaviour: i32,
}

impl SeqType {
    pub fn new() -> Self {
        Self {
            frames: None, iframes: None, delay: None, sound: None,
            loops: -1,
            walkmerge: None,
            reachforward: false,
            priority: 5,
            replaceheldleft: -1, replaceheldright: -1,
            maxloops: 99,
            preanim_move: -1, postanim_move: -1,
            duplicatebehaviour: 2,
        }
    }

    // @ObfuscatedName("eo.q(Lev;S)V") — SeqType.decode loop
    pub fn decode_all(&mut self, p: &mut Packet) {
        loop {
            let code = p.g1();
            if code == 0 { return; }
            self.decode(p, code);
        }
    }

    // @ObfuscatedName("eo.i(Lev;IB)V") — SeqType.decode(opcode)
    pub fn decode(&mut self, p: &mut Packet, code: i32) {
        match code {
            1 => {
                let n = p.g2() as usize;
                let mut delay = Vec::with_capacity(n);
                for _ in 0..n { delay.push(p.g2()); }
                self.delay = Some(delay);
                let mut frames = Vec::with_capacity(n);
                for _ in 0..n { frames.push(p.g2()); }
                for f in frames.iter_mut() { *f += p.g2() << 16; }
                self.frames = Some(frames);
            }
            2 => { self.loops = p.g2(); }
            3 => {
                let n = p.g1() as usize;
                let mut walk = Vec::with_capacity(n + 1);
                for _ in 0..n { walk.push(p.g1()); }
                walk.push(9_999_999);
                self.walkmerge = Some(walk);
            }
            4 => { self.reachforward = true; }
            5 => { self.priority = p.g1(); }
            6 => { self.replaceheldleft = p.g2(); }
            7 => { self.replaceheldright = p.g2(); }
            8 => { self.maxloops = p.g1(); }
            9 => { self.preanim_move = p.g1(); }
            10 => { self.postanim_move = p.g1(); }
            11 => { self.duplicatebehaviour = p.g1(); }
            12 => {
                let n = p.g1() as usize;
                let mut iframes = Vec::with_capacity(n);
                for _ in 0..n { iframes.push(p.g2()); }
                for f in iframes.iter_mut() { *f += p.g2() << 16; }
                self.iframes = Some(iframes);
            }
            13 => {
                let n = p.g1() as usize;
                let mut sound = Vec::with_capacity(n);
                for _ in 0..n { sound.push(p.g3()); }
                self.sound = Some(sound);
            }
            _ => {}
        }
    }

    // @ObfuscatedName("eo.s(B)V") — SeqType.postDecode
    pub fn post_decode(&mut self) {
        if self.preanim_move == -1 {
            self.preanim_move = if self.walkmerge.is_none() { 0 } else { 2 };
        }
        if self.postanim_move == -1 {
            self.postanim_move = if self.walkmerge.is_none() { 0 } else { 2 };
        }
    }
}

impl SeqType {
    // @ObfuscatedName("eo.u(Lfo;II)Lfo;") — SeqType.animateModel.
    // Verbatim port of SeqType.java:194-204. Clones the model + applies
    // the AnimFrame at frames[arg1].
    pub fn animate_model(
        &self,
        base: &crate::dash3d::model_lit::ModelLit,
        frame_index: i32,
    ) -> crate::dash3d::model_lit::ModelLit {
        let mut copy = base.clone();
        let Some(frames) = self.frames.as_ref() else { return copy; };
        let idx = frame_index as usize;
        if idx >= frames.len() { return copy; }
        let raw = frames[idx];
        let frameset_id = raw >> 16;
        let frame_id = raw & 0xFFFF;
        if let Some(fs) = crate::dash3d::anim_frame_set::get(frameset_id) {
            copy.animate(&fs, frame_id);
        }
        copy
    }

    // @ObfuscatedName("eo.w(Lfo;II)Lfo;") — SeqType.animateModel2. Used
    // exclusively by SpotType.getTempModel2; semantically identical to
    // animateModel since copyForAnim2 only differs in transparency
    // book-keeping which we collapse into the plain clone.
    pub fn animate_model_2(
        &self,
        base: &crate::dash3d::model_lit::ModelLit,
        frame_index: i32,
    ) -> crate::dash3d::model_lit::ModelLit {
        self.animate_model(base, frame_index)
    }

    // @ObfuscatedName("eo.e(Lfo;ILeo;II)Lfo;") — SeqType.splitAnimateModel.
    // Verbatim port of SeqType.java:252-271. Walks the primary frames
    // ref, then layers the secondary frames via ModelLit::mask_animate
    // bounded by `walkmerge` (the upper-/lower-body split mask).
    pub fn split_animate_model(
        &self,
        base: &crate::dash3d::model_lit::ModelLit,
        primary_frame: i32,
        secondary: &SeqType,
        secondary_frame: i32,
    ) -> crate::dash3d::model_lit::ModelLit {
        let Some(primary_frames) = self.frames.as_ref() else {
            return secondary.animate_model(base, secondary_frame);
        };
        let p_idx = primary_frame as usize;
        if p_idx >= primary_frames.len() {
            return secondary.animate_model(base, secondary_frame);
        }
        let p_raw = primary_frames[p_idx];
        let Some(primary_fs) = crate::dash3d::anim_frame_set::get(p_raw >> 16) else {
            return secondary.animate_model(base, secondary_frame);
        };
        let p_frame_id = p_raw & 0xFFFF;

        let Some(secondary_frames) = secondary.frames.as_ref() else {
            let mut copy = base.clone();
            copy.animate(&primary_fs, p_frame_id);
            return copy;
        };
        let s_idx = secondary_frame as usize;
        if s_idx >= secondary_frames.len() {
            let mut copy = base.clone();
            copy.animate(&primary_fs, p_frame_id);
            return copy;
        }
        let s_raw = secondary_frames[s_idx];
        let Some(secondary_fs) = crate::dash3d::anim_frame_set::get(s_raw >> 16) else {
            let mut copy = base.clone();
            copy.animate(&primary_fs, p_frame_id);
            return copy;
        };
        let s_frame_id = s_raw & 0xFFFF;

        let mut copy = base.clone();
        let walkmerge_owned;
        let walkmerge_slice = match self.walkmerge.as_ref() {
            Some(v) => v.as_slice(),
            None => {
                walkmerge_owned = Vec::<i32>::new();
                &walkmerge_owned[..]
            }
        };
        copy.mask_animate(&primary_fs, p_frame_id,
                          &secondary_fs, s_frame_id,
                          Some(walkmerge_slice));
        copy
    }

    // @ObfuscatedName("eo.b(Lfo;IB)Lfo;") — SeqType.animateModelWithExtra.
    // Verbatim port of SeqType.java:275-298. Applies primary then
    // (if iframes are set) secondary animations sequentially on the
    // same model.
    pub fn animate_model_with_extra(
        &self,
        base: &crate::dash3d::model_lit::ModelLit,
        frame_index: i32,
    ) -> crate::dash3d::model_lit::ModelLit {
        let mut copy = self.animate_model(base, frame_index);
        let Some(iframes) = self.iframes.as_ref() else { return copy; };
        let idx = frame_index as usize;
        if idx >= iframes.len() { return copy; }
        let raw = iframes[idx];
        let frame_id = raw & 0xFFFF;
        if frame_id == 0xFFFF { return copy; }
        if let Some(fs) = crate::dash3d::anim_frame_set::get(raw >> 16) {
            copy.animate(&fs, frame_id);
        }
        copy
    }

    // Pure accessor — safe-indexed delay lookup. Returns the per-frame
    // tick count or 0 when the frame is OOB / delay is None. Java reads
    // self.delay[index] directly throughout the animation update path;
    // we hoist the bounds check.
    pub fn frame_duration(&self, index: i32) -> i32 {
        let Some(delay) = self.delay.as_ref() else { return 0; };
        if index < 0 { return 0; }
        delay.get(index as usize).copied().unwrap_or(0)
    }

    // Pure accessor — last valid frame index. Java open-codes
    // `self.frames.length - 1` at every wrap-check site; hoisting here
    // gives us a single bounds-safe expression.
    pub fn last_frame_index(&self) -> i32 {
        self.frames.as_ref().map(|v| v.len() as i32 - 1).unwrap_or(-1)
    }
}

pub struct SeqStore {
    pub map: std::collections::HashMap<i32, SeqType>,
}
pub static STORE: std::sync::LazyLock<Mutex<SeqStore>> =
    std::sync::LazyLock::new(|| Mutex::new(SeqStore { map: std::collections::HashMap::new() }));

pub fn reset_cache() {
    STORE.lock().unwrap().map.clear();
}

// @ObfuscatedName("i.g(IB)Leo;") — SeqType.list(id)
pub fn list(id: i32) -> SeqType {
    {
        let s = STORE.lock().unwrap();
        if let Some(t) = s.map.get(&id) { return t.clone(); }
    }
    let bytes_opt = {
        let config_slot = CONFIG_SLOT.load(Ordering::Relaxed);
        if config_slot < 0 { None }
        else {
            let mut reg = js5_net::LOADERS.lock().unwrap();
            reg.get_mut(config_slot as usize)
                .and_then(|o| o.as_mut())
                .and_then(|l| l.fetch_file(12, id))
        }
    };
    // Same caching trap as FluType / FloType / texture_manager: don't
    // poison the cache with a default-zero SeqType when the config
    // archive hasn't streamed in. Retry next call.
    let Some(bytes) = bytes_opt else {
        let mut t = SeqType::new();
        t.post_decode();
        return t;
    };
    let mut t = SeqType::new();
    let mut p = Packet::from_vec(bytes);
    t.decode_all(&mut p);
    t.post_decode();
    let cloned = t.clone();
    STORE.lock().unwrap().map.insert(id, t);
    cloned
}

pub fn init(_archive: &Js5Loader) {}

impl SeqType {
    // @ObfuscatedName("eo.v(Lfo;IIB)Lfo;") — SeqType.animateModel90.
    // Verbatim port of SeqType.java:209-234. Rotates the model by
    // `rotation & 3` quarter-turns before animating, then rotates
    // back — used for loc anims whose meshes are baked at world
    // orientation 0. The existing `animate_model` (line 138 above)
    // handles the non-rotated path.
    pub fn animate_model_90(
        &self,
        src: &crate::dash3d::model_lit::ModelLit,
        seq_frame: i32,
        rotation: i32,
    ) -> crate::dash3d::model_lit::ModelLit {
        let mut copy = src.clone();
        let Some(frames) = self.frames.as_ref() else { return copy; };
        let Some(&frame_word) = frames.get(seq_frame as usize) else { return copy; };
        let fs_id = (frame_word >> 16) & 0xFFFF;
        let frame_idx = frame_word & 0xFFFF;
        let Some(fs) = crate::dash3d::anim_frame_set::get(fs_id) else { return copy; };
        let r = rotation & 0x3;
        match r {
            1 => copy.rotate270(),
            2 => copy.rotate180(),
            3 => copy.rotate90(),
            _ => {}
        }
        copy.animate(&fs, frame_idx);
        match r {
            1 => copy.rotate90(),
            2 => copy.rotate180(),
            3 => copy.rotate270(),
            _ => {}
        }
        copy
    }
}

pub fn install_archives(config_slot: i32, anims_slot: i32, bases_slot: i32) {
    CONFIG_SLOT.store(config_slot, Ordering::Relaxed);
    ANIMS_SLOT.store(anims_slot, Ordering::Relaxed);
    BASES_SLOT.store(bases_slot, Ordering::Relaxed);
}
