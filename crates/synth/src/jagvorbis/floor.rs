//! `jagex3.sound.Floor` — Vorbis I floor-1 decoder.

use super::bit::{bits_required, BitReader};
use super::codebook::CodeBook;

pub const RANGE_VECTOR: [i32; 4] = [256, 128, 86, 64];

pub struct Floor {
    pub xlist: Vec<i32>,
    pub floor1_multiplier: i32,
    pub partition_class_list: Vec<i32>,
    pub class_dimensions: Vec<i32>,
    pub class_subclasses: Vec<i32>,
    pub class_masterbooks: Vec<i32>,
    pub subclass_books: Vec<Vec<i32>>,
}

/// Per-decode scratch — Java keeps these as static fields shared across all floors. We
/// pass them by reference so the decoder is reentrant.
#[derive(Default)]
pub struct FloorScratch {
    pub sorted_x: Vec<i32>,
    pub post: Vec<i32>,
    pub step_flags: Vec<bool>,
}

impl Floor {
    pub fn decode(br: &mut BitReader<'_>) -> Self {
        let floor_type = br.read_bits(16);
        assert_eq!(floor_type, 1, "only floor type 1 supported");
        let partitions = br.read_bits(5);
        let mut max_class = 0i32;
        let mut partition_class_list = vec![0i32; partitions as usize];
        for p in partition_class_list.iter_mut() {
            *p = br.read_bits(4);
            if *p >= max_class {
                max_class = *p + 1;
            }
        }
        let mut class_dimensions = vec![0i32; max_class as usize];
        let mut class_subclasses = vec![0i32; max_class as usize];
        let mut class_masterbooks = vec![0i32; max_class as usize];
        let mut subclass_books: Vec<Vec<i32>> = Vec::with_capacity(max_class as usize);
        for c in 0..max_class as usize {
            class_dimensions[c] = br.read_bits(3) + 1;
            let subs = br.read_bits(2);
            class_subclasses[c] = subs;
            if subs != 0 {
                class_masterbooks[c] = br.read_bits(8);
            }
            let count = 1i32 << subs;
            let mut subclass = vec![0i32; count as usize];
            for sb in subclass.iter_mut() {
                *sb = br.read_bits(8) - 1;
            }
            subclass_books.push(subclass);
        }
        let floor1_multiplier = br.read_bits(2) + 1;
        let rangebits = br.read_bits(4);

        let mut values = 2i32;
        for &p in &partition_class_list {
            values += class_dimensions[p as usize];
        }
        let mut xlist = vec![0i32; values as usize];
        xlist[1] = 1i32 << rangebits;
        let mut idx = 2usize;
        for &p in &partition_class_list {
            for _ in 0..class_dimensions[p as usize] {
                xlist[idx] = br.read_bits(rangebits);
                idx += 1;
            }
        }
        Self {
            xlist,
            floor1_multiplier,
            partition_class_list,
            class_dimensions,
            class_subclasses,
            class_masterbooks,
            subclass_books,
        }
    }

    /// Returns Some(()) if floor is non-zero (caller should run synth_mul); None if zero.
    pub fn packet_decode(
        &self,
        br: &mut BitReader<'_>,
        codebooks: &[CodeBook],
        scratch: &mut FloorScratch,
    ) -> bool {
        if br.read_bit() == 0 {
            return false;
        }
        let n = self.xlist.len();
        if scratch.sorted_x.len() < n {
            scratch.sorted_x.resize(n, 0);
            scratch.post.resize(n, 0);
            scratch.step_flags.resize(n, false);
        }
        for i in 0..n {
            scratch.sorted_x[i] = self.xlist[i];
        }
        let range = RANGE_VECTOR[(self.floor1_multiplier - 1) as usize];
        let bits = bits_required(range - 1);
        scratch.post[0] = br.read_bits(bits);
        scratch.post[1] = br.read_bits(bits);
        let mut cursor = 2usize;
        for &p in &self.partition_class_list {
            let pc = p as usize;
            let dim = self.class_dimensions[pc];
            let subs = self.class_subclasses[pc];
            let mask = (1i32 << subs) - 1;
            let mut master = 0i32;
            if subs > 0 {
                master = codebooks[self.class_masterbooks[pc] as usize].decode_scalar(br);
            }
            for _ in 0..dim {
                let book = self.subclass_books[pc][(master & mask) as usize];
                master = ((master as u32) >> subs) as i32;
                scratch.post[cursor] = if book >= 0 {
                    codebooks[book as usize].decode_scalar(br)
                } else {
                    0
                };
                cursor += 1;
            }
        }
        true
    }

