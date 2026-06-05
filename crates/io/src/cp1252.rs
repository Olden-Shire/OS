//! Windows-1252 codec.
//!
//! Mirrors `jagex3.jstring.Cp1252` in the rev1 Java client. Strings on the wire and in the
//! cache are CP1252-encoded; we convert to/from Rust's UTF-8 `String` at the boundary.

/// Lookup for CP1252 bytes 0x80..=0x9F (the 32 codepoints that differ from Latin-1).
/// `'\0'` entries are undefined positions in CP1252.
const EXTENDED: [char; 32] = [
    '\u{20AC}', '\0',       '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\0',       '\u{017D}', '\0',
    '\0',       '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\0',       '\u{017E}', '\u{0178}',
];

/// Decode `len` bytes of CP1252 at `src[offset..]` into a UTF-8 `String`.
///
/// Embedded NUL bytes are skipped (matches `Cp1252.cp1252ToUtf8` in the Java client).
pub fn decode(src: &[u8], offset: usize, len: usize) -> String {
    let mut out = String::with_capacity(len);
    for &b in &src[offset..offset + len] {
        if b == 0 {
            continue;
        }
        let c = if (0x80..0xA0).contains(&b) {
            let ext = EXTENDED[(b - 0x80) as usize];
            if ext == '\0' { '?' } else { ext }
        } else {
            char::from(b)
        };
        out.push(c);
    }
    out
}

/// Encode `s` as CP1252 into `dst[offset..]`, returning the number of bytes written.
///
/// Characters that don't fit CP1252 are replaced with `'?'` (matches the Java client).
/// Caller is responsible for ensuring `dst` has at least `s.chars().count()` bytes free.
pub fn encode(s: &str, dst: &mut [u8], offset: usize) -> usize {
    let mut i = 0;
    for c in s.chars() {
        dst[offset + i] = encode_char(c);
        i += 1;
    }
    i
}

/// Number of CP1252 bytes a string will encode to. Mirrors `pjstrlen` minus the trailing NUL.
pub fn encoded_len(s: &str) -> usize {
    s.chars().count()
}

/// Jagex name hash: `h = (h << 5) - h + cp1252_byte_signed` per character, matching
/// `StringTools.computeCp1252HashFromUtf8` in the rev1 client. Used to resolve named files
/// in JS5 archives (e.g. `"m40_55"` → group id).
#[must_use]
pub fn name_hash(s: &str) -> i32 {
    let mut h = 0i32;
    for c in s.chars() {
        let b = encode_char(c) as i8; // sign-extend to match Java byte semantics
        h = h.wrapping_mul(31).wrapping_add(i32::from(b));
    }
    h
}

fn encode_char(c: char) -> u8 {
    let cp = c as u32;
    // ASCII and Latin-1 supplement pass through (matching the Java guard exactly).
    if (cp > 0 && cp < 128) || (160..=255).contains(&cp) {
        return cp as u8;
    }
    match cp {
        0x20AC => 0x80, 0x201A => 0x82, 0x0192 => 0x83, 0x201E => 0x84,
        0x2026 => 0x85, 0x2020 => 0x86, 0x2021 => 0x87, 0x02C6 => 0x88,
        0x2030 => 0x89, 0x0160 => 0x8A, 0x2039 => 0x8B, 0x0152 => 0x8C,
        0x017D => 0x8E, 0x2018 => 0x91, 0x2019 => 0x92, 0x201C => 0x93,
        0x201D => 0x94, 0x2022 => 0x95, 0x2013 => 0x96, 0x2014 => 0x97,
        0x02DC => 0x98, 0x2122 => 0x99, 0x0161 => 0x9A, 0x203A => 0x9B,
        0x0153 => 0x9C, 0x017E => 0x9E, 0x0178 => 0x9F,
        _ => b'?',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_roundtrip() {
        let mut buf = [0u8; 5];
        let n = encode("hello", &mut buf, 0);
        assert_eq!(n, 5);
        assert_eq!(&buf, b"hello");
        assert_eq!(decode(&buf, 0, n), "hello");
    }

    #[test]
    fn extended_glyphs() {
        // Euro, em-dash, smart quote, capital S-haček
        let mut buf = [0u8; 4];
        let n = encode("€—’Š", &mut buf, 0);
        assert_eq!(n, 4);
        assert_eq!(&buf, &[0x80, 0x97, 0x92, 0x8A]);
        assert_eq!(decode(&buf, 0, n), "€—’Š");
    }

    #[test]
    fn undefined_positions_decode_as_question_mark() {
        assert_eq!(decode(&[0x81, 0x8D, 0x90, 0x9D], 0, 4), "????");
    }

    #[test]
    fn unencodable_char_becomes_question_mark() {
        let mut buf = [0u8; 1];
        encode("漢", &mut buf, 0);
        assert_eq!(buf[0], b'?');
    }

    #[test]
    fn embedded_nul_is_dropped_on_decode() {
        assert_eq!(decode(&[b'a', 0, b'b'], 0, 3), "ab");
    }

    #[test]
    fn name_hash_matches_jagex_examples() {
        // Cross-checked against keys.json entries committed under cache/ — these names hash
        // to exactly the `name_hash` field in that file.
        assert_eq!(name_hash("l40_55"), -1_153_472_937);
        assert_eq!(name_hash("l45_73"), -1_153_323_922);
        assert_eq!(name_hash("l29_80"), -1_155_051_772);
        assert_eq!(name_hash("l36_52"), -1_154_217_715);
    }

    #[test]
    fn name_hash_empty_string_is_zero() {
        assert_eq!(name_hash(""), 0);
    }

    #[test]
    fn latin1_supplement_pass_through() {
        // 0xA0..=0xFF should round-trip identically to Latin-1.
        let mut buf = [0u8; 96];
        for i in 0..96 {
            buf[i] = 0xA0 + i as u8;
        }
        let s = decode(&buf, 0, 96);
        let mut back = [0u8; 96];
        encode(&s, &mut back, 0);
        assert_eq!(back, buf);
    }
}
