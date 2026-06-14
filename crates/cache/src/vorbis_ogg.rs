//! Jagex vorbis container ↔ standard Ogg/Vorbis.
//!
//! Archive 14 stores mono Vorbis audio with the Ogg framing stripped
//! (`jag::oldscape::sound::JagVorbis` — see the client's
//! `sound/jagvorbis/mod.rs` for the reference decoder):
//!
//! * group 0 / file 0 — the SHARED setup header every sample references:
//!   one prefix byte (`log2(blocksize0)` low nibble, `log2(blocksize1)`
//!   high nibble) followed by a byte-aligned, bit-identical copy of a
//!   standard Vorbis setup-header body (codebooks/floors/residues/
//!   mappings/modes, LSB-first — the `BCV` codebook sync sits at offset
//!   1..4). It stays on disk as `.dat`.
//! * every other group — one sample: `[rate g4][sampleCount g4]
//!   [loopStart g4][loopEnd g4 — bitwise-NOT when the sample loops]
//!   [nPackets g4]` then each raw audio packet length-prefixed in
//!   255-byte chunks.
//!
//! `to_ogg` wraps a sample in a real `.ogg`: synthesized id header
//! (mono, rate, blocksize byte = the shared header's prefix byte),
//! comment header carrying the container ints (`SAMPLECOUNT`,
//! `LOOPSTART`, `LOOPEND`, `LOOPED`), and the shared setup body as the
//! third header packet (`\x05vorbis` + group0[1..], framing bit intact
//! since the body is bit-identical to libvorbis output). `from_ogg`
//! recovers exactly the fields `encode_jag` needs, so
//! `encode_jag(from_ogg(to_ogg(x))) == x` byte-for-byte — verified per
//! sample before the content tree is converted.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

// ── Jagex container ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JagSample {
    pub sample_rate: i32,
    pub sample_count: i32,
    pub loop_start: i32,
    /// Raw field — bitwise-NOT of the loop end when `looped`.
    pub loop_end_raw: i32,
    pub packets: Vec<Vec<u8>>,
}

impl JagSample {
    pub fn looped(&self) -> bool {
        self.loop_end_raw < 0
    }

    /// Mirror of the client's `JagVorbis::decode`.
    pub fn parse(src: &[u8]) -> Result<Self, String> {
        let mut pos = 0usize;
        let mut g4 = |src: &[u8]| -> Result<i32, String> {
            let b = src.get(pos..pos + 4).ok_or("truncated header")?;
            pos += 4;
            Ok(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        };
        let sample_rate = g4(src)?;
        let sample_count = g4(src)?;
        let loop_start = g4(src)?;
        let loop_end_raw = g4(src)?;
        let n_packets = g4(src)?;
        if !(0..=1_000_000).contains(&n_packets) {
            return Err(format!("implausible packet count {n_packets}"));
        }
        let mut packets = Vec::with_capacity(n_packets as usize);
        for _ in 0..n_packets {
            let mut len = 0usize;
            loop {
                let b = *src.get(pos).ok_or("truncated packet length")?;
                pos += 1;
                len += b as usize;
                if b < 255 {
                    break;
                }
            }
            let data = src.get(pos..pos + len).ok_or("truncated packet")?;
            pos += len;
            packets.push(data.to_vec());
        }
        if pos != src.len() {
            return Err(format!("{} trailing bytes after packets", src.len() - pos));
        }
        Ok(Self { sample_rate, sample_count, loop_start, loop_end_raw, packets })
    }

    /// Exact inverse of [`Self::parse`].
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.sample_rate.to_be_bytes());
        out.extend_from_slice(&self.sample_count.to_be_bytes());
        out.extend_from_slice(&self.loop_start.to_be_bytes());
        out.extend_from_slice(&self.loop_end_raw.to_be_bytes());
        out.extend_from_slice(&(self.packets.len() as i32).to_be_bytes());
        for p in &self.packets {
            let mut len = p.len();
            while len >= 255 {
                out.push(255);
                len -= 255;
            }
            out.push(len as u8);
            out.extend_from_slice(p);
        }
        out
    }
}

// ── Ogg framing (hand-rolled — deterministic writes, tolerant reads) ────

/// Ogg page CRC: poly 0x04C11DB7, no reflection, init/xorout 0.
fn ogg_crc(data: &[u8]) -> u32 {
    let mut crc: u32 = 0;
    for &b in data {
        crc ^= (b as u32) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 { (crc << 1) ^ 0x04C1_1DB7 } else { crc << 1 };
        }
    }
    crc
}

