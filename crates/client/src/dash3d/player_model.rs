// @ObfuscatedName("ct") — jag::oldscape::rs2lib::PlayerModel.
//
// A player's appearance state: 12 appearance slots (0 = empty,
// 256..511 = IdkType body part + 256, >= 512 = worn ObjType + 512),
// the 5 skin/hair colour indices, gender, and the optional npc
// transmog. Composes the per-frame avatar ModelLit (worn models
// merged, recoloured by the RecolsRunescape palettes, lit, cached by
// the 64-bit appearance hash) and the chathead ModelUnlit. Verbatim
// port of PlayerModel.java.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::config::{idk_type, obj_type};
use crate::dash3d::model_lit::ModelLit;
use crate::dash3d::model_unlit::ModelUnlit;
use crate::dash3d::recols_runescape as recols;

// @ObfuscatedName("ct.i") — appearance-slot order for the 7 idk body
// parts (hair, jaw, torso, arms, hands, legs, feet).
pub const BASE_PART_MAP: [usize; 7] = [8, 11, 4, 6, 9, 7, 10];

// @ObfuscatedName("ct.s") — modelCache (Java LruCache(260); plain map
// per the loc/obj cache convention). Keyed by the appearance hash.
pub static MODEL_CACHE: std::sync::LazyLock<Mutex<HashMap<i64, Arc<ModelLit>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

// @ObfuscatedName("ba.q(I)V") — PlayerModel.resetCache.
pub fn reset_cache() {
    MODEL_CACHE.lock().unwrap().clear();
}

#[derive(Debug, Clone)]
pub struct PlayerModel {
    // @ObfuscatedName("ct.r")
    pub appearance: [i32; 12],
    // @ObfuscatedName("ct.d")
    pub colour: [i32; 5],
    // @ObfuscatedName("ct.l")
    pub gender: bool,
    // @ObfuscatedName("ct.m") — npc transmog id (-1 = none).
    pub transmog: i32,
    // @ObfuscatedName("ct.c") — the appearance hash / cache key.
    pub base_id: i64,
    // @ObfuscatedName("ct.n") — the last composed model's cache key,
    // used as a stale-frame fallback while new worn assets stream in.
    pub head_model_hash: i64,

    // Java keeps PlayerModel as a nullable field on ClientPlayer;
    // `ClientPlayer.ready()` is `model != null`. We always own the
    // struct, so this flag carries the "appearance decoded" state.
    pub applied: bool,
    // Convenience mirrors written by ClientPlayer.set_appearance for
    // overlay/tooltip consumers.
    pub name: String,
    pub combat_level: i32,
}

impl Default for PlayerModel {
    fn default() -> Self {
        Self {
            appearance: [0; 12],
            colour: [0; 5],
            gender: false,
            transmog: -1,
            base_id: 0,
            head_model_hash: -1,
            applied: false,
            name: String::new(),
            combat_level: 3,
        }
    }
}

impl PlayerModel {
    pub fn new() -> Self { Self::default() }

    // @ObfuscatedName("ct.r([I[IZII)V") — PlayerModel.setAppearance.
    // Verbatim port of PlayerModel.java:60-78. A None appearance
    // (the character-design default) fills the 7 idk parts with the
    // first enabled IdkType of each body-part type for the gender.
    pub fn set_appearance(&mut self, appearance: Option<[i32; 12]>,
                          colour: [i32; 5], gender: bool, transmog: i32) {
        let appearance = match appearance {
            Some(a) => a,
            None => {
                let mut a = [0i32; 12];
                let num = idk_type::NUM_DEFINITIONS
                    .load(std::sync::atomic::Ordering::Relaxed);
                for part in 0..7 {
                    for id in 0..num {
                        let t = idk_type::list(id);
                        if !t.disable && t.type_ == part as i32 + if gender { 7 } else { 0 } {
                            a[BASE_PART_MAP[part]] = id + 256;
                            break;
                        }
                    }
                }
                a
            }
        };
        self.appearance = appearance;
        self.colour = colour;
        self.gender = gender;
        self.transmog = transmog;
        self.applied = true;
        self.calc_base_id();
    }

    // Back-compat shim — the original stub's entry point used by
    // ClientPlayer.set_appearance.
    pub fn apply_appearance(&mut self, worn: [i32; 12], recols: [i32; 5],
                            female: bool, npc_override: i32) {
        self.set_appearance(Some(worn), recols, female, npc_override);
    }

    // @ObfuscatedName("ct.d(IZI)V") — PlayerModel.idkChangePart.
    // Verbatim port of PlayerModel.java:82-108: cycle body part
    // `part` (0-6) forward/back through the enabled IdkTypes of the
    // matching gendered type. Part 1 (jaw) is locked for females.
    pub fn idk_change_part(&mut self, part: usize, forward: bool) {
        if part == 1 && self.gender {
            return;
        }
        let mut id = self.appearance[BASE_PART_MAP[part]];
        if id == 0 {
            return;
        }
        id -= 256;
        let num = idk_type::NUM_DEFINITIONS.load(std::sync::atomic::Ordering::Relaxed);
        if num <= 0 {
            return;
        }
        loop {
            if forward {
                id += 1;
                if id >= num {
                    id = 0;
                }
            } else {
                id -= 1;
                if id < 0 {
                    id = num - 1;
                }
            }
            let t = idk_type::list(id);
            if !t.disable
                && t.type_ == if self.gender { 7 } else { 0 } + part as i32
            {
                break;
            }
        }
        self.appearance[BASE_PART_MAP[part]] = id + 256;
        self.calc_base_id();
    }

