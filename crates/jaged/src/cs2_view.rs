//! CS2 (clientscript) disassembler view. Delegates the heavy lifting to
//! [`cache::cs2_pretty`] — this file just owns view state (mode + search + folds) and
//! turns the [`cache::cs2_pretty::Line`] list into egui labels with addressing and
//! stack-depth columns.

use std::collections::HashSet;

use cache::config::{LocType, NpcType, ObjType, group as config_group};
use cache::cs2::ClientScript;
use cache::cs2_opcodes::is_branch;
use cache::cs2_pretty::{compute_labels, pretty_with, Line, NameResolver};
use cache::{Cache, CONFIG_ARCHIVE};
use eframe::egui;

const CLIENTSCRIPTS_ARCHIVE: u8 = 12;

pub struct Cs2View {
    pub mode: ViewMode,
    pub search: String,
    pub show_addrs: bool,
    pub show_stack: bool,
    /// Collapsed label numbers — when set, the listing hides the lines until the next
    /// label is encountered. Persisted across script switches so quickly bouncing
    /// between two scripts doesn't lose UI state.
    pub collapsed: HashSet<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// 1:1 with bytecode: one source pc per line, no folds.
    Raw,
    /// Folded mnemonics + sigils, single flat listing.
    Pretty,
    /// Folded + each label-bounded basic block wrapped in its own visual frame, with
    /// back-references showing which other blocks branch in. Closest thing we have to
    /// source-level CS2 without a full decompiler.
    Blocks,
}

impl Default for Cs2View {
    fn default() -> Self {
        Self {
            mode: ViewMode::Pretty,
            search: String::new(),
            show_addrs: true,
            show_stack: true,
            collapsed: HashSet::new(),
        }
    }
}

pub fn draw(
    ui: &mut egui::Ui,
    cache: &mut Cache,
    group_id: u32,
    bytes: &[u8],
    view: &mut Cs2View,
) {
    let Some(script) = ClientScript::decode(bytes) else {
        ui.colored_label(egui::Color32::LIGHT_RED, "script decode failed (buffer too short)");
        return;
    };

    header(ui, group_id, &script);
    toolbar(ui, view);
    ui.separator();

    let labels = compute_labels(&script);
    let lines = {
        let mut resolver = CacheResolver::new(cache);
        match view.mode {
            ViewMode::Pretty | ViewMode::Blocks => pretty_with(&script, &labels, &mut resolver),
            ViewMode::Raw => raw_lines(&script, &labels),
        }
    };
    // Reverse-index of branch predecessors per label, used only by blocks view.
    let preds: std::collections::BTreeMap<u32, Vec<u32>> = build_predecessors(&script, &labels);

    egui::ScrollArea::vertical()
        .id_salt("cs2_disasm_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| match view.mode {
            ViewMode::Blocks => render_blocks(ui, &script, &labels, &preds, &lines, view),
            _ => render_lines(ui, &script, &labels, &lines, view),
        });
}

/// For each labeled block, list the label numbers of every block that branches into
/// it. Drives the `← from label_NN` annotation in the blocks view.
fn build_predecessors(
    script: &ClientScript,
    labels: &std::collections::BTreeMap<usize, u32>,
) -> std::collections::BTreeMap<u32, Vec<u32>> {
    let mut out: std::collections::BTreeMap<u32, Vec<u32>> = std::collections::BTreeMap::new();
    // Sorted vec of (pc, label_num) for finding which block a pc belongs to.
    let mut sorted_labels: Vec<(usize, u32)> =
        labels.iter().map(|(&pc, &n)| (pc, n)).collect();
    sorted_labels.sort_by_key(|(pc, _)| *pc);

    let block_of = |pc: usize| -> Option<u32> {
        // The block containing `pc` is the latest label whose pc <= the given pc.
        let mut current = None;
        for &(lpc, n) in &sorted_labels {
            if lpc > pc {
                break;
            }
            current = Some(n);
        }
        current
    };

    for (pc, &op) in script.instructions.iter().enumerate() {
        if !is_branch(op) {
            continue;
        }
        let target = (pc as i64 + 1 + script.int_operands[pc] as i64) as usize;
        let Some(&target_label) = labels.get(&target) else { continue };
        let Some(src_label) = block_of(pc) else { continue };
        let preds = out.entry(target_label).or_default();
        if !preds.contains(&src_label) {
            preds.push(src_label);
        }
    }
    out
}

