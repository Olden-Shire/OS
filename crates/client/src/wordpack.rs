// @ObfuscatedName("by") — jag::game::Huffman
// @ObfuscatedName("dz") — jag::game::WordPack
//
// Chat-message codec: chat text is CP1252-encoded then Huffman-packed
// with a canonical code table shipped in the `binary` archive's
// "huffman" group (a 256-entry bit-length table). Verbatim ports of
// wordfilter2/Huffman.java and wordfilter2/WordPack.java.

#![allow(dead_code)]

use std::sync::Mutex;

use crate::io::packet::Packet;

pub struct Huffman {
    // @ObfuscatedName("by.r") — per-symbol left-aligned codewords.
    pub masks: Vec<i32>,
    // @ObfuscatedName("by.d") — per-symbol code bit-lengths (the raw
    // table from the cache).
    pub bits: Vec<u8>,
    // @ObfuscatedName("by.l") — flattened decode trie; index advances
    // by +1 on a 0-bit, jumps via keys[i] on a 1-bit, and a negative
    // entry is ~symbol (leaf).
    pub keys: Vec<i32>,
}

impl Huffman {
    // Verbatim port of Huffman.java:18-81 — builds the canonical
    // codewords from the bit-length table and the decode trie in one
    // pass.
    pub fn new(src: &[u8]) -> Self {
        let n = src.len();
        let mut masks = vec![0i32; n];
        let bits = src.to_vec();
        let mut var3 = [0i32; 33];
        let mut keys = vec![0i32; 8];
        let mut var4 = 0i32;

        for var5 in 0..n {
            let var6 = src[var5];
            if var6 == 0 {
                continue;
            }
            let var7 = 1i32.wrapping_shl(32 - var6 as u32);
            let var8 = var3[var6 as usize];
            masks[var5] = var8;
            let var9;
            if (var8 & var7) == 0 {
                var9 = var8 | var7;
                for var10 in (1..=(var6 as usize - 1)).rev() {
                    let var11 = var3[var10];
                    if var8 != var11 {
                        break;
                    }
                    let var12 = 1i32.wrapping_shl(32 - var10 as u32);
                    if (var11 & var12) != 0 {
                        var3[var10] = var3[var10 - 1];
                        break;
                    }
                    var3[var10] = var11 | var12;
                }
            } else {
                var9 = var3[var6 as usize - 1];
            }
            var3[var6 as usize] = var9;
            for var13 in (var6 as usize + 1)..=32 {
                if var3[var13] == var8 {
                    var3[var13] = var9;
                }
            }
            let mut var14 = 0usize;
            for var15 in 0..var6 as u32 {
                let var16 = (i32::MIN as u32 >> var15) as i32;
                if (var8 & var16) == 0 {
                    var14 += 1;
                } else {
                    if keys[var14] == 0 {
                        keys[var14] = var4;
                    }
                    var14 = keys[var14] as usize;
                }
                if var14 >= keys.len() {
                    keys.resize(keys.len() * 2, 0);
                }
            }
            keys[var14] = !(var5 as i32);
            if var14 as i32 >= var4 {
                var4 = var14 as i32 + 1;
            }
        }

        Self { masks, bits, keys }
    }

    // @ObfuscatedName("by.r([BII[BII)I") — Huffman.encode. Verbatim
    // port of Huffman.java:85-126. Packs `src[src_pos..src_pos+len]`
    // into `dst` starting at byte `dst_pos`; returns bytes written.
    pub fn encode(&self, src: &[u8], src_pos: usize, len: usize,
                  dst: &mut Vec<u8>, dst_pos: usize) -> usize {
        let mut var6: i32 = 0;
        let mut var7 = (dst_pos << 3) as i32;
        let end = src_pos + len;
        let mut pos = src_pos;
        // Worst-case growth: every symbol can take up to 32 bits.
        let need = dst_pos + len * 4 + 8;
        if dst.len() < need {
            dst.resize(need, 0);
        }
        while pos < end {
            let var9 = src[pos] as usize;
            let var10 = self.masks[var9];
            let var11 = self.bits[var9] as i32;
            if var11 == 0 {
                // Java throws RuntimeException — symbol has no code.
                panic!("huffman: unencodable symbol {var9}");
            }
            let mut var12 = (var7 >> 3) as usize;
            let mut var13 = var7 & 0x7;
            let var14 = var6 & (-var13 >> 31);
            let var15 = ((var11 + var13 - 1) >> 3) as usize + (var7 >> 3) as usize;
            let var16 = var13 + 24;
            var6 = var14 | ((var10 as u32) >> var16) as i32;
            dst[var12] = var6 as u8;
            if var12 < var15 {
                var12 += 1;
                var13 = var16 - 8;
                var6 = ((var10 as u32) >> var13) as i32;
                dst[var12] = var6 as u8;
                if var12 < var15 {
                    var12 += 1;
                    var13 -= 8;
                    var6 = ((var10 as u32) >> var13) as i32;
                    dst[var12] = var6 as u8;
                    if var12 < var15 {
                        var12 += 1;
                        var13 -= 8;
                        var6 = ((var10 as u32) >> var13) as i32;
                        dst[var12] = var6 as u8;
                        if var12 < var15 {
                            var12 += 1;
                            var13 -= 8;
                            var6 = var10.wrapping_shl((-var13) as u32);
                            dst[var12] = var6 as u8;
                        }
                    }
                }
            }
            var7 += var11;
            pos += 1;
        }
        ((var7 + 7) >> 3) as usize - dst_pos
    }

