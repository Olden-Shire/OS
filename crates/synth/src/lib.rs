//! Software synth — port of `jagex3.midi2` + `jagex3.sound` from the rev1 Java client.
//!
//! Top-level entrypoint is [`output::Player`] — open a cpal stream + a shared
//! [`midi_manager::MidiManager`], call `load_song` from the editor thread, render runs in
//! the cpal callback.

pub mod envelope;
pub mod file;
pub mod jagfx;
pub mod jagvorbis;
pub mod midi_manager;
pub mod midi_note;
pub mod midi_player;
pub mod output;
pub mod parser;
pub mod patch;
pub mod wave;
pub mod wave_cache;
pub mod wave_stream;

pub use envelope::EnvelopeSet;
pub use file::MidiFile;
pub use midi_manager::{MidiManager, SharedManager};
pub use midi_player::MidiPlayer;
pub use output::Player;
pub use parser::MidiParser;
pub use patch::Patch;
pub use wave::Wave;
pub use wave_cache::WaveCache;
