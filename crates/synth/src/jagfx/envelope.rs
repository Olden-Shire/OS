//! `jagex3.sound.Envelope` — piecewise-linear envelope used by Tone synthesis.
//!
//! Two flavours: a header `(form, start, end)` carrier plus shape points (used for
//! amplitude/frequency curves), or just shape points (used for nested filter range).

use io::packet::Packet;

#[derive(Debug, Default, Clone)]
pub struct Envelope {
    pub form: i32,
    pub start: i32,
    pub end: i32,
    pub shape_delta: Vec<i32>,
    pub shape_peak: Vec<i32>,

    // gen state (reset by gen_init)
    pub threshold: i32,
    pub position: usize,
    pub delta: i32,
    pub amplitude: i32,
    pub ticks: i32,
}

impl Envelope {
    pub fn new() -> Self {
        Self {
            shape_delta: vec![0, 65535],
            shape_peak: vec![0, 65535],
            ..Default::default()
        }
    }

    pub fn load(&mut self, p: &mut Packet) {
        self.form = p.g1();
        self.start = p.g4();
        self.end = p.g4();
        self.load_points(p);
    }

    pub fn load_points(&mut self, p: &mut Packet) {
        let n = p.g1() as usize;
        self.shape_delta = Vec::with_capacity(n);
        self.shape_peak = Vec::with_capacity(n);
        for _ in 0..n {
            self.shape_delta.push(p.g2());
            self.shape_peak.push(p.g2());
        }
    }

    pub fn gen_init(&mut self) {
        self.threshold = 0;
        self.position = 0;
        self.delta = 0;
        self.amplitude = 0;
        self.ticks = 0;
    }

    pub fn gen_next(&mut self, total: i32) -> i32 {
        if self.ticks >= self.threshold {
            let peak = (self.shape_peak[self.position] as i32).wrapping_shl(15);
            self.amplitude = peak;
            self.position += 1;
            if self.position >= self.shape_peak.len() {
                self.position = self.shape_peak.len() - 1;
            }
            self.threshold =
                (self.shape_delta[self.position] as f64 / 65536.0 * total as f64) as i32;
            if self.threshold > self.ticks {
                let next_peak = (self.shape_peak[self.position] as i32).wrapping_shl(15);
                self.delta = next_peak.wrapping_sub(self.amplitude) / (self.threshold - self.ticks);
            }
        }
        self.amplitude = self.amplitude.wrapping_add(self.delta);
        self.ticks += 1;
        self.amplitude.wrapping_sub(self.delta) >> 15
    }
}
