//! Interface group ↔ readable `.if` text (Content-old style).
//!
//! A whole interface (one archive-3 group) becomes ONE `.if` file: a
//! `[com_N]` section per component, `key = value` lines inside. Refs use
//! the Content-old symbolic convention where we have a stand-in (models →
//! `com_i{id}`); everything else is the numeric id (real names arrive in
//! the later rename pass).
//!
//! Byte-exactness rides on the verified `IfType::encode`: we serialise
//! each component, re-parse, re-encode, and require every component to
//! reproduce its original bytes. Any miss → `None`, and the whole group
//! stays in the per-file `.dat` layout (nothing lost).

use crate::iftype::{HookArg, IfType};

/// Serialise a whole interface group's components to `.if` text iff every
/// component re-encodes byte-exactly. `comps` is `(sub_id, raw_bytes)` in
/// file order. `group_id` is the interface id (high half of parent_id).
pub fn decode_group(group_id: u32, comps: &[(i32, Vec<u8>)]) -> Option<String> {
    let mut out = format!("// interface {group_id}\n");
    for (sub_id, bytes) in comps {
        if bytes.is_empty() {
            return None; // empty component — keep .dat
        }
        let parent = ((group_id as i32) << 16) | sub_id;
        let t = IfType::decode(parent, *sub_id, bytes);
        out.push_str(&format!("\n[com_{sub_id}]\n"));
        write_component(&mut out, &t);
    }
    // Verify: re-parse and re-encode every component to the exact bytes.
    let rebuilt = encode_group(group_id, &out)?;
    if rebuilt.len() != comps.len() {
        return None;
    }
    for ((sub, want), (sub2, got)) in comps.iter().zip(&rebuilt) {
        if sub != sub2 || want != got {
            return None;
        }
    }
    Some(out)
}

/// Parse `.if` text back to `(sub_id, raw_bytes)` per component, in the
/// order the sections appear. `None` on any unparseable section.
pub fn encode_group(group_id: u32, text: &str) -> Option<Vec<(i32, Vec<u8>)>> {
    let mut comps: Vec<(i32, Vec<u8>)> = Vec::new();
    let mut cur_sub: Option<i32> = None;
    let mut fields: Vec<(String, String)> = Vec::new();

    let flush = |sub: i32, fields: &[(String, String)], comps: &mut Vec<(i32, Vec<u8>)>| -> Option<()> {
        let t = parse_component(group_id, sub, fields)?;
        comps.push((sub, t.encode()?));
        Some(())
    };

    for raw in text.lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        if let Some(inside) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            if let Some(sub) = cur_sub.take() {
                flush(sub, &fields, &mut comps)?;
                fields.clear();
            }
            // Header is `com_N` or, once a component is named, `com_N <name>`.
            // The child index `com_N` is the structural key (used to encode the
            // component's identity); the optional trailing token is a tooling-
            // only display name (renamed from the IDE), ignored here — so the
            // cache bytes never depend on it.
            let idx_tok = inside.split_whitespace().next()?;
            let sub = idx_tok.strip_prefix("com_")?.parse::<i32>().ok()?;
            cur_sub = Some(sub);
        } else {
            // Split on the FIRST '=' and keep the value VERBATIM (no trim):
            // string fields may have significant leading/trailing spaces.
            let (k, v) = raw.split_once('=')?;
            fields.push((k.to_string(), v.to_string()));
        }
    }
    if let Some(sub) = cur_sub.take() {
        flush(sub, &fields, &mut comps)?;
    }
    Some(comps)
}

/// Debug: serialise one component to its `.if` section text (no verify).
#[doc(hidden)]
pub fn debug_component(group_id: u32, sub_id: i32, bytes: &[u8]) -> String {
    let parent = ((group_id as i32) << 16) | sub_id;
    let t = IfType::decode(parent, sub_id, bytes);
    let mut s = String::new();
    write_component(&mut s, &t);
    s
}

