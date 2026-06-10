//! Jagex byte buffer.
//!
//! Port of `jagex3.io.Packet` (+ `PacketBit` bit methods) from the rev1 Java client. Method
//! names are kept identical to the Java source — `p1`, `g1`, `g2b`, `psmart`, `p1_alt1`, etc.
//! — so grepping either tree leads to the same call.
//!
//! Differences from Java:
//! * Read methods that Java returns as `int` (i.e. unsigned-byte-as-int) return `i32` here too.
//!   The signed variants (`g1b`, `g2b`) keep their narrower signed return type.
//! * Write methods take `i32`/`i64` like Java and truncate to the on-wire width via `as`.
//! * Out-of-bounds reads panic (matches Java's `ArrayIndexOutOfBoundsException`).
//! * The buffer auto-grows on writes; reads do not grow it.

use crate::{cp1252, crc32};

const BITMASK: [u32; 33] = {
    let mut t = [0u32; 33];
    let mut i = 0;
    while i < 32 {
        t[i] = (1u32 << i) - 1;
        i += 1;
    }
    t[32] = 0xFFFF_FFFF;
    t
};

#[derive(Debug, Default)]
pub struct Packet {
    pub data: Vec<u8>,
    pub pos: usize,
    pub bit_pos: usize,
}

impl Packet {
    /// New packet with `capacity` bytes pre-allocated and zero-initialised.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self { data: vec![0; capacity], pos: 0, bit_pos: 0 }
    }

    /// Wrap an existing byte buffer for reading or writing. `pos` starts at 0.
    #[must_use]
    pub fn from_vec(data: Vec<u8>) -> Self {
        Self { data, pos: 0, bit_pos: 0 }
    }

    /// Grow the underlying buffer so that `self.pos + n` bytes are writable.
    fn ensure(&mut self, n: usize) {
        let need = self.pos + n;
        if self.data.len() < need {
            self.data.resize(need, 0);
        }
    }

    // ───── writes (standard, network byte order) ─────────────────────────────

    pub fn p1(&mut self, value: i32) {
        self.ensure(1);
        self.data[self.pos] = value as u8;
        self.pos += 1;
    }

    pub fn p2(&mut self, value: i32) {
        self.ensure(2);
        self.data[self.pos] = (value >> 8) as u8;
        self.data[self.pos + 1] = value as u8;
        self.pos += 2;
    }

    pub fn p3(&mut self, value: i32) {
        self.ensure(3);
        self.data[self.pos] = (value >> 16) as u8;
        self.data[self.pos + 1] = (value >> 8) as u8;
        self.data[self.pos + 2] = value as u8;
        self.pos += 3;
    }

    pub fn p4(&mut self, value: i32) {
        self.ensure(4);
        self.data[self.pos] = (value >> 24) as u8;
        self.data[self.pos + 1] = (value >> 16) as u8;
        self.data[self.pos + 2] = (value >> 8) as u8;
        self.data[self.pos + 3] = value as u8;
        self.pos += 4;
    }

    pub fn p8(&mut self, value: i64) {
        self.ensure(8);
        for i in 0..8 {
            self.data[self.pos + i] = (value >> (56 - i * 8)) as u8;
        }
        self.pos += 8;
    }

    pub fn pdata(&mut self, src: &[u8], offset: usize, len: usize) {
        self.ensure(len);
        self.data[self.pos..self.pos + len].copy_from_slice(&src[offset..offset + len]);
        self.pos += len;
    }

    // ───── reads (standard) ──────────────────────────────────────────────────

    pub fn g1(&mut self) -> i32 {
        let v = i32::from(self.data[self.pos]);
        self.pos += 1;
        v
    }

    pub fn g1b(&mut self) -> i8 {
        let v = self.data[self.pos] as i8;
        self.pos += 1;
        v
    }

    pub fn g2(&mut self) -> i32 {
        let v = (i32::from(self.data[self.pos]) << 8) | i32::from(self.data[self.pos + 1]);
        self.pos += 2;
        v
    }

    pub fn g2b(&mut self) -> i16 {
        let v = ((i32::from(self.data[self.pos]) << 8) | i32::from(self.data[self.pos + 1])) as i16;
        self.pos += 2;
        v
    }

    pub fn g3(&mut self) -> i32 {
        let v = (i32::from(self.data[self.pos]) << 16)
            | (i32::from(self.data[self.pos + 1]) << 8)
            | i32::from(self.data[self.pos + 2]);
        self.pos += 3;
        v
    }

    pub fn g4(&mut self) -> i32 {
        let v = (i32::from(self.data[self.pos]) << 24)
            | (i32::from(self.data[self.pos + 1]) << 16)
            | (i32::from(self.data[self.pos + 2]) << 8)
            | i32::from(self.data[self.pos + 3]);
        self.pos += 4;
        v
    }

    pub fn g8(&mut self) -> i64 {
        let hi = (self.g4() as u32) as i64;
        let lo = (self.g4() as u32) as i64;
        (hi << 32) | lo
    }

    pub fn gdata(&mut self, dst: &mut [u8], offset: usize, len: usize) {
        dst[offset..offset + len].copy_from_slice(&self.data[self.pos..self.pos + len]);
        self.pos += len;
    }

    // ───── strings ───────────────────────────────────────────────────────────

    /// Length in bytes that `pjstr(s)` would write (encoded chars + trailing NUL).
    #[must_use]
    pub fn pjstrlen(s: &str) -> usize {
        cp1252::encoded_len(s) + 1
    }

    /// Write a CP1252-encoded, null-terminated string. Panics if `s` contains a NUL.
    pub fn pjstr(&mut self, s: &str) {
        assert!(!s.contains('\0'), "pjstr: string contains embedded NUL");
        let n = cp1252::encoded_len(s);
        self.ensure(n + 1);
        self.pos += cp1252::encode(s, &mut self.data, self.pos);
        self.data[self.pos] = 0;
        self.pos += 1;
    }

    /// Read a null-terminated CP1252 string (NUL is consumed but not included in result).
    pub fn gjstr(&mut self) -> String {
        let start = self.pos;
        while self.data[self.pos] != 0 {
            self.pos += 1;
        }
        let len = self.pos - start;
        self.pos += 1;
        if len == 0 { String::new() } else { cp1252::decode(&self.data, start, len) }
    }

    /// Read a string preceded by a 0-byte marker (variant of `gjstr` used in some packets).
    pub fn gjstr2(&mut self) -> String {
        let marker = self.data[self.pos];
        self.pos += 1;
        assert_eq!(marker, 0, "gjstr2: expected leading 0 byte, got {marker}");
        self.gjstr()
    }

    /// `gjstr` but returns `None` if the string is just the leading NUL.
    pub fn fastgstr(&mut self) -> Option<String> {
        if self.data[self.pos] == 0 {
            self.pos += 1;
            None
        } else {
            Some(self.gjstr())
        }
    }

    /// Write a UTF-8 string with leading 0 byte + MIDI var-len length prefix.
    pub fn p_utf8(&mut self, s: &str) {
        let bytes = s.as_bytes();
        self.ensure(1);
        self.data[self.pos] = 0;
        self.pos += 1;
        self.p_midi_var_len(bytes.len() as i32);
        self.pdata(bytes, 0, bytes.len());
    }

    /// Read the leading-NUL + varlen + UTF-8 form written by `p_utf8`.
    ///
    /// Invalid sequences are replaced with U+FFFD, matching the Java client.
    pub fn g_utf8(&mut self) -> String {
        let marker = self.data[self.pos];
        self.pos += 1;
        assert_eq!(marker, 0, "g_utf8: expected leading 0 byte");
        let len = self.g_midi_var_len() as usize;
        let bytes = &self.data[self.pos..self.pos + len];
        let s = String::from_utf8_lossy(bytes).into_owned();
        self.pos += len;
        s
    }

    // ───── smart / variable-length ints ──────────────────────────────────────

    /// Write 1 or 2 bytes depending on range: `[0, 0x80)` → 1 byte; `[0, 0x8000)` → 2 bytes (with hi bit set).
    pub fn psmart(&mut self, n: i32) {
        assert!(n >= 0, "psmart: negative value {n}");
        if n < 0x80 {
            self.p1(n);
        } else if n < 0x8000 {
            self.p2(n + 0x8000);
        } else {
            panic!("psmart: out of range: {n}");
        }
    }

    /// Read 1 or 2 bytes (unsigned smart). High bit of first byte selects width.
    pub fn gsmart(&mut self) -> i32 {
        if self.data[self.pos] < 0x80 { self.g1() } else { self.g2() - 0x8000 }
    }

    /// Read 1 or 2 bytes as a *signed* smart (offsets are -64 / -16384).
    pub fn gsmarts(&mut self) -> i32 {
        if self.data[self.pos] < 0x80 { self.g1() - 64 } else { self.g2() - 49152 }
    }

    /// Read 2 or 4 bytes: high bit of first byte set ⇒ 4 bytes (with bit cleared); else 2.
    pub fn g_smart2or4(&mut self) -> i32 {
        if (self.data[self.pos] as i8) < 0 { self.g4() & 0x7FFF_FFFF } else { self.g2() }
    }

    /// MIDI-style variable-length quantity (7 bits/byte, MSB continuation flag).
    pub fn p_midi_var_len(&mut self, n: i32) {
        if (n & 0xFFFF_FF80_u32 as i32) != 0 {
            if (n & 0xFFFF_C000_u32 as i32) != 0 {
                if (n & 0xFFE0_0000_u32 as i32) != 0 {
                    if (n & 0xF000_0000_u32 as i32) != 0 {
                        self.p1(((n as u32) >> 28) as i32 | 0x80);
                    }
                    self.p1(((n as u32) >> 21) as i32 | 0x80);
                }
                self.p1(((n as u32) >> 14) as i32 | 0x80);
            }
            self.p1(((n as u32) >> 7) as i32 | 0x80);
        }
        self.p1(n & 0x7F);
    }

    pub fn g_midi_var_len(&mut self) -> i32 {
        let mut b = self.data[self.pos] as i8;
        self.pos += 1;
        let mut v = 0i32;
        while b < 0 {
            v = (v | i32::from(b) & 0x7F) << 7;
            b = self.data[self.pos] as i8;
            self.pos += 1;
        }
        v | i32::from(b)
    }

    // ───── backpatch size prefixes ───────────────────────────────────────────

    /// Backpatch a 1-byte length `len` that sits `len` bytes before the current `pos`.
    pub fn psize1(&mut self, len: i32) {
        self.data[self.pos - len as usize - 1] = len as u8;
    }

    pub fn psize2(&mut self, len: i32) {
        let p = self.pos - len as usize - 2;
        self.data[p] = (len >> 8) as u8;
        self.data[p + 1] = len as u8;
    }

    pub fn psize4(&mut self, len: i32) {
        let p = self.pos - len as usize - 4;
        self.data[p] = (len >> 24) as u8;
        self.data[p + 1] = (len >> 16) as u8;
        self.data[p + 2] = (len >> 8) as u8;
        self.data[p + 3] = len as u8;
    }

    // ───── alt byte/short/int variants (Jagex's "type A/C/S" obfuscation) ────
    // Type A = + 128, type C = -value, type S = 128 - value.

    pub fn p1_alt1(&mut self, value: i32) {
        self.ensure(1);
        self.data[self.pos] = (value as u8).wrapping_add(128);
        self.pos += 1;
    }
    pub fn p1_alt2(&mut self, value: i32) {
        self.ensure(1);
        self.data[self.pos] = (value as u8).wrapping_neg();
        self.pos += 1;
    }
    pub fn p1_alt3(&mut self, value: i32) {
        self.ensure(1);
        self.data[self.pos] = 128u8.wrapping_sub(value as u8);
        self.pos += 1;
    }

    pub fn g1_alt1(&mut self) -> i32 {
        let v = i32::from(self.data[self.pos].wrapping_sub(128));
        self.pos += 1;
        v
    }
    pub fn g1_alt2(&mut self) -> i32 {
        let v = i32::from(self.data[self.pos].wrapping_neg());
        self.pos += 1;
        v
    }
    pub fn g1_alt3(&mut self) -> i32 {
        let v = i32::from(128u8.wrapping_sub(self.data[self.pos]));
        self.pos += 1;
        v
    }

    pub fn g1b_alt1(&mut self) -> i8 {
        let v = self.data[self.pos].wrapping_sub(128) as i8;
        self.pos += 1;
        v
    }
    pub fn g1b_alt3(&mut self) -> i8 {
        let v = 128u8.wrapping_sub(self.data[self.pos]) as i8;
        self.pos += 1;
        v
    }

    /// p2 alt1: little-endian.
    pub fn p2_alt1(&mut self, value: i32) {
        self.ensure(2);
        self.data[self.pos] = value as u8;
        self.data[self.pos + 1] = (value >> 8) as u8;
        self.pos += 2;
    }
    /// p2 alt2: big-endian high, +128 low.
    pub fn p2_alt2(&mut self, value: i32) {
        self.ensure(2);
        self.data[self.pos] = (value >> 8) as u8;
        self.data[self.pos + 1] = (value as u8).wrapping_add(128);
        self.pos += 2;
    }
    /// p2 alt3: +128 high, big-endian low (mid-endian).
    pub fn p2_alt3(&mut self, value: i32) {
        self.ensure(2);
        self.data[self.pos] = (value as u8).wrapping_add(128);
        self.data[self.pos + 1] = (value >> 8) as u8;
        self.pos += 2;
    }

    pub fn g2_alt1(&mut self) -> i32 {
        let v = (i32::from(self.data[self.pos + 1]) << 8) | i32::from(self.data[self.pos]);
        self.pos += 2;
        v
    }
    pub fn g2_alt2(&mut self) -> i32 {
        let v = (i32::from(self.data[self.pos]) << 8)
            | i32::from(self.data[self.pos + 1].wrapping_sub(128));
        self.pos += 2;
        v
    }
    pub fn g2_alt3(&mut self) -> i32 {
        let v = (i32::from(self.data[self.pos + 1]) << 8)
            | i32::from(self.data[self.pos].wrapping_sub(128));
        self.pos += 2;
        v
    }

    pub fn g2b_alt1(&mut self) -> i16 {
        self.g2_alt1() as i16
    }
    pub fn g2b_alt2(&mut self) -> i16 {
        self.g2_alt2() as i16
    }
    pub fn g2b_alt3(&mut self) -> i16 {
        self.g2_alt3() as i16
    }

    /// 3-byte alt2: high-mid-low rearranged (low, high, mid).
    pub fn g3_alt2(&mut self) -> i32 {
        let v = i32::from(self.data[self.pos])
            | (i32::from(self.data[self.pos + 2]) << 16)
            | (i32::from(self.data[self.pos + 1]) << 8);
        self.pos += 3;
        v
    }

    /// p4 alt1: full little-endian.
    pub fn p4_alt1(&mut self, value: i32) {
        self.ensure(4);
        self.data[self.pos] = value as u8;
        self.data[self.pos + 1] = (value >> 8) as u8;
        self.data[self.pos + 2] = (value >> 16) as u8;
        self.data[self.pos + 3] = (value >> 24) as u8;
        self.pos += 4;
    }
    /// p4 alt2: swap (b1,b0,b3,b2) — middle-endian.
    pub fn p4_alt2(&mut self, value: i32) {
        self.ensure(4);
        self.data[self.pos] = (value >> 8) as u8;
        self.data[self.pos + 1] = value as u8;
        self.data[self.pos + 2] = (value >> 24) as u8;
        self.data[self.pos + 3] = (value >> 16) as u8;
        self.pos += 4;
    }
    /// p4 alt3: swap (b2,b3,b0,b1) — middle-endian alt.
    pub fn p4_alt3(&mut self, value: i32) {
        self.ensure(4);
        self.data[self.pos] = (value >> 16) as u8;
        self.data[self.pos + 1] = (value >> 24) as u8;
        self.data[self.pos + 2] = value as u8;
        self.data[self.pos + 3] = (value >> 8) as u8;
        self.pos += 4;
    }

    pub fn g4_alt1(&mut self) -> i32 {
        let v = i32::from(self.data[self.pos])
            | (i32::from(self.data[self.pos + 1]) << 8)
            | (i32::from(self.data[self.pos + 2]) << 16)
            | (i32::from(self.data[self.pos + 3]) << 24);
        self.pos += 4;
        v
    }
    pub fn g4_alt2(&mut self) -> i32 {
        let v = i32::from(self.data[self.pos + 1])
            | (i32::from(self.data[self.pos]) << 8)
            | (i32::from(self.data[self.pos + 2]) << 24)
            | (i32::from(self.data[self.pos + 3]) << 16);
        self.pos += 4;
        v
    }
    pub fn g4_alt3(&mut self) -> i32 {
        let v = i32::from(self.data[self.pos + 2])
            | (i32::from(self.data[self.pos + 3]) << 8)
            | (i32::from(self.data[self.pos + 1]) << 24)
            | (i32::from(self.data[self.pos]) << 16);
        self.pos += 4;
        v
    }

    /// Bulk read with bytes reversed (last-to-first into `dst`).
    pub fn gdata_alt1(&mut self, dst: &mut [u8], offset: usize, len: usize) {
        for i in (0..len).rev() {
            dst[offset + i] = self.data[self.pos];
            self.pos += 1;
        }
    }

    /// Bulk write with bytes reversed — the writer counterpart of
    /// `gdata_alt1` (used by the PLAYER_INFO appearance block).
    pub fn pdata_alt1(&mut self, src: &[u8]) {
        self.ensure(src.len());
        for &b in src.iter().rev() {
            self.data[self.pos] = b;
            self.pos += 1;
        }
    }

    // ───── CRC trailer ───────────────────────────────────────────────────────

    /// Compute CRC32 over bytes `start..pos` and write it as a 4-byte trailer.
    /// Returns the CRC value that was appended.
    pub fn addcrc(&mut self, start: usize) -> i32 {
        let crc = crc32::checksum(&self.data, start, self.pos - start) as i32;
        self.p4(crc);
        crc
    }

    /// Verify the trailing 4-byte CRC over bytes `0..pos-4`. Consumes the trailer.
    pub fn checkcrc(&mut self) -> bool {
        self.pos -= 4;
        let expected = crc32::checksum(&self.data, 0, self.pos);
        let actual = self.g4() as u32;
        expected == actual
    }

    // ───── XTEA in-place encrypt/decrypt ─────────────────────────────────────

    const XTEA_DELTA: i32 = -1_640_531_527; // 0x9E3779B9 as i32
    const XTEA_SUM: i32 = -957_401_312; // 0xC6EF3720 as i32 (delta * 32)

    /// Encrypt `[start..end]` in-place using XTEA with the 128-bit key (4 × i32). `pos` preserved.
    pub fn tinyenc(&mut self, key: &[i32; 4], start: usize, end: usize) {
        let saved = self.pos;
        self.pos = start;
        let blocks = (end - start) / 8;
        for _ in 0..blocks {
            let mut v0 = self.g4();
            let mut v1 = self.g4();
            let mut sum: i32 = 0;
            let delta = Self::XTEA_DELTA;
            for _ in 0..32 {
                v0 = v0.wrapping_add(
                    (((v1 << 4) ^ ((v1 as u32) >> 5) as i32).wrapping_add(v1))
                        ^ key[(sum & 3) as usize].wrapping_add(sum),
                );
                sum = sum.wrapping_add(delta);
                v1 = v1.wrapping_add(
                    (((v0 << 4) ^ ((v0 as u32) >> 5) as i32).wrapping_add(v0))
                        ^ key[((sum as u32) >> 11 & 3) as usize].wrapping_add(sum),
                );
            }
            self.pos -= 8;
            self.p4(v0);
            self.p4(v1);
        }
        self.pos = saved;
    }

    /// Decrypt `[start..end]` in-place using XTEA. `pos` preserved.
    pub fn tinydec(&mut self, key: &[i32; 4], start: usize, end: usize) {
        let saved = self.pos;
        self.pos = start;
        let blocks = (end - start) / 8;
        for _ in 0..blocks {
            let mut v0 = self.g4();
            let mut v1 = self.g4();
            let mut sum = Self::XTEA_SUM;
            let delta = Self::XTEA_DELTA;
            for _ in 0..32 {
                v1 = v1.wrapping_sub(
                    (((v0 << 4) ^ ((v0 as u32) >> 5) as i32).wrapping_add(v0))
                        ^ key[((sum as u32) >> 11 & 3) as usize].wrapping_add(sum),
                );
                sum = sum.wrapping_sub(delta);
                v0 = v0.wrapping_sub(
                    (((v1 << 4) ^ ((v1 as u32) >> 5) as i32).wrapping_add(v1))
                        ^ key[(sum & 3) as usize].wrapping_add(sum),
                );
            }
            self.pos -= 8;
            self.p4(v0);
            self.p4(v1);
        }
        self.pos = saved;
    }

    // ───── bit-level reads (and writes for server-side updates) ──────────────

    pub fn bit_start(&mut self) {
        self.bit_pos = self.pos * 8;
    }

    pub fn bit_end(&mut self) {
        self.pos = (self.bit_pos + 7) / 8;
    }

    #[must_use]
    pub fn bits_left(&self, buffer_size: usize) -> i32 {
        buffer_size as i32 * 8 - self.bit_pos as i32
    }

    /// Read `n` bits (1..=32) starting at the current bit cursor.
    pub fn g_bit(&mut self, n: usize) -> i32 {
        let mut n = n;
        let mut byte_pos = self.bit_pos >> 3;
        let mut remaining = 8 - (self.bit_pos & 7);
        let mut value: u32 = 0;
        self.bit_pos += n;
        while n > remaining {
            value += (u32::from(self.data[byte_pos]) & BITMASK[remaining]) << (n - remaining);
            byte_pos += 1;
            n -= remaining;
            remaining = 8;
        }
        let bits = if n == remaining {
            u32::from(self.data[byte_pos]) & BITMASK[remaining]
        } else {
            (u32::from(self.data[byte_pos]) >> (remaining - n)) & BITMASK[n]
        };
        (value + bits) as i32
    }

    /// Write the low `n` bits of `value`. Buffer must already be sized.
    pub fn p_bit(&mut self, n: usize, value: i32) {
        let mut n = n;
        let value = value as u32;
        let mut byte_pos = self.bit_pos >> 3;
        let mut remaining = 8 - (self.bit_pos & 7);
        self.bit_pos += n;
        // Make sure the bytes we're about to touch exist.
        let needed = (self.bit_pos + 7) / 8;
        if self.data.len() < needed {
            self.data.resize(needed, 0);
        }
        while n > remaining {
            self.data[byte_pos] &= !BITMASK[remaining] as u8;
            self.data[byte_pos] |= ((value >> (n - remaining)) & BITMASK[remaining]) as u8;
            byte_pos += 1;
            n -= remaining;
            remaining = 8;
        }
        if n == remaining {
            self.data[byte_pos] &= !BITMASK[remaining] as u8;
            self.data[byte_pos] |= (value & BITMASK[remaining]) as u8;
        } else {
            self.data[byte_pos] &= !((BITMASK[n] << (remaining - n)) as u8);
            self.data[byte_pos] |= ((value & BITMASK[n]) << (remaining - n)) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p1_g1_roundtrip() {
        let mut p = Packet::new(4);
        p.p1(0x42);
        p.p1(0xCA);
        p.p1(0xFE);
        p.p1(0xFF);
        assert_eq!(p.data[..4], [0x42, 0xCA, 0xFE, 0xFF]);
        p.pos = 0;
        assert_eq!(p.g1(), 0x42);
        assert_eq!(p.g1(), 0xCA);
        assert_eq!(p.g1(), 0xFE);
        assert_eq!(p.g1(), 0xFF);
    }

    #[test]
    fn signed_byte_read() {
        let mut p = Packet::from_vec(vec![0xFF, 0x80, 0x7F, 0x00]);
        assert_eq!(p.g1b(), -1);
        assert_eq!(p.g1b(), -128);
        assert_eq!(p.g1b(), 127);
        assert_eq!(p.g1b(), 0);
    }

    #[test]
    fn multibyte_big_endian() {
        let mut p = Packet::new(0);
        p.p2(0xCAFE);
        p.p3(0xABCDEF);
        p.p4(0x1234_5678);
        p.p8(0x0123_4567_89AB_CDEFi64);
        assert_eq!(p.data, vec![
            0xCA, 0xFE,
            0xAB, 0xCD, 0xEF,
            0x12, 0x34, 0x56, 0x78,
            0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF,
        ]);
        p.pos = 0;
        assert_eq!(p.g2(), 0xCAFE);
        assert_eq!(p.g3(), 0xABCDEF);
        assert_eq!(p.g4(), 0x1234_5678);
        assert_eq!(p.g8(), 0x0123_4567_89AB_CDEFi64);
    }

    #[test]
    fn p8_g8_roundtrip() {
        let mut p = Packet::new(8);
        p.p8(0x0123_4567_89AB_CDEFi64);
        p.pos = 0;
        assert_eq!(p.g8(), 0x0123_4567_89AB_CDEFi64);
    }

    #[test]
    fn signed_short_read() {
        let mut p = Packet::from_vec(vec![0xFF, 0xFF, 0x80, 0x00]);
        assert_eq!(p.g2b(), -1);
        assert_eq!(p.g2b(), -32768);
    }

    #[test]
    fn jstr_roundtrip() {
        let mut p = Packet::new(0);
        p.pjstr("hello");
        assert_eq!(p.data, b"hello\0");
        p.pos = 0;
        assert_eq!(p.gjstr(), "hello");
        assert_eq!(p.pos, 6);
    }

    #[test]
    fn jstr_empty() {
        let mut p = Packet::new(1);
        p.pjstr("");
        assert_eq!(p.data, vec![0]);
        p.pos = 0;
        assert_eq!(p.gjstr(), "");
    }

    #[test]
    fn fastgstr_handles_null_marker() {
        let mut p = Packet::from_vec(vec![0, b'a', b'b', 0]);
        assert_eq!(p.fastgstr(), None);
        assert_eq!(p.fastgstr(), Some("ab".to_string()));
    }

    #[test]
    fn psmart_1_byte() {
        let mut p = Packet::new(0);
        p.psmart(0);
        p.psmart(0x7F);
        assert_eq!(p.data, vec![0, 0x7F]);
        p.pos = 0;
        assert_eq!(p.gsmart(), 0);
        assert_eq!(p.gsmart(), 0x7F);
    }

    #[test]
    fn psmart_2_byte() {
        let mut p = Packet::new(0);
        p.psmart(0x80);
        p.psmart(0x7FFF);
        // 0x80 + 0x8000 = 0x8080 → 0x80 0x80; 0x7FFF + 0x8000 = 0xFFFF
        assert_eq!(p.data, vec![0x80, 0x80, 0xFF, 0xFF]);
        p.pos = 0;
        assert_eq!(p.gsmart(), 0x80);
        assert_eq!(p.gsmart(), 0x7FFF);
    }

    #[test]
    fn gsmarts_offsets() {
        // 1-byte branch: value - 64
        let mut p = Packet::from_vec(vec![0x00, 0x7F]);
        assert_eq!(p.gsmarts(), -64);
        assert_eq!(p.gsmarts(), 63);
        // 2-byte branch: (value - 49152)
        let mut p = Packet::from_vec(vec![0x80, 0x00]);
        assert_eq!(p.gsmarts(), -16384);
    }

    #[test]
    fn g_smart2or4() {
        let mut p = Packet::from_vec(vec![0x7F, 0xFF, 0x80, 0x00, 0x00, 0x42]);
        assert_eq!(p.g_smart2or4(), 0x7FFF); // high bit clear → g2
        assert_eq!(p.g_smart2or4(), 0x0000_0042); // high bit set → g4, mask off sign
    }

    #[test]
    fn midi_var_len_roundtrip() {
        for &n in &[0i32, 1, 0x7F, 0x80, 0x3FFF, 0x4000, 0x1F_FFFF, 0x200_0000, 0x7FFF_FFFF] {
            let mut p = Packet::new(0);
            p.p_midi_var_len(n);
            p.pos = 0;
            assert_eq!(p.g_midi_var_len(), n, "n = {n:#x}");
        }
    }

    #[test]
    fn psize_backpatch() {
        let mut p = Packet::new(0);
        p.p1(0); // placeholder
        let start = p.pos;
        p.pdata(b"hello", 0, 5);
        p.psize1((p.pos - start) as i32);
        assert_eq!(p.data, vec![5, b'h', b'e', b'l', b'l', b'o']);
    }

    #[test]
    fn alt1_byte_obfuscation() {
        // type A: + 128 on write, - 128 on read
        let mut p = Packet::new(0);
        p.p1_alt1(42);
        assert_eq!(p.data, vec![170]); // 42 + 128
        p.pos = 0;
        assert_eq!(p.g1_alt1(), 42);
    }

    #[test]
    fn alt2_byte_obfuscation() {
        // type C: negation
        let mut p = Packet::new(0);
        p.p1_alt2(42);
        assert_eq!(p.data, vec![256u16.wrapping_sub(42) as u8]); // -42 & 0xFF
        p.pos = 0;
        assert_eq!(p.g1_alt2(), 42);
    }

    #[test]
    fn alt3_byte_obfuscation() {
        // type S: 128 - value
        let mut p = Packet::new(0);
        p.p1_alt3(42);
        assert_eq!(p.data, vec![86]); // 128 - 42
        p.pos = 0;
        assert_eq!(p.g1_alt3(), 42);
    }

    #[test]
    fn p2_alt_endian_variants() {
        let cases: [(fn(&mut Packet, i32), [u8; 2]); 3] = [
            (Packet::p2_alt1, [0xCD, 0xAB]), // little-endian
            (Packet::p2_alt2, [0xAB, 0xCD_u8.wrapping_add(128)]),
            (Packet::p2_alt3, [0xCD_u8.wrapping_add(128), 0xAB]),
        ];
        for (write_fn, expected) in cases {
            let mut p = Packet::new(0);
            write_fn(&mut p, 0xABCD);
            assert_eq!(p.data, expected);
        }
    }

    #[test]
    fn alt_int_roundtrips() {
        for value in [0i32, 1, -1, 0x1234_5678, i32::MIN, i32::MAX] {
            let mut p = Packet::new(0);
            p.p4_alt1(value); p.p4_alt2(value); p.p4_alt3(value);
            p.pos = 0;
            assert_eq!(p.g4_alt1(), value);
            assert_eq!(p.g4_alt2(), value);
            assert_eq!(p.g4_alt3(), value);
        }
    }

    #[test]
    fn crc_roundtrip() {
        let mut p = Packet::new(0);
        p.pdata(b"hello world", 0, 11);
        let crc = p.addcrc(0);
        assert_eq!(p.data.len(), 15);
        p.pos = 15;
        assert!(p.checkcrc());
        // sanity
        assert_eq!(crc as u32, crate::crc32::checksum(b"hello world", 0, 11));
    }

    #[test]
    fn bit_roundtrip() {
        let mut p = Packet::new(0);
        p.bit_start();
        p.p_bit(1, 1);
        p.p_bit(3, 5);
        p.p_bit(5, 17);
        p.p_bit(11, 0x5A5);
        p.p_bit(7, 0x42);
        p.bit_end();

        let mut q = Packet::from_vec(p.data.clone());
        q.bit_start();
        assert_eq!(q.g_bit(1), 1);
        assert_eq!(q.g_bit(3), 5);
        assert_eq!(q.g_bit(5), 17);
        assert_eq!(q.g_bit(11), 0x5A5);
        assert_eq!(q.g_bit(7), 0x42);
    }

    #[test]
    fn xtea_roundtrip() {
        let key = [0x1234_5678, -0x6543_21FE, 0x4242_4242, -1];
        let mut p = Packet::new(0);
        p.pdata(b"sixteen-byte msg", 0, 16);
        p.tinyenc(&key, 0, 16);
        assert_ne!(&p.data[..], b"sixteen-byte msg");
        p.tinydec(&key, 0, 16);
        assert_eq!(&p.data[..], b"sixteen-byte msg");
    }
}
