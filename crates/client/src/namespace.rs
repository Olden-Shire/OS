// @ObfuscatedName("da") — jagex3.namespace.NameSpace.
//
// Verbatim port of NameSpace.java. Pure data enum of game-id /
// ordinal pairs the launcher passes through jav_config. Each entry
// pairs a stable `ordinal` (the JS5 / loginserver-facing number) with
// the rev1 client's internal `id` slot. Pure: no Mutex, no LOADERS.

#![allow(dead_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameSpaceEntry {
    pub ordinal: i32,
    pub id: i32,
    pub name: &'static str,
}

pub const RUNESCAPE:      NameSpaceEntry = NameSpaceEntry { ordinal: 6, id: 0,  name: "" };
pub const FUNORB:         NameSpaceEntry = NameSpaceEntry { ordinal: 1, id: 1,  name: "" };
pub const WAR_OF_LEGENDS: NameSpaceEntry = NameSpaceEntry { ordinal: 7, id: 2,  name: "" };
pub const STELLAR_DAWN:   NameSpaceEntry = NameSpaceEntry { ordinal: 0, id: 3,  name: "" };
pub const EIGHT_REALMS:   NameSpaceEntry = NameSpaceEntry { ordinal: 5, id: 4,  name: "" };
pub const TRANSFORMERS:   NameSpaceEntry = NameSpaceEntry { ordinal: 3, id: 5,  name: "" };
pub const SCRATCH:        NameSpaceEntry = NameSpaceEntry { ordinal: 2, id: 6,  name: "" };
pub const LEGACY:         NameSpaceEntry = NameSpaceEntry { ordinal: 4, id: -1, name: "" };

pub const ALL: &[NameSpaceEntry] = &[
    RUNESCAPE, FUNORB, WAR_OF_LEGENDS, STELLAR_DAWN,
    EIGHT_REALMS, TRANSFORMERS, SCRATCH, LEGACY,
];

pub fn by_id(id: i32) -> Option<NameSpaceEntry> {
    ALL.iter().copied().find(|n| n.id == id)
}

pub fn by_ordinal(ordinal: i32) -> Option<NameSpaceEntry> {
    ALL.iter().copied().find(|n| n.ordinal == ordinal)
}