fn header(ui: &mut egui::Ui, group_id: u32, script: &ClientScript) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("script #{group_id}")).strong());
        if let Some(name) = &script.name {
            ui.label(egui::RichText::new(format!("· {name}")).weak());
        }
        ui.separator();
        ui.label(
            egui::RichText::new(format!(
                "{} ops · {} int locals · {} str locals · {} int args · {} str args",
                script.instructions.len(),
                script.int_local_count,
                script.string_local_count,
                script.int_arg_count,
                script.string_arg_count,
            ))
            .weak()
            .small(),
        );
    });
}

fn toolbar(ui: &mut egui::Ui, view: &mut Cs2View) {
    ui.horizontal(|ui| {
        ui.label("view:");
        ui.selectable_value(&mut view.mode, ViewMode::Pretty, "pretty");
        ui.selectable_value(&mut view.mode, ViewMode::Blocks, "blocks");
        ui.selectable_value(&mut view.mode, ViewMode::Raw, "raw");
        ui.separator();
        ui.checkbox(&mut view.show_addrs, "addrs");
        ui.checkbox(&mut view.show_stack, "stack");
        ui.separator();
        ui.label("search:");
        ui.add(
            egui::TextEdit::singleline(&mut view.search)
                .desired_width(140.0)
                .hint_text("substring filter"),
        );
        if !view.search.is_empty() && ui.small_button("✕").clicked() {
            view.search.clear();
        }
    });
}

fn render_lines(
    ui: &mut egui::Ui,
    script: &ClientScript,
    labels: &std::collections::BTreeMap<usize, u32>,
    lines: &[Line],
    view: &mut Cs2View,
) {
    let body = egui::FontId::monospace(12.0);
    let label_color = egui::Color32::from_rgb(255, 200, 100);
    let addr_color = egui::Color32::from_rgb(95, 110, 130);
    let stack_color = egui::Color32::from_rgb(110, 140, 170);
    let dim = egui::Color32::from_rgb(150, 160, 175);
    let needle = view.search.to_lowercase();
    let n_ops = script.instructions.len();

    // Per-label collapse state — clicking a label header toggles. Iterate logical
    // lines and emit labels at the right source-pc boundaries.
    let mut hidden_until_next_label = false;
    for line in lines {
        let primary_pc = line.addrs[0];
        if let Some(&label_n) = labels.get(&primary_pc) {
            // Label header. Always shown so collapsed sections can be re-expanded.
            let block_size = block_size_from(lines, line, labels);
            let is_collapsed = view.collapsed.contains(&label_n);
            let arrow = if is_collapsed { "▶" } else { "▼" };
            let header_text = format!(
                "{arrow} label_{label_n:02}:   ({block_size} {})",
                if block_size == 1 { "op" } else { "ops" }
            );
            if ui
                .selectable_label(false, egui::RichText::new(header_text).font(body.clone()).color(label_color))
                .clicked()
            {
                if is_collapsed {
                    view.collapsed.remove(&label_n);
                } else {
                    view.collapsed.insert(label_n);
                }
            }
            hidden_until_next_label = view.collapsed.contains(&label_n);
        }
        if hidden_until_next_label {
            continue;
        }

        // Search filter — match against mnemonic + operand (case-insensitive).
        if !needle.is_empty() {
            let hay = format!("{} {}", line.mnemonic, line.operand).to_lowercase();
            if !hay.contains(&needle) {
                continue;
            }
        }

        ui.horizontal(|ui| {
            if view.show_addrs {
                ui.label(
                    egui::RichText::new(format_addrs(&line.addrs))
                        .font(body.clone())
                        .color(addr_color),
                );
            }
            // Mnemonic in default text, fixed-width.
            ui.label(
                egui::RichText::new(format!("  {:<14}", line.mnemonic))
                    .font(body.clone()),
            );
            // Operand in a slightly dim colour so the mnemonic reads first.
            ui.label(
                egui::RichText::new(&line.operand)
                    .font(body.clone())
                    .color(dim),
            );
            if view.show_stack {
                let s = format_stack(line.int_depth, line.str_depth);
                ui.label(
                    egui::RichText::new(format!("    {s}"))
                        .font(body.clone())
                        .color(stack_color)
                        .small(),
                );
            }
            if let Some(ann) = &line.annotation {
                ui.label(
                    egui::RichText::new(format!("    // {ann}"))
                        .font(body.clone())
                        .color(stack_color)
                        .small(),
                );
            }
            // For raw branches in raw mode, append the absolute target.
            if matches!(view.mode, ViewMode::Raw) && line.addrs.len() == 1 {
                let pc = line.addrs[0];
                if pc < n_ops && is_branch(script.instructions[pc]) {
                    let target = (pc as i64 + 1 + script.int_operands[pc] as i64) as i64;
                    ui.label(
                        egui::RichText::new(format!("// → {target:04}"))
                            .font(body.clone())
                            .color(stack_color)
                            .small(),
                    );
                }
            }
        });
    }
}

