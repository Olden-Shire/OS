//! One-time sync: fold Content/maps m/l `.dat` pairs into per-region `.jm2`
//! text, then prove the content tree still repacks CRC-identical to the
//! vanilla baseline (the same gate the server runs at startup).
//!
//! Usage: `cargo run --release --example sync_maps_jm2 -p cache -- [content_dir]`
//! Default content dir: `Content`.

use std::path::{Path, PathBuf};

use cache::content::maps_jm2;
use cache::verify::{self, Baseline};

fn main() {
    let content_dir = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("Content"), PathBuf::from);

    let maps_dir = content_dir.join("maps");
    println!("converting {maps_dir:?} ...");
    let stats = maps_jm2::convert_maps_dir(&maps_dir).expect("convert maps dir");
    println!(
        "  converted: {}  already-jm2: {}  kept-dat: {}",
        stats.converted, stats.already, stats.kept_dat
    );

    println!("verifying repack against cache/crc_baseline.json ...");
    let baseline = Baseline::load(Path::new("cache/crc_baseline.json")).expect("load baseline");
    let tmp = std::env::temp_dir().join("jm2_verify_repack");
    let report = verify::verify_repack(&content_dir, &baseline, &tmp).expect("verify repack");
    if report.is_ok() {
        println!("OK: repack is CRC-identical to the vanilla baseline");
    } else {
        println!("MISMATCH: {report:#?}");
        std::process::exit(1);
    }
}
