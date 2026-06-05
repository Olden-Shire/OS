//! IfType — interface component definitions (archive 3). Port of
//! `jagex3.config.iftype.IfType`.
//!
//! Each interface "parent" (a window like the inventory, bank, etc.) is a JS5 group whose
//! files are its subcomponents. Two on-disk formats:
//!
//! * **v1** (`data[0] != -1`): legacy format. `decode()`.
//! * **v3** (`data[0] == -1`): newer format with a richer script-hook table. `decode_v3()`.
//!
//! The decoder branches on the leading byte and dispatches.
//!
//! Server uses these for script hooks (onclick / onload / etc), button operations, and
//! inv-component sizing. Rendering fields (colours, transparency, fonts, model angles) are
//! retained verbatim for forwarding to the client.

use io::Packet;

/// One arg in a script hook. Java models these as `Object[]` with mixed `Integer`/`String`.
#[derive(Debug, Clone)]
pub enum HookArg {
    Int(i32),
    Str(String),
}

#[derive(Debug, Clone)]
pub struct IfType {
    pub v3: bool,
    pub parent_id: i32,
    pub sub_id: i32,
    pub type_: i32,
    pub button_type: i32,
    pub client_code: i32,
    pub x: i32,
    pub y: i32,
    pub data_x: i32,
    pub data_y: i32,
    pub width: i32,
    pub height: i32,
    pub layer_id: i32,
    pub hide: bool,
    pub scroll_pos_x: i32,
    pub scroll_pos_y: i32,
    pub scroll_width: i32,
    pub scroll_height: i32,
    pub colour: i32,
    pub colour2: i32,
    pub colour_over: i32,
    pub colour2_over: i32,
    pub fill: bool,
    pub trans: i32,
    pub line_width: i32,
    pub graphic: i32,
    pub graphic2: i32,
    pub rotate: i32,
    pub tiling: bool,
    pub outline: i32,
    pub shadow_colour: i32,
    pub v_flip: bool,
    pub h_flip: bool,
    pub model1_type: i32,
    pub model1_id: i32,
    pub model2_type: i32,
    pub model2_id: i32,
    pub model_anim: i32,
    pub model_anim2: i32,
    pub model_x_of: i32,
    pub model_y_of: i32,
    pub model_x_an: i32,
    pub model_y_an: i32,
    pub model_z_an: i32,
    pub model_zoom: i32,
    pub model_spin: i32,
    pub orthog: bool,
    pub font: i32,
    pub text: String,
    pub text2: String,
    pub line_height: i32,
    pub h_align: i32,
    pub v_align: i32,
    pub shadow: bool,
    pub margin_x: i32,
    pub margin_y: i32,
    pub inv_background_x: Vec<i32>,
    pub inv_background_y: Vec<i32>,
    pub inv_background: Vec<i32>,
    pub iop: [Option<String>; 5],
    pub event_code: i32,
    pub base_op_name: String,
    pub op_names: Vec<Option<String>>,
    pub dragdeadzone: i32,
    pub dragdeadtime: i32,
    pub draggable_behavior: bool,
    pub target_verb: String,
    pub hashook: bool,
    pub onload: Option<Vec<HookArg>>,
    pub onclick: Option<Vec<HookArg>>,
    pub onclickrepeat: Option<Vec<HookArg>>,
    pub onrelease: Option<Vec<HookArg>>,
    pub onhold: Option<Vec<HookArg>>,
    pub onmouseover: Option<Vec<HookArg>>,
    pub onmouserepeat: Option<Vec<HookArg>>,
    pub onmouseleave: Option<Vec<HookArg>>,
    pub ondrag: Option<Vec<HookArg>>,
    pub ondragcomplete: Option<Vec<HookArg>>,
    pub ontargetenter: Option<Vec<HookArg>>,
    pub ontargetleave: Option<Vec<HookArg>>,
    pub onvartransmit: Option<Vec<HookArg>>,
    pub oninvtransmit: Option<Vec<HookArg>>,
    pub onstattransmit: Option<Vec<HookArg>>,
    pub ontimer: Option<Vec<HookArg>>,
    pub onop: Option<Vec<HookArg>>,
    pub onscrollwheel: Option<Vec<HookArg>>,
    pub onvartransmitlist: Option<Vec<i32>>,
    pub oninvtransmitlist: Option<Vec<i32>>,
    pub onstattransmitlist: Option<Vec<i32>>,
    pub scripts: Option<Vec<Vec<i32>>>,
    pub script_comparator: Option<Vec<i32>>,
    pub script_operand: Option<Vec<i32>>,
    pub over_layer_id: i32,
    pub target_base: String,
    pub button_text: String,
    pub link_obj_type: Vec<i32>,
    pub link_obj_number: Vec<i32>,
}

