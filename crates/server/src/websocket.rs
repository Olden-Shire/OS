//! Minimal server-side WebSocket (RFC 6455) so the wasm client can speak
//! the JS5/game protocols from a browser — raw TCP isn't available there.
//!
//! The listener stays a plain TcpListener: Connection sniffs "GET " on the
//! first bytes (no game/js5 handshake opcode starts with 0x47) and flips
//! into WS mode — HTTP upgrade, then masked client binary frames de-frame
//! into the same inbuf the TCP path fills, and outgoing bytes wrap into
//! unmasked binary frames. Hand-rolled SHA-1/base64 keep the server
//! dependency-free.

/// Incremental state for one WebSocket connection.
#[derive(Default)]
pub struct WsState {
    /// HTTP upgrade finished; frames flow.
    pub active: bool,
    /// Unparsed wire bytes (HTTP header, then partial frames).
    pub raw: Vec<u8>,
    /// Peer sent a Close frame / protocol error.
    pub closed: bool,
}

impl WsState {
    /// Feed wire bytes. Returns de-framed binary payload bytes, and queues
    /// any handshake/pong/close replies into `out`.
    pub fn ingest(&mut self, bytes: &[u8], out: &mut Vec<u8>) -> Vec<u8> {
        self.raw.extend_from_slice(bytes);
        let mut payload = Vec::new();

        if !self.active {
            // Wait for the full HTTP request head.
            let Some(end) = find_header_end(&self.raw) else {
                return payload;
            };
            let head = String::from_utf8_lossy(&self.raw[..end]).into_owned();
            self.raw.drain(..end + 4);
            let Some(key) = header_value(&head, "sec-websocket-key") else {
                self.closed = true;
                return payload;
            };
            let accept = accept_key(key.trim());
            out.extend_from_slice(
                format!(
                    "HTTP/1.1 101 Switching Protocols\r\n\
                     Upgrade: websocket\r\n\
                     Connection: Upgrade\r\n\
                     Sec-WebSocket-Accept: {accept}\r\n\r\n"
                )
                .as_bytes(),
            );
            self.active = true;
        }

        // Frame pump.
        loop {
            let Some((opcode, body, used)) = parse_frame(&self.raw) else {
                return payload;
            };
            self.raw.drain(..used);
            match opcode {
                0x1 | 0x2 | 0x0 => payload.extend_from_slice(&body), // text/binary/continuation
                0x8 => {
                    // Close — echo and mark dead.
                    out.extend_from_slice(&frame_header(0x8, body.len()));
                    out.extend_from_slice(&body);
                    self.closed = true;
                    return payload;
                }
                0x9 => {
                    // Ping → Pong with same body.
                    out.extend_from_slice(&frame_header(0xA, body.len()));
                    out.extend_from_slice(&body);
                }
                0xA => {} // Pong — ignore.
                _ => {
                    self.closed = true;
                    return payload;
                }
            }
        }
    }

    /// Wrap outgoing bytes in a single unmasked binary frame.
    pub fn frame_out(&self, data: &[u8]) -> Vec<u8> {
        let mut f = frame_header(0x2, data.len());
        f.extend_from_slice(data);
        f
    }
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn header_value<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        if k.trim().eq_ignore_ascii_case(name) {
            Some(v.trim())
        } else {
            None
        }
    })
}

/// Parse one complete frame: (opcode, unmasked payload, bytes consumed).
fn parse_frame(buf: &[u8]) -> Option<(u8, Vec<u8>, usize)> {
    if buf.len() < 2 {
        return None;
    }
    let opcode = buf[0] & 0x0F;
    let masked = buf[1] & 0x80 != 0;
    let mut len = (buf[1] & 0x7F) as usize;
    let mut pos = 2;
    if len == 126 {
        if buf.len() < 4 {
            return None;
        }
        len = ((buf[2] as usize) << 8) | buf[3] as usize;
        pos = 4;
    } else if len == 127 {
        if buf.len() < 10 {
            return None;
        }
        len = 0;
        for &b in &buf[2..10] {
            len = (len << 8) | b as usize;
        }
        pos = 10;
    }
    let mask_len = if masked { 4 } else { 0 };
    if buf.len() < pos + mask_len + len {
        return None;
    }
    let mut body = buf[pos + mask_len..pos + mask_len + len].to_vec();
    if masked {
        let key = &buf[pos..pos + 4];
        for (i, b) in body.iter_mut().enumerate() {
            *b ^= key[i & 3];
        }
    }
    Some((opcode, body, pos + mask_len + len))
}

fn frame_header(opcode: u8, len: usize) -> Vec<u8> {
    let mut h = vec![0x80 | opcode];
    if len < 126 {
        h.push(len as u8);
    } else if len < 65536 {
        h.push(126);
        h.push((len >> 8) as u8);
        h.push(len as u8);
    } else {
        h.push(127);
        for shift in (0..8).rev() {
            h.push((len >> (shift * 8)) as u8);
        }
    }
    h
}

/// Sec-WebSocket-Accept = base64(sha1(key + RFC 6455 GUID)).
fn accept_key(key: &str) -> String {
    let mut input = key.as_bytes().to_vec();
    input.extend_from_slice(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    base64(&sha1(&input))
}

/// Plain SHA-1 (RFC 3174) — only used for the WS handshake, not security.
fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let ml = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&ml.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }

    let mut out = [0u8; 20];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ALPHABET[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc6455_example_accept_key() {
        // The handshake example from RFC 6455 §1.3.
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn masked_roundtrip() {
        // Client frame: FIN binary, masked "hello".
        let mut frame = vec![0x82, 0x85, 1, 2, 3, 4];
        for (i, b) in b"hello".iter().enumerate() {
            frame.push(b ^ [1u8, 2, 3, 4][i & 3]);
        }
        let (op, body, used) = parse_frame(&frame).unwrap();
        assert_eq!(op, 2);
        assert_eq!(body, b"hello");
        assert_eq!(used, frame.len());
    }
}
