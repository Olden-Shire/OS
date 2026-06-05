//! XTEA block cipher, standalone form. Used to decrypt OSRS map region loc files in archive 5.
//!
//! Identical to the `tinyenc`/`tinydec` methods on [`crate::Packet`] but operates on a plain
//! `&mut [u8]` slice. Only complete 8-byte blocks are processed — any trailing 1..=7 bytes
//! are left as-is, matching `jagex3.io.Packet::tinydec`.

const DELTA: i32 = -1_640_531_527; // 0x9E3779B9 as i32
const SUM_32: i32 = -957_401_312; // DELTA * 32 == 0xC6EF3720 as i32

/// Decrypt `data[start..end]` in place using the 128-bit XTEA key (4 × i32).
pub fn decrypt(data: &mut [u8], key: &[i32; 4], start: usize, end: usize) {
    let blocks = (end - start) / 8;
    for b in 0..blocks {
        let off = start + b * 8;
        let mut v0 = i32::from_be_bytes(data[off..off + 4].try_into().unwrap());
        let mut v1 = i32::from_be_bytes(data[off + 4..off + 8].try_into().unwrap());
        let mut sum = SUM_32;
        for _ in 0..32 {
            v1 = v1.wrapping_sub(
                (((v0 << 4) ^ ((v0 as u32) >> 5) as i32).wrapping_add(v0))
                    ^ key[((sum as u32) >> 11 & 3) as usize].wrapping_add(sum),
            );
            sum = sum.wrapping_sub(DELTA);
            v0 = v0.wrapping_sub(
                (((v1 << 4) ^ ((v1 as u32) >> 5) as i32).wrapping_add(v1))
                    ^ key[(sum & 3) as usize].wrapping_add(sum),
            );
        }
        data[off..off + 4].copy_from_slice(&v0.to_be_bytes());
        data[off + 4..off + 8].copy_from_slice(&v1.to_be_bytes());
    }
}

/// Encrypt `data[start..end]` in place using the 128-bit XTEA key (4 × i32).
pub fn encrypt(data: &mut [u8], key: &[i32; 4], start: usize, end: usize) {
    let blocks = (end - start) / 8;
    for b in 0..blocks {
        let off = start + b * 8;
        let mut v0 = i32::from_be_bytes(data[off..off + 4].try_into().unwrap());
        let mut v1 = i32::from_be_bytes(data[off + 4..off + 8].try_into().unwrap());
        let mut sum: i32 = 0;
        for _ in 0..32 {
            v0 = v0.wrapping_add(
                (((v1 << 4) ^ ((v1 as u32) >> 5) as i32).wrapping_add(v1))
                    ^ key[(sum & 3) as usize].wrapping_add(sum),
            );
            sum = sum.wrapping_add(DELTA);
            v1 = v1.wrapping_add(
                (((v0 << 4) ^ ((v0 as u32) >> 5) as i32).wrapping_add(v0))
                    ^ key[((sum as u32) >> 11 & 3) as usize].wrapping_add(sum),
            );
        }
        data[off..off + 4].copy_from_slice(&v0.to_be_bytes());
        data[off + 4..off + 8].copy_from_slice(&v1.to_be_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let key = [0x1234_5678i32, -0x6543_21FE, 0x4242_4242, -1];
        let mut data = b"sixteen-byte msg".to_vec();
        encrypt(&mut data, &key, 0, 16);
        assert_ne!(&data[..], b"sixteen-byte msg");
        decrypt(&mut data, &key, 0, 16);
        assert_eq!(&data[..], b"sixteen-byte msg");
    }

    #[test]
    fn partial_trailing_block_untouched() {
        // 17-byte buffer: 2 full blocks + 1 trailing byte. Trailing byte stays as 0xAB.
        let key = [1, 2, 3, 4];
        let mut data = vec![0u8; 17];
        data[16] = 0xAB;
        encrypt(&mut data, &key, 0, 17);
        assert_eq!(data[16], 0xAB);
        decrypt(&mut data, &key, 0, 17);
        assert_eq!(data, {
            let mut v = vec![0u8; 17];
            v[16] = 0xAB;
            v
        });
    }

    #[test]
    fn matches_packet_tinydec() {
        use crate::Packet;
        let key = [0x1234_5678i32, -0x6543_21FE, 0x4242_4242, -1];
        let plaintext = b"some plaintext..".to_vec(); // 16 bytes

        // Encrypt via standalone, decrypt via Packet (and vice versa) should agree.
        let mut a = plaintext.clone();
        encrypt(&mut a, &key, 0, 16);
        let mut p = Packet::from_vec(a.clone());
        p.tinydec(&key, 0, 16);
        assert_eq!(p.data, plaintext);
    }
}
