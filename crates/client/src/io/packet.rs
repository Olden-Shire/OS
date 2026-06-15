// @ObfuscatedName("ev")
//
// jagex3.io.Packet — Jagex's buffer + cursor type. Every read (`g*`) and
// write (`p*`) method from the gamepack is preserved here verbatim. Field
// layout matches Java (data, pos, crctable) so future revisions diff
// cleanly.

#![allow(dead_code, non_snake_case)]

use crate::settings;

use super::byte_array_pool;

pub struct Packet {
    // @ObfuscatedName("ev.m")
    pub data: Vec<u8>,

    // @ObfuscatedName("ev.c")
    pub pos: i32,

    // @ObfuscatedName("ea.i") — PacketBit.bitPos. Folded in here since we
    // don't model the Packet ↔ PacketBit inheritance hierarchy.
    pub bit_pos: i32,
}

// @ObfuscatedName("ev.n")
//
// CRC32 reflected polynomial table. Sized 256; lazily initialised on first
// access so the static init keeps the gamepack's initialisation order.
pub static CRCTABLE: std::sync::LazyLock<[i32; 256]> = std::sync::LazyLock::new(|| {
    let mut t = [0i32; 256];
    for var0 in 0..256usize {
        let mut var1: u32 = var0 as u32;
        for _ in 0..8 {
            if (var1 & 0x1) == 1 {
                var1 = (var1 >> 1) ^ 0xEDB88320;
            } else {
                var1 >>= 1;
            }
        }
        t[var0] = var1 as i32;
    }
    t
});

impl Packet {
    pub fn with_size(size: i32) -> Self {
        Self { data: byte_array_pool::alloc(size), pos: 0, bit_pos: 0 }
    }

    pub fn from_vec(data: Vec<u8>) -> Self {
        Self { data, pos: 0, bit_pos: 0 }
    }

    // @ObfuscatedName("ea.gy(S)V") — PacketBit.gBitStart
    pub fn g_bit_start(&mut self) {
        self.bit_pos = self.pos * 8;
    }

    // @ObfuscatedName("ea.gb(I)V") — PacketBit.gBitEnd
    pub fn g_bit_end(&mut self) {
        self.pos = (self.bit_pos + 7) / 8;
    }

    // @ObfuscatedName("ea.gs(II)I") — PacketBit.bitsLeft
    pub fn bits_left(&self, psize: i32) -> i32 {
        psize * 8 - self.bit_pos
    }

    // @ObfuscatedName("ea.gu(II)I") — PacketBit.gBit
    pub fn g_bit(&mut self, mut n: i32) -> i32 {
        const BITMASK: [i32; 33] = [
            0, 1, 3, 7, 15, 31, 63, 127, 255, 511, 1023, 2047, 4095, 8191,
            16383, 32767, 65535, 131071, 262143, 524287, 1048575, 2097151,
            4194303, 8388607, 16777215, 33554431, 67108863, 134217727,
            268435455, 536870911, 1073741823, i32::MAX, -1,
        ];
        let mut byte_idx = (self.bit_pos >> 3) as usize;
        let mut remain_in_byte = 8 - (self.bit_pos & 0x7);
        let mut result = 0i32;
        self.bit_pos += n;
        while n > remain_in_byte {
            result += ((self.data[byte_idx] as i32) & BITMASK[remain_in_byte as usize]) << (n - remain_in_byte);
            byte_idx += 1;
            n -= remain_in_byte;
            remain_in_byte = 8;
        }
        if n == remain_in_byte {
            result += (self.data[byte_idx] as i32) & BITMASK[remain_in_byte as usize];
        } else {
            result += (((self.data[byte_idx] as i32) >> (remain_in_byte - n)) & BITMASK[n as usize])
                + 0;
        }
        result
    }

    // @ObfuscatedName("ev.c(II)V")
    pub fn p1(&mut self, arg0: i32) {
        let i = self.pos as usize;
        self.pos += 1;
        self.data[i] = arg0 as u8;
    }

    // @ObfuscatedName("ev.dz(IB)V") — Packet.p1Enc. Writes a byte
    // XOR'd with the next ISAAC value. Used for the leading-opcode
    // byte of each outbound packet.
    //
    // Settings.NO_ISAAC short-circuits to plain p1 — without the
    // guard, callers on the "Engine2007 unencrypted" path corrupt
    // every opcode byte by XORing against a never-seeded ISAAC
    // stream.
    pub fn p1_enc(&mut self, arg0: i32, isaac: &mut crate::io::isaac::Isaac) {
        if settings::NO_ISAAC {
            self.p1(arg0);
            return;
        }
        let i = self.pos as usize;
        self.pos += 1;
        self.data[i] = (arg0 + isaac.take_next_value()) as u8;
    }