    /// Apply the decoded floor to `out` in-place. `n` is the half-block size.
    pub fn synth_mul(&self, out: &mut [f32], n: usize, scratch: &mut FloorScratch) {
        let xlen = self.xlist.len();
        let range = RANGE_VECTOR[(self.floor1_multiplier - 1) as usize];
        scratch.step_flags[0] = true;
        scratch.step_flags[1] = true;
        for i in 2..xlen {
            let lo = low_neighbour(&scratch.sorted_x, i);
            let hi = high_neighbour(&scratch.sorted_x, i);
            let predicted = render_point(
                scratch.sorted_x[lo],
                scratch.post[lo],
                scratch.sorted_x[hi],
                scratch.post[hi],
                scratch.sorted_x[i],
            );
            let val = scratch.post[i];
            let room1 = range - predicted;
            let bound = if room1 < predicted { room1 } else { predicted } << 1;
            if val == 0 {
                scratch.step_flags[i] = false;
                scratch.post[i] = predicted;
            } else {
                scratch.step_flags[lo] = true;
                scratch.step_flags[hi] = true;
                scratch.step_flags[i] = true;
                scratch.post[i] = if val >= bound {
                    if room1 > predicted { val - predicted + predicted } else { predicted - val + room1 - 1 }
                } else if val & 1 == 0 {
                    val / 2 + predicted
                } else {
                    predicted - (val + 1) / 2
                };
            }
        }
        quicksort(scratch, 0, xlen as i32 - 1);

        let mut last_x = 0i32;
        let mut last_y = scratch.post[0] * self.floor1_multiplier;
        for i in 1..xlen {
            if scratch.step_flags[i] {
                let x = scratch.sorted_x[i];
                let y = scratch.post[i] * self.floor1_multiplier;
                render_line(last_x, last_y, x, y, out, n as i32);
                if x >= n as i32 {
                    return;
                }
                last_x = x;
                last_y = y;
            }
        }
        let tail = INVERSE_DB_TABLE[last_y as usize];
        for s in &mut out[last_x as usize..n] {
            *s *= tail;
        }
    }
}

fn low_neighbour(a: &[i32], i: usize) -> usize {
    let target = a[i];
    let mut best_idx = 0usize;
    let mut best_val = i32::MIN;
    for j in 0..i {
        let v = a[j];
        if v < target && v > best_val {
            best_idx = j;
            best_val = v;
        }
    }
    best_idx
}

fn high_neighbour(a: &[i32], i: usize) -> usize {
    let target = a[i];
    let mut best_idx = 0usize;
    let mut best_val = i32::MAX;
    for j in 0..i {
        let v = a[j];
        if v > target && v < best_val {
            best_idx = j;
            best_val = v;
        }
    }
    best_idx
}

fn render_point(x0: i32, y0: i32, x1: i32, y1: i32, x: i32) -> i32 {
    let dy = y1 - y0;
    let dx = x1 - x0;
    let ady = dy.abs();
    let off = (x - x0) * ady;
    let q = off / dx;
    if dy < 0 { y0 - q } else { y0 + q }
}

fn render_line(x0: i32, y0: i32, x1: i32, y1: i32, out: &mut [f32], max_x: i32) {
    let dy = y1 - y0;
    let adx = x1 - x0;
    let mut ady = dy.abs();
    let base = dy / adx;
    let mut y = y0;
    let mut err = 0i32;
    let sy = if dy < 0 { base - 1 } else { base + 1 };
    ady -= base.abs() * adx;
    out[x0 as usize] *= INVERSE_DB_TABLE[y0 as usize];
    let limit = x1.min(max_x);
    for x in (x0 + 1)..limit {
        err += ady;
        if err >= adx {
            err -= adx;
            y += sy;
        } else {
            y += base;
        }
        out[x as usize] *= INVERSE_DB_TABLE[y as usize];
    }
}

fn quicksort(scratch: &mut FloorScratch, lo: i32, hi: i32) {
    if lo >= hi {
        return;
    }
    let mut store = lo;
    let pivot_x = scratch.sorted_x[lo as usize];
    let pivot_y = scratch.post[lo as usize];
    let pivot_step = scratch.step_flags[lo as usize];
    for i in (lo + 1)..=hi {
        let xi = scratch.sorted_x[i as usize];
        if xi < pivot_x {
            scratch.sorted_x[store as usize] = xi;
            scratch.post[store as usize] = scratch.post[i as usize];
            scratch.step_flags[store as usize] = scratch.step_flags[i as usize];
            store += 1;
            scratch.sorted_x[i as usize] = scratch.sorted_x[store as usize];
            scratch.post[i as usize] = scratch.post[store as usize];
            scratch.step_flags[i as usize] = scratch.step_flags[store as usize];
        }
    }
    scratch.sorted_x[store as usize] = pivot_x;
    scratch.post[store as usize] = pivot_y;
    scratch.step_flags[store as usize] = pivot_step;
    quicksort(scratch, lo, store - 1);
    quicksort(scratch, store + 1, hi);
}

