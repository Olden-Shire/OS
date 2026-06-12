// @ObfuscatedName("ah")
// jag::oldscape::sound::PcmPlayer (and JavaPcmPlayer) — cpal-backed audio
// sink. Owns the MidiManager via a shared mutex; the cpal callback locks
// briefly, calls render, writes samples. Drop the Player to stop.

#![allow(dead_code)]

use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
#[cfg(not(target_arch = "wasm32"))]
use parking_lot::Mutex;

use crate::midi2::midi_manager::SharedManager;
#[cfg(not(target_arch = "wasm32"))]
use crate::midi2::midi_manager::MidiManager;

pub struct PcmPlayer {
    // @ObfuscatedName("ah.r")
    pub manager: SharedManager,
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    stream: cpal::Stream,
    // @ObfuscatedName("ah.frequency")
    pub frequency: u32,
}

// wasm: no audio sink yet — the client treats a failed init as "no audio
// device" and runs silent. A WebAudio (AudioWorklet) backend is the
// follow-up once the core boots in the browser.
#[cfg(target_arch = "wasm32")]
impl PcmPlayer {
    pub fn init(_frequency: i32, _stereo: bool) -> Result<Self, Box<dyn std::error::Error>> {
        Err("audio not wired on wasm yet".into())
    }

    pub fn manager(&self) -> SharedManager {
        Arc::clone(&self.manager)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl PcmPlayer {
    // @ObfuscatedName("ah.init(IZIB)V") — PcmPlayer.init(freq, stereo, ...)
    pub fn init(_frequency: i32, stereo: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no default audio output device")?;
        let config = device.default_output_config()?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels() as usize;
        let manager: SharedManager =
            Arc::new(Mutex::new(MidiManager::new(sample_rate as i32, stereo)));
        let manager_cb = Arc::clone(&manager);
        let scratch: Arc<Mutex<Vec<i32>>> = Arc::new(Mutex::new(Vec::new()));
        let scratch_cb = Arc::clone(&scratch);

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &config.config(),
                move |out: &mut [f32], _| {
                    let frames = out.len() / channels.max(1);
                    let mut buf = scratch_cb.lock();
                    if buf.len() < frames * 2 { buf.resize(frames * 2, 0); }
                    manager_cb.lock().render(&mut buf, frames);
                    for f in 0..frames {
                        let l = buf[f * 2].clamp(-32_768 * 256, 32_767 * 256);
                        let r = buf[f * 2 + 1].clamp(-32_768 * 256, 32_767 * 256);
                        out[f * channels] = l as f32 / (32_768.0 * 256.0);
                        if channels >= 2 { out[f * channels + 1] = r as f32 / (32_768.0 * 256.0); }
                        for c in 2..channels { out[f * channels + c] = 0.0; }
                    }
                },
                |err| eprintln!("[audio] {err}"),
                None,
            )?,
            cpal::SampleFormat::I16 => device.build_output_stream(
                &config.config(),
                move |out: &mut [i16], _| {
                    let frames = out.len() / channels.max(1);
                    let mut buf = scratch_cb.lock();
                    if buf.len() < frames * 2 { buf.resize(frames * 2, 0); }
                    manager_cb.lock().render(&mut buf, frames);
                    for f in 0..frames {
                        let l = (buf[f * 2] >> 8).clamp(-32_768, 32_767) as i16;
                        let r = (buf[f * 2 + 1] >> 8).clamp(-32_768, 32_767) as i16;
                        out[f * channels] = l;
                        if channels >= 2 { out[f * channels + 1] = r; }
                        for c in 2..channels { out[f * channels + c] = 0; }
                    }
                },
                |err| eprintln!("[audio] {err}"),
                None,
            )?,
            other => return Err(format!("unsupported sample format: {other:?}").into()),
        };
        stream.play()?;
        Ok(Self { manager, stream, frequency: sample_rate })
    }

    pub fn manager(&self) -> SharedManager { Arc::clone(&self.manager) }
}
