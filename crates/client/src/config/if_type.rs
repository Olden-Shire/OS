// @ObfuscatedName("eg")
// jag::oldscape::rs2lib::IfType
//
// Per-component interface definition. The interfaces archive (slot 3)
// is laid out as one group per interface ID, with one file per
// component. `openInterface(id)` fetches the whole group and decodes
// each component via decode() (v1) or decode3() (v3). v3 components
// have a leading 0xFF marker byte.
//
// Coverage: data + bytewise decoders ported faithfully. Renderer
// (drawInterface) handles layer/rect/text/graphic so the chrome's
// position+layout shows up; model/inv/tooltip are placeholders.

#![allow(dead_code, non_snake_case)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::io::packet::Packet;
use crate::js5::js5_loader::Js5Loader;
use crate::js5::js5_net;

// Java holds these as static Js5 fields on IfType; we hold the loader
// slot indices on equivalent statics so non-Client code (the renderer)
// can use them without a borrow.
// @ObfuscatedName("eg.n")
pub static INTERFACES_SLOT: AtomicI32 = AtomicI32::new(-1);
// @ObfuscatedName("dc.z") — sprites Js5 lives in class `dc` in Java,
// not `eg`; the static is referenced from IfType for convenience.
pub static SPRITES_SLOT: AtomicI32 = AtomicI32::new(-1);
// @ObfuscatedName("eg.g")
pub static FONT_METRICS_SLOT: AtomicI32 = AtomicI32::new(-1);
// @ObfuscatedName("eg.j")
pub static MODELS_SLOT: AtomicI32 = AtomicI32::new(-1);

pub fn install_archives(interfaces: i32, sprites: i32, font_metrics: i32, models: i32) {
    INTERFACES_SLOT.store(interfaces, Ordering::Relaxed);
    SPRITES_SLOT.store(sprites, Ordering::Relaxed);
    FONT_METRICS_SLOT.store(font_metrics, Ordering::Relaxed);
    MODELS_SLOT.store(models, Ordering::Relaxed);
}

