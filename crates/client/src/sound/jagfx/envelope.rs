// @ObfuscatedName("k")
// jag::oldscape::sound::Envelope
//
// Piecewise-linear envelope used by Tone synthesis. Two flavours:
// a header (form, start, end) carrier plus shape points (used for
// amplitude/frequency curves), or just shape points (used for nested
// filter range).

#![allow(dead_code)]

use crate::io::packet::Packet;

#[derive(Debug, Default, Clone)]
pub struct Envelope {
    // @ObfuscatedName("k.n")
    pub form: i32,
    // @ObfuscatedName("k.m")
    pub start: i32,
    // @ObfuscatedName("k.c")
    pub end: i32,
    // @ObfuscatedName("k.d") — shape control points (delta=x axis 0..65535)
    pub shape_delta: Vec<i32>,
    // @ObfuscatedName("k.l") — shape control points (peak=y axis)
    pub shape_peak: Vec<i32>,
    // @ObfuscatedName("k.r") — number of shape points (delta/peak length)
    pub length: i32,

    // Generator state, reset by gen_init.
    // @ObfuscatedName("k.j")
    pub threshold: i32,
    // @ObfuscatedName("k.z")
    pub position: usize,
    // @ObfuscatedName("k.g")
    pub delta: i32,
    // @ObfuscatedName("k.q")
    pub amplitude: i32,
    // @ObfuscatedName("k.i")
    pub ticks: i32,
}

impl Envelope {
    pub fn new() -> Self {
        Self {
            length: 2,
            shape_delta: vec![0, 65535],
            shape_peak: vec![0, 65535],
            ..Default::default()
        }
    }

    // @ObfuscatedName("k.r(Lev;)V") — Envelope.load
    pub fn load(&mut self, p: &mut Packet) {
        self.form = p.g1();
        self.start = p.g4();
        self.end = p.g4();
        self.load_points(p);
    }

    // @ObfuscatedName("k.d(Lev;)V") — Envelope.loadPoints
    pub fn load_points(&mut self, p: &mut Packet) {
        self.length = p.g1();
        let n = self.length as usize;
        self.shape_delta = Vec::with_capacity(n);
        self.shape_peak = Vec::with_capacity(n);
        for _ in 0..n {
            self.shape_delta.push(p.g2());
            self.shape_peak.push(p.g2());
        }
    }

    // @ObfuscatedName("k.l()V") — Envelope.genInit
    pub fn gen_init(&mut self) {
        self.threshold = 0;
        self.position = 0;
        self.delta = 0;
        self.amplitude = 0;
        self.ticks = 0;
    }

    // @ObfuscatedName("k.m(I)I") — Envelope.genNext
    pub fn gen_next(&mut self, total: i32) -> i32 {
        if self.ticks >= self.threshold {
            let peak = (self.shape_peak[self.position] as i32).wrapping_shl(15);
            self.amplitude = peak;
            self.position += 1;
            if self.position >= self.length as usize {
                self.position = (self.length - 1) as usize;
            }
            self.threshold = (self.shape_delta[self.position] as f64 / 65536.0 * total as f64) as i32;
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
