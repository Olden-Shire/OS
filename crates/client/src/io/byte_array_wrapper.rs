// jagex3.io.ByteArrayWrapper — Java helper that optionally wraps a byte[]
// in a soft reference / GZIP recompresses it. JS5 calls `wrap(data, false)`
// (no recompression) and `unwrap(...)` to read it back. For the port we
// just round-trip the bytes through a Vec<u8>.

#![allow(dead_code)]

pub fn wrap(data: &[u8], _arg: bool) -> Vec<u8> {
    data.to_vec()
}

pub fn unwrap(data: &[u8], _arg: bool) -> Vec<u8> {
    data.to_vec()
}