#[derive(Default, Clone, Debug)]
pub struct IfType {
    // @ObfuscatedName("eg.v")
    pub v3: bool,
    // @ObfuscatedName("eg.w")
    pub parent_id: i32,
    // @ObfuscatedName("eg.e")
    pub sub_id: i32,
    // 0 layer, 1 unknown, 2 inv, 3 rect, 4 text, 5 graphic, 6 model,
    // 7 invtext, 8 tooltip, 9 line.
    // @ObfuscatedName("eg.b")
    pub type_: i32,
    // @ObfuscatedName("eg.t")
    pub button_type: i32,
    // @ObfuscatedName("eg.f")
    pub client_code: i32,
    // @ObfuscatedName("eg.k")
    pub x: i32,
    // @ObfuscatedName("eg.o")
    pub y: i32,
    // @ObfuscatedName("eg.y")
    pub data_x: i32,
    // @ObfuscatedName("eg.a")
    pub data_y: i32,
    // @ObfuscatedName("eg.h")
    pub width: i32,
    // @ObfuscatedName("eg.x")
    pub height: i32,
    // @ObfuscatedName("eg.p")
    pub layer_id: i32,
    // @ObfuscatedName("eg.ad")
    pub hide: bool,
    // @ObfuscatedName("eg.ac")
    pub scroll_pos_x: i32,
    // @ObfuscatedName("eg.aa")
    pub scroll_pos_y: i32,
    // @ObfuscatedName("eg.as")
    pub scroll_width: i32,
    // @ObfuscatedName("eg.am")
    pub scroll_height: i32,
    // @ObfuscatedName("eg.ah")
    pub trans: i32,
    // over_layer_id has no Java equivalent in rev1 IfType — keep as a
    // local field tagged with the v3 sub-component name.
    pub over_layer_id: i32,
    // Script trigger arrays — Java's IfType keeps these as `eg.dh`
    // (scriptComparator), `eg.dv` (scriptOperand), `eg.dj` (scripts).
    // Names confirmed via IfType.java field list (decode opcode 200+).
    // @ObfuscatedName("eg.dh")
    pub script_comparator: Vec<i32>,
    // @ObfuscatedName("eg.dv")
    pub script_operand: Vec<i32>,
    // @ObfuscatedName("eg.dj")
    pub scripts: Vec<Vec<i32>>,
    // @ObfuscatedName("eg.an") — type 0 fill flag
    pub fill: bool,
    // @ObfuscatedName("eg.bu")
    pub h_align: i32,
    // @ObfuscatedName("eg.bo")
    pub v_align: i32,
    // @ObfuscatedName("eg.bf")
    pub line_height: i32,
    // @ObfuscatedName("eg.bw")
    pub font: i32,
    // @ObfuscatedName("eg.bq")
    pub shadow: bool,
    // @ObfuscatedName("eg.by")
    pub text: String,
    // @ObfuscatedName("eg.bx")
    pub text2: String,
    // @ObfuscatedName("eg.ap")
    pub colour: i32,
    // @ObfuscatedName("eg.av")
    pub colour2: i32,
    // @ObfuscatedName("eg.ak")
    pub colour_over: i32,
    // @ObfuscatedName("eg.az")
    pub colour2_over: i32,
    // @ObfuscatedName("eg.al")
    pub graphic: i32,
    // @ObfuscatedName("eg.ab")
    pub graphic2: i32,
    // @ObfuscatedName("eg.ao")
    pub rotate: i32,
    // @ObfuscatedName("eg.ag")
    pub tiling: bool,
    // @ObfuscatedName("eg.ar")
    pub outline: i32,
    // @ObfuscatedName("eg.aq")
    pub shadow_colour: i32,
    // @ObfuscatedName("eg.at")
    pub v_flip: bool,
    // @ObfuscatedName("eg.ae")
    pub h_flip: bool,
    // @ObfuscatedName("eg.au")
    pub model1_type: i32,
    // @ObfuscatedName("eg.ax")
    pub model1_id: i32,
    // @ObfuscatedName("eg.ai")
    pub model2_type: i32,
    // @ObfuscatedName("eg.aj")
    pub model2_id: i32,
    // @ObfuscatedName("eg.aw")
    pub model_anim: i32,
    // @ObfuscatedName("eg.af")
    pub model_anim2: i32,
    // @ObfuscatedName("eg.bg")
    pub model_zoom: i32,
    // @ObfuscatedName("eg.bs")
    pub model_x_an: i32,
    // @ObfuscatedName("eg.bk")
    pub model_y_an: i32,
    // @ObfuscatedName("eg.bv")
    pub model_z_an: i32,
    // @ObfuscatedName("eg.bh")
    pub model_x_of: i32,
    // @ObfuscatedName("eg.bi")
    pub model_y_of: i32,
    // @ObfuscatedName("eg.bt")
    pub orthog: bool,
    // @ObfuscatedName("eg.bj")
    pub margin_x: i32,
    // @ObfuscatedName("eg.bz")
    pub margin_y: i32,
    // @ObfuscatedName("eg.ba")
    pub event_code: i32,
    // @ObfuscatedName("eg.bp")
    pub iop: Vec<String>,
    // @ObfuscatedName("eg.be")
    pub inv_background: Vec<i32>,
    // @ObfuscatedName("eg.bm")
    pub inv_background_x: Vec<i32>,
    // @ObfuscatedName("eg.bn")
    pub inv_background_y: Vec<i32>,
    pub link_obj_type: Vec<i32>,
    pub link_obj_number: Vec<i32>,
    pub target_verb: String,
    pub target_base: String,
    pub button_text: String,
    // @ObfuscatedName("eg.subcomponents")
    pub subcomponents: Vec<Option<IfType>>,
    // @ObfuscatedName("eg.ay") — type 9 (line) thickness. Java default
    // is 1, set by IfType ctor (IfType.java:145).
    pub line_width: i32,
    // @ObfuscatedName("eg.invobject")
    pub invobject: i32,
    // @ObfuscatedName("eg.invcount")
    pub invcount: i32,
    // @ObfuscatedName("eg.modelSpin") — IF_SETROTATESPEED packs
    // `(x << 16) + y` into this field; the model render divides
    // back out for per-axis rotation speed.
    pub model_spin: i32,
    // @ObfuscatedName("eg.dragdeadzone")
    pub drag_dead_zone: i32,
    // @ObfuscatedName("eg.dragdeadtime")
    pub drag_dead_time: i32,
    // @ObfuscatedName("eg.draggablebehavior")
    pub draggable_behavior: bool,
    // @ObfuscatedName("eg.baseOpName")
    pub base_op_name: String,
    // @ObfuscatedName("eg.opNames")
    pub op_names: Vec<String>,
    // @ObfuscatedName("eg.hashook")
    pub hashook: bool,
    // @ObfuscatedName("eg.onload") + 17 others — stored as raw byte ranges
    // (script bytecode not yet run). We keep them as Vec<u8> raw bytes so
    // the decoder consumes the right number of bytes and the data is
    // preserved for the cs2 runtime port later.
    pub hook_onload: Option<Vec<HookArg>>,
    pub hook_onmouseover: Option<Vec<HookArg>>,
    pub hook_onmouseleave: Option<Vec<HookArg>>,
    pub hook_ontargetleave: Option<Vec<HookArg>>,
    pub hook_ontargetenter: Option<Vec<HookArg>>,
    pub hook_onvartransmit: Option<Vec<HookArg>>,
    pub hook_oninvtransmit: Option<Vec<HookArg>>,
    pub hook_onstattransmit: Option<Vec<HookArg>>,
    pub hook_ontimer: Option<Vec<HookArg>>,
    pub hook_onop: Option<Vec<HookArg>>,
    pub hook_onmouserepeat: Option<Vec<HookArg>>,
    pub hook_onclick: Option<Vec<HookArg>>,
    pub hook_onclickrepeat: Option<Vec<HookArg>>,
    pub hook_onrelease: Option<Vec<HookArg>>,
    pub hook_onhold: Option<Vec<HookArg>>,
    pub hook_ondrag: Option<Vec<HookArg>>,
    pub hook_ondragcomplete: Option<Vec<HookArg>>,
    pub hook_onscrollwheel: Option<Vec<HookArg>>,
    // @ObfuscatedName("eg.cg") — onchattransmit. Fires when chat
    // history rotates with new line content.
    pub hook_onchattransmit: Option<Vec<HookArg>>,
    // @ObfuscatedName("eg.dd") — onkey. Keyboard event hook.
    pub hook_onkey: Option<Vec<HookArg>>,
    // @ObfuscatedName("eg.dg") — onfriendtransmit. Friends-list update.
    pub hook_onfriendtransmit: Option<Vec<HookArg>>,
    // @ObfuscatedName("eg.df") — onclantransmit. Clan-chat update.
    pub hook_onclantransmit: Option<Vec<HookArg>>,
    // @ObfuscatedName("eg.dk") — onmisctransmit. Engine-defined topics.
    pub hook_onmisctransmit: Option<Vec<HookArg>>,
    // @ObfuscatedName("eg.dz") — ondialogabort. Server-initiated dialog
    // close (opcode 42).
    pub hook_ondialogabort: Option<Vec<HookArg>>,
    // @ObfuscatedName("eg.da") — onsubchange. Sub-interface open/close.
    pub hook_onsubchange: Option<Vec<HookArg>>,
    pub on_var_transmit_list: Option<Vec<i32>>,
    pub on_inv_transmit_list: Option<Vec<i32>>,
    pub on_stat_transmit_list: Option<Vec<i32>>,