    // @ObfuscatedName("ct.l(IZI)V") — PlayerModel.idkChangeColour.
    pub fn idk_change_colour(&mut self, slot: usize, forward: bool) {
        let palette_len = recols::recol1d(slot).len() as i32;
        if palette_len == 0 {
            return;
        }
        let mut v = self.colour[slot];
        if forward {
            v += 1;
            if v >= palette_len {
                v = 0;
            }
        } else {
            v -= 1;
            if v < 0 {
                v = palette_len - 1;
            }
        }
        self.colour[slot] = v;
        self.calc_base_id();
    }

    // @ObfuscatedName("ct.m(ZI)V") — PlayerModel.idkChangeGender.
    pub fn idk_change_gender(&mut self, gender: bool) {
        if self.gender != gender {
            self.set_appearance(None, self.colour, gender, -1);
        }
    }

    // @ObfuscatedName("ct.c(Lev;I)V") — PlayerModel.idkSaveDesign.
    // Writes the design payload onto the IDK_SAVEDESIGN packet.
    pub fn idk_save_design(&self, out: &mut crate::io::packet::Packet) {
        out.p1(if self.gender { 1 } else { 0 });
        for part in 0..7 {
            let v = self.appearance[BASE_PART_MAP[part]];
            if v == 0 {
                out.p1(-1);
            } else {
                out.p1(v - 256);
            }
        }
        for slot in 0..5 {
            out.p1(self.colour[slot]);
        }
    }

    // @ObfuscatedName("ct.n(I)V") — PlayerModel.calcBaseId. Verbatim
    // port of PlayerModel.java:156-186: the 64-bit appearance hash
    // (slots 5/9 swapped during hashing), evicting the old cache
    // entry when the hash changes.
    pub fn calc_base_id(&mut self) {
        let old = self.base_id;
        let s5 = self.appearance[5];
        let s9 = self.appearance[9];
        self.appearance[5] = s9;
        self.appearance[9] = s5;
        self.base_id = 0;
        for i in 0..12 {
            self.base_id <<= 4;
            if self.appearance[i] >= 256 {
                self.base_id += (self.appearance[i] - 256) as i64;
            }
        }
        if self.appearance[0] >= 256 {
            self.base_id += ((self.appearance[0] - 256) >> 4) as i64;
        }
        if self.appearance[1] >= 256 {
            self.base_id += ((self.appearance[1] - 256) >> 8) as i64;
        }
        for i in 0..5 {
            self.base_id <<= 3;
            self.base_id += self.colour[i] as i64;
        }
        self.base_id <<= 1;
        self.base_id += if self.gender { 1 } else { 0 };
        self.appearance[5] = s5;
        self.appearance[9] = s9;
        if old != 0 && self.base_id != old {
            MODEL_CACHE.lock().unwrap().remove(&old);
        }
    }

