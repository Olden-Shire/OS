//! GZip decompression for Jagex cache payloads (mirrors `jagex3.io.GZip`).

use std::io::Read;

/// Decompress a standard GZip stream. Returns the decoded bytes.
#[must_use]
pub fn decompress(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(src)
        .read_to_end(&mut out)
        .expect("gzip decompress");
    out
}