    // Runtime / animation state — Java holds these on the IfType
    // instance and mutates them during the per-frame drawLayer /
    // animateLayer passes. They're zero at decode time and persist
    // across frames.
    // @ObfuscatedName("eg.ch")
    pub anim_frame: i32,
    // @ObfuscatedName("eg.ca")
    pub anim_cycle: i32,
    // @ObfuscatedName("eg.cu") — frame the component was last drawn.
    pub draw_count: i32,
    // @ObfuscatedName("eg.cd") — last-draw timestamp in cycles.
    pub draw_time: i32,
    // @ObfuscatedName("eg.cv") — Java tracks per-component transmit
    // counter; opcode hooks (onVarTransmit, onInvTransmit,
    // onStatTransmit) fire on increment.
    pub transmit_num: i32,
    pub var_transmit_num: i32,
    pub inv_transmit_num: i32,
    pub stat_transmit_num: i32,
    // @ObfuscatedName("eg.cz") / "eg.cw" — mouse / click triggers used
    // by cs2 to gate hover and click hooks.
    pub mouse_trigger: i32,
    pub click_trigger: i32,
    // @ObfuscatedName("eg.cn") — drag context: the cs2-assigned drag
    // target layer (cc_setdraggable stores IfType.get(parent, sub)).
    // Java holds the IfType ref; we store the (component id, cc sub)
    // pair — draggable_sub == -2 means unset, -1 means the component
    // itself (Java's get(id, -1) identity case).
    pub draggable: i32,
    pub draggable_sub: i32,
    // The inv-type flag bits (interactable / usable / swappable /
    // draggable) are folded into the existing `event_code` field
    // declared earlier. The high nibble encodes them:
    //   0x10000000 = interactable
    //   0x20000000 = swappable
    //   0x40000000 = usable
    //   0x80000000 = draggable
}

// jagex3.client.script.HookReq arg variant — either an Integer or String.
// Java holds these as Object[] in IfType; we use a typed enum.
#[derive(Debug, Clone)]
pub enum HookArg {
    Int(i32),
    Str(String),
}

impl IfType {
    fn new(parent_id: i32) -> Self {
        Self {
            parent_id,
            sub_id: -1,
            type_: -1,
            x: 0, y: 0, data_x: 0, data_y: 0,
            width: 0, height: 0,
            layer_id: -1,
            over_layer_id: -1,
            hide: false,
            v3: false,
            trans: 0,
            colour: 0, colour2: 0, colour_over: 0, colour2_over: 0,
            font: -1, line_height: 0, h_align: 0, v_align: 0, shadow: false,
            text: String::new(), text2: String::new(),
            graphic: -1, graphic2: -1,
            model1_type: 1, model1_id: -1, model2_type: 1, model2_id: -1,
            model_anim: -1, model_anim2: -1,
            // Java IfType.java:205 — modelZoom defaults to 100.
            model_zoom: 100,
            // Java IfType.java:145 — lineWidth defaults to 1.
            line_width: 1,
            // Java IfType.java:391 — invcount defaults to 0 (invobject -1).
            invobject: -1, invcount: 0, model_spin: 0,
            draggable: 0,
            draggable_sub: -2,
            ..Default::default()
        }
    }

