// jagex3.jstring.StringTools — only the helpers JS5 name lookup needs.

#![allow(dead_code, non_snake_case)]

// @ObfuscatedName("ck.j(Ljava/lang/CharSequence;I)I")
// jag::oldscape::core::stringtools::general::CP1252Tools::ComputeLowerCp1252HashFromUtf8
//
// Cp1252.wideToCp1252 collapses to the input byte for ASCII (which is all
// the title-screen sprite/font names use). Full Cp1252 mapping lands when
// chat / interface text gets ported.
pub fn computeCp1252HashFromUtf8(arg0: &str) -> i32 {
    let mut var2: i32 = 0;
    for ch in arg0.chars() {
        let cp = wide_to_cp1252(ch) as i32;
        var2 = ((var2 << 5).wrapping_sub(var2)).wrapping_add(cp);
    }
    var2
}

// Full Cp1252 0x80-0x9F extended block — the only non-Latin-1 region
// Java's Cp1252 maps. Each entry is a Unicode codepoint; -1 means
// "unmapped" (renders as '?' on the wire).
const CP1252_EXTENDED: [i32; 32] = [
    0x20AC, -1, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021,    // 0x80-0x87 (€,?,‚,ƒ,„,…,†,‡)
    0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, -1, 0x017D, -1,        // 0x88-0x8F (ˆ,‰,Š,‹,Œ,?,Ž,?)
    -1,     0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, // 0x90-0x97 (?,‘,’,“,”,•,–,—)
    0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, -1, 0x017E, 0x0178,    // 0x98-0x9F (˜,™,š,›,œ,?,ž,Ÿ)
];

pub fn wide_to_cp1252(ch: char) -> i32 {
    let c = ch as u32;
    if c < 0x80 || (0xA0..=0xFF).contains(&c) {
        return c as i32;
    }
    // Search the extended block for a matching codepoint.
    for (i, &mapped) in CP1252_EXTENDED.iter().enumerate() {
        if mapped == c as i32 {
            return (0x80 + i) as i32;
        }
    }
    b'?' as i32
}

// @ObfuscatedName("bk.d(CI)Z") — Cp1252.canEncodeToCp1252. Verbatim
// port of Cp1252.java:90-104. Note that the NUL char `\0` is
// explicitly NOT encodable (Java's `arg0 > 0 && arg0 < 128`); we
// preserve that quirk because it's the protocol layer's sentinel.
pub fn can_encode_to_cp1252(ch: char) -> bool {
    let c = ch as u32;
    if (c > 0 && c < 0x80) || (0xA0..=0xFF).contains(&c) {
        return true;
    }
    if c == 0 { return false; }
    CP1252_EXTENDED.contains(&(c as i32))
}

// @ObfuscatedName("ck.u(CB)C") — Cp1252.cp1252ToUtf8. Returns the
// Unicode char for a Cp1252 byte (0x00..0xFF). Unmapped bytes in the
// 0x80-0x9F range fall back to '?'.
pub fn cp1252_to_utf8(byte: u8) -> char {
    let b = byte as u32;
    if b < 0x80 || (0xA0..=0xFF).contains(&b) {
        char::from_u32(b).unwrap_or('?')
    } else {
        let mapped = CP1252_EXTENDED[(b - 0x80) as usize];
        if mapped < 0 { '?' } else { char::from_u32(mapped as u32).unwrap_or('?') }
    }
}

// @ObfuscatedName("ck.v(Ljava/lang/String;II[BI)I") — Cp1252.encodeStringToCp1252.
//
// Writes `s.chars()` into `out[off..]` as Cp1252 bytes, returning the
// number of bytes consumed. Non-encodable chars become '?'.
pub fn encode_string_to_cp1252(s: &str, out: &mut [u8], off: usize) -> usize {
    let mut written = 0;
    for ch in s.chars() {
        if off + written >= out.len() { break; }
        out[off + written] = (wide_to_cp1252(ch) & 0xFF) as u8;
        written += 1;
    }
    written
}

