//! LocType — scenery / world-location definitions (doors, trees, fences, decor). Port of
//! `jagex3.config.LocType` (group 6).
//!
//! Skips client-side `lowMem` byte-skipping (we always decode fully) and rendering helpers
//! (`getModel*`, `buildModel`). Post-decode mirrors `postDecode` + the `breakroutefinding`
//! correction that the Java `list` factory applies.

use io::Packet;

const HIDDEN: &str = "Hidden";

#[derive(Debug, Clone)]
pub struct LocType {
    pub id: i32,
    pub models: Vec<i32>,
    pub shapes: Vec<i32>,
    pub name: String,
    pub recol_s: Vec<i16>,
    pub recol_d: Vec<i16>,
    pub retex_s: Vec<i16>,
    pub retex_d: Vec<i16>,
    pub width: i32,
    pub length: i32,
    pub blockwalk: i32,
    pub blockrange: bool,
    pub active: i32,
    pub skew_type: i32,
    pub sharelight: bool,
    pub occlude: bool,
    pub anim: i32,
    pub wallwidth: i32,
    pub ambient: i32,
    pub contrast: i32,
    pub op: [Option<String>; 5],
    pub mapfunction: i32,
    pub mapscene: i32,
    pub mirror: bool,
    pub shadow: bool,
    pub resizex: i32,
    pub resizey: i32,
    pub resizez: i32,
    pub offsetx: i32,
    pub offsety: i32,
    pub offsetz: i32,
    pub forceapproach: i32,
    pub forcedecor: bool,
    pub breakroutefinding: bool,
    pub raiseobject: i32,
    pub multiloc: Vec<i32>,
    pub multivarbit: i32,
    pub multivarp: i32,
    pub bgsound_sound: i32,
    pub bgsound_range: i32,
    pub bgsound_mindelay: i32,
    pub bgsound_maxdelay: i32,
    pub bgsound_random: Vec<i32>,
}

impl Default for LocType {
    fn default() -> Self {
        Self {
            id: 0,
            models: Vec::new(),
            shapes: Vec::new(),
            name: "null".to_string(),
            recol_s: Vec::new(),
            recol_d: Vec::new(),
            retex_s: Vec::new(),
            retex_d: Vec::new(),
            width: 1,
            length: 1,
            blockwalk: 2,
            blockrange: true,
            active: -1,
            skew_type: -1,
            sharelight: false,
            occlude: false,
            anim: -1,
            wallwidth: 16,
            ambient: 0,
            contrast: 0,
            op: [const { None }; 5],
            mapfunction: -1,
            mapscene: -1,
            mirror: false,
            shadow: true,
            resizex: 128,
            resizey: 128,
            resizez: 128,
            offsetx: 0,
            offsety: 0,
            offsetz: 0,
            forceapproach: 0,
            forcedecor: false,
            breakroutefinding: false,
            raiseobject: -1,
            multiloc: Vec::new(),
            multivarbit: -1,
            multivarp: -1,
            bgsound_sound: -1,
            bgsound_range: 0,
            bgsound_mindelay: 0,
            bgsound_maxdelay: 0,
            bgsound_random: Vec::new(),
        }
    }
}

impl LocType {
    pub fn decode(id: i32, bytes: &[u8]) -> Self {
        let mut t = Self { id, ..Self::default() };
        let mut p = Packet::from_vec(bytes.to_vec());
        loop {
            let code = p.g1();
            if code == 0 {
                t.post_decode();
                return t;
            }
            t.decode_opcode(&mut p, code);
        }
    }

    fn decode_opcode(&mut self, p: &mut Packet, code: i32) {
        match code {
            1 => {
                let n = p.g1() as usize;
                self.models = Vec::with_capacity(n);
                self.shapes = Vec::with_capacity(n);
                for _ in 0..n {
                    self.models.push(p.g2());
                    self.shapes.push(p.g1());
                }
            }
            2 => self.name = p.gjstr(),
            5 => {
                let n = p.g1() as usize;
                self.shapes.clear();
                self.models = (0..n).map(|_| p.g2()).collect();
            }
            14 => self.width = p.g1(),
            15 => self.length = p.g1(),
            17 => {
                self.blockwalk = 0;
                self.blockrange = false;
            }
            18 => self.blockrange = false,
            19 => self.active = p.g1(),
            21 => self.skew_type = 0,
            22 => self.sharelight = true,
            23 => self.occlude = true,
            24 => {
                let v = p.g2();
                self.anim = if v == 65535 { -1 } else { v };
            }
            27 => self.blockwalk = 1,
            28 => self.wallwidth = p.g1(),
            29 => self.ambient = i32::from(p.g1b()),
            30..=34 => {
                let s = p.gjstr();
                self.op[(code - 30) as usize] =
                    if s.eq_ignore_ascii_case(HIDDEN) { None } else { Some(s) };
            }
            39 => self.contrast = i32::from(p.g1b()) * 25,
            40 => super::read_pairs(p, &mut self.recol_s, &mut self.recol_d),
            41 => super::read_pairs(p, &mut self.retex_s, &mut self.retex_d),
            60 => self.mapfunction = p.g2(),
            62 => self.mirror = true,
            64 => self.shadow = false,
            65 => self.resizex = p.g2(),
            66 => self.resizey = p.g2(),
            67 => self.resizez = p.g2(),
            68 => self.mapscene = p.g2(),
            69 => self.forceapproach = p.g1(),
            70 => self.offsetx = i32::from(p.g2b()),
            71 => self.offsety = i32::from(p.g2b()),
            72 => self.offsetz = i32::from(p.g2b()),
            73 => self.forcedecor = true,
            74 => self.breakroutefinding = true,
            75 => self.raiseobject = p.g1(),
            77 => {
                self.multivarbit = p.g2();
                if self.multivarbit == 65535 {
                    self.multivarbit = -1;
                }
                self.multivarp = p.g2();
                if self.multivarp == 65535 {
                    self.multivarp = -1;
                }
                let n = p.g1() as usize;
                self.multiloc = Vec::with_capacity(n + 1);
                for _ in 0..=n {
                    let v = p.g2();
                    self.multiloc.push(if v == 65535 { -1 } else { v });
                }
            }
            78 => {
                self.bgsound_sound = p.g2();
                self.bgsound_range = p.g1();
            }
            79 => {
                self.bgsound_mindelay = p.g2();
                self.bgsound_maxdelay = p.g2();
                self.bgsound_range = p.g1();
                let n = p.g1() as usize;
                self.bgsound_random = (0..n).map(|_| p.g2()).collect();
            }
            81 => self.skew_type = p.g1() * 256,
            _ => panic!("LocType {}: unknown opcode {code}", self.id),
        }
    }

    fn post_decode(&mut self) {
        if self.active == -1 {
            self.active = 0;
            // "Active if has a default model with no shape, or shape[0] == 10 (regular obj)"
            let default_shape =
                self.shapes.is_empty() || self.shapes.first().copied() == Some(10);
            if !self.models.is_empty() && default_shape {
                self.active = 1;
            }
            if self.op.iter().any(Option::is_some) {
                self.active = 1;
            }
        }
        if self.raiseobject == -1 {
            self.raiseobject = if self.blockwalk == 0 { 0 } else { 1 };
        }
        // The Java `list` factory applies this after `postDecode`; fold it in here so
        // decode() returns a fully-finalised record.
        if self.breakroutefinding {
            self.blockwalk = 0;
            self.blockrange = false;
        }
    }
}