// ── serialise ───────────────────────────────────────────────────────────
fn write_component(out: &mut String, t: &IfType) {
    // No spaces around '=' (Content-old style) so values that begin or
    // end with significant whitespace survive the round-trip.
    macro_rules! kv { ($k:expr, $v:expr) => { out.push_str(&format!("{}={}\n", $k, $v)); } }
    let model = |id: i32| if id == -1 { "-1".to_string() } else { format!("com_i{id}") };

    if t.v3 { kv!("fmt", "v3"); }
    kv!("type", t.type_);
    if !t.v3 { kv!("buttontype", t.button_type); }
    kv!("clientcode", t.client_code);
    kv!("x", t.x);
    kv!("y", t.y);
    kv!("width", t.width);
    kv!("height", t.height);
    if t.layer_id != -1 { kv!("layer", layer_ref(t.parent_id, t.layer_id)); }
    if !t.v3 && t.over_layer_id != -1 { kv!("overlayer", layer_ref(t.parent_id, t.over_layer_id)); }
    if t.v3 { kv!("hide", t.hide as i32); }
    if !t.v3 { kv!("trans", t.trans); }

    // v1 conditional scripts (cs2-ish predicate stack).
    if let (Some(c), Some(o)) = (&t.script_comparator, &t.script_operand) {
        let s: Vec<String> = c.iter().zip(o).map(|(a, b)| format!("{a}/{b}")).collect();
        kv!("scriptcmp", s.join(" "));
    }
    if let Some(scripts) = &t.scripts {
        let s: Vec<String> = scripts.iter()
            .map(|inner| inner.iter().map(i32::to_string).collect::<Vec<_>>().join("/"))
            .collect();
        kv!("scripts", s.join(" "));
    }

    match t.type_ {
        0 => {
            if t.v3 { kv!("scrollwidth", t.scroll_width); }
            kv!("scrollheight", t.scroll_height);
            if !t.v3 { kv!("hide", t.hide as i32); }
        }
        2 => {
            kv!("eventcode", format!("{:#x}", t.event_code as u32));
            kv!("marginx", t.margin_x);
            kv!("marginy", t.margin_y);
            for i in 0..20 {
                if t.inv_has_bg.get(i).copied().unwrap_or(false) {
                    kv!(format!("bg{i}"), format!("{},{},{}", t.inv_background_x[i], t.inv_background_y[i], t.inv_background[i]));
                }
            }
            for (i, op) in t.iop.iter().enumerate() {
                if let Some(s) = op { kv!(format!("iop{}", i + 1), s); }
            }
        }
        3 => {
            if t.v3 { kv!("colour", hexc(t.colour)); kv!("fill", t.fill as i32); kv!("trans", t.trans); }
            else { kv!("fill", t.fill as i32); }
        }
        5 => {
            kv!("graphic", t.graphic);
            if t.v3 {
                kv!("rotate", t.rotate); kv!("tiling", t.tiling as i32); kv!("trans", t.trans);
                kv!("outline", t.outline); kv!("shadowcolour", hexc(t.shadow_colour));
                kv!("vflip", t.v_flip as i32); kv!("hflip", t.h_flip as i32);
            } else {
                kv!("activegraphic", t.graphic2);
            }
        }
        6 => {
            kv!("model", model(t.model1_id));
            if t.v3 {
                kv!("modelxof", t.model_x_of); kv!("modelyof", t.model_y_of);
                kv!("xan", t.model_x_an); kv!("yan", t.model_y_an); kv!("zan", t.model_z_an);
                kv!("zoom", t.model_zoom);
                if t.model_anim != -1 { kv!("anim", t.model_anim); }
                kv!("orthog", t.orthog as i32);
            } else {
                kv!("activemodel", model(t.model2_id));
                if t.model_anim != -1 { kv!("anim", t.model_anim); }
                if t.model_anim2 != -1 { kv!("activeanim", t.model_anim2); }
                kv!("zoom", t.model_zoom); kv!("xan", t.model_x_an); kv!("yan", t.model_y_an);
            }
        }
        4 => {
            if t.v3 {
                kv!("font", t.font); kv!("text", esc(&t.text));
                kv!("lineheight", t.line_height); kv!("halign", t.h_align); kv!("valign", t.v_align);
                kv!("shadowed", t.shadow as i32); kv!("colour", hexc(t.colour));
            } else {
                kv!("halign", t.h_align); kv!("valign", t.v_align); kv!("lineheight", t.line_height);
                kv!("font", t.font); kv!("shadowed", t.shadow as i32);
                kv!("text", esc(&t.text)); kv!("activetext", esc(&t.text2));
            }
        }
        7 => {
            kv!("halign", t.h_align); kv!("font", t.font); kv!("shadowed", t.shadow as i32);
            kv!("colour", hexc(t.colour)); kv!("marginx", t.margin_x); kv!("marginy", t.margin_y);
            kv!("eventcode", format!("{:#x}", t.event_code as u32));
            for (i, op) in t.iop.iter().enumerate() {
                if let Some(s) = op { kv!(format!("iop{}", i + 1), s); }
            }
        }
        8 => { kv!("text", esc(&t.text)); }
        9 => { if t.v3 { kv!("linewidth", t.line_width); kv!("colour", hexc(t.colour)); } }
        _ => {}
    }
    // v1 text/rect colour block (types 3,4 share the 4-colour set).
    if !t.v3 && (t.type_ == 3 || t.type_ == 4) {
        kv!("colour", hexc(t.colour)); kv!("colour2", hexc(t.colour2));
        kv!("colourover", hexc(t.colour_over)); kv!("colour2over", hexc(t.colour2_over));
    }
    // v1 drag/target verbs.
    if !t.v3 && (t.button_type == 2 || t.type_ == 2) {
        kv!("targetverb", esc(&t.target_verb)); kv!("targetbase", esc(&t.target_base));
    }
    if !t.v3 && matches!(t.button_type, 1 | 4 | 5 | 6) {
        kv!("buttontext", esc(&t.button_text));
    }
    // v3 op block + hooks.
    if t.v3 {
        kv!("eventcode", format!("{:#x}", t.event_code as u32));
        kv!("baseop", esc(&t.base_op_name));
        for (i, op) in t.op_names.iter().enumerate() {
            kv!(format!("op{}", i + 1), op.as_deref().unwrap_or(""));
        }
        kv!("dragdeadzone", t.dragdeadzone); kv!("dragdeadtime", t.dragdeadtime);
        kv!("draggable", t.draggable_behavior as i32); kv!("targetverb", esc(&t.target_verb));
        write_hook(out, "onload", &t.onload);
        write_hook(out, "onmouseover", &t.onmouseover);
        write_hook(out, "onmouseleave", &t.onmouseleave);
        write_hook(out, "ontargetleave", &t.ontargetleave);
        write_hook(out, "ontargetenter", &t.ontargetenter);
        write_hook(out, "onvartransmit", &t.onvartransmit);
        write_hook(out, "oninvtransmit", &t.oninvtransmit);
        write_hook(out, "onstattransmit", &t.onstattransmit);
        write_hook(out, "ontimer", &t.ontimer);
        write_hook(out, "onop", &t.onop);
        write_hook(out, "onmouserepeat", &t.onmouserepeat);
        write_hook(out, "onclick", &t.onclick);
        write_hook(out, "onclickrepeat", &t.onclickrepeat);
        write_hook(out, "onrelease", &t.onrelease);
        write_hook(out, "onhold", &t.onhold);
        write_hook(out, "ondrag", &t.ondrag);
        write_hook(out, "ondragcomplete", &t.ondragcomplete);
        write_hook(out, "onscrollwheel", &t.onscrollwheel);
        write_list(out, "onvartransmitlist", &t.onvartransmitlist);
        write_list(out, "oninvtransmitlist", &t.oninvtransmitlist);
        write_list(out, "onstattransmitlist", &t.onstattransmitlist);
    }
}

