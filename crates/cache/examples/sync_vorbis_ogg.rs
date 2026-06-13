//! One-time sync: wrap Content/vorbis sample `.dat`s as standard `.ogg`
//! (group 0 — the shared setup header — stays `.dat`), then prove the
//! content tree still repacks CRC-identical to the vanilla baseline.
//!
//! Usage: `cargo run --release --example sync_vorbis_ogg -p cache -- [content_dir]`

use std::path::{Path, PathBuf};

use cache::verify::{self, Baseline};
use cache::vorbis_ogg;

fn main() {
    let content_dir = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("Content"), PathBuf::from);

    let vorbis_dir = content_dir.join("vorbis");
    println!("converting {vorbis_dir:?} ...");
    let stats = vorbis_ogg::convert_vorbis_dir(&vorbis_dir).expect("convert vorbis dir");
    println!(
        "  converted: {}  already-ogg: {}  kept-dat: {}",
        stats.converted, stats.already, stats.kept_dat
    );

    println!("verifying repack against cache/crc_baseline.json ...");
    let baseline = Baseline::load(Path::new("cache/crc_baseline.json")).expect("load baseline");
    let tmp = std::env::temp_dir().join("ogg_verify_repack");
    let report = verify::verify_repack(&content_dir, &baseline, &tmp).expect("verify repack");
    if report.is_ok() {
        println!("OK: repack is CRC-identical to the vanilla baseline");
    } else {
        println!("MISMATCH: {report:#?}");
        std::process::exit(1);
    }
}
