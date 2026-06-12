// jagex3.io.ByteArrayPool — pooling JVM byte arrays to lessen GC pressure.
// Rust port allocates fresh Vec<u8> each call; the pool semantics aren't
// observable from the JS5 protocol side.

#![allow(dead_code)]

pub fn alloc(size: i32) -> Vec<u8> {
    vec![0u8; size.max(0) as usize]
}