    // @ObfuscatedName("ev.dq(IB)I") — Packet.g1Enc. Inverse of p1Enc.
    pub fn g1_enc(&mut self, isaac: &mut crate::io::isaac::Isaac) -> i32 {
        if settings::NO_ISAAC {
            return self.g1();
        }
        let i = self.pos as usize;
        self.pos += 1;
        ((self.data[i] as i32) - isaac.take_next_value()) & 0xFF
    }

    // @ObfuscatedName("ev.n(II)V")
    pub fn p2(&mut self, arg0: i32) {
        let i = self.pos as usize;
        self.pos += 2;
        self.data[i] = (arg0 >> 8) as u8;
        self.data[i + 1] = arg0 as u8;
    }

    // @ObfuscatedName("ev.j(IB)V")
    pub fn p3(&mut self, arg0: i32) {
        let i = self.pos as usize;
        self.pos += 3;
        self.data[i] = (arg0 >> 16) as u8;
        self.data[i + 1] = (arg0 >> 8) as u8;
        self.data[i + 2] = arg0 as u8;
    }

    // @ObfuscatedName("ev.z(IB)V")
    pub fn p4(&mut self, arg0: i32) {
        let i = self.pos as usize;
        self.pos += 4;
        self.data[i] = (arg0 >> 24) as u8;
        self.data[i + 1] = (arg0 >> 16) as u8;
        self.data[i + 2] = (arg0 >> 8) as u8;
        self.data[i + 3] = arg0 as u8;
    }

    // @ObfuscatedName("ev.g(J)V")
    pub fn p8(&mut self, arg0: i64) {
        let i = self.pos as usize;
        self.pos += 8;
        self.data[i] = (arg0 >> 56) as u8;
        self.data[i + 1] = (arg0 >> 48) as u8;
        self.data[i + 2] = (arg0 >> 40) as u8;
        self.data[i + 3] = (arg0 >> 32) as u8;
        self.data[i + 4] = (arg0 >> 24) as u8;
        self.data[i + 5] = (arg0 >> 16) as u8;
        self.data[i + 6] = (arg0 >> 8) as u8;
        self.data[i + 7] = arg0 as u8;
    }

    // @ObfuscatedName("ea.q(Ljava/lang/String;I)I") — Packet.pjstrlen.
    // Java uses `String.length()` (UTF-16 code units) + 1 for the
    // trailing NUL. For all-ASCII strings (the only kind the protocol
    // actually carries today) this equals `chars().count()`. We mirror
    // the encoded-byte length via Cp1252 below; for ASCII it's still
    // 1 byte per char, so utf16_units == byte_count == char_count.
    pub fn pjstrlen(s: &str) -> i32 {
        // UTF-16 code unit count — matches Java's String.length(). We
        // sum 2 per non-BMP char and 1 for BMP chars.
        let mut units = 0i32;
        for ch in s.chars() {
            units += if (ch as u32) > 0xFFFF { 2 } else { 1 };
        }
        units + 1
    }

    // @ObfuscatedName("ev.i(Ljava/lang/String;I)V") — Packet.pjstr (the
    // "pStringUTF8ToCP1252" variant). Java encodes the string into
    // Cp1252 via `Cp1252.encodeStringToCp1252`. For ASCII chars (the
    // only ones the protocol uses today) Cp1252 matches UTF-8 byte-
    // for-byte. Non-ASCII Latin-1 (0x80-0xFF) chars in BMP also encode
    // to a single byte in Cp1252 — we replicate that via `as u8`
    // truncation on each char (matching Java's narrowing cast in
    // Cp1252.encodeStringToCp1252).
    pub fn pjstr(&mut self, s: &str) {
        if s.contains('\u{0}') {
            panic!("");
        }
        for ch in s.chars() {
            // Cp1252.encodeStringToCp1252 via the canonical jstring codec:
            // 1:1 for U+0001..U+007F + U+00A0..U+00FF, the smart-punctuation
            // override table for the 0x80..0x9F slots, '?' otherwise.
            let b = (crate::jstring::wide_to_cp1252(ch) & 0xFF) as u8;
            let i = self.pos as usize;
            self.pos += 1;
            self.data[i] = b;
        }
        let i = self.pos as usize;
        self.pos += 1;
        self.data[i] = 0;
    }

