// @ObfuscatedName("ah")
// jag::oldscape::sound::PcmPlayer (and JavaPcmPlayer) — cpal-backed audio
// sink. Owns the MidiManager via a shared mutex; the cpal callback locks
// briefly, calls render, writes samples. Drop the Player to stop.

#![allow(dead_code)]

use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;

use crate::midi2::midi_manager::{MidiManager, SharedManager};
use crate::sound::mixer::Mixer;

/// Shared handle to the FX `Mixer` (sound-effect voice bus). Java runs a
/// second `synthPlayer.playStream(mixer)` on its own audio line; we sum
/// the mixer into the single output callback instead (native cpal, or the
/// wasm ScriptProcessorNode) — one sink, no cross-line clock drift. Set at
/// `PcmPlayer::init`; the client tick (`soundsDoQueue`) and `bg_sound`
/// push `WaveStream` voices here.
pub static FX_MIXER: std::sync::Mutex<Option<Arc<Mutex<Mixer>>>> =
    std::sync::Mutex::new(None);

/// Push a `WaveStream` voice onto the live FX mixer; returns its voice id
/// (`-1` when audio isn't initialised — no device).
pub fn play_fx_stream(stream: crate::sound::wave_stream::WaveStream) -> i64 {
    if let Some(mixer) = FX_MIXER.lock().unwrap().as_ref() {
        return mixer.lock().add_stream(Box::new(stream));
    }
    -1
}

/// Stop the FX voice with `id` (BgSound.stopStream).
pub fn fx_stop(id: i64) {
    if let Some(mixer) = FX_MIXER.lock().unwrap().as_ref() {
        mixer.lock().stop_stream(id);
    }
}

/// Set the FX voice's volume (BgSound positional fade).
pub fn fx_apply_volume(id: i64, vol: i32) {
    if let Some(mixer) = FX_MIXER.lock().unwrap().as_ref() {
        mixer.lock().set_voice_volume(id, vol);
    }
}

/// Is the FX voice still active (WaveStream.isLinked)?
pub fn fx_is_linked(id: i64) -> bool {
    if let Some(mixer) = FX_MIXER.lock().unwrap().as_ref() {
        return mixer.lock().is_linked(id);
    }
    false
}

pub struct PcmPlayer {
    // @ObfuscatedName("ah.r")
    pub manager: SharedManager,
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    stream: cpal::Stream,
    // @ObfuscatedName("bo") — FX mixer (Java's `Client.mixer`).
    pub fx_mixer: Arc<Mutex<Mixer>>,
    // wasm WebAudio graph — kept alive for the player's lifetime (dropping
    // the AudioContext/node/closure would stop the sound).
    #[cfg(target_arch = "wasm32")]
    #[allow(dead_code)]
    ctx: web_sys::AudioContext,
    #[cfg(target_arch = "wasm32")]
    #[allow(dead_code)]
    node: web_sys::ScriptProcessorNode,
    #[cfg(target_arch = "wasm32")]
    #[allow(dead_code)]
    cb: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::AudioProcessingEvent)>,
    // @ObfuscatedName("ah.frequency")
    pub frequency: u32,
}

