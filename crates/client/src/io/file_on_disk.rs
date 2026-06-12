// @ObfuscatedName("dd") — jag::oldscape::io::FileOnDisk.
//
// Thin RandomAccessFile wrapper used by the JS5 disk cache. Java
// guards every write against `max_length` so a runaway cache writer
// can't fill the disk. The sentinel-byte trick (writing a 0 to the
// final allowed position when first opened) reserves the file size
// up-front so subsequent seeks land in valid disk space.

#![allow(dead_code)]

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

pub struct FileOnDisk {
    pub file: File,
    // @ObfuscatedName("dd.r")
    pub max_length: i64,
    // @ObfuscatedName("dd.d") — current logical length (tracked manually
    // because RandomAccessFile in Java exposes .length()).
    pub length: i64,
}

impl FileOnDisk {
    // @ObfuscatedName("dd.<init>") — opens the path read-write, optionally
    // writing a single sentinel byte at `max_length - 1` to pre-allocate.
    pub fn open(path: &Path, max_length: i64) -> std::io::Result<Self> {
        let mut file = OpenOptions::new().read(true).write(true).create(true).open(path)?;
        let length = file.metadata()?.len() as i64;
        if length > max_length && max_length > 0 {
            file.seek(SeekFrom::Start((max_length - 1) as u64))?;
            file.write_all(&[0])?;
        }
        Ok(Self { file, max_length, length })
    }

    pub fn seek(&mut self, pos: i64) -> std::io::Result<()> {
        self.file.seek(SeekFrom::Start(pos as u64)).map(|_| ())
    }

    pub fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.file.read(buf)
    }

    pub fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let pos = self.file.stream_position()? as i64;
        if self.max_length > 0 && pos + buf.len() as i64 > self.max_length {
            return Err(std::io::Error::new(std::io::ErrorKind::Other,
                "FileOnDisk: exceed max_length"));
        }
        let n = self.file.write(buf)?;
        self.length = self.length.max(pos + n as i64);
        Ok(n)
    }

    pub fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }

    // @ObfuscatedName("u.m(I)J") — FileOnDisk.length. Verbatim port
    // of FileOnDisk.java:67-69. Returns the actual file length from
    // the OS rather than the tracked logical length (useful for
    // initial-size queries during cache mount).
    pub fn file_length(&self) -> std::io::Result<i64> {
        Ok(self.file.metadata()?.len() as i64)
    }

    // @ObfuscatedName("u.l(I)V") — FileOnDisk.close. Verbatim port of
    // FileOnDisk.java:59-64. Rust's Drop closes automatically; this
    // method exists for Java API parity (callers can be explicit
    // about closing before the wrapper drops).
    pub fn close(self) -> std::io::Result<()> {
        // Consuming self drops the File which closes the handle.
        let _ = self;
        Ok(())
    }
}
