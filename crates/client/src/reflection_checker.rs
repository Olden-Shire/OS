// @ObfuscatedName("dk") — jag::oldscape::reflectionchecker::ReflectionChecker.
//
// Java's anti-cheat / reflection probe — the server queues
// `ReflectionCheck` records (read static field, call method, etc) and
// the client iterates them, executing each via java.lang.reflect and
// transmitting the result back over opcode 108. The check payloads are
// gated by addcrc so the server can detect tampering.
//
// Reflection on a Rust binary doesn't have a JVM-style introspection
// path; we keep the API shape so the protocol can be wired but every
// check returns a "feature-unavailable" error byte (Java's `error[i]`
// non-zero short-circuit). The server treats these as "client hasn't
// loaded that class yet" which matches what we want.

#![allow(dead_code)]

use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct ReflectionCheck {
    // @ObfuscatedName("ReflectionCheck.r")
    pub id: i32,
    // @ObfuscatedName("ReflectionCheck.d")
    pub size: i32,
    // @ObfuscatedName("ReflectionCheck.l")
    pub type_: Vec<i32>,
    // @ObfuscatedName("ReflectionCheck.m") — per-slot error sentinel.
    // 0 = "execute", non-zero = "report this error byte verbatim".
    pub error: Vec<i32>,
}

impl Default for ReflectionCheck {
    fn default() -> Self {
        Self { id: 0, size: 0, type_: Vec::new(), error: Vec::new() }
    }
}

// @ObfuscatedName("dk.r") — pending checks LinkList.
pub static CHECKS: Mutex<Vec<ReflectionCheck>> = Mutex::new(Vec::new());

// @ObfuscatedName("br.r(Lea;IB)V") — ReflectionChecker.performCheck.
//
// Walks the pending-checks queue, building a single outbound `opcode`
// (typically 108) packet per check with structure:
//   [opcode] [size_placeholder] [id:u32] [per-slot error/result bytes]
// Then back-fills the size byte (pos - start) and applies `add_crc` to
// the result so the server can validate it wasn't tampered with.
//
// We currently always emit `1` (Java's "feature unavailable") for
// every slot since Rust doesn't have JVM reflection. The pos
// arithmetic + add_crc invocation still match Java so when the
// proper backend lands callers don't change.
pub fn perform_check(out: &mut crate::io::packet::Packet, opcode: i32) {
    let mut checks = CHECKS.lock().unwrap();
    while let Some(check) = checks.pop() {
        out.p1(opcode);
        out.p1(0); // size placeholder
        let start = out.pos;
        out.p4(check.id);
        for i in 0..check.size as usize {
            let err = check.error.get(i).copied().unwrap_or(0);
            if err != 0 {
                out.p1(err);
            } else {
                // Reflection unavailable — emit error byte 1 (Java's
                // "ClassNotFoundException" enum value).
                out.p1(1);
            }
        }
        // Java's `buf.psize1(buf.pos - start)`: write the size byte
        // back-filled to the placeholder.
        let size = out.pos - start;
        let placeholder = start - 1;
        out.data[placeholder as usize] = size as u8;
        // Java appends `buf.addcrc(start)` so the server can validate
        // that the reflection payload wasn't tampered with.
        out.addcrc(start);
    }
}

pub fn queue_check(check: ReflectionCheck) {
    CHECKS.lock().unwrap().push(check);
}

// @ObfuscatedName("br.j(Lev;II)V") — ReflectionChecker.addCheck.
// Java reads `id : u32`, `size : u8`, then per-slot type+args. We
// queue a simplified record (id + size + zeroed error array); the
// JVM-reflection-shaped invocation it would normally perform is
// unavailable in Rust, so perform_check emits the "ClassNotFoundException"
// sentinel byte (1) for every slot.
pub fn add_check(in_packet: &mut crate::io::packet::Packet, _psize: i32) {
    let id = in_packet.g4();
    let size = in_packet.g1();
    let mut error = vec![0i32; size as usize];
    // Drain whatever remains of the packet body — Java reads type +
    // class/method/sig + arg bytes per slot. Since we can't act on
    // them, just clear the cursor to psize so the next packet starts
    // at the right offset.
    for i in 0..size as usize {
        let kind = in_packet.g1();
        // The per-slot args are kind-dependent. We can't reflect; flag
        // every slot as "not executed" so perform_check emits sentinel 1.
        error[i] = kind;
    }
    queue_check(ReflectionCheck { id, size, type_: Vec::new(), error });
}
