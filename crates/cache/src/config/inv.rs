//! InvType — inventory size definitions. Port of `jagex3.config.InvType` (group 5).

use io::Packet;

#[derive(Debug, Default, Clone)]
pub struct InvType {
    pub id: i32,
    pub size: i32,
}

impl InvType {
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
            2 => self.size = p.g2(),
            _ => panic!("InvType {}: unknown opcode {code}", self.id),
        }
    }
}
