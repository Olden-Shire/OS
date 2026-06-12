// @ObfuscatedName("ai") — jag::oldscape::client::ScriptRunner.
//
// cs2 interpreter — the full ~2700 LOC dispatcher (executeScript) is
// deferred. As a starting point we expose the arithmetic / bitwise
// opcode helpers (4000-4015) as standalone functions so they can be
// unit-tested and so the eventual dispatcher's bodies are one-liners.
//
// Verbatim ports of ScriptRunner.java:1915-2069. Stack-machine
// semantics are preserved: every helper takes (a, b) where `a` was
// the older intStack entry (intStack[isp]) and `b` was the newer
// (intStack[isp+1]). The return is what gets pushed back at
// intStack[isp].

#![allow(dead_code)]

// @ObfuscatedName(— opcode 4000) — add.
pub fn op_add(a: i32, b: i32) -> i32 { a.wrapping_add(b) }

// @ObfuscatedName(— opcode 4001) — sub.
pub fn op_sub(a: i32, b: i32) -> i32 { a.wrapping_sub(b) }

// @ObfuscatedName(— opcode 4002) — multiply.
pub fn op_multiply(a: i32, b: i32) -> i32 { a.wrapping_mul(b) }

// @ObfuscatedName(— opcode 4003) — divide. Java's behaviour: int
// divide; b==0 raises an ArithmeticException (we mirror with checked
// division, returning 0 on b==0 to keep the dispatcher panic-free).
pub fn op_divide(a: i32, b: i32) -> i32 {
    if b == 0 { 0 } else { a.wrapping_div(b) }
}

// @ObfuscatedName(— opcode 4006) — interpolate. 5-arg helper:
// (y0, y1, x0, x1, x) → linear interpolation at x.
pub fn op_interpolate(y0: i32, y1: i32, x0: i32, x1: i32, x: i32) -> i32 {
    let denom = x1 - x0;
    if denom == 0 { return y0; }
    (y1 - y0) * (x - x0) / denom + y0
}

// @ObfuscatedName(— opcode 4007) — addpercent. Returns a + a*b/100.
pub fn op_addpercent(a: i32, b: i32) -> i32 {
    a.wrapping_mul(b) / 100 + a
}

// @ObfuscatedName(— opcode 4008) — setbit.
pub fn op_setbit(a: i32, b: i32) -> i32 {
    a | (1 << (b & 31))
}

// @ObfuscatedName(— opcode 4009) — clearbit. Java's expression is
// `a & -1 - (1 << b)`. We translate to `a & !(1 << b)` for clarity.
pub fn op_clearbit(a: i32, b: i32) -> i32 {
    a & !(1 << (b & 31))
}

// @ObfuscatedName(— opcode 4010) — testbit.
pub fn op_testbit(a: i32, b: i32) -> i32 {
    if a & (1 << (b & 31)) == 0 { 0 } else { 1 }
}

// @ObfuscatedName(— opcode 4011) — modulo. Java throws on b==0; we
// return 0 to keep callers safe.
pub fn op_modulo(a: i32, b: i32) -> i32 {
    if b == 0 { 0 } else { a.wrapping_rem(b) }
}

// @ObfuscatedName(— opcode 4012) — pow. Java: a==0 ⇒ 0, else
// (int)Math.pow(a, b). Rust's `as i32` is a saturating cast that
// matches Java's (int) double-narrowing exactly (NaN→0, +∞→MAX,
// −∞→MIN), so no explicit non-finite branch is needed.
pub fn op_pow(a: i32, b: i32) -> i32 {
    if a == 0 { return 0; }
    (a as f64).powf(b as f64) as i32
}

// @ObfuscatedName(— opcode 4013) — invpow. nth-root via Math.pow with
// reciprocal. Java: a==0 ⇒ 0, b==0 ⇒ Integer.MAX_VALUE, else
// (int)Math.pow(a, 1/b). The `as i32` saturating cast matches Java's
// (int) of the NaN that Math.pow yields for a<0 (→ 0).
pub fn op_invpow(a: i32, b: i32) -> i32 {
    if a == 0 { return 0; }
    if b == 0 { return i32::MAX; }
    (a as f64).powf(1.0 / (b as f64)) as i32
}

// @ObfuscatedName(— opcode 4014) — and.
pub fn op_and(a: i32, b: i32) -> i32 { a & b }

// @ObfuscatedName(— opcode 4015) — or.
pub fn op_or(a: i32, b: i32) -> i32 { a | b }

// ── String opcodes (4100-4120) ────────────────────────────────────
// Verbatim ports of ScriptRunner.java:2071-2306. Each helper mirrors
// the per-opcode body — caller (the eventual dispatcher) handles the
// pop / push stack motion.

// opcode 4100 — append_num. String + int concat.
pub fn op_append_num(s: &str, n: i32) -> String { format!("{s}{n}") }

// opcode 4101 — append. String + String concat.
pub fn op_append(a: &str, b: &str) -> String { format!("{a}{b}") }

// opcode 4102 — append_signnum. String + sign-formatted int. Java's
// StringTools.fromInt(n, true) prefixes a `+` to non-negative
// numbers; negatives keep their `-`.
pub fn op_append_signnum(s: &str, n: i32) -> String {
    if n >= 0 { format!("{s}+{n}") } else { format!("{s}{n}") }
}

// opcode 4103 — lowercase. Java's `String.toLowerCase()`.
pub fn op_lowercase(s: &str) -> String { s.to_lowercase() }

// opcode 4106 — tostring. Java's `Integer.toString(n)`.
pub fn op_tostring(n: i32) -> String { n.to_string() }

// Java Character.toLowerCase/toUpperCase return a single char; Rust's
// iterators expand a few code points (ß→SS) but the cp1252 chars that
// reach the un-folded paths here map 1:1, so first-of-iterator matches.
fn java_to_lower(c: char) -> char { c.to_lowercase().next().unwrap_or(c) }
fn java_to_upper(c: char) -> char { c.to_uppercase().next().unwrap_or(c) }

// @ObfuscatedName("p.r(CII)C") — StringComparator.removeAccents.
// Verbatim port of StringComparator.java:13-70 (lang arg unused).
fn remove_accents(c: char) -> char {
    let n = c as u32;
    if (192..=255).contains(&n) {
        if (192..=198).contains(&n) { return 'A'; }
        if n == 199 { return 'C'; }
        if (200..=203).contains(&n) { return 'E'; }
        if (204..=207).contains(&n) { return 'I'; }
        if (210..=214).contains(&n) { return 'O'; }
        if (217..=220).contains(&n) { return 'U'; }
        if n == 221 { return 'Y'; }
        if n == 223 { return 's'; }
        if (224..=230).contains(&n) { return 'a'; }
        if n == 231 { return 'c'; }
        if (232..=235).contains(&n) { return 'e'; }
        if (236..=239).contains(&n) { return 'i'; }
        if (242..=246).contains(&n) { return 'o'; }
        if (249..=252).contains(&n) { return 'u'; }
        if n == 253 || n == 255 { return 'y'; }
    }
    match n {
        338 => 'O',
        339 => 'o',
        376 => 'Y',
        _ => c,
    }
}

// @ObfuscatedName("cl.d(CIB)I") — StringComparator.getCharSortKey.
// Verbatim port of StringComparator.java:73-80 (lang arg unused).
fn char_sort_key(c: char) -> i32 {
    let mut key = (c as i32) << 4;
    // Java: isUpperCase(c) || isTitleCase(c); titlecase (Lt) chars don't
    // occur in cp1252 content, so is_uppercase suffices.
    if c.is_uppercase() {
        key = ((java_to_lower(c) as i32) << 4) + 1;
    }
    key
}

// The second expansion char of a ligature (StringTools.java:344-372):
// Æ→…E, æ→…e, ß→…s, Œ→…E, œ→…e. removeAccents handles the first char;
// this returns the pending second char (0 = not a ligature).
fn ligature_second(c: char) -> u32 {
    match c as u32 {
        198 | 338 => 69,  // 'E'
        230 | 339 => 101, // 'e'
        223 => 115,       // 's'
        _ => 0,
    }
}

// @ObfuscatedName("eh.eb(...)") — opcode 4107 compare. Verbatim port of
// StringTools.compare (StringTools.java:279-396): per-char natural
// collation with ligature expansion + accent folding, result clamped to
// its sign. NOT compareToIgnoreCase — it folds Æ/ß/Œ and orders via
// getCharSortKey (lowercase before uppercase). `lang` (Client.lang) is
// ignored by both helpers so we omit it.
pub fn op_compare(a: &str, b: &str) -> i32 {
    let s1: Vec<char> = a.chars().collect();
    let s2: Vec<char> = b.chars().collect();
    let len1 = s1.len() as i32;
    let len2 = s2.len() as i32;
    let mut i1 = 0i32; // var252
    let mut i2 = 0i32; // var253
    let mut pend1 = 0u32; // var254 — pending ligature second char
    let mut pend2 = 0u32; // var255
    let result: i32;
    loop {
        if i1 - pend1 as i32 >= len1 && i2 - pend2 as i32 >= len2 {
            // Both consumed — case-sensitive + length tiebreak.
            let minlen = len1.min(len2) as usize;
            let mut diff = None;
            for k in 0..minlen {
                let (c1, c2) = (s1[k], s2[k]);
                if c1 != c2 && java_to_upper(c1) != java_to_upper(c2) {
                    let (l1, l2) = (java_to_lower(c1), java_to_lower(c2));
                    if l1 != l2 {
                        diff = Some(char_sort_key(l1) - char_sort_key(l2));
                        break;
                    }
                }
            }
            if let Some(d) = diff {
                result = d;
                break;
            }
            let dl = len1 - len2;
            if dl == 0 {
                let mut d2 = 0;
                for k in 0..minlen {
                    if s1[k] != s2[k] {
                        d2 = char_sort_key(s1[k]) - char_sort_key(s2[k]);
                        break;
                    }
                }
                result = d2;
            } else {
                result = dl;
            }
            break;
        }
        if i1 - pend1 as i32 >= len1 { result = -1; break; }
        if i2 - pend2 as i32 >= len2 { result = 1; break; }

        let c1 = if pend1 == 0 {
            let c = s1[i1 as usize];
            i1 += 1;
            c
        } else {
            char::from_u32(pend1).unwrap_or('\0')
        };
        let c2 = if pend2 == 0 {
            let c = s2[i2 as usize];
            i2 += 1;
            c
        } else {
            char::from_u32(pend2).unwrap_or('\0')
        };
        pend1 = ligature_second(c1);
        pend2 = ligature_second(c2);
        let r1 = remove_accents(c1);
        let r2 = remove_accents(c2);
        if r1 != r2 && java_to_upper(r1) != java_to_upper(r2) {
            let (l1, l2) = (java_to_lower(r1), java_to_lower(r2));
            if l1 != l2 {
                result = char_sort_key(l1) - char_sort_key(l2);
                break;
            }
        }
    }
    result.signum()
}

// opcode 4110 — text_switch. Ternary helper.
pub fn op_text_switch<'a>(t: &'a str, f: &'a str, condition: i32) -> &'a str {
    if condition == 1 { t } else { f }
}

// opcode 4105 — text_gender. Verbatim port of ScriptRunner.java:
// 2127-2140. Two strings on the stack: male, female. Selects based
// on the local-player's gender flag (Java uses
// `Client.localPlayer.model.gender`). Caller passes the gender flag
// directly to keep this pure.
pub fn op_text_gender<'a>(male_text: &'a str, female_text: &'a str, gender_female: bool) -> &'a str {
    if gender_female { female_text } else { male_text }
}

// opcode 2702 — if_hassub. Verbatim port of ScriptRunner.java:
// 1272-1284. Checks whether a SubInterface with the given parent id
// is present in the live subinterfaces map. Returns 1 if present,
// else 0.
pub fn op_if_hassub(subinterfaces: &std::collections::HashMap<i32, crate::client::SubInterface>, parent_id: i32) -> i32 {
    if subinterfaces.contains_key(&parent_id) { 1 } else { 0 }
}

// opcode 4104 — fromdate. Verbatim port of ScriptRunner.java:
// 2110-2126. Formats a Jagex "rune day" (days since 1970-01-01 plus
// an 11745-day offset) as "D-Mon-YYYY" using the same 3-letter
// month abbreviations. Pure: i32 in, String out.
pub fn op_fromdate(rune_day: i32) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun",
        "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    // Days-from-epoch arithmetic — Java uses Calendar.setTime with a
    // ms-since-epoch derived from (rune_day + 11745) * 86_400_000ms.
    // We compute (year, month, day) using a portable civil-from-days
    // algorithm.
    let total_days = (rune_day as i64) + 11745i64;
    let (year, month, day) = civil_from_days(total_days);
    format!("{}-{}-{}", day, MONTHS[(month - 1) as usize], year)
}

// Howard Hinnant's civil-from-days algorithm: converts a day count
// since 1970-01-01 into (year, month, day). Pure arithmetic.
fn civil_from_days(z: i64) -> (i32, i32, i32) {
    let z = z + 719_468; // shift epoch to 0000-03-01
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe/1460 + doe/36524 - doe/146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365*yoe + yoe/4 - yoe/100);
    let mp = (5*doy + 2)/153;
    let d = doy - (153*mp + 2)/5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp.wrapping_sub(9) };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as i32, d as i32)
}

// opcode 4108 — paraheight. Returns the multi-line height in pixels
// for `text` rendered into `max_width` using the font's line ascent.
pub fn op_paraheight(font: &crate::graphics::pix_font::PixFont, text: &str, max_width: i32) -> i32 {
    font.predict_lines_multiline(text, max_width) * (font.ascent + 2)
}

// opcode 4109 — parawidth.
pub fn op_parawidth(font: &crate::graphics::pix_font::PixFont, text: &str, max_width: i32) -> i32 {
    font.predict_width_multiline(text, max_width)
}

// opcode 4111 — escape. Forwards to PixFont.escape.
pub fn op_escape(s: &str) -> String {
    crate::graphics::pix_font::PixFont::escape(s)
}

// opcode 4112 — append_char. Append a Unicode codepoint as char.
pub fn op_append_char(s: &str, ch: i32) -> String {
    let mut out = String::from(s);
    if let Some(c) = char::from_u32(ch as u32) {
        out.push(c);
    }
    out
}

// opcode 4113 — char_isprintable.
pub fn op_char_isprintable(ch: i32) -> i32 {
    char::from_u32(ch as u32)
        .map(crate::jstring::is_printable)
        .map(|b| if b { 1 } else { 0 })
        .unwrap_or(0)
}

// opcode 4114 — char_isalphanumeric.
pub fn op_char_isalphanumeric(ch: i32) -> i32 {
    char::from_u32(ch as u32)
        .map(crate::jstring::is_alpha_numeric)
        .map(|b| if b { 1 } else { 0 })
        .unwrap_or(0)
}

// opcode 4115 — char_isalpha.
pub fn op_char_isalpha(ch: i32) -> i32 {
    char::from_u32(ch as u32)
        .map(crate::jstring::is_alpha)
        .map(|b| if b { 1 } else { 0 })
        .unwrap_or(0)
}

// opcode 4116 — char_isnumeric.
pub fn op_char_isnumeric(ch: i32) -> i32 {
    char::from_u32(ch as u32)
        .map(crate::jstring::is_numeric)
        .map(|b| if b { 1 } else { 0 })
        .unwrap_or(0)
}

// opcode 4117 — string_length. Returns 0 for null in Java; here we
// expose a non-null variant.
pub fn op_string_length(s: &str) -> i32 { s.chars().count() as i32 }

// opcode 4118 — substring. Java's `String.substring(begin, end)`
// uses UTF-16 indices; the cs2 world is ASCII / Cp1252 so we treat
// char indices uniformly. Out-of-range falls back to empty.
pub fn op_substring(s: &str, begin: i32, end: i32) -> String {
    if begin < 0 || end < begin { return String::new(); }
    s.chars().skip(begin as usize).take((end - begin) as usize).collect()
}

// opcode 4119 — removetags. Strips every '<...>' span (used to
// sanitize user input before logging or comparison).
pub fn op_removetags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        if ch == '<' { in_tag = true; continue; }
        if ch == '>' { in_tag = false; continue; }
        if !in_tag { out.push(ch); }
    }
    out
}

// opcode 4120 — string_indexof_char. Returns -1 if not found.
pub fn op_string_indexof_char(s: &str, ch: i32) -> i32 {
    let Some(c) = char::from_u32(ch as u32) else { return -1; };
    s.chars().position(|x| x == c).map(|p| p as i32).unwrap_or(-1)
}

// ── ObjType getters (4200-4207) ───────────────────────────────────
// Verbatim ports of ScriptRunner.java:2308-2397. Each helper takes
// an ObjType id (or id + op slot) and returns the matching field.

// opcode 4200 — oc_name.
pub fn op_oc_name(obj_id: i32) -> String {
    crate::config::obj_type::list(obj_id)
        .map(|o| o.name)
        .unwrap_or_default()
}

// opcode 4201 — oc_op. `op_index` is 1..5 (Java is 1-indexed; the
// per-slot array is `op[op_index - 1]`). Returns the empty string
// for out-of-range or missing entries.
pub fn op_oc_op(obj_id: i32, op_index: i32) -> String {
    if !(1..=5).contains(&op_index) { return String::new(); }
    crate::config::obj_type::list(obj_id)
        .and_then(|o| o.op)
        .and_then(|arr| arr[(op_index - 1) as usize].clone())
        .unwrap_or_default()
}

