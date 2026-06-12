//! `.jm2` text round-trip for map regions.
//!
//! Decodes a region's binary land (`m{x}_{y}`) and loc (`l{x}_{y}`) streams into an
//! editable text form and re-encodes them **byte-for-byte**, mirroring Engine-TS's
//! map unpack/pack tooling (`tools/unpack/map/Unpack.ts`, `tools/pack/map/Pack.js`).
//!
//! The *text shape* mirrors Engine-TS's `.jm2` (`==== MAP ====` / `==== LOC ====`
//! sections) so the files are interchangeable for editing, but the *binary codec*
//! follows OS1's rev1 client — exactly the inverse of the parent module's
//! [`decode_tile`](super)/[`decode_locs`](super::decode_locs) — NOT Engine-TS's newer
//! revision (whose height opcode lacks the rev1 `1 → 0` magic).
//!
//! Unlike [`Region`](super::Region), this keeps the "the stream emitted nothing here"
//! distinction (`None`) instead of filling perlin default heights, since those
//! defaults are regenerated at load time and must never be written back.

use io::Packet;

use super::{LocPlacement, REGION_LEVELS, REGION_SIZE};

/// An overlay placement on a tile (texture id + shape + rotation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawOverlay {
    pub id: u8,
    pub shape: u8,
    pub rotation: u8,
}

/// A tile's explicitly-emitted land data. `None` fields are values the stream never
/// emitted (a perlin-default height / an absent overlay/flags/underlay) — preserving
/// that distinction is what lets the re-encode reproduce the original bytes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RawTile {
    pub height: Option<i32>,
    pub overlay: Option<RawOverlay>,
    pub flags: Option<u8>,
    pub underlay: Option<u8>,
}

/// A region in its raw (un-perlin-filled) form: every tile's emitted land data plus
/// every loc placement. Round-trips losslessly to/from the binary streams and `.jm2`
/// text.
#[derive(Debug, Clone)]
pub struct RawRegion {
    /// `tiles[level][x][z]`.
    pub tiles: Box<[[[RawTile; REGION_SIZE]; REGION_SIZE]; REGION_LEVELS]>,
    pub locs: Vec<LocPlacement>,
}

/// The packed `(level, x, z)` key the loc stream delta-encodes against.
fn packed_pos(l: &LocPlacement) -> i32 {
    ((l.level as i32) << 12) | ((l.x as i32) << 6) | l.z as i32
}

impl RawRegion {
    /// Decode the binary land stream and an optional (already-decrypted) loc stream.
    #[must_use]
    pub fn decode(land_bytes: &[u8], loc_bytes: Option<&[u8]>) -> Self {
        Self {
            tiles: decode_land(land_bytes),
            locs: loc_bytes.map(super::decode_locs).unwrap_or_default(),
        }
    }

    /// Re-encode the land stream — the exact inverse of [`decode_land`].
    #[must_use]
    pub fn encode_land(&self) -> Vec<u8> {
        encode_land(&self.tiles)
    }

    /// Re-encode the loc stream — the exact inverse of [`super::decode_locs`].
    #[must_use]
    pub fn encode_locs(&self) -> Vec<u8> {
        encode_locs(&self.locs)
    }

    /// Serialise to the `.jm2` text form.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut s = String::from("==== MAP ====\n");
        for level in 0..REGION_LEVELS {
            for x in 0..REGION_SIZE {
                for z in 0..REGION_SIZE {
                    let t = &self.tiles[level][x][z];
                    let mut tokens = String::new();
                    if let Some(h) = t.height {
                        tokens.push_str(&format!("h{h} "));
                    }
                    if let Some(o) = t.overlay {
                        // Omit trailing zero shape/rotation, mirroring Engine-TS.
                        if o.shape != 0 && o.rotation != 0 {
                            tokens.push_str(&format!("o{};{};{} ", o.id, o.shape, o.rotation));
                        } else if o.shape != 0 {
                            tokens.push_str(&format!("o{};{} ", o.id, o.shape));
                        } else {
                            tokens.push_str(&format!("o{} ", o.id));
                        }
                    }
                    if let Some(f) = t.flags {
                        tokens.push_str(&format!("f{f} "));
                    }
                    if let Some(u) = t.underlay {
                        tokens.push_str(&format!("u{u} "));
                    }
                    if !tokens.is_empty() {
                        s.push_str(&format!("{level} {x} {z}: {}\n", tokens.trim_end()));
                    }
                }
            }
        }

