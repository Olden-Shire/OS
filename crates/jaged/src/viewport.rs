//! Center viewport — renders the selected asset using the appropriate visualizer.
//! Falls back to "no preview" text for archives without a viewer yet.
//!
//! Decode logic lives here AND in `details`. Cheap enough to do twice per frame for now;
//! if it ever shows up in a profile, share via a per-frame cache.

use cache::content::pack_file;
use cache::{
    ANIMS_ARCHIVE, BASES_ARCHIVE, CONFIG_ARCHIVE, Cache, INTERFACES_ARCHIVE, MAPS_ARCHIVE,
    MODELS_ARCHIVE,
};
use eframe::egui;

use crate::Selection;

const SONGS_ARCHIVE: u8 = 6;
const SPRITES_ARCHIVE: u8 = 8;
const BINARY_ARCHIVE: u8 = 10;
const JINGLES_ARCHIVE: u8 = 11;
const CLIENTSCRIPTS_ARCHIVE: u8 = 12;

#[allow(clippy::too_many_arguments)]
pub fn draw(
    ui: &mut egui::Ui,
    cache: &mut Cache,
    selection: &mut Selection,
    player: &mut Option<synth::Player>,
    player_error: &mut Option<String>,
    model_view: &mut crate::model_view::ModelView,
    cs2_view: &mut crate::cs2_view::Cs2View,
) {
    let sel = *selection;
    let (Some(archive), Some(group)) = (sel.archive, sel.group) else {
        center_placeholder(ui, "select a group from the left panel");
        return;
    };
    let _ = pack_file::pack_name_for_config_group; // hint: pack scope shown in details

    let bytes = match decompress_safe(cache, archive, group) {
        Ok(b) => b,
        Err(msg) => {
            ui.colored_label(egui::Color32::LIGHT_RED, msg);
            return;
        }
    };

    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| match archive {
        INTERFACES_ARCHIVE => crate::interface_view::draw(ui, cache, group, selection),
        MODELS_ARCHIVE => crate::model_view::draw(ui, group, &bytes, model_view),
        SPRITES_ARCHIVE => crate::sprite_view::draw(ui, group, &bytes),
        CLIENTSCRIPTS_ARCHIVE => crate::cs2_view::draw(ui, cache, group, &bytes, cs2_view),
        SONGS_ARCHIVE => draw_song(ui, cache, "song", group, &bytes, player, player_error),
        JINGLES_ARCHIVE => draw_song(ui, cache, "jingle", group, &bytes, player, player_error),
        BINARY_ARCHIVE => draw_binary_image(ui, &bytes),
        MAPS_ARCHIVE => map_placeholder(ui, &bytes),
        ANIMS_ARCHIVE | BASES_ARCHIVE | CONFIG_ARCHIVE => center_placeholder(
            ui,
            "no viewport for this asset — see the right panel for typed details.",
        ),
        _ => center_placeholder(ui, "no viewport for this archive yet."),
    });
}

fn center_placeholder(ui: &mut egui::Ui, msg: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(60.0);
        ui.label(egui::RichText::new(msg).weak().italics());
    });
}

fn map_placeholder(ui: &mut egui::Ui, bytes: &[u8]) {
    ui.vertical_centered(|ui| {
        ui.add_space(60.0);
        ui.label(egui::RichText::new("map viewport — pending Pix3D scene port").weak());
        ui.label(
            egui::RichText::new(format!("decompressed: {} bytes", bytes.len()))
                .monospace()
                .small()
                .weak(),
        );
    });
}

fn draw_binary_image(ui: &mut egui::Ui, bytes: &[u8]) {
    let is_jpeg = bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF;
    if !is_jpeg {
        center_placeholder(ui, "binary blob — no viewer for this content.");
        return;
    }
    let uri = format!("bytes://binary-{:x}.jpg", crc32_quick(bytes));
    ui.add(
        egui::Image::from_bytes(uri, bytes.to_vec())
            .max_height(ui.available_height() - 20.0)
            .fit_to_original_size(1.0),
    );
}

fn crc32_quick(b: &[u8]) -> u32 {
    io::crc32::checksum(b, 0, b.len())
}

fn draw_song(
    ui: &mut egui::Ui,
    cache: &mut Cache,
    scope: &str,
    id: u32,
    jagex_bytes: &[u8],
    player: &mut Option<synth::Player>,
    player_error: &mut Option<String>,
) {
    if jagex_bytes.is_empty() {
        ui.label("(empty)");
        return;
    }
    let midi = std::panic::catch_unwind(|| io::midi::decode(jagex_bytes)).unwrap_or_default();
    let name = crate::music::pack_name(scope, id, &format!("{scope}_{id}"));
    crate::music::draw(ui, cache, &name, &midi, player, player_error);
}

/// Read + decompress a group, mapping all the failure modes (missing, errored, panic'd
/// XTEA decompression) to a single Err string the caller can surface.
pub fn decompress_safe(cache: &mut Cache, archive: u8, group: u32) -> Result<Vec<u8>, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cache.read_group(archive, group)
    })) {
        Ok(Ok(Some(b))) => Ok(b),
        Ok(Ok(None)) => Ok(Vec::new()),
        Ok(Err(e)) => Err(format!("decode error: {e}")),
        Err(_) => Err("decode panic — likely an XTEA-encrypted map without a key.".into()),
    }
}
