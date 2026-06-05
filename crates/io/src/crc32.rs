//! CRC-32 with the Jagex polynomial 0xEDB88320 (standard ISO HDLC).
//!
//! Used by `Packet::addcrc`/`checkcrc` and JS5 archive integrity checks. Matches the table-driven
//! impl in `jagex3.io.Packet` (the rev1 client's static initializer).

const POLY: u32 = 0xEDB8_8320;

static TABLE: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut r = i as u32;
        let mut bit = 0;
        while bit < 8 {
            r = if r & 1 == 1 { (r >> 1) ^ POLY } else { r >> 1 };
            bit += 1;
        }
        t[i] = r;
        i += 1;
    }
    t
};

/// Compute the CRC-32 of `src[offset..offset + len]`.
#[must_use]
pub fn checksum(src: &[u8], offset: usize, len: usize) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for &b in &src[offset..offset + len] {
        crc = (crc >> 8) ^ TABLE[((crc ^ u32::from(b)) & 0xFF) as usize];
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_hdlc_check_value() {
        // Standard ISO HDLC check value: CRC("123456789") = 0xCBF43926.
        assert_eq!(checksum(b"123456789", 0, 9), 0xCBF4_3926);
    }

    #[test]
    fn empty_input() {
        assert_eq!(checksum(&[], 0, 0), 0);
    }
}