        s.push_str("\n==== LOC ====\n");
        let mut locs: Vec<&LocPlacement> = self.locs.iter().collect();
        // Stable tile-order output (level, x, z, id); the binary re-encode re-sorts
        // by id, so this ordering is purely for a deterministic, readable file.
        locs.sort_by_key(|l| (l.level, l.x, l.z, l.id, l.shape, l.rotation));
        for l in locs {
            if l.rotation == 0 {
                s.push_str(&format!("{} {} {}: {} {}\n", l.level, l.x, l.z, l.id, l.shape));
            } else {
                s.push_str(&format!(
                    "{} {} {}: {} {} {}\n",
                    l.level, l.x, l.z, l.id, l.shape, l.rotation
                ));
            }
        }
        s
    }

    /// Parse the `.jm2` text form. Unknown sections (e.g. `==== NPC ====`) are ignored.
    pub fn from_text(text: &str) -> Result<Self, String> {
        #[derive(PartialEq)]
        enum Section {
            None,
            Map,
            Loc,
            Other,
        }
        let mut tiles: Box<[[[RawTile; REGION_SIZE]; REGION_SIZE]; REGION_LEVELS]> =
            Box::new([[[RawTile::default(); REGION_SIZE]; REGION_SIZE]; REGION_LEVELS]);
        let mut locs = Vec::new();
        let mut section = Section::None;

        for (n, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(header) = line.strip_prefix("==== ") {
                section = match header.trim_end_matches(" ====").trim() {
                    "MAP" => Section::Map,
                    "LOC" => Section::Loc,
                    _ => Section::Other,
                };
                continue;
            }
            let err = |m: &str| format!("line {}: {m}: {line:?}", n + 1);
            match section {
                Section::Map => {
                    let (coord, rest) = line.split_once(':').ok_or_else(|| err("missing ':'"))?;
                    let (level, x, z) = parse_coord(coord).map_err(|m| err(&m))?;
                    let tile = &mut tiles[level][x][z];
                    for tok in rest.split_whitespace() {
                        let (tag, val) = tok.split_at(1);
                        match tag {
                            "h" => tile.height = Some(val.parse().map_err(|_| err("bad height"))?),
                            "f" => tile.flags = Some(val.parse().map_err(|_| err("bad flags"))?),
                            "u" => tile.underlay = Some(val.parse().map_err(|_| err("bad underlay"))?),
                            "o" => {
                                let mut parts = val.split(';');
                                let id = parts
                                    .next()
                                    .and_then(|p| p.parse().ok())
                                    .ok_or_else(|| err("bad overlay id"))?;
                                let shape = parts.next().map_or(Ok(0), |p| p.parse())
                                    .map_err(|_| err("bad overlay shape"))?;
                                let rotation = parts.next().map_or(Ok(0), |p| p.parse())
                                    .map_err(|_| err("bad overlay rotation"))?;
                                tile.overlay = Some(RawOverlay { id, shape, rotation });
                            }
                            _ => return Err(err("unknown land token")),
                        }
                    }
                }
                Section::Loc => {
                    let (coord, rest) = line.split_once(':').ok_or_else(|| err("missing ':'"))?;
                    let (level, x, z) = parse_coord(coord).map_err(|m| err(&m))?;
                    let mut it = rest.split_whitespace();
                    let id = it.next().and_then(|p| p.parse().ok())
                        .ok_or_else(|| err("bad loc id"))?;
                    let shape = it.next().and_then(|p| p.parse().ok())
                        .ok_or_else(|| err("bad loc shape"))?;
                    let rotation = it.next().map_or(Ok(0), str::parse)
                        .map_err(|_| err("bad loc angle"))?;
                    locs.push(LocPlacement {
                        id,
                        level: level as u8,
                        x: x as u8,
                        z: z as u8,
                        shape,
                        rotation,
                    });
                }
                Section::None | Section::Other => {}
            }
        }
        Ok(Self { tiles, locs })
    }
}

