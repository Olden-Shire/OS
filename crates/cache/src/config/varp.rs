//! VarpType — player-variable definitions. Port of `jagex3.config.VarpType` (group 16).
//!
//! Only opcode 5 (clientcode) is meaningful in rev1; all other VarP behavior is implicit in
//! the slot's index. Unknown opcodes panic — they indicate the cache isn't rev1.

use io::Packet;

#[derive(Debug, Default, Clone)]
pub struct VarpType {
    pub id: i32,
    pub clientcode: i32,
}

impl VarpType {
    pub fn decode(id: i32, bytes: &[u8]) -> Self {
        let mut t = Self { id, ..Self::default() };
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
            5 => self.clientcode = p.g2(),
            _ => panic!("VarpType {}: unknown opcode {code}", self.id),
        }
    }
}