// @ObfuscatedName("bh.c(Ljava/lang/CharSequence;II[BII)I") — full
// 5-arg Cp1252.encodeStringToCp1252. Verbatim port of Cp1252.java:
// 177-242. Encodes the [src_off, src_end) substring of `s` into
// `dst` starting at `dst_off`. Returns the byte count written
// (= src_end - src_off, since each char becomes 1 byte).
pub fn encode_string_to_cp1252_range(
    s: &str, src_off: usize, src_end: usize,
    dst: &mut [u8], dst_off: usize,
) -> usize {
    let mut count = 0usize;
    for (i, ch) in s.chars().enumerate() {
        if i < src_off { continue; }
        if i >= src_end { break; }
        if dst_off + count >= dst.len() { break; }
        dst[dst_off + count] = (wide_to_cp1252(ch) & 0xFF) as u8;
        count += 1;
    }
    count
}

// @ObfuscatedName("ck.w([BIIB)Ljava/lang/String;") — Cp1252.cp1252ToUtf8
// (the String variant). Decodes a Cp1252 byte slice into a Rust
// String.
pub fn cp1252_slice_to_string(buf: &[u8], off: usize, len: usize) -> String {
    buf[off..(off + len).min(buf.len())]
        .iter()
        .map(|&b| cp1252_to_utf8(b))
        .collect()
}

// @ObfuscatedName("cu.z(CI)Z") — StringTools.isAlpha.
pub fn is_alpha(c: char) -> bool {
    matches!(c, 'A'..='Z' | 'a'..='z')
}

// @ObfuscatedName(— StringTools.isNumeric, no signature stamp in
// rev1 .class) — verbatim port of StringTools.java:257-259.
pub fn is_numeric(c: char) -> bool {
    ('0'..='9').contains(&c)
}

// @ObfuscatedName(— StringTools.isAlphaNumeric).
pub fn is_alpha_numeric(c: char) -> bool {
    is_alpha(c) || is_numeric(c)
}

// @ObfuscatedName("ek.r(IB)Ljava/lang/String;") — StringTools.formatIPv4.
// Verbatim port of StringTools.java:15-17. Splits a 32-bit int into
// 4 octets and formats as "a.b.c.d". Used by login error messages.
pub fn format_ipv4(arg0: i32) -> String {
    format!("{}.{}.{}.{}",
        (arg0 >> 24) & 0xFF,
        (arg0 >> 16) & 0xFF,
        (arg0 >> 8)  & 0xFF,
        arg0 & 0xFF)
}

// @ObfuscatedName("bd.q(CII)Ljava/lang/String;") — StringTools.getRepeatedCharacter.
// Verbatim port of StringTools.java:248-253. Produces a length-`len`
// string of `ch`. Java allocates a char[] then constructs a String;
// we use `String::repeat` for the same semantic.
pub fn get_repeated_character(ch: char, len: i32) -> String {
    if len <= 0 { return String::new(); }
    ch.to_string().repeat(len as usize)
}

// @ObfuscatedName("ef.d([Ljava/lang/CharSequence;III)Ljava/lang/String;") —
// StringTools.join. Verbatim port of StringTools.java:21-52. Concats
// the [off, off+len) slice; a `None` entry maps to the literal "null".
pub fn join(src: &[Option<&str>], off: usize, len: usize) -> String {
    if len == 0 { return String::new(); }
    if len == 1 {
        return src.get(off).copied().flatten().unwrap_or("null").to_string();
    }
    let end = off + len;
    let mut cap = 0usize;
    for i in off..end {
        cap += match src.get(i).copied().flatten() {
            Some(s) => s.len(),
            None => 4,
        };
    }
    let mut out = String::with_capacity(cap);
    for i in off..end {
        match src.get(i).copied().flatten() {
            Some(s) => out.push_str(s),
            None => out.push_str("null"),
        }
    }
    out
}

