// @ObfuscatedName("dx") — jag::oldscape::ClientObj. Ground item entity
// (id + count); stamped into Client.groundObj[level][x][z] LinkLists.
// One opcode (OBJ_ADD / OBJ_DEL) per spawn / despawn.

#![allow(dead_code)]

#[derive(Debug, Clone, Copy)]
pub struct ClientObj {
    // @ObfuscatedName("dx.j") — ObjType id.
    pub id: i32,
    // @ObfuscatedName("dx.z") — stack count.
    pub count: i32,
    // custom — tile level / x / z. Java stores these on the
    // surrounding LinkList container; we keep them on the entity for
    // direct iteration.
    pub level: i32,
    pub tile_x: i32,
    pub tile_z: i32,
}

impl ClientObj {
    pub fn new(id: i32, count: i32, level: i32, tile_x: i32, tile_z: i32) -> Self {
        Self { id, count, level, tile_x, tile_z }
    }

    // @ObfuscatedName("fy.g(I)Lfo;") — ClientObj.getTempModel.
    // Verbatim port of ClientObj.java:18-20. Resolves the lit model via
    // ObjType.get_model_lit (which honours stack-size alt swaps so
    // 100/1000/10000 coin piles pick the right pyramid model).
    pub fn get_temp_model(&self) -> Option<std::sync::Arc<crate::dash3d::model_lit::ModelLit>> {
        crate::config::obj_type::list(self.id)?.get_model_lit(self.count)
    }
}