fn write_hook(out: &mut String, key: &str, hook: &Option<Vec<HookArg>>) {
    let Some(args) = hook else { return };
    // Args joined by ',' — string args may contain commas/whitespace, so
    // they're comma-escaped (esc_arg) and parsed by splitting only on
    // UNescaped commas with no trimming.
    let s: Vec<String> = args.iter().map(|a| match a {
        HookArg::Int(v) => format!("i:{v}"),
        HookArg::Str(s) => format!("s:{}", esc_arg(s)),
    }).collect();
    out.push_str(&format!("{key}={}\n", s.join(",")));
}

fn write_list(out: &mut String, key: &str, list: &Option<Vec<i32>>) {
    let Some(vs) = list else { return };
    let s: Vec<String> = vs.iter().map(i32::to_string).collect();
    out.push_str(&format!("{key}={}\n", s.join(" ")));
}

// ── parse ───────────────────────────────────────────────────────────────
fn parse_component(group_id: u32, sub: i32, fields: &[(String, String)]) -> Option<IfType> {
    let parent = ((group_id as i32) << 16) | sub;
    let mut t = IfType { parent_id: parent, sub_id: sub, ..Default::default() };
    let get = |k: &str| fields.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
    t.v3 = get("fmt") == Some("v3");
    // inv slots need width/height first; collect all bg* after.
    for (k, v) in fields {
        match k.as_str() {
            "fmt" => {}
            "type" => t.type_ = v.parse().ok()?,
            "buttontype" => t.button_type = v.parse().ok()?,
            "clientcode" => t.client_code = v.parse().ok()?,
            "x" => t.x = v.parse().ok()?,
            "y" => t.y = v.parse().ok()?,
            "width" => t.width = v.parse().ok()?,
            "height" => t.height = v.parse().ok()?,
            "layer" => t.layer_id = parse_layer(parent, v)?,
            "overlayer" => t.over_layer_id = parse_layer(parent, v)?,
            "hide" => t.hide = v == "1",
            "trans" => t.trans = v.parse().ok()?,
            "scriptcmp" => {
                let (mut c, mut o) = (Vec::new(), Vec::new());
                for pair in v.split_whitespace() {
                    let (a, b) = pair.split_once('/')?;
                    c.push(a.parse().ok()?); o.push(b.parse().ok()?);
                }
                t.script_comparator = Some(c); t.script_operand = Some(o);
            }
            "scripts" => {
                let mut all = Vec::new();
                for grp in v.split_whitespace() {
                    all.push(grp.split('/').map(|x| x.parse::<i32>()).collect::<Result<Vec<_>, _>>().ok()?);
                }
                t.scripts = Some(all);
            }
            "scrollwidth" => t.scroll_width = v.parse().ok()?,
            "scrollheight" => t.scroll_height = v.parse().ok()?,
            "eventcode" => t.event_code = parse_u32(v)? as i32,
            "marginx" => t.margin_x = v.parse().ok()?,
            "marginy" => t.margin_y = v.parse().ok()?,
            "fill" => t.fill = v == "1",
            "graphic" => t.graphic = v.parse().ok()?,
            "activegraphic" => t.graphic2 = v.parse().ok()?,
            "rotate" => t.rotate = v.parse().ok()?,
            "tiling" => t.tiling = v == "1",
            "outline" => t.outline = v.parse().ok()?,
            "shadowcolour" => t.shadow_colour = parse_u32(v)? as i32,
            "vflip" => t.v_flip = v == "1",
            "hflip" => t.h_flip = v == "1",
            "model" => t.model1_id = parse_model(v)?,
            "activemodel" => t.model2_id = parse_model(v)?,
            "modelxof" => t.model_x_of = v.parse().ok()?,
            "modelyof" => t.model_y_of = v.parse().ok()?,
            "xan" => t.model_x_an = v.parse().ok()?,
            "yan" => t.model_y_an = v.parse().ok()?,
            "zan" => t.model_z_an = v.parse().ok()?,
            "zoom" => t.model_zoom = v.parse().ok()?,
            "anim" => t.model_anim = v.parse().ok()?,
            "activeanim" => t.model_anim2 = v.parse().ok()?,
            "orthog" => t.orthog = v == "1",
            "font" => t.font = v.parse().ok()?,
            "text" => t.text = unesc(v),
            "activetext" => t.text2 = unesc(v),
            "lineheight" => t.line_height = v.parse().ok()?,
            "halign" => t.h_align = v.parse().ok()?,
            "valign" => t.v_align = v.parse().ok()?,
            "shadowed" => t.shadow = v == "1",
            "colour" => t.colour = parse_u32(v)? as i32,
            "colour2" => t.colour2 = parse_u32(v)? as i32,
            "colourover" => t.colour_over = parse_u32(v)? as i32,
            "colour2over" => t.colour2_over = parse_u32(v)? as i32,
            "linewidth" => t.line_width = v.parse().ok()?,
            "targetverb" => t.target_verb = unesc(v),
            "targetbase" => t.target_base = unesc(v),
            "buttontext" => t.button_text = unesc(v),
            "baseop" => t.base_op_name = unesc(v),
            "dragdeadzone" => t.dragdeadzone = v.parse().ok()?,
            "dragdeadtime" => t.dragdeadtime = v.parse().ok()?,
            "draggable" => t.draggable_behavior = v == "1",
            k if k.starts_with("bg") => {
                if t.inv_has_bg.is_empty() { t.inv_has_bg = vec![false; 20]; t.inv_background_x = vec![0; 20]; t.inv_background_y = vec![0; 20]; t.inv_background = vec![-1; 20]; }
                let idx: usize = k[2..].parse().ok()?;
                let mut p = v.split(',');
                t.inv_background_x[idx] = p.next()?.trim().parse().ok()?;
                t.inv_background_y[idx] = p.next()?.trim().parse().ok()?;
                t.inv_background[idx] = p.next()?.trim().parse().ok()?;
                t.inv_has_bg[idx] = true;
            }
            k if k.starts_with("iop") => {
                let i: usize = k[3..].parse::<usize>().ok()? - 1;
                if i < 5 { t.iop[i] = Some(unesc(v)); }
            }
            k if k.starts_with("op") && k[2..].chars().all(|c| c.is_ascii_digit()) => {
                t.op_names.push(Some(unesc(v)));
            }
            k if is_hook(k) => set_hook(&mut t, k, v)?,
            k if is_list(k) => set_list(&mut t, k, v)?,
            _ => return None, // unknown key — fail to .dat
        }
    }
    let _ = get; // (reserved for future cross-field checks)
    // Inv components need link arrays sized like decode does.
    if t.type_ == 2 || t.type_ == 7 {
        let slots = (t.width * t.height).max(0) as usize;
        t.link_obj_type = vec![0; slots];
        t.link_obj_number = vec![0; slots];
    }
    Some(t)
}

