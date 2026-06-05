//! EnumType — typed key→value lookup tables. Port of `jagex3.config.EnumType` (group 8).
//!
//! An EnumType is either string-valued or int-valued; `outputtype` (an ASCII char like 'i'
//! or 's') discriminates. `stringValues` is populated iff opcode 5 was seen; `intValues` iff
//! opcode 6 was seen.

use io::Packet;

#[derive(Debug, Default, Clone)]
pub struct EnumType {
    pub id: i32,
    pub inputtype: i32,
    pub outputtype: u8,
    pub default_string: String,
    pub default_int: i32,
    pub count: i32,
    pub keys: Vec<i32>,
    pub int_values: Vec<i32>,
    pub string_values: Vec<String>,
}

impl EnumType {
    pub fn decode(id: i32, bytes: &[u8]) -> Self {
        let mut t = Self {
            id,
            default_string: "null".to_string(),
            ..Self::default()
        };
        let mut p = Packet::from_vec(bytes.to_vec());
        loop {
            let code = p.g1();
            if code == 0 {
                return t;
            }
            t.decode_opcode(&mut p, code);
        }
    }

    fn decode_opcode(&mut self, p: &mut Packet, code: i32) {
        match code {
            1 => self.inputtype = p.g1(),
            2 => self.outputtype = p.g1() as u8,
            3 => self.default_string = p.gjstr(),
            4 => self.default_int = p.g4(),
            5 => {
                self.count = p.g2();
                self.keys = Vec::with_capacity(self.count as usize);
                self.string_values = Vec::with_capacity(self.count as usize);
                for _ in 0..self.count {
                    self.keys.push(p.g4());
                    self.string_values.push(p.gjstr());
                }
            }
            6 => {
                self.count = p.g2();
                self.keys = Vec::with_capacity(self.count as usize);
                self.int_values = Vec::with_capacity(self.count as usize);
                for _ in 0..self.count {
                    self.keys.push(p.g4());
                    self.int_values.push(p.g4());
                }
            }
            _ => panic!("EnumType {}: unknown opcode {code}", self.id),
        }
    }
}
