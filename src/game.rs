use std::collections::HashSet;

use crate::combat;
use crate::data;
use crate::entity::Entity;
use crate::fov;
use crate::map;
use crate::message_log::MessageLog;
use crate::spawn;

pub struct GameState {
    pub map: map::Map,
    pub entities: Vec<Entity>,
    pub fov_radius: i32,
    pub visible: HashSet<(i32, i32)>,
    pub explored: HashSet<(i32, i32)>,
    pub log: MessageLog,
    pub game_over: bool,
}

impl GameState {
    pub fn new(width: i32, height: i32) -> Self {
        let cfg = &data::CONFIG;
        let mut map = map::Map::new(width, height);
        let (px, py) = map.generate(cfg.max_rooms, cfg.room_size_min, cfg.room_size_max);

        let mut entities = vec![Entity::player(px, py)];
        let monsters = spawn::spawn_monsters(&map, data::SPAWN_TABLE, cfg.max_monsters_per_room);
        entities.extend(monsters);

        let visible = fov::compute_fov(&map, px, py, cfg.fov_radius);
        let explored = visible.clone();

        let mut log = MessageLog::new();
        log.add("Welcome to the dungeon! Prepare yourself.");

        GameState {
            map,
            entities,
            fov_radius: cfg.fov_radius,
            visible,
            explored,
            log,
            game_over: false,
        }
    }

    pub fn update_fov(&mut self) {
        let px = self.entities[0].x;
        let py = self.entities[0].y;
        self.visible = fov::compute_fov(&self.map, px, py, self.fov_radius);
        self.explored.extend(&self.visible);
    }

    /// Find a living entity at (x, y). Returns its index.
    pub fn entity_at(&self, x: i32, y: i32) -> Option<usize> {
        self.entities
            .iter()
            .position(|e| e.alive && e.x == x && e.y == y)
    }

    /// Try to move the player. If a living monster occupies the target cell, attack it instead.
    /// Returns true if the player took an action (moved or attacked).
    pub fn player_move_or_attack(&mut self, dx: i32, dy: i32) -> bool {
        let new_x = self.entities[0].x + dx;
        let new_y = self.entities[0].y + dy;

        if let Some(target_idx) = self.entity_at(new_x, new_y) {
            if target_idx != 0 {
                combat::melee_attack(&mut self.entities, 0, target_idx, &mut self.log);
                return true;
            }
        }

        if self.map.is_walkable(new_x, new_y) {
            self.entities[0].x = new_x;
            self.entities[0].y = new_y;
            return true;
        }

        false
    }
}
