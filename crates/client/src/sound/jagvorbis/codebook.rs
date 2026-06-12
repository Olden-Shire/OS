// @ObfuscatedName("CodeBook") — jag::oldscape::sound::CodeBook
// Vorbis I codebook (Huffman tree + optional VQ lookup).

#![allow(dead_code)]

use super::bit::{bits_required, float32_unpack, BitReader};

pub struct CodeBook {
    pub dimensions: i32,
    pub entries: i32,
    pub lengths: Vec<i32>,
    pub multiplicands: Vec<i32>,
    pub vq_lookup: Vec<Vec<f32>>,
    pub huffman_tree: Vec<i32>,
}

impl CodeBook {
    pub fn decode(br: &mut BitReader<'_>) -> Self {
        br.read_bits(24);
        let dimensions = br.read_bits(16);
        let entries = br.read_bits(24);
        let mut lengths = vec![0i32; entries as usize];
        let ordered = br.read_bit() != 0;
        if ordered {
            let mut current_entry = 0i32;
            let mut current_length = br.read_bits(5) + 1;
            while current_entry < entries {
                let n = br.read_bits(bits_required(entries - current_entry - 1));
                for _ in 0..n {
                    lengths[current_entry as usize] = current_length;
                    current_entry += 1;
                }
                current_length += 1;
            }
        } else {
            let present = br.read_bit() != 0;
            for v in lengths.iter_mut() {
                if present && br.read_bit() == 0 {
                    *v = 0;
                } else {
                    *v = br.read_bits(5) + 1;
                }
            }
        }
        let huffman_tree = build_huffman_tree(&lengths, entries);

        let lookup_type = br.read_bits(4);
        let (multiplicands, vq_lookup) = if lookup_type > 0 {
            let min_value = float32_unpack(br.read_bits(32));
            let delta_value = float32_unpack(br.read_bits(32));
            let value_bits = br.read_bits(4) + 1;
            let sequence_p = br.read_bit() != 0;
            let lookup_values = if lookup_type == 1 {
                lookup1_values(entries, dimensions)
            } else {
                entries * dimensions
            };
            let mut multiplicands = vec![0i32; lookup_values as usize];
            for m in multiplicands.iter_mut() {
                *m = br.read_bits(value_bits);
            }
            let mut vq = vec![vec![0.0f32; dimensions as usize]; entries as usize];
            if lookup_type == 1 {
                for e in 0..entries {
                    let mut last = 0.0f32;
                    let mut idx_div = 1i32;
                    for d in 0..dimensions {
                        let m_idx = (e / idx_div) % lookup_values;
                        let v = multiplicands[m_idx as usize] as f32 * delta_value + min_value + last;
                        vq[e as usize][d as usize] = v;
                        if sequence_p {
                            last = v;
                        }
                        idx_div *= lookup_values;
                    }
                }
            } else {
                let mut idx = 0usize;
                for e in 0..entries as usize {
                    let mut last = 0.0f32;
                    for d in 0..dimensions as usize {
                        let v = multiplicands[idx] as f32 * delta_value + min_value + last;
                        vq[e][d] = v;
                        if sequence_p {
                            last = v;
                        }
                        idx += 1;
                    }
                }
            }
            (multiplicands, vq)
        } else {
            (Vec::new(), Vec::new())
        };

        Self { dimensions, entries, lengths, multiplicands, vq_lookup, huffman_tree }
    }

    pub fn decode_scalar(&self, br: &mut BitReader<'_>) -> i32 {
        let mut idx = 0usize;
        while self.huffman_tree[idx] >= 0 {
            if br.read_bit() == 0 {
                idx += 1;
            } else {
                idx = self.huffman_tree[idx] as usize;
            }
        }
        !self.huffman_tree[idx]
    }

    pub fn decode_vq(&self, br: &mut BitReader<'_>) -> &[f32] {
        let entry = self.decode_scalar(br) as usize;
        &self.vq_lookup[entry]
    }
}

fn lookup1_values(entries: i32, dimensions: i32) -> i32 {
    let mut r = (entries as f64).powf(1.0 / dimensions as f64) as i32 + 1;
    loop {
        let mut base = r;
        let mut exp = dimensions;
        let mut acc = 1i32;
        while exp > 1 {
            if exp & 1 != 0 {
                acc = base.wrapping_mul(acc);
            }
            base = base.wrapping_mul(base);
            exp >>= 1;
        }
        let total = if exp == 1 { base.wrapping_mul(acc) } else { acc };
        if total <= entries {
            return r;
        }
        r -= 1;
    }
}

fn build_huffman_tree(lengths: &[i32], entries: i32) -> Vec<i32> {
    let mut codes = vec![0i32; entries as usize];
    let mut next_code = [0i32; 33];
    for entry in 0..entries as usize {
        let len = lengths[entry];
        if len == 0 {
            continue;
        }
        let len_mask = 1i32 << (32 - len);
        let code = next_code[len as usize];
        codes[entry] = code;
        let new_code = if (code & len_mask) == 0 {
            let next_at_len = code | len_mask;
            for shorter in (1..len).rev() {
                let other = next_code[shorter as usize];
                if code != other {
                    break;
                }
                let mask = 1i32 << (32 - shorter);
                if (other & mask) != 0 {
                    next_code[shorter as usize] = next_code[(shorter - 1) as usize];
                    break;
                }
                next_code[shorter as usize] = other | mask;
            }
            next_at_len
        } else {
            next_code[(len - 1) as usize]
        };
        next_code[len as usize] = new_code;
        for longer in (len + 1)..=32 {
            if next_code[longer as usize] == code {
                next_code[longer as usize] = new_code;
            }
        }
    }

    let mut tree = vec![0i32; 8];
    let mut tree_size = 0i32;
    for entry in 0..entries as usize {
        let len = lengths[entry];
        if len == 0 {
            continue;
        }
        let code = codes[entry];
        let mut cursor = 0usize;
        for bit_idx in 0..len {
            let mask = (i32::MIN as u32 >> bit_idx) as i32;
            if (code & mask) == 0 {
                cursor += 1;
            } else {
                if tree[cursor] == 0 {
                    tree[cursor] = tree_size;
                }
                cursor = tree[cursor] as usize;
            }
            while cursor >= tree.len() {
                let new_len = tree.len() * 2;
                tree.resize(new_len, 0);
            }
        }
        tree[cursor] = !(entry as i32);
        if (cursor as i32) >= tree_size {
            tree_size = cursor as i32 + 1;
        }
    }
    tree
}
