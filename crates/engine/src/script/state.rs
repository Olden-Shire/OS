//! Interpreter state — mirrors the Engine-TS reference ScriptState.ts.
//! Entity references become slot handles (Rust can't hold aliasing
//! references into the World); ops resolve them through &mut World.

use std::collections::HashMap;
use std::sync::Arc;

use crate::script::file::ScriptFile;
use crate::script::trigger::Trigger;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Execution {
    Aborted,
    Running,
    Finished,
    Suspended,
    PauseButton,
    CountDialog,
    NpcSuspended,
    WorldSuspended,
}

/// Pointer-safety flags (ScriptPointer in the reference).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Pointer {
    ActivePlayer = 0,
    ActivePlayer2 = 1,
    ProtectedActivePlayer = 2,
    ProtectedActivePlayer2 = 3,
    ActiveNpc = 4,
    ActiveNpc2 = 5,
    ActiveLoc = 6,
    ActiveLoc2 = 7,
    ActiveObj = 8,
    ActiveObj2 = 9,
}

pub const ACTIVE_PLAYER: [Pointer; 2] = [Pointer::ActivePlayer, Pointer::ActivePlayer2];
pub const PROTECTED_ACTIVE_PLAYER: [Pointer; 2] =
    [Pointer::ProtectedActivePlayer, Pointer::ProtectedActivePlayer2];
pub const ACTIVE_NPC: [Pointer; 2] = [Pointer::ActiveNpc, Pointer::ActiveNpc2];

pub struct GosubFrame {
    pub script: Arc<ScriptFile>,
    pub pc: i32,
    pub int_locals: Vec<i32>,
    pub string_locals: Vec<String>,
}

/// One script-call argument.
#[derive(Clone, Debug)]
pub enum ScriptArg {
    Int(i32),
    Str(String),
}

pub struct ScriptState {
    pub script: Arc<ScriptFile>,
    pub trigger: Trigger,
    pub execution: Execution,
    /// Ticks requested by the P_DELAY that suspended this script (Engine-TS
    /// `state.delay`); the world resumes the script after they elapse.
    pub delay: i32,

    pub pc: i32,
    pub opcount: u32,

    pub frames: Vec<GosubFrame>,
    pub debug_frames: Vec<(Arc<ScriptFile>, i32)>,

    pub int_stack: Vec<i32>,
    pub string_stack: Vec<String>,
    pub int_locals: Vec<i32>,
    pub string_locals: Vec<String>,

    pointers: u32,

    // Active entity handles (slot indices into World).
    pub active_player: Option<usize>,
    pub active_player2: Option<usize>,
    pub active_npc: Option<usize>,
    pub active_npc2: Option<usize>,

    /// Active ground-item handle (Engine-TS `activeObj`) as an `(x, z, level,
    /// id)` locator — set by OBJ_FIND, read by OBJ_COORD/OBJ_TYPE/OBJ_COUNT.
    pub active_obj: Option<(i32, i32, i32, i32)>,

    /// Active map-object handle (Engine-TS `activeLoc`) as `(x, z, level, id,
    /// shape, angle)` — set by LOC_FIND, read by LOC_COORD/TYPE/SHAPE/ANGLE.
    pub active_loc: Option<(i32, i32, i32, i32, i32, i32)>,

    /// The "active value" for this script invocation (Engine-TS `state.lastInt`):
    /// the resume value from a pause-button / count dialog, read by LAST_INT.
    pub last_int: i32,

    /// define_array storage (5 arrays like the cs2 VM; the reference
    /// leaves these unimplemented server-side but the state slot
    /// exists for parity).
    pub arrays: HashMap<i32, Vec<i32>>,

    /// Pending npc-search results (Engine-TS `npcIterator`): NPC_FINDALL fills
    /// it (nearest last, so NPC_FINDNEXT pops nearest first).
    pub npc_iterator: Vec<usize>,

    /// Pending ground-item search results (Engine-TS `objIterator`) as
    /// `(x, z, level, id)` locators: OBJ_FINDALLZONE fills it, OBJ_FINDNEXT pops.
    pub obj_iterator: Vec<(i32, i32, i32, i32)>,

    /// Pending map-object search results (Engine-TS `locIterator`) as
    /// `(x, z, level, id, shape, angle)`: LOC_FINDALLZONE fills, LOC_FINDNEXT pops.
    pub loc_iterator: Vec<(i32, i32, i32, i32, i32, i32)>,

    /// Pending player-search results (Engine-TS `playerIterator`): HUNTALL fills
    /// it (nearest last, so HUNTNEXT pops nearest first).
    pub player_iterator: Vec<usize>,
}