struct OggWriter {
    out: Vec<u8>,
    serial: u32,
    page_seq: u32,
    // Pending lacing values + payload for the page under construction.
    lacing: Vec<u8>,
    payload: Vec<u8>,
    first_page: bool,
}

impl OggWriter {
    fn new(serial: u32) -> Self {
        Self { out: Vec::new(), serial, page_seq: 0, lacing: Vec::new(), payload: Vec::new(), first_page: true }
    }

    /// Queue one whole packet (we never split packets across pages: every
    /// sample packet is well under one page's 255×255 capacity).
    fn packet(&mut self, data: &[u8], flush_after: bool, granule: u64, eos: bool) {
        let mut len = data.len();
        while len >= 255 {
            self.lacing.push(255);
            len -= 255;
        }
        self.lacing.push(len as u8);
        self.payload.extend_from_slice(data);
        // Keep headroom so a following packet's lacing always fits.
        if flush_after || self.lacing.len() > 200 {
            self.flush_page(granule, eos);
        }
    }

    fn flush_page(&mut self, granule: u64, eos: bool) {
        if self.lacing.is_empty() {
            return;
        }
        let mut page = Vec::with_capacity(27 + self.lacing.len() + self.payload.len());
        page.extend_from_slice(b"OggS");
        page.push(0); // version
        let mut htype = 0u8;
        if self.first_page {
            htype |= 0x02; // BOS
        }
        if eos {
            htype |= 0x04;
        }
        page.push(htype);
        page.extend_from_slice(&granule.to_le_bytes());
        page.extend_from_slice(&self.serial.to_le_bytes());
        page.extend_from_slice(&self.page_seq.to_le_bytes());
        page.extend_from_slice(&[0, 0, 0, 0]); // crc placeholder
        page.push(self.lacing.len() as u8);
        page.extend_from_slice(&self.lacing);
        page.extend_from_slice(&self.payload);
        let crc = ogg_crc(&page);
        page[22..26].copy_from_slice(&crc.to_le_bytes());
        self.out.extend_from_slice(&page);
        self.page_seq += 1;
        self.first_page = false;
        self.lacing.clear();
        self.payload.clear();
    }
}

/// Extract the logical packets of the first (only) stream in an Ogg file.
/// Handles packets continued across pages; nil packets preserved.
fn ogg_packets(src: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    let mut packets: Vec<Vec<u8>> = Vec::new();
    let mut partial: Vec<u8> = Vec::new();
    let mut open = false;
    let mut pos = 0usize;
    while pos + 27 <= src.len() {
        if &src[pos..pos + 4] != b"OggS" {
            return Err(format!("bad page capture at {pos}"));
        }
        let nseg = src[pos + 26] as usize;
        let lacing = src.get(pos + 27..pos + 27 + nseg).ok_or("truncated lacing")?;
        let mut body = pos + 27 + nseg;
        for (i, &l) in lacing.iter().enumerate() {
            let seg = src.get(body..body + l as usize).ok_or("truncated segment")?;
            partial.extend_from_slice(seg);
            open = true;
            body += l as usize;
            let _ = i;
            if l < 255 {
                packets.push(std::mem::take(&mut partial));
                open = false;
            }
        }
        pos = body;
    }
    if open {
        return Err("file ends mid-packet".into());
    }
    Ok(packets)
}

// ── Sample ↔ .ogg ───────────────────────────────────────────────────────

