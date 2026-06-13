//! Compare two cache dirs group-by-group (read_raw + master). Args: <a> <b>.
//! Reports any group whose raw bytes differ, plus groups present in one only.
use std::path::Path;
use cache::Cache;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let a_dir = args.get(1).map(String::as_str).unwrap_or("cache");
    let b_dir = args.get(2).map(String::as_str).unwrap_or_else(|| {
        eprintln!("usage: diff_caches <a> <b>");
        std::process::exit(2)
    });
    let mut a = Cache::open(Path::new(a_dir)).expect("open a");
    let mut b = Cache::open(Path::new(b_dir)).expect("open b");
    let mut diffs = 0usize;
    let mut checked = 0usize;
    for archive in 0u8..16 {
        let ga: Vec<i32> = a.index(archive).group_ids.clone();
        let gb: Vec<i32> = b.index(archive).group_ids.clone();
        if ga != gb {
            println!("archive {archive}: GROUP ID SET DIFFERS (a={} b={})", ga.len(), gb.len());
            let sa: std::collections::BTreeSet<_> = ga.iter().collect();
            let sb: std::collections::BTreeSet<_> = gb.iter().collect();
            let only_a: Vec<_> = sa.difference(&sb).take(10).collect();
            let only_b: Vec<_> = sb.difference(&sa).take(10).collect();
            println!("  only in a: {only_a:?}");
            println!("  only in b: {only_b:?}");
        }
        for gid in &ga {
            let gid = *gid as u32;
            let ra = a.read_raw(archive, gid).ok().flatten();
            let rb = b.read_raw(archive, gid).ok().flatten();
            checked += 1;
            if ra != rb {
                diffs += 1;
                let la = ra.as_ref().map(|v| v.len());
                let lb = rb.as_ref().map(|v| v.len());
                if diffs <= 30 {
                    println!("DIFF archive={archive} group={gid} len_a={la:?} len_b={lb:?}");
                }
            }
        }
        // master
        let ma = a.read_master_raw(archive).ok().flatten();
        let mb = b.read_master_raw(archive).ok().flatten();
        if ma != mb {
            println!("DIFF MASTER archive={archive} len_a={:?} len_b={:?}",
                ma.as_ref().map(|v| v.len()), mb.as_ref().map(|v| v.len()));
        }
    }
    println!("checked {checked} groups, {diffs} differ");
}
