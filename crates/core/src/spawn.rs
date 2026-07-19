use rand::Rng;

use crate::data::MonsterDef;
use crate::entity::Entity;
use crate::item::{self, Item, ItemKind};
use crate::map::Map;
use crate::rules::monster_table as mt;
use crate::rules::spawn as rules_spawn;
use crate::types::Stat;

fn apply_coward_roll(entity: &mut Entity, template: &MonsterDef, rng: &mut impl Rng) {
    let Some(kind) = template.monster_kind() else {
        return;
    };
    let chance = mt::coward_chance(kind);
    if chance == 0 {
        return;
    }
    let roll: u8 = rng.gen_range(0..100);
    entity.ai = rules_spawn::roll_coward(chance, roll, entity.ai);
}

/// Pick a random monster from the weighted spawn table.
///
/// Returns `None` if the table is empty or all weights are zero.
pub fn pick_monster(table: &[MonsterDef], rng: &mut impl Rng) -> Option<Entity> {
    let entries: Vec<&MonsterDef> = table.iter().filter(|m| m.spawn_weight > 0).collect();
    let total_weight: u32 = entries.iter().map(|e| e.spawn_weight).sum();
    if total_weight == 0 {
        return None;
    }

    let mut roll = rng.gen_range(0..total_weight);
    for entry in &entries {
        if roll < entry.spawn_weight {
            let mut e = Entity::from_template(entry, 0, 0);
            apply_coward_roll(&mut e, entry, rng);
            return Some(e);
        }
        roll -= entry.spawn_weight;
    }
    None
}

/// Spawn monsters in each room (except room 0, where the player starts).
/// Uses the weighted spawn table to pick monster types.
pub fn spawn_monsters(
    map: &Map,
    table: &[MonsterDef],
    max_per_room: Stat,
    rng: &mut impl Rng,
) -> Vec<Entity> {
    let mut monsters = Vec::new();

    let entries: Vec<&MonsterDef> = table.iter().filter(|m| m.spawn_weight > 0).collect();
    let total_weight: u32 = entries.iter().map(|e| e.spawn_weight).sum();
    if total_weight == 0 {
        return monsters;
    }

    for room in map.rooms.iter().skip(1) {
        let count = rng.gen_range(0..=max_per_room);
        for _ in 0..count {
            let x = rng.gen_range(room.x1 + 1..room.x2);
            let y = rng.gen_range(room.y1 + 1..room.y2);

            let mut roll = rng.gen_range(0..total_weight);
            for entry in &entries {
                if roll < entry.spawn_weight {
                    let mut e = Entity::from_template(entry, x, y);
                    apply_coward_roll(&mut e, entry, rng);
                    monsters.push(e);
                    break;
                }
                roll -= entry.spawn_weight;
            }
        }
    }

    monsters
}

/// Pick a random item kind from the weighted spawn table.
///
/// Returns `None` if the table is empty or all weights are zero.
pub fn pick_item(table: &[(ItemKind, u32)], rng: &mut impl Rng) -> Option<ItemKind> {
    let total_weight: u32 = table.iter().map(|(_, w)| w).sum();
    if total_weight == 0 {
        return None;
    }

    let mut roll = rng.gen_range(0..total_weight);
    for &(kind, weight) in table {
        if roll < weight {
            return Some(kind);
        }
        roll -= weight;
    }
    None
}

