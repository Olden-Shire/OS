//! Right panel — typed details for whatever's selected. Always shows a summary card
//! (compression, sizes, name hash, file count) up top; below that, an archive-specific
//! field grid (npc/obj/loc/etc. for configs, IfType fields for interfaces, …).
//!
//! No 3D rendering happens here — the center viewport owns that. This panel is
//! read-only data inspection.

use cache::config::{
    EnumType, FloType, FluType, IdkType, InvType, LocType, NpcType, ObjType, SeqType,
    SpotType, VarBitType, VarpType, group as config_group,
};
use cache::content::pack_file;
use cache::{
    ANIMS_ARCHIVE, BASES_ARCHIVE, CONFIG_ARCHIVE, Cache, INTERFACES_ARCHIVE, MAPS_ARCHIVE,
    MODELS_ARCHIVE,
};
use eframe::egui;

use crate::{Selection, browser, typeinfo, viewport};

pub fn draw(ui: &mut egui::Ui, cache: &mut Cache, selection: &mut Selection) {
    let sel = *selection;
    let (Some(archive), Some(group)) = (sel.archive, sel.group) else {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("nothing selected").weak().italics());
        });
        return;
    };

    let info = typeinfo::for_group(archive, group, sel.file_id);
    ui.horizontal(|ui| {
        browser::chip(ui, info.ext, info.color);
        let mut breadcrumb = format!("{}  /  {group}", crate::archive_label(archive));
        if let Some(fid) = sel.file_id {
            breadcrumb.push_str(&format!("  /  {fid}"));
        }
        ui.label(egui::RichText::new(breadcrumb).strong());
    });
    ui.separator();

    let raw = match cache.read_raw(archive, group) {
        Ok(Some(b)) => b,
        Ok(None) => {
            ui.label("group missing on disk.");
            return;
        }
        Err(e) => {
            ui.colored_label(egui::Color32::RED, format!("read error: {e}"));
            return;
        }
    };
    let decompressed = viewport::decompress_safe(cache, archive, group).unwrap_or_default();

    section(ui, "summary", |ui| {
        egui::Grid::new("group_meta").num_columns(2).striped(true).show(ui, |ui| {
            kv(ui, "compression", ctype_label(raw.first().copied().unwrap_or(0)));
            kv(ui, "raw bytes", &raw.len().to_string());
            kv(ui, "decompressed", &decompressed.len().to_string());
            let index = cache.index(archive);
            if let Some(file_ids) = index.file_ids.get(group as usize) {
                kv(ui, "files in group", &file_ids.len().to_string());
            }
            if let Some(hash_table) = &index.group_name_hashes {
                kv(
                    ui,
                    "name hash",
                    &format!(
                        "{:#010x}",
                        hash_table.get(group as usize).copied().unwrap_or(0)
                    ),
                );
            }
        });
    });

    // Archive-specific overview cards that pair nicely with `summary` (sit right next
    // to it in the right column).
    if archive == MODELS_ARCHIVE && !decompressed.is_empty() {
        draw_mesh_card(ui, &decompressed);
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt("details_scroll")
        .show(ui, |ui| match archive {
            ANIMS_ARCHIVE => draw_anim(ui, cache, group, sel.file_id),
            BASES_ARCHIVE => draw_anim_base(ui, group, &decompressed),
            CONFIG_ARCHIVE => draw_config(ui, cache, group, sel.file_id, &decompressed),
            INTERFACES_ARCHIVE => draw_iftype(ui, cache, group, sel.file_id),
            MAPS_ARCHIVE => draw_map_meta(ui, &decompressed),
            _ => draw_hex_preview(ui, &decompressed),
        });
}

fn section(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(title.to_uppercase()).small().weak());
    egui::Frame::group(ui.style())
        .corner_radius(4.0)
        .inner_margin(egui::Margin::same(10))
        .show(ui, body);
    ui.add_space(6.0);
}

fn ctype_label(b: u8) -> &'static str {
    match b {
        0 => "0 (none)",
        1 => "1 (bzip2)",
        2 => "2 (gzip)",
        _ => "?",
    }
}