    // @ObfuscatedName("ev.s(Ljava/lang/CharSequence;I)V")
    pub fn pUTF8(&mut self, arg0: &str) {
        let bytes = arg0.as_bytes();
        let len = bytes.len() as i32;
        let i = self.pos as usize;
        self.pos += 1;
        self.data[i] = 0;
        self.pMidiVarLen(len);
        for &b in bytes {
            let j = self.pos as usize;
            self.pos += 1;
            self.data[j] = b;
        }
    }

    // @ObfuscatedName("ev.u([BIIB)V")
    pub fn pdata(&mut self, arg0: &[u8], arg1: i32, arg2: i32) {
        for k in arg1..(arg1 + arg2) {
            let i = self.pos as usize;
            self.pos += 1;
            self.data[i] = arg0[k as usize];
        }
    }

    // @ObfuscatedName("ev.v(II)V")
    pub fn psize4(&mut self, len: i32) {
        let p = self.pos;
        self.data[(p - len - 4) as usize] = (len >> 24) as u8;
        self.data[(p - len - 3) as usize] = (len >> 16) as u8;
        self.data[(p - len - 2) as usize] = (len >> 8) as u8;
        self.data[(p - len - 1) as usize] = len as u8;
    }

    // @ObfuscatedName("ev.w(II)V")
    pub fn psize2(&mut self, len: i32) {
        let p = self.pos;
        self.data[(p - len - 2) as usize] = (len >> 8) as u8;
        self.data[(p - len - 1) as usize] = len as u8;
    }

    // @ObfuscatedName("ev.e(IB)V")
    pub fn psize1(&mut self, len: i32) {
        let p = self.pos;
        self.data[(p - len - 1) as usize] = len as u8;
    }

    // @ObfuscatedName("ev.b(II)V")
    pub fn psmart(&mut self, n: i32) {
        if (0..0x80).contains(&n) {
            self.p1(n);
        } else if (0..0x8000).contains(&n) {
            self.p2(n + 0x8000);
        } else {
            panic!("");
        }
    }

    // @ObfuscatedName("ev.y(II)V")
    pub fn pMidiVarLen(&mut self, n: i32) {
        let n_u = n as u32;
        if (n_u & 0xFFFFFF80) != 0 {
            if (n_u & 0xFFFFC000) != 0 {
                if (n_u & 0xFFE00000) != 0 {
                    if (n_u & 0xF0000000) != 0 {
                        self.p1(((n_u >> 28) | 0x80) as i32);
                    }
                    self.p1(((n_u >> 21) | 0x80) as i32);
                }
                self.p1(((n_u >> 14) | 0x80) as i32);
            }
            self.p1(((n_u >> 7) | 0x80) as i32);
        }
        self.p1((n & 0x7F) as i32);
    }

    // @ObfuscatedName("ev.t(I)I")
    pub fn g1(&mut self) -> i32 {
        let i = self.pos as usize;
        self.pos += 1;
        self.data[i] as i32 & 0xFF
    }

    // @ObfuscatedName("ev.f(I)B")
    pub fn g1b(&mut self) -> i8 {
        let i = self.pos as usize;
        self.pos += 1;
        self.data[i] as i8
    }

    // @ObfuscatedName("ev.k(I)I")
    pub fn g2(&mut self) -> i32 {
        self.pos += 2;
        let p = self.pos as usize;
        ((self.data[p - 2] as i32 & 0xFF) << 8) + (self.data[p - 1] as i32 & 0xFF)
    }

    // @ObfuscatedName("ev.o(I)I")
    pub fn g2b(&mut self) -> i32 {
        self.pos += 2;
        let p = self.pos as usize;
        let mut var1 = ((self.data[p - 2] as i32 & 0xFF) << 8) + (self.data[p - 1] as i32 & 0xFF);
        if var1 > 32767 {
            var1 -= 65536;
        }
        var1
    }

    // @ObfuscatedName("ev.a(B)I")
    pub fn g3(&mut self) -> i32 {
        self.pos += 3;
        let p = self.pos as usize;
        (self.data[p - 1] as i32 & 0xFF)
            + ((self.data[p - 2] as i32 & 0xFF) << 8)
            + ((self.data[p - 3] as i32 & 0xFF) << 16)
    }

    // @ObfuscatedName("ev.h(I)I")
    pub fn g4(&mut self) -> i32 {
        self.pos += 4;
        let p = self.pos as usize;
        (self.data[p - 1] as i32 & 0xFF)
            + ((self.data[p - 2] as i32 & 0xFF) << 8)
            + ((self.data[p - 3] as i32 & 0xFF) << 16)
            + ((self.data[p - 4] as i32 & 0xFF) << 24)
    }