/// Spawn items in each room (except room 0, where the player starts).
/// Uses the weighted spawn table filtered by `depth` to pick item types.
pub fn spawn_items(
    map: &Map,
    max_per_room: Stat,
    depth: u8,
    catalog: &[crate::data::ItemDef],
    rng: &mut impl Rng,
) -> Vec<Item> {
    let mut items = Vec::new();
    let table = item::spawn_table_for_depth_with(catalog, depth);
    if table.is_empty() {
        return items;
    }

    for room in map.rooms.iter().skip(1) {
        let count = rng.gen_range(0..=max_per_room);
        for _ in 0..count {
            let x = rng.gen_range(room.x1 + 1..room.x2);
            let y = rng.gen_range(room.y1 + 1..room.y2);

            if let Some(kind) = pick_item(&table, rng) {
                items.push(Item { x, y, kind });
            }
        }
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data;
    use crate::item::ItemKind;
    use crate::map::Rect;
    use rand::{SeedableRng, rngs::StdRng};

    fn map_with_rooms(rooms: Vec<Rect>) -> Map {
        let mut m = Map::new(80, 50);
        for room in &rooms {
            // Carve floors inside rooms so spawn positions are valid
            for y in (room.y1 + 1)..room.y2 {
                for x in (room.x1 + 1)..room.x2 {
                    if m.in_bounds(x, y) {
                        let idx = m.idx(x, y);
                        m.tiles[idx] = crate::map::Tile::Floor;
                    }
                }
            }
        }
        m.rooms = rooms;
        m
    }

    #[test]
    fn skips_room_zero() {
        let rooms = vec![
            Rect::new(1, 1, 8, 8), // room 0 — player start
        ];
        let m = map_with_rooms(rooms);
        let mut rng = StdRng::seed_from_u64(42);
        let monsters = spawn_monsters(&m, &data::defaults().monsters, 5, &mut rng);
        assert!(monsters.is_empty());
    }

    #[test]
    fn empty_spawn_table_returns_empty() {
        let rooms = vec![Rect::new(1, 1, 8, 8), Rect::new(20, 20, 8, 8)];
        let m = map_with_rooms(rooms);
        let mut rng = StdRng::seed_from_u64(42);
        let monsters = spawn_monsters(&m, &[], 5, &mut rng);
        assert!(monsters.is_empty());
    }

    #[test]
    fn spawned_entities_are_alive_and_in_room() {
        let rooms = vec![Rect::new(1, 1, 8, 8), Rect::new(20, 20, 8, 8)];
        let m = map_with_rooms(rooms);
        // Run multiple seeds to get some spawns
        for seed in 0..10 {
            let mut rng = StdRng::seed_from_u64(seed);
            let monsters = spawn_monsters(&m, &data::defaults().monsters, 3, &mut rng);
            for monster in &monsters {
                assert!(monster.alive);
                // Should be inside room 1 (room index 1): x in 21..28, y in 21..28
                let room = &m.rooms[1];
                assert!(
                    monster.x > room.x1 && monster.x < room.x2,
                    "monster x={} not in room x range {}..{}",
                    monster.x,
                    room.x1 + 1,
                    room.x2,
                );
                assert!(
                    monster.y > room.y1 && monster.y < room.y2,
                    "monster y={} not in room y range {}..{}",
                    monster.y,
                    room.y1 + 1,
                    room.y2,
                );
            }
        }
    }

    // --- Item spawn tests ---

    #[test]
    fn item_spawn_skips_room_zero() {
        let rooms = vec![Rect::new(1, 1, 8, 8)];
        let m = map_with_rooms(rooms);
        let mut rng = StdRng::seed_from_u64(42);
        let items = spawn_items(&m, 5, 1, &data::defaults().items, &mut rng);
        assert!(items.is_empty());
    }

    #[test]
    fn pick_item_empty_table_returns_none() {
        let mut rng = StdRng::seed_from_u64(42);
        assert!(pick_item(&[], &mut rng).is_none());
    }

    #[test]
    fn spawned_items_are_in_room() {
        let rooms = vec![Rect::new(1, 1, 8, 8), Rect::new(20, 20, 28, 28)];
        let m = map_with_rooms(rooms);
        for seed in 0..10 {
            let mut rng = StdRng::seed_from_u64(seed);
            let items = spawn_items(&m, 3, 1, &data::defaults().items, &mut rng);
            for it in &items {
                let room = &m.rooms[1];
                assert!(
                    it.x > room.x1 && it.x < room.x2,
                    "item x={} not in room x range {}..{}",
                    it.x,
                    room.x1 + 1,
                    room.x2,
                );
                assert!(
                    it.y > room.y1 && it.y < room.y2,
                    "item y={} not in room y range {}..{}",
                    it.y,
                    room.y1 + 1,
                    room.y2,
                );
            }
        }
    }

    #[test]
    fn pick_item_returns_valid_kind() {
        let table = item::spawn_table();
        let mut rng = StdRng::seed_from_u64(42);
        for _ in 0..50 {
            let kind = pick_item(&table, &mut rng).unwrap();
            assert!(matches!(
                kind,
                ItemKind::HealthPotion
                    | ItemKind::ShortSword
                    | ItemKind::LeatherArmor
                    | ItemKind::IronMace
                    | ItemKind::LongSword
                    | ItemKind::ChainMail
                    | ItemKind::GreaterHealthPotion
                    | ItemKind::StrengthPotion
                    | ItemKind::ToughnessPotion
            ));
        }
    }
}
