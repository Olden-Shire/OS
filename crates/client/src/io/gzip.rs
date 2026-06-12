// jagex3.io.GZip — thin wrapper around the flate2 inflate backend.

#![allow(dead_code)]

use std::io::Read;

use flate2::read::GzDecoder;

use super::packet::Packet;

pub struct GZip;

impl GZip {
    pub fn new() -> Self {
        Self
    }

    // GZip.decompress(Packet src, byte[] dst)
    pub fn decompress(&self, src: &mut Packet, dst: &mut [u8]) -> std::io::Result<()> {
        let remainder = &src.data[src.pos as usize..];
        let mut decoder = GzDecoder::new(remainder);
        decoder.read_exact(dst)
    }
}

impl Default for GZip {
    fn default() -> Self {
        Self::new()
    }
}