    // @ObfuscatedName("ct.j(Leo;ILeo;IB)Lfo;") — PlayerModel.
    // getTempModel. Verbatim port of PlayerModel.java:190-274:
    // transmog → NpcType path; seq replaceheld overrides patch the
    // weapon/shield slots into a derived hash; the base composed
    // model caches by hash (with the previous frame's model as a
    // fallback while worn assets stream); then the primary/secondary
    // seq animation pass.
    pub fn get_temp_model(
        &mut self,
        primary: Option<&crate::config::seq_type::SeqType>,
        primary_frame: i32,
        secondary: Option<&crate::config::seq_type::SeqType>,
        secondary_frame: i32,
    ) -> Option<ModelLit> {
        if self.transmog != -1 {
            return crate::config::npc_type::list(self.transmog)
                .get_temp_model(primary, primary_frame, secondary, secondary_frame);
        }

        let mut hash = self.base_id;
        let mut slots = self.appearance;
        if let Some(p) = primary {
            if p.replaceheldleft >= 0 || p.replaceheldright >= 0 {
                // Java PlayerModel.java:202/206 — `replaceheldX - appearance[n]`
                // is an int, and `int << 40` / `int << 48` mask the shift to
                // its low 5 bits (40&31=8, 48&31=16), computed in 32-bit then
                // sign-extended to long for `var5 +=`. wrapping_shl replicates
                // that masking exactly; casting to i64 first (as the old code
                // did) would shift the full 40/48 and diverge the cache key.
                if p.replaceheldleft >= 0 {
                    hash += p.replaceheldleft.wrapping_sub(self.appearance[5])
                        .wrapping_shl(40) as i64;
                    slots[5] = p.replaceheldleft;
                }
                if p.replaceheldright >= 0 {
                    hash += p.replaceheldright.wrapping_sub(self.appearance[3])
                        .wrapping_shl(48) as i64;
                    slots[3] = p.replaceheldright;
                }
            }
        }

        let mut base = MODEL_CACHE.lock().unwrap().get(&hash).cloned();
        if base.is_none() {
            // Asset readiness sweep — any missing model falls back to
            // the previous frame's composed model (or skips the frame).
            let mut missing = false;
            for &slot in &slots {
                if (256..512).contains(&slot)
                    && !idk_type::list(slot - 256).check_model()
                {
                    missing = true;
                }
                if slot >= 512 {
                    let ready = obj_type::list(slot - 512)
                        .map_or(false, |o| o.check_wear_model(self.gender));
                    if !ready {
                        missing = true;
                    }
                }
            }
            if missing {
                if self.head_model_hash != -1 {
                    base = MODEL_CACHE.lock().unwrap()
                        .get(&self.head_model_hash).cloned();
                }
                if base.is_none() {
                    return None;
                }
            }

            if base.is_none() {
                let mut parts: Vec<ModelUnlit> = Vec::with_capacity(12);
                for &slot in &slots {
                    if (256..512).contains(&slot) {
                        if let Some(m) = idk_type::list(slot - 256).get_model_no_check() {
                            parts.push(m);
                        }
                    }
                    if slot >= 512 {
                        if let Some(m) = obj_type::list(slot - 512)
                            .and_then(|o| o.get_wear_model_no_check(self.gender))
                        {
                            parts.push(m);
                        }
                    }
                }
                if parts.is_empty() {
                    eprintln!("[dbg-walk] PlayerModel parts EMPTY: slots={:?} gender={}",
                              slots, self.gender);
                }
                let mut unlit = ModelUnlit::merge(&parts.iter().collect::<Vec<_>>());
                for i in 0..5 {
                    let pal1 = recols::recol1d(i);
                    if (self.colour[i] as usize) < pal1.len() {
                        unlit.recolour(recols::RECOL1S[i], pal1[self.colour[i] as usize]);
                    }
                    let pal2 = recols::recol2d(i);
                    if (self.colour[i] as usize) < pal2.len() {
                        unlit.recolour(recols::RECOL2S[i], pal2[self.colour[i] as usize]);
                    }
                }
                let lit = Arc::new(ModelLit::light(&mut unlit, 64, 850, -30, -50, -30));
                MODEL_CACHE.lock().unwrap().insert(hash, Arc::clone(&lit));
                self.head_model_hash = hash;
                base = Some(lit);
            }
        }
        let base = base?;

        let model = match (primary, secondary) {
            (None, None) => (*base).clone(),
            (Some(p), Some(s)) => p.split_animate_model(&base, primary_frame, s, secondary_frame),
            (None, Some(s)) => s.animate_model(&base, secondary_frame),
            (Some(p), None) => p.animate_model(&base, primary_frame),
        };
        Some(model)
    }

    // @ObfuscatedName("ct.z(I)Lfw;") — PlayerModel.getHeadModel.
    // Verbatim port of PlayerModel.java:278-322 — the unlit chathead
    // composition (idk head overlays + worn head models, recoloured).
    pub fn get_head_model(&self) -> Option<ModelUnlit> {
        if self.transmog != -1 {
            return crate::config::npc_type::list(self.transmog).get_head();
        }

        for &slot in &self.appearance {
            if (256..512).contains(&slot) && !idk_type::list(slot - 256).check_head() {
                return None;
            }
            if slot >= 512 {
                let ready = obj_type::list(slot - 512)
                    .map_or(false, |o| o.check_head_model(self.gender));
                if !ready {
                    return None;
                }
            }
        }

        let mut parts: Vec<ModelUnlit> = Vec::with_capacity(12);
        for &slot in &self.appearance {
            if (256..512).contains(&slot) {
                if let Some(m) = idk_type::list(slot - 256).get_head_no_check() {
                    parts.push(m);
                }
            }
            if slot >= 512 {
                if let Some(m) = obj_type::list(slot - 512)
                    .and_then(|o| o.get_head_model_no_check(self.gender))
                {
                    parts.push(m);
                }
            }
        }
        let mut model = ModelUnlit::merge(&parts.iter().collect::<Vec<_>>());
        for i in 0..5 {
            let pal1 = recols::recol1d(i);
            if (self.colour[i] as usize) < pal1.len() {
                model.recolour(recols::RECOL1S[i], pal1[self.colour[i] as usize]);
            }
            let pal2 = recols::recol2d(i);
            if (self.colour[i] as usize) < pal2.len() {
                model.recolour(recols::RECOL2S[i], pal2[self.colour[i] as usize]);
            }
        }
        Some(model)
    }

    // @ObfuscatedName("ct.g(I)I") — the player-head identity hash used
    // by cc_setplayerhead_self / IfType type-3 components.
    pub fn head_hash(&self) -> i32 {
        if self.transmog != -1 {
            return self.transmog + 0x12345678;
        }
        (self.appearance[11] << 5)
            + (self.appearance[8] << 10)
            + (self.appearance[0] << 15)
            + (self.colour[4] << 20)
            + (self.colour[0] << 25)
            + self.appearance[1]
    }
}