// @ObfuscatedName("j.l(Ljava/lang/CharSequence;I)Z") — StringTools.isInt.
// Verbatim port of StringTools.java:56-110. Base-10 parse predicate
// (matches Java's overflow-detecting accumulator). Returns false on
// overflow, empty, or non-digit/sign characters.
pub fn is_int(s: &str) -> bool {
    let mut neg = false;
    let mut any = false;
    let mut acc: i32 = 0;
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if i == 0 {
            if c == '-' { neg = true; continue; }
            if c == '+' { continue; }
        }
        let d = if ('0'..='9').contains(&c) { c as i32 - '0' as i32 }
                else if ('A'..='Z').contains(&c) { c as i32 - '7' as i32 }
                else if ('a'..='z').contains(&c) { c as i32 - 'W' as i32 }
                else { return false; };
        if d >= 10 { return false; }
        let d = if neg { -d } else { d };
        let next = match acc.checked_mul(10).and_then(|v| v.checked_add(d)) {
            Some(v) if v / 10 == acc => v,
            _ => return false,
        };
        acc = next;
        any = true;
    }
    any
}

// @ObfuscatedName("bn.n(IZI)Ljava/lang/String;") — StringTools.fromInt.
// Verbatim port of StringTools.java:170-200. With `explicit_plus`
// false or for negative values, just to_string(); with explicit_plus
// true on a non-negative, prefixes a '+'. Java builds a char-array;
// we use format!.
pub fn from_int(arg0: i32, explicit_plus: bool) -> String {
    if !explicit_plus || arg0 < 0 {
        arg0.to_string()
    } else {
        format!("+{}", arg0)
    }
}

// @ObfuscatedName("ck.m(Ljava/lang/CharSequence;I)[B") — Cp1252.utf8ToCp1252.
// Verbatim port of Cp1252.java:108-149 — but simpler in Rust since
// our `wide_to_cp1252` already handles every char. Maps the source
// string char-by-char into Cp1252 bytes.
pub fn utf8_to_cp1252(s: &str) -> Vec<u8> {
    s.chars().map(|c| wide_to_cp1252(c) as u8).collect()
}

// @ObfuscatedName("n.g(Ljava/lang/String;B)Ljava/lang/String;") —
// StringTools.forceCapitalisationOfWords. Verbatim port of
// StringTools.java:221-243. Tracks a 3-state position marker:
//   2 = start of sentence (next letter becomes title-case)
//   1 = mid-token (next letter becomes lower-case)
//   0 = after-letter (next letter becomes lower-case)
// Punctuation `.!?` resets to state 2; whitespace returns to 1
// (unless already in 2). Used by chat text capitalisation.
pub fn force_capitalisation_of_words(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut state: i32 = 2;
    for ch in s.chars() {
        let mut c = ch;
        if state == 0 {
            c = c.to_lowercase().next().unwrap_or(c);
        } else if state == 2 || c.is_uppercase() {
            c = to_title_case(c);
        }
        if c.is_alphabetic() {
            state = 0;
        } else if c == '.' || c == '?' || c == '!' {
            state = 2;
        } else if !is_space_char(c) {
            state = 1;
        } else if state != 2 {
            state = 1;
        }
        out.push(c);
    }
    out
}

// Java Character.isSpaceChar — true only for the Unicode SPACE_SEPARATOR
// (Zs), LINE_SEPARATOR (Zl) and PARAGRAPH_SEPARATOR (Zp) categories. This
// is narrower than Rust's `char::is_whitespace` (White_Space property),
// which also matches \t \n \v \f \r — chars Java treats as non-space in
// forceCapitalisationOfWords. In the cp1252 chat domain only U+0020 and
// U+00A0 occur, but match the full BMP separator set for fidelity.
fn is_space_char(c: char) -> bool {
    matches!(c,
        '\u{0020}' | '\u{00A0}' | '\u{1680}'
        | '\u{2000}'..='\u{200A}' | '\u{2028}' | '\u{2029}'
        | '\u{202F}' | '\u{205F}' | '\u{3000}')
}

