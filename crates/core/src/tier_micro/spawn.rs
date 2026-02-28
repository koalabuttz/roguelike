//! Weighted monster and item spawning for the micro tier.
//!
//! Uses `rules::monster_table` and `rules::items` spawn weights and stat lookups.

use super::entity::EntityStore;
use super::item_store::ItemStore;
use super::map::MicroMap;
use super::prng::LfsrRng16;
use super::types::*;
use crate::rules::balance;
use crate::rules::items::{self as rules_items, ItemKind, KIND_COUNT as ITEM_KIND_COUNT};
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

// ---------------------------------------------------------------------------
// Item spawning
// ---------------------------------------------------------------------------

/// Pick a random item kind using the rules/ spawn weights.
pub fn pick_item_kind(rng: &mut LfsrRng16) -> ItemKind {
    let mut total: u8 = 0;
    for i in 0..ITEM_KIND_COUNT {
        total = total.saturating_add(rules_items::SPAWN_TABLE[i].1);
    }
    let mut roll = rng.range_u8(0, total - 1);
    for i in 0..ITEM_KIND_COUNT {
        let (kind, weight) = rules_items::SPAWN_TABLE[i];
        if roll < weight {
            return kind;
        }
        roll -= weight;
    }
    rules_items::SPAWN_TABLE[0].0
}

/// Spawn items in rooms (skip room 0 = player start, max 1 per room).
pub fn spawn_items(items: &mut ItemStore, map: &MicroMap, rng: &mut LfsrRng16) {
    for ri in 1..map.room_count {
        let room = map.rooms[ri as usize];
        let count = rng.range_u8(0, balance::MAX_ITEMS_PER_ROOM);
        for _ in 0..count {
            if room.w < 3 || room.h < 3 {
                continue;
            }
            let ix = rng.range_u8(room.x + 1, room.x + room.w - 1);
            let iy = rng.range_u8(room.y + 1, room.y + room.h - 1);
            let kind = pick_item_kind(rng);
            items.spawn(ix, iy, kind);
        }
    }
}

// ---------------------------------------------------------------------------
// Depth scaling
// ---------------------------------------------------------------------------

/// Apply per-floor stat increases to a single entity at `idx`.
pub fn scale_monster(entities: &mut EntityStore, idx: usize, depth: u8) {
    if depth <= 1 {
        return;
    }
    let hp_bonus = balance::MONSTER_HP_PER_FLOOR.saturating_mul(depth - 1);
    let atk_bonus = balance::MONSTER_ATK_PER_FLOOR.saturating_mul(depth - 1);
    entities.hp[idx] = entities.hp[idx].saturating_add(hp_bonus);
    entities.max_hp[idx] = entities.max_hp[idx].saturating_add(hp_bonus);
    entities.atk[idx] = entities.atk[idx].saturating_add(atk_bonus);
}

/// Apply per-floor stat increases to spawned monsters (slots 1..count).
pub fn apply_depth_scaling(entities: &mut EntityStore, depth: u8) {
    for i in 1..entities.count as usize {
        scale_monster(entities, i, depth);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::monster_table::AiBehavior;

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
    fn apply_depth_scaling_increases_stats() {
        let mut entities = EntityStore::new();
        entities.spawn_player(0, 0);
        entities.spawn_monster(MonsterKind::Goblin, 5, 5, AiBehavior::Chase);

        let base_hp = entities.hp[1];
        let base_atk = entities.atk[1];

        apply_depth_scaling(&mut entities, 3); // depth 3 → +2 bonus

        assert_eq!(entities.hp[1], base_hp + 2);
        assert_eq!(entities.max_hp[1], base_hp + 2);
        assert_eq!(entities.atk[1], base_atk + 2);
        // Player should be unchanged
        assert_eq!(entities.hp[0], balance::PLAYER_HP);
        assert_eq!(entities.atk[0], balance::PLAYER_ATK);
    }

    #[test]
    fn apply_depth_scaling_noop_at_depth_1() {
        let mut entities = EntityStore::new();
        entities.spawn_player(0, 0);
        entities.spawn_monster(MonsterKind::Goblin, 5, 5, AiBehavior::Chase);

        let base_hp = entities.hp[1];
        apply_depth_scaling(&mut entities, 1);
        assert_eq!(entities.hp[1], base_hp);
    }

    #[test]
    fn pick_item_kind_returns_valid_kind() {
        let mut rng = LfsrRng16::new(42);
        for _ in 0..100 {
            let kind = pick_item_kind(&mut rng);
            match kind {
                ItemKind::HealthPotion | ItemKind::ShortSword | ItemKind::LeatherArmor => {}
            }
        }
    }

    #[test]
    fn spawn_items_populates_store() {
        let mut rng = LfsrRng16::new(42);
        let mut map = MicroMap::new_default();
        map.generate(&mut rng);

        let mut items = ItemStore::new();
        spawn_items(&mut items, &map, &mut rng);
        assert!(items.count > 0, "should have spawned at least one item");
    }

    #[test]
    fn spawn_items_skips_room_zero() {
        let mut rng = LfsrRng16::new(42);
        let mut map = MicroMap::new_default();
        let (sx, sy) = map.generate(&mut rng);

        let mut items = ItemStore::new();
        spawn_items(&mut items, &map, &mut rng);

        // No item should be at the player start room center
        use crate::tier_micro::item_store::NO_ITEM;
        assert_eq!(
            items.item_at(sx, sy),
            NO_ITEM,
            "no item at player start position"
        );
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
