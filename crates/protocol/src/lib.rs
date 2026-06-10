//! rev1 (2007-era OS1) game protocol: server→client message builders
//! and client→server packet metadata. Opcode + size tables mirror the
//! Engine2007 reference repositories (src/network/os1/{server,client});
//! wire formats are verified against the rev1 client decode in
//! crates/client.

pub mod client;
pub mod server;

pub use server::{ServerPacket, SizeKind};
