use std::collections::HashSet;

use serde::Serialize;

use crate::ai;
use crate::combat;
use crate::data;
use crate::entity::{Entity, EntityKind};
use crate::fov;
use crate::input::GameCommand;
use crate::map;
use crate::message_log::MessageLog;
use crate::spawn;

/// Result of executing one complete game step (player command + monster turns).
pub struct StepResult {
    /// Whether the player's command consumed a turn.
    pub action_taken: bool,
    /// Messages generated during this step (combat, deaths, etc.).
    pub new_messages: Vec<String>,
    /// Whether the game ended this step (player died).
    pub game_over: bool,
}

/// A snapshot of the visible game state, suitable for serialization.
#[derive(Serialize)]
pub struct GameObservation {
    pub player_hp: i32,
    pub player_max_hp: i32,
    pub player_attack: i32,
    pub player_defense: i32,
    pub player_x: i32,
    pub player_y: i32,
    pub map_ascii: Vec<String>,
    pub visible_entities: Vec<EntityInfo>,
    pub recent_messages: Vec<String>,
    pub game_over: bool,
}

/// Info about a visible entity (monster or corpse).
#[derive(Serialize)]
pub struct EntityInfo {
    pub name: String,
    pub glyph: char,
    pub x: i32,
    pub y: i32,
    pub hp: i32,
    pub max_hp: i32,
    pub alive: bool,
}

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

    /// Execute one complete game step: player command, FOV update, monster turns.
    ///
    /// This is the atomic turn operation used by the MCP server and any other
    /// non-terminal consumer. It bundles the logic that `main.rs` performs
    /// across multiple calls into a single method.
    pub fn step(&mut self, cmd: GameCommand) -> StepResult {
        let msg_count_before = self.log.len();
        let action_taken = self.handle_command(cmd);

        if action_taken {
            self.update_fov();
            if ai::run_monster_turns(&mut self.entities, &self.map, &self.visible, &mut self.log) {
                self.game_over = true;
            }
        }

        StepResult {
            action_taken,
            new_messages: self.log.messages_since(msg_count_before),
            game_over: self.game_over,
        }
    }

    /// Produce a snapshot of the current visible game state.
    ///
    /// Returns structured data suitable for JSON serialization. The ASCII map
    /// shows only visible tiles (non-visible positions are spaces) with entity
    /// glyphs overlaid, matching the terminal renderer's behavior.
    pub fn observe(&self) -> GameObservation {
        let player = &self.entities[0];

        // Build ASCII map — only rows with visible content
        let mut map_lines = Vec::new();
        for y in 0..self.map.height {
            let mut line = String::with_capacity(self.map.width as usize);
            let mut has_content = false;

            for x in 0..self.map.width {
                if self.visible.contains(&(x, y)) {
                    has_content = true;
                    // Check for entities (alive first, then dead — matching render order)
                    if let Some(glyph) = self.glyph_at(x, y) {
                        line.push(glyph);
                    } else {
                        match self.map.tiles[self.map.idx(x, y)] {
                            map::Tile::Floor => line.push('.'),
                            map::Tile::Wall => line.push('#'),
                        }
                    }
                } else {
                    line.push(' ');
                }
            }

            if has_content {
                map_lines.push(line.trim_end().to_string());
            }
        }

        // Visible entities (excluding player)
        let visible_entities: Vec<EntityInfo> = self
            .entities
            .iter()
            .filter(|e| e.kind != EntityKind::Player && self.visible.contains(&(e.x, e.y)))
            .map(|e| EntityInfo {
                name: e.name.clone(),
                glyph: if e.alive { e.glyph } else { '%' },
                x: e.x,
                y: e.y,
                hp: e.hp,
                max_hp: e.max_hp,
                alive: e.alive,
            })
            .collect();

        GameObservation {
            player_hp: player.hp,
            player_max_hp: player.max_hp,
            player_attack: player.attack,
            player_defense: player.defense,
            player_x: player.x,
            player_y: player.y,
            map_ascii: map_lines,
            visible_entities,
            recent_messages: self.log.recent(10).to_vec(),
            game_over: self.game_over,
        }
    }

    /// Get the display glyph for the topmost entity at (x, y).
    /// Living entities take priority over dead ones (corpses).
    fn glyph_at(&self, x: i32, y: i32) -> Option<char> {
        // Alive entity on top
        if let Some(idx) = self
            .entities
            .iter()
            .position(|e| e.alive && e.x == x && e.y == y)
        {
            return Some(self.entities[idx].glyph);
        }
        // Dead entity as corpse
        if self
            .entities
            .iter()
            .any(|e| !e.alive && e.x == x && e.y == y)
        {
            return Some('%');
        }
        None
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

    // --- step() tests ---

    #[test]
    fn step_move_advances_turn() {
        let mut gs = test_game();
        let result = gs.step(GameCommand::Move { dx: 1, dy: 0 });
        assert!(result.action_taken);
        assert!(!result.game_over);
        assert_eq!(gs.entities[0].x, 6);
    }

    #[test]
    fn step_into_wall_does_not_advance() {
        let mut gs = test_game();
        gs.entities[0].x = 1;
        gs.entities[0].y = 1;
        let result = gs.step(GameCommand::Move { dx: -1, dy: 0 });
        assert!(!result.action_taken);
        assert_eq!(gs.entities[0].x, 1);
    }

    #[test]
    fn step_includes_monster_turn() {
        let mut gs = test_game();
        let monster = Entity::from_template(&data::GOBLIN, 6, 5);
        gs.entities.push(monster);
        gs.update_fov();
        let hp_before = gs.entities[0].hp;
        let result = gs.step(GameCommand::Wait);
        assert!(result.action_taken);
        // Monster should have attacked (or moved closer), generating messages
        assert!(!result.new_messages.is_empty());
        // Goblin adjacent → attacks: dmg = max(0, 3 - 2) = 1
        assert_eq!(gs.entities[0].hp, hp_before - 1);
    }

    #[test]
    fn step_reports_game_over_on_death() {
        let mut gs = test_game();
        gs.entities[0].hp = 1;
        gs.entities[0].defense = 0;
        let monster = Entity::from_template(&data::GOBLIN, 6, 5);
        gs.entities.push(monster);
        gs.update_fov();
        let result = gs.step(GameCommand::Wait);
        assert!(result.game_over);
        assert!(gs.game_over);
    }

    #[test]
    fn step_captures_only_new_messages() {
        let mut gs = test_game();
        gs.log.add("pre-existing message");
        let monster = Entity::from_template(&data::GOBLIN, 6, 5);
        gs.entities.push(monster);
        gs.update_fov();
        let result = gs.step(GameCommand::Wait);
        // new_messages should not include the pre-existing message
        assert!(
            !result
                .new_messages
                .iter()
                .any(|m| m == "pre-existing message")
        );
        assert!(!result.new_messages.is_empty());
    }

    // --- observe() tests ---

    #[test]
    fn observe_includes_player_stats() {
        let mut gs = test_game();
        gs.update_fov();
        let obs = gs.observe();
        assert_eq!(obs.player_hp, 30);
        assert_eq!(obs.player_max_hp, 30);
        assert_eq!(obs.player_attack, 5);
        assert_eq!(obs.player_defense, 2);
        assert!(!obs.game_over);
    }

    #[test]
    fn observe_map_contains_player_glyph() {
        let mut gs = test_game();
        gs.update_fov();
        let obs = gs.observe();
        let map_text = obs.map_ascii.join("\n");
        assert!(map_text.contains('@'));
    }

    #[test]
    fn observe_shows_visible_monsters() {
        let mut gs = test_game();
        let monster = Entity::from_template(&data::GOBLIN, 6, 5);
        gs.entities.push(monster);
        gs.update_fov();
        let obs = gs.observe();
        assert_eq!(obs.visible_entities.len(), 1);
        assert_eq!(obs.visible_entities[0].name, "Goblin");
        assert!(obs.visible_entities[0].alive);
    }

    #[test]
    fn observe_hides_non_visible_monsters() {
        let mut gs = test_game();
        // Place monster far away, outside the carved area and FOV
        let monster = Entity::from_template(&data::GOBLIN, 19, 19);
        gs.entities.push(monster);
        gs.update_fov();
        let obs = gs.observe();
        assert!(obs.visible_entities.is_empty());
    }

    #[test]
    fn observe_shows_corpses() {
        let mut gs = test_game();
        let mut corpse = Entity::from_template(&data::GOBLIN, 6, 5);
        corpse.alive = false;
        gs.entities.push(corpse);
        gs.update_fov();
        let obs = gs.observe();
        assert_eq!(obs.visible_entities.len(), 1);
        assert!(!obs.visible_entities[0].alive);
        assert_eq!(obs.visible_entities[0].glyph, '%');
    }

    #[test]
    fn observe_serializes_to_json() {
        let mut gs = test_game();
        gs.update_fov();
        let obs = gs.observe();
        let json = serde_json::to_string(&obs);
        assert!(json.is_ok());
    }

    #[test]
    fn observe_includes_recent_messages() {
        let mut gs = test_game();
        gs.update_fov();
        gs.log.add("Test message");
        let obs = gs.observe();
        assert!(obs.recent_messages.iter().any(|m| m == "Test message"));
    }

    // --- glyph_at() tests ---

    #[test]
    fn glyph_at_returns_alive_entity() {
        let mut gs = test_game();
        let monster = Entity::from_template(&data::GOBLIN, 6, 5);
        gs.entities.push(monster);
        assert_eq!(gs.glyph_at(6, 5), Some('g'));
    }

    #[test]
    fn glyph_at_returns_corpse_for_dead() {
        let mut gs = test_game();
        let mut monster = Entity::from_template(&data::GOBLIN, 6, 5);
        monster.alive = false;
        gs.entities.push(monster);
        assert_eq!(gs.glyph_at(6, 5), Some('%'));
    }

    #[test]
    fn glyph_at_alive_over_dead() {
        let mut gs = test_game();
        let mut dead = Entity::from_template(&data::GOBLIN, 6, 5);
        dead.alive = false;
        gs.entities.push(dead);
        let alive = Entity::from_template(&data::ORC, 6, 5);
        gs.entities.push(alive);
        // Alive entity should win
        assert_eq!(gs.glyph_at(6, 5), Some('o'));
    }

    #[test]
    fn glyph_at_empty_cell() {
        let gs = test_game();
        assert_eq!(gs.glyph_at(3, 3), None);
    }
}
