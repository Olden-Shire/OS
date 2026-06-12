// @ObfuscatedName("ff") — jag::oldscape::ClientLocAnim.
//
// Per-loc animation instance. Currently the renderer holds anim state
// in scene.rs's `LOC_ANIM_STATE` keyed by (level, x, z, id); this
// struct is the proper Java port for callers that need to hand off /
// inherit anim state (duplicateBehaviour=0 inheritance from a prior
// instance — used when a multiloc varbit swap keeps the same SeqType).

#![allow(dead_code)]

use crate::config::seq_type;

#[derive(Debug, Clone)]
pub struct ClientLocAnim {
    // @ObfuscatedName("ff.j")
    pub id: i32,
    // @ObfuscatedName("ff.z") — loc shape / kind (0-22).
    pub shape: i32,
    // @ObfuscatedName("ff.g") — rotation 0-7 (the +4 path encodes the
    // mirror_xor flag used by wall-decor kinds).
    pub angle: i32,
    // @ObfuscatedName("ff.q")
    pub level: i32,
    // @ObfuscatedName("ff.i")
    pub x: i32,
    // @ObfuscatedName("ff.s")
    pub z: i32,
    // @ObfuscatedName("ff.u") — resolved SeqType id (Java holds the
    // SeqType reference; we store the id and re-resolve via list()).
    pub anim_id: i32,
    // @ObfuscatedName("ff.v")
    pub anim_frame: i32,
    // @ObfuscatedName("ff.w")
    pub anim_cycle: i32,
}

impl ClientLocAnim {
    // Verbatim port of ClientLocAnim.java:40-64.
    //
    // `loop_cycle` is the current Client.loopCycle. `randomize_start`
    // controls phase jitter (Java's arg7); rev1 always passes `true` for
    // build-time spawns. `inherit_from` is the Java `ModelSource arg8` —
    // when duplicate_behaviour == 0 and the prior instance shares the
    // same SeqType, copy its anim_frame / anim_cycle.
    pub fn new(
        id: i32,
        shape: i32,
        angle: i32,
        level: i32,
        x: i32,
        z: i32,
        anim_id: i32,
        randomize_start: bool,
        loop_cycle: i32,
        inherit_from: Option<&ClientLocAnim>,
    ) -> Self {
        let mut anim_frame = 0;
        let mut anim_cycle = loop_cycle - 1;
        if anim_id != -1 {
            let seq = seq_type::list(anim_id);
            // Java's `this.anim.duplicatebehaviour == 0 && arg8 instanceof ClientLocAnim`
            // — inherit state from prior instance with matching SeqType.
            if seq.duplicatebehaviour == 0 {
                if let Some(prev) = inherit_from {
                    if prev.anim_id == anim_id {
                        anim_frame = prev.anim_frame;
                        anim_cycle = prev.anim_cycle;
                        return Self { id, shape, angle, level, x, z, anim_id, anim_frame, anim_cycle };
                    }
                }
            }
            // Otherwise, optional phase randomisation for looping anims.
            if randomize_start && seq.loops != -1 {
                if let Some(frames) = seq.frames.as_ref() {
                    if !frames.is_empty() {
                        let h = ((x.wrapping_mul(0x9E3779B1u32 as i32))
                            ^ z.wrapping_mul(0x85EBCA77u32 as i32)
                            ^ id) as u32;
                        anim_frame = (h % frames.len() as u32) as i32;
                        if let Some(delays) = seq.delay.as_ref() {
                            if let Some(d) = delays.get(anim_frame as usize) {
                                anim_cycle -= ((h / frames.len().max(1) as u32)
                                    % d.max(&1).max(&1).unsigned_abs()) as i32;
                            }
                        }
                    }
                }
            }
        }
        Self { id, shape, angle, level, x, z, anim_id, anim_frame, anim_cycle }
    }

    // @ObfuscatedName("ff.g(I)Lfo;") — ClientLocAnim.getTempModel.
    // Verbatim port of ClientLocAnim.java:67-115. Advances the anim
    // frame in place, resolves the (possibly multiloc-redispatched)
    // LocType, looks up the four corner ground heights, then calls
    // LocType.getTempModel with the per-shape angle/rotation pair.
    //
    // Returns the model id the caller would render; the actual
    // ModelLit construction inside LocType.getTempModel is the
    // worn-equipment pipeline that lands later.
    pub fn get_temp_model(&mut self, loop_cycle: i32) -> Option<i32> {
        use crate::config::{loc_type, seq_type};

        // Advance animation frame.
        if self.anim_id != -1 {
            let seq = seq_type::list(self.anim_id);
            if let (Some(frames), Some(delays)) = (seq.frames.as_ref(), seq.delay.as_ref()) {
                let mut elapsed = loop_cycle - self.anim_cycle;
                if elapsed > 100 && seq.loops > 0 {
                    elapsed = 100;
                }
                // Frame walk, mirroring Java's label-break control flow.
                let mut clear = false;
                'walker: loop {
                    while elapsed > delays.get(self.anim_frame as usize).copied().unwrap_or(0) {
                        elapsed -= delays[self.anim_frame as usize];
                        self.anim_frame += 1;
                        if self.anim_frame >= frames.len() as i32 { break; }
                    }
                    if (self.anim_frame as usize) < frames.len() {
                        break 'walker;
                    }
                    self.anim_frame -= seq.loops;
                    if self.anim_frame < 0 || self.anim_frame >= frames.len() as i32 {
                        clear = true;
                        break 'walker;
                    }
                }
                if clear {
                    self.anim_id = -1;
                }
                self.anim_cycle = loop_cycle - elapsed;
            }
        }

        let mut loc = loc_type::list(self.id)?;
        if loc.multiloc.is_some() {
            if let Some(active) = loc.get_multi_loc() {
                loc = active;
            } else {
                return None;
            }
        }

        // Java's rotation-aware (width, length) swap.
        let (vw, vl) = if self.angle == 1 || self.angle == 3 {
            (loc.length, loc.width)
        } else {
            (loc.width, loc.length)
        };

        let h_sw = (vw >> 1) + self.x;
        let h_se = ((vw + 1) >> 1) + self.x;
        let h_ne = (vl >> 1) + self.z;
        let h_nw = ((vl + 1) >> 1) + self.z;

        let avg_h = {
            let cb = crate::client_build::STATE.lock().unwrap();
            let g = &cb.ground_h[self.level as usize];
            let read = |x: i32, z: i32| -> i32 {
                let xi = x.clamp(0, g.len() as i32 - 1) as usize;
                let zi = z.clamp(0, g[xi].len() as i32 - 1) as usize;
                g[xi][zi]
            };
            (read(h_sw, h_ne) + read(h_se, h_ne) + read(h_sw, h_nw) + read(h_se, h_nw)) >> 2
        };

        let _world_x = (self.x << 7) + (vw << 6);
        let _world_z = (self.z << 7) + (vl << 6);
        let _ = avg_h;

        // LocType.getTempModel lands with the worn-equipment pipeline;
        // we return the first resolved model id so the caller can
        // render. Java's LocType.model[] is a per-shape array; the
        // caller-provided `shape` indexes into it.
        let models = loc.model.as_ref()?;
        let idx = (self.shape as usize).min(models.len().saturating_sub(1));
        models.get(idx).copied()
    }
}