    // @ObfuscatedName("ev.x(I)J")
    pub fn g8(&mut self) -> i64 {
        let var1 = self.g4() as i64 & 0xFFFFFFFF;
        let var3 = self.g4() as i64 & 0xFFFFFFFF;
        (var1 << 32) + var3
    }

    // @ObfuscatedName("ev.p(I)Ljava/lang/String;")
    pub fn fastgstr(&mut self) -> Option<String> {
        if self.data[self.pos as usize] == 0 {
            self.pos += 1;
            None
        } else {
            Some(self.gjstr())
        }
    }

    // @ObfuscatedName("ev.ad(I)Ljava/lang/String;")
    pub fn gjstr(&mut self) -> String {
        let var1 = self.pos as usize;
        loop {
            let i = self.pos as usize;
            self.pos += 1;
            if self.data[i] == 0 {
                break;
            }
        }
        let var2 = self.pos as usize - var1 - 1;
        if var2 == 0 {
            String::new()
        } else {
            crate::jstring::cp1252_slice_to_string(&self.data, var1, var2)
        }
    }

    // @ObfuscatedName("ev.ac(B)Ljava/lang/String;")
    pub fn gjstr2(&mut self) -> String {
        let i = self.pos as usize;
        self.pos += 1;
        let var1 = self.data[i];
        if var1 != 0 {
            panic!("");
        }
        let var2 = self.pos as usize;
        loop {
            let j = self.pos as usize;
            self.pos += 1;
            if self.data[j] == 0 {
                break;
            }
        }
        let var3 = self.pos as usize - var2 - 1;
        if var3 == 0 {
            String::new()
        } else {
            crate::jstring::cp1252_slice_to_string(&self.data, var2, var3)
        }
    }

    // @ObfuscatedName("ev.aa(I)Ljava/lang/String;") — Packet.gUTF8.
    //
    // Verbatim port of Java's per-sequence UTF-8 decoder. Each malformed
    // byte / overlong / truncated sequence emits U+FFFD (65533) in
    // place; valid sequences widen to char and are concatenated. The
    // leading byte must be 0 (Java's version marker) followed by a
    // MIDI-varlen byte count. Diverges from `from_utf8_lossy`: Rust's
    // helper replaces malformed RUNS with a single U+FFFD, whereas
    // Java's decoder emits one U+FFFD per malformed byte. Match Java
    // exactly so any string that round-trips through the protocol stays
    // identical.
    #[allow(non_snake_case)]
    pub fn gUTF8(&mut self) -> String {
        let i = self.pos as usize;
        self.pos += 1;
        if self.data[i] != 0 {
            panic!("Packet.gUTF8: expected version marker 0");
        }
        let var2 = self.gMidiVarLen() as usize;
        if self.pos as usize + var2 > self.data.len() {
            panic!("Packet.gUTF8: truncated payload");
        }
        let var3 = &self.data;
        let mut var7 = self.pos as usize;
        let var8 = var2 + var7;
        let mut out = String::with_capacity(var2);
        while var7 < var8 {
            let var9 = var3[var7] as i32 & 0xFF;
            var7 += 1;
            let var10: u32;
            if var9 < 128 {
                var10 = if var9 == 0 { 0xFFFD } else { var9 as u32 };
            } else if var9 < 192 {
                var10 = 0xFFFD;
            } else if var9 < 224 {
                if var7 < var8 && (var3[var7] as i32 & 0xC0) == 128 {
                    let b1 = var3[var7] as i32 & 0x3F;
                    var7 += 1;
                    let code = ((var9 & 0x1F) << 6) | b1;
                    var10 = if code < 128 { 0xFFFD } else { code as u32 };
                } else {
                    var10 = 0xFFFD;
                }
            } else if var9 < 240 {
                if var7 + 1 < var8
                    && (var3[var7] as i32 & 0xC0) == 128
                    && (var3[var7 + 1] as i32 & 0xC0) == 128
                {
                    let b1 = var3[var7] as i32 & 0x3F;
                    let b2 = var3[var7 + 1] as i32 & 0x3F;
                    var7 += 2;
                    let code = ((var9 & 0xF) << 12) | (b1 << 6) | b2;
                    var10 = if code < 2048 { 0xFFFD } else { code as u32 };
                } else {
                    var10 = 0xFFFD;
                }
            } else if var9 >= 248 {
                var10 = 0xFFFD;
            } else if var7 + 2 < var8
                && (var3[var7] as i32 & 0xC0) == 128
                && (var3[var7 + 1] as i32 & 0xC0) == 128
                && (var3[var7 + 2] as i32 & 0xC0) == 128
            {
                // Java's literal here always yields var10 = 65533 in
                // BOTH branches of the inner if — preserved verbatim.
                // (Probably a Jagex bug intended to assemble the surrogate
                // pair properly; the compiled gamepack just emits
                // replacement chars for any 4-byte sequence.)
                let _b1 = var3[var7] as i32 & 0x3F;
                let _b2 = var3[var7 + 1] as i32 & 0x3F;
                let _b3 = var3[var7 + 2] as i32 & 0x3F;
                var7 += 3;
                var10 = 0xFFFD;
            } else {
                var10 = 0xFFFD;
            }
            // Java casts var10 to `char` (16-bit). Code points above
            // 0xFFFF truncate, but the path above never produces them.
            let ch = char::from_u32(var10 & 0xFFFF).unwrap_or('\u{FFFD}');
            out.push(ch);
        }
        self.pos += var2 as i32;
        out
    }