fn layer_ref(parent: i32, layer_id: i32) -> String {
    // Same-interface layer → com_{child}; else raw global id.
    if (layer_id & 0xFFFF_0000_u32 as i32) == (parent & 0xFFFF_0000_u32 as i32) {
        format!("com_{}", layer_id & 0xFFFF)
    } else {
        layer_id.to_string()
    }
}

fn parse_layer(parent: i32, v: &str) -> Option<i32> {
    if let Some(child) = v.strip_prefix("com_") {
        Some((parent & 0xFFFF_0000_u32 as i32) | child.parse::<i32>().ok()?)
    } else {
        v.parse().ok()
    }
}

fn parse_model(v: &str) -> Option<i32> {
    if v == "-1" { return Some(-1); }
    v.strip_prefix("com_i").and_then(|s| s.parse().ok())
}

fn hexc(c: i32) -> String { format!("{:#08x}", c as u32) }
fn parse_u32(v: &str) -> Option<u32> {
    if let Some(h) = v.strip_prefix("0x") { u32::from_str_radix(h, 16).ok() } else { v.parse().ok() }
}

fn esc(s: &str) -> String { s.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "\\r") }
fn unesc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'), Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'), Some(o) => { out.push('\\'); out.push(o); }
                None => out.push('\\'),
            }
        } else { out.push(c); }
    }
    out
}

