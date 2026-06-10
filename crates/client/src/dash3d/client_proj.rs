// @ObfuscatedName("fh") — jag::oldscape::ClientProj. Projectile entity
// (arrow, spell, throwable). Tracks SpotType anim + cubic-arc motion
// between (srcX, srcZ, h1) and a moving target reached by t2.

#![allow(dead_code)]

#[derive(Debug, Clone)]
pub struct ClientProj {
    // @ObfuscatedName("fh.j")
    pub spotanim: i32,
    // @ObfuscatedName("fh.z")
    pub level: i32,
    // @ObfuscatedName("fh.g")
    pub src_x: i32,
    // @ObfuscatedName("fh.q")
    pub src_z: i32,
    // @ObfuscatedName("fh.i")
    pub h1: i32,
    // @ObfuscatedName("fh.s")
    pub h2: i32,
    // @ObfuscatedName("fh.u")
    pub t1: i32,
    // @ObfuscatedName("fh.v")
    pub t2: i32,
    // @ObfuscatedName("fh.w") — initial pitch angle.
    pub angle: i32,
    // @ObfuscatedName("fh.e") — start position along (src→target).
    pub startpos: i32,
    // @ObfuscatedName("fh.b") — target entity id (negative for npc,
    // positive for player; see Client.targetId conventions).
    pub target: i32,
    // @ObfuscatedName("fh.y") — once `move` has run, the projectile is
    // mobile and setTarget no longer re-anchors x/z/y.
    pub mobile: bool,

    // @ObfuscatedName("fh.t") / "fh.f" / "fh.k" — interpolated world
    // position. Double precision matches Java's float-physics.
    pub x: f64,
    pub z: f64,
    pub y: f64,
    // @ObfuscatedName("fh.o") / "fh.a"
    pub velocity_x: f64,
    pub velocity_z: f64,
    // @ObfuscatedName("fh.h")
    pub velocity: f64,
    // @ObfuscatedName("fh.x")
    pub velocity_y: f64,
    // @ObfuscatedName("fh.p")
    pub acceleration_y: f64,
    // @ObfuscatedName("fh.ad")
    pub yaw: i32,
    // @ObfuscatedName("fh.ac")
    pub pitch: i32,
    // @ObfuscatedName("fh.aa") — resolved SeqType id from
    // SpotType.list(spotanim).anim. -1 = no animation.
    pub anim: i32,
    // @ObfuscatedName("fh.as")
    pub anim_frame: i32,
    // @ObfuscatedName("fh.am")
    pub anim_cycle: i32,
}

impl ClientProj {
    // Verbatim port of ClientProj.java:86-105 — looks up SpotType.anim
    // to seed the SeqType reference.
    pub fn new(
        spotanim: i32,
        level: i32,
        src_x: i32,
        src_z: i32,
        h1: i32,
        t1: i32,
        t2: i32,
        angle: i32,
        startpos: i32,
        target: i32,
        h2: i32,
    ) -> Self {
        let anim = crate::config::spot_type::list(spotanim).anim;
        Self {
            spotanim, level, src_x, src_z, h1, h2, t1, t2,
            angle, startpos, target,
            mobile: false,
            x: 0.0, z: 0.0, y: 0.0,
            velocity_x: 0.0, velocity_z: 0.0, velocity: 0.0,
            velocity_y: 0.0, acceleration_y: 0.0,
            yaw: 0, pitch: 0,
            anim, anim_frame: 0, anim_cycle: 0,
        }
    }

    // @ObfuscatedName("fh.b(IIIII)V") — ClientProj.setTarget. Verbatim
    // port of lines 109-126. First-call (mobile=false) snaps the
    // projectile to its startpos along the src→target vector, then
    // computes constant velocity to reach the target by t2.
    pub fn set_target(&mut self, target_x: i32, target_z: i32, target_y: i32, cycle: i32) {
        if !self.mobile {
            let dx = (target_x - self.src_x) as f64;
            let dz = (target_z - self.src_z) as f64;
            let dist = (dx * dx + dz * dz).sqrt();
            self.x = (self.startpos as f64) * dx / dist + self.src_x as f64;
            self.z = (self.startpos as f64) * dz / dist + self.src_z as f64;
            self.y = self.h1 as f64;
        }
        let dt = (self.t2 + 1 - cycle) as f64;
        self.velocity_x = (target_x as f64 - self.x) / dt;
        self.velocity_z = (target_z as f64 - self.z) / dt;
        self.velocity = (self.velocity_z * self.velocity_z + self.velocity_x * self.velocity_x).sqrt();
        if !self.mobile {
            self.velocity_y = -self.velocity * (self.angle as f64 * 0.02454369_f64).tan();
        }
        self.acceleration_y = ((target_y as f64) - self.y - self.velocity_y * dt) * 2.0 / (dt * dt);
    }

    // @ObfuscatedName("fh.y(IB)V") — ClientProj.move. Verbatim port of
    // lines 130-154. Advances the projectile by `cycles` ticks: linear
    // X/Z, ballistic Y (constant accel), atan2 yaw/pitch for facing.
    // Also advances SeqType frame counters.
    pub fn move_(&mut self, cycles: i32) {
        use crate::config::seq_type;
        self.mobile = true;
        let c = cycles as f64;
        self.x += c * self.velocity_x;
        self.z += c * self.velocity_z;
        self.y += self.acceleration_y * 0.5 * c * c + c * self.velocity_y;
        self.velocity_y += c * self.acceleration_y;
        // Java's `* 325.949` converts radians → 2048-step yaw (= 2048/(2π)).
        self.yaw = ((self.velocity_x.atan2(self.velocity_z) * 325.949) as i32 + 1024) & 0x7FF;
        self.pitch = ((self.velocity_y.atan2(self.velocity) * 325.949) as i32) & 0x7FF;
        if self.anim != -1 {
            let seq = seq_type::list(self.anim);
            let Some(delays) = seq.delay.as_ref() else { return };
            let Some(frames) = seq.frames.as_ref() else { return };
            if delays.is_empty() || frames.is_empty() { return; }
            self.anim_cycle += cycles;
            loop {
                while self.anim_cycle > delays[self.anim_frame as usize] {
                    self.anim_cycle -= delays[self.anim_frame as usize];
                    self.anim_frame += 1;
                    if self.anim_frame >= frames.len() as i32 {
                        break;
                    }
                }
                if (self.anim_frame as usize) < frames.len() {
                    return;
                }
                self.anim_frame -= seq.loops;
                if self.anim_frame < 0 || self.anim_frame >= frames.len() as i32 {
                    self.anim_frame = 0;
                    return;
                }
            }
        }
    }

    // @ObfuscatedName("fh.g(I)Lfo;") — ClientProj.getTempModel.
    // Verbatim port of ClientProj.java:157-167: the SpotType's
    // animation frame supplies the model, rotated on the X axis by
    // `pitch` to match the projectile's flight angle.
    pub fn get_temp_model(&self) -> Option<crate::dash3d::model_lit::ModelLit> {
        let spot = crate::config::spot_type::list(self.spotanim);
        let mut m = spot.get_temp_model2(self.anim_frame)?;
        m.rotate_x_axis(self.pitch);
        Some(m)
    }
}
