//! World tick, entities (Player/Npc), entity-update protocol
//! builders, and the RuneScript runtime (mirrors the Engine-TS
//! reference `src/engine`; Engine2007/Engine-TS are reference only).

pub mod entity;
pub mod info;
pub mod script;
pub mod world;

pub use world::World;
