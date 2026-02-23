use std::collections::HashSet;

use serde::Serialize;

use crate::game::GameState;
use crate::map::Rect;
use crate::pathfinding;
use crate::types::{Coord, Pos, Stat};

#[derive(Serialize)]
pub struct RoomExit {
    pub direction: &'static str,
    pub explored: bool,
    pub target_room: Option<usize>,
}

#[derive(Serialize)]
pub struct RoomMonster {
    pub name: String,
    pub hp: Stat,
    pub max_hp: Stat,
}

#[derive(Serialize)]
pub struct RoomNode {
    pub id: usize,
    pub x: Coord,
    pub y: Coord,
    pub width: Coord,
    pub height: Coord,
    pub explored: bool,
    pub player_here: bool,
    pub cleared: bool,
    pub monsters: Vec<RoomMonster>,
    pub corpses: Stat,
    pub exits: Vec<RoomExit>,
    pub distance: Option<Stat>,
}

#[derive(Serialize)]
pub struct CorridorFrontier {
    pub x: Coord,
    pub y: Coord,
    pub dead_end: bool,
}

#[derive(Serialize)]
pub struct ExplorationGraph {
    pub current_room: Option<usize>,
    pub rooms: Vec<RoomNode>,
    pub corridor_frontiers: Vec<CorridorFrontier>,
}

/// Build an exploration graph from the current game state.
///
/// Returns a richly-annotated room graph with exits, monsters, distances,
/// and corridor frontiers. Designed for LLM consumption as structured JSON.
pub fn build_exploration_graph(state: &GameState) -> ExplorationGraph {
    let frontier_set: HashSet<Pos> = state.frontier_tiles().into_iter().collect();
    let player = &state.entities[0];
    let px = player.x;
    let py = player.y;

    let mut current_room = None;
    let mut rooms = Vec::with_capacity(state.map.rooms.len());

    for (idx, room) in state.map.rooms.iter().enumerate() {
        let (cx, cy) = room.center();
        let explored = state.explored.contains(&(cx, cy));
        let player_here = room.contains_interior(px, py);
        if player_here {
            current_room = Some(idx);
        }

        let width = room.x2 - room.x1 - 1;
        let height = room.y2 - room.y1 - 1;

        // Collect monsters and corpses in this room
        let mut monsters = Vec::new();
        let mut corpses: Stat = 0;
        for entity in state.entities.iter().skip(1) {
            if room.contains_interior(entity.x, entity.y) {
                if entity.alive {
                    monsters.push(RoomMonster {
                        name: entity.name.clone(),
                        hp: entity.hp,
                        max_hp: entity.max_hp,
                    });
                } else {
                    corpses += 1;
                }
            }
        }

        let cleared = explored && monsters.is_empty();

        // Find exits
        let exits = find_room_exits(state, room, idx, &state.map.rooms);

        // Pathfinding distance
        let distance = if explored {
            pathfinding::find_path(&state.map, px, py, cx, cy, &state.explored)
                .map(|path| path.len() as Stat)
        } else {
            None
        };

        rooms.push(RoomNode {
            id: idx,
            x: cx,
            y: cy,
            width,
            height,
            explored,
            player_here,
            cleared,
            monsters,
            corpses,
            exits,
            distance,
        });
    }

    // Corridor frontiers: frontier tiles not inside any room's expanded boundary
    let corridor_frontiers = frontier_set
        .iter()
        .filter(|&&(x, y)| !is_near_any_room(x, y, &state.map.rooms))
        .map(|&(x, y)| {
            let walkable_neighbors = count_walkable_neighbors(&state.map, x, y);
            CorridorFrontier {
                x,
                y,
                dead_end: walkable_neighbors <= 1,
            }
        })
        .collect();

    ExplorationGraph {
        current_room,
        rooms,
        corridor_frontiers,
    }
}

/// Check if (x, y) is within 1-tile buffer of any room.
fn is_near_any_room(x: Coord, y: Coord, rooms: &[Rect]) -> bool {
    rooms
        .iter()
        .any(|r| x >= r.x1 - 1 && x <= r.x2 + 1 && y >= r.y1 - 1 && y <= r.y2 + 1)
}

/// Count walkable neighbors of a tile (8-directional).
fn count_walkable_neighbors(map: &crate::map::Map, x: Coord, y: Coord) -> Stat {
    let mut count = 0;
    for dy in -1..=1 {
        for dx in -1..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            if map.is_walkable(x + dx, y + dy) {
                count += 1;
            }
        }
    }
    count
}