// @ObfuscatedName(— StringTools.isPrintable). Verbatim port of
// StringTools.java:267-276: the printable ASCII band (0x20..0x7E), the
// Latin-1 supplement (0xA0..0xFF), and exactly five named cp1252
// extras — € (8364), Œ (338), — (8212), œ (339), Ÿ (376). NOTE: this
// is narrower than can_encode_to_cp1252, which also matches the other
// cp1252 0x80-0x9F punctuation Java's isPrintable rejects.
pub fn is_printable(c: char) -> bool {
    let v = c as u32;
    (0x20..=0x7E).contains(&v)
        || (0xA0..=0xFF).contains(&v)
        || matches!(v, 8364 | 338 | 8212 | 339 | 376)
}

// @ObfuscatedName("cj.r") — JString.cp1252Mapping. The 37-entry
// username alphabet (underscore + a-z + 0-9). Indexed by `userhash %
// 37` during decoding.
pub const USERHASH_ALPHABET: [char; 37] = [
    '_',
    'a','b','c','d','e','f','g','h','i','j','k','l','m','n','o','p','q','r','s','t','u','v','w','x','y','z',
    '0','1','2','3','4','5','6','7','8','9',
];

// @ObfuscatedName("cj.r(Ljava/lang/CharSequence;I)J") — JString.toUserhash.
// Verbatim port of JString.java:22-43. Encodes a username (case-
// insensitive, alphanumeric + underscore) into a single 64-bit hash
// using base-37 packing. Overflow guard at 177_917_621_779_460_413
// matches Java; trailing zero digits are stripped.
pub fn to_userhash(s: &str) -> i64 {
    let mut acc: i64 = 0;
    for ch in s.chars() {
        acc = acc.wrapping_mul(37);
        match ch {
            'A'..='Z' => acc += (ch as i64) + 1 - 65,
            'a'..='z' => acc += (ch as i64) + 1 - 97,
            '0'..='9' => acc += (ch as i64) + 27 - 48,
            _ => {}
        }
        if acc >= 177_917_621_779_460_413 {
            break;
        }
    }
    while acc != 0 && acc % 37 == 0 {
        acc /= 37;
    }
    acc
}

// @ObfuscatedName("bk.d(J)Ljava/lang/String;") — JString.toRawUsername(long).
// Verbatim port of JString.java:47-65. Returns the original lowercase
// username (underscores preserved). Returns None for out-of-range or
// zero-tail-stripped hashes.
pub fn to_raw_username(mut hash: i64) -> Option<String> {
    if hash <= 0 || hash >= 6_582_952_005_840_035_281 { return None; }
    if hash % 37 == 0 { return None; }
    let mut chars: Vec<char> = Vec::new();
    while hash != 0 {
        let digit = (hash % 37) as usize;
        hash /= 37;
        chars.push(USERHASH_ALPHABET[digit]);
    }
    chars.reverse();
    Some(chars.into_iter().collect())
}

// @ObfuscatedName("bg.l(J)Ljava/lang/String;") — JString.toScreenName(long).
// Verbatim port of JString.java:69-95. Returns the display-name form:
// the first character is title-cased, and every underscore is replaced
// with a non-breaking space (0xA0) that title-cases the *next* letter.
pub fn to_screen_name(mut hash: i64) -> Option<String> {
    if hash <= 0 || hash >= 6_582_952_005_840_035_281 { return None; }
    if hash % 37 == 0 { return None; }
    // Java builds the string least-significant digit first (so it is the
    // REVERSE of the final name) and, on hitting '_', upper-cases the
    // char at length-1 — i.e. the digit appended in the *previous*
    // iteration, which is the one that ends up immediately AFTER the
    // underscore once reversed. Pre-reversing the digits (as the old
    // code did) made it upper-case the char *before* the underscore
    // instead: "bob_smith" → "BoB smith" rather than "Bob Smith".
    let mut out: Vec<char> = Vec::new();
    while hash != 0 {
        let prev = hash;
        hash /= 37;
        let digit = (prev - hash * 37) as usize;
        let mut ch = USERHASH_ALPHABET[digit];
        if ch == '_' {
            // Java 85-87 — upper-case the previously appended char and
            // swap the underscore for a non-breaking space (0xA0).
            if let Some(last) = out.last_mut()
                && let Some(uc) = last.to_uppercase().next()
            {
                *last = uc;
            }
            ch = '\u{00A0}';
        }
        out.push(ch);
    }
    out.reverse();
    // Java 92 — title-case the very first character.
    if let Some(first) = out.first_mut()
        && let Some(uc) = first.to_uppercase().next()
    {
        *first = uc;
    }
    Some(out.into_iter().collect())
}

