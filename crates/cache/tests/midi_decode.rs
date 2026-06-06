//! Round-trip every cache song + jingle through decode → encode and assert bytes match.

use std::path::PathBuf;

use cache::Cache;

fn cache_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../cache")
}

#[test]
fn midi_codec_round_trips_every_song_and_jingle() {
    let mut c = Cache::open(&cache_dir()).expect("open");

    let mut ok = 0u32;
    let mut fail = Vec::<(u8, i32, String)>::new();

    for archive in [6u8, 11u8] {
        let gids: Vec<i32> = c.index(archive).group_ids.clone();
        for gid in gids {
            let orig = c.read_group(archive, gid as u32).unwrap().unwrap();
            let decoded = match std::panic::catch_unwind(|| io::midi::decode(&orig)) {
                Ok(d) => d,
                Err(_) => { fail.push((archive, gid, "decode panic".into())); continue; }
            };
            let encoded = match std::panic::catch_unwind(|| io::midi::encode(&decoded)) {
                Ok(e) => e,
                Err(_) => { fail.push((archive, gid, "encode panic".into())); continue; }
            };
            if encoded != orig {
                let len_diff = encoded.len() as i64 - orig.len() as i64;
                let first_diff = (0..encoded.len().min(orig.len()))
                    .find(|&i| encoded[i] != orig[i])
                    .map(|i| format!("first diff @ {i}: {:02X} vs {:02X}", encoded[i], orig[i]))
                    .unwrap_or_default();
                fail.push((archive, gid, format!("bytes differ (len diff {len_diff}, {first_diff})")));
                continue;
            }
            ok += 1;
        }
    }

    eprintln!("round-trip OK: {ok}, failed: {}", fail.len());
    for (a, g, msg) in fail.iter().take(5) {
        eprintln!("  {a}/{g}: {msg}");
    }
    assert!(fail.is_empty(), "{} failures", fail.len());
}
