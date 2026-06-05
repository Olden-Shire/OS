//! ObjType — inventory item / "object" definitions. Port of `jagex3.config.ObjType` (group 10).
//!
//! Skips post-decode certificate template merging (`genCert`) and members-name masking —
//! both are server-side runtime concerns rather than cache data.

use io::Packet;

const HIDDEN: &str = "Hidden";
const TAKE: &str = "Take";
const DROP: &str = "Drop";

#[derive(Debug, Clone)]
pub struct ObjType {
    pub id: i32,
    pub model: i32,
    pub name: String,
    pub recol_s: Vec<i16>,
    pub recol_d: Vec<i16>,
    pub retex_s: Vec<i16>,
    pub retex_d: Vec<i16>,
    pub zoom2d: i32,
    pub xan2d: i32,
    pub yan2d: i32,
    pub zan2d: i32,
    pub xof2d: i32,
    pub yof2d: i32,
    pub stackable: i32,
    pub cost: i32,
    pub members: bool,
    pub op: [Option<String>; 5],
    pub iop: [Option<String>; 5],
    pub manwear: i32,
    pub manwear2: i32,
    pub manwear_offset_y: i32,
    pub womanwear: i32,
    pub womanwear2: i32,
    pub womanwear_offset_y: i32,
    pub manwear3: i32,
    pub womanwear3: i32,
    pub manhead: i32,
    pub manhead2: i32,
    pub womanhead: i32,
    pub womanhead2: i32,
    /// 10-entry table; populated only if any opcode 100..110 appeared.
    pub countobj: Vec<i32>,
    pub countco: Vec<i32>,
    pub certlink: i32,
    pub certtemplate: i32,
    pub resizex: i32,
    pub resizey: i32,
    pub resizez: i32,
    pub ambient: i32,
    pub contrast: i32,
    pub team: i32,
}

impl Default for ObjType {
    fn default() -> Self {
        Self {
            id: 0,
            model: 0,
            name: "null".to_string(),
            recol_s: Vec::new(),
            recol_d: Vec::new(),
            retex_s: Vec::new(),
            retex_d: Vec::new(),
            zoom2d: 2000,
            xan2d: 0,
            yan2d: 0,
            zan2d: 0,
            xof2d: 0,
            yof2d: 0,
            stackable: 0,
            cost: 1,
            members: false,
            op: [None, None, Some(TAKE.to_string()), None, None],
            iop: [None, None, None, None, Some(DROP.to_string())],
            manwear: -1,
            manwear2: -1,
            manwear_offset_y: 0,
            womanwear: -1,
            womanwear2: -1,
            womanwear_offset_y: 0,
            manwear3: -1,
            womanwear3: -1,
            manhead: -1,
            manhead2: -1,
            womanhead: -1,
            womanhead2: -1,
            countobj: Vec::new(),
            countco: Vec::new(),
            certlink: -1,
            certtemplate: -1,
            resizex: 128,
            resizey: 128,
            resizez: 128,
            ambient: 0,
            contrast: 0,
            team: 0,
        }
    }
}

impl ObjType {
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
            2 => self.name = p.gjstr(),
            4 => self.zoom2d = p.g2(),
            5 => self.xan2d = p.g2(),
            6 => self.yan2d = p.g2(),
            7 => self.xof2d = i32::from(p.g2b()),
            8 => self.yof2d = i32::from(p.g2b()),
            11 => self.stackable = 1,
            12 => self.cost = p.g4(),
            16 => self.members = true,
            23 => {
                self.manwear = p.g2();
                self.manwear_offset_y = p.g1();
            }
            24 => self.manwear2 = p.g2(),
            25 => {
                self.womanwear = p.g2();
                self.womanwear_offset_y = p.g1();
            }
            26 => self.womanwear2 = p.g2(),
            30..=34 => {
                let s = p.gjstr();
                self.op[(code - 30) as usize] =
                    if s.eq_ignore_ascii_case(HIDDEN) { None } else { Some(s) };
            }
            35..=39 => self.iop[(code - 35) as usize] = Some(p.gjstr()),
            40 => super::read_pairs(p, &mut self.recol_s, &mut self.recol_d),
            41 => super::read_pairs(p, &mut self.retex_s, &mut self.retex_d),
            78 => self.manwear3 = p.g2(),
            79 => self.womanwear3 = p.g2(),
            90 => self.manhead = p.g2(),
            91 => self.womanhead = p.g2(),
            92 => self.manhead2 = p.g2(),
            93 => self.womanhead2 = p.g2(),
            95 => self.zan2d = p.g2(),
            97 => self.certlink = p.g2(),
            98 => self.certtemplate = p.g2(),
            100..=109 => {
                if self.countobj.is_empty() {
                    self.countobj = vec![0; 10];
                    self.countco = vec![0; 10];
                }
                let slot = (code - 100) as usize;
                self.countobj[slot] = p.g2();
                self.countco[slot] = p.g2();
            }
            110 => self.resizex = p.g2(),
            111 => self.resizey = p.g2(),
            112 => self.resizez = p.g2(),
            113 => self.ambient = i32::from(p.g1b()),
            114 => self.contrast = i32::from(p.g1b()) * 5,
            115 => self.team = p.g1(),
            _ => panic!("ObjType {}: unknown opcode {code}", self.id),
        }
    }
}
