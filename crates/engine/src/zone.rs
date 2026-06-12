//! Zone spatial index — mirrors Engine-TS `ZoneMap`/`Zone`. The world is
//! partitioned into 8×8 tile zones; each tracks the players and NPCs inside
//! it so "nearby entity" queries (info packets, broadcasts, ground items,
//! loc changes) don't scan every entity. This is the entity-membership core;
//! loc/obj/ground-item tracking layers on top later.

use std::collections::HashMap;

/// Packed zone id for a tile — `((x>>3)&0x7ff) | (((z>>3)&0x7ff)<<11) | ((level&3)<<22)`.
pub fn zone_index(x: i32, z: i32, level: i32) -> i32 {
    ((x >> 3) & 0x7ff) | (((z >> 3) & 0x7ff) << 11) | ((level & 0x3) << 22)
}

/// Ticks a player-dropped item stays private before going public.
pub const OBJ_REVEAL_TICKS: i32 = 100;
/// Ticks until a dropped item despawns.
pub const OBJ_DESPAWN_TICKS: i32 = 200;

/// A ground item occupying a tile within a zone.
#[derive(Clone, Debug)]
pub struct Obj {
    pub id: i32,
    pub count: i32,
    pub x: i32,
    pub z: i32,
    pub level: i32,
    /// Owner player id while private (-1 = public/server-spawned).
    pub receiver: i32,
    /// Ticks until the item becomes public (-1 = already public).
    pub reveal: i32,
    /// Ticks until the item despawns (-1 = never).
    pub despawn: i32,
}

impl Obj {
    /// Whether player `pid` can currently see this item (public, or its owner).
    pub fn visible_to(&self, pid: usize) -> bool {
        self.reveal < 0 || self.receiver == pid as i32
    }
}

/// A dynamic map-object change (spawn/retype) layered over the base map loc.
#[derive(Clone, Debug)]
pub struct Loc {
    pub id: i32,
    pub shape: i32,
    pub angle: i32,
    pub x: i32,
    pub z: i32,
    pub level: i32,
    /// Ticks until this change reverts/despawns (-1 = permanent).
    pub despawn: i32,
}

/// Entities, ground items, and loc changes in one 8×8 zone.
#[derive(Default)]
pub struct Zone {
    pub players: Vec<usize>,
    pub npcs: Vec<usize>,
    pub objs: Vec<Obj>,
    pub locs: Vec<Loc>,
}

/// Sparse map of zone index → [`Zone`]; zones are created on demand.
#[derive(Default)]
pub struct ZoneMap {
    zones: HashMap<i32, Zone>,
}

impl ZoneMap {
    pub fn zone(&self, index: i32) -> Option<&Zone> {
        self.zones.get(&index)
    }

    pub fn zone_count(&self) -> usize {
        self.zones.len()
    }

    pub fn enter_player(&mut self, index: i32, pid: usize) {
        let z = self.zones.entry(index).or_default();
        if !z.players.contains(&pid) {
            z.players.push(pid);
        }
    }

    pub fn leave_player(&mut self, index: i32, pid: usize) {
        if let Some(z) = self.zones.get_mut(&index) {
            z.players.retain(|&p| p != pid);
        }
    }

    pub fn move_player(&mut self, pid: usize, from: i32, to: i32) {
        if from == to {
            return;
        }
        self.leave_player(from, pid);
        self.enter_player(to, pid);
    }

    pub fn enter_npc(&mut self, index: i32, nid: usize) {
        let z = self.zones.entry(index).or_default();
        if !z.npcs.contains(&nid) {
            z.npcs.push(nid);
        }
    }

    pub fn leave_npc(&mut self, index: i32, nid: usize) {
        if let Some(z) = self.zones.get_mut(&index) {
            z.npcs.retain(|&n| n != nid);
        }
    }

    pub fn move_npc(&mut self, nid: usize, from: i32, to: i32) {
        if from == to {
            return;
        }
        self.leave_npc(from, nid);
        self.enter_npc(to, nid);
    }

    /// Players standing in the given zone (empty if the zone is unused).
    pub fn players_in(&self, index: i32) -> &[usize] {
        self.zones.get(&index).map_or(&[], |z| z.players.as_slice())
    }

    pub fn npcs_in(&self, index: i32) -> &[usize] {
        self.zones.get(&index).map_or(&[], |z| z.npcs.as_slice())
    }

    /// Record a ground item in its zone.
    pub fn add_obj(&mut self, obj: Obj) {
        self.zones
            .entry(zone_index(obj.x, obj.z, obj.level))
            .or_default()
            .objs
            .push(obj);
    }

