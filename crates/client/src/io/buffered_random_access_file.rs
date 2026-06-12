// @ObfuscatedName("dq") — jag::oldscape::io::BufferedRandomAccessFile.
//
// Read+write buffer over FileOnDisk. The disk JS5 cache splits its idx
// and dat files between this and FileOnDisk — idx blocks (6 bytes
// each) cluster in a write-back buffer, dat reads use a sliding read
// buffer. We mirror the same data layout with `Vec<u8>` buffers.

#![allow(dead_code)]

use std::io::{Read, Seek, SeekFrom, Write};

use super::file_on_disk::FileOnDisk;

pub struct BufferedRandomAccessFile {
    pub file: FileOnDisk,
    // @ObfuscatedName("dq.r") — read buffer.
    pub read_buf: Vec<u8>,
    // @ObfuscatedName("dq.d") — write buffer.
    pub write_buf: Vec<u8>,
    // @ObfuscatedName("dq.l") — file position the read buffer covers.
    pub read_pos: i64,
}

impl BufferedRandomAccessFile {
    pub fn new(file: FileOnDisk, read_capacity: usize, write_capacity: usize) -> Self {
        Self {
            file,
            read_buf: Vec::with_capacity(read_capacity),
            write_buf: Vec::with_capacity(write_capacity),
            read_pos: 0,
        }
    }

    pub fn seek(&mut self, pos: i64) -> std::io::Result<()> {
        self.flush()?;
        self.read_buf.clear();
        self.read_pos = pos;
        self.file.seek(pos)
    }

    pub fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // Refill the read buffer if exhausted.
        if self.read_buf.is_empty() {
            let mut tmp = vec![0u8; self.read_buf.capacity().max(buf.len())];
            let n = self.file.read(&mut tmp)?;
            tmp.truncate(n);
            self.read_buf = tmp;
        }
        let n = buf.len().min(self.read_buf.len());
        buf[..n].copy_from_slice(&self.read_buf[..n]);
        self.read_buf.drain(..n);
        Ok(n)
    }

    pub fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.write_buf.extend_from_slice(buf);
        if self.write_buf.len() >= self.write_buf.capacity() {
            self.flush()?;
        }
        Ok(buf.len())
    }

    pub fn flush(&mut self) -> std::io::Result<()> {
        if !self.write_buf.is_empty() {
            let buf = std::mem::take(&mut self.write_buf);
            self.file.write(&buf)?;
            self.write_buf.clear();
        }
        self.file.flush()
    }

    // @ObfuscatedName("v.r(I)V") — BufferedRandomAccessFile.close.
    // Verbatim port of BufferedRandomAccessFile.java:54-57. Flushes
    // the write buffer to disk then closes the underlying file.
    pub fn close(mut self) -> std::io::Result<()> {
        self.flush()?;
        self.file.close()
    }

    // @ObfuscatedName("v.l(I)J") — BufferedRandomAccessFile.length.
    // Verbatim port of BufferedRandomAccessFile.java:68-70. Returns
    // the actual file length from the OS — the high watermark of
    // bytes written, accounting for the pending write buffer.
    pub fn length(&self) -> std::io::Result<i64> {
        self.file.file_length()
    }
}