fn comment(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

/// Build a standard mono Ogg/Vorbis file for `sample`. `shared_setup` is
/// the vorbis archive's group-0 payload (prefix byte + setup body).
pub fn to_ogg(sample: &JagSample, shared_setup: &[u8], serial: u32) -> Result<Vec<u8>, String> {
    if shared_setup.len() < 2 {
        return Err("shared setup header too short".into());
    }
    // Identification header.
    let mut id = Vec::with_capacity(30);
    id.extend_from_slice(b"\x01vorbis");
    id.extend_from_slice(&0u32.to_le_bytes()); // vorbis version
    id.push(1); // channels (rev1 samples are mono)
    id.extend_from_slice(&(sample.sample_rate as u32).to_le_bytes());
    id.extend_from_slice(&0i32.to_le_bytes()); // bitrate max
    id.extend_from_slice(&0i32.to_le_bytes()); // bitrate nominal
    id.extend_from_slice(&0i32.to_le_bytes()); // bitrate min
    id.push(shared_setup[0]); // blocksize nibbles — same layout as the id header's
    id.push(0x01); // framing

    // Comment header — carries the container ints the .ogg has no slot for.
    let mut cm = Vec::new();
    cm.extend_from_slice(b"\x03vorbis");
    let vendor = "OS jag-rev1 vorbis";
    cm.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    cm.extend_from_slice(vendor.as_bytes());
    let (loop_end, looped) = if sample.loop_end_raw < 0 {
        (!sample.loop_end_raw, 1)
    } else {
        (sample.loop_end_raw, 0)
    };
    cm.extend_from_slice(&4u32.to_le_bytes());
    comment(&mut cm, &format!("SAMPLECOUNT={}", sample.sample_count));
    comment(&mut cm, &format!("LOOPSTART={}", sample.loop_start));
    comment(&mut cm, &format!("LOOPEND={loop_end}"));
    comment(&mut cm, &format!("LOOPED={looped}"));
    cm.push(0x01); // framing

    // Setup header — the shared body is bit-identical to libvorbis output
    // (framing bit included), so a byte-aligned splice is a valid packet.
    let mut setup = Vec::with_capacity(7 + shared_setup.len() - 1);
    setup.extend_from_slice(b"\x05vorbis");
    setup.extend_from_slice(&shared_setup[1..]);

    let mut w = OggWriter::new(serial);
    w.packet(&id, true, 0, false); // BOS page: id header alone (spec)
    w.packet(&cm, false, 0, false);
    w.packet(&setup, true, 0, false); // headers end their page (spec)
    let n = sample.packets.len();
    if n == 0 {
        // Degenerate but representable: emit an empty EOS page.
        w.flush_page(sample.sample_count as u64, true);
    }
    for (i, p) in sample.packets.iter().enumerate() {
        let last = i == n - 1;
        // Granule = final PCM position. Intermediate pages reuse it (we
        // don't decode packet modes to track exact positions; duration
        // and audition behave, and the round trip never reads granules).
        w.packet(p, last, sample.sample_count as u64, last);
    }
    Ok(w.out)
}

/// Recover a [`JagSample`] from an Ogg built by [`to_ogg`] (or re-saved by
/// standard tools, as long as the comment fields survive).
pub fn from_ogg(src: &[u8]) -> Result<JagSample, String> {
    let packets = ogg_packets(src)?;
    if packets.len() < 3 {
        return Err("ogg has no audio packets".into());
    }
    let id = &packets[0];
    if id.len() < 30 || &id[0..7] != b"\x01vorbis" {
        return Err("first packet is not a vorbis id header".into());
    }
    let channels = id[11];
    if channels != 1 {
        return Err(format!("expected mono, got {channels} channels"));
    }
    let sample_rate = u32::from_le_bytes([id[12], id[13], id[14], id[15]]) as i32;

    let cm = &packets[1];
    if cm.len() < 7 || &cm[0..7] != b"\x03vorbis" {
        return Err("second packet is not a vorbis comment header".into());
    }
    let mut fields: BTreeMap<String, i64> = BTreeMap::new();
    let mut pos = 7usize;
    let rd_u32 = |b: &[u8], p: usize| -> Result<u32, String> {
        b.get(p..p + 4)
            .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
            .ok_or_else(|| "truncated comment header".into())
    };
    let vendor_len = rd_u32(cm, pos)? as usize;
    pos += 4 + vendor_len;
    let count = rd_u32(cm, pos)?;
    pos += 4;
    for _ in 0..count {
        let len = rd_u32(cm, pos)? as usize;
        pos += 4;
        let s = cm.get(pos..pos + len).ok_or("truncated comment")?;
        pos += len;
        if let Ok(s) = std::str::from_utf8(s)
            && let Some((k, v)) = s.split_once('=')
            && let Ok(v) = v.parse::<i64>()
        {
            fields.insert(k.to_ascii_uppercase(), v);
        }
    }
    let get = |k: &str| -> Result<i32, String> {
        fields.get(k).map(|&v| v as i32).ok_or_else(|| format!("missing {k}= comment"))
    };
    let sample_count = get("SAMPLECOUNT")?;
    let loop_start = get("LOOPSTART")?;
    let loop_end = get("LOOPEND")?;
    let looped = get("LOOPED")? != 0;
    let loop_end_raw = if looped { !loop_end } else { loop_end };

    // packets[2] is the setup header (the shared body) — ignored; the pack
    // side sources the header group from its own .dat.
    Ok(JagSample {
        sample_rate,
        sample_count,
        loop_start,
        loop_end_raw,
        packets: packets[3..].to_vec(),
    })
}

// ── Content-tree conversion (post-unpack pass, idempotent) ──────────────

#[derive(Debug, Default)]
pub struct OggStats {
    pub converted: u32,
    pub already: u32,
    pub kept_dat: u32,
}

/// Convert every sample group under `vorbis_dir` from `.dat` to `.ogg`,
/// updating `_meta.json` in place. Group 0 (the shared setup header) stays
/// `.dat`. Each conversion is gated on `encode(from_ogg(to_ogg(x))) == x`.
pub fn convert_vorbis_dir(vorbis_dir: &Path) -> std::io::Result<OggStats> {
    use crate::content::manifest::ArchiveManifest;
    let meta_path = vorbis_dir.join("_meta.json");
    let mut manifest: ArchiveManifest = serde_json::from_slice(&fs::read(&meta_path)?)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{meta_path:?}: {e}")))?;

    let shared_setup = manifest
        .groups
        .iter()
        .find(|g| g.id == 0)
        .map(|g| fs::read(vorbis_dir.join(&g.path)))
        .transpose()?
        .unwrap_or_default();

    let mut stats = OggStats::default();
    for g in &mut manifest.groups {
        let p = Path::new(&g.path);
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext.eq_ignore_ascii_case("ogg") {
            stats.already += 1;
            continue;
        }
        if g.id == 0 || !ext.eq_ignore_ascii_case("dat") || g.file_ids.is_some() {
            continue;
        }
        let dat_path = vorbis_dir.join(&g.path);
        let raw = fs::read(&dat_path)?;
        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();

        let ok = (|| -> Result<Vec<u8>, String> {
            let sample = JagSample::parse(&raw)?;
            if sample.encode() != raw {
                return Err("binary round-trip mismatch".into());
            }
            let ogg = to_ogg(&sample, &shared_setup, g.id)?;
            let back = from_ogg(&ogg)?;
            if back.encode() != raw {
                return Err("ogg round-trip mismatch".into());
            }
            Ok(ogg)
        })();
        match ok {
            Ok(ogg) => {
                fs::write(vorbis_dir.join(format!("{stem}.ogg")), &ogg)?;
                fs::remove_file(&dat_path)?;
                g.path = format!("{stem}.ogg");
                stats.converted += 1;
            }
            Err(e) => {
                eprintln!("[vorbis-ogg] group {} ({}): {e} — keeping .dat", g.id, g.path);
                stats.kept_dat += 1;
            }
        }
    }

    let json = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    fs::write(&meta_path, json)?;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(packets: Vec<Vec<u8>>, loop_end_raw: i32) -> JagSample {
        JagSample {
            sample_rate: 22050,
            sample_count: 29652,
            loop_start: 23323,
            loop_end_raw,
            packets,
        }
    }

    // A fake shared header: blocksize byte + an arbitrary "setup body".
    const SETUP: &[u8] = &[0xAA, 0x42, 0x43, 0x56, 0x01, 0x02, 0x03];

    #[test]
    fn jag_container_round_trips() {
        let s = sample(vec![vec![0x20; 10], vec![0x55; 300], vec![]], !29000);
        let bytes = s.encode();
        let back = JagSample::parse(&bytes).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.encode(), bytes);
        assert!(back.looped());
    }

    #[test]
    fn ogg_round_trips_exactly() {
        // Includes a 255-multiple packet (lacing edge: needs the
        // explicit 0-length terminator) and an empty packet.
        let s = sample(vec![vec![1; 255], vec![], vec![9; 700], vec![3, 1, 4]], 29000);
        let ogg = to_ogg(&s, SETUP, 7).unwrap();
        assert_eq!(&ogg[0..4], b"OggS");
        let back = from_ogg(&ogg).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.encode(), s.encode());
    }

    #[test]
    fn ogg_preserves_loop_flag() {
        let s = sample(vec![vec![7; 32]], !12345);
        let back = from_ogg(&to_ogg(&s, SETUP, 1).unwrap()).unwrap();
        assert_eq!(back.loop_end_raw, !12345);
        assert!(back.looped());
    }

    #[test]
    fn ogg_pages_carry_valid_crcs() {
        let s = sample(vec![vec![0xAB; 4000]], 0);
        let ogg = to_ogg(&s, SETUP, 3).unwrap();
        // Walk pages, re-CRC each, compare against the stored field.
        let mut pos = 0usize;
        let mut pages = 0;
        while pos + 27 <= ogg.len() {
            assert_eq!(&ogg[pos..pos + 4], b"OggS");
            let nseg = ogg[pos + 27 - 1] as usize;
            let body_len: usize = ogg[pos + 27..pos + 27 + nseg].iter().map(|&l| l as usize).sum();
            let end = pos + 27 + nseg + body_len;
            let mut page = ogg[pos..end].to_vec();
            let stored = u32::from_le_bytes([page[22], page[23], page[24], page[25]]);
            page[22..26].fill(0);
            assert_eq!(ogg_crc(&page), stored, "page {pages} crc");
            pos = end;
            pages += 1;
        }
        assert!(pages >= 2);
    }
}