    // @ObfuscatedName("ev.as([BIII)V")
    pub fn gdata(&mut self, arg0: &mut [u8], arg1: i32, arg2: i32) {
        for k in arg1..(arg1 + arg2) {
            let i = self.pos as usize;
            self.pos += 1;
            arg0[k as usize] = self.data[i];
        }
    }

    // @ObfuscatedName("ev.am(I)I")
    pub fn gsmarts(&mut self) -> i32 {
        let var1 = self.data[self.pos as usize] as i32 & 0xFF;
        if var1 < 128 { self.g1() - 64 } else { self.g2() - 49152 }
    }

    // @ObfuscatedName("ev.ap(I)I")
    pub fn gsmart(&mut self) -> i32 {
        let var1 = self.data[self.pos as usize] as i32 & 0xFF;
        if var1 < 128 { self.g1() } else { self.g2() - 32768 }
    }

    // @ObfuscatedName("ev.av(S)I")
    pub fn gSmart2or4(&mut self) -> i32 {
        if (self.data[self.pos as usize] as i8) < 0 { self.g4() & 0x7fffffff } else { self.g2() }
    }

    // @ObfuscatedName("ev.ak(B)I")
    pub fn gMidiVarLen(&mut self) -> i32 {
        let i = self.pos as usize;
        self.pos += 1;
        let mut var1 = self.data[i] as i8;
        let mut var2: i32 = 0;
        while var1 < 0 {
            var2 = (var2 | (var1 as i32 & 0x7F)) << 7;
            let j = self.pos as usize;
            self.pos += 1;
            var1 = self.data[j] as i8;
        }
        var2 | var1 as i32
    }

    // @ObfuscatedName("ev.az([IIII)V")
    pub fn tinyenc(&mut self, arg0: &[i32; 4], arg1: i32, arg2: i32) {
        if settings::NO_TINYENC {
            return;
        }
        let saved = self.pos;
        self.pos = arg1;
        let var5 = (arg2 - arg1) / 8;
        for _ in 0..var5 {
            let mut var7 = self.g4();
            let mut var8 = self.g4();
            let mut var9: i32 = 0;
            let var10: i32 = -1640531527;
            let mut var11 = 32;
            while {
                let c = var11 > 0;
                var11 -= 1;
                c
            } {
                var7 = var7.wrapping_add(
                    ((var8 << 4) ^ ((var8 as u32 >> 5) as i32))
                        .wrapping_add(var8)
                        ^ arg0[(var9 & 0x3) as usize].wrapping_add(var9),
                );
                var9 = var9.wrapping_add(var10);
                var8 = var8.wrapping_add(
                    ((var7 << 4) ^ ((var7 as u32 >> 5) as i32))
                        .wrapping_add(var7)
                        ^ arg0[((var9 as u32 >> 11) as i32 & 0x3) as usize].wrapping_add(var9),
                );
            }
            self.pos -= 8;
            self.p4(var7);
            self.p4(var8);
        }
        self.pos = saved;
    }