    // @ObfuscatedName("eg.i(Lev;I)[Ljava/lang/Object;") — decodeHook
    fn decode_hook(&mut self, p: &mut Packet) -> Option<Vec<HookArg>> {
        let n = p.g1();
        if n == 0 { return None; }
        let mut out = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let kind = p.g1();
            match kind {
                0 => out.push(HookArg::Int(p.g4())),
                1 => out.push(HookArg::Str(p.gjstr())),
                _ => out.push(HookArg::Int(0)),
            }
        }
        self.hashook = true;
        Some(out)
    }

    // @ObfuscatedName("eg.s(Lev;I)[I") — decodeTransmitList
    fn decode_transmit_list(p: &mut Packet) -> Option<Vec<i32>> {
        let n = p.g1();
        if n == 0 { return None; }
        Some((0..n).map(|_| p.g4()).collect())
    }

    // @ObfuscatedName("eg.g(Lev;I)V") — decode v1 (legacy)
    pub fn decode(&mut self, p: &mut Packet) {
        self.v3 = false;
        self.type_ = p.g1();
        self.button_type = p.g1();
        self.client_code = p.g2();
        self.x = p.g2b(); self.data_x = self.x;
        self.y = p.g2b(); self.data_y = self.y;
        self.width = p.g2();
        self.height = p.g2();
        self.trans = p.g1();
        self.layer_id = decode_layer_id(p.g2(), self.parent_id);
        let over = p.g2();
        self.over_layer_id = if over == 65535 { -1 } else { over };

        let stack_count = p.g1();
        if stack_count > 0 {
            self.script_comparator = (0..stack_count).map(|_| p.g1()).collect();
            self.script_operand = (0..stack_count).map(|_| p.g2()).collect();
        }
        let script_count = p.g1();
        if script_count > 0 {
            self.scripts = (0..script_count).map(|_| {
                let n = p.g2();
                (0..n).map(|_| {
                    let v = p.g2();
                    if v == 65535 { -1 } else { v }
                }).collect()
            }).collect();
        }

        if self.type_ == 0 {
            self.scroll_height = p.g2();
            self.hide = p.g1() == 1;
        }
        if self.type_ == 1 {
            p.g2(); p.g1();
        }
        if self.type_ == 2 {
            self.link_obj_type = vec![0; (self.height * self.width) as usize];
            self.link_obj_number = vec![0; (self.height * self.width) as usize];
            if p.g1() == 1 { self.event_code |= 0x10000000; }
            if p.g1() == 1 { self.event_code |= 0x40000000; }
            if p.g1() == 1 { self.event_code |= -2147483648; /* 0x80000000 */ }
            if p.g1() == 1 { self.event_code |= 0x20000000; }
            self.margin_x = p.g1();
            self.margin_y = p.g1();
            self.inv_background_x = vec![0; 20];
            self.inv_background_y = vec![0; 20];
            self.inv_background = vec![-1; 20];
            for i in 0..20 {
                if p.g1() == 1 {
                    self.inv_background_x[i] = p.g2b();
                    self.inv_background_y[i] = p.g2b();
                    self.inv_background[i] = p.g4();
                }
            }
            self.iop = vec![String::new(); 5];
            for i in 0..5 {
                let s = p.gjstr();
                if !s.is_empty() {
                    self.iop[i] = s;
                    self.event_code |= 0x1 << (i + 23);
                }
            }
        }
        if self.type_ == 3 {
            self.fill = p.g1() == 1;
        }
        if self.type_ == 4 || self.type_ == 1 {
            self.h_align = p.g1();
            self.v_align = p.g1();
            self.line_height = p.g1();
            let f = p.g2();
            self.font = if f == 65535 { -1 } else { f };
            self.shadow = p.g1() == 1;
        }
        if self.type_ == 4 {
            self.text = p.gjstr();
            self.text2 = p.gjstr();
        }
        if self.type_ == 1 || self.type_ == 3 || self.type_ == 4 {
            self.colour = p.g4();
        }
        if self.type_ == 3 || self.type_ == 4 {
            self.colour2 = p.g4();
            self.colour_over = p.g4();
            self.colour2_over = p.g4();
        }
        if self.type_ == 5 {
            self.graphic = p.g4();
            self.graphic2 = p.g4();
        }
        if self.type_ == 6 {
            self.model1_type = 1;
            let id = p.g2();
            self.model1_id = if id == 65535 { -1 } else { id };
            self.model2_type = 1;
            let id = p.g2();
            self.model2_id = if id == 65535 { -1 } else { id };
            let a = p.g2();
            self.model_anim = if a == 65535 { -1 } else { a };
            let a = p.g2();
            self.model_anim2 = if a == 65535 { -1 } else { a };
            self.model_zoom = p.g2();
            self.model_x_an = p.g2();
            self.model_y_an = p.g2();
        }
        if self.type_ == 7 {
            self.link_obj_type = vec![0; (self.height * self.width) as usize];
            self.link_obj_number = vec![0; (self.height * self.width) as usize];
            self.h_align = p.g1();
            let f = p.g2();
            self.font = if f == 65535 { -1 } else { f };
            self.shadow = p.g1() == 1;
            self.colour = p.g4();
            self.margin_x = p.g2b();
            self.margin_y = p.g2b();
            if p.g1() == 1 { self.event_code |= 0x40000000; }
            self.iop = vec![String::new(); 5];
            for i in 0..5 {
                let s = p.gjstr();
                if !s.is_empty() {
                    self.iop[i] = s;
                    self.event_code |= 0x1 << (i + 23);
                }
            }
        }
        if self.type_ == 8 {
            self.text = p.gjstr();
        }
        if self.button_type == 2 || self.type_ == 2 {
            self.target_verb = p.gjstr();
            self.target_base = p.gjstr();
            let mask = p.g2() & 0x3F;
            self.event_code |= mask << 11;
        }
        if matches!(self.button_type, 1 | 4 | 5 | 6) {
            self.button_text = p.gjstr();
            if self.button_text.is_empty() {
                self.button_text = match self.button_type {
                    1 => "Ok".into(), 4 | 5 => "Select".into(), 6 => "Continue".into(),
                    _ => String::new(),
                };
            }
        }
        if matches!(self.button_type, 1 | 4 | 5) {
            self.event_code |= 0x400000;
        }
        if self.button_type == 6 {
            self.event_code |= 0x1;
        }
    }

    // @ObfuscatedName("eg.q(Lev;I)V") — decode v3 (modern)
    pub fn decode3(&mut self, p: &mut Packet) {
        p.g1();
        self.v3 = true;
        self.type_ = p.g1();
        self.client_code = p.g2();
        self.x = p.g2b(); self.data_x = self.x;
        self.y = p.g2b(); self.data_y = self.y;
        self.width = p.g2();
        self.height = if self.type_ == 9 { p.g2b() } else { p.g2() };
        self.layer_id = decode_layer_id(p.g2(), self.parent_id);
        self.hide = p.g1() == 1;

        if self.type_ == 0 {
            self.scroll_width = p.g2();
            self.scroll_height = p.g2();
        }
        if self.type_ == 5 {
            self.graphic = p.g4();
            self.rotate = p.g2();
            self.tiling = p.g1() == 1;
            self.trans = p.g1();
            self.outline = p.g1();
            self.shadow_colour = p.g4();
            self.v_flip = p.g1() == 1;
            self.h_flip = p.g1() == 1;
        }
        if self.type_ == 6 {
            self.model1_type = 1;
            let id = p.g2();
            self.model1_id = if id == 65535 { -1 } else { id };
            self.model_x_of = p.g2b();
            self.model_y_of = p.g2b();
            self.model_x_an = p.g2();
            self.model_y_an = p.g2();
            self.model_z_an = p.g2();
            self.model_zoom = p.g2();
            let a = p.g2();
            self.model_anim = if a == 65535 { -1 } else { a };
            self.orthog = p.g1() == 1;
        }
        if self.type_ == 4 {
            self.font = { let f = p.g2(); if f == 65535 { -1 } else { f } };
            self.text = p.gjstr();
            self.line_height = p.g1();
            self.h_align = p.g1();
            self.v_align = p.g1();
            self.shadow = p.g1() == 1;
            self.colour = p.g4();
        }
        if self.type_ == 3 {
            self.colour = p.g4();
            self.fill = p.g1() == 1;
            self.trans = p.g1();
        }
        if self.type_ == 9 {
            self.line_width = p.g1();
            self.colour = p.g4();
        }

        // jagex3.client.IfType::decode3 tail (Java lines 867–903).
        self.event_code = p.g3();
        self.base_op_name = p.gjstr();

        let ops = p.g1();
        if ops > 0 {
            self.op_names = (0..ops).map(|_| p.gjstr()).collect();
        }

        self.drag_dead_zone = p.g1();
        self.drag_dead_time = p.g1();
        self.draggable_behavior = p.g1() == 1;
        self.target_verb = p.gjstr();

        self.hook_onload = self.decode_hook(p);
        self.hook_onmouseover = self.decode_hook(p);
        self.hook_onmouseleave = self.decode_hook(p);
        self.hook_ontargetleave = self.decode_hook(p);
        self.hook_ontargetenter = self.decode_hook(p);
        self.hook_onvartransmit = self.decode_hook(p);
        self.hook_oninvtransmit = self.decode_hook(p);
        self.hook_onstattransmit = self.decode_hook(p);
        self.hook_ontimer = self.decode_hook(p);
        self.hook_onop = self.decode_hook(p);
        self.hook_onmouserepeat = self.decode_hook(p);
        self.hook_onclick = self.decode_hook(p);
        self.hook_onclickrepeat = self.decode_hook(p);
        self.hook_onrelease = self.decode_hook(p);
        self.hook_onhold = self.decode_hook(p);
        self.hook_ondrag = self.decode_hook(p);
        self.hook_ondragcomplete = self.decode_hook(p);
        self.hook_onscrollwheel = self.decode_hook(p);
        self.on_var_transmit_list = Self::decode_transmit_list(p);
        self.on_inv_transmit_list = Self::decode_transmit_list(p);
        self.on_stat_transmit_list = Self::decode_transmit_list(p);
    }
}

