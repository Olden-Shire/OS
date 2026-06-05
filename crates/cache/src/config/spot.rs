//! SpotType — short-lived spot/graphical-effect animation. Port of `jagex3.config.SpotType`
//! (group 13). Server uses these to schedule "play spotanim at coord" effects; only the
//! data fields matter, not the model-loading methods.

use io::Packet;

#[derive(Debug, Clone)]
pub struct SpotType {
    pub id: i32,
    pub model: i32,
    pub anim: i32,
    pub recol_s: Vec<i16>,
    pub recol_d: Vec<i16>,
    pub retex_s: Vec<i16>,
    pub retex_d: Vec<i16>,
    pub resizeh: i32,
    pub resizev: i32,
    pub angle: i32,
    pub ambient: i32,
    pub contrast: i32,
}

impl Default for SpotType {
    fn default() -> Self {
        Self {
            id: 0,
            model: 0,
            anim: -1,
            recol_s: Vec::new(),
            recol_d: Vec::new(),
            retex_s: Vec::new(),
            retex_d: Vec::new(),
            resizeh: 128,
            resizev: 128,
            angle: 0,
            ambient: 0,
            contrast: 0,
        }
    }
}

impl SpotType {
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
            1 => self.model = p.g2(),
            2 => self.anim = p.g2(),
            4 => self.resizeh = p.g2(),
            5 => self.resizev = p.g2(),
            6 => self.angle = p.g2(),
            7 => self.ambient = p.g1(),
            8 => self.contrast = p.g1(),
            40 => super::read_pairs(p, &mut self.recol_s, &mut self.recol_d),
            41 => super::read_pairs(p, &mut self.retex_s, &mut self.retex_d),
            _ => panic!("SpotType {}: unknown opcode {code}", self.id),
        }
    }
}
