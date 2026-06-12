//! `jagex3.sound.Residue` — Vorbis I residue (types 0/1, no type 2 used in JagVorbis).

use super::bit::BitReader;
use super::codebook::CodeBook;

pub struct Residue {
    pub kind: i32,
    pub begin: i32,
    pub end: i32,
    pub partition_size: i32,
    pub classifications: i32,
    pub classbook: i32,
    pub residue_books: Vec<i32>,
}

impl Residue {
    pub fn decode(br: &mut BitReader<'_>) -> Self {
        let kind = br.read_bits(16);
        let begin = br.read_bits(24);
        let end = br.read_bits(24);
        let partition_size = br.read_bits(24) + 1;
        let classifications = br.read_bits(6) + 1;
        let classbook = br.read_bits(8);

        let mut cascade = vec![0i32; classifications as usize];
        for c in cascade.iter_mut() {
            let low = br.read_bits(3);
            let high = if br.read_bit() != 0 { br.read_bits(5) } else { 0 };
            *c = (high << 3) | low;
        }
        let mut residue_books = vec![0i32; (classifications * 8) as usize];
        for i in 0..(classifications * 8) as usize {
            residue_books[i] = if (cascade[i >> 3] & (1 << (i & 0x7))) == 0 {
                -1
            } else {
                br.read_bits(8)
            };
        }
        Self { kind, begin, end, partition_size, classifications, classbook, residue_books }
    }

    pub fn packet_decode(
        &self,
        br: &mut BitReader<'_>,
        codebooks: &[CodeBook],
        out: &mut [f32],
        half_block: usize,
        skip: bool,
    ) {
        for s in &mut out[..half_block] {
            *s = 0.0;
        }
        if skip {
            return;
        }
        let class_dims = codebooks[self.classbook as usize].dimensions;
        let span = self.end - self.begin;
        let partitions = span / self.partition_size;
        let mut classifications = vec![0i32; partitions as usize];
        for pass in 0..8i32 {
            let mut p = 0i32;
            while p < partitions {
                if pass == 0 {
                    let mut scalar = codebooks[self.classbook as usize].decode_scalar(br);
                    for d in (0..class_dims).rev() {
                        if p + d < partitions {
                            classifications[(p + d) as usize] = scalar % self.classifications;
                        }
                        scalar /= self.classifications;
                    }
                }
                for _ in 0..class_dims {
                    let cls = classifications[p as usize];
                    let book_idx = self.residue_books[(cls * 8 + pass) as usize];
                    if book_idx >= 0 {
                        let base = (self.partition_size * p + self.begin) as usize;
                        let book = &codebooks[book_idx as usize];
                        let dim = book.dimensions as usize;
                        if self.kind == 0 {
                            let step = (self.partition_size as usize) / dim;
                            for s in 0..step {
                                let vq = book.decode_vq(br);
                                for d in 0..dim {
                                    out[step * d + base + s] += vq[d];
                                }
                            }
                        } else {
                            let mut idx = 0usize;
                            while idx < self.partition_size as usize {
                                let vq = book.decode_vq(br);
                                for d in 0..dim {
                                    out[base + idx] += vq[d];
                                    idx += 1;
                                }
                            }
                        }
                    }
                    p += 1;
                    if p >= partitions {
                        break;
                    }
                }
            }
        }
    }
}