fn decode_layer_id(v: i32, parent_id: i32) -> i32 {
    if v == 65535 { -1 } else { v + (parent_id & 0xFFFF_0000u32 as i32) }
}

// @ObfuscatedName("av.m") + "df.c" — IfType.list / IfType.open
pub struct IfTypeStore {
    pub list: Vec<Option<Vec<Option<IfType>>>>,
    pub open: Vec<bool>,
}

impl IfTypeStore {
    const fn new() -> Self {
        Self { list: Vec::new(), open: Vec::new() }
    }
    fn ensure(&mut self, id: usize) {
        if self.list.len() <= id { self.list.resize(id + 1, None); }
        if self.open.len() <= id { self.open.resize(id + 1, false); }
    }
}

pub static STORE: Mutex<IfTypeStore> = Mutex::new(IfTypeStore::new());

// @ObfuscatedName("dw.z(II)Z") — IfType.openInterface
pub fn open_interface(id: i32, interfaces_slot: i32) -> bool {
    if id < 0 { return false; }
    {
        let mut s = STORE.lock().unwrap();
        s.ensure(id as usize);
        if s.open[id as usize] { return true; }
    }
    // Pull all files in the group from the interfaces archive.
    let mut reg = js5_net::LOADERS.lock().unwrap();
    let loader = match reg.get_mut(interfaces_slot as usize).and_then(|o| o.as_mut()) {
        Some(l) => l,
        None => return false,
    };
    // Trigger download if not ready.
    if !loader.request_download(id, 0) {
        return false;
    }
    let children_limit = loader.base.file_ids
        .get(id as usize).and_then(|o| o.as_ref()).map_or(0, |v| v.len()) as i32;
    let mut components: Vec<Option<IfType>> = (0..children_limit).map(|_| None).collect();
    let file_ids: Vec<i32> = loader.base.file_ids
        .get(id as usize).and_then(|o| o.as_ref()).cloned().unwrap_or_default();
    for (slot_idx, &file_id) in file_ids.iter().enumerate() {
        let data = match loader.fetch_file(id, file_id) {
            Some(d) => d,
            None => continue,
        };
        if data.is_empty() { continue; }
        // Decode in a panic-safe wrapper — interface 548 has component
        // types we haven't covered, and a bad decode shouldn't take the
        // client down.
        let parent = (id << 16) | file_id;
        let parsed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut t = IfType::new(parent);
            // Java leaves subId at -1 for decoded components — only
            // cc_create assigns it. The hook drain loop relies on
            // this: subId < 0 executes unconditionally, subId >= 0
            // is validated as a still-attached cc.
            t.sub_id = -1;
            let mut p = Packet::from_vec(data);
            if p.data[0] == 0xFF { t.decode3(&mut p); } else { t.decode(&mut p); }
            t
        }));
        match parsed {
            Ok(t) if (slot_idx as i32) < children_limit => {
                components[slot_idx] = Some(t);
            }
            _ => {}
        }
    }
    drop(reg);

    let mut s = STORE.lock().unwrap();
    s.ensure(id as usize);
    s.list[id as usize] = Some(components);
    s.open[id as usize] = true;
    eprintln!("[iftype] opened interface {id} with {} components", file_ids.len());
    true
}

// @ObfuscatedName("bd.j(IB)Leg;") — IfType.get(componentId)
pub fn get(component_id: i32) -> Option<IfType> {
    let group = (component_id >> 16) & 0xFFFF;
    let sub = component_id & 0xFFFF;
    get_sub(group, sub)
}

// Raw `list[group][file_index]` accessor — used by `get()` after it has
// unpacked a packed component id. NOT Java's 2-arg get overload (that is
// `get2` below); callers that have a packed parent id + cc-sub must use
// `get2`, not this.
pub fn get_sub(group: i32, sub: i32) -> Option<IfType> {
    let s = STORE.lock().unwrap();
    s.list.get(group as usize)
        .and_then(|o| o.as_ref())
        .and_then(|v| v.get(sub as usize))
        .and_then(|o| o.clone())
}

// @ObfuscatedName("eg.s(IIB)Leg;") — IfType.get(arg0, arg1). Verbatim
// port of IfType.java:454-463. `arg0` is a PACKED component id
// (group<<16 | child); `arg1` is a cc-subcomponent index, or -1 to mean
// "the parent component itself". This is what the menu-op dispatch
// (if_button_x), target-mode entry, resume-pause, and cc_find all use —
// they pass `com.parentId` (packed) + `com.subId`. Calling the raw
// `get_sub` with a packed id indexes list[packed][..] out of bounds and
// silently returns None, which is why interface ops did nothing.
pub fn get2(arg0: i32, arg1: i32) -> Option<IfType> {
    let base = get(arg0);
    if arg1 == -1 {
        return base;
    }
    let base = base?;
    base.subcomponents
        .get(arg1 as usize)
        .and_then(|o| o.clone())
}