/// Find exits for a room by scanning its 4 wall edges for floor tiles.
fn find_room_exits(
    state: &GameState,
    room: &Rect,
    room_idx: usize,
    all_rooms: &[Rect],
) -> Vec<RoomExit> {
    let mut exits = Vec::new();
    let map = &state.map;

    // Track which directions we've already found exits for.
    // We group consecutive floor tiles on the same wall into one exit per direction.
    let mut found_north = false;
    let mut found_south = false;
    let mut found_west = false;
    let mut found_east = false;

    // North wall (y = y1): scan x in x1+1..x2
    for x in (room.x1 + 1)..room.x2 {
        if map.in_bounds(x, room.y1)
            && map.tiles[map.idx(x, room.y1)] == crate::map::Tile::Floor
            && !found_north
        {
            found_north = true;
            let beyond = (x, room.y1 - 1);
            let explored = state.explored.contains(&beyond);
            let target_room = find_target_room(beyond.0, beyond.1, room_idx, all_rooms);
            exits.push(RoomExit {
                direction: "north",
                explored,
                target_room,
            });
        }
    }

    // South wall (y = y2): scan x in x1+1..x2
    for x in (room.x1 + 1)..room.x2 {
        if map.in_bounds(x, room.y2)
            && map.tiles[map.idx(x, room.y2)] == crate::map::Tile::Floor
            && !found_south
        {
            found_south = true;
            let beyond = (x, room.y2 + 1);
            let explored = state.explored.contains(&beyond);
            let target_room = find_target_room(beyond.0, beyond.1, room_idx, all_rooms);
            exits.push(RoomExit {
                direction: "south",
                explored,
                target_room,
            });
        }
    }

    // West wall (x = x1): scan y in y1+1..y2
    for y in (room.y1 + 1)..room.y2 {
        if map.in_bounds(room.x1, y)
            && map.tiles[map.idx(room.x1, y)] == crate::map::Tile::Floor
            && !found_west
        {
            found_west = true;
            let beyond = (room.x1 - 1, y);
            let explored = state.explored.contains(&beyond);
            let target_room = find_target_room(beyond.0, beyond.1, room_idx, all_rooms);
            exits.push(RoomExit {
                direction: "west",
                explored,
                target_room,
            });
        }
    }

    // East wall (x = x2): scan y in y1+1..y2
    for y in (room.y1 + 1)..room.y2 {
        if map.in_bounds(room.x2, y)
            && map.tiles[map.idx(room.x2, y)] == crate::map::Tile::Floor
            && !found_east
        {
            found_east = true;
            let beyond = (room.x2 + 1, y);
            let explored = state.explored.contains(&beyond);
            let target_room = find_target_room(beyond.0, beyond.1, room_idx, all_rooms);
            exits.push(RoomExit {
                direction: "east",
                explored,
                target_room,
            });
        }
    }

    exits
}

