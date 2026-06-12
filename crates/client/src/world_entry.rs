// @ObfuscatedName("c") — jag::oldscape::GameWorld::WorldEntry.
//
// One entry in the world-switch screen list. Populated by
// TitleScreen.listFetch from the worldlist URL; the world-switch
// renderer iterates the slots, plots the country flag + members star,
// and routes click events back to a host/port.

#![allow(dead_code)]

use std::sync::Mutex;

#[derive(Debug, Clone, Default)]
pub struct WorldEntry {
    // @ObfuscatedName("c.r")
    pub id: i32,
    // @ObfuscatedName("c.d")
    pub players: i32,
    // @ObfuscatedName("c.l")
    pub host: String,
    // @ObfuscatedName("c.m")
    pub country: i32,
    // @ObfuscatedName("c.c")
    pub index: i32,
    // @ObfuscatedName("c.n")
    pub members: bool,
}

impl WorldEntry {
    pub fn new(id: i32, players: i32, host: String, country: i32, index: i32, members: bool) -> Self {
        Self { id, players, host, country, index, members }
    }
}

// @ObfuscatedName("GameWorld") — TitleScreen's static-list slots.
// Java holds these as `WorldEntry[] list`, `int num`, plus sort state.
pub struct GameWorld {
    // @ObfuscatedName("GameWorld.r")
    pub num: i32,
    // @ObfuscatedName("GameWorld.d")
    pub list: Vec<WorldEntry>,
    // @ObfuscatedName("GameWorld.l") — primary sort key (id / players /
    // index / members). Index into the per-column header. Default
    // [0, 1, 2, 3] = ascending id, then players, etc.
    pub ordering: [i32; 4],
    // @ObfuscatedName("GameWorld.m") — sort direction per column,
    // +1 = ascending, -1 = descending.
    pub dirs: [i32; 4],
    // @ObfuscatedName("GameWorld.c") — id of the last world the user
    // picked (used to pre-highlight on next visit). -1 = never picked.
    pub sl_last_world: i32,
    // @ObfuscatedName("GameWorld.n") — true when the world-switch
    // sub-screen is being shown over the title screen.
    pub switch_screen: bool,
}

impl Default for GameWorld {
    fn default() -> Self {
        Self {
            num: 0,
            list: Vec::new(),
            ordering: [0, 1, 2, 3],
            dirs: [1, 1, 1, 1],
            sl_last_world: -1,
            switch_screen: false,
        }
    }
}

pub static WORLDS: Mutex<GameWorld> = Mutex::new(GameWorld {
    num: 0,
    list: Vec::new(),
    ordering: [0, 1, 2, 3],
    dirs: [1, 1, 1, 1],
    sl_last_world: -1,
    switch_screen: false,
});