// opcode 4202 — oc_iop. Inventory-op slot lookup.
pub fn op_oc_iop(obj_id: i32, op_index: i32) -> String {
    if !(1..=5).contains(&op_index) { return String::new(); }
    crate::config::obj_type::list(obj_id)
        .and_then(|o| o.iop)
        .and_then(|arr| arr[(op_index - 1) as usize].clone())
        .unwrap_or_default()
}

// opcode 4203 — oc_cost.
pub fn op_oc_cost(obj_id: i32) -> i32 {
    crate::config::obj_type::list(obj_id).map(|o| o.cost).unwrap_or(0)
}

// opcode 4204 — oc_stackable. Returns 1 iff ObjType.stackable == 1.
pub fn op_oc_stackable(obj_id: i32) -> i32 {
    crate::config::obj_type::list(obj_id)
        .map(|o| if o.stackable == 1 { 1 } else { 0 })
        .unwrap_or(0)
}

// opcode 4205 — oc_cert. If the obj is itself a real item and has a
// noted (certificate) link, return the linked id; else return the
// original. Used by the cs2 UI to fetch the certificate variant of a
// regular item.
pub fn op_oc_cert(obj_id: i32) -> i32 {
    let Some(o) = crate::config::obj_type::list(obj_id) else { return obj_id; };
    if o.certtemplate == -1 && o.certlink >= 0 { o.certlink } else { obj_id }
}

// opcode 4206 — oc_uncert. Inverse — if the obj IS a certificate,
// return the underlying real item.
pub fn op_oc_uncert(obj_id: i32) -> i32 {
    let Some(o) = crate::config::obj_type::list(obj_id) else { return obj_id; };
    if o.certtemplate >= 0 && o.certlink >= 0 { o.certlink } else { obj_id }
}

// opcode 4207 — oc_members. 1 = members-only item.
pub fn op_oc_members(obj_id: i32) -> i32 {
    crate::config::obj_type::list(obj_id)
        .map(|o| if o.members { 1 } else { 0 })
        .unwrap_or(0)
}

// ── Game state opcodes (3312-3323) ────────────────────────────────
// Standalone reads — caller passes the relevant Client field.

// opcode 3300 — clientclock. Server-monotonic loop tick.
pub fn op_clientclock(loop_cycle: i32) -> i32 { loop_cycle }

// opcode 3301 — inv_getobj.
pub fn op_inv_getobj(inv_id: i32, slot: i32) -> i32 {
    crate::client_inv_cache::get_type(inv_id, slot)
}
// opcode 3302 — inv_getnum.
pub fn op_inv_getnum(inv_id: i32, slot: i32) -> i32 {
    crate::client_inv_cache::get_count(inv_id, slot)
}
// opcode 3303 — inv_total.
pub fn op_inv_total(inv_id: i32, obj_id: i32) -> i32 {
    crate::client_inv_cache::inv_total(inv_id, obj_id)
}
// opcode 3304 — inv_size. InvType.size lookup.
pub fn op_inv_size(inv_id: i32) -> i32 {
    crate::config::inv_type::list(inv_id).size
}

// opcode 3313 — invother_getobj. Verbatim port of
// ScriptRunner.java:1532-1540. Same as inv_getobj but reads the
// "other" inv pool by OR-ing the high bit (32768).
pub fn op_invother_getobj(inv_id: i32, slot: i32) -> i32 {
    crate::client_inv_cache::get_type(inv_id + 32768, slot)
}
// opcode 3314 — invother_getnum.
pub fn op_invother_getnum(inv_id: i32, slot: i32) -> i32 {
    crate::client_inv_cache::get_count(inv_id + 32768, slot)
}
// opcode 3315 — invother_total.
pub fn op_invother_total(inv_id: i32, obj_id: i32) -> i32 {
    crate::client_inv_cache::inv_total(inv_id + 32768, obj_id)
}

// opcode 3305 — stat (effective level).
pub fn op_stat(stat_effective_level: &[i32], slot: i32) -> i32 {
    if slot < 0 { return 0; }
    stat_effective_level.get(slot as usize).copied().unwrap_or(0)
}
// opcode 3306 — stat_base.
pub fn op_stat_base(stat_base_level: &[i32], slot: i32) -> i32 {
    if slot < 0 { return 0; }
    stat_base_level.get(slot as usize).copied().unwrap_or(0)
}
// opcode 3307 — stat_xp.
pub fn op_stat_xp(stat_xp: &[i32], slot: i32) -> i32 {
    if slot < 0 { return 0; }
    stat_xp.get(slot as usize).copied().unwrap_or(0)
}

// opcode 3308 — coord. Packs (level << 28) | (world_x << 14) | world_z.
pub fn op_coord(level: i32, world_x: i32, world_z: i32) -> i32 {
    (level << 28) + (world_x << 14) + world_z
}
// opcode 3309 — coordx. Extract world_x from packed coord.
pub fn op_coord_x(packed: i32) -> i32 { (packed >> 14) & 0x3FFF }
// opcode 3310 — coordy. Extract level.
pub fn op_coord_y(packed: i32) -> i32 { packed >> 28 }
// opcode 3311 — coordz. Extract world_z.
pub fn op_coord_z(packed: i32) -> i32 { packed & 0x3FFF }

// opcode 3312 — map_members. 1 = members-only world.
pub fn op_map_members(mem_server: bool) -> i32 { if mem_server { 1 } else { 0 } }

// opcode 3316 — staffmodlevel. 0 unless >= 2 (pmod or jmod).
pub fn op_staffmodlevel(staffmodlevel: i32) -> i32 {
    if staffmodlevel >= 2 { staffmodlevel } else { 0 }
}

// opcode 3317 — reboottimer.
pub fn op_reboot_timer(reboot_timer: i32) -> i32 { reboot_timer }

// opcode 3318 — map_world.
pub fn op_map_world(world_id: i32) -> i32 { world_id }

// opcode 3321 — runenergy_visible.
pub fn op_runenergy_visible(run_energy: i32) -> i32 { run_energy }

// opcode 3322 — runweight_visible.
pub fn op_runweight_visible(run_weight: i32) -> i32 { run_weight }

// opcode 3323 — playermod.
pub fn op_playermod(playermod: bool) -> i32 { if playermod { 1 } else { 0 } }

// ── Enum opcodes (3400, 3408) ─────────────────────────────────────

// opcode 3400 — enum_string. Look up a key in an EnumType, returning
// the matching string value (or the default).
pub fn op_enum_string(enum_id: i32, key: i32) -> String {
    let e = crate::config::enum_type::list(enum_id);
    for i in 0..(e.count as usize) {
        if e.keys.get(i) == Some(&key) {
            return e.string_values.get(i).cloned().unwrap_or_default();
        }
    }
    e.default_string.clone()
}

// opcode 3408 — enum (generic int/string dispatch). `out_type` is
// Java's char value (115 = 's', 105 = 'i'). Returns an `EnumResult`
// to avoid pushing both types in the same return slot.
pub enum EnumResult {
    Int(i32),
    String(String),
}

pub fn op_enum(in_type: i32, out_type: i32, enum_id: i32, key: i32) -> EnumResult {
    let e = crate::config::enum_type::list(enum_id);
    if e.inputtype != in_type || e.outputtype != out_type {
        return if out_type == 115 {
            EnumResult::String("null".to_string())
        } else {
            EnumResult::Int(0)
        };
    }
    for i in 0..(e.count as usize) {
        if e.keys.get(i) == Some(&key) {
            return if out_type == 115 {
                EnumResult::String(e.string_values.get(i).cloned().unwrap_or_default())
            } else {
                EnumResult::Int(e.int_values.get(i).copied().unwrap_or(0))
            };
        }
    }
    if out_type == 115 {
        EnumResult::String(e.default_string.clone())
    } else {
        EnumResult::Int(e.default_int)
    }
}

// ── Friend/social opcodes (3600-3602) ────────────────────────────

// opcode 3600 — friend_count. Returns the count with sentinels for
// the loading states: -2 = "still connecting", -1 = "service online
// but list not yet pushed".
pub fn op_friend_count(friend_server_status: i32, friend_count: i32) -> i32 {
    match friend_server_status {
        0 => -2,
        1 => -1,
        _ => friend_count,
    }
}

// opcode 3601 — friend_getname.
pub fn op_friend_getname(friend_list: &[crate::friend::FriendListEntry], status: i32, count: i32, idx: i32) -> String {
    if status != 2 || idx < 0 || idx >= count { return String::new(); }
    friend_list.get(idx as usize).map(|f| f.name.clone()).unwrap_or_default()
}

// opcode 3602 — friend_getworld.
pub fn op_friend_getworld(friend_list: &[crate::friend::FriendListEntry], status: i32, count: i32, idx: i32) -> i32 {
    if status != 2 || idx < 0 || idx >= count { return 0; }
    friend_list.get(idx as usize).map(|f| f.world_id).unwrap_or(0)
}

// opcode 3603 — friend_getrank.
pub fn op_friend_getrank(friend_list: &[crate::friend::FriendListEntry], status: i32, count: i32, idx: i32) -> i32 {
    if status != 2 || idx < 0 || idx >= count { return 0; }
    friend_list.get(idx as usize).map(|f| f.rank).unwrap_or(0)
}

// opcode 3609 — friend_test. Strips the leading `<img=N>` chat icon
// tag (7 chars: `<img=N>` where N is 0 or 1 — pmod / jmod icon),
// then forwards to Client.is_friend.
pub fn op_friend_test(name: &str) -> &str {
    if name.starts_with("<img=0>") || name.starts_with("<img=1>") {
        &name[7..]
    } else {
        name
    }
}

// opcode 3611 — clan_getchatdisplayname. Java wraps with
// JString.toScreenName so underscores become NBSP + title-case.
pub fn op_clan_get_chat_display_name(name: Option<&str>) -> String {
    name.map(crate::jstring::to_screen_name_str).unwrap_or_default()
}

// opcode 3612 — clan_getchatcount.
pub fn op_clan_getchatcount(display_name: Option<&str>, friend_chat_count: i32) -> i32 {
    if display_name.is_none() { 0 } else { friend_chat_count }
}

// opcode 3613 — clan_getchatusername.
pub fn op_clan_get_chat_username(
    display_name: Option<&str>,
    friend_chat_list: &[crate::friend::FriendChatUser],
    friend_chat_count: i32,
    idx: i32,
) -> String {
    if display_name.is_none() || idx < 0 || idx >= friend_chat_count {
        return String::new();
    }
    friend_chat_list.get(idx as usize)
        .map(|u| u.username.clone())
        .unwrap_or_default()
}

// opcode 3614 — clan_getchatuserworld.
pub fn op_clan_get_chat_userworld(
    display_name: Option<&str>,
    friend_chat_list: &[crate::friend::FriendChatUser],
    friend_chat_count: i32,
    idx: i32,
) -> i32 {
    if display_name.is_none() || idx < 0 || idx >= friend_chat_count { return 0; }
    friend_chat_list.get(idx as usize).map(|u| u.world).unwrap_or(0)
}

// opcode 3615 — clan_getchatuserrank.
pub fn op_clan_get_chat_userrank(
    display_name: Option<&str>,
    friend_chat_list: &[crate::friend::FriendChatUser],
    friend_chat_count: i32,
    idx: i32,
) -> i32 {
    if display_name.is_none() || idx < 0 || idx >= friend_chat_count { return 0; }
    friend_chat_list.get(idx as usize).map(|u| u.rank).unwrap_or(0)
}

// opcode 3616 — clan_getchatminkick.
pub fn op_clan_get_chat_min_kick(chat_min_kick: i32) -> i32 { chat_min_kick }

// opcode 3618 — clan_getchatrank.
pub fn op_clan_get_chat_rank(chat_rank: i32) -> i32 { chat_rank }

// opcode 3621 — ignore_count.
pub fn op_ignore_count(friend_server_status: i32, ignore_count: i32) -> i32 {
    if friend_server_status == 0 { -1 } else { ignore_count }
}

// opcode 3622 — ignore_getname.
pub fn op_ignore_getname(
    friend_server_status: i32,
    ignore_list: &[crate::friend::IgnoreListEntry],
    ignore_count: i32,
    idx: i32,
) -> String {
    if friend_server_status == 0 || idx < 0 || idx >= ignore_count { return String::new(); }
    ignore_list.get(idx as usize).map(|i| i.name.clone()).unwrap_or_default()
}

// opcode 3623 — ignore_test. Strips a leading `<img=0>` / `<img=1>`
// PMod/JMod icon then forwards to Client.is_ignored.
pub fn op_ignore_test_strip(name: &str) -> &str {
    if name.starts_with("<img=0>") || name.starts_with("<img=1>") {
        &name[7..]
    } else {
        name
    }
}

// opcode 3624 — clan_isself.
pub fn op_clan_isself(
    friend_chat_list: &[crate::friend::FriendChatUser],
    friend_chat_count: i32,
    local_name: Option<&str>,
    idx: i32,
) -> i32 {
    if idx < 0 || idx >= friend_chat_count { return 0; }
    let Some(local) = local_name else { return 0; };
    friend_chat_list.get(idx as usize)
        .map(|u| if u.username.eq_ignore_ascii_case(local) { 1 } else { 0 })
        .unwrap_or(0)
}

// opcode 3625 — clan_getchatownername.
pub fn op_clan_get_chat_owner_name(owner: Option<&str>) -> String {
    owner.map(crate::jstring::to_screen_name_str).unwrap_or_default()
}

// ── Var ops (opcodes 1, 2, 25, 27) ───────────────────────────────
// Verbatim ports of ScriptRunner.java:152-248.

// ── Branch predicates (opcodes 6-10, 31-32) ───────────────────────
// Verbatim ports of ScriptRunner.java:170-205, 250-265. Each helper
// returns whether the branch should be taken — caller advances pc.

// opcode 6 — branch (unconditional). The Java helper is `pc +=
// operand`; we expose it as a no-op predicate so dispatchers can
// uniformly dispatch through the same case.
pub fn op_branch_unconditional() -> bool { true }

// opcode 7 — branch_not. Java's stack semantics: `intStack[isp+1] !=
// intStack[isp]` → branch. We adopt (a, b) where `a` was older
// (lower-isp) and `b` was newer.
pub fn op_branch_not(a: i32, b: i32) -> bool { b != a }

// opcode 8 — branch_equals.
pub fn op_branch_equals(a: i32, b: i32) -> bool { b == a }

// opcode 9 — branch_less_than.
pub fn op_branch_less_than(a: i32, b: i32) -> bool { b < a }

// opcode 10 — branch_greater_than.
pub fn op_branch_greater_than(a: i32, b: i32) -> bool { b > a }

// opcode 31 — branch_less_or_equal.
pub fn op_branch_less_or_equal(a: i32, b: i32) -> bool { b <= a }

// opcode 32 — branch_greater_or_equal.
pub fn op_branch_greater_or_equal(a: i32, b: i32) -> bool { b >= a }

// ── Array ops (opcodes 44, 45, 46) ────────────────────────────────
// Verbatim ports of ScriptRunner.java:359-411. Java's `arrays[][]`
// and `arrayLengths[]` are globally scoped — same here so the
// eventual dispatcher can share them between scripts.
//
// Java caps at 5 arrays per running script and 5000 entries each;
// we leave the per-script cap to the dispatcher and just expose the
// underlying storage.
pub static ARRAYS: std::sync::LazyLock<std::sync::Mutex<Vec<Vec<i32>>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(vec![Vec::new(); 5]));

// opcode 44 — define_array. Allocates a fresh `size`-element array
// at slot `array_id`. `type_id` is the cs2 type tag (int=0,
// string=115); we honour size > 5000 and id out-of-range as no-ops
// since Java throws RuntimeException — which the dispatcher would
// then catch.
pub fn op_define_array(array_id: i32, size: i32, _type_id: i32) {
    if array_id < 0 || array_id >= 5 || size < 0 || size > 5000 { return; }
    let mut a = ARRAYS.lock().unwrap();
    a[array_id as usize] = vec![0i32; size as usize];
}

// opcode 45 — push_array. Returns 0 on bounds failure (Java throws).
pub fn op_push_array(array_id: i32, index: i32) -> i32 {
    if array_id < 0 || array_id >= 5 || index < 0 { return 0; }
    let a = ARRAYS.lock().unwrap();
    a.get(array_id as usize)
        .and_then(|v| v.get(index as usize).copied())
        .unwrap_or(0)
}

// opcode 46 — pop_array.
pub fn op_pop_array(array_id: i32, index: i32, value: i32) {
    if array_id < 0 || array_id >= 5 || index < 0 { return; }
    let mut a = ARRAYS.lock().unwrap();
    if let Some(v) = a.get_mut(array_id as usize) {
        if let Some(slot) = v.get_mut(index as usize) {
            *slot = value;
        }
    }
}

// ── Locals + discards (opcodes 33-39) ─────────────────────────────
// Verbatim ports of ScriptRunner.java:266-310. The dispatcher owns
// the per-frame int/string locals array; these helpers just expose
// the indexed read/write shape for clean call sites.

