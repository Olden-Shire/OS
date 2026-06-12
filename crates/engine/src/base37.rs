//! Base37 username encoding — 1:1 with Engine-TS `JString`. OSRS packs short
//! usernames (≤12 chars, `[a-z0-9_]`) into a base-37 integer for friends,
//! ignore lists, private messages and account identity. `to_safe_name` /
//! `to_display_name` canonicalise a typed name by round-tripping it through that
//! encoding, so "Zezima", "zezima" and "ZEZIMA" all resolve to one account.
//!
//! Names are ASCII; the encoded value never exceeds 37¹², which fits in a `u64`.

/// 37^12 — one past the largest valid encoded name.
const MAX_BASE37: u64 = 6_582_952_005_840_035_281;

/// Decode table: index 0 = '_', 1..=26 = a-z, 27..=36 = 0-9.
const BASE37_LOOKUP: [u8; 37] = *b"_abcdefghijklmnopqrstuvwxyz0123456789";

/// Pack a name into its base-37 value (Engine-TS `toBase37`). Only A-Z, a-z and
/// 0-9 contribute (case-folded to the same code); other characters advance the
/// position but add nothing. Trailing '_' digits are stripped so equivalent
/// names share one value.
pub fn to_base37(name: &str) -> u64 {
    let name = name.trim();
    let mut l: u64 = 0;
    for (i, c) in name.bytes().enumerate() {
        if i >= 12 {
            break;
        }
        l *= 37;
        if c.is_ascii_uppercase() {
            l += (c + 1 - b'A') as u64;
        } else if c.is_ascii_lowercase() {
            l += (c + 1 - b'a') as u64;
        } else if c.is_ascii_digit() {
            l += (c + 27 - b'0') as u64;
        }
    }
    while l % 37 == 0 && l != 0 {
        l /= 37;
    }
    l
}

/// Decode a base-37 value back to its canonical lowercase name (Engine-TS
/// `fromBase37`). Out-of-range or 37-divisible values (incl. 0) yield
/// `"invalid_name"`.
pub fn from_base37(mut value: u64) -> String {
    if value >= MAX_BASE37 {
        return "invalid_name".to_string();
    }
    if value % 37 == 0 {
        return "invalid_name".to_string();
    }
    let mut chars = [0u8; 12];
    let mut len = 0usize;
    while value != 0 {
        let prev = value;
        value /= 37;
        chars[11 - len] = BASE37_LOOKUP[(prev - value * 37) as usize];
        len += 1;
    }
    // chars are pure ASCII from the lookup table.
    String::from_utf8_lossy(&chars[12 - len..]).into_owned()
}

/// Title-case each whitespace-separated word — first letter upper, rest lower
/// (Engine-TS `toTitleCase`).
pub fn to_title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start_of_word = true;
    for c in s.chars() {
        if c.is_whitespace() {
            start_of_word = true;
            out.push(c);
        } else if start_of_word {
            out.extend(c.to_uppercase());
            start_of_word = false;
        } else {
            out.extend(c.to_lowercase());
        }
    }
    out
}

/// Canonical account name (Engine-TS `toSafeName`): lowercase, only `[a-z0-9_]`,
/// trimmed of trailing underscores — the stable identity for a typed username.
pub fn to_safe_name(name: &str) -> String {
    from_base37(to_base37(name))
}

/// Human-facing name (Engine-TS `toDisplayName`): the safe name with
/// underscores shown as spaces and each word title-cased.
pub fn to_display_name(name: &str) -> String {
    to_title_case(&to_safe_name(name).replace('_', " "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base37_round_trips_and_canonicalises_case() {
        // Case-insensitive: every spelling maps to one value / safe name.
        let v = to_base37("Zezima");
        assert_eq!(to_base37("zezima"), v);
        assert_eq!(to_base37("ZEZIMA"), v);
        assert_eq!(from_base37(v), "zezima");
        assert_eq!(to_safe_name("ZeZiMa"), "zezima");
    }

    #[test]
    fn spaces_become_underscores_via_encoding() {
        // A space isn't a base-37 digit, but the client sends names with '_';
        // an underscore decodes back to '_'.
        assert_eq!(to_safe_name("bob_smith"), "bob_smith");
        assert_eq!(to_display_name("bob_smith"), "Bob Smith");
        assert_eq!(to_display_name("zezima"), "Zezima");
    }

    #[test]
    fn digits_and_length_cap() {
        assert_eq!(to_safe_name("Woox16"), "woox16");
        // Only the first 12 characters are encoded.
        assert_eq!(to_safe_name("abcdefghijklMNOP"), "abcdefghijkl");
    }

    #[test]
    fn invalid_values_report_invalid_name() {
        assert_eq!(from_base37(0), "invalid_name");
        assert_eq!(from_base37(MAX_BASE37), "invalid_name");
        // An empty / all-symbol name encodes to 0.
        assert_eq!(to_base37("   "), 0);
        assert_eq!(to_safe_name("!!!"), "invalid_name");
    }
}
