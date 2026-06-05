//! BZip2 decompression for Jagex cache payloads.
//!
//! The rev1 client's `jagex3.io.BZip2` decodes blocks **without** the standard `BZh*` file
//! header — Jagex strips it to save 4 bytes per group. The `bzip2` crate expects the header,
//! so we prepend `BZh1` (block size 1 = 100kB, matching Jagex) before delegating.

use std::io::Read;

/// Decompress a header-less BZip2 stream. Returns the decoded bytes.
///
/// Panics if the underlying bzip2 decoder rejects the input — cache data is trusted so a
/// malformed stream indicates either disk corruption or a port bug, not user input.
#[must_use]
pub fn decompress(src: &[u8]) -> Vec<u8> {
    let mut full = Vec::with_capacity(src.len() + 4);
    full.extend_from_slice(b"BZh1");
    full.extend_from_slice(src);
    let mut out = Vec::new();
    bzip2::read::BzDecoder::new(&full[..])
        .read_to_end(&mut out)
        .expect("bzip2 decompress");
    out
}