/// Parse a `"{level} {x} {z}"` coordinate prefix, validating the ranges.
fn parse_coord(coord: &str) -> Result<(usize, usize, usize), String> {
    let mut it = coord.split_whitespace();
    let mut next = |what: &str| -> Result<usize, String> {
        it.next()
            .ok_or_else(|| format!("missing {what}"))?
            .parse()
            .map_err(|_| format!("bad {what}"))
    };
    let level = next("level")?;
    let x = next("x")?;
    let z = next("z")?;
    if level >= REGION_LEVELS || x >= REGION_SIZE || z >= REGION_SIZE {
        return Err("coord out of range".into());
    }
    Ok((level, x, z))
}

/// Decode the binary land stream into raw per-tile data (no perlin fill) — the
/// inverse of [`encode_land`], matching the parent module's `decode_tile`.
fn decode_land(bytes: &[u8]) -> Box<[[[RawTile; REGION_SIZE]; REGION_SIZE]; REGION_LEVELS]> {
    let mut tiles: Box<[[[RawTile; REGION_SIZE]; REGION_SIZE]; REGION_LEVELS]> =
        Box::new([[[RawTile::default(); REGION_SIZE]; REGION_SIZE]; REGION_LEVELS]);
    let mut p = Packet::from_vec(bytes.to_vec());
    for level in 0..REGION_LEVELS {
        for x in 0..REGION_SIZE {
            for z in 0..REGION_SIZE {
                let tile = &mut tiles[level][x][z];
                loop {
                    let code = p.g1();
                    if code == 0 {
                        break;
                    }
                    if code == 1 {
                        // rev1 client: a stored byte of 1 means height 0.
                        let mut h = p.g1();
                        if h == 1 {
                            h = 0;
                        }
                        tile.height = Some(h);
                        break;
                    }
                    if code <= 49 {
                        tile.overlay = Some(RawOverlay {
                            id: p.g1() as u8,
                            shape: ((code - 2) / 4) as u8,
                            rotation: ((code - 2) & 0x3) as u8,
                        });
                    } else if code <= 81 {
                        tile.flags = Some((code - 49) as u8);
                    } else {
                        tile.underlay = Some((code - 81) as u8);
                    }
                }
            }
        }
    }
    tiles
}

/// Encode raw per-tile land data into the binary stream. Emits opcodes in the
/// canonical order (overlay, flags, underlay, then the height/terminator) the
/// original Jagex encoder uses, so a decode→encode round-trip is byte-exact.
fn encode_land(tiles: &[[[RawTile; REGION_SIZE]; REGION_SIZE]; REGION_LEVELS]) -> Vec<u8> {
    let mut p = Packet::new(4096);
    for level in 0..REGION_LEVELS {
        for x in 0..REGION_SIZE {
            for z in 0..REGION_SIZE {
                let tile = &tiles[level][x][z];
                if let Some(o) = tile.overlay {
                    p.p1(2 + ((o.shape as i32) << 2) + o.rotation as i32);
                    p.p1(o.id as i32);
                }
                if let Some(f) = tile.flags {
                    p.p1(f as i32 + 49);
                }
                if let Some(u) = tile.underlay {
                    p.p1(u as i32 + 81);
                }
                match tile.height {
                    // Inverse of the `1 → 0` decode magic.
                    Some(h) => {
                        p.p1(1);
                        p.p1(if h == 0 { 1 } else { h });
                    }
                    None => p.p1(0),
                }
            }
        }
    }
    p.data[..p.pos].to_vec()
}

