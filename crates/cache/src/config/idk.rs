//! IdkType — "Identity Kit" body part definitions for player customization. Port of
//! `jagex3.config.IdkType` (group 3).

use io::Packet;

#[derive(Debug, Clone)]
pub struct IdkType {
    pub id: i32,
    /// Body part slot type (-1 = unset; head/body/legs/etc. encoded by Jagex)
    pub type_: i32,
    pub models: Vec<i32>,
    pub recol_s: Vec<i16>,
    pub recol_d: Vec<i16>,
    pub retex_s: Vec<i16>,
    pub retex_d: Vec<i16>,
    /// Five head-model slots. -1 means absent.
    pub head: [i32; 5],
    pub disable: bool,
}

impl Default for IdkType {
    fn default() -> Self {
        Self {
            id: 0,
            type_: -1,
            models: Vec::new(),
            recol_s: Vec::new(),
            recol_d: Vec::new(),
            retex_s: Vec::new(),
            retex_d: Vec::new(),
            head: [-1; 5],
            disable: false,
        }
    }
}

impl IdkType {
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
            1 => self.type_ = p.g1(),
            2 => {
                let n = p.g1() as usize;
                self.models = (0..n).map(|_| p.g2()).collect();
            }
            3 => self.disable = true,
            40 => super::read_pairs(p, &mut self.recol_s, &mut self.recol_d),
            41 => super::read_pairs(p, &mut self.retex_s, &mut self.retex_d),
            60..=69 => self.head[(code - 60) as usize] = p.g2(),
            _ => panic!("IdkType {}: unknown opcode {code}", self.id),
        }
    }
}