impl Default for IfType {
    fn default() -> Self {
        Self {
            v3: false,
            parent_id: -1,
            sub_id: -1,
            type_: 0,
            button_type: 0,
            client_code: 0,
            x: 0,
            y: 0,
            data_x: 0,
            data_y: 0,
            width: 0,
            height: 0,
            layer_id: -1,
            hide: false,
            scroll_pos_x: 0,
            scroll_pos_y: 0,
            scroll_width: 0,
            scroll_height: 0,
            colour: 0,
            colour2: 0,
            colour_over: 0,
            colour2_over: 0,
            fill: false,
            trans: 0,
            line_width: 1,
            graphic: -1,
            graphic2: -1,
            rotate: 0,
            tiling: false,
            outline: 0,
            shadow_colour: 0,
            v_flip: false,
            h_flip: false,
            model1_type: 1,
            model1_id: -1,
            model2_type: 1,
            model2_id: -1,
            model_anim: -1,
            model_anim2: -1,
            model_x_of: 0,
            model_y_of: 0,
            model_x_an: 0,
            model_y_an: 0,
            model_z_an: 0,
            model_zoom: 100,
            model_spin: 0,
            orthog: false,
            font: -1,
            text: String::new(),
            text2: String::new(),
            line_height: 0,
            h_align: 0,
            v_align: 0,
            shadow: false,
            margin_x: 0,
            margin_y: 0,
            inv_background_x: Vec::new(),
            inv_background_y: Vec::new(),
            inv_background: Vec::new(),
            iop: [const { None }; 5],
            event_code: 0,
            base_op_name: String::new(),
            op_names: Vec::new(),
            dragdeadzone: 0,
            dragdeadtime: 0,
            draggable_behavior: false,
            target_verb: String::new(),
            hashook: false,
            onload: None,
            onclick: None,
            onclickrepeat: None,
            onrelease: None,
            onhold: None,
            onmouseover: None,
            onmouserepeat: None,
            onmouseleave: None,
            ondrag: None,
            ondragcomplete: None,
            ontargetenter: None,
            ontargetleave: None,
            onvartransmit: None,
            oninvtransmit: None,
            onstattransmit: None,
            ontimer: None,
            onop: None,
            onscrollwheel: None,
            onvartransmitlist: None,
            oninvtransmitlist: None,
            onstattransmitlist: None,
            scripts: None,
            script_comparator: None,
            script_operand: None,
            over_layer_id: -1,
            target_base: String::new(),
            button_text: "OK".to_string(),
            link_obj_type: Vec::new(),
            link_obj_number: Vec::new(),
        }
    }
}

impl IfType {
    /// Decode a subcomponent from its raw file bytes. `parent_id` is `(group << 16) | sub`.
    /// Branches on the leading byte: `-1` → v3 layout, otherwise → legacy v1.
    pub fn decode(parent_id: i32, sub_id: i32, bytes: &[u8]) -> Self {
        let mut t = Self { parent_id, sub_id, ..Self::default() };
        if !bytes.is_empty() && bytes[0] == 0xFF {
            t.decode_v3(bytes);
        } else {
            t.decode_v1(bytes);
        }
        t
    }

