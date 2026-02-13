use std::collections::HashSet;

use crate::combat;
use crate::data;
use crate::entity::Entity;
use crate::fov;
use crate::input::GameCommand;
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

        if let Some(target_idx) = self.entity_at(new_x, new_y)
            && target_idx != 0
        {
            combat::melee_attack(&mut self.entities, 0, target_idx, &mut self.log);
            return true;
        }

        if self.map.is_walkable(new_x, new_y) {
            self.entities[0].x = new_x;
            self.entities[0].y = new_y;
            return true;
        }

        false
    }

    /// Dispatch a game command. Returns `true` if the player took an action
    /// (i.e. a turn was consumed), `false` otherwise.
    pub fn handle_command(&mut self, cmd: GameCommand) -> bool {
        match cmd {
            GameCommand::Move { dx, dy } => self.player_move_or_attack(dx, dy),
            GameCommand::Wait => true,
            GameCommand::Quit => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{Entity, EntityKind};
    use crate::map::{Map, Tile};

    /// Build a minimal GameState with a custom open map (no random generation).
    fn test_game() -> GameState {
        let mut m = Map::new(20, 20);
        // Carve a 10x10 open area from (1,1) to (10,10)
        for y in 1..=10 {
            for x in 1..=10 {
                let idx = m.idx(x, y);
                m.tiles[idx] = Tile::Floor;
            }
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
        }
    }

    #[test]
    fn player_is_entities_zero() {
        let gs = test_game();
        assert_eq!(gs.entities[0].kind, EntityKind::Player);
    }

    #[test]
    fn entity_at_finds_living_entity() {
        let mut gs = test_game();
        let monster = Entity::from_template(&data::GOBLIN, 6, 5);
        gs.entities.push(monster);
        assert_eq!(gs.entity_at(6, 5), Some(1));
    }

    #[test]
    fn entity_at_ignores_dead() {
        let mut gs = test_game();
        let mut monster = Entity::from_template(&data::GOBLIN, 6, 5);
        monster.alive = false;
        gs.entities.push(monster);
        assert_eq!(gs.entity_at(6, 5), None);
    }

    #[test]
    fn entity_at_returns_none_for_empty_cell() {
        let gs = test_game();
        assert_eq!(gs.entity_at(3, 3), None);
    }

    #[test]
    fn player_moves_into_open_floor() {
        let mut gs = test_game();
        let acted = gs.player_move_or_attack(1, 0); // move right
        assert!(acted);
        assert_eq!(gs.entities[0].x, 6);
        assert_eq!(gs.entities[0].y, 5);
    }

    #[test]
    fn player_blocked_by_wall() {
        let mut gs = test_game();
        // Move player to edge of open area
        gs.entities[0].x = 1;
        gs.entities[0].y = 1;
        let acted = gs.player_move_or_attack(-1, 0); // into wall at (0,1)
        assert!(!acted);
        assert_eq!(gs.entities[0].x, 1);
    }

    #[test]
    fn player_attacks_monster() {
        let mut gs = test_game();
        let monster = Entity::from_template(&data::ORC, 6, 5);
        let monster_hp = monster.hp;
        gs.entities.push(monster);
        let acted = gs.player_move_or_attack(1, 0); // attack orc at (6,5)
        assert!(acted);
        // Player should not have moved
        assert_eq!(gs.entities[0].x, 5);
        assert_eq!(gs.entities[0].y, 5);
        // Orc should have taken damage (player atk=5, orc def=1, dmg=4)
        assert_eq!(gs.entities[1].hp, monster_hp - 4);
    }

    #[test]
    fn handle_command_move_moves_player() {
        let mut gs = test_game();
        let acted = gs.handle_command(GameCommand::Move { dx: 1, dy: 0 });
        assert!(acted);
        assert_eq!(gs.entities[0].x, 6);
        assert_eq!(gs.entities[0].y, 5);
    }

    #[test]
    fn handle_command_wait_consumes_turn() {
        let mut gs = test_game();
        let acted = gs.handle_command(GameCommand::Wait);
        assert!(acted);
        // Player should not have moved
        assert_eq!(gs.entities[0].x, 5);
        assert_eq!(gs.entities[0].y, 5);
    }

    #[test]
    fn handle_command_quit_does_not_consume_turn() {
        let mut gs = test_game();
        let acted = gs.handle_command(GameCommand::Quit);
        assert!(!acted);
    }
}
