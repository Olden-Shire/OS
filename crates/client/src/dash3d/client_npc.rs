// @ObfuscatedName("ge") — jag::oldscape::ClientNpc extends ClientEntity.
//
// One instance per visible NPC. The slot array `Client.npcs[32768]` is
// keyed by the server's NPC id; NewVis/Extended update packets allocate
// new slots and ExtractFromNet fills `type` from NpcType.list. Until
// the full update pipeline lands, this struct exists primarily as a
// container that scene.rs's render loop can iterate.

#![allow(dead_code)]

use crate::dash3d::client_entity::ClientEntity;

#[derive(Debug, Clone)]
pub struct ClientNpc {
    pub entity: ClientEntity,
    // @ObfuscatedName("ge.bu") — NpcType.list(id); cached as id for
    // now since LocType-style cache resolution lives behind a static
    // lookup. Renderer calls npc_type::list(npc.type_id) at use sites.
    pub type_id: i32,
}

impl Default for ClientNpc {
    fn default() -> Self {
        Self { entity: ClientEntity::default(), type_id: -1 }
    }
}

impl ClientNpc {
    pub fn new(type_id: i32, size: i32) -> Self {
        let mut entity = ClientEntity::default();
        entity.size = size.max(1);
        Self { entity, type_id }
    }

    // @ObfuscatedName("ge.f(I)Z") — ClientNpc.ready
    pub fn ready(&self) -> bool {
        self.type_id != -1
    }

    // @ObfuscatedName("ge.g(I)Lfo;") — ClientNpc.getTempModel.
    // Verbatim port of ClientNpc.java:17-41: resolve the NpcType
    // animated model via the entity's primary/secondary seq state,
    // stamp height, stack the spotanim (translated up by its height),
    // and toggle AABB mouse-check for single-tile NPCs.
    pub fn get_temp_model(&mut self) -> Option<crate::dash3d::model_lit::ModelLit> {
        use crate::dash3d::model_lit::ModelLit;
        if !self.ready() {
            return None;
        }
        let t = crate::config::npc_type::list(self.type_id);

        let primary = if self.entity.primary_seq_id != -1 && self.entity.primary_seq_delay == 0 {
            Some(crate::config::seq_type::list(self.entity.primary_seq_id))
        } else {
            None
        };
        let secondary = if self.entity.secondary_seq_id == -1
            || (self.entity.secondary_seq_id == self.entity.readyanim && primary.is_some())
        {
            None
        } else {
            Some(crate::config::seq_type::list(self.entity.secondary_seq_id))
        };

        let mut model = t.get_temp_model(
            primary.as_ref(), self.entity.primary_seq_frame,
            secondary.as_ref(), self.entity.secondary_seq_frame)?;

        model.calc_bounding_cylinder();
        self.entity.height = model.min_y;

        if self.entity.spotanim_id != -1 && self.entity.spotanim_frame != -1 {
            let spot = crate::config::spot_type::list(self.entity.spotanim_id)
                .get_temp_model2(self.entity.spotanim_frame);
            if let Some(mut spot) = spot {
                spot.translate(0, -self.entity.spotanim_height, 0);
                model = ModelLit::merge(&[&model, &spot]);
            }
        }

        if t.size == 1 {
            model.use_aabb_mouse_check = true;
        }
        Some(model)
    }
}
