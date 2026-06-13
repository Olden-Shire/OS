//! Unpack the rev1 cache to a fresh Content tree for inspection.
use std::path::PathBuf;
use cache::Cache;
use cache::content::unpack;
use cache::maps::XteaKeys;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut c = Cache::open(&root.join("cache")).expect("open cache");
    let keys = XteaKeys::load(&root.join("cache/keys.json")).unwrap_or_else(|_| XteaKeys { by_mapsquare: Default::default() });
    let dest = root.join("Content-new");
    let s = unpack(&mut c, &keys, &dest).expect("unpack");
    eprintln!("unpacked {} groups → {}", s.total_groups, dest.display());
}
