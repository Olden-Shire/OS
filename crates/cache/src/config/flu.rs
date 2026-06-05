//! FluType — floor underlay color. Port of `jagex3.config.FluType` (group 1).
//!
//! Only opcode 1 (colour) carries data; the rest of the Java class is HSL derivation for
//! rendering.

use io::Packet;

#[derive(Debug, Default, Clone)]
pub struct FluType {
    pub id: i32,
    pub colour: i32,
}

impl FluType {
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
            1 => self.colour = p.g3(),
            _ => panic!("FluType {}: unknown opcode {code}", self.id),
        }
    }
}