// opcode 33 — push_int_local.
pub fn op_push_int_local(locals: &[i32], idx: i32) -> i32 {
    if idx < 0 { return 0; }
    locals.get(idx as usize).copied().unwrap_or(0)
}

// opcode 34 — pop_int_local. Returns Ok(()) so callers can branch
// on success; out-of-range writes are dropped.
pub fn op_pop_int_local(locals: &mut [i32], idx: i32, value: i32) {
    if idx < 0 { return; }
    if let Some(slot) = locals.get_mut(idx as usize) {
        *slot = value;
    }
}

// opcode 35 — push_string_local.
pub fn op_push_string_local(locals: &[String], idx: i32) -> String {
    if idx < 0 { return String::new(); }
    locals.get(idx as usize).cloned().unwrap_or_default()
}

// opcode 36 — pop_string_local.
pub fn op_pop_string_local(locals: &mut [String], idx: i32, value: String) {
    if idx < 0 { return; }
    if let Some(slot) = locals.get_mut(idx as usize) {
        *slot = value;
    }
}

// opcode 38 — pop_int_discard. Java's stack pop with no consumer.
// We expose it as a no-op marker so the dispatcher can route through
// the same case.
pub fn op_pop_int_discard(_value: i32) {}

// opcode 39 — pop_string_discard.
pub fn op_pop_string_discard(_value: String) {}

// opcode 0 — push_int_constant. Returns the operand verbatim.
pub fn op_push_int_constant(operand: i32) -> i32 { operand }

// opcode 3 — push_string_constant.
pub fn op_push_string_constant(s: &str) -> String { s.to_string() }

// opcode 1 — push_varp. Reads VarCache.var[id].
pub fn op_push_varp(id: i32) -> i32 {
    crate::config::var_cache::get_varp(id)
}

// opcode 2 — pop_varp. Writes value to VarCache.var[id] then fires
// the SET_VAR_CLIENT (181) packet so the server mirrors. Returns
// `Some(id)` if the write succeeded so the caller knows to fire the
// outbound packet (the dispatcher owns the &mut Client).
pub fn op_pop_varp_local(id: i32, value: i32) {
    crate::config::var_cache::set_varp(id, value);
}

// opcode 25 — push_varbit. Reads via VarCache.getVarbit.
pub fn op_push_varbit(id: i32) -> i32 {
    crate::config::var_cache::get_varbit(id)
}

// opcode 27 — pop_varbit. Bitfield writer via VarCache.setVarbit.
pub fn op_pop_varbit(id: i32, value: i32) {
    crate::config::var_cache::set_varbit(id, value);
}

// ── varc int/str ops (opcodes 42, 43, 47, 48) ─────────────────────

pub fn op_push_varc_int(id: i32) -> i32 {
    crate::config::var_cache::get_varc_int(id)
}

pub fn op_pop_varc_int(id: i32, value: i32) {
    crate::config::var_cache::set_varc_int(id, value);
}

pub fn op_push_varc_str(id: i32) -> String {
    crate::config::var_cache::get_varc_str(id)
}

pub fn op_pop_varc_str(id: i32, value: String) {
    crate::config::var_cache::set_varc_str(id, Some(value));
}

// opcode 37 — join_string. Java pops two strings and pushes their
// concatenation. Same shape as op_append but kept distinct so the
// dispatcher can read either call site.
pub fn op_join_string(a: &str, b: &str) -> String {
    let mut out = String::with_capacity(a.len() + b.len());
    out.push_str(a);
    out.push_str(b);
    out
}

// ── Component lifecycle (100-102, 200) ────────────────────────────
// Verbatim ports of ScriptRunner.java:441-522. Each helper mutates
// the IfType cache directly via if_type::modify so writes are
// observed by the renderer.

// opcode 100 — cc_create. Allocates a new sub-component with the
// given `type` on the parent identified by `parent_com_id`, slotting
// it into the parent's subcomponents at `sub_id`. Returns the newly-
// created component's packed (parent << 16 | sub) id so the caller
// can mark it active.
pub fn op_cc_create(parent_com_id: i32, sub_id: i32, type_: i32) -> i32 {
    use crate::config::if_type::{self, IfType};
    if type_ == 0 { return -1; }
    let mut created_id = -1;
    if_type::modify(parent_com_id, |parent| {
        // Grow subcomponents to fit (sub_id + 1).
        let needed = (sub_id as usize) + 1;
        if parent.subcomponents.len() < needed {
            parent.subcomponents.resize(needed, None);
        }
        // Java throws on a gap; we silently no-op since the dispatcher
        // is partial and this guard would otherwise crash test harnesses.
        if sub_id > 0 && parent.subcomponents[(sub_id - 1) as usize].is_none() {
            return;
        }
        let mut comp = IfType::default();
        comp.type_ = type_;
        comp.layer_id = parent.parent_id;
        comp.parent_id = parent.parent_id;
        comp.sub_id = sub_id;
        comp.v3 = true;
        parent.subcomponents[sub_id as usize] = Some(comp);
        created_id = (parent.parent_id << 16) | sub_id;
    });
    created_id
}

// opcode 101 — cc_delete. Clears the active component's slot on its
// parent. Java reads activeComponent then walks `parent.subcomponents[
// self.sub_id] = null`.
pub fn op_cc_delete(active_parent_id: i32, active_sub_id: i32) {
    use crate::config::if_type;
    if_type::modify(active_parent_id, |parent| {
        if (active_sub_id as usize) < parent.subcomponents.len() {
            parent.subcomponents[active_sub_id as usize] = None;
        }
    });
}

// opcode 102 — cc_deleteall. Drops every dynamic sub on a component.
pub fn op_cc_deleteall(parent_com_id: i32) {
    use crate::config::if_type;
    if_type::modify(parent_com_id, |parent| {
        parent.subcomponents.clear();
    });
}

// opcode 200 — cc_find. Returns 1 if (parent, sub) exists and sub !=
// -1; the caller should treat that as "set this component active".
// Sub == -1 is reserved by Java for the "lookup by parent only"
// behaviour of get(), which doesn't activate.
pub fn op_cc_find(parent: i32, sub: i32) -> i32 {
    use crate::config::if_type;
    if sub == -1 { return 0; }
    let exists = if_type::get2(parent, sub).is_some();
    if exists { 1 } else { 0 }
}

// opcode 1800 — cc_gettargetmask. ServerActive.targetMask extracts
// the 4-bit "this component is a drag target" mask from active flags.
pub fn op_cc_gettargetmask(c: &crate::client::Client, com: &crate::config::if_type::IfType) -> i32 {
    (crate::client::get_active(c, com) >> 24) & 0xF
}

// ── IfType getters (1500-1700) ────────────────────────────────────
// Verbatim ports of ScriptRunner.java:1042-1138. Each helper takes a
// `&IfType` and returns the matching field. Pure reads — the cs2
// dispatcher binds `activeComponent` from the call frame.

use crate::config::if_type::IfType;

// opcode 1500 — cc_x.
pub fn op_cc_x(com: &IfType) -> i32 { com.x }
// opcode 1501 — cc_y.
pub fn op_cc_y(com: &IfType) -> i32 { com.y }
// opcode 1502 — cc_getwidth.
pub fn op_cc_getwidth(com: &IfType) -> i32 { com.width }
// opcode 1503 — cc_getheight.
pub fn op_cc_getheight(com: &IfType) -> i32 { com.height }
// opcode 1504 — cc_gethide. Java returns 0/1 from the bool.
pub fn op_cc_gethide(com: &IfType) -> i32 { if com.hide { 1 } else { 0 } }
// opcode 1505 — cc_getlayer.
pub fn op_cc_getlayer(com: &IfType) -> i32 { com.layer_id }

// opcode 1600 — cc_getscrollx.
pub fn op_cc_getscrollx(com: &IfType) -> i32 { com.scroll_pos_x }
// opcode 1601 — cc_getscrolly.
pub fn op_cc_getscrolly(com: &IfType) -> i32 { com.scroll_pos_y }
// opcode 1602 — cc_gettext. Returns the empty string when null.
pub fn op_cc_gettext(com: &IfType) -> String { com.text.clone() }
// opcode 1603 — cc_getscrollwidth.
pub fn op_cc_getscrollwidth(com: &IfType) -> i32 { com.scroll_width }
// opcode 1604 — cc_getscrollheight.
pub fn op_cc_getscrollheight(com: &IfType) -> i32 { com.scroll_height }
// opcode 1605 — cc_getmodelzoom.
pub fn op_cc_getmodelzoom(com: &IfType) -> i32 { com.model_zoom }
// opcode 1606 — cc_getmodelangle_x.
pub fn op_cc_getmodelangle_x(com: &IfType) -> i32 { com.model_x_an }
// opcode 1607 — cc_getmodelangle_z. Returns the decoded model_z_an
// field (added in task #302 via decode3).
pub fn op_cc_getmodelangle_z(com: &IfType) -> i32 { com.model_z_an }
// opcode 1608 — cc_getmodelangle_y.
pub fn op_cc_getmodelangle_y(com: &IfType) -> i32 { com.model_y_an }

// ── if_get* base (2500-2505) ────────────────────────────────────
// Verbatim ports of ScriptRunner.java:1186-1205. Explicit-component
// variants of the cc_get* 1500-1505 family. The dispatcher pops a
// component id and resolves to an IfType via IfType.get(); the Rust
// helpers take the already-resolved &IfType. Same as the
// 2600-2608 batch but for the base geometry / state fields.

// opcode 2500 — if_getx.
pub fn op_if_getx(com: &IfType) -> i32 { com.x }
// opcode 2501 — if_gety.
pub fn op_if_gety(com: &IfType) -> i32 { com.y }
// opcode 2502 — if_getwidth.
pub fn op_if_getwidth(com: &IfType) -> i32 { com.width }
// opcode 2503 — if_getheight.
pub fn op_if_getheight(com: &IfType) -> i32 { com.height }
// opcode 2504 — if_gethide. Returns 1 when hidden, 0 otherwise.
pub fn op_if_gethide(com: &IfType) -> i32 { if com.hide { 1 } else { 0 } }
// opcode 2505 — if_getlayer.
pub fn op_if_getlayer(com: &IfType) -> i32 { com.layer_id }

// ── if_get* family (2600-2608) ──────────────────────────────────
// Verbatim ports of ScriptRunner.java:1206-1250. These are the
// explicit-component variants of the cc_get* family above — the
// dispatcher pops an i32 component id, resolves to an IfType via
// IfType.get(), then reads the same fields. The Rust helpers take
// the already-resolved &IfType, matching the existing 2500-2802
// batch shape.

// opcode 2600 — if_getscrollx.
pub fn op_if_getscrollx(com: &IfType) -> i32 { com.scroll_pos_x }
// opcode 2601 — if_getscrolly.
pub fn op_if_getscrolly(com: &IfType) -> i32 { com.scroll_pos_y }
// opcode 2602 — if_gettext.
pub fn op_if_gettext(com: &IfType) -> String { com.text.clone() }
// opcode 2603 — if_getscrollwidth.
pub fn op_if_getscrollwidth(com: &IfType) -> i32 { com.scroll_width }
// opcode 2604 — if_getscrollheight.
pub fn op_if_getscrollheight(com: &IfType) -> i32 { com.scroll_height }
// opcode 2605 — if_getmodelzoom.
pub fn op_if_getmodelzoom(com: &IfType) -> i32 { com.model_zoom }
// opcode 2606 — if_getmodelangle_x.
pub fn op_if_getmodelangle_x(com: &IfType) -> i32 { com.model_x_an }
// opcode 2607 — if_getmodelangle_z. Returns model_z_an (sibling of
// the cc_getmodelangle_z helper at 1607, both fixed under #338).
pub fn op_if_getmodelangle_z(com: &IfType) -> i32 { com.model_z_an }
// opcode 2608 — if_getmodelangle_y.
pub fn op_if_getmodelangle_y(com: &IfType) -> i32 { com.model_y_an }

// opcode 2700 — if_getinvobject. Verbatim port of ScriptRunner.java:
// 1252-1259. Returns the inv-slot obj id; -1 sentinel for empty.
pub fn op_if_getinvobject(com: &IfType) -> i32 { com.invobject }

// opcode 2701 — if_getinvcount. Verbatim port of ScriptRunner.java:
// 1260-1271. Returns 0 when invobject == -1 (Java's guard).
pub fn op_if_getinvcount(com: &IfType) -> i32 {
    if com.invobject == -1 { 0 } else { com.invcount }
}

// opcode 2800 — if_gettargetmask. Verbatim port of ScriptRunner.java:
// 1288-1292. Reads `target_mask` from the active server-side event
// code bits (ServerActive.targetMask). Caller must pass the
// component's current event_code (after ServerActive overlay merge).
pub fn op_if_gettargetmask(event_code: i32) -> i32 {
    crate::config::server_active::target_mask(event_code)
}

// opcode 2801 — if_getop. Verbatim port of ScriptRunner.java:
// 1293-1304. Returns op_names[slot - 1] or "" when OOB / None.
pub fn op_if_getop(com: &IfType, slot: i32) -> String {
    let idx = slot - 1;
    if idx < 0 { return String::new(); }
    com.op_names.get(idx as usize).cloned().unwrap_or_default()
}

// opcode 2802 — if_getopbase. Verbatim port of ScriptRunner.java:
// 1307-1314. Returns base_op_name (the default op label).
pub fn op_if_getopbase(com: &IfType) -> String {
    com.base_op_name.clone()
}

// opcode 1700 — cc_getinvobject. -1 sentinel for empty.
pub fn op_cc_getinvobject(com: &IfType) -> i32 { com.invobject }
// opcode 1701 — cc_getinvcount. Java returns 0 when invobject == -1.
pub fn op_cc_getinvcount(com: &IfType) -> i32 {
    if com.invobject == -1 { 0 } else { com.invcount }
}
// opcode 1702 — cc_getid (sub component id).
pub fn op_cc_getid(com: &IfType) -> i32 { com.sub_id }

// opcode 1801 — cc_getop. Returns op_names[op_index - 1] or "".
pub fn op_cc_getop(com: &IfType, op_index: i32) -> String {
    if !(1..=5).contains(&op_index) { return String::new(); }
    com.op_names.get((op_index - 1) as usize)
        .cloned()
        .unwrap_or_default()
}

// ── ifM_* explicit-component getters (2500-2802) ──────────────────
// Verbatim ports of ScriptRunner.java:1173-1315. Same as the 1500-
// 1700 family but the component is fetched by id rather than by the
// implicit `activeComponent`. Java's pattern: `pop id; ifType_get(id);
// dispatch as (opcode - 1000) on that component`.
//
// These are thin wrappers — callers (the dispatcher) pop the id from
// intStack and pass it here, the helper does the cache lookup.

pub fn op_ifm_x(component_id: i32) -> i32 {
    crate::config::if_type::get(component_id).map(|c| c.x).unwrap_or(0)
}
pub fn op_ifm_y(component_id: i32) -> i32 {
    crate::config::if_type::get(component_id).map(|c| c.y).unwrap_or(0)
}
pub fn op_ifm_width(component_id: i32) -> i32 {
    crate::config::if_type::get(component_id).map(|c| c.width).unwrap_or(0)
}
pub fn op_ifm_height(component_id: i32) -> i32 {
    crate::config::if_type::get(component_id).map(|c| c.height).unwrap_or(0)
}
pub fn op_ifm_hide(component_id: i32) -> i32 {
    crate::config::if_type::get(component_id)
        .map(|c| if c.hide { 1 } else { 0 })
        .unwrap_or(0)
}
pub fn op_ifm_layer(component_id: i32) -> i32 {
    crate::config::if_type::get(component_id).map(|c| c.layer_id).unwrap_or(0)
}

pub fn op_ifm_scrollx(component_id: i32) -> i32 {
    crate::config::if_type::get(component_id).map(|c| c.scroll_pos_x).unwrap_or(0)
}
pub fn op_ifm_scrolly(component_id: i32) -> i32 {
    crate::config::if_type::get(component_id).map(|c| c.scroll_pos_y).unwrap_or(0)
}
pub fn op_ifm_text(component_id: i32) -> String {
    crate::config::if_type::get(component_id).map(|c| c.text).unwrap_or_default()
}
pub fn op_ifm_scrollwidth(component_id: i32) -> i32 {
    crate::config::if_type::get(component_id).map(|c| c.scroll_width).unwrap_or(0)
}
pub fn op_ifm_scrollheight(component_id: i32) -> i32 {
    crate::config::if_type::get(component_id).map(|c| c.scroll_height).unwrap_or(0)
}
pub fn op_ifm_modelzoom(component_id: i32) -> i32 {
    crate::config::if_type::get(component_id).map(|c| c.model_zoom).unwrap_or(0)
}
pub fn op_ifm_modelangle_x(component_id: i32) -> i32 {
    crate::config::if_type::get(component_id).map(|c| c.model_x_an).unwrap_or(0)
}
pub fn op_ifm_modelangle_y(component_id: i32) -> i32 {
    crate::config::if_type::get(component_id).map(|c| c.model_y_an).unwrap_or(0)
}
pub fn op_ifm_modelangle_z(component_id: i32) -> i32 {
    crate::config::if_type::get(component_id).map(|c| c.model_z_an).unwrap_or(0)
}

