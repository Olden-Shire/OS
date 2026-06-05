//! NpcType — NPC definitions. Port of `jagex3.config.NpcType` (group 9).

use io::Packet;

const HIDDEN: &str = "Hidden";

#[derive(Debug, Clone)]
pub struct NpcType {
    pub id: i32,
    pub name: String,
    pub size: i32,
    pub models: Vec<i32>,
    pub head_models: Vec<i32>,
    pub readyanim: i32,
    pub turnleftanim: i32,
    pub turnrightanim: i32,
    pub walkanim: i32,
    pub walkanim_b: i32,
    pub walkanim_r: i32,
    pub walkanim_l: i32,
    pub recol_s: Vec<i16>,
    pub recol_d: Vec<i16>,
    pub retex_s: Vec<i16>,
    pub retex_d: Vec<i16>,
    pub op: [Option<String>; 5],
    pub minimap: bool,
    pub vislevel: i32,
    pub resizeh: i32,
    pub resizev: i32,
    pub alwaysontop: bool,
    pub ambient: i32,
    pub contrast: i32,
    pub headicon: i32,
    pub turnspeed: i32,
    pub multinpc: Vec<i32>,
    pub multivarbit: i32,
    pub multivarp: i32,
    pub active: bool,
    pub walksmoothing: bool,
}

impl Default for NpcType {
    fn default() -> Self {
        Self {
            id: 0,
            name: "null".to_string(),
            size: 1,
            models: Vec::new(),
            head_models: Vec::new(),
            readyanim: -1,
            turnleftanim: -1,
            turnrightanim: -1,
            walkanim: -1,
            walkanim_b: -1,
            walkanim_r: -1,
            walkanim_l: -1,
            recol_s: Vec::new(),
            recol_d: Vec::new(),
            retex_s: Vec::new(),
            retex_d: Vec::new(),
            op: [const { None }; 5],
            minimap: true,
            vislevel: -1,
            resizeh: 128,
            resizev: 128,
            alwaysontop: false,
            ambient: 0,
            contrast: 0,
            headicon: -1,
            turnspeed: 32,
            multinpc: Vec::new(),
            multivarbit: -1,
            multivarp: -1,
            active: true,
            walksmoothing: true,
        }
    }
}

impl NpcType {
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
                let n = p.g1() as usize;
                self.models = (0..n).map(|_| p.g2()).collect();
            }
            2 => self.name = p.gjstr(),
            12 => self.size = p.g1(),
            13 => self.readyanim = p.g2(),
            14 => self.walkanim = p.g2(),
            15 => self.turnleftanim = p.g2(),
            16 => self.turnrightanim = p.g2(),
            17 => {
                self.walkanim = p.g2();
                self.walkanim_b = p.g2();
                self.walkanim_r = p.g2();
                self.walkanim_l = p.g2();
            }
            30..=34 => {
                let s = p.gjstr();
                self.op[(code - 30) as usize] =
                    if s.eq_ignore_ascii_case(HIDDEN) { None } else { Some(s) };
            }
            40 => super::read_pairs(p, &mut self.recol_s, &mut self.recol_d),
            41 => super::read_pairs(p, &mut self.retex_s, &mut self.retex_d),
            60 => {
                let n = p.g1() as usize;
                self.head_models = (0..n).map(|_| p.g2()).collect();
            }
            93 => self.minimap = false,
            95 => self.vislevel = p.g2(),
            97 => self.resizeh = p.g2(),
            98 => self.resizev = p.g2(),
            99 => self.alwaysontop = true,
            100 => self.ambient = i32::from(p.g1b()),
            101 => self.contrast = i32::from(p.g1b()) * 5,
            102 => self.headicon = p.g2(),
            103 => self.turnspeed = p.g2(),
            106 => {
                self.multivarbit = p.g2();
                if self.multivarbit == 65535 {
                    self.multivarbit = -1;
                }
                self.multivarp = p.g2();
                if self.multivarp == 65535 {
                    self.multivarp = -1;
                }
                let n = p.g1() as usize;
                self.multinpc = Vec::with_capacity(n + 1);
                for _ in 0..=n {
                    let v = p.g2();
                    self.multinpc.push(if v == 65535 { -1 } else { v });
                }
            }
            107 => self.active = false,
            109 => self.walksmoothing = false,
            _ => panic!("NpcType {}: unknown opcode {code}", self.id),
        }
    }
}