    fn decode_v1(&mut self, bytes: &[u8]) {
        let mut buf = Packet::from_vec(bytes.to_vec());
        self.v3 = false;
        self.type_ = buf.g1();
        self.button_type = buf.g1();
        self.client_code = buf.g2();
        self.x = i32::from(buf.g2b());
        self.data_x = self.x;
        self.y = i32::from(buf.g2b());
        self.data_y = self.y;
        self.width = buf.g2();
        self.height = buf.g2();
        self.trans = buf.g1();

        self.layer_id = buf.g2();
        if self.layer_id == 65535 {
            self.layer_id = -1;
        } else {
            self.layer_id += self.parent_id & 0xFFFF_0000_u32 as i32;
        }

        self.over_layer_id = buf.g2();
        if self.over_layer_id == 65535 {
            self.over_layer_id = -1;
        }

        let script_stack = buf.g1() as usize;
        if script_stack > 0 {
            let mut cmps = Vec::with_capacity(script_stack);
            let mut ops = Vec::with_capacity(script_stack);
            for _ in 0..script_stack {
                cmps.push(buf.g1());
                ops.push(buf.g2());
            }
            self.script_comparator = Some(cmps);
            self.script_operand = Some(ops);
        }

        let scripts_n = buf.g1() as usize;
        if scripts_n > 0 {
            let mut scripts = Vec::with_capacity(scripts_n);
            for _ in 0..scripts_n {
                let inner = buf.g2() as usize;
                let mut s = Vec::with_capacity(inner);
                for _ in 0..inner {
                    let v = buf.g2();
                    s.push(if v == 65535 { -1 } else { v });
                }
                scripts.push(s);
            }
            self.scripts = Some(scripts);
        }

        match self.type_ {
            0 => {
                self.scroll_height = buf.g2();
                self.hide = buf.g1() == 1;
            }
            1 => {
                buf.g2();
                buf.g1();
            }
            2 => {
                let slots = (self.height * self.width) as usize;
                self.link_obj_type = vec![0; slots];
                self.link_obj_number = vec![0; slots];

                if buf.g1() == 1 { self.event_code |= 0x1000_0000; }
                if buf.g1() == 1 { self.event_code |= 0x4000_0000; }
                if buf.g1() == 1 { self.event_code |= 0x8000_0000_u32 as i32; }
                if buf.g1() == 1 { self.event_code |= 0x2000_0000; }

                self.margin_x = buf.g1();
                self.margin_y = buf.g1();

                self.inv_background_x = vec![0; 20];
                self.inv_background_y = vec![0; 20];
                self.inv_background = vec![0; 20];
                for i in 0..20 {
                    if buf.g1() == 1 {
                        self.inv_background_x[i] = i32::from(buf.g2b());
                        self.inv_background_y[i] = i32::from(buf.g2b());
                        self.inv_background[i] = buf.g4();
                    } else {
                        self.inv_background[i] = -1;
                    }
                }

                for i in 0..5 {
                    let op = buf.gjstr();
                    if !op.is_empty() {
                        self.iop[i] = Some(op);
                        self.event_code |= 0x1 << (i + 23);
                    }
                }
            }
            3 => {
                self.fill = buf.g1() == 1;
            }
            _ => {}
        }

        if self.type_ == 4 || self.type_ == 1 {
            self.h_align = buf.g1();
            self.v_align = buf.g1();
            self.line_height = buf.g1();
            let font = buf.g2();
            self.font = if font == 65535 { -1 } else { font };
            self.shadow = buf.g1() == 1;
        }
        if self.type_ == 4 {
            self.text = buf.gjstr();
            self.text2 = buf.gjstr();
        }
        if self.type_ == 1 || self.type_ == 3 || self.type_ == 4 {
            self.colour = buf.g4();
        }
        if self.type_ == 3 || self.type_ == 4 {
            self.colour2 = buf.g4();
            self.colour_over = buf.g4();
            self.colour2_over = buf.g4();
        }
        if self.type_ == 5 {
            self.graphic = buf.g4();
            self.graphic2 = buf.g4();
        }
        if self.type_ == 6 {
            self.model1_type = 1;
            let m1 = buf.g2();
            self.model1_id = if m1 == 65535 { -1 } else { m1 };
            self.model2_type = 1;
            let m2 = buf.g2();
            self.model2_id = if m2 == 65535 { -1 } else { m2 };
            let ma = buf.g2();
            self.model_anim = if ma == 65535 { -1 } else { ma };
            let ma2 = buf.g2();
            self.model_anim2 = if ma2 == 65535 { -1 } else { ma2 };
            self.model_zoom = buf.g2();
            self.model_x_an = buf.g2();
            self.model_y_an = buf.g2();
        }
        if self.type_ == 7 {
            let slots = (self.height * self.width) as usize;
            self.link_obj_type = vec![0; slots];
            self.link_obj_number = vec![0; slots];
            self.h_align = buf.g1();
            let font = buf.g2();
            self.font = if font == 65535 { -1 } else { font };
            self.shadow = buf.g1() == 1;
            self.colour = buf.g4();
            self.margin_x = i32::from(buf.g2b());
            self.margin_y = i32::from(buf.g2b());
            if buf.g1() == 1 { self.event_code |= 0x4000_0000; }
            for i in 0..5 {
                let op = buf.gjstr();
                if !op.is_empty() {
                    self.iop[i] = Some(op);
                    self.event_code |= 0x1 << (i + 23);
                }
            }
        }
        if self.type_ == 8 {
            self.text = buf.gjstr();
        }
        if self.button_type == 2 || self.type_ == 2 {
            self.target_verb = buf.gjstr();
            self.target_base = buf.gjstr();
            let target_mask = buf.g2() & 0x3F;
            self.event_code |= target_mask << 11;
        }
        if matches!(self.button_type, 1 | 4 | 5 | 6) {
            self.button_text = buf.gjstr();
            if self.button_text.is_empty() {
                self.button_text = match self.button_type {
                    1 => "OK",
                    4 | 5 => "Select",
                    6 => "Continue",
                    _ => "",
                }
                .to_string();
            }
        }
        if matches!(self.button_type, 1 | 4 | 5) {
            self.event_code |= 0x40_0000;
        }
        if self.button_type == 6 {
            self.event_code |= 0x1;
        }
    }

