// @ObfuscatedName("ae") — jag::oldscape::dash3d::AnimFrame
//
// Single keyframe of an animation. Decoded packet contains a series of
// (bone index, transformation delta) entries. The `ti` array holds
// bone indices in the order they apply; `tx`/`ty`/`tz` are the per-axis
// deltas — translation, rotation, or scale per the bone's AnimBase
// type. animateTransparencies is set when any bone has type==5.

#![allow(dead_code)]

use std::sync::Arc;

use crate::dash3d::anim_base::AnimBase;
use crate::io::packet::Packet;

#[derive(Debug, Clone)]
pub struct AnimFrame {
    // @ObfuscatedName("ae.c") — Bone hierarchy this frame references.
    pub base: Arc<AnimBase>,
    // @ObfuscatedName("ae.n")
    pub size: i32,
    // @ObfuscatedName("ae.j") / "ae.z" / "ae.g" / "ae.q"
    pub ti: Vec<i32>,
    pub tx: Vec<i32>,
    pub ty: Vec<i32>,
    pub tz: Vec<i32>,
    // @ObfuscatedName("ae.i")
    pub animate_transparencies: bool,
}

impl AnimFrame {
    // Java AnimFrame ctor reads two cursors over the same buffer: one
    // for the bone-mask byte stream, one for the smart-int delta
    // stream. We mirror the same dual-cursor walk.
    pub fn decode(src: &[u8], base: Arc<AnimBase>) -> Option<Self> {
        let mut p_mask = Packet::from_vec(src.to_vec());
        let mut p_data = Packet::from_vec(src.to_vec());
        p_mask.pos = 2;
        let var5 = p_mask.g1();
        let mut last_bone = -1i32;
        let mut length = 0usize;
        p_data.pos = p_mask.pos + var5;

        let mut temp_ti = vec![0i32; 500];
        let mut temp_tx = vec![0i32; 500];
        let mut temp_ty = vec![0i32; 500];
        let mut temp_tz = vec![0i32; 500];
        let mut animate_transparencies = false;
        for var8 in 0..var5 {
            let var9 = p_mask.g1();
            if var9 <= 0 { continue; }

            if base.r#type.get(var8 as usize).copied().unwrap_or(0) != 0 {
                // Walk back to find the closest preceding type-0 bone
                // and emit a "reset origin" entry so subsequent ops
                // anchor at that bone's group.
                for var10 in (last_bone + 1..var8).rev() {
                    if base.r#type[var10 as usize] == 0 {
                        temp_ti[length] = var10;
                        temp_tx[length] = 0;
                        temp_ty[length] = 0;
                        temp_tz[length] = 0;
                        length += 1;
                        break;
                    }
                }
            }

            temp_ti[length] = var8;
            // Java's neutral element for type-3 (scale) is 128; for
            // every other type it's 0.
            let neutral = if base.r#type.get(var8 as usize).copied().unwrap_or(0) == 3 { 128 } else { 0 };
            temp_tx[length] = if (var9 & 0x1) == 0 { neutral } else { p_data.gsmarts() };
            temp_ty[length] = if (var9 & 0x2) == 0 { neutral } else { p_data.gsmarts() };
            temp_tz[length] = if (var9 & 0x4) == 0 { neutral } else { p_data.gsmarts() };
            last_bone = var8;
            length += 1;
            if base.r#type.get(var8 as usize).copied().unwrap_or(0) == 5 {
                animate_transparencies = true;
            }
        }
        if p_data.pos as usize != src.len() {
            // Java's AnimFrame.java:109-111 throws RuntimeException.
            // We can't mirror it: panic here propagates through the
            // anim_frame_set::get → animate_loc_model → fetch_loc_model
            // chain, and the only `catch_unwind` upstream is scoped to
            // the `from_unlit_flat` call — NOT to the animation step.
            // So a single corrupt frame would kill the entire loc
            // render loop, leaving only terrain visible. Returning
            // None drops just the offending frame.
            return None;
        }
        Some(AnimFrame {
            base,
            size: length as i32,
            ti: temp_ti[..length].to_vec(),
            tx: temp_tx[..length].to_vec(),
            ty: temp_ty[..length].to_vec(),
            tz: temp_tz[..length].to_vec(),
            animate_transparencies,
        })
    }
}