fn draw_anim(ui: &mut egui::Ui, cache: &mut Cache, group: u32, file_id: Option<i32>) {
    let Some(fid) = file_id else {
        ui.label(egui::RichText::new("pick a frame from the bottom-left files panel.").weak().italics());
        return;
    };
    let files = match cache.read_files(ANIMS_ARCHIVE, group) {
        Ok(Some(f)) => f,
        _ => {
            ui.label("no frame data.");
            return;
        }
    };
    let Some((_, frame_bytes)) = files.iter().find(|(id, _)| *id == fid) else {
        ui.label("frame not in group.");
        return;
    };
    section(ui, "frame", |ui| {
        egui::Grid::new("frame").num_columns(2).striped(true).show(ui, |ui| {
            kv(ui, "id", &fid.to_string());
            kv(ui, "bytes", &frame_bytes.len().to_string());
            if frame_bytes.len() >= 2 {
                let base_id = ((frame_bytes[0] as i32) << 8) | (frame_bytes[1] as i32 & 0xFF);
                kv(ui, "base id", &base_id.to_string());
            }
        });
    });
    section(ui, "raw", |ui| draw_hex_preview(ui, frame_bytes));
}

/// Mesh stats card — point / face / texture counts, decoded from the raw model bytes.
/// Panics in the OB3 decoder are swallowed so a corrupt entry doesn't break the panel.
fn draw_mesh_card(ui: &mut egui::Ui, bytes: &[u8]) {
    let model = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cache::model::Model::decode(bytes)
    })) {
        Ok(m) => m,
        Err(_) => {
            section(ui, "mesh", |ui| {
                ui.colored_label(egui::Color32::LIGHT_RED, "model decode failed");
            });
            return;
        }
    };
    section(ui, "mesh", |ui| {
        egui::Grid::new("mesh_meta").num_columns(2).striped(true).show(ui, |ui| {
            kv(ui, "points", &model.num_points.to_string());
            kv(ui, "faces", &model.num_faces.to_string());
            kv(ui, "textures", &model.num_t.to_string());
        });
    });
}

fn draw_anim_base(ui: &mut egui::Ui, id: u32, bytes: &[u8]) {
    let base = cache::anim::AnimBase::decode(id as i32, bytes);
    section(ui, "skeleton", |ui| {
        egui::Grid::new("base").num_columns(2).striped(true).show(ui, |ui| {
            kv(ui, "joints", &base.types.len().to_string());
            for (i, t) in base.types.iter().enumerate().take(20) {
                kv(ui, &format!("joint {i} type"), joint_type_label(*t));
            }
            if base.types.len() > 20 {
                kv(ui, "...", &format!("+{} more", base.types.len() - 20));
            }
        });
    });
}

fn joint_type_label(t: i32) -> &'static str {
    match t {
        0 => "0 (pivot)",
        1 => "1 (translate)",
        2 => "2 (rotate)",
        3 => "3 (scale)",
        5 => "5 (transparency)",
        _ => "?",
    }
}

fn draw_map_meta(ui: &mut egui::Ui, bytes: &[u8]) {
    section(ui, "region", |ui| {
        ui.label(egui::RichText::new(format!("decompressed: {} bytes", bytes.len())).monospace());
    });
    section(ui, "raw", |ui| draw_hex_preview(ui, bytes));
}

fn draw_iftype(ui: &mut egui::Ui, cache: &mut Cache, group: u32, file_id: Option<i32>) {
    let Some(fid) = file_id else {
        ui.label(egui::RichText::new("pick a subcomponent from the bottom-left.").weak().italics());
        return;
    };
    let files = match cache.read_files(INTERFACES_ARCHIVE, group) {
        Ok(Some(f)) => f,
        _ => {
            ui.label("no subcomponents.");
            return;
        }
    };
    let Some((_, comp_bytes)) = files.iter().find(|(id, _)| *id == fid) else {
        ui.label("file not in group.");
        return;
    };
    let parent_id = ((group as i32) << 16) | fid;
    let if_ = cache::iftype::IfType::decode(parent_id, fid, comp_bytes);
    section(ui, "interface component", |ui| {
        egui::Grid::new("if").num_columns(2).striped(true).show(ui, |ui| {
            kv(ui, "format", if if_.v3 { "v3" } else { "v1" });
            kv(ui, "type", component_type(if_.type_));
            kv(ui, "button_type", &if_.button_type.to_string());
            kv(ui, "pos", &format!("({}, {})", if_.x, if_.y));
            kv(ui, "size", &format!("{}x{}", if_.width, if_.height));
            kv(ui, "hide", yes_no(if_.hide));
            if !if_.text.is_empty() {
                kv(ui, "text", &if_.text);
            }
            if if_.font != -1 {
                kv(ui, "font", &if_.font.to_string());
            }
            if if_.model1_id != -1 {
                kv(ui, "model", &if_.model1_id.to_string());
            }
            if !if_.target_verb.is_empty() {
                kv(ui, "target verb", &if_.target_verb);
            }
            if !if_.op_names.is_empty() {
                kv(
                    ui,
                    "ops",
                    &if_.op_names.iter().flatten().cloned().collect::<Vec<_>>().join(", "),
                );
            }
            kv(ui, "has script hooks", yes_no(if_.hashook));
        });
    });
}