impl ScriptState {
    pub fn new(script: Arc<ScriptFile>, args: &[ScriptArg]) -> ScriptState {
        let trigger = (script.info.lookup_key & 0xff) as Trigger;

        let mut int_locals = Vec::new();
        let mut string_locals = Vec::new();
        for arg in args {
            match arg {
                ScriptArg::Int(v) => int_locals.push(*v),
                ScriptArg::Str(s) => string_locals.push(s.clone()),
            }
        }
        int_locals.resize(int_locals.len().max(script.int_local_count), 0);
        string_locals.resize(string_locals.len().max(script.string_local_count), String::new());

        ScriptState {
            script,
            trigger,
            execution: Execution::Running,
            delay: 0,
            pc: -1,
            opcount: 0,
            frames: Vec::new(),
            debug_frames: Vec::new(),
            int_stack: Vec::new(),
            string_stack: Vec::new(),
            int_locals,
            string_locals,
            pointers: 0,
            active_player: None,
            active_player2: None,
            last_int: 0,
            active_npc: None,
            active_npc2: None,
            active_obj: None,
            active_loc: None,
            arrays: HashMap::new(),
            npc_iterator: Vec::new(),
            obj_iterator: Vec::new(),
            loc_iterator: Vec::new(),
            player_iterator: Vec::new(),
        }
    }

    // ── Pointers ──────────────────────────────────────────────────

    pub fn pointer_add(&mut self, p: Pointer) {
        self.pointers |= 1 << (p as u32);
    }

    pub fn pointer_remove(&mut self, p: Pointer) {
        self.pointers &= !(1 << (p as u32));
    }

    pub fn pointer_get(&self, p: Pointer) -> bool {
        self.pointers & (1 << (p as u32)) != 0
    }

    pub fn pointer_check(&self, p: Pointer) -> Result<(), String> {
        if !self.pointer_get(p) {
            return Err(format!("required pointer: {p:?}"));
        }
        Ok(())
    }

    // ── Operands ──────────────────────────────────────────────────

    pub fn int_operand(&self) -> i32 {
        self.script.int_operands[self.pc as usize]
    }

    pub fn string_operand(&self) -> &str {
        self.script.string_operands[self.pc as usize]
            .as_deref()
            .unwrap_or("")
    }

    /// The `.command` secondary-target flag (operand == 1 selects the
    /// secondary active entity).
    pub fn secondary(&self) -> bool {
        self.int_operand() == 1
    }

    // ── Stacks ────────────────────────────────────────────────────

    pub fn push_int(&mut self, v: i32) {
        self.int_stack.push(v);
    }

    pub fn pop_int(&mut self) -> i32 {
        self.int_stack.pop().unwrap_or(0)
    }

    pub fn pop_ints<const N: usize>(&mut self) -> [i32; N] {
        let mut out = [0i32; N];
        for i in (0..N).rev() {
            out[i] = self.pop_int();
        }
        out
    }

    pub fn push_string(&mut self, v: String) {
        self.string_stack.push(v);
    }

    pub fn pop_string(&mut self) -> String {
        self.string_stack.pop().unwrap_or_default()
    }

    pub fn pop_strings(&mut self, n: usize) -> Vec<String> {
        let mut out = vec![String::new(); n];
        for i in (0..n).rev() {
            out[i] = self.pop_string();
        }
        out
    }

    // ── Frames ────────────────────────────────────────────────────

    pub fn pop_frame(&mut self) {
        self.debug_frames.pop();
        let frame = self.frames.pop().expect("pop_frame on empty stack");
        self.pc = frame.pc;
        self.script = frame.script;
        self.int_locals = frame.int_locals;
        self.string_locals = frame.string_locals;
    }

    pub fn gosub_frame(&mut self, proc: Arc<ScriptFile>) {
        self.debug_frames.push((Arc::clone(&self.script), self.pc));
        self.frames.push(GosubFrame {
            script: Arc::clone(&self.script),
            pc: self.pc,
            int_locals: std::mem::take(&mut self.int_locals),
            string_locals: std::mem::take(&mut self.string_locals),
        });
        self.setup_new_script(proc);
    }

    pub fn goto_frame(&mut self, label: Arc<ScriptFile>) {
        self.debug_frames.push((Arc::clone(&self.script), self.pc));
        self.frames.clear();
        self.setup_new_script(label);
    }

    fn setup_new_script(&mut self, script: Arc<ScriptFile>) {
        let mut int_locals = vec![0i32; script.int_local_count];
        for i in 0..script.int_arg_count {
            int_locals[script.int_arg_count - i - 1] = self.pop_int();
        }
        let mut string_locals = vec![String::new(); script.string_local_count];
        for i in 0..script.string_arg_count {
            string_locals[script.string_arg_count - i - 1] = self.pop_string();
        }

        self.pc = -1;
        self.script = script;
        self.int_locals = int_locals;
        self.string_locals = string_locals;
    }
}
