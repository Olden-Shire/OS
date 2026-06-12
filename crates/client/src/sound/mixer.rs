// @ObfuscatedName("bo") — jag::oldscape::sound::Mixer.
//
// Audio mix bus. Java holds a LinkList of PcmStreams (active voices)
// and a sorted ring of MixerControllers (envelope/note timing events).
// Each `mix(int[] buf, int off, int len)` walks the streams, sums their
// PCM samples into `buf`, and asks controllers to step their clocks.
//
// This is the scaffolding port: the LinkList becomes Vec<Box<dyn
// PcmStream>>, and MixerController dispatch becomes a sorted Vec
// keyed by `nextTime`. Routing into the PcmPlayer output ring stays
// in `pcm_player.rs`.

#![allow(dead_code)]

use std::sync::Mutex;

use super::pcm_stream::PcmStream;

pub struct Mixer {
    // @ObfuscatedName("bo.r") — active voices.
    pub streams: Vec<Box<dyn PcmStream + Send>>,
    // @ObfuscatedName("bo.d") — pending events sorted by `next_time`.
    pub controllers: Vec<Box<dyn MixerController + Send>>,
    // @ObfuscatedName("bo.l") — sample-rate clock (44_100 default).
    pub sample_rate: i32,
    // @ObfuscatedName("bo.m") — current absolute sample position.
    pub position: i64,
}

impl Mixer {
    pub fn new(sample_rate: i32) -> Self {
        Self {
            streams: Vec::new(),
            controllers: Vec::new(),
            sample_rate,
            position: 0,
        }
    }

    pub fn add_stream(&mut self, stream: Box<dyn PcmStream + Send>) {
        self.streams.push(stream);
    }

    pub fn add_controller(&mut self, ctrl: Box<dyn MixerController + Send>) {
        // Sorted-insert by next_time so the front of the vec is always
        // the earliest pending controller.
        let t = ctrl.next_time();
        let idx = self.controllers.partition_point(|c| c.next_time() <= t);
        self.controllers.insert(idx, ctrl);
    }

    // @ObfuscatedName("bo.j([III)V") — Mixer.mix. Sums each stream's
    // doMix output into `buf` and advances the controller queue.
    pub fn mix(&mut self, buf: &mut [i32], off: i32, len: i32) {
        // Run controllers due before the next sample.
        while let Some(ctrl) = self.controllers.first() {
            if ctrl.next_time() > self.position { break; }
            let mut c = self.controllers.remove(0);
            c.step();
            if c.is_active() {
                let t = c.next_time();
                let idx = self.controllers.partition_point(|o| o.next_time() <= t);
                self.controllers.insert(idx, c);
            }
        }
        // Mix each stream. Java drops streams that report -1 from
        // doMix; we do the same.
        let mut i = 0;
        while i < self.streams.len() {
            let done = self.streams[i].do_mix(buf, off, len);
            if done { self.streams.swap_remove(i); }
            else { i += 1; }
        }
        self.position += len as i64;
    }
}

// @ObfuscatedName("dq") — jag::oldscape::sound::MixerController.
//
// Abstract scheduler base — held in Mixer's sorted Vec. Each impl
// advances some envelope / note / sequencer state when `step()` is
// called, then reports the next absolute sample time and whether to
// stay in the queue.
pub trait MixerController {
    fn next_time(&self) -> i64;
    fn step(&mut self);
    fn is_active(&self) -> bool { true }
}

pub static MIXER: std::sync::LazyLock<Mutex<Mixer>> =
    std::sync::LazyLock::new(|| Mutex::new(Mixer::new(44_100)));