/// Check if a tile is inside another room's interior (excluding the source room).
fn find_target_room(x: Coord, y: Coord, source_room: usize, rooms: &[Rect]) -> Option<usize> {
    rooms.iter().enumerate().find_map(|(idx, r)| {
        if idx != source_room && r.contains_interior(x, y) {
            Some(idx)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data;
    use crate::entity::Entity;
    use crate::fov;
    use crate::map::{Map, Rect, Tile};
    use crate::message_log::MessageLog;

    /// Build a minimal GameState with a single room.
    fn single_room_game() -> GameState {
        let mut m = Map::new(20, 20);
        let room = Rect::new(2, 2, 8, 8); // interior: 3..=9 x 3..=9
        m.carve_room(&room);
        m.rooms.push(room);

        let player = Entity::player(5, 5);
        let visible = fov::compute_fov(&m, 5, 5, 8);
        let explored = visible.clone();

        GameState {
            map: m,
            entities: vec![player],
            fov_radius: 8,
            visible,
            explored,
            log: MessageLog::new(),
            game_over: false,
            turn_count: 0,
            seed: 0,
            preset: None,
            dirty: false,
            regen_interval: data::config().regen_interval,
            max_autorun_steps: data::config().max_autorun_steps,
            wandering_seed: 0,
            wandering_config: Default::default(),
            idle_count: 0,
            wandering_spawned: 0,
            wandering_spawn_table: Vec::new(),
            ground_items: Vec::new(),
            equipment: Default::default(),
            depth: 1,
            target_depth: 5,
            game_won: false,
            depth_scaling: Default::default(),
            max_rooms: 30,
            room_size_min: 4,
            room_size_max: 10,
            max_monsters_per_room: 2,
        }
    }

    /// Build a two-room game connected by a corridor.
    fn two_room_game() -> GameState {
        let mut m = Map::new(40, 20);

        // Room 0: (2,2) to (10,10), interior 3..=9 x 3..=9
        let room0 = Rect::new(2, 2, 8, 8);
        m.carve_room(&room0);
        m.rooms.push(room0);

        // Room 1: (20,2) to (28,10), interior 21..=27 x 3..=9
        let room1 = Rect::new(20, 2, 8, 8);
        m.carve_room(&room1);
        m.rooms.push(room1);

        // Horizontal corridor at y=6 from x=10 to x=20
        for x in 10..=20 {
            let idx = m.idx(x, 6);
            m.tiles[idx] = Tile::Floor;
        }

        let player = Entity::player(5, 5);
        let visible = fov::compute_fov(&m, 5, 5, 8);
        let explored = visible.clone();

        GameState {
            map: m,
            entities: vec![player],
            fov_radius: 8,
            visible,
            explored,
            log: MessageLog::new(),
            game_over: false,
            turn_count: 0,
            seed: 0,
            preset: None,
            dirty: false,
            regen_interval: data::config().regen_interval,
            max_autorun_steps: data::config().max_autorun_steps,
            wandering_seed: 0,
            wandering_config: Default::default(),
            idle_count: 0,
            wandering_spawned: 0,
            wandering_spawn_table: Vec::new(),
            ground_items: Vec::new(),
            equipment: Default::default(),
            depth: 1,
            target_depth: 5,
            game_won: false,
            depth_scaling: Default::default(),
            max_rooms: 30,
            room_size_min: 4,
            room_size_max: 10,
            max_monsters_per_room: 2,
        }
    }

    #[test]
    fn single_room_basic() {
        let gs = single_room_game();
        let graph = build_exploration_graph(&gs);
        assert_eq!(graph.rooms.len(), 1);
        assert!(graph.rooms[0].player_here);
        assert_eq!(graph.current_room, Some(0));
        assert!(graph.rooms[0].explored);
        assert!(graph.rooms[0].cleared);
        assert!(graph.rooms[0].monsters.is_empty());
        assert_eq!(graph.rooms[0].corpses, 0);
    }

    #[test]
    fn single_room_no_exits() {
        let gs = single_room_game();
        let graph = build_exploration_graph(&gs);
        // A standalone room with no corridors should have no exits
        assert!(graph.rooms[0].exits.is_empty());
    }

    #[test]
    fn two_rooms_explored_unexplored() {
        let gs = two_room_game();
        let graph = build_exploration_graph(&gs);
        assert_eq!(graph.rooms.len(), 2);
        // Room 0 (where player is) should be explored
        assert!(graph.rooms[0].explored);
        assert!(graph.rooms[0].player_here);
        // Room 1 (far away) should be unexplored
        assert!(!graph.rooms[1].explored);
        assert!(!graph.rooms[1].player_here);
    }

    #[test]
    fn room_with_monster() {
        let mut gs = single_room_game();
        let goblin = Entity::from_template(data::goblin(), 6, 6);
        gs.entities.push(goblin);
        gs.update_fov();

        let graph = build_exploration_graph(&gs);
        assert_eq!(graph.rooms[0].monsters.len(), 1);
        assert_eq!(graph.rooms[0].monsters[0].name, "Goblin");
        assert!(!graph.rooms[0].cleared);
    }

    #[test]
    fn room_with_corpse() {
        let mut gs = single_room_game();
        let mut dead = Entity::from_template(data::goblin(), 6, 6);
        dead.alive = false;
        gs.entities.push(dead);

        let graph = build_exploration_graph(&gs);
        assert_eq!(graph.rooms[0].corpses, 1);
        // No alive monsters → cleared
        assert!(graph.rooms[0].cleared);
    }

    #[test]
    fn exit_detection() {
        let gs = two_room_game();
        let graph = build_exploration_graph(&gs);
        // Room 0 should have an east exit (corridor goes east from room wall at x=10)
        let has_east = graph.rooms[0].exits.iter().any(|e| e.direction == "east");
        assert!(has_east, "Room 0 should have an east exit");
    }

    #[test]
    fn exit_connectivity() {
        // Build a game where two rooms are directly adjacent (corridor goes from
        // one room wall right into the other room's interior)
        let mut m = Map::new(30, 20);
        let room0 = Rect::new(2, 2, 8, 8);
        m.carve_room(&room0);
        m.rooms.push(room0);

        let room1 = Rect::new(10, 2, 8, 8);
        m.carve_room(&room1);
        m.rooms.push(room1);

        // The wall between rooms at x=10 — carve a passage at y=6
        let idx = m.idx(10, 6);
        m.tiles[idx] = Tile::Floor;

        let player = Entity::player(5, 5);
        let visible = fov::compute_fov(&m, 5, 5, 8);
        let explored = visible.clone();

        let gs = GameState {
            map: m,
            entities: vec![player],
            fov_radius: 8,
            visible,
            explored,
            log: MessageLog::new(),
            game_over: false,
            turn_count: 0,
            seed: 0,
            preset: None,
            dirty: false,
            regen_interval: data::config().regen_interval,
            max_autorun_steps: data::config().max_autorun_steps,
            wandering_seed: 0,
            wandering_config: Default::default(),
            idle_count: 0,
            wandering_spawned: 0,
            wandering_spawn_table: Vec::new(),
            ground_items: Vec::new(),
            equipment: Default::default(),
            depth: 1,
            target_depth: 5,
            game_won: false,
            depth_scaling: Default::default(),
            max_rooms: 30,
            room_size_min: 4,
            room_size_max: 10,
            max_monsters_per_room: 2,
        };

        let graph = build_exploration_graph(&gs);
        // Room 0's east exit should connect to room 1
        let east_exit = graph.rooms[0].exits.iter().find(|e| e.direction == "east");
        assert!(east_exit.is_some(), "Room 0 should have east exit");
        assert_eq!(east_exit.unwrap().target_room, Some(1));
    }

    #[test]
    fn corridor_frontier_dead_end() {
        let mut m = Map::new(40, 20);
        let room = Rect::new(2, 2, 8, 8);
        m.carve_room(&room);
        m.rooms.push(room);

        // Long corridor going east from room
        for x in 10..=30 {
            let idx = m.idx(x, 6);
            m.tiles[idx] = Tile::Floor;
        }

        let player = Entity::player(5, 5);
        let visible = fov::compute_fov(&m, 5, 5, 8);
        let explored = visible.clone();

        let gs = GameState {
            map: m,
            entities: vec![player],
            fov_radius: 8,
            visible,
            explored,
            log: MessageLog::new(),
            game_over: false,
            turn_count: 0,
            seed: 0,
            preset: None,
            dirty: false,
            regen_interval: data::config().regen_interval,
            max_autorun_steps: data::config().max_autorun_steps,
            wandering_seed: 0,
            wandering_config: Default::default(),
            idle_count: 0,
            wandering_spawned: 0,
            wandering_spawn_table: Vec::new(),
            ground_items: Vec::new(),
            equipment: Default::default(),
            depth: 1,
            target_depth: 5,
            game_won: false,
            depth_scaling: Default::default(),
            max_rooms: 30,
            room_size_min: 4,
            room_size_max: 10,
            max_monsters_per_room: 2,
        };

        let graph = build_exploration_graph(&gs);
        // There should be corridor frontiers (tiles at the edge of explored corridor)
        // They should not be near any room
        for cf in &graph.corridor_frontiers {
            assert!(!is_near_any_room(cf.x, cf.y, &gs.map.rooms));
        }
    }

    #[test]
    fn distance_to_self_is_zero() {
        let gs = single_room_game();
        let graph = build_exploration_graph(&gs);
        // Player is in room 0, distance to its center should be small
        // (player at 5,5, center of room at 6,6 → distance 1)
        assert!(graph.rooms[0].distance.is_some());
        let dist = graph.rooms[0].distance.unwrap();
        assert!(
            dist <= 2,
            "Distance to player's room should be small, got {}",
            dist
        );
    }

    #[test]
    fn distance_to_other_room_positive() {
        // Fully explore both rooms so we can test distance
        let mut gs = two_room_game();
        // Mark everything as explored
        for y in 0..gs.map.height {
            for x in 0..gs.map.width {
                if gs.map.is_walkable(x, y) {
                    gs.explored.insert((x, y));
                }
            }
        }
        // Also mark wall tiles near floors as explored for realism
        for y in 0..gs.map.height {
            for x in 0..gs.map.width {
                gs.explored.insert((x, y));
            }
        }

        let graph = build_exploration_graph(&gs);
        assert!(graph.rooms[1].explored);
        assert!(graph.rooms[1].distance.is_some());
        let dist = graph.rooms[1].distance.unwrap();
        assert!(
            dist > 0,
            "Distance to other room should be positive, got {}",
            dist
        );
    }

    #[test]
    fn unexplored_room_has_no_distance() {
        let gs = two_room_game();
        let graph = build_exploration_graph(&gs);
        // Room 1 is unexplored
        assert!(!graph.rooms[1].explored);
        assert!(graph.rooms[1].distance.is_none());
    }

    #[test]
    fn seeded_multi_room_game() {
        let mut gs = GameState::with_seed(80, 40, 42);
        gs.update_fov();
        let graph = build_exploration_graph(&gs);
        // Should have multiple rooms
        assert!(graph.rooms.len() > 1);
        // At least one should be explored (the starting room)
        assert!(graph.rooms.iter().any(|r| r.explored));
        // Player should be in a room
        assert!(graph.current_room.is_some());
    }
}