/// Render mode where each label-bounded block sits inside its own egui group. Pre-
/// header lines (before the first label) form the "entry" block. The header text
/// includes incoming predecessor labels so users can trace control flow without
/// chasing branch targets.
fn render_blocks(
    ui: &mut egui::Ui,
    script: &ClientScript,
    labels: &std::collections::BTreeMap<usize, u32>,
    preds: &std::collections::BTreeMap<u32, Vec<u32>>,
    lines: &[Line],
    view: &mut Cs2View,
) {
    let label_color = egui::Color32::from_rgb(255, 200, 100);
    let needle = view.search.to_lowercase();

    // Pre-bucket lines into blocks keyed by their starting label (or `None` for entry).
    let mut blocks: Vec<(Option<u32>, Vec<&Line>)> = vec![(None, Vec::new())];
    for line in lines {
        let primary = line.addrs[0];
        if let Some(&n) = labels.get(&primary) {
            blocks.push((Some(n), Vec::new()));
        }
        blocks.last_mut().unwrap().1.push(line);
    }

    for (label, block_lines) in &blocks {
        if block_lines.is_empty() {
            continue;
        }
        egui::Frame::group(ui.style())
            .corner_radius(4.0)
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                let header = match label {
                    None => "entry".to_owned(),
                    Some(n) => match preds.get(n) {
                        Some(ps) if !ps.is_empty() => format!(
                            "label_{n:02}    ← from {}",
                            ps.iter()
                                .map(|p| format!("label_{p:02}"))
                                .collect::<Vec<_>>()
                                .join(", "),
                        ),
                        _ => format!("label_{n:02}"),
                    },
                };
                let is_collapsed = label.is_some_and(|n| view.collapsed.contains(&n));
                let arrow = if is_collapsed { "▶" } else { "▼" };
                let header_text =
                    format!("{arrow} {header}    ({} ops)", block_lines.len());
                if ui
                    .selectable_label(
                        false,
                        egui::RichText::new(header_text)
                            .font(egui::FontId::monospace(12.0))
                            .color(label_color),
                    )
                    .clicked()
                {
                    if let Some(n) = label {
                        if is_collapsed {
                            view.collapsed.remove(n);
                        } else {
                            view.collapsed.insert(*n);
                        }
                    }
                }
                if is_collapsed {
                    return;
                }
                for line in block_lines {
                    if !needle.is_empty() {
                        let hay =
                            format!("{} {}", line.mnemonic, line.operand).to_lowercase();
                        if !hay.contains(&needle) {
                            continue;
                        }
                    }
                    render_one_line(ui, script, line, view);
                }
            });
        ui.add_space(2.0);
    }
}