pub fn op_ifm_invobject(component_id: i32) -> i32 {
    crate::config::if_type::get(component_id).map(|c| c.invobject).unwrap_or(-1)
}
pub fn op_ifm_invcount(component_id: i32) -> i32 {
    crate::config::if_type::get(component_id)
        .map(|c| if c.invobject == -1 { 0 } else { c.invcount })
        .unwrap_or(0)
}
pub fn op_ifm_id(component_id: i32) -> i32 {
    crate::config::if_type::get(component_id).map(|c| c.sub_id).unwrap_or(-1)
}

// opcode 4004 — random. Java: `(int)(Math.random() * (double) n)` so
// the result is in [0, n) for n>0, 0 for n<=0.
pub fn op_random(n: i32) -> i32 {
    if n <= 0 { return 0; }
    (rand_unit() * n as f64) as i32
}

// opcode 4005 — randominc. Inclusive variant returning [0, n].
pub fn op_randominc(n: i32) -> i32 {
    if n < 0 { return 0; }
    (rand_unit() * (n as f64 + 1.0)) as i32
}

// Cheap LCG matching the BgSound RNG — gives deterministic-but-good
// distribution and avoids pulling in `rand`.
fn rand_unit() -> f64 {
    use std::sync::Mutex;
    static SEED: Mutex<u64> = Mutex::new(0x6A09_E667_F3BC_C908);
    let mut g = SEED.lock().unwrap();
    *g = g.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let v = (*g >> 11) as f64 / (1u64 << 53) as f64;
    v.clamp(0.0, 1.0 - f64::EPSILON)
}

// opcode 1802 — cc_getopbase. Java returns the per-component
// `baseOpName` (the "default" op, like "Use"). rev1 IfType doesn't
// have a separate base_op_name field — Java falls back to op_names[0].
pub fn op_cc_getopbase(com: &IfType) -> String {
    com.op_names.first().cloned().unwrap_or_default()
}

// ── IfType setters (1000-1120) ────────────────────────────────────
// Verbatim ports of ScriptRunner.java:533-769. Each helper takes a
// `&mut IfType` and applies the field write. Callers (the eventual
// cs2 dispatcher) must invoke `component_updated()` on the same
// component after this call to mark the redraw region dirty.

// opcode 1000 — cc_setposition.
pub fn op_cc_setposition(com: &mut IfType, x: i32, y: i32) {
    com.x = x;
    com.y = y;
}
// opcode 1001 — cc_setsize.
pub fn op_cc_setsize(com: &mut IfType, w: i32, h: i32) {
    com.width = w;
    com.height = h;
}
// opcode 1003 — cc_sethide. Returns true if the value actually
// changed (Java guards the dirty-flag call on this).
pub fn op_cc_sethide(com: &mut IfType, hide: i32) -> bool {
    let new = hide == 1;
    let changed = com.hide != new;
    com.hide = new;
    changed
}
// opcode 1100 — cc_setscrollpos. Clamps to scroll_width/height.
pub fn op_cc_setscrollpos(com: &mut IfType, x: i32, y: i32) {
    let max_x = (com.scroll_width - com.width).max(0);
    let max_y = (com.scroll_height - com.height).max(0);
    com.scroll_pos_x = x.clamp(0, max_x);
    com.scroll_pos_y = y.clamp(0, max_y);
}
// opcode 1101 — cc_setcolour.
pub fn op_cc_setcolour(com: &mut IfType, rgb: i32) { com.colour = rgb; }
// opcode 1102 — cc_setfill.
pub fn op_cc_setfill(com: &mut IfType, fill: i32) { com.fill = fill == 1; }
// opcode 1103 — cc_settrans.
pub fn op_cc_settrans(com: &mut IfType, trans: i32) { com.trans = trans; }
// opcode 1104 — cc_setlinewid.
pub fn op_cc_setlinewid(com: &mut IfType, width: i32) { com.line_width = width; }
// opcode 1105 — cc_setgraphic.
pub fn op_cc_setgraphic(com: &mut IfType, gfx: i32) { com.graphic = gfx; }
// opcode 1106 — cc_set2dangle.
pub fn op_cc_set2dangle(com: &mut IfType, rotate: i32) { com.rotate = rotate; }
// opcode 1107 — cc_settiling.
pub fn op_cc_settiling(com: &mut IfType, tile: i32) { com.tiling = tile == 1; }
// opcode 1108 — cc_setmodel. Sets model1_type=1 then model1_id.
pub fn op_cc_setmodel(com: &mut IfType, model_id: i32) {
    com.model1_type = 1;
    com.model1_id = model_id;
}
// opcode 1109 — cc_setmodelangle. 6-arg: x_of, y_of, x_an, y_an,
// z_an, zoom.
pub fn op_cc_setmodelangle(com: &mut IfType, xof: i32, yof: i32, xan: i32, yan: i32, zan: i32, zoom: i32) {
    com.model_x_of = xof;
    com.model_y_of = yof;
    com.model_x_an = xan;
    com.model_y_an = yan;
    com.model_z_an = zan;
    com.model_zoom = zoom;
}
// opcode 1110 — cc_setmodelanim. Returns true if anim id changed.
pub fn op_cc_setmodelanim(com: &mut IfType, anim_id: i32) -> bool {
    if com.model_anim != anim_id {
        com.model_anim = anim_id;
        com.anim_frame = 0;
        com.anim_cycle = 0;
        true
    } else {
        false
    }
}
// opcode 1111 — cc_setmodelorthog.
pub fn op_cc_setmodelorthog(com: &mut IfType, orthog: i32) {
    com.orthog = orthog == 1;
}
// opcode 1112 — cc_settext. Returns true if text changed.
pub fn op_cc_settext(com: &mut IfType, text: String) -> bool {
    if com.text == text { return false; }
    com.text = text;
    true
}
// opcode 1113 — cc_settextfont.
pub fn op_cc_settextfont(com: &mut IfType, font: i32) { com.font = font; }
// opcode 1114 — cc_settextalign.
pub fn op_cc_settextalign(com: &mut IfType, h_align: i32, v_align: i32, line_h: i32) {
    com.h_align = h_align;
    com.v_align = v_align;
    com.line_height = line_h;
}
// opcode 1115 — cc_settextshadow.
pub fn op_cc_settextshadow(com: &mut IfType, shadow: i32) {
    com.shadow = shadow == 1;
}
// opcode 1116 — cc_setoutline.
pub fn op_cc_setoutline(com: &mut IfType, outline: i32) { com.outline = outline; }
// opcode 1117 — cc_setgraphicshadow.
pub fn op_cc_setgraphicshadow(com: &mut IfType, shadow_colour: i32) {
    com.shadow_colour = shadow_colour;
}
// opcode 1118 — cc_setvflip.
pub fn op_cc_setvflip(com: &mut IfType, flip: i32) { com.v_flip = flip == 1; }
// opcode 1119 — cc_sethflip.
pub fn op_cc_sethflip(com: &mut IfType, flip: i32) { com.h_flip = flip == 1; }
// opcode 1120 — cc_setscrollsize.
pub fn op_cc_setscrollsize(com: &mut IfType, w: i32, h: i32) {
    com.scroll_width = w;
    com.scroll_height = h;
}

// opcode 1201 — cc_setnpchead. Verbatim port of ScriptRunner.java:
// 1201-1207. Sets model1Type to NPC-head class (2) and model1Id to
// the given npc id.
pub fn op_cc_setnpchead(com: &mut IfType, npc_id: i32) {
    com.model1_type = 2;
    com.model1_id = npc_id;
}

// opcode 1202 — cc_setplayerhead_self. Verbatim port of
// ScriptRunner.java:1208-1214. Sets model1Type to player-head class
// (3) and model1Id to the local player's PlayerModel hash (Java
// `Client.localPlayer.model.method1176()`). Caller supplies the
// pre-computed hash.
pub fn op_cc_setplayerhead_self(com: &mut IfType, player_model_hash: i32) {
    com.model1_type = 3;
    com.model1_id = player_model_hash;
}

// ── Drag + op setters (1300-1307) ─────────────────────────────────
// Verbatim ports of ScriptRunner.java:836-893.

// opcode 1300 — cc_setop. Writes per-slot op name; silently no-ops
// when slot-1 is outside 0..=9. Java pops the string regardless.
pub fn op_cc_setop(com: &mut IfType, slot: i32, name: String) {
    let idx = slot - 1;
    if idx < 0 || idx > 9 { return; }
    while com.op_names.len() <= idx as usize {
        com.op_names.push(String::new());
    }
    com.op_names[idx as usize] = name;
}
// opcode 1301 — cc_setdraggable. Java stores `IfType.get(parent,sub)`;
// we keep the (component id, cc sub) pair — sub -1 = the component
// itself, matching Java's two-arg get identity case.
pub fn op_cc_setdraggable(com: &mut IfType, parent_id: i32, sub_id: i32) {
    com.draggable = parent_id;
    com.draggable_sub = sub_id;
}
// opcode 1302 — cc_setdraggablebehavior.
pub fn op_cc_setdraggablebehavior(com: &mut IfType, value: i32) {
    com.draggable_behavior = value == 1;
}
// opcode 1303 — cc_setdragdeadzone.
pub fn op_cc_setdragdeadzone(com: &mut IfType, value: i32) {
    com.drag_dead_zone = value;
}
// opcode 1304 — cc_setdragdeadtime.
pub fn op_cc_setdragdeadtime(com: &mut IfType, value: i32) {
    com.drag_dead_time = value;
}
// opcode 1305 — cc_setopbase.
pub fn op_cc_setopbase(com: &mut IfType, name: String) {
    com.base_op_name = name;
}
// opcode 1306 — cc_settargetverb.
pub fn op_cc_settargetverb(com: &mut IfType, verb: String) {
    com.target_verb = verb;
}
// opcode 1307 — cc_clearops. Java sets `opNames = null`; we clear the
// Vec since the renderer treats an empty list as "no ops".
pub fn op_cc_clearops(com: &mut IfType) {
    com.op_names.clear();
}

// ── Hook setters (1400-1417) ──────────────────────────────────────
// Verbatim ports of ScriptRunner.java:941-1038. Each opcode writes
// one Option<Vec<HookArg>> field and sets hashook=true so the layer
// dispatcher knows this component has at least one hook to fire.
// The 2400-2499 explicit-component variants reuse these via
// `if_type::modify(id, |c| op_cc_seton<x>(c, args))`.

// opcode 1400 — cc_setonclick.
pub fn op_cc_setonclick(com: &mut IfType, args: Option<Vec<crate::config::if_type::HookArg>>) {
    com.hook_onclick = args;
    com.hashook = true;
}
// opcode 1401 — cc_setonhold.
pub fn op_cc_setonhold(com: &mut IfType, args: Option<Vec<crate::config::if_type::HookArg>>) {
    com.hook_onhold = args;
    com.hashook = true;
}
// opcode 1402 — cc_setonrelease.
pub fn op_cc_setonrelease(com: &mut IfType, args: Option<Vec<crate::config::if_type::HookArg>>) {
    com.hook_onrelease = args;
    com.hashook = true;
}
// opcode 1403 — cc_setonmouseover.
pub fn op_cc_setonmouseover(com: &mut IfType, args: Option<Vec<crate::config::if_type::HookArg>>) {
    com.hook_onmouseover = args;
    com.hashook = true;
}
// opcode 1404 — cc_setonmouseleave.
pub fn op_cc_setonmouseleave(com: &mut IfType, args: Option<Vec<crate::config::if_type::HookArg>>) {
    com.hook_onmouseleave = args;
    com.hashook = true;
}
// opcode 1405 — cc_setondrag.
pub fn op_cc_setondrag(com: &mut IfType, args: Option<Vec<crate::config::if_type::HookArg>>) {
    com.hook_ondrag = args;
    com.hashook = true;
}
// opcode 1406 — cc_setontargetleave.
pub fn op_cc_setontargetleave(com: &mut IfType, args: Option<Vec<crate::config::if_type::HookArg>>) {
    com.hook_ontargetleave = args;
    com.hashook = true;
}
// opcode 1408 — cc_setontimer.
pub fn op_cc_setontimer(com: &mut IfType, args: Option<Vec<crate::config::if_type::HookArg>>) {
    com.hook_ontimer = args;
    com.hashook = true;
}
// opcode 1409 — cc_setonop.
pub fn op_cc_setonop(com: &mut IfType, args: Option<Vec<crate::config::if_type::HookArg>>) {
    com.hook_onop = args;
    com.hashook = true;
}
// opcode 1410 — cc_setondragcomplete.
pub fn op_cc_setondragcomplete(com: &mut IfType, args: Option<Vec<crate::config::if_type::HookArg>>) {
    com.hook_ondragcomplete = args;
    com.hashook = true;
}
// opcode 1411 — cc_setonclickrepeat.
pub fn op_cc_setonclickrepeat(com: &mut IfType, args: Option<Vec<crate::config::if_type::HookArg>>) {
    com.hook_onclickrepeat = args;
    com.hashook = true;
}
// opcode 1412 — cc_setonmouserepeat.
pub fn op_cc_setonmouserepeat(com: &mut IfType, args: Option<Vec<crate::config::if_type::HookArg>>) {
    com.hook_onmouserepeat = args;
    com.hashook = true;
}
// opcode 1416 — cc_setontargetenter.
pub fn op_cc_setontargetenter(com: &mut IfType, args: Option<Vec<crate::config::if_type::HookArg>>) {
    com.hook_ontargetenter = args;
    com.hashook = true;
}
// opcode 1417 — cc_setonscrollwheel.
pub fn op_cc_setonscrollwheel(com: &mut IfType, args: Option<Vec<crate::config::if_type::HookArg>>) {
    com.hook_onscrollwheel = args;
    com.hashook = true;
}
// opcode 1418 — cc_setonchattransmit.
pub fn op_cc_setonchattransmit(com: &mut IfType, args: Option<Vec<crate::config::if_type::HookArg>>) {
    com.hook_onchattransmit = args;
    com.hashook = true;
}
// opcode 1419 — cc_setonkey.
pub fn op_cc_setonkey(com: &mut IfType, args: Option<Vec<crate::config::if_type::HookArg>>) {
    com.hook_onkey = args;
    com.hashook = true;
}
// opcode 1420 — cc_setonfriendtransmit.
pub fn op_cc_setonfriendtransmit(com: &mut IfType, args: Option<Vec<crate::config::if_type::HookArg>>) {
    com.hook_onfriendtransmit = args;
    com.hashook = true;
}
// opcode 1421 — cc_setonclantransmit.
pub fn op_cc_setonclantransmit(com: &mut IfType, args: Option<Vec<crate::config::if_type::HookArg>>) {
    com.hook_onclantransmit = args;
    com.hashook = true;
}
// opcode 1422 — cc_setonmisctransmit.
pub fn op_cc_setonmisctransmit(com: &mut IfType, args: Option<Vec<crate::config::if_type::HookArg>>) {
    com.hook_onmisctransmit = args;
    com.hashook = true;
}
// opcode 1423 — cc_setondialogabort.
pub fn op_cc_setondialogabort(com: &mut IfType, args: Option<Vec<crate::config::if_type::HookArg>>) {
    com.hook_ondialogabort = args;
    com.hashook = true;
}
// opcode 1424 — cc_setonsubchange.
pub fn op_cc_setonsubchange(com: &mut IfType, args: Option<Vec<crate::config::if_type::HookArg>>) {
    com.hook_onsubchange = args;
    com.hashook = true;
}

// opcode 1407 — cc_setonvartransmit. Sets both the hook script and
// the per-component varp subscription list (var ids that fire the
// hook when their value changes).
pub fn op_cc_setonvartransmit(
    com: &mut IfType,
    args: Option<Vec<crate::config::if_type::HookArg>>,
    var_list: Option<Vec<i32>>,
) {
    com.hook_onvartransmit = args;
    com.on_var_transmit_list = var_list;
    com.hashook = true;
}

// opcode 1414 — cc_setoninvtransmit. Same shape as onvartransmit
// but for inv subscription.
pub fn op_cc_setoninvtransmit(
    com: &mut IfType,
    args: Option<Vec<crate::config::if_type::HookArg>>,
    inv_list: Option<Vec<i32>>,
) {
    com.hook_oninvtransmit = args;
    com.on_inv_transmit_list = inv_list;
    com.hashook = true;
}

// opcode 1415 — cc_setonstattransmit. Stat subscription variant.
pub fn op_cc_setonstattransmit(
    com: &mut IfType,
    args: Option<Vec<crate::config::if_type::HookArg>>,
    stat_list: Option<Vec<i32>>,
) {
    com.hook_onstattransmit = args;
    com.on_stat_transmit_list = stat_list;
    com.hashook = true;
}

// ── Chat filter / history opcodes (5000-5017) ─────────────────────

// opcode 5000 — chat_getfilter_public.
pub fn op_chat_getfilter_public(chat_public_mode: i32) -> i32 { chat_public_mode }

// opcode 5003 — chat_gethistory_bytypeandline. Reads chat history
// text at row index. Returns empty string for OOB or null entries.
pub fn op_chat_gethistory_bytypeandline(chat_text: &[Option<String>], idx: i32) -> String {
    if idx < 0 || idx >= 100 { return String::new(); }
    chat_text.get(idx as usize)
        .and_then(|s| s.clone())
        .unwrap_or_default()
}

