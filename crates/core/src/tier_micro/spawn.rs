//! Weighted monster spawning for the micro tier.
//!
//! Uses `rules::monster_table` spawn weights and stat lookups.

use super::entity::EntityStore;
use super::map::MicroMap;
use super::prng::LfsrRng16;
use super::types::*;
use crate::rules::balance;
use crate::rules::monster_table::{self, KIND_COUNT, MonsterKind, SPAWN_KINDS, SPAWN_WEIGHTS};

/// Pick a random monster kind using the rules/ spawn weights.
pub fn pick_monster_kind(rng: &mut LfsrRng16) -> MonsterKind {
    let mut total: u8 = 0;
    for &w in &SPAWN_WEIGHTS[..KIND_COUNT] {
        total = total.saturating_add(w);
    }
    let mut roll = rng.range_u8(0, total - 1);
    for i in 0..KIND_COUNT {
        if roll < SPAWN_WEIGHTS[i] {
            return SPAWN_KINDS[i];
        }
        roll -= SPAWN_WEIGHTS[i];
    }
    SPAWN_KINDS[0]
}

/// Spawn monsters in rooms (skip room 0 = player start).
pub fn spawn_monsters(entities: &mut EntityStore, map: &MicroMap, rng: &mut LfsrRng16) {
    for ri in 1..map.room_count {
        let room = map.rooms[ri as usize];
        let count = rng.range_u8(0, balance::MAX_MONSTERS_PER_ROOM);
        for _ in 0..count {
            if room.w < 3 || room.h < 3 {
                continue;
            }
            let mx = rng.range_u8(room.x + 1, room.x + room.w - 1);
            let my = rng.range_u8(room.y + 1, room.y + room.h - 1);
            if entities.entity_at(mx, my) != NO_ENTITY {
                continue;
            }
            let kind = pick_monster_kind(rng);
            entities.spawn_monster(kind, mx, my, monster_table::ai_behavior(kind));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_monster_kind_returns_valid_kind() {
        let mut rng = LfsrRng16::new(42);
        for _ in 0..100 {
            let kind = pick_monster_kind(&mut rng);
            match kind {
                MonsterKind::Goblin | MonsterKind::Orc | MonsterKind::Troll => {}
            }
        }
    }

    #[test]
    fn spawn_monsters_populates_entities() {
        let mut rng = LfsrRng16::new(42);
        let mut map = MicroMap::new_default();
        map.generate(&mut rng);

        let mut entities = EntityStore::new();
        entities.spawn_player(map.rooms[0].cx(), map.rooms[0].cy());

        let before = entities.count;
        spawn_monsters(&mut entities, &map, &mut rng);
        assert!(
            entities.count > before,
            "should have spawned at least one monster"
        );
    }

    #[test]
    fn spawn_monsters_skips_room_zero() {
        let mut rng = LfsrRng16::new(42);
        let mut map = MicroMap::new_default();
        let (sx, sy) = map.generate(&mut rng);

        let mut entities = EntityStore::new();
        entities.spawn_player(sx, sy);
        spawn_monsters(&mut entities, &map, &mut rng);

        // No monster should be exactly at the player's start position
        assert_eq!(entities.monster_at(sx, sy), NO_ENTITY);
    }

    #[test]
    fn spawn_does_not_exceed_max_entities() {
        let mut rng = LfsrRng16::new(42);
        let mut map = MicroMap::new_default();
        map.generate(&mut rng);

        let mut entities = EntityStore::new();
        entities.spawn_player(map.rooms[0].cx(), map.rooms[0].cy());
        spawn_monsters(&mut entities, &map, &mut rng);
        assert!((entities.count as usize) <= MAX_ENTITIES);
    }
}
