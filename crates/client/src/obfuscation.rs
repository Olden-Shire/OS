// @ObfuscatedName("cy")
// jag::oldscape::obfuscation::Protocol — the per-opcode size table the
// rev1 client uses to drive its server packet loop. Indexed by ptype.
// -1 = u8-length-prefixed variable size, -2 = u16-length-prefixed,
// otherwise fixed size in bytes.

#![allow(dead_code)]

// @ObfuscatedName("cy.gh")
//
// Verbatim copy of Java's `Protocol.SERVERPROT_SIZES` (256 entries,
// indexed by ptype). This MUST match the Java table byte-for-byte —
// it drives the server packet loop's size lookup, so any divergence
// desyncs the stream. Our own server (crates/server) speaks this same
// rev1 protocol, so no per-opcode overrides are needed.
pub const SERVERPROT_SIZES: [i32; 256] = [
     0,  2,  0,  0,  0,  0,  4,  2,  0,  0,  0,  0,  0,  0,  0,  0,
     0,  4,  0,  0,  6, -2,  0,  0,  0, -2, 10,  0,  0, -2,  0,  0,
    15,  0,  0,  0,  0,  0,  0, -2,  0,  1,  0,  0,  0,  0,  0,  0,
    12,  0,  6,  0,  0,  5,  0,  0,  0, -1,  0,  0,  0,  0,  0,  0,
     0,  0,  6,  2,  0,  0,  1,  0,  0, -2,  0,  0,  0,  0,  0,  0,
    -2,  0,  0,  0,  5,  8, -2,  4,  3,  2,  0,  0, -2,  0,  0,  0,
     0,  2,  0,  0, -1,  0, 10,  0,  0,  0,  7,  0,  0,  0,  0,  0,
     0, -2,  0,  0,  0,  4,  0,  0, -2,  0,  0,  0,  0,  0,  0,  0,
     0,  0,  0, -2,  0,  0,  0,  0,  0,  2,  0,  0, -1,  6, -2,  0,
     0,  0,  0,  2,  0,  0,  0,  0,  0,  0,  4,  0,  0,  0,  0,  0,
     6,  0,  0,  0, -1,  0,  0, -2, -2,  6,  0,  4,  2,  5,  0,  0,
     6,  0,  0,  0,  6,  0,  0,  0,  7,  0,  0,  0,  0,  0,  1,  0,
     0,  0,  0,  0,  0, -2,  0,  0,  0,  0,  0,  0,  0,  5,  0,  3,
     6,  0,  0,  2,  0,  0, 28,  7,  0,  8,  0,  0,  0,  0, -2,  0,
     0,  6,  0,  0,  0,  5,  0,  0,  0,  0,  6,  0,  0,  0,  0,  0,
     0,  4,  0,  0,  0, 14,  3,  0,  0,  0,  0,  6,  0,  0,  0,  0,
];
