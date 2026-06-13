// Sanity-check converted vorbis .oggs with lewton (a standard Vorbis
// decoder, fully independent of the gamepack's jagvorbis port): if these
// decode, any player will take them.
//
// Usage: cargo run --release -p client --example ogg_check -- [dir]

use std::fs::File;

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "Content/vorbis".into());
    let mut checked = 0u32;
    let mut samples_total = 0u64;
    for entry in std::fs::read_dir(&dir).expect("read dir") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("ogg") {
            continue;
        }
        let f = File::open(&path).expect("open");
        let mut rdr = lewton::inside_ogg::OggStreamReader::new(f)
            .unwrap_or_else(|e| panic!("{path:?}: header decode failed: {e:?}"));
        assert_eq!(rdr.ident_hdr.audio_channels, 1, "{path:?}: not mono");
        let mut n = 0u64;
        while let Some(pcm) = rdr
            .read_dec_packet_itl()
            .unwrap_or_else(|e| panic!("{path:?}: packet decode failed: {e:?}"))
        {
            n += pcm.len() as u64;
        }
        assert!(n > 0, "{path:?}: decoded zero samples");
        checked += 1;
        samples_total += n;
    }
    println!("decoded {checked} oggs, {samples_total} PCM samples total — all standard-Vorbis clean");
}
