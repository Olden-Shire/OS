//! Cache ↔ Content-shaped directory tree, CRC-identical both ways.
//!
//! Layout:
//!
//! ```text
//! content/
//!   _master/                # archive 255 — Js5Index per game archive (decompressed)
//!     _meta.json
//!     0.dat .. 15.dat
//!   anims/                  # archive 0 — multi-file groups (per-frame .dat inside group dir)
//!     _meta.json
//!     anim_0/  0.dat  1.dat  ...
//!     anim_1/  ...
//!   bases/                  # archive 1 — single-file groups
//!     _meta.json
//!     base_0.dat  base_1.dat  ...
//!   config/                 # archive 2 — typed subdirs per group
//!     _meta.json
//!     npc/  0.dat  1.dat  ...    # group 9
//!     obj/  ...                  # group 10
//!     loc/  ...                  # group 6
//!     ...
//!   interfaces/             # archive 3 — multi-file (one file per subcomponent)
//!     _meta.json
//!     if_0/  0.dat  ...
//!     ...
//!   maps/                   # archive 5 — named per region from name_hash
//!     _meta.json            # carries per-loc-file XTEA keys
//!     m40_55.dat            # terrain (decompressed)
//!     l40_55.dat            # locs (decompressed + decrypted)
//!     ...
//!   models/                 # archive 7
//!     _meta.json
//!     model_0.dat  model_1.dat  ...
//!   ...
//! ```
//!
//! ## CRC identity
//!
//! The round-trip is byte-identical at the per-group level (which is what JS5 cache sync
//! checksums). The on-disk dat2 sector layout differs from the original (we allocate
//! sequentially from sector 1), but `Cache::read_raw(archive, group)` returns the same
//! bytes after a full unpack → pack cycle.
//!
//! Two facts make this work:
//!
//! * Jagex's packer used **`Z_DEFAULT_COMPRESSION` (deflate level 6)**, not level 9 as one
//!   might assume. At level 6 with the vendored zlib (libz-sys) we reproduce all 38,561
//!   gzip groups byte-for-byte. bzip2 is already deterministic at block size 1.
//! * The gzip header bytes are Java `GZIPOutputStream` defaults (`xfl=00, OS=00`) — we
//!   build the 10-byte header by hand since flate2 emits `xfl=02, OS=0xFF`.
//!
//! XTEA-encrypted map loc files use the `[5..len-2]` decrypt range (the version trailer
//! is appended *after* encryption at pack time) — see
//! `Cache::read_group_with_key` for the read side.

pub mod manifest;
pub mod pack;
pub mod unpack;

pub use manifest::{ArchiveManifest, GroupMeta, MasterManifest};
pub use pack::{PackStats, pack};
pub use unpack::{UnpackStats, unpack};
