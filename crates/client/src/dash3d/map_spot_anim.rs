// @ObfuscatedName("fn") — jag::oldscape::MapSpotAnim.
//
// Map-pinned spot animation — a SpotType animation anchored to a
// specific (x, y, z) world tile. Used for things like teleport glow,
// chest sparkles, herblore bubbles. The MAP_PROJANIM / MAP_ANIM
// packets spawn these into the per-level MapSpotAnim list.
//
// Verbatim port of MapSpotAnim.java.

#![allow(dead_code)]

#[derive(Debug, Clone)]
pub struct MapSpotAnim {
    // @ObfuscatedName("fn.j") — SpotType id.
    pub type_: i32,
    // @ObfuscatedName("fn.z") — world-space Y (height).
    pub y: i32,
    // @ObfuscatedName("fn.g") — loop_cycle + delay (when the anim is
    // first allowed to draw).
    pub start_cycle: i32,
    // @ObfuscatedName("fn.q")
    pub level: i32,
    // @ObfuscatedName("fn.i") — world-space X.
    pub x: i32,
    // @ObfuscatedName("fn.s") — world-space Z.
    pub z: i32,
    // @ObfuscatedName("fn.u") — resolved SeqType id (Java holds the
    // SeqType reference; we store the id and re-resolve via list()).
    pub anim_id: i32,
    // @ObfuscatedName("fn.v")
    pub anim_frame: i32,
    // @ObfuscatedName("fn.w")
    pub anim_cycle: i32,
    // @ObfuscatedName("fn.e") — flips to true when the SeqType has
    // played its last frame; the renderer then drops this entry.
    pub anim_complete: bool,
}

impl MapSpotAnim {
    // Verbatim port of MapSpotAnim.java:41-55.
    pub fn new(type_: i32, level: i32, x: i32, z: i32, y: i32, loop_cycle: i32, delay: i32) -> Self {
        use crate::config::spot_type;
        let spot_anim = spot_type::list(type_).anim;
        let mut out = Self {
            type_, level, x, z, y,
            start_cycle: loop_cycle + delay,
            anim_id: spot_anim,
            anim_frame: 0,
            anim_cycle: 0,
            anim_complete: spot_anim == -1,
        };
        if spot_anim != -1 {
            // Java resolves SeqType.list immediately; we lazily fetch
            // in do_anim so the SeqType cache populates on demand.
            let _ = out.anim_id;
        }
        out
    }

    // @ObfuscatedName("fn.b(II)V") — MapSpotAnim.doAnim. Verbatim port
    // of MapSpotAnim.java:59-72. `delta` is the elapsed ticks since
    // the last poll (Java's arg0).
    pub fn do_anim(&mut self, delta: i32) {
        if self.anim_complete || self.anim_id == -1 { return; }
        use crate::config::seq_type;
        let seq = seq_type::list(self.anim_id);
        let (Some(frames), Some(delays)) = (seq.frames.as_ref(), seq.delay.as_ref()) else {
            self.anim_complete = true;
            return;
        };
        self.anim_cycle += delta;
        while self.anim_cycle > delays.get(self.anim_frame as usize).copied().unwrap_or(0) {
            self.anim_cycle -= delays[self.anim_frame as usize];
            self.anim_frame += 1;
            if self.anim_frame >= frames.len() as i32 {
                self.anim_complete = true;
                return;
            }
        }
    }

    // @ObfuscatedName("fn.g(I)Lfo;") — MapSpotAnim.getTempModel.
    // Verbatim port of MapSpotAnim.java:76-85. Resolves SpotType's
    // frame model: -1 frame after completion, else `anim_frame`.
    pub fn get_temp_model(&self) -> Option<crate::dash3d::model_lit::ModelLit> {
        use crate::config::spot_type;
        let spot = spot_type::list(self.type_);
        if self.anim_complete {
            spot.get_temp_model2(-1)
        } else {
            spot.get_temp_model2(self.anim_frame)
        }
    }
}