const HOOKS: &[&str] = &[
    "onload","onmouseover","onmouseleave","ontargetleave","ontargetenter","onvartransmit",
    "oninvtransmit","onstattransmit","ontimer","onop","onmouserepeat","onclick","onclickrepeat",
    "onrelease","onhold","ondrag","ondragcomplete","onscrollwheel",
];
const LISTS: &[&str] = &["onvartransmitlist","oninvtransmitlist","onstattransmitlist"];
fn is_hook(k: &str) -> bool { HOOKS.contains(&k) }
fn is_list(k: &str) -> bool { LISTS.contains(&k) }

fn parse_hook(v: &str) -> Option<Vec<HookArg>> {
    let mut out = Vec::new();
    for part in split_unescaped_commas(v) {
        // No trim — string args keep leading/trailing whitespace.
        let (kind, val) = part.split_once(':')?;
        out.push(match kind {
            "i" => HookArg::Int(val.trim().parse().ok()?),
            "s" => HookArg::Str(unesc_arg(val)),
            _ => return None,
        });
    }
    Some(out)
}

/// Split on commas that are not preceded by a backslash (escaped commas
/// belong to a string arg). Returns the unescaping responsibility to the
/// caller via `unesc_arg`.
fn split_unescaped_commas(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            cur.push('\\');
            if let Some(n) = chars.next() { cur.push(n); }
        } else if c == ',' {
            parts.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    parts.push(cur);
    parts
}

