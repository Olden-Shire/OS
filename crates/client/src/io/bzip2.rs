// jagex3.io.BZip2 — JS5 streams BZip2-compressed groups with the `BZh` magic
// stripped (Jagex saves 4 bytes by omitting it). We prepend it back before
// handing the buffer to bzip2-rs.

#![allow(dead_code)]

use std::io::Read;

use bzip2_rs::DecoderReader;

// BZip2.decompress(byte[] dst, int ulen, byte[] src, int clen, int blockSize)
pub fn decompress(dst: &mut [u8], _ulen: i32, src: &[u8], clen: i32, _block_size: i32) -> std::io::Result<()> {
    // Java passes `src` starting at the JS5 compressed-payload offset (9 bytes
    // into the group: 1 ctype + 4 clen + 4 ulen). The compressed payload is
    // missing the standard `BZh<blocksize>` 4-byte header.
    let payload = &src[9..(9 + clen as usize)];
    let mut buf = Vec::with_capacity(payload.len() + 4);
    buf.extend_from_slice(b"BZh1");
    buf.extend_from_slice(payload);
    let mut decoder = DecoderReader::new(&buf[..]);
    decoder.read_exact(dst).map_err(|e| std::io::Error::other(e))
}
