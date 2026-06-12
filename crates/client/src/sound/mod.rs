// jagex3.sound package.
//
// PCM playback (PcmPlayer, Mixer, PcmStream*) and the wave family
// (Wave, WaveCache, WaveStream + the JagFX/JagVorbis decoders behind
// them). The Java layout has both midi2 and sound here; the midi2
// package lives in `crate::midi2`. Per-class @ObfuscatedName tags are on
// each module's top-of-file header.

pub mod bg_sound;
pub mod decimator;
pub mod js5_cache;
pub mod mixer;
pub mod pcm_player;
pub mod pcm_stream;
pub mod pcm_streamable;
pub mod wave;
pub mod wave_cache;
pub mod wave_stream;
pub mod jagfx;
pub mod jagvorbis;
