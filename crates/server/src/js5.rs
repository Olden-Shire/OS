//! JS5 (on-demand cache) service — mirrors the Engine2007 reference
//! Js5.ts + Js5GroupResponseEncoder.ts. Serves compressed groups from
//! the local cache with the 512-byte 0xFF block framing, and the
//! 255/255 master checksum table.

use cache::Cache;
use io::packet::Packet;

/// Client→server JS5 request opcodes (4 bytes each: op + archive +
/// group16).
pub const JS5_PREFETCH: u8 = 0;
pub const JS5_URGENT: u8 = 1;
pub const JS5_PRIORITY_HIGH: u8 = 2;
pub const JS5_PRIORITY_LOW: u8 = 3;
pub const JS5_XOR: u8 = 4;

/// Build the 255/255 checksum table: per archive, the CRC32 of its
/// master index group (the 255/255 response). The client decodes this
/// in `read_master_index_at` as an uncompressed group container —
/// `compression_type(1) + length(4)` header, then `crc(4) + version(4)`
/// per archive at offset `archive*8 + 5`. So we emit a complete
/// uncompressed container here; `group_response` sends it verbatim.
pub fn build_checksum_table(cache: &Cache, archives: u8) -> Vec<u8> {
    // Payload: per-archive (index crc, index revision). These are the
    // exact values the client validates the downloaded indexes against
    // (Js5Index.crc = crc32 of the same idx255 bytes the client later
    // fetches for archive X; revision is the index version number).
    let mut payload = Packet::new(archives as usize * 8);
    for archive in 0..archives {
        let index = cache.index(archive);
        payload.p4(index.crc as i32);
        payload.p4(index.revision);
    }
    let payload_len = payload.pos;

    // Wrap as an uncompressed JS5 container: compression_type 0 + the
    // 4-byte payload length, then the payload.
    let mut p = Packet::new(payload_len + 5);
    p.p1(0);
    p.p4(payload_len as i32);
    p.pdata(&payload.data, 0, payload_len);
    let mut data = p.data;
    data.truncate(p.pos);
    data
}

/// Frame one group response: archive u8, group u16, then the
/// compressed container re-blocked with a 0xFF marker every 512 bytes
/// of the response stream. The 255/255 table is sent raw.
pub fn group_response(archive: u8, group: u16, data: &[u8]) -> Vec<u8> {
    let mut buf = Packet::new(data.len() + 8 + data.len() / 511 + 8);
    buf.p1(archive as i32);
    buf.p2(group as i32);

    if archive == 255 && group == 255 {
        buf.pdata(data, 0, data.len());
    } else {
        let compression = data[0] as i32;
        let length = ((data[1] as i32) << 24)
            | ((data[2] as i32) << 16)
            | ((data[3] as i32) << 8)
            | (data[4] as i32);
        let real_length = if compression != 0 { length + 4 } else { length };

        buf.p1(compression);
        buf.p4(length);

        for i in 5..(real_length + 5) as usize {
            if buf.pos % 512 == 0 {
                buf.p1(0xff);
            }
            buf.p1(data[i] as i32);
        }
    }

    let mut out = buf.data;
    out.truncate(buf.pos);
    out
}
