//! Byte/bit IO primitives — Packet, ISAAC, RSA, BZip2/GZip, Jagfile (mirrors Engine-TS `src/io`).
//!
//! Behavioral truth: the rev1 Java client at `src/main/java/jagex3/io/`. Names of public
//! functions/methods are kept identical to the Java source where Rust syntax allows.

pub mod bzip2;
pub mod cp1252;
pub mod crc32;
pub mod gzip;
pub mod packet;

pub use packet::Packet;
