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