fn component_type(t: i32) -> &'static str {
    match t {
        0 => "0 (layer)",
        1 => "1 (unknown)",
        2 => "2 (inv)",
        3 => "3 (rect)",
        4 => "4 (text)",
        5 => "5 (graphic)",
        6 => "6 (model)",
        7 => "7 (invtext)",
        8 => "8 (tooltip)",
        9 => "9 (line)",
        _ => "?",
    }
}

fn draw_config(
    ui: &mut egui::Ui,
    cache: &mut Cache,
    group: u32,
    file_id: Option<i32>,
    _bytes: &[u8],
) {
    let Some(fid) = file_id else {
        ui.label("pick a record from the bottom-left.");
        return;
    };
    let files = match cache.read_files(CONFIG_ARCHIVE, group) {
        Ok(Some(f)) => f,
        _ => {
            ui.label("no records.");
            return;
        }
    };
    let Some((_, record)) = files.iter().find(|(id, _)| *id == fid) else {
        ui.label("record not in group.");
        return;
    };
    match group {
        config_group::NPC => draw_npc(ui, fid, record),
        config_group::OBJ => draw_obj(ui, fid, record),
        config_group::LOC => draw_loc(ui, fid, record),
        config_group::SEQ => draw_seq(ui, fid, record),
        config_group::SPOT => draw_spot(ui, fid, record),
        config_group::IDK => draw_idk(ui, fid, record),
        config_group::FLO => draw_flo(ui, fid, record),
        config_group::FLU => draw_flu(ui, fid, record),
        config_group::INV => draw_inv(ui, fid, record),
        config_group::ENUM => draw_enum(ui, fid, record),
        config_group::VARBIT => draw_varbit(ui, fid, record),
        config_group::VARP => draw_varp(ui, fid, record),
        _ => {
            ui.label(format!("unknown config group {group}, record {fid}"));
            draw_hex_preview(ui, record);
        }
    }
    if let Some(scope) = pack_file::pack_name_for_config_group(group) {
        ui.separator();
        ui.label(format!("pack-file scope: {scope}.pack"));
    }
}

fn draw_npc(ui: &mut egui::Ui, id: i32, bytes: &[u8]) {
    let n = NpcType::decode(id, bytes);
    section(ui, "npc", |ui| {
        egui::Grid::new("npc").num_columns(2).striped(true).show(ui, |ui| {
            kv(ui, "name", &n.name);
            kv(ui, "size", &n.size.to_string());
            kv(ui, "vislevel", &n.vislevel.to_string());
            kv(ui, "models", &format!("{:?}", n.models));
            if !n.head_models.is_empty() {
                kv(ui, "head models", &format!("{:?}", n.head_models));
            }
            if n.readyanim != -1 {
                kv(ui, "ready anim", &n.readyanim.to_string());
            }
            if n.walkanim != -1 {
                kv(ui, "walk anim", &n.walkanim.to_string());
            }
            let ops: Vec<_> = n
                .op
                .iter()
                .enumerate()
                .filter_map(|(i, o)| o.as_ref().map(|s| format!("{}:{s}", i + 1)))
                .collect();
            if !ops.is_empty() {
                kv(ui, "ops", &ops.join(", "));
            }
            kv(ui, "minimap", yes_no(n.minimap));
            kv(ui, "active", yes_no(n.active));
        });
    });
}

