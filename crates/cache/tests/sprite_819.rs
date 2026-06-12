//! Repro: sprite group 819 was hard-crashing the editor. Reproduce the exact decode here.

#[test]
fn decode_sprite_819() {
    let path = std::path::Path::new("../../cache");
    if !path.join("main_file_cache.dat2").exists() {
        eprintln!("skip: no cache");
        return;
    }
    let mut c = cache::Cache::open(path).unwrap();
    let bytes = c.read_group(8, 819).unwrap().expect("sprite 819 should exist");
    eprintln!("sprite 819: {} bytes, first 16 = {:?}", bytes.len(), &bytes[..16.min(bytes.len())]);
    eprintln!("last 16 = {:?}", &bytes[bytes.len().saturating_sub(16)..]);

    let sheet = cache::sprite::SpriteSheet::decode(&bytes);
    eprintln!(
        "decoded: outer {}×{}, {} palette colors, {} sprites",
        sheet.outer_width,
        sheet.outer_height,
        sheet.palette.len(),
        sheet.sprites.len()
    );
    for (i, s) in sheet.sprites.iter().enumerate().take(5) {
        eprintln!(
            "  sprite #{i}: {}×{} @ ({},{}), {} indices",
            s.width,
            s.height,
            s.x_offset,
            s.y_offset,
            s.indices.len()
        );
    }
    // Try to_rgba on each sprite — this is what the viewer does.
    for (i, s) in sheet.sprites.iter().enumerate() {
        let rgba = s.to_rgba(&sheet.palette);
        assert_eq!(rgba.len(), s.indices.len() * 4, "sprite {i} rgba length mismatch");
    }
}
