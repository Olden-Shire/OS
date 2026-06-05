//! SeqType — animation sequence definitions. Port of `jagex3.config.SeqType` (group 12).
//!
//! Frame entries pack two integers into one i32: `frames[i] >> 16` is the frame-set id (an
//! index into archive 0/anims), `frames[i] & 0xFFFF` is the frame within that set. Same
//! encoding for `iframes` (interleaved animations).

use io::Packet;

#[derive(Debug, Clone)]
pub struct SeqType {
    pub id: i32,
    pub frames: Vec<i32>,
    pub iframes: Vec<i32>,
    pub delay: Vec<i32>,
    pub sound: Vec<i32>,
    pub loops: i32,
    pub walkmerge: Vec<i32>,
    pub reachforward: bool,
    pub priority: i32,
    pub replaceheldleft: i32,
    pub replaceheldright: i32,
    pub maxloops: i32,
    pub preanim_move: i32,
    pub postanim_move: i32,
    pub duplicatebehaviour: i32,
}

impl Default for SeqType {
    fn default() -> Self {
        Self {
            id: 0,
            frames: Vec::new(),
            iframes: Vec::new(),
            delay: Vec::new(),
            sound: Vec::new(),
            loops: -1,
            walkmerge: Vec::new(),
            reachforward: false,
            priority: 5,
            replaceheldleft: -1,
            replaceheldright: -1,
            maxloops: 99,
            preanim_move: -1,
            postanim_move: -1,
            duplicatebehaviour: 2,
        }
    }
}

impl SeqType {
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
                let n = p.g2() as usize;
                self.delay = (0..n).map(|_| p.g2()).collect();
                self.frames = (0..n).map(|_| p.g2()).collect();
                // Second pass merges the high 16 bits (frame-set id).
                for f in self.frames.iter_mut().take(n) {
                    *f += p.g2() << 16;
                }
            }
            2 => self.loops = p.g2(),
            3 => {
                let n = p.g1() as usize;
                self.walkmerge = Vec::with_capacity(n + 1);
                for _ in 0..n {
                    self.walkmerge.push(p.g1());
                }
                // Sentinel terminator (matches Java's 9999999 magic value).
                self.walkmerge.push(9_999_999);
            }
            4 => self.reachforward = true,
            5 => self.priority = p.g1(),
            6 => self.replaceheldleft = p.g2(),
            7 => self.replaceheldright = p.g2(),
            8 => self.maxloops = p.g1(),
            9 => self.preanim_move = p.g1(),
            10 => self.postanim_move = p.g1(),
            11 => self.duplicatebehaviour = p.g1(),
            12 => {
                let n = p.g1() as usize;
                self.iframes = (0..n).map(|_| p.g2()).collect();
                for f in self.iframes.iter_mut().take(n) {
                    *f += p.g2() << 16;
                }
            }
            13 => {
                let n = p.g1() as usize;
                self.sound = (0..n).map(|_| p.g3()).collect();
            }
            _ => panic!("SeqType {}: unknown opcode {code}", self.id),
        }
    }

    /// Mirrors `postDecode`: if pre/postanim_move wasn't set, default it based on whether
    /// walkmerge is present (0 = no walk merging, 2 = walk merges).
    fn post_decode(&mut self) {
        if self.preanim_move == -1 {
            self.preanim_move = if self.walkmerge.is_empty() { 0 } else { 2 };
        }
        if self.postanim_move == -1 {
            self.postanim_move = if self.walkmerge.is_empty() { 0 } else { 2 };
        }
    }
}