fn draw_obj(ui: &mut egui::Ui, id: i32, bytes: &[u8]) {
    let o = ObjType::decode(id, bytes);
    section(ui, "obj", |ui| {
        egui::Grid::new("obj").num_columns(2).striped(true).show(ui, |ui| {
            kv(ui, "name", &o.name);
            kv(ui, "model", &o.model.to_string());
            kv(ui, "cost", &o.cost.to_string());
            kv(ui, "stackable", yes_no(o.stackable == 1));
            kv(ui, "members", yes_no(o.members));
            let ops: Vec<_> = o
                .op
                .iter()
                .enumerate()
                .filter_map(|(i, x)| x.as_ref().map(|s| format!("{}:{s}", i + 1)))
                .collect();
            if !ops.is_empty() {
                kv(ui, "ops", &ops.join(", "));
            }
            let iops: Vec<_> = o
                .iop
                .iter()
                .enumerate()
                .filter_map(|(i, x)| x.as_ref().map(|s| format!("{}:{s}", i + 1)))
                .collect();
            if !iops.is_empty() {
                kv(ui, "iops", &iops.join(", "));
            }
            if o.certtemplate != -1 {
                kv(ui, "cert template", &o.certtemplate.to_string());
                kv(ui, "cert link", &o.certlink.to_string());
            }
            kv(ui, "zoom2d", &o.zoom2d.to_string());
        });
    });
}

fn draw_loc(ui: &mut egui::Ui, id: i32, bytes: &[u8]) {
    let l = LocType::decode(id, bytes);
    section(ui, "loc", |ui| {
        egui::Grid::new("loc").num_columns(2).striped(true).show(ui, |ui| {
            kv(ui, "name", &l.name);
            kv(ui, "size", &format!("{}x{}", l.width, l.length));
            kv(ui, "models", &format!("{:?}", l.models));
            if !l.shapes.is_empty() {
                kv(ui, "shapes", &format!("{:?}", l.shapes));
            }
            kv(ui, "blockwalk", &l.blockwalk.to_string());
            kv(ui, "blockrange", yes_no(l.blockrange));
            kv(ui, "active", &l.active.to_string());
            if l.anim != -1 {
                kv(ui, "anim", &l.anim.to_string());
            }
            let ops: Vec<_> = l
                .op
                .iter()
                .enumerate()
                .filter_map(|(i, x)| x.as_ref().map(|s| format!("{}:{s}", i + 1)))
                .collect();
            if !ops.is_empty() {
                kv(ui, "ops", &ops.join(", "));
            }
            if l.mapscene != -1 {
                kv(ui, "mapscene", &l.mapscene.to_string());
            }
            if l.mapfunction != -1 {
                kv(ui, "mapfunction", &l.mapfunction.to_string());
            }
        });
    });
}

fn draw_seq(ui: &mut egui::Ui, id: i32, bytes: &[u8]) {
    let s = SeqType::decode(id, bytes);
    section(ui, "seq", |ui| {
        egui::Grid::new("seq").num_columns(2).striped(true).show(ui, |ui| {
            kv(ui, "frames", &format!("{}", s.frames.len()));
            kv(ui, "iframes", &format!("{}", s.iframes.len()));
            kv(ui, "loops", &s.loops.to_string());
            kv(ui, "priority", &s.priority.to_string());
            kv(ui, "preanim_move", &s.preanim_move.to_string());
            kv(ui, "postanim_move", &s.postanim_move.to_string());
            if !s.delay.is_empty() {
                let sum: i32 = s.delay.iter().sum();
                kv(ui, "total delay", &sum.to_string());
            }
        });
    });
}

fn draw_spot(ui: &mut egui::Ui, id: i32, bytes: &[u8]) {
    let s = SpotType::decode(id, bytes);
    section(ui, "spotanim", |ui| {
        egui::Grid::new("spot").num_columns(2).striped(true).show(ui, |ui| {
            kv(ui, "model", &s.model.to_string());
            kv(ui, "anim", &s.anim.to_string());
            kv(ui, "angle", &s.angle.to_string());
            kv(ui, "size", &format!("{}x{}", s.resizeh, s.resizev));
        });
    });
}