    // @ObfuscatedName("ev.an([IIII)V")
    pub fn tinydec(&mut self, arg0: &[i32; 4], arg1: i32, arg2: i32) {
        let saved = self.pos;
        self.pos = arg1;
        let var5 = (arg2 - arg1) / 8;
        for _ in 0..var5 {
            let mut var7 = self.g4();
            let mut var8 = self.g4();
            let mut var9: i32 = -957401312;
            let var10: i32 = -1640531527;
            let mut var11 = 32;
            while {
                let c = var11 > 0;
                var11 -= 1;
                c
            } {
                var8 = var8.wrapping_sub(
                    ((var7 << 4) ^ ((var7 as u32 >> 5) as i32))
                        .wrapping_add(var7)
                        ^ arg0[((var9 as u32 >> 11) as i32 & 0x3) as usize].wrapping_add(var9),
                );
                var9 = var9.wrapping_sub(var10);
                var7 = var7.wrapping_sub(
                    ((var8 << 4) ^ ((var8 as u32 >> 5) as i32))
                        .wrapping_add(var8)
                        ^ arg0[(var9 & 0x3) as usize].wrapping_add(var9),
                );
            }
            self.pos -= 8;
            self.p4(var7);
            self.p4(var8);
        }
        self.pos = saved;
    }

    // @ObfuscatedName("ev.ah(Ljava/math/BigInteger;Ljava/math/BigInteger;I)V")
    //
    // RSA-encrypt the bytes in [0, pos). With Settings.NO_RSA the gamepack
    // re-frames the payload as a plain length-prefixed blob; we mirror that
    // and skip the BigInteger pow because the local server expects plain.
    pub fn rsaenc(&mut self, _modulus: &[u8], _exponent: &[u8]) {
        let var3 = self.pos;
        self.pos = 0;
        let mut var4 = vec![0u8; var3 as usize];
        self.gdata(&mut var4, 0, var3);

        if settings::NO_RSA {
            self.p2(var4.len() as i32);
            let len = var4.len() as i32;
            self.pdata(&var4, 0, len);
        } else {
            // Full RSA path requires BigInteger porting; the JS5 server in
            // Settings.NO_RSA mode never sees a real RSA blob anyway.
            unimplemented!("Packet.rsaenc with NO_RSA=false not ported yet");
        }
    }

    // @ObfuscatedName("ev.ay(II)I")
    pub fn addcrc(&mut self, arg0: i32) -> i32 {
        let var3 = self.pos;
        let mut var4: i32 = -1;
        for var5 in arg0..var3 {
            let b = self.data[var5 as usize] as i32;
            var4 = (var4 as u32 >> 8) as i32 ^ CRCTABLE[((var4 ^ b) & 0xFF) as usize];
        }
        let var6 = !var4;
        self.p4(var6);
        var6
    }

    // @ObfuscatedName("ev.al(I)Z")
    pub fn checkcrc(&mut self) -> bool {
        self.pos -= 4;
        let var2 = self.pos;
        let mut var3: i32 = -1;
        for var4 in 0..var2 {
            let b = self.data[var4 as usize] as i32;
            var3 = (var3 as u32 >> 8) as i32 ^ CRCTABLE[((var3 ^ b) & 0xFF) as usize];
        }
        let var5 = !var3;
        let var8 = self.g4();
        var5 == var8
    }

    // @ObfuscatedName("ev.ab(II)V")
    pub fn p1_alt1(&mut self, arg0: i32) {
        if settings::NO_ALT_METHODS {
            self.p1(arg0);
            return;
        }
        let i = self.pos as usize;
        self.pos += 1;
        self.data[i] = (arg0 + 128) as u8;
    }

    // @ObfuscatedName("ev.ao(IS)V")
    pub fn p1_alt2(&mut self, arg0: i32) {
        if settings::NO_ALT_METHODS {
            self.p1(arg0);
            return;
        }
        let i = self.pos as usize;
        self.pos += 1;
        self.data[i] = (-arg0) as u8;
    }

    // @ObfuscatedName("ev.ag(II)V")
    pub fn p1_alt3(&mut self, arg0: i32) {
        if settings::NO_ALT_METHODS {
            self.p1(arg0);
            return;
        }
        let i = self.pos as usize;
        self.pos += 1;
        self.data[i] = (128 - arg0) as u8;
    }

    // @ObfuscatedName("ev.ar(I)I")
    pub fn g1_alt1(&mut self) -> i32 {
        if settings::NO_ALT_METHODS {
            return self.g1();
        }
        let i = self.pos as usize;
        self.pos += 1;
        (self.data[i] as i32 - 128) & 0xFF
    }

    // @ObfuscatedName("ev.aq(I)I")
    pub fn g1_alt2(&mut self) -> i32 {
        if settings::NO_ALT_METHODS {
            return self.g1();
        }
        let i = self.pos as usize;
        self.pos += 1;
        (-(self.data[i] as i32)) & 0xFF
    }

