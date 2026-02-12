use rand::Rng;

use crate::data::SpawnEntry;
use crate::entity::Entity;
use crate::map::Map;

/// Spawn monsters in each room (except room 0, where the player starts).
/// Uses the weighted spawn table to pick monster types.
pub fn spawn_monsters(map: &Map, table: &[SpawnEntry], max_per_room: i32) -> Vec<Entity> {
    let mut rng = rand::thread_rng();
    let mut monsters = Vec::new();

    let total_weight: u32 = table.iter().map(|e| e.weight).sum();
    if total_weight == 0 {
        return monsters;
    }

    for room in map.rooms.iter().skip(1) {
        let count = rng.gen_range(0..=max_per_room);
        for _ in 0..count {
            let x = rng.gen_range(room.x1 + 1..room.x2);
            let y = rng.gen_range(room.y1 + 1..room.y2);

            let mut roll = rng.gen_range(0..total_weight);
            for entry in table {
                if roll < entry.weight {
                    monsters.push(Entity::from_template(entry.template, x, y));
                    break;
                }
                roll -= entry.weight;
            }
        }
    }

    monsters
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data;
    use crate::map::Rect;

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
        let monsters = spawn_monsters(&m, data::SPAWN_TABLE, 5);
        assert!(monsters.is_empty());
    }

    #[test]
    fn empty_spawn_table_returns_empty() {
        let rooms = vec![Rect::new(1, 1, 8, 8), Rect::new(20, 20, 8, 8)];
        let m = map_with_rooms(rooms);
        let monsters = spawn_monsters(&m, &[], 5);
        assert!(monsters.is_empty());
    }

    #[test]
    fn spawned_entities_are_alive_and_in_room() {
        let rooms = vec![Rect::new(1, 1, 8, 8), Rect::new(20, 20, 8, 8)];
        let m = map_with_rooms(rooms);
        // Run multiple times to get some spawns (randomness)
        for _ in 0..10 {
            let monsters = spawn_monsters(&m, data::SPAWN_TABLE, 3);
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
}