fn draw_idk(ui: &mut egui::Ui, id: i32, bytes: &[u8]) {
    let i = IdkType::decode(id, bytes);
    section(ui, "idk", |ui| {
        egui::Grid::new("idk").num_columns(2).striped(true).show(ui, |ui| {
            kv(ui, "type", &i.type_.to_string());
            kv(ui, "models", &format!("{:?}", i.models));
            kv(ui, "head", &format!("{:?}", i.head));
            kv(ui, "disable", yes_no(i.disable));
        });
    });
}

fn draw_flo(ui: &mut egui::Ui, id: i32, bytes: &[u8]) {
    let f = FloType::decode(id, bytes);
    section(ui, "floor", |ui| {
        egui::Grid::new("flo").num_columns(2).striped(true).show(ui, |ui| {
            kv(ui, "colour", &format!("{:#08x}", f.colour as u32));
            kv(ui, "texture", &f.texture.to_string());
            kv(ui, "occlude", yes_no(f.occlude));
            kv(ui, "mapcolour", &format!("{:#08x}", f.mapcolour as u32));
        });
    });
}

fn draw_flu(ui: &mut egui::Ui, id: i32, bytes: &[u8]) {
    let f = FluType::decode(id, bytes);
    section(ui, "fluid", |ui| {
        egui::Grid::new("flu").num_columns(2).striped(true).show(ui, |ui| {
            kv(ui, "colour", &format!("{:#08x}", f.colour as u32));
        });
    });
}

fn draw_inv(ui: &mut egui::Ui, id: i32, bytes: &[u8]) {
    let v = InvType::decode(id, bytes);
    section(ui, "inv", |ui| {
        egui::Grid::new("inv").num_columns(2).striped(true).show(ui, |ui| {
            kv(ui, "size", &v.size.to_string());
        });
    });
}

fn draw_enum(ui: &mut egui::Ui, id: i32, bytes: &[u8]) {
    let e = EnumType::decode(id, bytes);
    section(ui, "enum", |ui| {
        egui::Grid::new("enum").num_columns(2).striped(true).show(ui, |ui| {
            kv(ui, "inputtype", &e.inputtype.to_string());
            kv(ui, "outputtype", &format!("'{}'", e.outputtype as char));
            kv(ui, "default str", &e.default_string);
            kv(ui, "default int", &e.default_int.to_string());
            kv(ui, "entries", &e.count.to_string());
        });
    });
}

fn draw_varbit(ui: &mut egui::Ui, id: i32, bytes: &[u8]) {
    let v = VarBitType::decode(id, bytes);
    section(ui, "varbit", |ui| {
        egui::Grid::new("varbit").num_columns(2).striped(true).show(ui, |ui| {
            kv(ui, "basevar", &v.basevar.to_string());
            kv(ui, "startbit", &v.startbit.to_string());
            kv(ui, "endbit", &v.endbit.to_string());
        });
    });
}

fn draw_varp(ui: &mut egui::Ui, id: i32, bytes: &[u8]) {
    let v = VarpType::decode(id, bytes);
    section(ui, "varp", |ui| {
        egui::Grid::new("varp").num_columns(2).striped(true).show(ui, |ui| {
            kv(ui, "clientcode", &v.clientcode.to_string());
        });
    });
}

fn kv(ui: &mut egui::Ui, k: &str, v: &str) {
    ui.label(egui::RichText::new(k).weak());
    ui.label(egui::RichText::new(v).monospace());
    ui.end_row();
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

fn draw_hex_preview(ui: &mut egui::Ui, bytes: &[u8]) {
    ui.label(format!("bytes: {}", bytes.len()));
    let preview = &bytes[..bytes.len().min(256)];
    let mut text = String::with_capacity(preview.len() * 3 + 64);
    for (i, b) in preview.iter().enumerate() {
        if i % 16 == 0 && i != 0 {
            text.push('\n');
        }
        text.push_str(&format!("{b:02X} "));
    }
    if bytes.len() > 256 {
        text.push_str(&format!("\n... +{} more bytes", bytes.len() - 256));
    }
    ui.add(
        egui::TextEdit::multiline(&mut text.as_str())
            .font(egui::TextStyle::Monospace)
            .desired_width(f32::INFINITY)
            .desired_rows(8),
    );
}