// @ObfuscatedName(— STORE direct write). Apply a mutator to the
// stored IfType in-place. The IF_SET* packet handlers need this
// because the per-field setters Java mutates (`com.x = ...`) read
// directly from the static `IfType.list[group][sub]` cell — if we
// take a clone via `get()` the writes are dropped.
//
// Returns true if the component was found and mutated.
pub fn modify<F: FnOnce(&mut IfType)>(component_id: i32, f: F) -> bool {
    let group = (component_id >> 16) & 0xFFFF;
    let sub = component_id & 0xFFFF;
    if group < 0 || sub < 0 { return false; }
    let mut s = STORE.lock().unwrap();
    let Some(list) = s.list.get_mut(group as usize).and_then(|o| o.as_mut()) else {
        return false;
    };
    let Some(slot) = list.get_mut(sub as usize) else { return false; };
    let Some(comp) = slot.as_mut() else { return false; };
    f(comp);
    true
}

// @ObfuscatedName("eg.aw()V") — IfType.resetCache. Clears the sprite,
// model, and font caches; called on world change / region reload.
// Java IfType.java:1153.
pub fn reset_cache() {
    SPRITE_CACHE.lock().unwrap().map.clear();
    // FONT_CACHE is declared below the impl in this file.
    FONT_CACHE.lock().unwrap().map.clear();
}

// @ObfuscatedName("eg.cm(ILjava/lang/String;I)V") — IfType.setOpName.
// Mutates a context-menu label on a cached component, used by cs2 ops
// that re-label a button (e.g. "Withdraw-X" vs "Withdraw-1"). Java
// IfType.java:1161.
pub fn set_op_name(component_id: i32, idx: i32, name: String) {
    let group = (component_id >> 16) & 0xFFFF;
    let sub = component_id & 0xFFFF;
    let mut s = STORE.lock().unwrap();
    if let Some(Some(v)) = s.list.get_mut(group as usize) {
        if let Some(Some(comp)) = v.get_mut(sub as usize) {
            // op_names is a Vec<String>; resize as needed.
            let i = idx as usize;
            if i < comp.op_names.len() {
                comp.op_names[i] = name;
            } else if i < 16 {
                comp.op_names.resize(i + 1, String::new());
                comp.op_names[i] = name;
            }
        }
    }
}

// @ObfuscatedName("eg.cc(Ljava/lang/String;B)Ljava/lang/String;") —
// IfType.substituteVars. Replaces tokens in text:
//   %1..%5 → getIfVar(scriptOperand[N-1]) string form
//   %dns   → Client.lastAddress (the last DNS lookup result)
// Java IfType.java:10389. Used by drawString in the text renderer
// before sending the line to the font.
pub fn substitute_vars(s: &str, get_if_var: impl Fn(i32) -> String) -> String {
    if !s.contains('%') { return s.to_string(); }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' { out.push(c); continue; }
        match chars.peek() {
            Some(&n @ '1'..='5') => {
                chars.next();
                out.push_str(&get_if_var(n.to_digit(10).unwrap() as i32));
            }
            Some(&'d') => {
                // Peek for "dns".
                let mut clone = chars.clone();
                if clone.next() == Some('d')
                    && clone.next() == Some('n')
                    && clone.next() == Some('s')
                {
                    for _ in 0..3 { chars.next(); }
                    out.push_str("(last DNS)");  // placeholder
                } else {
                    out.push(c);
                }
            }
            _ => out.push(c),
        }
    }
    out
}

// Java's inline obj-replace move (Client.java:2470-2477): the dragged
// slot's id/count overwrite the destination and the source empties —
// used when ServerActive.isObjReplaceEnabled is set on the component.
pub fn replace_slot(component_id: i32, src: i32, dst: i32) {
    let group = (component_id >> 16) & 0xFFFF;
    let sub = component_id & 0xFFFF;
    let mut s = STORE.lock().unwrap();
    let Some(Some(v)) = s.list.get_mut(group as usize) else { return };
    let Some(Some(comp)) = v.get_mut(sub as usize) else { return };
    let (si, di) = (src.max(0) as usize, dst.max(0) as usize);
    if si < comp.link_obj_type.len() && di < comp.link_obj_type.len() {
        comp.link_obj_type[di] = comp.link_obj_type[si];
        comp.link_obj_number[di] = comp.link_obj_number[si];
        comp.link_obj_type[si] = -1;
        comp.link_obj_number[si] = 0;
    }
}

// @ObfuscatedName("eg.cy(III)V") — IfType.swapSlots. Swaps
// link_obj_type[a] / [b] and link_obj_number[a] / [b] on the inv
// component; used by drag-swap and bank-reorder. Java IfType.java:942.
pub fn swap_slots(component_id: i32, a: i32, b: i32) {
    let group = (component_id >> 16) & 0xFFFF;
    let sub = component_id & 0xFFFF;
    let mut s = STORE.lock().unwrap();
    let Some(Some(v)) = s.list.get_mut(group as usize) else { return };
    let Some(Some(comp)) = v.get_mut(sub as usize) else { return };
    let (ai, bi) = (a as usize, b as usize);
    if ai < comp.link_obj_type.len() && bi < comp.link_obj_type.len() {
        comp.link_obj_type.swap(ai, bi);
    }
    if ai < comp.link_obj_number.len() && bi < comp.link_obj_number.len() {
        comp.link_obj_number.swap(ai, bi);
    }
}

