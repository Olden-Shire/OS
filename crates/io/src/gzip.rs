//! GZip decompression for Jagex cache payloads (mirrors `jagex3.io.GZip`).

use std::io::{Read, Write};

use crate::crc32;

/// Compress `src` as a GZip stream that's byte-identical to what Jagex's packer wrote for
/// every group in the rev1 cache. Two pieces of byte-level fidelity matter:
///
/// * **Header**: flate2's `GzEncoder` emits `xfl=02, OS=0xFF`; Jagex / Java's
///   `GZIPOutputStream` emits `xfl=00, OS=00`. We build the 10-byte header by hand.
/// * **Deflate body**: Jagex used the default compression level (`Z_DEFAULT_COMPRESSION` =
///   level 6), *not* level 9 as one might assume for an archived asset. zlib (vendored via
///   libz-sys) at level 6 reproduces all 36,709 gzip groups byte-for-byte.
///
/// `level` lets callers override the deflate level for non-cache use cases. For the rev1
/// cache repack, **always use level 6**.
#[must_use]
pub fn compress(src: &[u8], level: u32) -> Vec<u8> {
    let mut deflated = Vec::new();
    {
        let mut enc = flate2::write::DeflateEncoder::new(
            &mut deflated,
            flate2::Compression::new(level),
        );
        enc.write_all(src).expect("deflate write");
        enc.finish().expect("deflate finish");
    }

    let mut out = Vec::with_capacity(10 + deflated.len() + 8);
    // gzip header: magic (1F 8B) | method=deflate (08) | flags=0 | mtime=0 | xfl=0 | OS=0
    out.extend_from_slice(&[0x1F, 0x8B, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    out.extend_from_slice(&deflated);
    out.extend_from_slice(&crc32::checksum(src, 0, src.len()).to_le_bytes());
    out.extend_from_slice(&(src.len() as u32).to_le_bytes());
    out
}

/// Decompress a standard GZip stream. Returns the decoded bytes.
#[must_use]
pub fn decompress(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(src)
        .read_to_end(&mut out)
        .expect("gzip decompress");
    out
}