// 256-entry inverse-dB table from Vorbis I spec / Java source.
const INVERSE_DB_TABLE: [f32; 256] = [
    1.0649863e-07, 1.1341951e-07, 1.2079015e-07, 1.2863978e-07,
    1.3699951e-07, 1.4590251e-07, 1.5538408e-07, 1.6548181e-07,
    1.7623575e-07, 1.8768855e-07, 1.9988561e-07, 2.1287530e-07,
    2.2670913e-07, 2.4144197e-07, 2.5713223e-07, 2.7384213e-07,
    2.9163793e-07, 3.1059021e-07, 3.3077411e-07, 3.5226968e-07,
    3.7516214e-07, 3.9954229e-07, 4.2550680e-07, 4.5315863e-07,
    4.8260743e-07, 5.1396998e-07, 5.4737065e-07, 5.8294187e-07,
    6.2082472e-07, 6.6116941e-07, 7.0413592e-07, 7.4989464e-07,
    7.9862701e-07, 8.5052630e-07, 9.0579828e-07, 9.6466216e-07,
    1.0273513e-06, 1.0941144e-06, 1.1652161e-06, 1.2409384e-06,
    1.3215816e-06, 1.4074654e-06, 1.4989305e-06, 1.5963394e-06,
    1.7000785e-06, 1.8105592e-06, 1.9282195e-06, 2.0535261e-06,
    2.1869758e-06, 2.3290978e-06, 2.4804557e-06, 2.6416497e-06,
    2.8133190e-06, 2.9961443e-06, 3.1908506e-06, 3.3982101e-06,
    3.6190449e-06, 3.8542308e-06, 4.1047004e-06, 4.3714470e-06,
    4.6555282e-06, 4.9580707e-06, 5.2802740e-06, 5.6234160e-06,
    5.9888572e-06, 6.3780469e-06, 6.7925283e-06, 7.2339451e-06,
    7.7040476e-06, 8.2047000e-06, 8.7378876e-06, 9.3057248e-06,
    9.9104632e-06, 1.0554501e-05, 1.1240392e-05, 1.1970856e-05,
    1.2748789e-05, 1.3577278e-05, 1.4459606e-05, 1.5399272e-05,
    1.6400004e-05, 1.7465768e-05, 1.8600792e-05, 1.9809576e-05,
    2.1096914e-05, 2.2467911e-05, 2.3928002e-05, 2.5482978e-05,
    2.7139006e-05, 2.8902651e-05, 3.0780908e-05, 3.2781225e-05,
    3.4911534e-05, 3.7180282e-05, 3.9596466e-05, 4.2169667e-05,
    4.4910090e-05, 4.7828601e-05, 5.0936773e-05, 5.4246931e-05,
    5.7772202e-05, 6.1526565e-05, 6.5524908e-05, 6.9783085e-05,
    7.4317983e-05, 7.9147585e-05, 8.4291040e-05, 8.9768747e-05,
    9.5602426e-05, 0.00010181521, 0.00010843174, 0.00011547824,
    0.00012298267, 0.00013097477, 0.00013948625, 0.00014855085,
    0.00015820453, 0.00016848555, 0.00017943469, 0.00019109536,
    0.00020351382, 0.00021673929, 0.00023082423, 0.00024582449,
    0.00026179955, 0.00027881276, 0.00029693158, 0.00031622787,
    0.00033677814, 0.00035866388, 0.00038197188, 0.00040679456,
    0.00043323036, 0.00046138411, 0.00049136745, 0.00052329927,
    0.00055730621, 0.00059352311, 0.00063209358, 0.00067317058,
    0.00071691700, 0.00076350630, 0.00081312324, 0.00086596457,
    0.00092223983, 0.00098217216, 0.0010459992, 0.0011139742,
    0.0011863665, 0.0012634633, 0.0013455702, 0.0014330129,
    0.0015261382, 0.0016253153, 0.0017309374, 0.0018434235,
    0.0019632195, 0.0020908006, 0.0022266726, 0.0023713743,
    0.0025254795, 0.0026895994, 0.0028643847, 0.0030505286,
    0.0032487691, 0.0034598925, 0.0036847358, 0.0039241906,
    0.0041792066, 0.0044507950, 0.0047400328, 0.0050480668,
    0.0053761186, 0.0057254891, 0.0060975636, 0.0064938176,
    0.0069158225, 0.0073652516, 0.0078438871, 0.0083536271,
    0.0088964928, 0.009474637, 0.010090352, 0.010746080,
    0.011444421, 0.012188144, 0.012980198, 0.013823725,
    0.014722068, 0.015678791, 0.016697687, 0.017782797,
    0.018938423, 0.020169149, 0.021479854, 0.022875735,
    0.024362330, 0.025945531, 0.027631618, 0.029427276,
    0.031339626, 0.033376252, 0.035545228, 0.037855157,
    0.040315199, 0.042935108, 0.045725273, 0.048696758,
    0.051861348, 0.055231591, 0.058820850, 0.062643361,
    0.066714279, 0.071049749, 0.075666962, 0.080584227,
    0.085821044, 0.091398179, 0.097337747, 0.10366330,
    0.11039993, 0.11757434, 0.12521498, 0.13335215,
    0.14201813, 0.15124727, 0.16107617, 0.17154380,
    0.18269168, 0.19456402, 0.20720788, 0.22067342,
    0.23501402, 0.25028656, 0.26655159, 0.28387361,
    0.30232132, 0.32196786, 0.34289114, 0.36517414,
    0.38890521, 0.41417847, 0.44109412, 0.46975890,
    0.50028648, 0.53279791, 0.56742212, 0.60429640,
    0.64356699, 0.68538959, 0.72993007, 0.77736504,
    0.82788260, 0.88168307, 0.9389798, 1.0,
];