/// Encode loc placements into the binary stream — the inverse of
/// [`super::decode_locs`]. Locs are sorted by id then packed position and
/// delta-encoded, exactly as Engine-TS's packer (and the original cache) do.
fn encode_locs(locs: &[LocPlacement]) -> Vec<u8> {
    let mut sorted: Vec<&LocPlacement> = locs.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| packed_pos(a).cmp(&packed_pos(b))));

    let mut p = Packet::new(4096);
    let mut last_id: i32 = -1;
    let mut i = 0;
    while i < sorted.len() {
        let id = sorted[i].id;
        p.psmart(id - last_id);
        last_id = id;
        let mut last_pos: i32 = 0;
        while i < sorted.len() && sorted[i].id == id {
            let loc = sorted[i];
            let pos = packed_pos(loc);
            p.psmart(pos - last_pos + 1);
            last_pos = pos;
            p.p1(((loc.shape as i32) << 2) | loc.rotation as i32);
            i += 1;
        }
        p.psmart(0); // end of this loc id's placements
    }
    p.psmart(0); // end of region
    p.data[..p.pos].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A region with a spread of land features and locs, covering every opcode path.
    fn sample() -> RawRegion {
        let mut tiles: Box<[[[RawTile; REGION_SIZE]; REGION_SIZE]; REGION_LEVELS]> =
            Box::new([[[RawTile::default(); REGION_SIZE]; REGION_SIZE]; REGION_LEVELS]);
        // Explicit non-zero height + underlay.
        tiles[0][0][0] = RawTile {
            height: Some(40),
            underlay: Some(7),
            ..Default::default()
        };
        // Explicit height 0 (the `1 → 0` magic path) + flags.
        tiles[0][1][2] = RawTile {
            height: Some(0),
            flags: Some(3),
            ..Default::default()
        };
        // Overlay with shape + rotation, an underlay, no explicit height (perlin).
        tiles[1][5][9] = RawTile {
            overlay: Some(RawOverlay { id: 12, shape: 11, rotation: 2 }),
            underlay: Some(20),
            ..Default::default()
        };
        // Overlay with shape only (rotation 0).
        tiles[2][63][63] = RawTile {
            overlay: Some(RawOverlay { id: 5, shape: 4, rotation: 0 }),
            ..Default::default()
        };
        // Overlay flat (shape 0, rotation 0).
        tiles[3][10][10] = RawTile {
            overlay: Some(RawOverlay { id: 9, shape: 0, rotation: 0 }),
            ..Default::default()
        };

        let locs = vec![
            LocPlacement { id: 3, level: 0, x: 0, z: 0, shape: 10, rotation: 0 },
            LocPlacement { id: 3, level: 0, x: 0, z: 1, shape: 10, rotation: 3 },
            LocPlacement { id: 50, level: 1, x: 20, z: 40, shape: 22, rotation: 1 },
            LocPlacement { id: 1200, level: 0, x: 5, z: 5, shape: 0, rotation: 2 },
        ];
        RawRegion { tiles, locs }
    }

    fn sorted_locs(r: &RawRegion) -> Vec<LocPlacement> {
        let mut v = r.locs.clone();
        v.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| packed_pos(a).cmp(&packed_pos(b))));
        v
    }

    #[test]
    fn binary_codec_round_trips() {
        let region = sample();
        let land = region.encode_land();
        let loc = region.encode_locs();
        let back = RawRegion::decode(&land, Some(&loc));
        assert_eq!(region.tiles, back.tiles, "land round-trips through binary");
        assert_eq!(sorted_locs(&region), sorted_locs(&back), "locs round-trip through binary");
        // And the binary is stable: re-encoding the decoded form is byte-identical.
        assert_eq!(land, back.encode_land(), "land bytes are stable");
        assert_eq!(loc, back.encode_locs(), "loc bytes are stable");
    }

    #[test]
    fn text_round_trips() {
        let region = sample();
        let text = region.to_text();
        assert!(text.contains("==== MAP ===="));
        assert!(text.contains("==== LOC ===="));
        let back = RawRegion::from_text(&text).expect("parse");
        assert_eq!(region.tiles, back.tiles, "land round-trips through text");
        assert_eq!(sorted_locs(&region), sorted_locs(&back), "locs round-trip through text");
        // Text → binary matches the direct binary encode (the real goal: edit as
        // text, repack to the exact bytes).
        assert_eq!(region.encode_land(), back.encode_land(), "text→land bytes match");
        assert_eq!(region.encode_locs(), back.encode_locs(), "text→loc bytes match");
    }

    #[test]
    fn text_is_stable_across_a_second_round() {
        let text = sample().to_text();
        let reparsed = RawRegion::from_text(&text).unwrap().to_text();
        assert_eq!(text, reparsed, "to_text∘from_text is idempotent");
    }

    #[test]
    fn empty_region_round_trips() {
        let empty = RawRegion {
            tiles: Box::new([[[RawTile::default(); REGION_SIZE]; REGION_SIZE]; REGION_LEVELS]),
            locs: Vec::new(),
        };
        let land = empty.encode_land();
        // Every tile emits a single 0 terminator.
        assert_eq!(land.len(), REGION_LEVELS * REGION_SIZE * REGION_SIZE);
        assert!(land.iter().all(|&b| b == 0));
        let back = RawRegion::decode(&land, Some(&empty.encode_locs()));
        assert_eq!(empty.tiles, back.tiles);
        assert!(back.locs.is_empty());
    }
}