// wasm: WebAudio sink via a ScriptProcessorNode. Its onaudioprocess
// callback re-enters wasm (single-threaded, so no real concurrency) and
// renders the same MIDI + FX-mixer path as the native cpal callback.
#[cfg(target_arch = "wasm32")]
impl PcmPlayer {
    pub fn init(_frequency: i32, stereo: bool) -> Result<Self, Box<dyn std::error::Error>> {
        use wasm_bindgen::JsCast;
        use wasm_bindgen::closure::Closure;

        let ctx = web_sys::AudioContext::new().map_err(|_| "no AudioContext")?;
        let sample_rate = ctx.sample_rate() as i32;

        let manager: SharedManager =
            Arc::new(Mutex::new(MidiManager::new(sample_rate, stereo)));
        let fx_mixer: Arc<Mutex<Mixer>> = Arc::new(Mutex::new(Mixer::new(sample_rate)));

        // ScriptProcessorNode(bufferSize, 0 inputs, 2 outputs). Deprecated,
        // but the only no-AudioWorklet way to get a synchronous pull
        // callback that can re-enter wasm to render.
        const BUF: u32 = 4096;
        let node = ctx
            .create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(BUF, 0, 2)
            .map_err(|_| "createScriptProcessor failed")?;

        let manager_cb = Arc::clone(&manager);
        let fx_cb = Arc::clone(&fx_mixer);
        let mut buf: Vec<i32> = Vec::new();
        let mut chan_l: Vec<f32> = Vec::new();
        let mut chan_r: Vec<f32> = Vec::new();
        let cb = Closure::<dyn FnMut(web_sys::AudioProcessingEvent)>::new(
            move |e: web_sys::AudioProcessingEvent| {
                let Ok(out) = e.output_buffer() else { return };
                let frames = out.length() as usize;
                if buf.len() < frames * 2 { buf.resize(frames * 2, 0); }
                for s in buf[..frames * 2].iter_mut() { *s = 0; }
                manager_cb.lock().render(&mut buf, frames);
                fx_cb.lock().mix(&mut buf, 0, frames as i32);
                if chan_l.len() < frames {
                    chan_l.resize(frames, 0.0);
                    chan_r.resize(frames, 0.0);
                }
                // Same scale as the native F32 path: i32 buf is ×256.
                for f in 0..frames {
                    let l = buf[f * 2].clamp(-32_768 * 256, 32_767 * 256);
                    let r = buf[f * 2 + 1].clamp(-32_768 * 256, 32_767 * 256);
                    chan_l[f] = l as f32 / (32_768.0 * 256.0);
                    chan_r[f] = r as f32 / (32_768.0 * 256.0);
                }
                let _ = out.copy_to_channel(&mut chan_l[..frames], 0);
                let _ = out.copy_to_channel(&mut chan_r[..frames], 1);
            },
        );
        node.set_onaudioprocess(Some(cb.as_ref().unchecked_ref()));
        node.connect_with_audio_node(&ctx.destination())
            .map_err(|_| "connect failed")?;

        // Autoplay policy: the context starts suspended until a user
        // gesture. Try now, and resume on the first input event too.
        let _ = ctx.resume();
        install_resume_on_gesture(&ctx);

        *FX_MIXER.lock().unwrap() = Some(Arc::clone(&fx_mixer));
        Ok(Self { manager, fx_mixer, ctx, node, cb, frequency: sample_rate as u32 })
    }

    pub fn manager(&self) -> SharedManager { Arc::clone(&self.manager) }
    pub fn fx_mixer(&self) -> Arc<Mutex<Mixer>> { Arc::clone(&self.fx_mixer) }
}

// Browsers keep the AudioContext suspended until a user gesture. Attach
// one-shot-ish listeners that resume it on the first pointer/key/touch
// event (resume is idempotent once running). Closures are leaked via
// forget() since they live for the page's lifetime.
#[cfg(target_arch = "wasm32")]
fn install_resume_on_gesture(ctx: &web_sys::AudioContext) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    let Some(win) = web_sys::window() else { return };
    for ev in ["mousedown", "keydown", "touchstart", "pointerdown"] {
        let ctx2 = ctx.clone();
        let cb = Closure::<dyn FnMut()>::new(move || { let _ = ctx2.resume(); });
        let _ = win.add_event_listener_with_callback(ev, cb.as_ref().unchecked_ref());
        cb.forget();
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
        let fx_mixer: Arc<Mutex<Mixer>> =
            Arc::new(Mutex::new(Mixer::new(sample_rate as i32)));
        let fx_cb = Arc::clone(&fx_mixer);
        let fx_cb2 = Arc::clone(&fx_mixer);
        let scratch: Arc<Mutex<Vec<i32>>> = Arc::new(Mutex::new(Vec::new()));
        let scratch_cb = Arc::clone(&scratch);

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &config.config(),
                move |out: &mut [f32], _| {
                    let frames = out.len() / channels.max(1);
                    let mut buf = scratch_cb.lock();
                    if buf.len() < frames * 2 { buf.resize(frames * 2, 0); }
                    for s in buf[..frames * 2].iter_mut() { *s = 0; }
                    manager_cb.lock().render(&mut buf, frames);
                    fx_cb.lock().mix(&mut buf, 0, frames as i32);
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
                    for s in buf[..frames * 2].iter_mut() { *s = 0; }
                    manager_cb.lock().render(&mut buf, frames);
                    fx_cb2.lock().mix(&mut buf, 0, frames as i32);
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
        *FX_MIXER.lock().unwrap() = Some(Arc::clone(&fx_mixer));
        Ok(Self { manager, stream, fx_mixer, frequency: sample_rate })
    }

    pub fn manager(&self) -> SharedManager { Arc::clone(&self.manager) }

    /// Shared handle to the FX mixer for pushing `WaveStream` voices.
    pub fn fx_mixer(&self) -> Arc<Mutex<Mixer>> { Arc::clone(&self.fx_mixer) }
}
