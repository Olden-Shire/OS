//! cpal-driven audio output. Wraps a [`SharedManager`] and pumps mixed stereo i16 samples
//! to the default output device.
//!
//! The [`Player`] handle owns the cpal stream — drop it to stop playback. Internally the
//! cpal callback locks the manager briefly, calls `render`, and writes the result.

use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;

use crate::midi_manager::{MidiManager, SharedManager};

pub struct Player {
    manager: SharedManager,
    #[allow(dead_code)] // dropped on stop to tear down the cpal callback.
    stream: cpal::Stream,
    sample_rate: u32,
}

impl Player {
    /// Open the default output device at its preferred sample rate. The returned `Player`
    /// holds the cpal stream open until dropped.
    pub fn open() -> Result<Self, Box<dyn std::error::Error>> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no default audio output device")?;
        let config = device.default_output_config()?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels() as usize;
        let manager: SharedManager = Arc::new(Mutex::new(MidiManager::new(sample_rate as i32, true)));
        let manager_for_cb = Arc::clone(&manager);

        // Local scratch — accumulated in mixer's i32 space, then clipped on write.
        let scratch: Arc<Mutex<Vec<i32>>> = Arc::new(Mutex::new(Vec::new()));
        let scratch_cb = Arc::clone(&scratch);

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &config.config(),
                move |out: &mut [f32], _| {
                    let frames = out.len() / channels;
                    let mut buf = scratch_cb.lock();
                    if buf.len() < frames * 2 {
                        buf.resize(frames * 2, 0);
                    }
                    manager_for_cb.lock().render(&mut buf, frames);
                    for f in 0..frames {
                        let l = buf[f * 2].clamp(-32_768 * 256, 32_767 * 256);
                        let r = buf[f * 2 + 1].clamp(-32_768 * 256, 32_767 * 256);
                        let lf = l as f32 / (32_768.0 * 256.0);
                        let rf = r as f32 / (32_768.0 * 256.0);
                        out[f * channels] = lf;
                        if channels >= 2 {
                            out[f * channels + 1] = rf;
                        }
                        for c in 2..channels {
                            out[f * channels + c] = 0.0;
                        }
                    }
                },
                |err| eprintln!("audio output error: {err}"),
                None,
            )?,
            cpal::SampleFormat::I16 => device.build_output_stream(
                &config.config(),
                move |out: &mut [i16], _| {
                    let frames = out.len() / channels;
                    let mut buf = scratch_cb.lock();
                    if buf.len() < frames * 2 {
                        buf.resize(frames * 2, 0);
                    }
                    manager_for_cb.lock().render(&mut buf, frames);
                    for f in 0..frames {
                        let l = (buf[f * 2] >> 8).clamp(-32_768, 32_767) as i16;
                        let r = (buf[f * 2 + 1] >> 8).clamp(-32_768, 32_767) as i16;
                        out[f * channels] = l;
                        if channels >= 2 {
                            out[f * channels + 1] = r;
                        }
                        for c in 2..channels {
                            out[f * channels + c] = 0;
                        }
                    }
                },
                |err| eprintln!("audio output error: {err}"),
                None,
            )?,
            other => return Err(format!("unsupported sample format: {other:?}").into()),
        };
        stream.play()?;
        Ok(Self { manager, stream, sample_rate })
    }

    pub fn manager(&self) -> SharedManager {
        Arc::clone(&self.manager)
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}