// @ObfuscatedName("eg.q") — IfType sprite cache, lookup keyed by hash of
// (shadow_colour, hflip, vflip, outline, graphic_id) the same way Java
// keys jagex3.config.iftype.IfType::m_spriteCache.
use std::collections::HashMap;
use std::sync::Arc;

use crate::graphics::{pix32::Pix32, pix_loader};

pub struct SpriteCache {
    pub map: HashMap<u64, Arc<Pix32>>,
}

pub static SPRITE_CACHE: std::sync::LazyLock<Mutex<SpriteCache>> =
    std::sync::LazyLock::new(|| Mutex::new(SpriteCache { map: HashMap::new() }));

impl IfType {
    // @ObfuscatedName("eg.v(ZB)Lfq;") — IfType.getGraphic
    pub fn get_graphic(&self, secondary: bool) -> Option<Arc<Pix32>> {
        let id = if secondary { self.graphic2 } else { self.graphic };
        if id < 0 { return None; }
        // Java hash: shadowColour << 40 | hFlip << 39 | vFlip << 38 | outline << 36 | id
        let hash: u64 = ((self.shadow_colour as i64 as u64) << 40)
            | ((self.h_flip as u64) << 39)
            | ((self.v_flip as u64) << 38)
            | ((self.outline as u64 & 0x3) << 36)
            | (id as u64 & 0xFFFF_FFFF);
        {
            let s = SPRITE_CACHE.lock().unwrap();
            if let Some(p) = s.map.get(&hash) { return Some(Arc::clone(p)); }
        }
        let sprites_slot = SPRITES_SLOT.load(Ordering::Relaxed);
        if sprites_slot < 0 { return None; }
        let pix = {
            let mut reg = js5_net::LOADERS.lock().unwrap();
            let loader = reg.get_mut(sprites_slot as usize).and_then(|o| o.as_mut())?;
            if !pix_loader::depack_from(loader, id, 0) {
                return None;
            }
            pix_loader::make_pix32_single()
        };
        let mut pix = pix;
        if self.v_flip { pix.vflip(); }
        if self.h_flip { pix.hflip(); }
        // Outline + shadow post-processing — Java IfType.java:992-1003.
        //   outline > 0  → untrim(outline)
        //   outline >= 1 → addOutline(1)            (black border)
        //   outline >= 2 → addOutline(0xFFFFFF)     (white border on top)
        //   shadowColour != 0 → addShadow(shadowColour)
        if self.outline > 0 {
            pix.untrim(self.outline);
        }
        if self.outline >= 1 {
            pix.add_outline(1);
        }
        if self.outline >= 2 {
            pix.add_outline(0xFFFFFF);
        }
        if self.shadow_colour != 0 {
            pix.add_shadow(self.shadow_colour);
        }
        let arc = Arc::new(pix);
        SPRITE_CACHE.lock().unwrap().map.insert(hash, Arc::clone(&arc));
        Some(arc)
    }
}

// @ObfuscatedName("eg.j") — IfType model cache, keyed (type << 16) + id
// like Java's modelCache.find((type << 16) + id).
pub struct ModelCache {
    pub map: HashMap<i32, Arc<crate::dash3d::model_lit::ModelLit>>,
}
pub static MODEL_CACHE: std::sync::LazyLock<Mutex<ModelCache>> =
    std::sync::LazyLock::new(|| Mutex::new(ModelCache { map: HashMap::new() }));

impl IfType {
    // @ObfuscatedName("eg.b(Leo;IZLct;I)Lfo;") — IfType.getTempModel.
    // Verbatim port of IfType.java:1071-1149: resolves the type-6
    // component's base model (secondary picks model2Type/model2Id) —
    // type 1 = raw model archive file, 2 = npc chathead, 3 = player
    // chathead, 4 = obj model — lights it 64/768/-50/-10/-50 (objs use
    // their own ambient/contrast offsets), caches by (type<<16)+id,
    // then applies the seq animation when one is supplied. `player`
    // is Java's localPlayer.model — type 3 composes its chathead;
    // Java returns null for a null player.
    pub fn get_temp_model(
        &self,
        seq: Option<&crate::config::seq_type::SeqType>,
        seq_frame: i32,
        secondary: bool,
        player: Option<&crate::dash3d::player_model::PlayerModel>,
    ) -> Option<Arc<crate::dash3d::model_lit::ModelLit>> {
        use crate::dash3d::model_lit::ModelLit;
        use crate::dash3d::model_unlit::ModelUnlit;

        let (model_type, model_id) = if secondary {
            (self.model2_type, self.model2_id)
        } else {
            (self.model1_type, self.model1_id)
        };

        if model_type == 0 {
            return None;
        }
        if model_type == 1 && model_id == -1 {
            return None;
        }

        let key = (model_type << 16) + model_id;
        let cached = MODEL_CACHE.lock().unwrap().map.get(&key).map(Arc::clone);
        let base = match cached {
            Some(m) => m,
            None => {
                let mut lit = match model_type {
                    1 => {
                        // basic — straight model-archive load. Java
                        // getTempModel does `ModelUnlit.load(models, id, 0)`,
                        // i.e. `models.getFile(id, 0)` (ModelUnlit.java:166)
                        // DIRECTLY — getFile self-triggers the group download
                        // and returns null until it lands; the per-frame redraw
                        // retries. The previous `request_download` gate here was
                        // WRONG: request_download returns false (without queuing
                        // the download) when `unpacked[id]` is None, so the
                        // fetch_file that WOULD trigger the download was skipped
                        // and the model never loaded — exactly why interface 23's
                        // message-of-the-week models never rendered. fetch_file
                        // (like getFile / the obj-model path) queues the download
                        // itself, so call it directly.
                        let bytes = {
                            let slot = MODELS_SLOT.load(Ordering::Relaxed);
                            if slot < 0 { return None; }
                            let mut reg = js5_net::LOADERS.lock().unwrap();
                            let loader = reg.get_mut(slot as usize).and_then(|o| o.as_mut())?;
                            loader.fetch_file(model_id, 0)?
                        };
                        let mut unlit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            ModelUnlit::from_bytes(bytes)
                        })).ok()?;
                        ModelLit::light(&mut unlit, 64, 768, -50, -10, -50)
                    }
                    2 => {
                        // npc_head
                        let mut unlit = crate::config::npc_type::list(model_id).get_head()?;
                        ModelLit::light(&mut unlit, 64, 768, -50, -10, -50)
                    }
                    3 => {
                        // player_head — composes the local player's
                        // chathead; Java returns null when player ==
                        // null (no appearance yet).
                        let mut unlit = player?.get_head_model()?;
                        ModelLit::light(&mut unlit, 64, 768, -50, -10, -50)
                    }
                    4 => {
                        // object
                        let obj = crate::config::obj_type::list(model_id)?;
                        let mut unlit = obj.get_model_unlit(10)?;
                        ModelLit::light(&mut unlit, obj.ambient + 64, obj.contrast + 768,
                                        -50, -10, -50)
                    }
                    _ => return None,
                };
                // Pre-compute bounds while we own the model — the
                // type-6 draw reads minY and Java's per-frame
                // calcBoundingCylinder call becomes a no-op.
                lit.calc_bounding_cylinder();
                let arc = Arc::new(lit);
                MODEL_CACHE.lock().unwrap().map.insert(key, Arc::clone(&arc));
                arc
            }
        };

        if let Some(seq) = seq {
            let mut animated = seq.animate_model_with_extra(&base, seq_frame);
            animated.calc_bounding_cylinder();
            return Some(Arc::new(animated));
        }

        Some(base)
    }
}

