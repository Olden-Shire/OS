//! ClientScript (CS2) bytecode decoder. 1:1 port of `jagex3.client.ClientScript.get`
//! (`src/main/java/jagex3/client/ClientScript.java:39-85`).
//!
//! ## Binary layout
//!
//! ```text
//! [name: optional null-terminated string]   ← read via fastgstr (None if first byte == 0)
//! [instructions: variable length]
//! [trailer (12 bytes, big-endian)]:
//!   u32 instruction_count
//!   u16 int_local_count
//!   u16 string_local_count
//!   u16 int_arg_count
//!   u16 string_arg_count
//! ```
//!
//! ## Instruction encoding
//!
//! Each instruction is `u16 opcode` followed by one of:
//! - `op == 3`  → null-terminated string (`gjstr`), stored in `string_operands[i]`
//! - `op >= 100 || op == 21 || op == 38 || op == 39` → `u8` widened to `i32` in `int_operands[i]`
//! - otherwise → big-endian `i32` in `int_operands[i]`
//!
//! Operand selection matches Java's predicate exactly so a Rust decode of any cache
//! script produces the same `instructions` / `int_operands` / `string_operands` arrays
//! as the Java client's in-memory representation.

use io::packet::Packet;

/// Special opcode for `push_constant_string` (the only string-operand instruction).
pub const OP_PUSH_CONST_STRING: u16 = 3;

/// Opcodes < 100 that nonetheless use a 1-byte operand (`return`, `pop_int_discard`,
/// `pop_string_discard`). Matches Java's predicate `op == 21 || op == 38 || op == 39`.
const SMALL_OPERAND_BELOW_100: &[u16] = &[21, 38, 39];

#[derive(Debug, Clone)]
pub struct ClientScript {
    /// Optional script name read via `fastgstr`. Almost always `None` for official
    /// caches — the field was used in the 468 client for debugging and stripped from
    /// the OSRS release builds.
    pub name: Option<String>,

    /// Parallel arrays — for instruction `i`, the opcode is `instructions[i]`, and one
    /// of `int_operands[i]` / `string_operands[i]` carries the operand (per the
    /// encoding rules above). The "other" array's entry at that index is left at its
    /// default (`0` / empty `String`) and shouldn't be read.
    pub instructions: Vec<u16>,
    pub int_operands: Vec<i32>,
    pub string_operands: Vec<String>,

    pub int_local_count: u16,
    pub string_local_count: u16,
    pub int_arg_count: u16,
    pub string_arg_count: u16,
}