/// Like `esc` but also escapes commas (hook arg separator). Backslash is
/// escaped first so the escapes we add aren't re-escaped.
fn esc_arg(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "\\r").replace(',', "\\,")
}
/// Single-pass inverse of `esc_arg` (handles \\, \n, \r, \,).
fn unesc_arg(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(',') => out.push(','),
                Some(o) => { out.push('\\'); out.push(o); }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn set_hook(t: &mut IfType, k: &str, v: &str) -> Option<()> {
    let h = Some(parse_hook(v)?);
    match k {
        "onload" => t.onload = h, "onmouseover" => t.onmouseover = h,
        "onmouseleave" => t.onmouseleave = h, "ontargetleave" => t.ontargetleave = h,
        "ontargetenter" => t.ontargetenter = h, "onvartransmit" => t.onvartransmit = h,
        "oninvtransmit" => t.oninvtransmit = h, "onstattransmit" => t.onstattransmit = h,
        "ontimer" => t.ontimer = h, "onop" => t.onop = h, "onmouserepeat" => t.onmouserepeat = h,
        "onclick" => t.onclick = h, "onclickrepeat" => t.onclickrepeat = h,
        "onrelease" => t.onrelease = h, "onhold" => t.onhold = h, "ondrag" => t.ondrag = h,
        "ondragcomplete" => t.ondragcomplete = h, "onscrollwheel" => t.onscrollwheel = h,
        _ => return None,
    }
    Some(())
}

fn set_list(t: &mut IfType, k: &str, v: &str) -> Option<()> {
    let list: Vec<i32> = v.split_whitespace().map(|x| x.parse::<i32>()).collect::<Result<_, _>>().ok()?;
    let l = Some(list);
    match k {
        "onvartransmitlist" => t.onvartransmitlist = l,
        "oninvtransmitlist" => t.oninvtransmitlist = l,
        "onstattransmitlist" => t.onstattransmitlist = l,
        _ => return None,
    }
    Some(())
}