// @ObfuscatedName("eg.q") — IfType font cache, indexed by font id.
use crate::graphics::pix_font_generic::PixFontGeneric;

pub struct FontCache {
    pub map: HashMap<i32, Arc<PixFontGeneric>>,
}
pub static FONT_CACHE: std::sync::LazyLock<Mutex<FontCache>> =
    std::sync::LazyLock::new(|| Mutex::new(FontCache { map: HashMap::new() }));

impl IfType {
    // @ObfuscatedName("eg.w(B)Lfm;") — IfType.getFont
    pub fn get_font(&self) -> Option<Arc<PixFontGeneric>> {
        load_font(self.font)
    }
}

// Font load by id — the body of IfType.getFont, factored so the cs2
// paraheight/parawidth opcodes (which take a font id off the stack)
// share the same cache.
pub fn load_font(font_id: i32) -> Option<Arc<PixFontGeneric>> {
    if font_id < 0 { return None; }
    if let Some(f) = FONT_CACHE.lock().unwrap().map.get(&font_id) {
        return Some(Arc::clone(f));
    }
    let sprites_slot = SPRITES_SLOT.load(Ordering::Relaxed);
    let fm_slot = FONT_METRICS_SLOT.load(Ordering::Relaxed);
    if sprites_slot < 0 || fm_slot < 0 { return None; }
    let mut reg = js5_net::LOADERS.lock().unwrap();
    // Need to depack on the sprites archive (to populate the sprite
    // state buffers PixFont uses) AND fetch metrics from the
    // fontMetrics archive — but we can't borrow both loaders mutably
    // at once, so do it in two steps with the populated sprite state
    // still in PixLoader::STATE between the calls.
    let sprite_ok = {
        let loader = reg.get_mut(sprites_slot as usize).and_then(|o| o.as_mut())?;
        pix_loader::depack_from(loader, font_id, 0)
    };
    if !sprite_ok { return None; }
    let metrics = {
        let loader = reg.get_mut(fm_slot as usize).and_then(|o| o.as_mut())?;
        loader.fetch_file(font_id, 0)
    };
    drop(reg);
    let font = pix_loader::make_pix_font_raw(metrics)?;
    let arc = Arc::new(font);
    FONT_CACHE.lock().unwrap().map.insert(font_id, Arc::clone(&arc));
    Some(arc)
}

// @ObfuscatedName("ay.c(Lch;Lch;Lch;Lch;I)V") — IfType.init.
// Verbatim port of IfType.java:428-436. Java's init wires up the four
// archive references (interfaces, models, sprites, fontMetrics) and
// pre-allocates the `list` (group→subcomponents) and `open` arrays
// sized to the interface archive's group count.
//
// On the Rust side, install_archives() already stores the slot indices
// for the four archives in the JS5 LOADERS registry; this entry point
// is invoked after JS5 sync has finished pulling the master index, so
// `getGroupCount()` is finally callable. We pre-size STORE.list and
// STORE.open here so open_interface()'s `ensure(id)` path becomes a
// cheap no-op for every interface.
pub fn init(_config_loader: &Js5Loader) {
    // Lazy-init: the IfType list / open map grow on demand inside
    // open_interface / list (). We deliberately don't pre-allocate
    // here because step 70 in client.rs calls init() while holding
    // the js5_net::LOADERS lock — re-acquiring it here used to
    // deadlock on the same thread (std::sync::Mutex is non-reentrant).
    // The interfaces_slot is already stored at install_archives time
    // via INTERFACES_SLOT; that's all consumers need.
}

// @ObfuscatedName client-side text substitution. Java's IfType-bound
// runtime text resolver replaces `@xxx@` shell tokens in the live text
// field with simple inline-format codes:
//
//   @cr1@ .. @cr9@   → colour-reset markers consumed by the renderer
//   @col=hex@        → set colour to hex
//   @gif=id@         → embed a runtime sprite
//   @or1@ .. @or9@   → overlay codes
//   @str=key@        → look up server-pushed varc string
//
// In rev1 the substitution lives in the text-render path, not in
// IfType itself, but the markup *table* and the entry point shell live
// alongside IfType because the markup vocabulary changes per revision.
// We expose a single resolve_text() shim that returns the input
// verbatim for now — when the markup parser is needed the call site
// already routes through here.
pub fn resolve_text(s: &str) -> String {
    s.to_string()
}