    // @ObfuscatedName("by.d([BI[BIIB)I") — Huffman.decode. Verbatim
    // port of Huffman.java:130-246 with the 8-bit unrolled walk
    // collapsed into a loop (bit order MSB→LSB, identical output).
    // Returns bytes consumed from `src`.
    pub fn decode(&self, src: &[u8], src_pos: usize,
                  dst: &mut [u8], dst_pos: usize, len: usize) -> usize {
        if len == 0 {
            return 0;
        }
        let mut var6 = 0usize;
        let end = dst_pos + len;
        let mut out = dst_pos;
        let mut var8 = src_pos;
        loop {
            let var9 = src[var8] as i8;
            for bit in (0..8).rev() {
                if (var9 as i32) & (1 << bit) == 0 {
                    var6 += 1;
                } else {
                    var6 = self.keys[var6] as usize;
                }
                let v = self.keys[var6];
                if v < 0 {
                    dst[out] = (!v) as u8;
                    out += 1;
                    if out >= end {
                        return var8 + 1 - src_pos;
                    }
                    var6 = 0;
                }
            }
            var8 += 1;
        }
    }
}

// @ObfuscatedName("dz.r") — WordPack.huffman singleton, installed at
// load step from binary archive group "huffman" (Client.java:1971).
pub static HUFFMAN: Mutex<Option<Huffman>> = Mutex::new(None);

// @ObfuscatedName("bw.r(Lby;I)V") — WordPack.setHuffman.
pub fn set_huffman(h: Huffman) {
    *HUFFMAN.lock().unwrap() = Some(h);
}

pub fn huffman_loaded() -> bool {
    HUFFMAN.lock().unwrap().is_some()
}

// @ObfuscatedName("bp.d(Lev;Ljava/lang/String;B)I") — WordPack.pack.
// CP1252-encode + psmart length + Huffman bits appended at dst.pos.
// Returns bytes written (length prefix + packed payload).
pub fn pack(dst: &mut Packet, text: &str) -> usize {
    let start = dst.pos;
    let src = crate::jstring::utf8_to_cp1252(text);
    dst.psmart(src.len() as i32);
    let h = HUFFMAN.lock().unwrap();
    let Some(h) = h.as_ref() else {
        // Java would NPE before the table loads; chat can't be sent
        // until the load step has run, so this is unreachable in
        // practice — keep the packet well-formed regardless.
        return (dst.pos - start) as usize;
    };
    let pos = dst.pos as usize;
    let written = h.encode(&src, 0, src.len(), &mut dst.data, pos);
    dst.pos += written as i32;
    (dst.pos - start) as usize
}

// @ObfuscatedName("ca.l(Lev;I)Ljava/lang/String;") — WordPack.unpack.
// Java catches every decode error and returns "Cabbage".
pub fn unpack(buf: &mut Packet) -> String {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut len = buf.gsmart();
        if len > 32767 {
            len = 32767;
        }
        let mut out = vec![0u8; len as usize];
        let h = HUFFMAN.lock().unwrap();
        let h = h.as_ref().expect("huffman not loaded");
        let consumed = h.decode(&buf.data, buf.pos as usize, &mut out, 0, len as usize);
        buf.pos += consumed as i32;
        out.iter().map(|&b| crate::jstring::cp1252_to_utf8(b)).collect::<String>()
    }));
    result.unwrap_or_else(|_| "Cabbage".to_string())
}
