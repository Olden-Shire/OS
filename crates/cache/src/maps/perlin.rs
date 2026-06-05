//! Perlin-like noise used to compute default terrain heights for tiles that didn't carry
//! an explicit height in the map stream.
//!
//! Port of `jagex3.client.ClientBuild::{perlinNoise,interpolatedNoise,smoothNoise,noise}`
//! and the cosine LUT from `jagex3.dash3d.Pix3D` (2048 entries spanning [0, 2π), 16.16 fixed).

use std::sync::LazyLock;

static COS_TABLE: LazyLock<[i32; 2048]> = LazyLock::new(|| {
    let mut t = [0i32; 2048];
    for (i, slot) in t.iter_mut().enumerate() {
        *slot = (f64::cos(i as f64 * 0.003_067_961_5) * 65536.0) as i32;
    }
    t
});

/// Final perlin value: clamped int in 10..=60. Three-octave sum at periods 4/2/1.
#[must_use]
pub fn perlin(x: i32, z: i32) -> i32 {
    let octave_4 = interpolated_noise(x.wrapping_add(45365), z.wrapping_add(91923), 4) - 128;
    let octave_2 = (interpolated_noise(x.wrapping_add(10294), z.wrapping_add(37821), 2) - 128) >> 1;
    let octave_1 = (interpolated_noise(x, z, 1) - 128) >> 2;
    let raw = octave_4 + octave_2 + octave_1;
    let v = (raw as f64 * 0.3) as i32 + 35;
    v.clamp(10, 60)
}

fn interpolated_noise(x: i32, z: i32, period: i32) -> i32 {
    let cell_x = x / period;
    let frac_x = x & (period - 1);
    let cell_z = z / period;
    let frac_z = z & (period - 1);

    let n00 = smooth_noise(cell_x, cell_z);
    let n10 = smooth_noise(cell_x + 1, cell_z);
    let n01 = smooth_noise(cell_x, cell_z + 1);
    let n11 = smooth_noise(cell_x + 1, cell_z + 1);

    let cos = &*COS_TABLE;
    let weight_x = (65536 - cos[(frac_x * 1024 / period) as usize]) >> 1;
    let top = ((65536 - weight_x) * n00 >> 16) + (n10 * weight_x >> 16);
    let bottom = ((65536 - weight_x) * n01 >> 16) + (n11 * weight_x >> 16);

    let weight_z = (65536 - cos[(frac_z * 1024 / period) as usize]) >> 1;
    ((65536 - weight_z) * top >> 16) + (bottom * weight_z >> 16)
}

fn smooth_noise(x: i32, z: i32) -> i32 {
    let corners = noise(x - 1, z - 1) + noise(x + 1, z - 1) + noise(x - 1, z + 1) + noise(x + 1, z + 1);
    let sides = noise(x - 1, z) + noise(x + 1, z) + noise(x, z - 1) + noise(x, z + 1);
    let center = noise(x, z);
    center / 4 + corners / 16 + sides / 8
}

fn noise(x: i32, z: i32) -> i32 {
    let v = z.wrapping_mul(57).wrapping_add(x);
    let v = (v << 13) ^ v;
    let v = v
        .wrapping_mul(v)
        .wrapping_mul(15731)
        .wrapping_add(789_221)
        .wrapping_mul(v)
        .wrapping_add(1_376_312_589)
        & i32::MAX;
    (v >> 19) & 0xFF
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_deterministic_and_in_byte_range() {
        for x in 0..32 {
            for z in 0..32 {
                let n = noise(x, z);
                assert!((0..=255).contains(&n));
                assert_eq!(noise(x, z), n);
            }
        }
    }

    #[test]
    fn perlin_in_clamped_range() {
        // Sample a representative grid; result must always sit in 10..=60.
        for x in (-200..200).step_by(7) {
            for z in (-200..200).step_by(7) {
                let p = perlin(x, z);
                assert!((10..=60).contains(&p), "perlin({x},{z}) = {p}");
            }
        }
    }

    #[test]
    fn cos_table_matches_jagex_formula() {
        let cos = &*COS_TABLE;
        // cosTable[0] = cos(0) * 65536 = 65536
        assert_eq!(cos[0], 65536);
        // cosTable[512] ~ cos(π/2) * 65536 ~ 0 (a few off due to fp truncation)
        assert!(cos[512].abs() < 100);
        // cosTable[1024] = cos(π) * 65536 = -65536
        assert!((cos[1024] - -65536).abs() < 5);
    }
}