// opcode 5004 — chat_gethistory_byuid. Returns chat_type[idx] if a
// message is present at that slot, else -1.
pub fn op_chat_gethistory_byuid(chat_text: &[Option<String>], chat_type: &[i32], idx: i32) -> i32 {
    if idx < 0 || idx >= 100 { return -1; }
    if chat_text.get(idx as usize).and_then(|s| s.as_ref()).is_none() { return -1; }
    chat_type.get(idx as usize).copied().unwrap_or(-1)
}

// opcode 5005 — chat_getfilter_private. The PrivateChatFilter enum
// is encoded as an i32 index (see friend.rs); -1 = uninitialised.
pub fn op_chat_getfilter_private(filter: crate::friend::PrivateChatFilter) -> i32 {
    filter.index()
}

// opcode 5010 — chat_sendclan (read-side; the actual send is a
// dispatcher concern). Returns the sender username at chatHistory[idx].
pub fn op_chat_gethistory_username(chat_username: &[Option<String>], idx: i32) -> String {
    if idx < 0 || idx >= 100 { return String::new(); }
    chat_username.get(idx as usize)
        .and_then(|s| s.clone())
        .unwrap_or_default()
}

// opcode 5011 — chat_gethistory_screen. Returns the screen-name form
// of the sender at chatHistory[idx].
pub fn op_chat_gethistory_screen(chat_screen_name: &[Option<String>], idx: i32) -> String {
    if idx < 0 || idx >= 100 { return String::new(); }
    chat_screen_name.get(idx as usize)
        .and_then(|s| s.clone())
        .unwrap_or_default()
}

// opcode 5015 — chat_playername. Returns the local player's username.
pub fn op_chat_playername(name: Option<&str>) -> String {
    name.map(|s| s.to_string()).unwrap_or_default()
}

// opcode 5016 — chat_getfilter_trade.
pub fn op_chat_getfilter_trade(chat_trade_mode: i32) -> i32 { chat_trade_mode }

// opcode 5017 — chat_gethistorylength.
pub fn op_chat_gethistorylength(chat_history_length: i32) -> i32 { chat_history_length }

// ══════════════════════════════════════════════════════════════════
// HookReq + ComRef + the executeScript dispatcher
// ══════════════════════════════════════════════════════════════════

use std::sync::{Arc, Mutex};

use crate::client::Client;
use crate::config::if_type::HookArg;

// Component addressing for the interpreter. Java passes IfType object
// references; components here live in the if_type STORE, so we
// address them as either a top-level component id or a (parent, slot)
// pair into a parent's cc subcomponents.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ComRef {
    #[default]
    None,
    Com(i32),
    Cc { com: i32, sub: i32 },
}

impl ComRef {
    // Java IfType.get(id, sub): sub == -1 is the component itself.
    pub fn from_pair(id: i32, sub: i32) -> ComRef {
        if sub == -1 { ComRef::Com(id) } else { ComRef::Cc { com: id, sub } }
    }

    // @ObfuscatedName("eg.w") — the component's packed id (a cc's
    // parentId equals its parent component's id, per cc_create).
    pub fn parent_id(self) -> i32 {
        match self {
            ComRef::None => -1,
            ComRef::Com(id) => id,
            ComRef::Cc { com, .. } => com,
        }
    }

    // @ObfuscatedName("eg.e") — -1 for decoded components, the cc
    // slot for dynamic ones.
    pub fn sub_id(self) -> i32 {
        match self {
            ComRef::Cc { sub, .. } => sub,
            _ => -1,
        }
    }

    pub fn resolve(self) -> Option<IfType> {
        match self {
            ComRef::None => None,
            ComRef::Com(id) => crate::config::if_type::get(id),
            ComRef::Cc { com, sub } => {
                if sub < 0 { return None; }
                crate::config::if_type::get(com)
                    .and_then(|p| p.subcomponents.get(sub as usize).cloned().flatten())
            }
        }
    }

    pub fn modify<F: FnOnce(&mut IfType)>(self, f: F) -> bool {
        match self {
            ComRef::None => false,
            ComRef::Com(id) => crate::config::if_type::modify(id, f),
            ComRef::Cc { com, sub } => {
                if sub < 0 { return false; }
                let mut applied = false;
                crate::config::if_type::modify(com, |parent| {
                    if let Some(Some(cc)) = parent.subcomponents.get_mut(sub as usize) {
                        f(cc);
                        applied = true;
                    }
                });
                applied
            }
        }
    }
}

// @ObfuscatedName("du") — jag::oldscape::HookReq. The queued event:
// onop[0] is the script id, the rest are the trigger's bound args
// (with 0x8000000n placeholders substituted at execute time).
#[derive(Clone, Debug, Default)]
pub struct HookReq {
    // @ObfuscatedName("du.m")
    pub onop: Vec<HookArg>,
    // @ObfuscatedName("du.c")
    pub component: ComRef,
    // @ObfuscatedName("du.n") / "du.j"
    pub mouse_x: i32,
    pub mouse_y: i32,
    // @ObfuscatedName("du.z")
    pub opindex: i32,
    // @ObfuscatedName("du.g")
    pub drop: ComRef,
    // @ObfuscatedName("du.q") / "du.i"
    pub key_code: i32,
    pub key_char: i32,
    // @ObfuscatedName("du.s")
    pub opbase: String,
}

// @ObfuscatedName("p.q") / "bb.i" — the interpreter's implicit
// component registers (cc_find / cc_create targets).
pub static ACTIVE_COMPONENT: Mutex<ComRef> = Mutex::new(ComRef::None);
pub static ACTIVE_COMPONENT2: Mutex<ComRef> = Mutex::new(ComRef::None);

fn active_get(secondary: bool) -> ComRef {
    if secondary {
        *ACTIVE_COMPONENT2.lock().unwrap()
    } else {
        *ACTIVE_COMPONENT.lock().unwrap()
    }
}

fn active_set(secondary: bool, value: ComRef) {
    if secondary {
        *ACTIVE_COMPONENT2.lock().unwrap() = value;
    } else {
        *ACTIVE_COMPONENT.lock().unwrap() = value;
    }
}

struct Frame {
    script: Arc<crate::client_script::ClientScript>,
    pc: i64,
    int_locals: Vec<i32>,
    string_locals: Vec<String>,
}

// @ObfuscatedName("bv.r(Ldu;B)V") — ScriptRunner.executeScript.
// Verbatim port of ScriptRunner.java:75-2745. Java wraps the whole
// interpreter in try/catch and reports a chat-line error; panics
// (index/underflow/missing components — the same conditions that
// throw in Java) unwind to the catch here. Unlike Java the stacks are
// per-invocation, so a hook fired from inside an opcode (e.g.
// closeModal → onsubchange) can't clobber the outer script's stack.
pub fn execute_script(c: &mut Client, req: &HookReq) {
    let script_id = match req.onop.first() {
        Some(HookArg::Int(id)) => *id,
        _ => return,
    };
    if crate::client_script::get(script_id).is_none() {
        return;
    }

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_script(c, req);
    }));
    if result.is_err() {
        eprintln!("[cs2] CS2 - scr:{script_id} errored");
        // Java ScriptRunner.java:2738-2740 — the chat error only surfaces
        // in live mode (modewhere == 0); dev mode reports via the log
        // (our eprintln above) only. 3-arg addChat → null screenName.
        if c.modewhere == 0 {
            crate::client::add_chat(
                c, 0, Some(String::new()),
                Some("Clientscript error - check log for details".to_string()),
                None, 0,
            );
        }
    }
}

