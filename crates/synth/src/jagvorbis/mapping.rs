//! `jagex3.sound.Mapping` — Vorbis I mapping (channel/submap routing).

use super::bit::BitReader;

pub struct Mapping {
    pub submaps: i32,
    pub mux: i32,
    pub submap_floor: Vec<i32>,
    pub submap_residue: Vec<i32>,
}

impl Mapping {
    pub fn decode(br: &mut BitReader<'_>) -> Self {
        br.read_bits(16); // mapping_type, discarded
        let submaps = if br.read_bit() == 0 { 1 } else { br.read_bits(4) + 1 };
        if br.read_bit() != 0 {
            br.read_bits(8);
        }
        br.read_bits(2);
        let mux = if submaps > 1 { br.read_bits(4) } else { 0 };
        let mut submap_floor = vec![0i32; submaps as usize];
        let mut submap_residue = vec![0i32; submaps as usize];
        for i in 0..submaps as usize {
            br.read_bits(8); // discard
            submap_floor[i] = br.read_bits(8);
            submap_residue[i] = br.read_bits(8);
        }
        Self { submaps, mux, submap_floor, submap_residue }
    }
}
