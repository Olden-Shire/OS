//! Skeletal animation data — `AnimBase` (archive 1, base poses / joint metadata) and
//! `AnimFrameSet` (archive 0, per-frame transform deltas relative to a base).
//!
//! Server uses these for animation frame counts + per-frame durations (a SeqType points
//! into a frame set by `frame_id >> 16` for the set and `& 0xFFFF` for the frame). Most
//! of the per-vertex animation math stays on the client.
//!
//! Port of `jagex3.dash3d.{AnimBase, AnimFrame, AnimFrameSet}`.

use std::collections::HashMap;

use io::Packet;

use crate::Cache;

/// Skeletal "base" — joint/group metadata that frames apply transforms against.
#[derive(Debug, Clone)]
pub struct AnimBase {
    pub id: i32,
    /// Per-joint transform type (0 = origin/pivot, 1 = translate, 2 = rotate, 3 = scale,
    /// 5 = transparency animation).
    pub types: Vec<i32>,
    /// `labels[i]` lists the vertex-group ids that joint `i` operates on.
    pub labels: Vec<Vec<i32>>,
}

impl AnimBase {
    pub fn decode(id: i32, bytes: &[u8]) -> Self {
        let mut p = Packet::from_vec(bytes.to_vec());
        let size = p.g1() as usize;
        let mut types = Vec::with_capacity(size);
        for _ in 0..size {
            types.push(p.g1());
        }
        let mut labels: Vec<Vec<i32>> = (0..size).map(|_| Vec::with_capacity(p.g1() as usize)).collect();
        for label_set in labels.iter_mut() {
            let n = label_set.capacity();
            for _ in 0..n {
                label_set.push(p.g1());
            }
        }
        Self { id, types, labels }
    }
}

/// One animation frame: a sparse list of joint transforms `(joint_index, tx, ty, tz)` to
/// apply against the base pose. The Java client splats these onto vertex groups; for the
/// server we just retain the per-joint deltas.
#[derive(Debug, Clone)]
pub struct AnimFrame {
    /// id of the AnimBase this frame transforms.
    pub base_id: i32,
    /// `true` if any joint with `type == 5` (transparency) was animated.
    pub animate_transparencies: bool,
    /// Joint indices (into `AnimBase::types` / `labels`) touched by this frame.
    pub ti: Vec<i32>,
    pub tx: Vec<i32>,
    pub ty: Vec<i32>,
    pub tz: Vec<i32>,
}

impl AnimFrame {
    /// Decode one frame given its raw bytes and the resolved AnimBase. The first two bytes
    /// of `bytes` are the base id (BE u16), which the caller has already resolved.
    pub fn decode(bytes: &[u8], base: &AnimBase) -> Self {
        // Two cursors over the same buffer: mask byte stream + transform value stream.
        let mut masks = Packet::from_vec(bytes.to_vec());
        let mut values = Packet::from_vec(bytes.to_vec());
        masks.pos = 2;
        let mask_count = masks.g1() as usize;
        values.pos = masks.pos + mask_count;

        let mut ti = Vec::new();
        let mut tx = Vec::new();
        let mut ty = Vec::new();
        let mut tz = Vec::new();
        let mut animate_transparencies = false;
        let mut last_explicit: i32 = -1;

        for joint in 0..mask_count {
            let mask = masks.g1();
            if mask <= 0 {
                continue;
            }

            // If this joint isn't a pivot (type != 0), implicitly emit a zero-delta entry
            // for the most recent pivot joint between `last_explicit + 1` and `joint`.
            if base.types[joint] != 0 {
                for back in ((last_explicit + 1) as usize..joint).rev() {
                    if base.types[back] == 0 {
                        ti.push(back as i32);
                        tx.push(0);
                        ty.push(0);
                        tz.push(0);
                        break;
                    }
                }
            }

            ti.push(joint as i32);
            // type 3 (scale) defaults to 128 (identity); others default to 0.
            let default_value = if base.types[joint] == 3 { 128i32 } else { 0i32 };

            tx.push(if mask & 0x1 != 0 { values.gsmarts() } else { default_value });
            ty.push(if mask & 0x2 != 0 { values.gsmarts() } else { default_value });
            tz.push(if mask & 0x4 != 0 { values.gsmarts() } else { default_value });

            last_explicit = joint as i32;
            if base.types[joint] == 5 {
                animate_transparencies = true;
            }
        }

        assert_eq!(
            values.pos,
            bytes.len(),
            "AnimFrame: trailing {} bytes unconsumed",
            bytes.len() - values.pos
        );

        Self { base_id: base.id, animate_transparencies, ti, tx, ty, tz }
    }
}

/// A set of frames sharing the same animation (one group in archive 0). Indexed by file id
/// within the group — gaps are `None`.
#[derive(Debug, Clone)]
pub struct AnimFrameSet {
    pub frames: Vec<Option<AnimFrame>>,
}

impl AnimFrameSet {
    /// Load a frame set from archive 0 group `id`. Each frame file's first two bytes are
    /// the AnimBase id; bases are loaded on demand from archive 1 (one base per group,
    /// file 0). `base_cache` lets callers amortize base loads across many frame sets.
    pub fn load(
        cache: &mut Cache,
        id: u32,
        base_cache: &mut HashMap<i32, AnimBase>,
    ) -> std::io::Result<Self> {
        let frame_files = cache
            .read_files(crate::ANIMS_ARCHIVE, id)?
            .unwrap_or_else(|| panic!("anims archive missing group {id}"));
        let limit = frame_files.iter().map(|(fid, _)| *fid).max().unwrap_or(-1) + 1;
        let mut frames: Vec<Option<AnimFrame>> = (0..limit).map(|_| None).collect();

        for (fid, bytes) in frame_files {
            let base_id = ((bytes[0] as i32) << 8) | (bytes[1] as i32 & 0xFF);
            if !base_cache.contains_key(&base_id) {
                let base_bytes = cache
                    .read_files(crate::BASES_ARCHIVE, base_id as u32)?
                    .unwrap_or_else(|| panic!("bases archive missing group {base_id}"));
                let (_, base_file) = base_bytes
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| panic!("bases group {base_id} has no files"));
                base_cache.insert(base_id, AnimBase::decode(base_id, &base_file));
            }
            let base = &base_cache[&base_id];
            frames[fid as usize] = Some(AnimFrame::decode(&bytes, base));
        }

        Ok(Self { frames })
    }
}
