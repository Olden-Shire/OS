//! VarBitType — a bit-range slice of a VarP. Port of `jagex3.config.VarBitType` (group 14).

use io::Packet;

#[derive(Debug, Default, Clone)]
pub struct VarBitType {
    pub id: i32,
    pub basevar: i32,
    pub startbit: i32,
    pub endbit: i32,
}

impl VarBitType {
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
            1 => {
                self.basevar = p.g2();
                self.startbit = p.g1();
                self.endbit = p.g1();
            }
            _ => panic!("VarBitType {}: unknown opcode {code}", self.id),
        }
    }
}
