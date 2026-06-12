//! Per-archive (and per-config-type) file format labels + colors. Used by the browser
//! and inspector to show a consistent "extension chip" for each entry.

use cache::config::group as config_group;
use cache::{
    ANIMS_ARCHIVE, BASES_ARCHIVE, CONFIG_ARCHIVE, INTERFACES_ARCHIVE, MAPS_ARCHIVE,
    MODELS_ARCHIVE,
};
use eframe::egui::Color32;

// Archive ids not yet exported as constants in `cache::lib` — match the layout in
// `ARCHIVE_NAMES`.
const JAGFX_ARCHIVE: u8 = 4;
const SONGS_ARCHIVE: u8 = 6;
const SPRITES_ARCHIVE: u8 = 8;
const TEXTURES_ARCHIVE: u8 = 9;
const BINARY_ARCHIVE: u8 = 10;
const JINGLES_ARCHIVE: u8 = 11;
const CLIENTSCRIPTS_ARCHIVE: u8 = 12;
const FONTS_ARCHIVE: u8 = 13;
const VORBIS_ARCHIVE: u8 = 14;
const PATCHES_ARCHIVE: u8 = 15;

/// Display info for a file at (archive, group, optional file_id within group).
pub struct TypeInfo {
    pub ext: &'static str,
    pub color: Color32,
    #[allow(dead_code)] // reserved for future browser filtering/grouping by kind.
    pub kind: Kind,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Kind {
    Model,
    Anim,
    AnimBase,
    Map,
    Music,
    Sprite,
    Texture,
    Font,
    Image,
    Script,
    Config(ConfigKind),
    Interface,
    Generic,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum ConfigKind {
    Npc, Obj, Loc, Seq, Spot, Idk, Flo, Flu, Inv, Enum, VarBit, Varp,
}

pub fn for_group(archive: u8, group: u32, file_id: Option<i32>) -> TypeInfo {
    if archive == CONFIG_ARCHIVE {
        return for_config(group, file_id);
    }
    match archive {
        ANIMS_ARCHIVE => TypeInfo { ext: "anim",   color: C_ANIM,    kind: Kind::Anim },
        BASES_ARCHIVE => TypeInfo { ext: "base",   color: C_ANIM,    kind: Kind::AnimBase },
        INTERFACES_ARCHIVE => TypeInfo { ext: "if", color: C_UI,     kind: Kind::Interface },
        JAGFX_ARCHIVE => TypeInfo { ext: "jagfx",  color: C_GENERIC, kind: Kind::Generic },
        MAPS_ARCHIVE => TypeInfo { ext: "map",     color: C_MAP,     kind: Kind::Map },
        SONGS_ARCHIVE => TypeInfo { ext: "mid",    color: C_MUSIC,   kind: Kind::Music },
        MODELS_ARCHIVE => TypeInfo { ext: "ob2",   color: C_MODEL,   kind: Kind::Model },
        SPRITES_ARCHIVE => TypeInfo { ext: "spr",  color: C_IMAGE,   kind: Kind::Sprite },
        TEXTURES_ARCHIVE => TypeInfo { ext: "tex", color: C_IMAGE,   kind: Kind::Texture },
        BINARY_ARCHIVE => TypeInfo { ext: "bin",   color: C_GENERIC, kind: Kind::Image },
        JINGLES_ARCHIVE => TypeInfo { ext: "mid",  color: C_MUSIC,   kind: Kind::Music },
        CLIENTSCRIPTS_ARCHIVE => TypeInfo { ext: "cs2", color: C_SCRIPT, kind: Kind::Script },
        FONTS_ARCHIVE => TypeInfo { ext: "font",   color: C_IMAGE,   kind: Kind::Font },
        VORBIS_ARCHIVE => TypeInfo { ext: "vorb",  color: C_MUSIC,   kind: Kind::Generic },
        PATCHES_ARCHIVE => TypeInfo { ext: "patch", color: C_GENERIC, kind: Kind::Generic },
        _ => TypeInfo { ext: "dat", color: C_GENERIC, kind: Kind::Generic },
    }
}

fn for_config(group: u32, _file_id: Option<i32>) -> TypeInfo {
    use ConfigKind::*;
    let (ext, kind) = match group {
        g if g == config_group::NPC    => ("npc",    Npc),
        g if g == config_group::OBJ    => ("obj",    Obj),
        g if g == config_group::LOC    => ("loc",    Loc),
        g if g == config_group::SEQ    => ("seq",    Seq),
        g if g == config_group::SPOT   => ("spot",   Spot),
        g if g == config_group::IDK    => ("idk",    Idk),
        g if g == config_group::FLO    => ("flo",    Flo),
        g if g == config_group::FLU    => ("flu",    Flu),
        g if g == config_group::INV    => ("inv",    Inv),
        g if g == config_group::ENUM   => ("enum",   Enum),
        g if g == config_group::VARBIT => ("varbit", VarBit),
        g if g == config_group::VARP   => ("varp",   Varp),
        _ => return TypeInfo { ext: "dat", color: C_GENERIC, kind: Kind::Generic },
    };
    TypeInfo { ext, color: C_CONFIG, kind: Kind::Config(kind) }
}

// Palette — calm, distinguishable, dark-mode-friendly.
const C_MODEL:   Color32 = Color32::from_rgb(140, 200, 130);  // green
const C_ANIM:    Color32 = Color32::from_rgb(200, 170, 100);  // amber
const C_MAP:     Color32 = Color32::from_rgb(110, 180, 220);  // sky
const C_MUSIC:   Color32 = Color32::from_rgb(200, 130, 200);  // magenta
const C_IMAGE:   Color32 = Color32::from_rgb(220, 150, 110);  // peach
const C_SCRIPT:  Color32 = Color32::from_rgb(150, 170, 220);  // periwinkle
const C_CONFIG:  Color32 = Color32::from_rgb(180, 180, 110);  // ochre
const C_UI:      Color32 = Color32::from_rgb(170, 200, 230);  // pale-blue
const C_GENERIC: Color32 = Color32::from_rgb(140, 140, 140);  // grey
