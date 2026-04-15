//! Weighted monster and item spawning for the micro tier.
//!
//! Uses `rules::spawn` for weighted selection and depth scaling,
//! `rules::monster_table` and `rules::items` for spawn data.

use super::entity::EntityStore;
use super::item_store::ItemStore;
use super::map::MicroMap;
use super::prng::LfsrRng16;
use super::types::*;
use crate::rules::balance;
use crate::rules::items::{self as rules_items, ItemKind, KIND_COUNT as ITEM_KIND_COUNT};
use crate::rules::monster_table::{self, KIND_COUNT, MonsterKind, SPAWN_KINDS, SPAWN_WEIGHTS};
use crate::rules::spawn as spawn_rules;

/// Pick a random monster kind using the rules/ spawn weights.
pub fn pick_monster_kind(rng: &mut LfsrRng16) -> MonsterKind {
    let total = spawn_rules::total_weight(&SPAWN_WEIGHTS[..KIND_COUNT]);
    if total == 0 {
        return SPAWN_KINDS[0];
    }
    let roll = rng.range_u8(0, total - 1);
    SPAWN_KINDS[spawn_rules::weighted_select(roll, &SPAWN_WEIGHTS[..KIND_COUNT])]
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

/// Pick a random item kind using the rules/ spawn weights, filtered by depth.
pub fn pick_item_kind(rng: &mut LfsrRng16, depth: u8) -> ItemKind {
    let mut weights = [0u8; ITEM_KIND_COUNT];
    for (i, &(kind, weight)) in rules_items::SPAWN_TABLE.iter().enumerate() {
        if rules_items::min_depth(kind) <= depth {
            weights[i] = weight;
        }
    }
    let total = spawn_rules::total_weight(&weights);
    if total == 0 {
        return rules_items::SPAWN_TABLE[0].0;
    }
    let roll = rng.range_u8(0, total - 1);
    let idx = spawn_rules::weighted_select(roll, &weights);
    rules_items::SPAWN_TABLE[idx].0
}

/// Spawn items in rooms (skip room 0 = player start, max 1 per room).
/// Only items with `min_depth <= depth` are eligible.
pub fn spawn_items(items: &mut ItemStore, map: &MicroMap, depth: u8, rng: &mut LfsrRng16) {
    for ri in 1..map.room_count {
        let room = map.rooms[ri as usize];
        let count = rng.range_u8(0, balance::MAX_ITEMS_PER_ROOM);
        for _ in 0..count {
            if room.w < 3 || room.h < 3 {
                continue;
            }
            let ix = rng.range_u8(room.x + 1, room.x + room.w - 1);
            let iy = rng.range_u8(room.y + 1, room.y + room.h - 1);
            let kind = pick_item_kind(rng, depth);
            items.spawn(ix, iy, kind);
        }
    }
}

// ---------------------------------------------------------------------------
// Depth scaling
// ---------------------------------------------------------------------------

/// Apply per-floor stat increases to a single entity at `idx`.
pub fn scale_monster(entities: &mut EntityStore, idx: usize, depth: u8) {
    let (hp_bonus, atk_bonus) = spawn_rules::depth_bonus(depth);
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

        // depth 7 → (7-1)/3 = 2 steps → +2 bonus
        apply_depth_scaling(&mut entities, 7);

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
    fn apply_depth_scaling_noop_within_first_interval() {
        let mut entities = EntityStore::new();
        entities.spawn_player(0, 0);
        entities.spawn_monster(MonsterKind::Goblin, 5, 5, AiBehavior::Chase);

        let base_hp = entities.hp[1];
        apply_depth_scaling(&mut entities, 3);
        assert_eq!(entities.hp[1], base_hp);
    }

    #[test]
    fn pick_item_kind_returns_valid_kind() {
        let mut rng = LfsrRng16::new(42);
        for _ in 0..100 {
            let kind = pick_item_kind(&mut rng, 5);
            match kind {
                ItemKind::HealthPotion
                | ItemKind::ShortSword
                | ItemKind::LeatherArmor
                | ItemKind::IronMace
                | ItemKind::LongSword
                | ItemKind::ChainMail
                | ItemKind::GreaterHealthPotion
                | ItemKind::StrengthPotion => {}
            }
        }
    }

    #[test]
    fn spawn_items_populates_store() {
        let mut rng = LfsrRng16::new(42);
        let mut map = MicroMap::new_default();
        map.generate(&mut rng);

        let mut items = ItemStore::new();
        spawn_items(&mut items, &map, 1, &mut rng);
        assert!(items.count > 0, "should have spawned at least one item");
    }

    #[test]
    fn spawn_items_skips_room_zero() {
        let mut rng = LfsrRng16::new(42);
        let mut map = MicroMap::new_default();
        let (sx, sy) = map.generate(&mut rng);

        let mut items = ItemStore::new();
        spawn_items(&mut items, &map, 1, &mut rng);

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