    /// Remove the first matching ground item from its tile; returns it if found.
    pub fn remove_obj(&mut self, id: i32, x: i32, z: i32, level: i32) -> Option<Obj> {
        let zone = self.zones.get_mut(&zone_index(x, z, level))?;
        let pos = zone
            .objs
            .iter()
            .position(|o| o.id == id && o.x == x && o.z == z && o.level == level)?;
        Some(zone.objs.remove(pos))
    }

    pub fn objs_in(&self, index: i32) -> &[Obj] {
        self.zones.get(&index).map_or(&[], |z| z.objs.as_slice())
    }

    /// Restack a ground item in place (OBJ_COUNT): match the pile by id and its
    /// current count, set the new count, and return its `(receiver, reveal)`
    /// visibility so the caller can target the broadcast. `None` if absent.
    pub fn update_obj_count(
        &mut self,
        id: i32,
        x: i32,
        z: i32,
        level: i32,
        old_count: i32,
        new_count: i32,
    ) -> Option<(i32, i32)> {
        let zone = self.zones.get_mut(&zone_index(x, z, level))?;
        let obj = zone
            .objs
            .iter_mut()
            .find(|o| o.id == id && o.x == x && o.z == z && o.count == old_count)?;
        obj.count = new_count;
        Some((obj.receiver, obj.reveal))
    }

    /// Advance all ground-item lifecycle timers one tick. Returns the items
    /// that went public this tick (for OBJ_REVEAL) and those that despawned
    /// (already removed here; for OBJ_DEL).
    pub fn tick_objs(&mut self) -> (Vec<Obj>, Vec<Obj>) {
        let mut revealed = Vec::new();
        let mut despawned = Vec::new();
        for zone in self.zones.values_mut() {
            zone.objs.retain_mut(|o| {
                if o.reveal > 0 {
                    o.reveal -= 1;
                    if o.reveal == 0 {
                        o.reveal = -1; // now public
                        revealed.push(o.clone());
                    }
                }
                if o.despawn > 0 {
                    o.despawn -= 1;
                    if o.despawn == 0 {
                        despawned.push(o.clone());
                        return false;
                    }
                }
                true
            });
        }
        (revealed, despawned)
    }

    /// Record a loc change, replacing any prior change on the same tile/shape.
    pub fn add_loc(&mut self, loc: Loc) {
        let zone = self.zones.entry(zone_index(loc.x, loc.z, loc.level)).or_default();
        zone.locs
            .retain(|l| !(l.x == loc.x && l.z == loc.z && l.level == loc.level && l.shape == loc.shape));
        zone.locs.push(loc);
    }

    /// Remove the loc change on a tile with the given shape; returns it if any.
    pub fn remove_loc(&mut self, x: i32, z: i32, level: i32, shape: i32) -> Option<Loc> {
        let zone = self.zones.get_mut(&zone_index(x, z, level))?;
        let pos = zone
            .locs
            .iter()
            .position(|l| l.x == x && l.z == z && l.level == level && l.shape == shape)?;
        Some(zone.locs.remove(pos))
    }

    pub fn locs_in(&self, index: i32) -> &[Loc] {
        self.zones.get(&index).map_or(&[], |z| z.locs.as_slice())
    }

    /// Advance timed loc changes one tick; returns the ones that expired this
    /// tick (already removed here; for LOC_DEL).
    pub fn tick_locs(&mut self) -> Vec<Loc> {
        let mut expired = Vec::new();
        for zone in self.zones.values_mut() {
            zone.locs.retain_mut(|l| {
                if l.despawn > 0 {
                    l.despawn -= 1;
                    if l.despawn == 0 {
                        expired.push(l.clone());
                        return false;
                    }
                }
                true
            });
        }
        expired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_index_packs_8x8_blocks() {
        // Same zone for all tiles in the 8×8 block.
        assert_eq!(zone_index(3200, 3200, 0), zone_index(3207, 3207, 0));
        // Crossing an 8-tile boundary changes the zone.
        assert_ne!(zone_index(3207, 3200, 0), zone_index(3208, 3200, 0));
        // Level participates.
        assert_ne!(zone_index(3200, 3200, 0), zone_index(3200, 3200, 1));
    }

    #[test]
    fn membership_follows_movement() {
        let mut map = ZoneMap::default();
        let a = zone_index(3200, 3200, 0);
        let b = zone_index(3208, 3200, 0);

        map.enter_player(a, 7);
        assert_eq!(map.players_in(a), &[7]);

        map.move_player(7, a, b);
        assert!(map.players_in(a).is_empty());
        assert_eq!(map.players_in(b), &[7]);

        map.leave_player(b, 7);
        assert!(map.players_in(b).is_empty());
    }
}