    fn decode_v3(&mut self, bytes: &[u8]) {
        let mut buf = Packet::from_vec(bytes.to_vec());
        buf.g1(); // sentinel (-1)
        self.v3 = true;

        self.type_ = buf.g1();
        self.client_code = buf.g2();
        self.x = i32::from(buf.g2b());
        self.data_x = self.x;
        self.y = i32::from(buf.g2b());
        self.data_y = self.y;
        self.width = buf.g2();
        self.height = if self.type_ == 9 { i32::from(buf.g2b()) } else { buf.g2() };

        self.layer_id = buf.g2();
        if self.layer_id == 65535 {
            self.layer_id = -1;
        } else {
            self.layer_id += self.parent_id & 0xFFFF_0000_u32 as i32;
        }
        self.hide = buf.g1() == 1;

        match self.type_ {
            0 => {
                self.scroll_width = buf.g2();
                self.scroll_height = buf.g2();
            }
            5 => {
                self.graphic = buf.g4();
                self.rotate = buf.g2();
                self.tiling = buf.g1() == 1;
                self.trans = buf.g1();
                self.outline = buf.g1();
                self.shadow_colour = buf.g4();
                self.v_flip = buf.g1() == 1;
                self.h_flip = buf.g1() == 1;
            }
            6 => {
                self.model1_type = 1;
                let m1 = buf.g2();
                self.model1_id = if m1 == 65535 { -1 } else { m1 };
                self.model_x_of = i32::from(buf.g2b());
                self.model_y_of = i32::from(buf.g2b());
                self.model_x_an = buf.g2();
                self.model_y_an = buf.g2();
                self.model_z_an = buf.g2();
                self.model_zoom = buf.g2();
                let ma = buf.g2();
                self.model_anim = if ma == 65535 { -1 } else { ma };
                self.orthog = buf.g1() == 1;
            }
            4 => {
                let font = buf.g2();
                self.font = if font == 65535 { -1 } else { font };
                self.text = buf.gjstr();
                self.line_height = buf.g1();
                self.h_align = buf.g1();
                self.v_align = buf.g1();
                self.shadow = buf.g1() == 1;
                self.colour = buf.g4();
            }
            3 => {
                self.colour = buf.g4();
                self.fill = buf.g1() == 1;
                self.trans = buf.g1();
            }
            9 => {
                self.line_width = buf.g1();
                self.colour = buf.g4();
            }
            _ => {}
        }

        self.event_code = buf.g3();
        self.base_op_name = buf.gjstr();

        let ops = buf.g1() as usize;
        if ops > 0 {
            self.op_names = Vec::with_capacity(ops);
            for _ in 0..ops {
                self.op_names.push(Some(buf.gjstr()));
            }
        }

        self.dragdeadzone = buf.g1();
        self.dragdeadtime = buf.g1();
        self.draggable_behavior = buf.g1() == 1;
        self.target_verb = buf.gjstr();

        self.onload = self.decode_hook(&mut buf);
        self.onmouseover = self.decode_hook(&mut buf);
        self.onmouseleave = self.decode_hook(&mut buf);
        self.ontargetleave = self.decode_hook(&mut buf);
        self.ontargetenter = self.decode_hook(&mut buf);
        self.onvartransmit = self.decode_hook(&mut buf);
        self.oninvtransmit = self.decode_hook(&mut buf);
        self.onstattransmit = self.decode_hook(&mut buf);
        self.ontimer = self.decode_hook(&mut buf);
        self.onop = self.decode_hook(&mut buf);
        self.onmouserepeat = self.decode_hook(&mut buf);
        self.onclick = self.decode_hook(&mut buf);
        self.onclickrepeat = self.decode_hook(&mut buf);
        self.onrelease = self.decode_hook(&mut buf);
        self.onhold = self.decode_hook(&mut buf);
        self.ondrag = self.decode_hook(&mut buf);
        self.ondragcomplete = self.decode_hook(&mut buf);
        self.onscrollwheel = self.decode_hook(&mut buf);
        self.onvartransmitlist = decode_int_list(&mut buf);
        self.oninvtransmitlist = decode_int_list(&mut buf);
        self.onstattransmitlist = decode_int_list(&mut buf);
    }

    fn decode_hook(&mut self, p: &mut Packet) -> Option<Vec<HookArg>> {
        let n = p.g1() as usize;
        if n == 0 {
            return None;
        }
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let kind = p.g1();
            out.push(match kind {
                0 => HookArg::Int(p.g4()),
                1 => HookArg::Str(p.gjstr()),
                _ => panic!("IfType hook arg: unknown kind {kind}"),
            });
        }
        self.hashook = true;
        Some(out)
    }
}

fn decode_int_list(p: &mut Packet) -> Option<Vec<i32>> {
    let n = p.g1() as usize;
    if n == 0 {
        return None;
    }
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(p.g4());
    }
    Some(out)
}
