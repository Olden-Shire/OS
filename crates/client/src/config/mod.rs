// jagex3.config package — gamepack Type classes. Each Type reads its
// records out of the `config` JS5 archive. The ObfuscatedName tags
// below are diff anchors against future-revision gamepacks — verified
// against `src/main/java/jagex3/config/<X>Type.java`'s class-level
// @ObfuscatedName, not the module name we use locally.

#![allow(dead_code)]

// @ObfuscatedName("fb")
pub mod flo_type;
// @ObfuscatedName("ec")
pub mod flu_type;
// @ObfuscatedName("fd")
pub mod idk_type;
// @ObfuscatedName("ey")
pub mod loc_type;
// @ObfuscatedName("em")
pub mod npc_type;
// @ObfuscatedName("fj")
pub mod obj_type;
// @ObfuscatedName("eo")
pub mod seq_type;
// @ObfuscatedName("eu")
pub mod spot_type;
// @ObfuscatedName("fc")
pub mod var_bit_type;
// @ObfuscatedName("fg")
pub mod varp_type;
// @ObfuscatedName("fp")
pub mod inv_type;
// @ObfuscatedName("fe")
pub mod enum_type;
// @ObfuscatedName("eg") — lives in `jagex3.config.iftype.IfType`.
pub mod if_type;
// @ObfuscatedName("cm") — lives in `jagex3.var.VarCache`, not jagex3.config.
pub mod var_cache;
// @ObfuscatedName("el") — ServerActive bit-field helpers.
pub mod server_active;