    // @ObfuscatedName("ev.at(I)I")
    pub fn g1_alt3(&mut self) -> i32 {
        if settings::NO_ALT_METHODS {
            return self.g1();
        }
        let i = self.pos as usize;
        self.pos += 1;
        (128 - self.data[i] as i32) & 0xFF
    }

    // @ObfuscatedName("ev.ae(I)B")
    pub fn g1b_alt1(&mut self) -> i8 {
        if settings::NO_ALT_METHODS {
            return self.g1b();
        }
        let i = self.pos as usize;
        self.pos += 1;
        (self.data[i] as i32 - 128) as i8
    }

    // NOTE: Java's Packet has no g1b_alt2 — only g1b_alt1 ("ev.aw") and
    // g1b_alt3 ("ev.au"). A negate-byte variant previously here carried
    // @ObfuscatedName("ev.ay"), but ev.ay(II)I is actually addcrc; the
    // method was invented during porting and had no callers. Removed.

    // @ObfuscatedName("ev.au(I)B")
    pub fn g1b_alt3(&mut self) -> i8 {
        if settings::NO_ALT_METHODS {
            return self.g1b();
        }
        let i = self.pos as usize;
        self.pos += 1;
        (128 - self.data[i] as i32) as i8
    }

    // @ObfuscatedName("ev.ax(IB)V")
    pub fn p2_alt1(&mut self, arg0: i32) {
        if settings::NO_ALT_METHODS {
            self.p2(arg0);
            return;
        }
        let i = self.pos as usize;
        self.pos += 2;
        self.data[i] = arg0 as u8;
        self.data[i + 1] = (arg0 >> 8) as u8;
    }

    // @ObfuscatedName("ev.ai(IB)V")
    pub fn p2_alt2(&mut self, arg0: i32) {
        if settings::NO_ALT_METHODS {
            self.p2(arg0);
            return;
        }
        let i = self.pos as usize;
        self.pos += 2;
        self.data[i] = (arg0 >> 8) as u8;
        self.data[i + 1] = (arg0 + 128) as u8;
    }

    // @ObfuscatedName("ev.aj(II)V")
    pub fn p2_alt3(&mut self, arg0: i32) {
        if settings::NO_ALT_METHODS {
            self.p2(arg0);
            return;
        }
        let i = self.pos as usize;
        self.pos += 2;
        self.data[i] = (arg0 + 128) as u8;
        self.data[i + 1] = (arg0 >> 8) as u8;
    }

    // @ObfuscatedName("ev.aw(I)I")
    pub fn g2_alt1(&mut self) -> i32 {
        if settings::NO_ALT_METHODS {
            return self.g2();
        }
        self.pos += 2;
        let p = self.pos as usize;
        ((self.data[p - 1] as i32 & 0xFF) << 8) + (self.data[p - 2] as i32 & 0xFF)
    }

    // @ObfuscatedName("ev.af(I)I")
    pub fn g2_alt2(&mut self) -> i32 {
        if settings::NO_ALT_METHODS {
            return self.g2();
        }
        self.pos += 2;
        let p = self.pos as usize;
        ((self.data[p - 2] as i32 & 0xFF) << 8) + ((self.data[p - 1] as i32 - 128) & 0xFF)
    }

    // @ObfuscatedName("ev.bh(I)I")
    pub fn g2_alt3(&mut self) -> i32 {
        if settings::NO_ALT_METHODS {
            return self.g2();
        }
        self.pos += 2;
        let p = self.pos as usize;
        ((self.data[p - 1] as i32 & 0xFF) << 8) + ((self.data[p - 2] as i32 - 128) & 0xFF)
    }

    // @ObfuscatedName("ev.bi(I)I")
    pub fn g2b_alt1(&mut self) -> i32 {
        if settings::NO_ALT_METHODS {
            return self.g2b();
        }
        self.pos += 2;
        let p = self.pos as usize;
        let mut var1 = ((self.data[p - 1] as i32 & 0xFF) << 8) + (self.data[p - 2] as i32 & 0xFF);
        if var1 > 32767 {
            var1 -= 65536;
        }
        var1
    }

    // @ObfuscatedName("ev.bs(B)I")
    pub fn g2b_alt2(&mut self) -> i32 {
        if settings::NO_ALT_METHODS {
            return self.g2b();
        }
        self.pos += 2;
        let p = self.pos as usize;
        let mut var1 =
            ((self.data[p - 2] as i32 & 0xFF) << 8) + ((self.data[p - 1] as i32 - 128) & 0xFF);
        if var1 > 32767 {
            var1 -= 65536;
        }
        var1
    }

