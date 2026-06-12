// Port of jagex3.javconfig — applet jav_config.ws / start-up parameter
// enums. All three classes are pure-data lookups keyed by integer id
// or string code. No Mutex, no LOADERS — the jav_config layer just
// surfaces the launcher params to the running client.

#![allow(dead_code)]

// @ObfuscatedName("cd") — jagex3.javconfig.JavConfigParameter.
// Verbatim port of JavConfigParameter.java:6-41. The launcher writes
// these decimal IDs into the jav_config.ws file; the client reads
// them back via NameSpace lookups.
pub mod jav_config_parameter {
    pub const JS:            &str = "1";  // @ObfuscatedName("cd.r")
    pub const WORLDID:       &str = "2";  // @ObfuscatedName("cd.n")
    pub const PLUG:          &str = "3";  // @ObfuscatedName("cd.c")
    pub const LANG:          &str = "4";  // @ObfuscatedName("cd.z")
    pub const MODEWHAT:      &str = "5";  // @ObfuscatedName("cd.l")
    pub const MEMBERS:       &str = "6";  // @ObfuscatedName("cd.j")
    pub const MODEWHERE:     &str = "7";  // @ObfuscatedName("cd.d")
    pub const GAME:          &str = "8";  // @ObfuscatedName("cd.m")
    pub const WORLDLIST_URL: &str = "9";  // @ObfuscatedName("cd.g")
}

// @ObfuscatedName("bp") — jagex3.javconfig.ModeGame.
// Verbatim port of ModeGame.java:6-55. The "which game" enum chosen
// by the launcher; rev1 is always OLDSCAPE (id 5).
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeGame {
    Runescape = 0,        // @ObfuscatedName("bp.r")
    StellarDawn = 1,      // @ObfuscatedName("bp.d")
    AlternateReality = 2, // @ObfuscatedName("bp.l")
    Transformers = 3,     // @ObfuscatedName("bp.m")
    Scratch = 4,          // @ObfuscatedName("bp.c")
    Oldscape = 5,         // @ObfuscatedName("bp.n")
}

impl ModeGame {
    pub fn index(self) -> i32 { self as i32 }

    pub fn from_index(id: i32) -> Option<Self> {
        match id {
            0 => Some(Self::Runescape),
            1 => Some(Self::StellarDawn),
            2 => Some(Self::AlternateReality),
            3 => Some(Self::Transformers),
            4 => Some(Self::Scratch),
            5 => Some(Self::Oldscape),
            _ => None,
        }
    }
}

// @ObfuscatedName("be") — jagex3.javconfig.ModeWhat.
// Verbatim port of ModeWhat.java:6-47. The "which build channel"
// enum: LIVE (production), RC (release candidate), WIP (work in
// progress), BUILDLIVE (live but built from a non-trunk branch).
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeWhat {
    Live = 0,       // @ObfuscatedName("be.r")
    Rc = 1,         // @ObfuscatedName("be.l")
    Wip = 2,        // @ObfuscatedName("be.m")
    BuildLive = 3,  // @ObfuscatedName("be.d")
}

impl ModeWhat {
    pub fn id(self) -> i32 { self as i32 }

    pub fn name(self) -> &'static str {
        match self {
            Self::Live => "LIVE",
            Self::Rc => "RC",
            Self::Wip => "WIP",
            Self::BuildLive => "BUILDLIVE",
        }
    }

    pub fn from_id(id: i32) -> Option<Self> {
        match id {
            0 => Some(Self::Live),
            1 => Some(Self::Rc),
            2 => Some(Self::Wip),
            3 => Some(Self::BuildLive),
            _ => None,
        }
    }
}
