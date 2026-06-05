//! FloType — floor overlay texture. Port of `jagex3.config.FloType` (group 4).
//!
//! Rendering-side helpers (`postDecode`, `getHsl`) are skipped — the server doesn't paint
//! tiles. We keep only the raw fields the decoder writes.

use io::Packet;

#[derive(Debug, Clone)]
pub struct FloType {
    pub id: i32,
    pub colour: i32,
    pub texture: i32,
    pub occlude: bool,
    pub mapcolour: i32,
}

impl Default for FloType {
    fn default() -> Self {
        Self { id: 0, colour: 0, texture: -1, occlude: true, mapcolour: -1 }
    }
}

impl FloType {
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
            2 => self.texture = p.g1(),
            5 => self.occlude = false,
            7 => self.mapcolour = p.g3(),
            8 => { /* "default water = id" — Java client TODO, no field assignment */ }
            _ => panic!("FloType {}: unknown opcode {code}", self.id),
        }
    }
}