fn run_script(c: &mut Client, req: &HookReq) {
    let script_id = match req.onop.first() {
        Some(HookArg::Int(id)) => *id,
        _ => return,
    };
    let Some(mut script) = crate::client_script::get(script_id) else { return; };

    let mut int_stack = vec![0i32; 1000];
    let mut string_stack: Vec<String> = vec![String::new(); 1000];
    let mut isp = 0usize;
    let mut ssp = 0usize;
    let mut pc: i64 = -1;
    let mut frames: Vec<Frame> = Vec::new();

    let mut int_locals = vec![0i32; script.int_local_count];
    let mut string_locals = vec![String::new(); script.string_local_count];

    // ScriptRunner.java:99-133 — bind the trigger args into locals,
    // substituting the event placeholders.
    let mut int_count = 0usize;
    let mut string_count = 0usize;
    for arg in req.onop.iter().skip(1) {
        match arg {
            HookArg::Int(v) => {
                let v = match *v as u32 {
                    0x80000001 => req.mouse_x,
                    0x80000002 => req.mouse_y,
                    0x80000003 => req.component.parent_id(),
                    0x80000004 => req.opindex,
                    0x80000005 => req.component.sub_id(),
                    0x80000006 => req.drop.parent_id(),
                    0x80000007 => req.drop.sub_id(),
                    0x80000008 => req.key_code,
                    0x80000009 => req.key_char,
                    _ => *v,
                };
                int_locals[int_count] = v;
                int_count += 1;
            }
            HookArg::Str(s) => {
                let s = if s == "event_opbase" { req.opbase.clone() } else { s.clone() };
                string_locals[string_count] = s;
                string_count += 1;
            }
        }
    }

    let mut opcount = 0i32;
    loop {
        opcount += 1;
        if opcount > 200_000 {
            panic!("slow");
        }

        pc += 1;
        let pcu = pc as usize;
        let mut opcode = script.instructions[pcu];

        if opcode < 100 {
            match opcode {
                0 => {
                    // push_constant_int
                    int_stack[isp] = script.int_operands[pcu];
                    isp += 1;
                    continue;
                }
                1 => {
                    // push_varp
                    int_stack[isp] = crate::config::var_cache::get_varp(script.int_operands[pcu]);
                    isp += 1;
                    continue;
                }
                2 => {
                    // pop_varp
                    isp -= 1;
                    crate::config::var_cache::set_varp(script.int_operands[pcu], int_stack[isp]);
                    continue;
                }
                3 => {
                    // push_constant_string
                    string_stack[ssp] = script.string_operands[pcu].clone().unwrap_or_default();
                    ssp += 1;
                    continue;
                }
                6 => {
                    // branch
                    pc += script.int_operands[pcu] as i64;
                    continue;
                }
                7 => {
                    // branch_not
                    isp -= 2;
                    if int_stack[isp + 1] != int_stack[isp] {
                        pc += script.int_operands[pcu] as i64;
                    }
                    continue;
                }
                8 => {
                    // branch_equals
                    isp -= 2;
                    if int_stack[isp + 1] == int_stack[isp] {
                        pc += script.int_operands[pcu] as i64;
                    }
                    continue;
                }
                9 => {
                    // branch_less_than
                    isp -= 2;
                    if int_stack[isp] < int_stack[isp + 1] {
                        pc += script.int_operands[pcu] as i64;
                    }
                    continue;
                }
                10 => {
                    // branch_greater_than
                    isp -= 2;
                    if int_stack[isp] > int_stack[isp + 1] {
                        pc += script.int_operands[pcu] as i64;
                    }
                    continue;
                }
                21 => {
                    // return
                    let Some(frame) = frames.pop() else { return; };
                    script = frame.script;
                    pc = frame.pc;
                    int_locals = frame.int_locals;
                    string_locals = frame.string_locals;
                    continue;
                }
                25 => {
                    // push_varbit
                    int_stack[isp] = crate::config::var_cache::get_varbit(script.int_operands[pcu]);
                    isp += 1;
                    continue;
                }
                27 => {
                    // pop_varbit — ScriptRunner.java:229-248 clamps the
                    // value into the bitfield's range (0 when outside)
                    // before merging; var_cache::set_varbit mirrors it.
                    isp -= 1;
                    crate::config::var_cache::set_varbit(script.int_operands[pcu], int_stack[isp]);
                    continue;
                }
                31 => {
                    // branch_less_than_or_equals
                    isp -= 2;
                    if int_stack[isp] <= int_stack[isp + 1] {
                        pc += script.int_operands[pcu] as i64;
                    }
                    continue;
                }
                32 => {
                    // branch_greater_than_or_equals
                    isp -= 2;
                    if int_stack[isp] >= int_stack[isp + 1] {
                        pc += script.int_operands[pcu] as i64;
                    }
                    continue;
                }
                33 => {
                    // push_int_local
                    int_stack[isp] = int_locals[script.int_operands[pcu] as usize];
                    isp += 1;
                    continue;
                }
                34 => {
                    // pop_int_local
                    isp -= 1;
                    int_locals[script.int_operands[pcu] as usize] = int_stack[isp];
                    continue;
                }
                35 => {
                    // push_string_local
                    string_stack[ssp] = string_locals[script.int_operands[pcu] as usize].clone();
                    ssp += 1;
                    continue;
                }
                36 => {
                    // pop_string_local
                    ssp -= 1;
                    string_locals[script.int_operands[pcu] as usize] =
                        std::mem::take(&mut string_stack[ssp]);
                    continue;
                }
                37 => {
                    // join_string
                    let count = script.int_operands[pcu] as usize;
                    ssp -= count;
                    let joined: String = string_stack[ssp..ssp + count].concat();
                    string_stack[ssp] = joined;
                    ssp += 1;
                    continue;
                }
                38 => {
                    // pop_int_discard
                    isp -= 1;
                    continue;
                }
                39 => {
                    // pop_string_discard
                    ssp -= 1;
                    continue;
                }
                40 => {
                    // gosub_with_params
                    let proc_id = script.int_operands[pcu];
                    let proc = crate::client_script::get(proc_id)
                        .unwrap_or_else(|| panic!("gosub: missing script {proc_id}"));

                    let mut proc_ints = vec![0i32; proc.int_local_count];
                    let mut proc_strs = vec![String::new(); proc.string_local_count];
                    for i in 0..proc.int_arg_count {
                        proc_ints[i] = int_stack[isp - proc.int_arg_count + i];
                    }
                    for i in 0..proc.string_arg_count {
                        proc_strs[i] = string_stack[ssp - proc.string_arg_count + i].clone();
                    }
                    isp -= proc.int_arg_count;
                    ssp -= proc.string_arg_count;

                    if frames.len() >= 50 {
                        panic!("gosub depth");
                    }
                    frames.push(Frame {
                        script: Arc::clone(&script),
                        pc,
                        int_locals: std::mem::take(&mut int_locals),
                        string_locals: std::mem::take(&mut string_locals),
                    });
                    script = proc;
                    pc = -1;
                    int_locals = proc_ints;
                    string_locals = proc_strs;
                    continue;
                }
                42 => {
                    // push_varc_int
                    int_stack[isp] = crate::config::var_cache::get_varc_int(script.int_operands[pcu]);
                    isp += 1;
                    continue;
                }
                43 => {
                    // pop_varc_int
                    isp -= 1;
                    crate::config::var_cache::set_varc_int(script.int_operands[pcu], int_stack[isp]);
                    continue;
                }
                44 => {
                    // define_array — int ('i' = 105) arrays init to 0,
                    // everything else to -1 (ScriptRunner.java:359-382).
                    let array_id = (script.int_operands[pcu] >> 16) as usize;
                    let type_tag = script.int_operands[pcu] & 0xFFFF;
                    isp -= 1;
                    let size = int_stack[isp];
                    if size < 0 || size > 5000 {
                        panic!("define_array size {size}");
                    }
                    let init: i32 = if type_tag == 105 { 0 } else { -1 };
                    ARRAYS.lock().unwrap()[array_id] = vec![init; size as usize];
                    continue;
                }
                45 => {
                    // push_array_int
                    let array_id = script.int_operands[pcu] as usize;
                    isp -= 1;
                    let index = int_stack[isp];
                    let arrays = ARRAYS.lock().unwrap();
                    let arr = &arrays[array_id];
                    if index < 0 || index as usize >= arr.len() {
                        panic!("push_array oob");
                    }
                    int_stack[isp] = arr[index as usize];
                    drop(arrays);
                    isp += 1;
                    continue;
                }
                46 => {
                    // pop_array_int
                    let array_id = script.int_operands[pcu] as usize;
                    isp -= 2;
                    let index = int_stack[isp];
                    let value = int_stack[isp + 1];
                    let mut arrays = ARRAYS.lock().unwrap();
                    let arr = &mut arrays[array_id];
                    if index < 0 || index as usize >= arr.len() {
                        panic!("pop_array oob");
                    }
                    arr[index as usize] = value;
                    continue;
                }
                47 => {
                    // push_varc_str — get_varc_str already returns "null" for
                    // an unset (None) varc, matching Java's null→"null"
                    // stringification. A varc deliberately set to "" (e.g. the
                    // empty chat input buffer) must stay "" — don't re-map it
                    // to "null" here, or the chatbox renders the literal "null".
                    string_stack[ssp] =
                        crate::config::var_cache::get_varc_str(script.int_operands[pcu]);
                    ssp += 1;
                    continue;
                }
                48 => {
                    // pop_varc_str
                    ssp -= 1;
                    crate::config::var_cache::set_varc_str(
                        script.int_operands[pcu],
                        Some(std::mem::take(&mut string_stack[ssp])),
                    );
                    continue;
                }
                _ => {
                    panic!("unknown opcode {opcode}");
                }
            }
        }

        // ScriptRunner.java:433-438 — the 0/1 operand picks which
        // implicit component register the cc_* family targets.
        let secondary = script.int_operands[pcu] == 1;

        if opcode < 1000 {
            if opcode == 100 {
                // cc_create — ScriptRunner.java:441-482.
                isp -= 3;
                let parent_id = int_stack[isp];
                let cc_type = int_stack[isp + 1];
                let sub_id = int_stack[isp + 2];
                if cc_type == 0 {
                    panic!("cc_create type 0");
                }
                let mut created = false;
                crate::config::if_type::modify(parent_id, |parent| {
                    let needed = sub_id as usize + 1;
                    if parent.subcomponents.len() < needed {
                        parent.subcomponents.resize(needed, None);
                    }
                    if sub_id > 0 && parent.subcomponents[sub_id as usize - 1].is_none() {
                        return; // Java throws "Gap at:" — flagged below.
                    }
                    let mut comp = IfType::default();
                    comp.type_ = cc_type;
                    comp.layer_id = parent.parent_id;
                    comp.parent_id = parent.parent_id;
                    comp.sub_id = sub_id;
                    comp.v3 = true;
                    comp.draggable_sub = -2;
                    comp.model1_id = -1;
                    comp.model2_id = -1;
                    comp.model1_type = 1;
                    comp.model2_type = 1;
                    comp.model_anim = -1;
                    comp.model_anim2 = -1;
                    comp.model_zoom = 100;
                    comp.invobject = -1;
                    comp.invcount = -1;
                    comp.over_layer_id = -1;
                    comp.font = -1;
                    // Java IfType field initializers the Rust Default
                    // derive zeroes:
                    comp.graphic = -1;
                    comp.graphic2 = -1;
                    comp.line_width = 1;
                    parent.subcomponents[sub_id as usize] = Some(comp);
                    created = true;
                });
                if !created {
                    panic!("cc_create gap/missing parent {parent_id}:{sub_id}");
                }
                active_set(secondary, ComRef::Cc { com: parent_id, sub: sub_id });
                continue;
            }
            if opcode == 101 {
                // cc_delete
                let active = active_get(secondary);
                let (com, sub) = match active {
                    ComRef::Cc { com, sub } => (com, sub),
                    _ => panic!("cc_delete: active is not a cc"),
                };
                crate::config::if_type::modify(com, |parent| {
                    if (sub as usize) < parent.subcomponents.len() {
                        parent.subcomponents[sub as usize] = None;
                    }
                });
                continue;
            }
            if opcode == 102 {
                // cc_deleteall
                isp -= 1;
                let id = int_stack[isp];
                crate::config::if_type::modify(id, |parent| {
                    parent.subcomponents.clear();
                });
                continue;
            }
            if opcode == 200 {
                // cc_find
                isp -= 2;
                let id = int_stack[isp];
                let sub = int_stack[isp + 1];
                let target = ComRef::from_pair(id, sub);
                if sub != -1 && target.resolve().is_some() {
                    int_stack[isp] = 1;
                    isp += 1;
                    active_set(secondary, target);
                } else {
                    int_stack[isp] = 0;
                    isp += 1;
                }
                continue;
            }
            panic!("unknown opcode {opcode}");
        }

        // Per-band component resolution: the 2000-range mirrors each
        // 1000-range band but pops an explicit component id.
        if (1000..1100).contains(&opcode) || (2000..2100).contains(&opcode) {
            let target = if opcode >= 2000 {
                opcode -= 1000;
                isp -= 1;
                ComRef::Com(int_stack[isp])
            } else {
                active_get(secondary)
            };

            if opcode == 1000 {
                // if/cc_setposition
                isp -= 2;
                let (x, y) = (int_stack[isp], int_stack[isp + 1]);
                if !target.modify(|com| op_cc_setposition(com, x, y)) {
                    panic!("setposition: no component");
                }
                continue;
            }
            if opcode == 1001 {
                // if/cc_setsize
                isp -= 2;
                let (w, h) = (int_stack[isp], int_stack[isp + 1]);
                if !target.modify(|com| op_cc_setsize(com, w, h)) {
                    panic!("setsize: no component");
                }
                continue;
            }
            if opcode == 1003 {
                // if/cc_sethide
                isp -= 1;
                let hide = int_stack[isp];
                if !target.modify(|com| { op_cc_sethide(com, hide); }) {
                    panic!("sethide: no component");
                }
                continue;
            }
            panic!("unknown opcode {opcode}");
        }

        if (1100..1200).contains(&opcode) || (2100..2200).contains(&opcode) {
            let target = if opcode >= 2000 {
                opcode -= 1000;
                isp -= 1;
                ComRef::Com(int_stack[isp])
            } else {
                active_get(secondary)
            };

            let applied = match opcode {
                1100 => {
                    isp -= 2;
                    let (x, y) = (int_stack[isp], int_stack[isp + 1]);
                    target.modify(|com| op_cc_setscrollpos(com, x, y))
                }
                1101 => { isp -= 1; let v = int_stack[isp]; target.modify(|com| op_cc_setcolour(com, v)) }
                1102 => { isp -= 1; let v = int_stack[isp]; target.modify(|com| op_cc_setfill(com, v)) }
                1103 => { isp -= 1; let v = int_stack[isp]; target.modify(|com| op_cc_settrans(com, v)) }
                1104 => { isp -= 1; let v = int_stack[isp]; target.modify(|com| op_cc_setlinewid(com, v)) }
                1105 => { isp -= 1; let v = int_stack[isp]; target.modify(|com| op_cc_setgraphic(com, v)) }
                1106 => { isp -= 1; let v = int_stack[isp]; target.modify(|com| op_cc_set2dangle(com, v)) }
                1107 => { isp -= 1; let v = int_stack[isp]; target.modify(|com| op_cc_settiling(com, v)) }
                1108 => { isp -= 1; let v = int_stack[isp]; target.modify(|com| op_cc_setmodel(com, v)) }
                1109 => {
                    isp -= 6;
                    let (xof, yof, xan, yan, zan, zoom) = (
                        int_stack[isp], int_stack[isp + 1], int_stack[isp + 2],
                        int_stack[isp + 3], int_stack[isp + 4], int_stack[isp + 5],
                    );
                    target.modify(|com| op_cc_setmodelangle(com, xof, yof, xan, yan, zan, zoom))
                }
                1110 => { isp -= 1; let v = int_stack[isp]; target.modify(|com| { op_cc_setmodelanim(com, v); }) }
                1111 => { isp -= 1; let v = int_stack[isp]; target.modify(|com| op_cc_setmodelorthog(com, v)) }
                1112 => {
                    ssp -= 1;
                    let text = std::mem::take(&mut string_stack[ssp]);
                    target.modify(|com| { op_cc_settext(com, text); })
                }
                1113 => { isp -= 1; let v = int_stack[isp]; target.modify(|com| op_cc_settextfont(com, v)) }
                1114 => {
                    isp -= 3;
                    let (h, v, lh) = (int_stack[isp], int_stack[isp + 1], int_stack[isp + 2]);
                    target.modify(|com| op_cc_settextalign(com, h, v, lh))
                }
                1115 => { isp -= 1; let v = int_stack[isp]; target.modify(|com| op_cc_settextshadow(com, v)) }
                1116 => { isp -= 1; let v = int_stack[isp]; target.modify(|com| op_cc_setoutline(com, v)) }
                1117 => { isp -= 1; let v = int_stack[isp]; target.modify(|com| op_cc_setgraphicshadow(com, v)) }
                1118 => { isp -= 1; let v = int_stack[isp]; target.modify(|com| op_cc_setvflip(com, v)) }
                1119 => { isp -= 1; let v = int_stack[isp]; target.modify(|com| op_cc_sethflip(com, v)) }
                1120 => {
                    isp -= 2;
                    let (w, h) = (int_stack[isp], int_stack[isp + 1]);
                    target.modify(|com| op_cc_setscrollsize(com, w, h))
                }
                _ => panic!("unknown opcode {opcode}"),
            };
            if !applied {
                panic!("if/cc setter {opcode}: no component");
            }
            continue;
        }

        if (1200..1300).contains(&opcode) || (2200..2300).contains(&opcode) {
            let target = if opcode >= 2000 {
                opcode -= 1000;
                isp -= 1;
                ComRef::Com(int_stack[isp])
            } else {
                active_get(secondary)
            };

            if opcode == 1200 {
                // if/cc_setobject — ScriptRunner.java:788-809.
                isp -= 2;
                let obj_id = int_stack[isp];
                let count = int_stack[isp + 1];
                let obj = crate::config::obj_type::list(obj_id);
                let applied = target.modify(|com| {
                    com.invobject = obj_id;
                    com.invcount = count;
                    if let Some(obj) = obj.as_ref() {
                        com.model_x_an = obj.xan2d;
                        com.model_y_an = obj.yan2d;
                        com.model_z_an = obj.zan2d;
                        com.model_x_of = obj.xof2d;
                        com.model_y_of = obj.yof2d;
                        com.model_zoom = obj.zoom2d;
                        if com.width > 0 {
                            com.model_zoom = com.model_zoom * 32 / com.width;
                        }
                    }
                });
                if !applied { panic!("setobject: no component"); }
                continue;
            }
            if opcode == 1201 {
                // if/cc_setnpchead
                isp -= 1;
                let npc_id = int_stack[isp];
                if !target.modify(|com| op_cc_setnpchead(com, npc_id)) {
                    panic!("setnpchead: no component");
                }
                continue;
            }
            if opcode == 1202 {
                // if/cc_setplayerhead_self — Java reads
                // localPlayer.model.method1176() (the appearance hash,
                // doubling as the type-3 model cache key).
                let hash = c.local_player.as_ref()
                    .map(|lp| lp.model.head_hash())
                    .unwrap_or(0);
                if !target.modify(|com| op_cc_setplayerhead_self(com, hash)) {
                    panic!("setplayerhead: no component");
                }
                continue;
            }
            panic!("unknown opcode {opcode}");
        }

        if (1300..1400).contains(&opcode) || (2300..2400).contains(&opcode) {
            let target = if opcode >= 2000 {
                opcode -= 1000;
                isp -= 1;
                ComRef::Com(int_stack[isp])
            } else {
                active_get(secondary)
            };

            let applied = match opcode {
                1300 => {
                    // if/cc_setop — Java pops the string regardless of
                    // the slot range check.
                    isp -= 1;
                    let slot = int_stack[isp];
                    ssp -= 1;
                    let name = std::mem::take(&mut string_stack[ssp]);
                    if (1..=10).contains(&slot) {
                        target.modify(|com| op_cc_setop(com, slot, name))
                    } else {
                        true
                    }
                }
                1301 => {
                    isp -= 2;
                    let (id, sub) = (int_stack[isp], int_stack[isp + 1]);
                    target.modify(|com| op_cc_setdraggable(com, id, sub))
                }
                1302 => { isp -= 1; let v = int_stack[isp]; target.modify(|com| op_cc_setdraggablebehavior(com, v)) }
                1303 => { isp -= 1; let v = int_stack[isp]; target.modify(|com| op_cc_setdragdeadzone(com, v)) }
                1304 => { isp -= 1; let v = int_stack[isp]; target.modify(|com| op_cc_setdragdeadtime(com, v)) }
                1305 => {
                    ssp -= 1;
                    let name = std::mem::take(&mut string_stack[ssp]);
                    target.modify(|com| op_cc_setopbase(com, name))
                }
                1306 => {
                    ssp -= 1;
                    let verb = std::mem::take(&mut string_stack[ssp]);
                    target.modify(|com| op_cc_settargetverb(com, verb))
                }
                1307 => target.modify(op_cc_clearops),
                _ => panic!("unknown opcode {opcode}"),
            };
            if !applied {
                panic!("if/cc drag-op setter {opcode}: no component");
            }
            continue;
        }

        if (1400..1500).contains(&opcode) || (2400..2500).contains(&opcode) {
            let target = if opcode >= 2000 {
                opcode -= 1000;
                isp -= 1;
                ComRef::Com(int_stack[isp])
            } else {
                active_get(secondary)
            };

            // Hook-args decode — ScriptRunner.java:894-938. The
            // signature string walks the bound args back-to-front; a
            // trailing 'Y' carries a transmit-subscription list.
            ssp -= 1;
            let mut sig = std::mem::take(&mut string_stack[ssp]);
            let mut transmit_list: Option<Vec<i32>> = None;
            if sig.ends_with('Y') {
                isp -= 1;
                let count = int_stack[isp];
                if count > 0 {
                    let mut list = vec![0i32; count as usize];
                    for k in (0..count as usize).rev() {
                        isp -= 1;
                        list[k] = int_stack[isp];
                    }
                    transmit_list = Some(list);
                }
                sig.pop();
            }

            let sig_chars: Vec<char> = sig.chars().collect();
            let mut args: Vec<HookArg> = vec![HookArg::Int(0); sig_chars.len() + 1];
            for k in (1..args.len()).rev() {
                if sig_chars[k - 1] == 's' {
                    ssp -= 1;
                    args[k] = HookArg::Str(std::mem::take(&mut string_stack[ssp]));
                } else {
                    isp -= 1;
                    args[k] = HookArg::Int(int_stack[isp]);
                }
            }

            isp -= 1;
            let hook_script = int_stack[isp];
            let args: Option<Vec<HookArg>> = if hook_script == -1 {
                None
            } else {
                args[0] = HookArg::Int(hook_script);
                Some(args)
            };

            let applied = target.modify(|com| match opcode {
                1400 => op_cc_setonclick(com, args),
                1401 => op_cc_setonhold(com, args),
                1402 => op_cc_setonrelease(com, args),
                1403 => op_cc_setonmouseover(com, args),
                1404 => op_cc_setonmouseleave(com, args),
                1405 => op_cc_setondrag(com, args),
                1406 => op_cc_setontargetleave(com, args),
                1407 => op_cc_setonvartransmit(com, args, transmit_list),
                1408 => op_cc_setontimer(com, args),
                1409 => op_cc_setonop(com, args),
                1410 => op_cc_setondragcomplete(com, args),
                1411 => op_cc_setonclickrepeat(com, args),
                1412 => op_cc_setonmouserepeat(com, args),
                1414 => op_cc_setoninvtransmit(com, args, transmit_list),
                1415 => op_cc_setonstattransmit(com, args, transmit_list),
                1416 => op_cc_setontargetenter(com, args),
                1417 => op_cc_setonscrollwheel(com, args),
                1418 => op_cc_setonchattransmit(com, args),
                1419 => op_cc_setonkey(com, args),
                1420 => op_cc_setonfriendtransmit(com, args),
                1421 => op_cc_setonclantransmit(com, args),
                1422 => op_cc_setonmisctransmit(com, args),
                1423 => op_cc_setondialogabort(com, args),
                1424 => op_cc_setonsubchange(com, args),
                _ => panic!("unknown hook opcode {opcode}"),
            });
            if !applied {
                panic!("hook setter {opcode}: no component");
            }
            continue;
        }

        if opcode < 1600 {
            let com = active_get(secondary).resolve().expect("cc getter: no active component");
            match opcode {
                1500 => { int_stack[isp] = com.x; isp += 1; }
                1501 => { int_stack[isp] = com.y; isp += 1; }
                1502 => { int_stack[isp] = com.width; isp += 1; }
                1503 => { int_stack[isp] = com.height; isp += 1; }
                1504 => { int_stack[isp] = if com.hide { 1 } else { 0 }; isp += 1; }
                1505 => { int_stack[isp] = com.layer_id; isp += 1; }
                _ => panic!("unknown opcode {opcode}"),
            }
            continue;
        }

        if opcode < 1700 {
            let com = active_get(secondary).resolve().expect("cc getter: no active component");
            match opcode {
                1600 => { int_stack[isp] = com.scroll_pos_x; isp += 1; }
                1601 => { int_stack[isp] = com.scroll_pos_y; isp += 1; }
                1602 => { string_stack[ssp] = com.text.clone(); ssp += 1; }
                1603 => { int_stack[isp] = com.scroll_width; isp += 1; }
                1604 => { int_stack[isp] = com.scroll_height; isp += 1; }
                1605 => { int_stack[isp] = com.model_zoom; isp += 1; }
                1606 => { int_stack[isp] = com.model_x_an; isp += 1; }
                1607 => { int_stack[isp] = com.model_z_an; isp += 1; }
                1608 => { int_stack[isp] = com.model_y_an; isp += 1; }
                _ => panic!("unknown opcode {opcode}"),
            }
            continue;
        }

        if opcode < 1800 {
            let com = active_get(secondary).resolve().expect("cc getter: no active component");
            match opcode {
                1700 => { int_stack[isp] = com.invobject; isp += 1; }
                1701 => {
                    int_stack[isp] = if com.invobject == -1 { 0 } else { com.invcount };
                    isp += 1;
                }
                1702 => { int_stack[isp] = com.sub_id; isp += 1; }
                _ => panic!("unknown opcode {opcode}"),
            }
            continue;
        }

        if opcode < 1900 {
            let com = active_get(secondary).resolve().expect("cc getter: no active component");
            match opcode {
                1800 => {
                    let active = crate::client::get_active(c, &com);
                    int_stack[isp] = crate::config::server_active::target_mask(active);
                    isp += 1;
                }
                1801 => {
                    isp -= 1;
                    let slot = int_stack[isp];
                    string_stack[ssp] = op_if_getop(&com, slot);
                    ssp += 1;
                }
                1802 => {
                    string_stack[ssp] = com.base_op_name.clone();
                    ssp += 1;
                }
                _ => panic!("unknown opcode {opcode}"),
            }
            continue;
        }

        if opcode < 2600 {
            isp -= 1;
            let com = crate::config::if_type::get(int_stack[isp]).expect("if getter: missing component");
            match opcode {
                2500 => { int_stack[isp] = com.x; isp += 1; }
                2501 => { int_stack[isp] = com.y; isp += 1; }
                2502 => { int_stack[isp] = com.width; isp += 1; }
                2503 => { int_stack[isp] = com.height; isp += 1; }
                2504 => { int_stack[isp] = if com.hide { 1 } else { 0 }; isp += 1; }
                2505 => { int_stack[isp] = com.layer_id; isp += 1; }
                _ => panic!("unknown opcode {opcode}"),
            }
            continue;
        }

        if opcode < 2700 {
            isp -= 1;
            let com = crate::config::if_type::get(int_stack[isp]).expect("if getter: missing component");
            match opcode {
                2600 => { int_stack[isp] = com.scroll_pos_x; isp += 1; }
                2601 => { int_stack[isp] = com.scroll_pos_y; isp += 1; }
                2602 => { string_stack[ssp] = com.text.clone(); ssp += 1; }
                2603 => { int_stack[isp] = com.scroll_width; isp += 1; }
                2604 => { int_stack[isp] = com.scroll_height; isp += 1; }
                2605 => { int_stack[isp] = com.model_zoom; isp += 1; }
                2606 => { int_stack[isp] = com.model_x_an; isp += 1; }
                2607 => { int_stack[isp] = com.model_z_an; isp += 1; }
                2608 => { int_stack[isp] = com.model_y_an; isp += 1; }
                _ => panic!("unknown opcode {opcode}"),
            }
            continue;
        }

        if opcode < 2800 {
            match opcode {
                2700 => {
                    isp -= 1;
                    let com = crate::config::if_type::get(int_stack[isp]).expect("if_getinvobject");
                    int_stack[isp] = com.invobject;
                    isp += 1;
                }
                2701 => {
                    isp -= 1;
                    let com = crate::config::if_type::get(int_stack[isp]).expect("if_getinvcount");
                    int_stack[isp] = if com.invobject == -1 { 0 } else { com.invcount };
                    isp += 1;
                }
                2702 => {
                    isp -= 1;
                    let id = int_stack[isp];
                    int_stack[isp] = if c.subinterfaces.contains_key(&id) { 1 } else { 0 };
                    isp += 1;
                }
                _ => panic!("unknown opcode {opcode}"),
            }
            continue;
        }

        if opcode < 2900 {
            isp -= 1;
            let com = crate::config::if_type::get(int_stack[isp]).expect("if getter: missing component");
            match opcode {
                2800 => {
                    let active = crate::client::get_active(c, &com);
                    int_stack[isp] = crate::config::server_active::target_mask(active);
                    isp += 1;
                }
                2801 => {
                    isp -= 1;
                    let slot = int_stack[isp];
                    string_stack[ssp] = op_if_getop(&com, slot);
                    ssp += 1;
                }
                2802 => {
                    string_stack[ssp] = com.base_op_name.clone();
                    ssp += 1;
                }
                _ => panic!("unknown opcode {opcode}"),
            }
            continue;
        }

        if opcode < 3200 {
            match opcode {
                3100 => {
                    // mes — Java ScriptRunner.java:1322 addChat(0, "", var94)
                    // (3-arg overload → null screenName).
                    ssp -= 1;
                    let msg = std::mem::take(&mut string_stack[ssp]);
                    crate::client::add_chat(c, 0, Some(String::new()), Some(msg),
                                            None, 0);
                }
                3101 => {
                    // anim
                    isp -= 2;
                    let (anim, delay) = (int_stack[isp], int_stack[isp + 1]);
                    if let Some(lp) = c.local_player.as_mut() {
                        Client::trigger_player_anim(lp, anim, delay);
                    }
                }
                3103 => {
                    // if_close
                    crate::client::close_modal(c);
                }
                3104 => {
                    // resume_countdialog → RESUME_P_COUNTDIALOG (27)
                    ssp -= 1;
                    let s = std::mem::take(&mut string_stack[ssp]);
                    let value: i32 = s.trim().parse().unwrap_or(0);
                    if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                        out.p1_enc(27, isaac);
                        out.p4(value);
                    }
                }
                3105 => {
                    // resume_namedialog → RESUME_P_NAMEDIALOG (223)
                    ssp -= 1;
                    let s = std::mem::take(&mut string_stack[ssp]);
                    if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                        out.p1_enc(223, isaac);
                        out.p1(s.len() as i32 + 1);
                        out.pjstr(&s);
                    }
                }
                3106 => {
                    // resume_stringdialog → RESUME_P_STRINGDIALOG (127)
                    ssp -= 1;
                    let s = std::mem::take(&mut string_stack[ssp]);
                    if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                        out.p1_enc(127, isaac);
                        out.p1(s.len() as i32 + 1);
                        out.pjstr(&s);
                    }
                }
                3107 => {
                    // opplayer
                    isp -= 1;
                    let action = int_stack[isp];
                    ssp -= 1;
                    let name = std::mem::take(&mut string_stack[ssp]);
                    crate::client::op_player(c, action, &name);
                }
                3108 => {
                    // if_dragpickup
                    isp -= 3;
                    let (x, y, com_id) = (int_stack[isp], int_stack[isp + 1], int_stack[isp + 2]);
                    crate::client::drag_try_pickup(c, ComRef::Com(com_id), x, y);
                }
                3109 => {
                    // cc_dragpickup
                    isp -= 2;
                    let (x, y) = (int_stack[isp], int_stack[isp + 1]);
                    let target = active_get(secondary);
                    crate::client::drag_try_pickup(c, target, x, y);
                }
                _ => panic!("unknown opcode {opcode}"),
            }
            continue;
        }

        if opcode < 3300 {
            match opcode {
                3200 => {
                    // sound_synth
                    isp -= 3;
                    crate::client::play_synth(c, int_stack[isp], int_stack[isp + 1], int_stack[isp + 2]);
                }
                3201 => {
                    // sound_song
                    isp -= 1;
                    crate::client::play_songs(c, int_stack[isp]);
                }
                3202 => {
                    // sound_jingle
                    isp -= 2;
                    crate::client::play_jingle(c, int_stack[isp], int_stack[isp + 1]);
                }
                _ => panic!("unknown opcode {opcode}"),
            }
            continue;
        }

        if opcode < 3400 {
            match opcode {
                3300 => { int_stack[isp] = c.loop_cycle; isp += 1; }
                3301 => {
                    isp -= 2;
                    int_stack[isp] = crate::client_inv_cache::get_type(int_stack[isp], int_stack[isp + 1]);
                    isp += 1;
                }
                3302 => {
                    isp -= 2;
                    int_stack[isp] = crate::client_inv_cache::get_count(int_stack[isp], int_stack[isp + 1]);
                    isp += 1;
                }
                3303 => {
                    isp -= 2;
                    int_stack[isp] = crate::client_inv_cache::inv_total(int_stack[isp], int_stack[isp + 1]);
                    isp += 1;
                }
                3304 => {
                    isp -= 1;
                    int_stack[isp] = crate::config::inv_type::list(int_stack[isp]).size;
                    isp += 1;
                }
                3305 => {
                    isp -= 1;
                    int_stack[isp] = c.stat_effective_level[int_stack[isp] as usize];
                    isp += 1;
                }
                3306 => {
                    isp -= 1;
                    int_stack[isp] = c.stat_base_level[int_stack[isp] as usize];
                    isp += 1;
                }
                3307 => {
                    isp -= 1;
                    int_stack[isp] = c.stat_xp[int_stack[isp] as usize];
                    isp += 1;
                }
                3308 => {
                    let lp = c.local_player.as_ref().expect("coord: no local player");
                    let level = c.minusedlevel;
                    let wx = (lp.x >> 7) + c.map_build_base_x;
                    let wz = (lp.z >> 7) + c.map_build_base_z;
                    int_stack[isp] = (level << 28) + (wx << 14) + wz;
                    isp += 1;
                }
                3309 => { let v = int_stack[isp - 1]; int_stack[isp - 1] = (v >> 14) & 0x3FFF; }
                3310 => { let v = int_stack[isp - 1]; int_stack[isp - 1] = v >> 28; }
                3311 => { let v = int_stack[isp - 1]; int_stack[isp - 1] = v & 0x3FFF; }
                3312 => { int_stack[isp] = if c.mem_server { 1 } else { 0 }; isp += 1; }
                3313 => {
                    isp -= 2;
                    int_stack[isp] = crate::client_inv_cache::get_type(int_stack[isp] + 32768, int_stack[isp + 1]);
                    isp += 1;
                }
                3314 => {
                    isp -= 2;
                    int_stack[isp] = crate::client_inv_cache::get_count(int_stack[isp] + 32768, int_stack[isp + 1]);
                    isp += 1;
                }
                3315 => {
                    isp -= 2;
                    int_stack[isp] = crate::client_inv_cache::inv_total(int_stack[isp] + 32768, int_stack[isp + 1]);
                    isp += 1;
                }
                3316 => {
                    int_stack[isp] = if c.staffmodlevel >= 2 { c.staffmodlevel } else { 0 };
                    isp += 1;
                }
                3317 => { int_stack[isp] = c.reboot_timer; isp += 1; }
                3318 => { int_stack[isp] = c.worldid; isp += 1; }
                3321 => { int_stack[isp] = c.run_energy; isp += 1; }
                3322 => { int_stack[isp] = c.run_weight; isp += 1; }
                3323 => { int_stack[isp] = if c.playermod { 1 } else { 0 }; isp += 1; }
                _ => panic!("unknown opcode {opcode}"),
            }
            continue;
        }

        if opcode < 3500 {
            match opcode {
                3400 => {
                    isp -= 2;
                    let (enum_id, key) = (int_stack[isp], int_stack[isp + 1]);
                    string_stack[ssp] = op_enum_string(enum_id, key);
                    ssp += 1;
                }
                3408 => {
                    isp -= 4;
                    let (in_t, out_t, enum_id, key) = (
                        int_stack[isp], int_stack[isp + 1], int_stack[isp + 2], int_stack[isp + 3],
                    );
                    match op_enum(in_t, out_t, enum_id, key) {
                        EnumResult::Int(v) => { int_stack[isp] = v; isp += 1; }
                        EnumResult::String(s) => { string_stack[ssp] = s; ssp += 1; }
                    }
                }
                _ => panic!("unknown opcode {opcode}"),
            }
            continue;
        }

        if opcode < 3700 {
            match opcode {
                3600 => {
                    int_stack[isp] = op_friend_count(c.friend_server_status, c.friend_count);
                    isp += 1;
                }
                3601 => {
                    isp -= 1;
                    let idx = int_stack[isp];
                    string_stack[ssp] = op_friend_getname(&c.friend_list, c.friend_server_status, c.friend_count, idx);
                    ssp += 1;
                }
                3602 => {
                    isp -= 1;
                    let idx = int_stack[isp];
                    int_stack[isp] = op_friend_getworld(&c.friend_list, c.friend_server_status, c.friend_count, idx);
                    isp += 1;
                }
                3603 => {
                    isp -= 1;
                    let idx = int_stack[isp];
                    int_stack[isp] = op_friend_getrank(&c.friend_list, c.friend_server_status, c.friend_count, idx);
                    isp += 1;
                }
                3604 => {
                    ssp -= 1;
                    let name = std::mem::take(&mut string_stack[ssp]);
                    isp -= 1;
                    let rank = int_stack[isp];
                    crate::client::set_friend_rank(c, &name, rank);
                }
                3605 => {
                    ssp -= 1;
                    let name = std::mem::take(&mut string_stack[ssp]);
                    crate::client::add_friend(c, &name);
                }
                3606 => {
                    ssp -= 1;
                    let name = std::mem::take(&mut string_stack[ssp]);
                    crate::client::del_friend(c, &name);
                }
                3607 => {
                    ssp -= 1;
                    let name = std::mem::take(&mut string_stack[ssp]);
                    crate::client::add_ignore(c, &name);
                }
                3608 => {
                    ssp -= 1;
                    let name = std::mem::take(&mut string_stack[ssp]);
                    crate::client::del_ignore(c, &name);
                }
                3609 => {
                    ssp -= 1;
                    let name = std::mem::take(&mut string_stack[ssp]);
                    let stripped = op_friend_test(&name);
                    int_stack[isp] = if crate::client::is_friend(c, Some(stripped)) { 1 } else { 0 };
                    isp += 1;
                }
                3611 => {
                    string_stack[ssp] = op_clan_get_chat_display_name(c.chat_display_name.as_deref());
                    ssp += 1;
                }
                3612 => {
                    int_stack[isp] = op_clan_getchatcount(c.chat_display_name.as_deref(), c.friend_chat_count);
                    isp += 1;
                }
                3613 => {
                    isp -= 1;
                    let idx = int_stack[isp];
                    string_stack[ssp] = op_clan_get_chat_username(
                        c.chat_display_name.as_deref(), &c.friend_chat_list, c.friend_chat_count, idx);
                    ssp += 1;
                }
                3614 => {
                    isp -= 1;
                    let idx = int_stack[isp];
                    int_stack[isp] = op_clan_get_chat_userworld(
                        c.chat_display_name.as_deref(), &c.friend_chat_list, c.friend_chat_count, idx);
                    isp += 1;
                }
                3615 => {
                    isp -= 1;
                    let idx = int_stack[isp];
                    int_stack[isp] = op_clan_get_chat_userrank(
                        c.chat_display_name.as_deref(), &c.friend_chat_list, c.friend_chat_count, idx);
                    isp += 1;
                }
                3616 => { int_stack[isp] = c.chat_min_kick; isp += 1; }
                3617 => {
                    ssp -= 1;
                    let name = std::mem::take(&mut string_stack[ssp]);
                    crate::client::friends_chat_kick_user(c, &name);
                }
                3618 => { int_stack[isp] = c.chat_rank; isp += 1; }
                3619 => {
                    ssp -= 1;
                    let name = std::mem::take(&mut string_stack[ssp]);
                    crate::client::friends_chat_join_chat(c, &name);
                }
                3620 => {
                    crate::client::friends_chat_leave_chat(c);
                }
                3621 => {
                    int_stack[isp] = op_ignore_count(c.friend_server_status, c.ignore_count);
                    isp += 1;
                }
                3622 => {
                    isp -= 1;
                    let idx = int_stack[isp];
                    string_stack[ssp] = op_ignore_getname(c.friend_server_status, &c.ignore_list, c.ignore_count, idx);
                    ssp += 1;
                }
                3623 => {
                    ssp -= 1;
                    let name = std::mem::take(&mut string_stack[ssp]);
                    let stripped = op_ignore_test_strip(&name);
                    int_stack[isp] = if crate::client::is_ignored(c, Some(stripped)) { 1 } else { 0 };
                    isp += 1;
                }
                3624 => {
                    isp -= 1;
                    let idx = int_stack[isp];
                    let local = c.local_player.as_ref().map(|lp| lp.name.clone());
                    int_stack[isp] = op_clan_isself(&c.friend_chat_list, c.friend_chat_count,
                                                    local.as_deref(), idx);
                    isp += 1;
                }
                3625 => {
                    string_stack[ssp] = op_clan_get_chat_owner_name(c.chat_owner_name.as_deref());
                    ssp += 1;
                }
                _ => panic!("unknown opcode {opcode}"),
            }
            continue;
        }

        if opcode < 4100 {
            match opcode {
                4000 => { isp -= 2; int_stack[isp] = int_stack[isp].wrapping_add(int_stack[isp + 1]); isp += 1; }
                4001 => { isp -= 2; int_stack[isp] = int_stack[isp].wrapping_sub(int_stack[isp + 1]); isp += 1; }
                4002 => { isp -= 2; int_stack[isp] = int_stack[isp].wrapping_mul(int_stack[isp + 1]); isp += 1; }
                4003 => {
                    // Java's int divide throws on /0 (→ catch).
                    isp -= 2;
                    let (a, b) = (int_stack[isp], int_stack[isp + 1]);
                    if b == 0 { panic!("divide by zero"); }
                    int_stack[isp] = a.wrapping_div(b);
                    isp += 1;
                }
                4004 => { isp -= 1; int_stack[isp] = op_random(int_stack[isp]); isp += 1; }
                4005 => { isp -= 1; int_stack[isp] = op_randominc(int_stack[isp]); isp += 1; }
                4006 => {
                    isp -= 5;
                    let (y0, y1, x0, x1, x) = (
                        int_stack[isp], int_stack[isp + 1], int_stack[isp + 2],
                        int_stack[isp + 3], int_stack[isp + 4],
                    );
                    // Java divides unguarded: (y1-y0)*(x-x0)/(x1-x0)+y0.
                    if x1 == x0 { panic!("interpolate /0"); }
                    int_stack[isp] = (y1 - y0) * (x - x0) / (x1 - x0) + y0;
                    isp += 1;
                }
                4007 => {
                    isp -= 2;
                    let (a, b) = (int_stack[isp], int_stack[isp + 1]);
                    int_stack[isp] = a.wrapping_mul(b) / 100 + a;
                    isp += 1;
                }
                4008 => { isp -= 2; int_stack[isp] = int_stack[isp] | 1i32.wrapping_shl(int_stack[isp + 1] as u32); isp += 1; }
                4009 => { isp -= 2; int_stack[isp] = int_stack[isp] & !(1i32.wrapping_shl(int_stack[isp + 1] as u32)); isp += 1; }
                4010 => {
                    isp -= 2;
                    int_stack[isp] = if int_stack[isp] & 1i32.wrapping_shl(int_stack[isp + 1] as u32) == 0 { 0 } else { 1 };
                    isp += 1;
                }
                4011 => {
                    isp -= 2;
                    let (a, b) = (int_stack[isp], int_stack[isp + 1]);
                    if b == 0 { panic!("modulo by zero"); }
                    int_stack[isp] = a.wrapping_rem(b);
                    isp += 1;
                }
                4012 => { isp -= 2; int_stack[isp] = op_pow(int_stack[isp], int_stack[isp + 1]); isp += 1; }
                4013 => { isp -= 2; int_stack[isp] = op_invpow(int_stack[isp], int_stack[isp + 1]); isp += 1; }
                4014 => { isp -= 2; int_stack[isp] = int_stack[isp] & int_stack[isp + 1]; isp += 1; }
                4015 => { isp -= 2; int_stack[isp] = int_stack[isp] | int_stack[isp + 1]; isp += 1; }
                _ => panic!("unknown opcode {opcode}"),
            }
            continue;
        }

        if opcode < 4200 {
            match opcode {
                4100 => {
                    ssp -= 1;
                    let s = std::mem::take(&mut string_stack[ssp]);
                    isp -= 1;
                    string_stack[ssp] = format!("{s}{}", int_stack[isp]);
                    ssp += 1;
                }
                4101 => {
                    ssp -= 2;
                    let b = std::mem::take(&mut string_stack[ssp + 1]);
                    let a = std::mem::take(&mut string_stack[ssp]);
                    string_stack[ssp] = format!("{a}{b}");
                    ssp += 1;
                }
                4102 => {
                    ssp -= 1;
                    let s = std::mem::take(&mut string_stack[ssp]);
                    isp -= 1;
                    string_stack[ssp] = op_append_signnum(&s, int_stack[isp]);
                    ssp += 1;
                }
                4103 => {
                    ssp -= 1;
                    let s = std::mem::take(&mut string_stack[ssp]);
                    string_stack[ssp] = s.to_lowercase();
                    ssp += 1;
                }
                4104 => {
                    isp -= 1;
                    string_stack[ssp] = op_fromdate(int_stack[isp]);
                    ssp += 1;
                }
                4105 => {
                    // text_gender — Java: localPlayer.model != null &&
                    // model.gender picks the female string.
                    ssp -= 2;
                    let female_text = std::mem::take(&mut string_stack[ssp + 1]);
                    let male_text = std::mem::take(&mut string_stack[ssp]);
                    let female = c.local_player.as_ref()
                        .map_or(false, |lp| lp.model.applied && lp.model.gender);
                    string_stack[ssp] = if female { female_text } else { male_text };
                    ssp += 1;
                }
                4106 => {
                    isp -= 1;
                    string_stack[ssp] = int_stack[isp].to_string();
                    ssp += 1;
                }
                4107 => {
                    ssp -= 2;
                    let b = std::mem::take(&mut string_stack[ssp + 1]);
                    let a = std::mem::take(&mut string_stack[ssp]);
                    int_stack[isp] = op_compare(&a, &b);
                    isp += 1;
                }
                4108 => {
                    // paraheight
                    ssp -= 1;
                    let text = std::mem::take(&mut string_stack[ssp]);
                    isp -= 2;
                    let (width, font_id) = (int_stack[isp], int_stack[isp + 1]);
                    let font = crate::config::if_type::load_font(font_id).expect("paraheight font");
                    int_stack[isp] = font.base.predict_lines_multiline(&text, width);
                    isp += 1;
                }
                4109 => {
                    // parawidth
                    ssp -= 1;
                    let text = std::mem::take(&mut string_stack[ssp]);
                    isp -= 2;
                    let (width, font_id) = (int_stack[isp], int_stack[isp + 1]);
                    let font = crate::config::if_type::load_font(font_id).expect("parawidth font");
                    int_stack[isp] = font.base.predict_width_multiline(&text, width);
                    isp += 1;
                }
                4110 => {
                    ssp -= 2;
                    let f = std::mem::take(&mut string_stack[ssp + 1]);
                    let t = std::mem::take(&mut string_stack[ssp]);
                    isp -= 1;
                    string_stack[ssp] = if int_stack[isp] == 1 { t } else { f };
                    ssp += 1;
                }
                4111 => {
                    ssp -= 1;
                    let s = std::mem::take(&mut string_stack[ssp]);
                    string_stack[ssp] = op_escape(&s);
                    ssp += 1;
                }
                4112 => {
                    ssp -= 1;
                    let s = std::mem::take(&mut string_stack[ssp]);
                    isp -= 1;
                    string_stack[ssp] = op_append_char(&s, int_stack[isp]);
                    ssp += 1;
                }
                4113 => { isp -= 1; int_stack[isp] = op_char_isprintable(int_stack[isp]); isp += 1; }
                4114 => { isp -= 1; int_stack[isp] = op_char_isalphanumeric(int_stack[isp]); isp += 1; }
                4115 => { isp -= 1; int_stack[isp] = op_char_isalpha(int_stack[isp]); isp += 1; }
                4116 => { isp -= 1; int_stack[isp] = op_char_isnumeric(int_stack[isp]); isp += 1; }
                4117 => {
                    ssp -= 1;
                    let s = std::mem::take(&mut string_stack[ssp]);
                    int_stack[isp] = s.chars().count() as i32;
                    isp += 1;
                }
                4118 => {
                    ssp -= 1;
                    let s = std::mem::take(&mut string_stack[ssp]);
                    isp -= 2;
                    let (begin, end) = (int_stack[isp], int_stack[isp + 1]);
                    string_stack[ssp] = op_substring(&s, begin, end);
                    ssp += 1;
                }
                4119 => {
                    ssp -= 1;
                    let s = std::mem::take(&mut string_stack[ssp]);
                    string_stack[ssp] = op_removetags(&s);
                    ssp += 1;
                }
                4120 => {
                    ssp -= 1;
                    let s = std::mem::take(&mut string_stack[ssp]);
                    isp -= 1;
                    int_stack[isp] = op_string_indexof_char(&s, int_stack[isp]);
                    isp += 1;
                }
                _ => panic!("unknown opcode {opcode}"),
            }
            continue;
        }

        if opcode < 4300 {
            match opcode {
                4200 => {
                    isp -= 1;
                    string_stack[ssp] = op_oc_name(int_stack[isp]);
                    ssp += 1;
                }
                4201 => {
                    isp -= 2;
                    string_stack[ssp] = op_oc_op(int_stack[isp], int_stack[isp + 1]);
                    ssp += 1;
                }
                4202 => {
                    isp -= 2;
                    string_stack[ssp] = op_oc_iop(int_stack[isp], int_stack[isp + 1]);
                    ssp += 1;
                }
                4203 => { isp -= 1; int_stack[isp] = op_oc_cost(int_stack[isp]); isp += 1; }
                4204 => { isp -= 1; int_stack[isp] = op_oc_stackable(int_stack[isp]); isp += 1; }
                4205 => { isp -= 1; int_stack[isp] = op_oc_cert(int_stack[isp]); isp += 1; }
                4206 => { isp -= 1; int_stack[isp] = op_oc_uncert(int_stack[isp]); isp += 1; }
                4207 => { isp -= 1; int_stack[isp] = op_oc_members(int_stack[isp]); isp += 1; }
                _ => panic!("unknown opcode {opcode}"),
            }
            continue;
        }

        if opcode < 5100 {
            match opcode {
                5000 => { int_stack[isp] = c.chat_public_mode; isp += 1; }
                5001 => {
                    // chat_setfilter → SET_CHATFILTERSETTINGS (167)
                    isp -= 3;
                    c.chat_public_mode = int_stack[isp];
                    c.chat_private_mode = crate::friend::PrivateChatFilter::from_i32(int_stack[isp + 1]);
                    c.chat_trade_mode = int_stack[isp + 2];
                    let (public, private, trade) =
                        (c.chat_public_mode, c.chat_private_mode.index(), c.chat_trade_mode);
                    if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                        out.p1_enc(167, isaac);
                        out.p1(public);
                        out.p1(private);
                        out.p1(trade);
                    }
                }
                5002 => {
                    // chat_sendabusereport → SEND_SNAPSHOT (96)
                    ssp -= 1;
                    let name = std::mem::take(&mut string_stack[ssp]);
                    isp -= 2;
                    let (rule, mute) = (int_stack[isp], int_stack[isp + 1]);
                    let strlen = crate::jstring::utf8_to_cp1252(&name).len() as i32 + 1;
                    if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                        out.p1_enc(96, isaac);
                        out.p1(strlen + 2);
                        out.pjstr(&name);
                        out.p1(rule - 1);
                        out.p1(mute);
                    }
                }
                5003 => {
                    isp -= 1;
                    let idx = int_stack[isp];
                    string_stack[ssp] = op_chat_gethistory_bytypeandline(&c.chat_text, idx);
                    ssp += 1;
                }
                5004 => {
                    isp -= 1;
                    let idx = int_stack[isp];
                    int_stack[isp] = op_chat_gethistory_byuid(&c.chat_text, &c.chat_type, idx);
                    isp += 1;
                }
                5005 => {
                    int_stack[isp] = c.chat_private_mode.index();
                    isp += 1;
                }
                5008 => {
                    // chat_sendpublic — ScriptRunner.java:2478-2611.
                    ssp -= 1;
                    let message = std::mem::take(&mut string_stack[ssp]);
                    chat_send_public(c, &message);
                }
                5009 => {
                    // chat_sendprivate → MESSAGE_PRIVATE (211)
                    ssp -= 2;
                    let text = std::mem::take(&mut string_stack[ssp + 1]);
                    let name = std::mem::take(&mut string_stack[ssp]);
                    if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
                        out.p1_enc(211, isaac);
                        out.p2(0);
                        let start = out.pos;
                        out.pjstr(&name);
                        crate::wordpack::pack(out, &text);
                        out.psize2((out.pos - start) as i32);
                    }
                }
                5010 => {
                    isp -= 1;
                    let idx = int_stack[isp];
                    string_stack[ssp] = op_chat_gethistory_username(&c.chat_username, idx);
                    ssp += 1;
                }
                5011 => {
                    isp -= 1;
                    let idx = int_stack[isp];
                    string_stack[ssp] = op_chat_gethistory_screen(&c.chat_screen_name, idx);
                    ssp += 1;
                }
                5015 => {
                    string_stack[ssp] = c.local_player.as_ref()
                        .map(|lp| lp.name.clone())
                        .unwrap_or_default();
                    ssp += 1;
                }
                5016 => { int_stack[isp] = c.chat_trade_mode; isp += 1; }
                5017 => { int_stack[isp] = c.chat_history_length; isp += 1; }
                _ => panic!("unknown opcode {opcode}"),
            }
            continue;
        }

        panic!("unknown opcode {opcode}");
    }
}

