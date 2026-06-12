//! Headless test: load song 0 from the cache, render 2 seconds of audio at 48 kHz, and
//! report what got produced. Useful for diagnosing whether the synth pipeline produces
//! any non-zero samples without involving cpal / the audio device.

use std::path::Path;

use cache::Cache;
use synth::MidiManager;

const SONGS_ARCHIVE: u8 = 6;

#[test]
fn renders_some_audio() {
    let path = Path::new("../../cache");
    if !path.join("main_file_cache.dat2").exists() {
        eprintln!("skip: no cache");
        return;
    }
    let mut cache = Cache::open(path).unwrap();

    // Pick the first non-empty song.
    let song_groups: Vec<u32> = cache.index(SONGS_ARCHIVE).group_ids.iter().map(|&g| g as u32).collect();
    let mut chosen = None;
    for gid in &song_groups {
        let bytes = cache.read_group(SONGS_ARCHIVE, *gid).unwrap().unwrap_or_default();
        if !bytes.is_empty() {
            chosen = Some((*gid, bytes));
            break;
        }
    }
    let (gid, jagex_bytes) = chosen.expect("at least one song");
    let standard_midi = io::midi::decode(&jagex_bytes);
    eprintln!("song {gid}: {} jagex bytes → {} standard midi bytes", jagex_bytes.len(), standard_midi.len());

    let mut mgr = MidiManager::new(48_000, true);
    let (mp, mw) = mgr.load_song(&mut cache, standard_midi, false);
    eprintln!(
        "loaded: {} patches in table, {} waves in table; missing patches={mp}, missing waves={mw}",
        mgr.patch_table.len(),
        mgr.wave_table.len()
    );
    assert!(!mgr.patch_table.is_empty(), "no patches loaded");

    // Render 2 seconds at 48k stereo. Buffer is i32 interleaved L,R.
    let frames = 48_000 * 2;
    let mut buf = vec![0i32; frames * 2];
    // cpal serves callbacks of ~1024 frames; simulate that.
    let chunk = 1024usize;
    let mut done = 0usize;
    while done < frames {
        let n = chunk.min(frames - done);
        mgr.render(&mut buf[done * 2..(done + n) * 2], n);
        done += n;
    }

    // Summarize: peak amplitude, RMS, count of frames with any non-zero sample.
    let mut peak = 0i64;
    let mut nonzero_frames = 0usize;
    let mut sum_sq: f64 = 0.0;
    for f in 0..frames {
        let l = buf[f * 2] as i64;
        let r = buf[f * 2 + 1] as i64;
        let mag = l.abs().max(r.abs());
        if mag > peak {
            peak = mag;
        }
        if mag > 0 {
            nonzero_frames += 1;
        }
        sum_sq += (l as f64).powi(2) + (r as f64).powi(2);
    }
    let rms = (sum_sq / (frames as f64 * 2.0)).sqrt();
    eprintln!(
        "rendered {frames} frames; peak={peak}, rms={rms:.1}, nonzero frames={nonzero_frames} ({:.1}%)",
        nonzero_frames as f64 * 100.0 / frames as f64
    );
    eprintln!(
        "active notes after render: {}; queued events tick={}, time={}",
        mgr.player.notes.len(),
        mgr.player.track_current_tick,
        mgr.player.track_current_time,
    );
    eprintln!(
        "note-ons: seen={}, played={}, missing patch={}, missing wave={}",
        mgr.debug_note_ons_seen,
        mgr.debug_note_ons_played,
        mgr.debug_note_ons_missing_patch,
        mgr.debug_note_ons_missing_wave,
    );
    // For every loaded patch, count keys with non-zero wave_id and envelopes.
    let mut sorted_pids: Vec<i32> = mgr.patch_table.keys().copied().collect();
    sorted_pids.sort();
    for pid in sorted_pids {
        let p = &mgr.patch_table[&pid];
        let nonzero = (0..128).filter(|&k| p.note_wave_id[k] != 0).count();
        eprintln!(
            "patch {pid}: vol={}, {} keys with wave_id, {} envelopes",
            p.volume,
            nonzero,
            p.envelopes.len()
        );
    }
    assert!(peak > 0, "no audible samples produced in 2 seconds");
}
