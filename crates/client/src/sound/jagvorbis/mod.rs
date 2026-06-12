// @ObfuscatedName("dt") — jag::oldscape::sound::JagVorbis + dependencies.
//
// The rev1 client's Vorbis storage strips standard Ogg framing and stores
// raw audio packets in a small custom container. All samples in the cache
// share a single header file (codebooks/floors/residues/mappings/modes),
// kept here in `VorbisHeaders` (group 0 / file 0 of archive 14).

#![allow(dead_code)]

pub mod bit;
pub mod codebook;
pub mod floor;
pub mod mapping;
pub mod residue;

use bit::{bits_required, BitReader};
use codebook::CodeBook;
use floor::{Floor, FloorScratch};
use mapping::Mapping;
use residue::Residue;

use crate::io::packet::Packet;
use crate::sound::wave::Wave;

/// Shared, immutable headers loaded once from the vorbis archive's group 0 / file 0.
pub struct VorbisHeaders {
    pub blocksize0: i32,
    pub blocksize1: i32,
    pub codebooks: Vec<CodeBook>,
    pub floors: Vec<Floor>,
    pub residues: Vec<Residue>,
    pub mappings: Vec<Mapping>,
    pub blockflag: Vec<bool>,
    pub mapping_indices: Vec<i32>,
    // Pre-computed IMDCT tables.
    pub imdct_prev_short: Vec<f32>,
    pub imdct_step_short: Vec<f32>,
    pub imdct_post_short: Vec<f32>,
    pub bit_reverse_short: Vec<i32>,
    pub imdct_prev_long: Vec<f32>,
    pub imdct_step_long: Vec<f32>,
    pub imdct_post_long: Vec<f32>,
    pub bit_reverse_long: Vec<i32>,
}

impl VorbisHeaders {
    pub fn parse(src: &[u8]) -> Self {
        let mut br = BitReader::new(src);
        let blocksize0 = 1i32 << br.read_bits(4);
        let blocksize1 = 1i32 << br.read_bits(4);
        let (imdct_prev_short, imdct_step_short, imdct_post_short, bit_reverse_short) =
            build_imdct_tables(blocksize0);
        let (imdct_prev_long, imdct_step_long, imdct_post_long, bit_reverse_long) =
            build_imdct_tables(blocksize1);

        let codebook_count = br.read_bits(8) + 1;
        let codebooks: Vec<CodeBook> = (0..codebook_count).map(|_| CodeBook::decode(&mut br)).collect();
        let time_count = br.read_bits(6) + 1;
        for _ in 0..time_count {
            br.read_bits(16);
        }
        let floor_count = br.read_bits(6) + 1;
        let floors: Vec<Floor> = (0..floor_count).map(|_| Floor::decode(&mut br)).collect();
        let residue_count = br.read_bits(6) + 1;
        let residues: Vec<Residue> = (0..residue_count).map(|_| Residue::decode(&mut br)).collect();
        let mapping_count = br.read_bits(6) + 1;
        let mappings: Vec<Mapping> = (0..mapping_count).map(|_| Mapping::decode(&mut br)).collect();
        let mode_count = br.read_bits(6) + 1;
        let mut blockflag = vec![false; mode_count as usize];
        let mut mapping_indices = vec![0i32; mode_count as usize];
        for i in 0..mode_count as usize {
            blockflag[i] = br.read_bit() != 0;
            br.read_bits(16); // windowtype
            br.read_bits(16); // transformtype
            mapping_indices[i] = br.read_bits(8);
        }
        Self {
            blocksize0,
            blocksize1,
            codebooks,
            floors,
            residues,
            mappings,
            blockflag,
            mapping_indices,
            imdct_prev_short,
            imdct_step_short,
            imdct_post_short,
            bit_reverse_short,
            imdct_prev_long,
            imdct_step_long,
            imdct_post_long,
            bit_reverse_long,
        }
    }
}

