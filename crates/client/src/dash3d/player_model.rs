// @ObfuscatedName("fk") — jag::oldscape::PlayerModel. Holds a player's
// worn-equipment + appearance state and produces the per-frame
// composed ModelLit for the avatar render.
//
// This is currently a stub — the appearance opcode in login.rs throws
// away the bytes today, and scene.rs's avatar render falls back to a
// capsule placeholder. We keep the struct + field shape so callers can
// wire it once the protocol decoder lands.

#![allow(dead_code)]

#[derive(Debug, Clone)]
pub struct PlayerModel {
    // @ObfuscatedName("fk.j") — 12 equipped-slot model ids (or -1).
    // Slots match Java's loop in PlayerModel.setAppearance.
    pub appearance: [i32; 12],
    // @ObfuscatedName("fk.z") — head/face/hair/skin recol 5-vector.
    pub recol_d: [i32; 5],
    // @ObfuscatedName("fk.g") — per-anim ids by part (idle, walk,
    // turnLeft, turnRight, run, ready, ...; 7 entries in Java).
    pub anims: [i32; 7],
    // @ObfuscatedName("fk.q") — display name.
    pub name: String,
    // @ObfuscatedName("fk.i") — gender / appearance flags.
    pub gender: i32,
    // @ObfuscatedName("fk.s") — head icon override.
    pub head_icon: i32,
    // @ObfuscatedName("fk.u") — combat level (rendered in tooltips).
    pub combat_level: i32,
    // Java keeps PlayerModel as a nullable field on ClientPlayer;
    // `ClientPlayer.ready()` is `model != null`. We always own the
    // struct, so this flag carries the "appearance decoded" state.
    pub applied: bool,
}

impl Default for PlayerModel {
    fn default() -> Self {
        Self {
            appearance: [-1; 12],
            recol_d: [0; 5],
            anims: [-1; 7],
            name: String::new(),
            gender: 0,
            head_icon: -1,
            combat_level: 3,
            applied: false,
        }
    }
}


impl PlayerModel {
    pub fn new() -> Self { Self::default() }

    // @ObfuscatedName("fk.c(Lev;)V") — PlayerModel.setAppearance.
    // Stub — protocol decoder will fill the 12 worn-obj + 5 recol +
    // 7 anim ids + display name from the inbound Packet.
    pub fn set_appearance_stub(&mut self) {
        // Appearance payload decode lands with the opcode port; the
        // flag still flips so ClientPlayer.ready() turns true once the
        // server has sent an appearance block.
        self.applied = true;
    }

    // @ObfuscatedName("fk.cb([I[IZI)V") — PlayerModel.setAppearance
    // body. Verbatim port. Takes the 12 worn-slot ids (already
    // unpacked from the 1- or 2-byte g1 prefix), the 5 recol ids,
    // the gender flag (Java's `var2 == 1`), and the optional
    // npc-override id (Java's `var3`, used when var4[0] == 65535).
    pub fn apply_appearance(&mut self, worn: [i32; 12], recols: [i32; 5], female: bool, npc_override: i32) {
        self.appearance = worn;
        self.recol_d = recols;
        self.gender = if female { 1 } else { 0 };
        self.applied = true;
        let _ = npc_override; // npc-override path (head model) lands
                              // with the per-model composition pass.
    }
}
