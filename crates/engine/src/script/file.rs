//! Compiled RuneScript container — mirrors the Engine-TS reference
//! ScriptFile.ts (the same binary format as clientscript2 plus the
//! compiler's name/source/line-number trailer header).

use std::collections::HashMap;

use io::packet::Packet;

use crate::script::opcode;

#[derive(Debug, Default)]
pub struct ScriptInfo {
    pub script_name: String,
    pub source_file_path: String,
    /// i64 since pack v27: component subjects pack (interface<<16)|child,
    /// which shifted <<10 into the key exceeds 32 bits.
    pub lookup_key: i64,
    pub parameter_types: Vec<u8>,
    pub pcs: Vec<i32>,
    pub lines: Vec<i32>,
}

#[derive(Debug)]
pub struct ScriptFile {
    pub id: i32,
    pub info: ScriptInfo,
    pub int_local_count: usize,
    pub string_local_count: usize,
    pub int_arg_count: usize,
    pub string_arg_count: usize,
    pub switch_tables: Vec<HashMap<i32, i32>>,
    pub opcodes: Vec<u16>,
    pub int_operands: Vec<i32>,
    pub string_operands: Vec<Option<String>>,
}

impl ScriptFile {
    pub fn name(&self) -> &str {
        &self.info.script_name
    }

    pub fn file_name(&self) -> &str {
        self.info.source_file_path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&self.info.source_file_path)
    }

    pub fn line_number(&self, pc: i32) -> i32 {
        for i in 0..self.info.pcs.len() {
            if self.info.pcs[i] > pc {
                if i == 0 {
                    return 0;
                }
                return self.info.lines[i - 1];
            }
        }
        self.info.lines.last().copied().unwrap_or(0)
    }

    /// Decode one script blob — ScriptFile.ts `decode`. `version` is the
    /// pack's compiler version: the lookup key widened from i32 to i64 in
    /// v27 (component subjects pack (interface<<16)|child, which exceeds 32
    /// bits once shifted into the key), so older packs read a 4-byte key.
    pub fn decode(id: i32, data: Vec<u8>, version: i32) -> Result<ScriptFile, String> {
        let length = data.len();
        if length < 16 {
            return Err("invalid script file (minimum length)".to_string());
        }

        let mut stream = Packet::from_vec(data);

        stream.pos = length - 2;
        let trailer_len = stream.g2() as usize;
        let trailer_pos = length as i64 - trailer_len as i64 - 12 - 2;
        if trailer_pos < 0 || trailer_pos >= length as i64 {
            return Err("invalid script file (bad trailer pos)".to_string());
        }
        let trailer_pos = trailer_pos as usize;

        stream.pos = trailer_pos;
        let _instructions = stream.g4();
        let int_local_count = stream.g2() as usize;
        let string_local_count = stream.g2() as usize;
        let int_arg_count = stream.g2() as usize;
        let string_arg_count = stream.g2() as usize;

        let switches = stream.g1();
        let mut switch_tables = Vec::with_capacity(switches as usize);
        for _ in 0..switches {
            let count = stream.g2();
            let mut table = HashMap::with_capacity(count as usize);
            for _ in 0..count {
                let key = stream.g4();
                let offset = stream.g4();
                table.insert(key, offset);
            }
            switch_tables.push(table);
        }

        stream.pos = 0;
        let mut info = ScriptInfo {
            script_name: stream.gjstr(),
            source_file_path: stream.gjstr(),
            lookup_key: if version >= 27 { stream.g8() } else { stream.g4() as i64 },
            ..Default::default()
        };
        let parameter_type_count = stream.g1();
        for _ in 0..parameter_type_count {
            info.parameter_types.push(stream.g1() as u8);
        }
        let line_table_len = stream.g2();
        for _ in 0..line_table_len {
            info.pcs.push(stream.g4());
            info.lines.push(stream.g4());
        }

        let mut opcodes: Vec<u16> = Vec::new();
        let mut int_operands: Vec<i32> = Vec::new();
        let mut string_operands: Vec<Option<String>> = Vec::new();
        while stream.pos < trailer_pos {
            let op = stream.g2() as u16;
            if op == opcode::PUSH_CONSTANT_STRING {
                string_operands.push(Some(stream.gjstr()));
                int_operands.push(0);
            } else if opcode::is_large_operand(op) {
                int_operands.push(stream.g4());
                string_operands.push(None);
            } else {
                int_operands.push(stream.g1());
                string_operands.push(None);
            }
            opcodes.push(op);
        }

        Ok(ScriptFile {
            id,
            info,
            int_local_count,
            string_local_count,
            int_arg_count,
            string_arg_count,
            switch_tables,
            opcodes,
            int_operands,
            string_operands,
        })
    }
}