/// One-line renderer factored out so both flat and blocks views share formatting.
fn render_one_line(
    ui: &mut egui::Ui,
    script: &ClientScript,
    line: &Line,
    view: &Cs2View,
) {
    let body = egui::FontId::monospace(12.0);
    let addr_color = egui::Color32::from_rgb(95, 110, 130);
    let stack_color = egui::Color32::from_rgb(110, 140, 170);
    let dim = egui::Color32::from_rgb(150, 160, 175);
    let n_ops = script.instructions.len();
    ui.horizontal(|ui| {
        if view.show_addrs {
            ui.label(
                egui::RichText::new(format_addrs(&line.addrs))
                    .font(body.clone())
                    .color(addr_color),
            );
        }
        ui.label(
            egui::RichText::new(format!("  {:<14}", line.mnemonic))
                .font(body.clone()),
        );
        ui.label(
            egui::RichText::new(&line.operand)
                .font(body.clone())
                .color(dim),
        );
        if view.show_stack {
            let s = format_stack(line.int_depth, line.str_depth);
            ui.label(
                egui::RichText::new(format!("    {s}"))
                    .font(body.clone())
                    .color(stack_color)
                    .small(),
            );
        }
        if let Some(ann) = &line.annotation {
            ui.label(
                egui::RichText::new(format!("    // {ann}"))
                    .font(body.clone())
                    .color(stack_color)
                    .small(),
            );
        }
        if matches!(view.mode, ViewMode::Raw) && line.addrs.len() == 1 {
            let pc = line.addrs[0];
            if pc < n_ops && is_branch(script.instructions[pc]) {
                let target = (pc as i64 + 1 + script.int_operands[pc] as i64) as i64;
                ui.label(
                    egui::RichText::new(format!("// → {target:04}"))
                        .font(body.clone())
                        .color(stack_color)
                        .small(),
                );
            }
        }
    });
}

/// Address column. Single pc: `0000`. Folded pair: `0000-0001`. Folded triple: `0000-0002`.
fn format_addrs(addrs: &[usize]) -> String {
    if addrs.len() == 1 {
        format!("{:04}", addrs[0])
    } else {
        format!("{:04}-{:04}", addrs[0], addrs[addrs.len() - 1])
    }
}

fn format_stack(int_d: Option<i32>, str_d: Option<i32>) -> String {
    let i = int_d.map_or_else(|| "?".to_owned(), |d| d.to_string());
    let s = str_d.map_or_else(|| "?".to_owned(), |d| d.to_string());
    format!("[i:{i} s:{s}]")
}

/// Count of logical lines until the next label header. Used for the collapsed
/// summary so users can see block size at a glance.
fn block_size_from(
    lines: &[Line],
    from: &Line,
    labels: &std::collections::BTreeMap<usize, u32>,
) -> usize {
    let start_pc = from.addrs[0];
    let mut count = 0;
    for line in lines {
        let pc = line.addrs[0];
        if pc < start_pc {
            continue;
        }
        if pc != start_pc && labels.contains_key(&pc) {
            break;
        }
        count += 1;
    }
    count
}