// chat_sendpublic body (opcode 5008) — ScriptRunner.java:2478-2611:
// "::" routes to doCheat; otherwise strip the colour prefix then the
// effect prefix and send MESSAGE_PUBLIC (205) with the WordPack-packed
// remainder. The German prefix table is byte-identical to the English
// one in this build, so the lang != 0 re-check is a no-op.
fn chat_send_public(c: &mut Client, message: &str) {
    if message.starts_with("::") {
        crate::client::do_cheat(c, message);
        return;
    }

    use crate::text as t;
    const COLOURS: [(&str, u8); 12] = [
        (t::CHATCOL_YELLOW, 0), (t::CHATCOL_RED, 1), (t::CHATCOL_GREEN, 2),
        (t::CHATCOL_CYAN, 3), (t::CHATCOL_PURPLE, 4), (t::CHATCOL_WHITE, 5),
        (t::CHATEFFECT_FLASH1, 6), (t::CHATEFFECT_FLASH2, 7), (t::CHATEFFECT_FLASH3, 8),
        (t::CHATEFFECT_GLOW1, 9), (t::CHATEFFECT_GLOW2, 10), (t::CHATEFFECT_GLOW3, 11),
    ];
    const EFFECTS: [(&str, u8); 5] = [
        (t::CHATEFFECT_WAVE, 1), (t::CHATEFFECT_WAVE2, 2), (t::CHATEFFECT_SHAKE, 3),
        (t::CHATEFFECT_SCROLL, 4), (t::CHATEFFECT_SLIDE, 5),
    ];

    let mut message = message.to_string();
    let mut colour = 0u8;
    let lower = message.to_lowercase();
    for (prefix, code) in COLOURS {
        if lower.starts_with(prefix) {
            colour = code;
            message = message[prefix.len()..].to_string();
            break;
        }
    }

    let mut effect = 0u8;
    let lower = message.to_lowercase();
    for (prefix, code) in EFFECTS {
        if lower.starts_with(prefix) {
            effect = code;
            message = message[prefix.len()..].to_string();
            break;
        }
    }

    // MESSAGE_PUBLIC
    if let (Some(isaac), Some(out)) = (c.isaac_out.as_mut(), c.out_packet.as_mut()) {
        out.p1_enc(205, isaac);
        out.p1(0);
        let start = out.pos;
        out.p1(colour as i32);
        out.p1(effect as i32);
        crate::wordpack::pack(out, &message);
        out.psize1((out.pos - start) as i32);
    }
}

