// @ObfuscatedName("r") — jag::oldscape::StringConstants.
//
// Tag generators for chat / interface markup, plus a handful of
// punctuation strings the deobfuscator left as named constants.
// Verbatim port of StringConstants.java.

#![allow(dead_code)]

// @ObfuscatedName("r.r") — "true" literal.
pub const TRUE_S: &str = "true";
// @ObfuscatedName("r.d")
pub const COMMA: &str = ",";
// @ObfuscatedName("r.l")
pub const PIPE: &str = "|";
// @ObfuscatedName("r.m")
pub const OPEN_BRACKET: &str = " (";
// @ObfuscatedName("r.c")
pub const CLOSE_BRACKET: &str = ")";
// @ObfuscatedName("r.n")
pub const TAG_ARROW: &str = "->";
// @ObfuscatedName("r.j")
pub const TAG_BREAK: &str = "<br>";
// @ObfuscatedName("r.z")
pub const TAG_COLOURCLOSE: &str = "</col>";

// @ObfuscatedName("j.r(IS)Ljava/lang/String;") — TAG_IMG(int).
// Verbatim port of StringConstants.java:39-41.
pub fn tag_img(id: i32) -> String {
    format!("<img={id}>")
}

// @ObfuscatedName("i.d(II)Ljava/lang/String;") — TAG_COLOUR(int).
// Verbatim port of StringConstants.java:45-47. Java emits the colour
// in lowercase hex *without* the `#` prefix or zero padding —
// `Integer.toHexString` semantics. `format!("{:x}")` matches.
pub fn tag_colour(rgb: i32) -> String {
    format!("<col={:x}>", rgb as u32)
}