/// Raw mode: one line per source opcode, no folds. Useful for cross-referencing with
/// the Java client's `ScriptRunner` switch.
fn raw_lines(
    script: &ClientScript,
    labels: &std::collections::BTreeMap<usize, u32>,
) -> Vec<Line> {
    // Trivial wrapper: each pc gets its own Line via cs2_pretty's single-op emit path.
    // Easiest to just re-run pretty but disable folding by inserting a label at every
    // op so the guard never lets the fold happen. Cheaper: re-implement here.
    use cache::cs2::OP_PUSH_CONST_STRING;
    use cache::cs2_opcodes::{
        branch_keyword, is_conditional_branch, is_gosub, mnemonic, operand_kind,
        stack_delta, OperandKind,
    };

    let mut out = Vec::with_capacity(script.instructions.len());
    let mut int_depth: Option<i32> = Some(i32::from(script.int_arg_count));
    let mut str_depth: Option<i32> = Some(i32::from(script.string_arg_count));
    for (pc, &op) in script.instructions.iter().enumerate() {
        let mn = if is_conditional_branch(op) {
            format!("if {}", branch_keyword(op))
        } else {
            mnemonic(op).to_owned()
        };
        let opnd = if op == OP_PUSH_CONST_STRING {
            format!("{:?}", script.string_operands[pc])
        } else {
            let operand = script.int_operands[pc];
            match operand_kind(op) {
                OperandKind::Filler => operand.to_string(),
                OperandKind::Int => operand.to_string(),
                OperandKind::String => format!("{:?}", script.string_operands[pc]),
                OperandKind::VarpId => format!("@varp[{operand}]"),
                OperandKind::VarbitId => format!("@varbit[{operand}]"),
                OperandKind::VarcIntId => format!("@varc_int[{operand}]"),
                OperandKind::VarcStrId => format!("@varc_str[{operand}]"),
                OperandKind::LocalInt => format!("%il{operand}"),
                OperandKind::LocalStr => format!("%sl{operand}"),
                OperandKind::BranchOffset => {
                    let target = (pc as i64 + 1 + operand as i64) as usize;
                    labels
                        .get(&target)
                        .map_or_else(|| format!("{operand:+}"), |n| format!("→label_{n:02}"))
                }
                OperandKind::ScriptId => {
                    if is_gosub(op) {
                        format!("→script #{operand}")
                    } else {
                        operand.to_string()
                    }
                }
                OperandKind::ArraySlot => format!("array[{}]", operand & 0xFF),
                OperandKind::ArrayDef => {
                    let id = (operand >> 16) & 0xFFFF;
                    let typ = operand & 0xFFFF;
                    format!("array[{id}] type={typ}")
                }
                OperandKind::JoinCount => format!("n={operand}"),
                OperandKind::SecondaryFlag => operand.to_string(),
            }
        };
        match stack_delta(op) {
            Some((id, sd)) => {
                if let Some(d) = int_depth.as_mut() {
                    *d += id;
                }
                if let Some(d) = str_depth.as_mut() {
                    *d += sd;
                }
            }
            None => {
                int_depth = None;
                str_depth = None;
            }
        }
        out.push(Line {
            addrs: vec![pc],
            mnemonic: mn,
            operand: opnd,
            int_depth,
            str_depth,
            annotation: None,
        });
    }
    out
}

/// [`NameResolver`] backed by the live cache. Decodes the relevant config record on
/// each lookup; cheap relative to the rest of the disasm pipeline and keeps state
/// minimal. Panics in the decoder are swallowed so a broken config entry doesn't
/// kill the script view.
struct CacheResolver<'a> {
    cache: &'a mut Cache,
}

impl<'a> CacheResolver<'a> {
    fn new(cache: &'a mut Cache) -> Self {
        Self { cache }
    }

    fn config_record(&mut self, group: u32, id: i32) -> Option<Vec<u8>> {
        if id < 0 {
            return None;
        }
        let files = self.cache.read_files(CONFIG_ARCHIVE, group).ok().flatten()?;
        files.into_iter().find(|(fid, _)| *fid == id).map(|(_, b)| b)
    }
}

impl NameResolver for CacheResolver<'_> {
    fn obj_name(&mut self, id: i32) -> Option<String> {
        let bytes = self.config_record(config_group::OBJ, id)?;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let o = ObjType::decode(id, &bytes);
            if o.name.is_empty() { None } else { Some(o.name) }
        }))
        .ok()
        .flatten()
    }

    fn npc_name(&mut self, id: i32) -> Option<String> {
        let bytes = self.config_record(config_group::NPC, id)?;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let n = NpcType::decode(id, &bytes);
            if n.name.is_empty() { None } else { Some(n.name) }
        }))
        .ok()
        .flatten()
    }

    fn loc_name(&mut self, id: i32) -> Option<String> {
        let bytes = self.config_record(config_group::LOC, id)?;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let l = LocType::decode(id, &bytes);
            if l.name.is_empty() { None } else { Some(l.name) }
        }))
        .ok()
        .flatten()
    }

    fn seq_name(&mut self, _id: i32) -> Option<String> {
        // SeqType has no name in rev1 — return None.
        None
    }

    fn script_name(&mut self, id: i32) -> Option<String> {
        if id < 0 {
            return None;
        }
        let bytes = self
            .cache
            .read_group(CLIENTSCRIPTS_ARCHIVE, id as u32)
            .ok()
            .flatten()?;
        ClientScript::decode(&bytes).and_then(|s| s.name)
    }
}