fn build_imdct_tables(blocksize: i32) -> (Vec<f32>, Vec<f32>, Vec<f32>, Vec<i32>) {
    let n = blocksize;
    let n2 = n >> 1;
    let n4 = n >> 2;
    let n8 = n >> 3;
    let mut prev = vec![0.0f32; n2 as usize];
    for i in 0..n4 as usize {
        prev[i * 2] = ((i as f64 * 4.0) * std::f64::consts::PI / n as f64).cos() as f32;
        prev[i * 2 + 1] = -((i as f64 * 4.0) * std::f64::consts::PI / n as f64).sin() as f32;
    }
    let mut step = vec![0.0f32; n2 as usize];
    for i in 0..n4 as usize {
        step[i * 2] =
            ((i as f64 * 2.0 + 1.0) * std::f64::consts::PI / (n as f64 * 2.0)).cos() as f32;
        step[i * 2 + 1] =
            ((i as f64 * 2.0 + 1.0) * std::f64::consts::PI / (n as f64 * 2.0)).sin() as f32;
    }
    let mut post = vec![0.0f32; n4 as usize];
    for i in 0..n8 as usize {
        post[i * 2] = ((i as f64 * 4.0 + 2.0) * std::f64::consts::PI / n as f64).cos() as f32;
        post[i * 2 + 1] =
            -((i as f64 * 4.0 + 2.0) * std::f64::consts::PI / n as f64).sin() as f32;
    }
    let bits = bits_required(n8 - 1);
    let mut bit_reverse = vec![0i32; n8 as usize];
    for i in 0..n8 as usize {
        let mut v = i as i32;
        let mut bb = bits;
        let mut r = 0i32;
        while bb > 0 {
            r = (r << 1) | (v & 0x1);
            v >>= 1;
            bb -= 1;
        }
        bit_reverse[i] = r;
    }
    (prev, step, post, bit_reverse)
}

pub struct JagVorbis {
    pub sample_rate: i32,
    pub sample_count: i32,
    pub loop_start: i32,
    pub loop_end: i32,
    pub has_loop: bool,
    pub audio_packets: Vec<Vec<u8>>,
}

impl JagVorbis {
    pub fn decode(src: &[u8]) -> Self {
        let mut p = Packet::from_vec(src.to_vec());
        let sample_rate = p.g4();
        let sample_count = p.g4();
        let loop_start = p.g4();
        let mut loop_end = p.g4();
        let mut has_loop = false;
        if loop_end < 0 {
            loop_end = !loop_end;
            has_loop = true;
        }
        let n_packets = p.g4() as usize;
        let mut audio_packets = Vec::with_capacity(n_packets);
        for _ in 0..n_packets {
            // Length-prefixed by 255-byte chunks.
            let mut len = 0i32;
            loop {
                let b = p.g1();
                len += b;
                if b < 255 {
                    break;
                }
            }
            let mut buf = vec![0u8; len as usize];
            let pos = p.pos as usize;
            buf.copy_from_slice(&p.data[pos..pos + len as usize]);
            p.pos += len as i32;
            audio_packets.push(buf);
        }
        Self { sample_rate, sample_count, loop_start, loop_end, has_loop, audio_packets }
    }

    /// Decode all packets into an 8-bit signed PCM [`Wave`]. `headers` must be the headers
    /// loaded once from the vorbis archive's group 0 / file 0.
    pub fn to_wave(&self, headers: &VorbisHeaders) -> Wave {
        let mut state = DecodeState::new(headers);
        let mut pcm = vec![0i8; self.sample_count as usize];
        let mut write_pos = 0usize;
        for packet in &self.audio_packets {
            if let Some(samples) = state.decode_packet(packet) {
                let to_take = samples.len().min(self.sample_count as usize - write_pos);
                for s in samples.iter().take(to_take) {
                    let v = (s * 128.0 + 128.0) as i32;
                    let clamped = if v & 0xFFFF_FF00u32 as i32 != 0 { !v >> 31 } else { v };
                    pcm[write_pos] = (clamped - 128) as i8;
                    write_pos += 1;
                }
            }
        }
        Wave {
            sampling_frequency: self.sample_rate,
            samples: pcm,
            loop_start_position: self.loop_start,
            loop_end_position: self.loop_end,
            loop_reversed: self.has_loop,
        }
    }
}

/// Per-stream decode state — previous window for overlap-add, IMDCT scratch.
struct DecodeState<'h> {
    h: &'h VorbisHeaders,
    work_buf: Vec<f32>,
    previous_window: Vec<f32>,
    previous_size: i32,
    previous_right_start: i32,
    previous_unused: bool,
    floor_scratch: FloorScratch,
}