// @ObfuscatedName("bs.m(Ljava/lang/CharSequence;I)Ljava/lang/String;") —
// JString.toRawUsername(CharSequence). Convenience wrapper.
pub fn to_raw_username_str(s: &str) -> String {
    to_raw_username(to_userhash(s)).unwrap_or_default()
}

// @ObfuscatedName(— JString.toScreenName(String)). Convenience wrapper.
pub fn to_screen_name_str(s: &str) -> String {
    to_screen_name(to_userhash(s)).unwrap_or_default()
}

// @ObfuscatedName("y.l(CB)C") — JString.toTitleCase. Verbatim port of
// JString.java:98-100. The two latin glyphs at U+00B5 (micro) and
// U+0192 (florin) pass through unchanged — Java avoids the JDK title-
// case rounding bug on those.
pub fn to_title_case(c: char) -> char {
    if c == '\u{00B5}' || c == '\u{0192}' {
        return c;
    }
    c.to_uppercase().next().unwrap_or(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_name_underscore_capitalises_following_letter() {
        // Java JString.toScreenName: first char + the char AFTER each
        // underscore are upper-cased; underscores become 0xA0 (nbsp).
        assert_eq!(to_screen_name_str("bob_smith"), "Bob\u{a0}Smith");
        assert_eq!(to_screen_name_str("a_b_c"), "A\u{a0}B\u{a0}C");
        assert_eq!(to_screen_name_str("zezima"), "Zezima");
        assert_eq!(to_screen_name_str("the_old_nite"), "The\u{a0}Old\u{a0}Nite");
    }

    #[test]
    fn force_capitalisation() {
        // First letter of the line + first letter after . ? ! capitalise;
        // everything else lower-cases.
        assert_eq!(force_capitalisation_of_words("hello world"), "Hello world");
        assert_eq!(force_capitalisation_of_words("hi. there"), "Hi. There");
        assert_eq!(force_capitalisation_of_words("really?yes!ok"), "Really?Yes!Ok");
        // A word starting uppercase keeps that letter (isUpperCase clause).
        assert_eq!(force_capitalisation_of_words("HELLO WORLD"), "Hello World");
        // Tab is NOT a Java space-separator, so the word after ". \t" stays
        // lower-case — `is_whitespace` would wrongly capitalise it.
        assert_eq!(force_capitalisation_of_words("hi.\tthere"), "Hi.\tthere");
    }

    #[test]
    fn is_printable_matches_java() {
        // Printable bands.
        for c in ['A', ' ', '~', '\u{a0}', '\u{ff}'] {
            assert!(is_printable(c), "{c:?} should be printable");
        }
        // The five named cp1252 extras Java whitelists.
        for c in ['\u{20ac}', '\u{152}', '\u{2014}', '\u{153}', '\u{178}'] {
            assert!(is_printable(c), "{c:?} should be printable");
        }
        // Control chars and other cp1252 punctuation Java rejects.
        for c in ['\u{1f}', '\u{7f}', '\u{201a}', '\u{2022}', '\u{2122}'] {
            assert!(!is_printable(c), "{c:?} should NOT be printable");
        }
    }

    #[test]
    fn userhash_roundtrip() {
        // Raw username round-trips (lowercase, underscores preserved).
        for name in ["bob_smith", "zezima", "a_b_c", "player123"] {
            let h = to_userhash(name);
            assert_eq!(to_raw_username(h).as_deref(), Some(name), "raw {name}");
        }
    }
}
