// @ObfuscatedName("ez") — jag::oldscape::dash3d::AnimBase
//
// Bone hierarchy for animated models. `type[i]` is the transformation
// kind for bone i (0=group/origin, 1=translate, 2=rotate, 3=scale,
// 5=animate face alpha). `labels[i]` is the list of vertex labels the
// bone affects. ModelLit.animate uses these to apply per-frame deltas.

#![allow(dead_code)]

use crate::io::packet::Packet;

#[derive(Debug, Clone)]
pub struct AnimBase {
    // @ObfuscatedName("ez.m")
    pub id: i32,
    // @ObfuscatedName("ez.c")
    pub size: i32,
    // @ObfuscatedName("ez.n") — bone type per index.
    pub r#type: Vec<i32>,
    // @ObfuscatedName("ez.j") — vertex labels affected by each bone.
    pub labels: Vec<Vec<i32>>,
}

impl AnimBase {
    pub fn decode(id: i32, src: &[u8]) -> Self {
        let mut buf = Packet::from_vec(src.to_vec());
        let size = buf.g1();
        let mut r#type = vec![0i32; size as usize];
        let mut labels: Vec<Vec<i32>> = (0..size as usize).map(|_| Vec::new()).collect();
        for i in 0..size as usize {
            r#type[i] = buf.g1();
        }
        for i in 0..size as usize {
            let n = buf.g1() as usize;
            labels[i] = vec![0i32; n];
        }
        for i in 0..size as usize {
            for j in 0..labels[i].len() {
                labels[i][j] = buf.g1();
            }
        }
        AnimBase { id, size, r#type, labels }
    }
}