impl<'h> DecodeState<'h> {
    fn new(headers: &'h VorbisHeaders) -> Self {
        Self {
            h: headers,
            work_buf: vec![0.0; headers.blocksize1 as usize],
            previous_window: vec![0.0; headers.blocksize1 as usize],
            previous_size: 0,
            previous_right_start: 0,
            previous_unused: false,
            floor_scratch: FloorScratch::default(),
        }
    }

    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
    fn decode_packet(&mut self, packet: &[u8]) -> Option<Vec<f32>> {
        let mut br = BitReader::new(packet);
        br.read_bit(); // packet type
        let mode = br.read_bits(bits_required(self.h.mapping_indices.len() as i32 - 1));
        let long_block = self.h.blockflag[mode as usize];
        let n = if long_block { self.h.blocksize1 } else { self.h.blocksize0 };

        let (prev_flag, next_flag) = if long_block {
            (br.read_bit() != 0, br.read_bit() != 0)
        } else {
            (false, false)
        };

        let n2 = n >> 1;
        let blocksize0 = self.h.blocksize0;
        let (left_window_start, left_window_end, left_n) = if long_block && !prev_flag {
            ((n >> 2) - (blocksize0 >> 2), (blocksize0 >> 2) + (n >> 2), blocksize0 >> 1)
        } else {
            (0, n2, n >> 1)
        };
        let (right_window_start, right_window_end, right_n) = if long_block && !next_flag {
            (n - (n >> 2) - (blocksize0 >> 2), (blocksize0 >> 2) + (n - (n >> 2)), blocksize0 >> 1)
        } else {
            (n2, n, n >> 1)
        };

        let mapping = &self.h.mappings[self.h.mapping_indices[mode as usize] as usize];
        let submap_idx = mapping.mux as usize;
        let floor_idx = mapping.submap_floor[submap_idx];
        let floor = &self.h.floors[floor_idx as usize];
        let floor_active = floor.packet_decode(&mut br, &self.h.codebooks, &mut self.floor_scratch);
        let unused = !floor_active;

        for submap in 0..mapping.submaps as usize {
            let residue = &self.h.residues[mapping.submap_residue[submap] as usize];
            residue.packet_decode(&mut br, &self.h.codebooks, &mut self.work_buf, (n >> 1) as usize, unused);
        }
        if floor_active {
            floor.synth_mul(&mut self.work_buf, (n >> 1) as usize, &mut self.floor_scratch);
        }
        let nu = n as usize;
        let n2u = n2 as usize;
        let n4u = (n >> 2) as usize;
        let n8u = (n >> 3) as usize;

        if !floor_active {
            for v in &mut self.work_buf[n2u..nu] {
                *v = 0.0;
            }
        } else {
            // IMDCT
            for v in &mut self.work_buf[..n2u] {
                *v *= 0.5;
            }
            for k in n2u..nu {
                self.work_buf[k] = -self.work_buf[nu - k - 1];
            }
            let prev_tbl = if long_block { &self.h.imdct_prev_long } else { &self.h.imdct_prev_short };
            let step_tbl = if long_block { &self.h.imdct_step_long } else { &self.h.imdct_step_short };
            let post_tbl = if long_block { &self.h.imdct_post_long } else { &self.h.imdct_post_short };
            let bit_reverse = if long_block { &self.h.bit_reverse_long } else { &self.h.bit_reverse_short };

            for k in 0..n4u {
                let a = self.work_buf[k * 4] - self.work_buf[nu - k * 4 - 1];
                let b = self.work_buf[k * 4 + 2] - self.work_buf[nu - k * 4 - 3];
                let c = prev_tbl[k * 2];
                let d = prev_tbl[k * 2 + 1];
                self.work_buf[nu - k * 4 - 1] = a * c - b * d;
                self.work_buf[nu - k * 4 - 3] = a * d + b * c;
            }
            for k in 0..n8u {
                let i1 = self.work_buf[k * 4 + n2u + 3];
                let i2 = self.work_buf[k * 4 + n2u + 1];
                let i3 = self.work_buf[k * 4 + 3];
                let i4 = self.work_buf[k * 4 + 1];
                self.work_buf[k * 4 + n2u + 3] = i1 + i3;
                self.work_buf[k * 4 + n2u + 1] = i2 + i4;
                let c = prev_tbl[n2u - 4 - k * 4];
                let d = prev_tbl[n2u - 3 - k * 4];
                self.work_buf[k * 4 + 3] = (i1 - i3) * c - (i2 - i4) * d;
                self.work_buf[k * 4 + 1] = (i1 - i3) * d + (i2 - i4) * c;
            }
            let total_bits = bits_required(n - 1);
            for stage in 0..(total_bits - 3) {
                let length = (n >> (stage + 2)) as usize;
                let step_size = 8usize << stage;
                for j in 0..(2usize << stage) {
                    let limit_a = nu - length * 2 * j;
                    let limit_b = nu - (j * 2 + 1) * length;
                    for k in 0..(n >> (stage + 4)) as usize {
                        let kk = k * 4;
                        let a = self.work_buf[limit_a - 1 - kk];
                        let b = self.work_buf[limit_a - 3 - kk];
                        let c = self.work_buf[limit_b - 1 - kk];
                        let d = self.work_buf[limit_b - 3 - kk];
                        self.work_buf[limit_a - 1 - kk] = a + c;
                        self.work_buf[limit_a - 3 - kk] = b + d;
                        let cc = prev_tbl[step_size * k];
                        let dd = prev_tbl[step_size * k + 1];
                        self.work_buf[limit_b - 1 - kk] = (a - c) * cc - (b - d) * dd;
                        self.work_buf[limit_b - 3 - kk] = (a - c) * dd + (b - d) * cc;
                    }
                }
            }
            for j in 1..(n8u - 1) {
                let r = bit_reverse[j] as usize;
                if j < r {
                    let j8 = j * 8;
                    let r8 = r * 8;
                    for off in [1usize, 3, 5, 7] {
                        self.work_buf.swap(j8 + off, r8 + off);
                    }
                }
            }
            for k in 0..n2u {
                self.work_buf[k] = self.work_buf[k * 2 + 1];
            }
            for k in 0..n8u {
                self.work_buf[nu - 1 - k * 2] = self.work_buf[k * 4];
                self.work_buf[nu - 2 - k * 2] = self.work_buf[k * 4 + 1];
                self.work_buf[nu - n4u - 1 - k * 2] = self.work_buf[k * 4 + 2];
                self.work_buf[nu - n4u - 2 - k * 2] = self.work_buf[k * 4 + 3];
            }
            for k in 0..n8u {
                let c = post_tbl[k * 2];
                let d = post_tbl[k * 2 + 1];
                let a = self.work_buf[k * 2 + n2u];
                let b = self.work_buf[k * 2 + n2u + 1];
                let e = self.work_buf[nu - 2 - k * 2];
                let f = self.work_buf[nu - 1 - k * 2];
                let x = (a - e) * d + (b + f) * c;
                self.work_buf[k * 2 + n2u] = (a + e + x) * 0.5;
                self.work_buf[nu - 2 - k * 2] = (a + e - x) * 0.5;
                let y = (b + f) * d - (a - e) * c;
                self.work_buf[k * 2 + n2u + 1] = (b - f + y) * 0.5;
                self.work_buf[nu - 1 - k * 2] = (-b + f + y) * 0.5;
            }
            for k in 0..n4u {
                self.work_buf[k] = step_tbl[k * 2] * self.work_buf[k * 2 + n2u]
                    + step_tbl[k * 2 + 1] * self.work_buf[k * 2 + 1 + n2u];
                self.work_buf[n2u - 1 - k] = self.work_buf[k * 2 + n2u] * step_tbl[k * 2 + 1]
                    - step_tbl[k * 2] * self.work_buf[k * 2 + 1 + n2u];
            }
            for k in 0..n4u {
                self.work_buf[nu - n4u + k] = -self.work_buf[k];
            }
            for k in 0..n4u {
                self.work_buf[k] = self.work_buf[n4u + k];
            }
            for k in 0..n4u {
                self.work_buf[n4u + k] = -self.work_buf[n4u - k - 1];
            }
            for k in 0..n4u {
                self.work_buf[n2u + k] = self.work_buf[nu - k - 1];
            }
            for win in left_window_start..left_window_end {
                let t = (((win - left_window_start) as f64 + 0.5) / left_n as f64 * 0.5 * std::f64::consts::PI).sin();
                self.work_buf[win as usize] *=
                    (t * std::f64::consts::FRAC_PI_2 * t).sin() as f32;
            }
            for win in right_window_start..right_window_end {
                let t = (((win - right_window_start) as f64 + 0.5) / right_n as f64 * 0.5
                    * std::f64::consts::PI
                    + std::f64::consts::FRAC_PI_2)
                    .sin();
                self.work_buf[win as usize] *=
                    (t * std::f64::consts::FRAC_PI_2 * t).sin() as f32;
            }
        }

        let mut out_samples: Option<Vec<f32>> = None;
        if self.previous_size > 0 {
            let out_len = ((self.previous_size + n) >> 2) as usize;
            let mut samples = vec![0.0f32; out_len];
            if !self.previous_unused {
                for k in 0..self.previous_right_start as usize {
                    let src = (self.previous_size >> 1) as usize + k;
                    samples[k] += self.previous_window[src];
                }
            }
            if !unused {
                for k in left_window_start..(n >> 1) {
                    let dst = out_len - (n >> 1) as usize + k as usize;
                    samples[dst] += self.work_buf[k as usize];
                }
            }
            out_samples = Some(samples);
        }

        std::mem::swap(&mut self.previous_window, &mut self.work_buf);
        self.previous_size = n;
        self.previous_right_start = right_window_end - (n >> 1);
        self.previous_unused = unused;
        out_samples
    }
}