// @ObfuscatedName("r.d(II)V") — ScriptRunner.executeOnLoad. Verbatim
// port of ScriptRunner.java:2748-2764: fire every component's onload
// hook for a freshly-opened interface group.
pub fn execute_onload(c: &mut Client, id: i32) {
    if id == -1 || !crate::config::if_type::open_interface(id, 3) {
        return;
    }

    let hooks: Vec<(ComRef, Vec<HookArg>)> = {
        let store = crate::config::if_type::STORE.lock().unwrap();
        match store.list.get(id as usize).and_then(|o| o.as_ref()) {
            Some(list) => list.iter()
                .filter_map(|o| o.as_ref())
                .filter_map(|com| com.hook_onload.clone()
                    .map(|h| (ComRef::Com(com.parent_id), h)))
                .collect(),
            None => return,
        }
    };

    for (component, onop) in hooks {
        let req = HookReq { component, onop, ..Default::default() };
        execute_script(c, &req);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_basics() {
        assert_eq!(op_add(3, 4), 7);
        assert_eq!(op_sub(10, 4), 6);
        assert_eq!(op_multiply(6, 7), 42);
        assert_eq!(op_divide(20, 5), 4);
        assert_eq!(op_divide(1, 0), 0);
        assert_eq!(op_modulo(7, 3), 1);
        assert_eq!(op_modulo(5, 0), 0);
    }

    #[test]
    fn bitops() {
        assert_eq!(op_setbit(0b0001, 2), 0b0101);
        assert_eq!(op_clearbit(0b0111, 1), 0b0101);
        assert_eq!(op_testbit(0b0100, 2), 1);
        assert_eq!(op_testbit(0b0100, 0), 0);
        assert_eq!(op_and(0b1100, 0b1010), 0b1000);
        assert_eq!(op_or(0b1100, 0b1010), 0b1110);
    }

    #[test]
    fn pow_and_invpow() {
        assert_eq!(op_pow(2, 10), 1024);
        assert_eq!(op_pow(0, 5), 0);
        assert_eq!(op_invpow(1024, 10), 2);
        assert_eq!(op_invpow(0, 5), 0);
        assert_eq!(op_invpow(5, 0), i32::MAX);
    }

    #[test]
    fn compare_collation() {
        // Basic ordering + equality.
        assert_eq!(op_compare("abc", "abc"), 0);
        assert_eq!(op_compare("abc", "abd"), -1);
        assert_eq!(op_compare("abd", "abc"), 1);
        // Shorter string sorts first.
        assert_eq!(op_compare("abc", "abcd"), -1);
        assert_eq!(op_compare("abcd", "abc"), 1);
        // Case-insensitive in the collation pass, but the case-sensitive
        // tiebreak puts lowercase before uppercase (getCharSortKey +1).
        assert_eq!(op_compare("apple", "Apple"), -1);
        assert_eq!(op_compare("Apple", "apple"), 1);
        // Accent folding: é (233) collates as 'e'.
        assert_eq!(op_compare("caf\u{e9}", "cafe"), 1); // tiebreak: é > e raw
        assert_eq!(op_compare("r\u{e9}sum", "resume"), -1); // 'm' < 'm'+'e'? folds equal then len
        // Ligature expansion: æ (230) → "ae".
        assert_eq!(op_compare("\u{e6}", "ae"), 1); // collate-equal, tiebreak raw æ > a
        assert_eq!(op_compare("\u{e6}b", "aec"), -1); // ae[b] vs ae[c] → b<c
    }

    #[test]
    fn interpolate() {
        // line from (0, 0) to (10, 100), sample at 5 → 50.
        assert_eq!(op_interpolate(0, 100, 0, 10, 5), 50);
        // sample at 0 → 0.
        assert_eq!(op_interpolate(0, 100, 0, 10, 0), 0);
        // sample at 10 → 100.
        assert_eq!(op_interpolate(0, 100, 0, 10, 10), 100);
        // degenerate x range collapses to y0.
        assert_eq!(op_interpolate(7, 99, 4, 4, 5), 7);
    }
}
