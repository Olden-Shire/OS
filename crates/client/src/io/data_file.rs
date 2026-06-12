// @ObfuscatedName("ap")
//
// jagex3.io.DataFile — Js5 on-disk cache (idx + dat files). The port
// currently runs without a disk cache: readFromFile returns None so Js5Loader
// downloads everything from the server, and writeToFile is a no-op. The full
// BufferedRandomAccessFile-backed implementation will land alongside the
// jaged cache crate.

#![allow(dead_code)]

pub struct DataFile {
    // @ObfuscatedName("ap.r")
    pub temp: [u8; 520],

    // @ObfuscatedName("ap.d") — BufferedRandomAccessFile idx, omitted.
    pub idx: (),

    // @ObfuscatedName("ap.l") — BufferedRandomAccessFile dat, omitted.
    pub dat: (),

    // @ObfuscatedName("ap.m")
    pub archive: i32,

    // @ObfuscatedName("ap.c")
    pub max_file_size: i32,
}

impl DataFile {
    pub fn new(archive: i32, max_file_size: i32) -> Self {
        Self {
            temp: [0; 520],
            idx: (),
            dat: (),
            archive,
            max_file_size,
        }
    }

    // @ObfuscatedName("ap.r(II)[B")
    pub fn read_from_file(&mut self, _arg0: i32) -> Option<Vec<u8>> {
        None
    }

    // @ObfuscatedName("ap.d(I[BII)Z")
    pub fn write_to_file(&mut self, _arg0: i32, _arg1: &[u8], _arg2: i32) -> bool {
        true
    }
}
