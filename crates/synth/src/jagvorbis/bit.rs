//! `jagex3.sound.JagVorbis::BitUnpacker` — LSB-first bit reader.
//!
//! Java uses a static buffer; here we keep it instance-based so multiple readers can
//! coexist (e.g., header decode + packet decode). All other Vorbis components take a
//! `&mut BitReader` to pull bits.

pub struct BitReader<'a> {
    pub data: &'a [u8],
    pub byte_pos: usize,
    pub bit_pos: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, byte_pos: 0, bit_pos: 0 }
    }

    pub fn read_bit(&mut self) -> i32 {
        let v = ((self.data[self.byte_pos] as i8) >> self.bit_pos) & 0x1;
        self.bit_pos += 1;
        self.byte_pos += self.bit_pos >> 3;
        self.bit_pos &= 0x7;
        v as i32
    }

    /// Read `n` bits as an unsigned integer (returned as i32 for Java parity).
    pub fn read_bits(&mut self, mut n: i32) -> i32 {
        let mut result = 0i32;
        let mut shift = 0i32;
        while n >= 8 - self.bit_pos as i32 {
            let take = 8 - self.bit_pos as i32;
            let mask = (1i32 << take) - 1;
            result |= ((self.data[self.byte_pos] as i8 as i32) >> self.bit_pos & mask) << shift;
            self.bit_pos = 0;
            self.byte_pos += 1;
            shift += take;
            n -= take;
        }
        if n > 0 {
            let mask = (1i32 << n) - 1;
            result |= ((self.data[self.byte_pos] as i8 as i32) >> self.bit_pos & mask) << shift;
            self.bit_pos += n as usize;
        }
        result
    }
}

/// Java `MathTool.bitsRequired(n)` — `ceil(log2(n + 1))`. Returns 0 for n ≤ 0.
pub fn bits_required(n: i32) -> i32 {
    if n <= 0 { 0 } else { 32 - (n as u32).leading_zeros() as i32 }
}

/// Java `JagVorbis.float32Unpack` — pack/unpack of a custom float-32 representation
/// (1 sign bit, 10 exponent bits, 21 mantissa bits).
pub fn float32_unpack(packed: i32) -> f32 {
    let mut mant = packed & 0x1F_FFFF;
    let sign = packed & i32::MIN;
    let exp = (packed >> 21) & 0x3FF;
    if sign != 0 {
        mant = -mant;
    }
    (mant as f64 * 2f64.powi(exp - 788)) as f32
}