impl ClientScript {
    /// Returns `None` if the buffer is shorter than the 12-byte trailer.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 12 {
            return None;
        }
        let mut buf = Packet::from_vec(bytes.to_vec());

        // Trailer first — gives us instruction count and local/arg counts.
        buf.pos = bytes.len() - 12;
        let instruction_count = buf.g4() as usize;
        let int_local_count = buf.g2() as u16;
        let string_local_count = buf.g2() as u16;
        let int_arg_count = buf.g2() as u16;
        let string_arg_count = buf.g2() as u16;

        // Optional name at offset 0.
        buf.pos = 0;
        let name = buf.fastgstr();

        let mut instructions = Vec::with_capacity(instruction_count);
        let mut int_operands = vec![0i32; instruction_count];
        let mut string_operands = vec![String::new(); instruction_count];

        let trailer_pos = bytes.len() - 12;
        let mut i = 0usize;
        while buf.pos < trailer_pos && i < instruction_count {
            let op = buf.g2() as u16;
            if op == OP_PUSH_CONST_STRING {
                string_operands[i] = buf.gjstr();
            } else if op >= 100 || SMALL_OPERAND_BELOW_100.contains(&op) {
                int_operands[i] = buf.g1() as i32;
            } else {
                int_operands[i] = buf.g4();
            }
            instructions.push(op);
            i += 1;
        }

        // Truncate operand arrays to actually-decoded length in case the stream ran
        // short (better to surface a partial script than to panic on indexing later).
        int_operands.truncate(instructions.len());
        string_operands.truncate(instructions.len());

        Some(Self {
            name,
            instructions,
            int_operands,
            string_operands,
            int_local_count,
            string_local_count,
            int_arg_count,
            string_arg_count,
        })
    }

    /// Encode back to the on-disk CS2 byte layout. Exact inverse of [`Self::decode`]:
    /// for any vanilla cache script `encode(decode(bytes)) == bytes`.
    ///
    /// The per-instruction operand width predicate is identical to decode's, so the
    /// produced stream re-decodes to the same parallel arrays. The trailer's
    /// instruction count is `instructions.len()` (decode truncates its arrays to the
    /// number actually read, so this matches for well-formed scripts).
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Packet::new(64);

        // Optional name: `None` is the single 0 byte that `fastgstr` reads as absent;
        // `Some` is a CP1252 null-terminated string whose first byte is non-zero.
        match &self.name {
            None => buf.p1(0),
            Some(s) => buf.pjstr(s),
        }

        for (i, &op) in self.instructions.iter().enumerate() {
            buf.p2(i32::from(op));
            if op == OP_PUSH_CONST_STRING {
                buf.pjstr(&self.string_operands[i]);
            } else if op >= 100 || SMALL_OPERAND_BELOW_100.contains(&op) {
                buf.p1(self.int_operands[i] & 0xFF);
            } else {
                buf.p4(self.int_operands[i]);
            }
        }

        buf.p4(self.instructions.len() as i32);
        buf.p2(i32::from(self.int_local_count));
        buf.p2(i32::from(self.string_local_count));
        buf.p2(i32::from(self.int_arg_count));
        buf.p2(i32::from(self.string_arg_count));

        let mut data = buf.data;
        data.truncate(buf.pos);
        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip helper: build a minimal CS2 buffer from in-memory parts, decode,
    /// compare. Verifies trailer layout and per-opcode operand widths.
    fn encode_trailer(buf: &mut Vec<u8>, instr_count: u32, intl: u16, strl: u16, inta: u16, stra: u16) {
        buf.extend_from_slice(&instr_count.to_be_bytes());
        buf.extend_from_slice(&intl.to_be_bytes());
        buf.extend_from_slice(&strl.to_be_bytes());
        buf.extend_from_slice(&inta.to_be_bytes());
        buf.extend_from_slice(&stra.to_be_bytes());
    }

    #[test]
    fn decodes_trailer_and_single_int_op() {
        // No name (first byte 0), one 4-byte-operand instruction (op=0 push_const_int 42).
        let mut buf = vec![0u8]; // fastgstr → None
        buf.extend_from_slice(&0u16.to_be_bytes()); // opcode 0
        buf.extend_from_slice(&42i32.to_be_bytes()); // operand
        encode_trailer(&mut buf, 1, 0, 0, 0, 0);

        let s = ClientScript::decode(&buf).unwrap();
        assert_eq!(s.name, None);
        assert_eq!(s.instructions, vec![0]);
        assert_eq!(s.int_operands, vec![42]);
        assert_eq!(s.string_operands, vec![String::new()]);
    }

    #[test]
    fn string_operand_uses_gjstr() {
        // op=3 push_const_string "hi"
        let mut buf = vec![0u8]; // no name
        buf.extend_from_slice(&3u16.to_be_bytes());
        buf.extend_from_slice(b"hi\0"); // gjstr reads until 0
        encode_trailer(&mut buf, 1, 0, 0, 0, 0);

        let s = ClientScript::decode(&buf).unwrap();
        assert_eq!(s.instructions, vec![3]);
        assert_eq!(s.string_operands[0], "hi");
        // int_operands[0] stays at its default 0 (string opcode doesn't write it).
        assert_eq!(s.int_operands[0], 0);
    }

    #[test]
    fn small_operand_opcodes_read_one_byte() {
        // op=100 (cc_create — 1-byte operand) with operand 7.
        let mut buf = vec![0u8];
        buf.extend_from_slice(&100u16.to_be_bytes());
        buf.push(7);
        encode_trailer(&mut buf, 1, 0, 0, 0, 0);

        let s = ClientScript::decode(&buf).unwrap();
        assert_eq!(s.int_operands[0], 7);
    }

    #[test]
    fn op21_uses_one_byte_operand_even_though_below_100() {
        // op=21 (return) is in the < 100 special-case list.
        let mut buf = vec![0u8];
        buf.extend_from_slice(&21u16.to_be_bytes());
        buf.push(0);
        encode_trailer(&mut buf, 1, 0, 0, 0, 0);

        let s = ClientScript::decode(&buf).unwrap();
        assert_eq!(s.instructions, vec![21]);
        assert_eq!(s.int_operands[0], 0);
    }

    #[test]
    fn trailer_counts_round_trip() {
        let mut buf = vec![0u8]; // no name, no instructions
        encode_trailer(&mut buf, 0, 3, 5, 7, 11);

        let s = ClientScript::decode(&buf).unwrap();
        assert_eq!(s.int_local_count, 3);
        assert_eq!(s.string_local_count, 5);
        assert_eq!(s.int_arg_count, 7);
        assert_eq!(s.string_arg_count, 11);
    }

    #[test]
    fn name_decodes_when_present() {
        let mut buf = b"myscript\0".to_vec(); // fastgstr reads non-null then null-terminated
        encode_trailer(&mut buf, 0, 0, 0, 0, 0);

        let s = ClientScript::decode(&buf).unwrap();
        assert_eq!(s.name.as_deref(), Some("myscript"));
    }

    #[test]
    fn too_short_returns_none() {
        assert!(ClientScript::decode(&[1, 2, 3]).is_none());
    }

    /// `encode` must be a byte-exact inverse of `decode` for every operand width.
    #[test]
    fn encode_is_byte_exact_inverse_of_decode() {
        // A script exercising all four operand encodings plus the trailer counts:
        // op 0 (4-byte int), op 3 (string), op 100 (1-byte), op 21 (1-byte < 100).
        let mut buf = vec![0u8]; // no name
        buf.extend_from_slice(&0u16.to_be_bytes());
        buf.extend_from_slice(&123_456i32.to_be_bytes());
        buf.extend_from_slice(&3u16.to_be_bytes());
        buf.extend_from_slice(b"hello\0");
        buf.extend_from_slice(&100u16.to_be_bytes());
        buf.push(9);
        buf.extend_from_slice(&21u16.to_be_bytes());
        buf.push(0);
        encode_trailer(&mut buf, 4, 2, 1, 3, 1);

        let s = ClientScript::decode(&buf).unwrap();
        assert_eq!(s.encode(), buf);
    }

    #[test]
    fn encode_round_trips_named_script() {
        let mut buf = b"myscript\0".to_vec();
        buf.extend_from_slice(&0u16.to_be_bytes()); // op 0
        buf.extend_from_slice(&7i32.to_be_bytes());
        encode_trailer(&mut buf, 1, 0, 0, 0, 0);

        let s = ClientScript::decode(&buf).unwrap();
        assert_eq!(s.name.as_deref(), Some("myscript"));
        assert_eq!(s.encode(), buf);
    }
}