    // @ObfuscatedName("ev.bk(S)I")
    pub fn g2b_alt3(&mut self) -> i32 {
        if settings::NO_ALT_METHODS {
            return self.g2b();
        }
        self.pos += 2;
        let p = self.pos as usize;
        let mut var1 =
            ((self.data[p - 1] as i32 & 0xFF) << 8) + ((self.data[p - 2] as i32 - 128) & 0xFF);
        if var1 > 32767 {
            var1 -= 65536;
        }
        var1
    }

    // @ObfuscatedName("ev.bv(I)I")
    pub fn g3_alt2(&mut self) -> i32 {
        if settings::NO_ALT_METHODS {
            return self.g3();
        }
        self.pos += 3;
        let p = self.pos as usize;
        (self.data[p - 3] as i32 & 0xFF)
            + ((self.data[p - 1] as i32 & 0xFF) << 16)
            + ((self.data[p - 2] as i32 & 0xFF) << 8)
    }

    // @ObfuscatedName("ev.bw(IS)V")
    pub fn p4_alt1(&mut self, arg0: i32) {
        if settings::NO_ALT_METHODS {
            self.p4(arg0);
            return;
        }
        let i = self.pos as usize;
        self.pos += 4;
        self.data[i] = arg0 as u8;
        self.data[i + 1] = (arg0 >> 8) as u8;
        self.data[i + 2] = (arg0 >> 16) as u8;
        self.data[i + 3] = (arg0 >> 24) as u8;
    }

    // @ObfuscatedName("ev.by(II)V")
    pub fn p4_alt2(&mut self, arg0: i32) {
        if settings::NO_ALT_METHODS {
            self.p4(arg0);
            return;
        }
        let i = self.pos as usize;
        self.pos += 4;
        self.data[i] = (arg0 >> 8) as u8;
        self.data[i + 1] = arg0 as u8;
        self.data[i + 2] = (arg0 >> 24) as u8;
        self.data[i + 3] = (arg0 >> 16) as u8;
    }

    // @ObfuscatedName("ev.bx(IS)V")
    pub fn p4_alt3(&mut self, arg0: i32) {
        if settings::NO_ALT_METHODS {
            self.p4(arg0);
            return;
        }
        let i = self.pos as usize;
        self.pos += 4;
        self.data[i] = (arg0 >> 16) as u8;
        self.data[i + 1] = (arg0 >> 24) as u8;
        self.data[i + 2] = arg0 as u8;
        self.data[i + 3] = (arg0 >> 8) as u8;
    }

    // @ObfuscatedName("ev.bf(I)I")
    pub fn g4_alt1(&mut self) -> i32 {
        if settings::NO_ALT_METHODS {
            return self.g4();
        }
        self.pos += 4;
        let p = self.pos as usize;
        (self.data[p - 4] as i32 & 0xFF)
            + ((self.data[p - 3] as i32 & 0xFF) << 8)
            + ((self.data[p - 2] as i32 & 0xFF) << 16)
            + ((self.data[p - 1] as i32 & 0xFF) << 24)
    }

    // @ObfuscatedName("ev.bu(I)I")
    pub fn g4_alt2(&mut self) -> i32 {
        if settings::NO_ALT_METHODS {
            return self.g4();
        }
        self.pos += 4;
        let p = self.pos as usize;
        (self.data[p - 3] as i32 & 0xFF)
            + ((self.data[p - 4] as i32 & 0xFF) << 8)
            + ((self.data[p - 2] as i32 & 0xFF) << 24)
            + ((self.data[p - 1] as i32 & 0xFF) << 16)
    }

    // @ObfuscatedName("ev.bo(B)I")
    pub fn g4_alt3(&mut self) -> i32 {
        if settings::NO_ALT_METHODS {
            return self.g4();
        }
        self.pos += 4;
        let p = self.pos as usize;
        (self.data[p - 2] as i32 & 0xFF)
            + ((self.data[p - 1] as i32 & 0xFF) << 8)
            + ((self.data[p - 3] as i32 & 0xFF) << 24)
            + ((self.data[p - 4] as i32 & 0xFF) << 16)
    }

    // @ObfuscatedName("ev.bq([BIII)V")
    pub fn gdata_alt1(&mut self, arg0: &mut [u8], arg1: i32, arg2: i32) {
        if settings::NO_ALT_METHODS {
            self.gdata(arg0, arg1, arg2);
            return;
        }
        let mut k = arg1 + arg2 - 1;
        while k >= arg1 {
            let i = self.pos as usize;
            self.pos += 1;
            arg0[k as usize] = self.data[i];
            k -= 1;
        }
    }
}
