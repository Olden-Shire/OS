//! Per-scope file extensions for the on-disk Content tree.
//!
//! Only scopes where the raw cache bytes ARE the conventional format get a typed
//! extension. Things that need format-level decode/encode (Jagex MIDI → standard MIDI,
//! Jagex sprites → PNG, custom vorbis variant, etc.) stay `.dat` until those codecs land
//! — using a misleading extension would imply the file is in a standard format when it
//! isn't.

/// Extension for a single-file group's payload, given its archive and a peek at the bytes.
/// `payload` is the *raw cache bytes* (post-decompression, pre-codec).
///
/// Archives whose raw bytes need a format-aware codec to produce a standard file (songs,
/// jingles, sprites, …) carry their *standardized* extension here so the on-disk file
/// reflects what's actually written. The unpack pipeline applies the codec; pack reverses
/// it for CRC-identical repack.
#[must_use]
pub fn single_file_ext(archive: u8, payload: &[u8]) -> &'static str {
    match archive {
        6 => "mid",     // songs — decoded via io::midi to standard MIDI
        7 => "ob2",     // models — raw cache bytes are the .ob2 format
        10 => sniff_binary(payload),
        11 => "mid",    // jingles — same codec as songs
        _ => "dat",
    }
}

/// `true` if a single-file scope requires the Jagex MIDI codec to convert between raw
/// cache bytes and on-disk standard MIDI.
#[must_use]
pub const fn is_midi_archive(archive: u8) -> bool {
    matches!(archive, 6 | 11)
}

/// Extension for a file *inside* a multi-file group directory (e.g. anim frames).
#[must_use]
pub fn multi_file_inner_ext(archive: u8, _group_id: u32) -> &'static str {
    match archive {
        0 => "anim",
        _ => "dat",
    }
}

fn sniff_binary(bytes: &[u8]) -> &'static str {
    // JFIF / EXIF JPEG: starts with FF D8 FF E0 (JFIF) or FF D8 FF E1 (EXIF).
    if bytes.len() >= 4 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return "jpg";
    }
    "dat"
}
