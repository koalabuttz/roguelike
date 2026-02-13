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

/// Why autorun stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AutorunStopReason {
    /// Hit a wall or map edge.
    WallReached,
    /// A new living monster entered the field of view.
    MonsterSpotted,
    /// Player took damage from a monster.
    DamageTaken,
    /// Player died.
    GameOver,
    /// Corridor branched or opened into a room.
    CorridorBranches,
    /// Safety cap on steps reached.
    MaxSteps,
}

/// Result of an autorun sequence — multiple steps collapsed into one call.
#[derive(Debug, Serialize)]
pub struct AutorunResult {
    /// How many tiles the player moved.
    pub steps_taken: i32,
    /// Why the run stopped.
    pub stop_reason: AutorunStopReason,
    /// All messages generated during the run.
    pub messages: Vec<String>,
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
    // --- game stats ---
    pub kills: i32,
    pub total_monsters: i32,
    pub rooms_found: i32,
    pub total_rooms: i32,
    pub explored_pct: i32,
}

/// Result of an auto-fight sequence — combat resolved in one call.
#[derive(Debug, Serialize)]
pub struct AutoFightResult {
    /// How many rounds (full turns) the fight lasted.
    pub rounds: i32,
    /// Name of the monster fought.
    pub target_name: String,
    /// Whether the target was killed.
    pub target_killed: bool,
    /// Total HP the player lost during the fight (from all sources).
    pub player_hp_lost: i32,
    /// All messages generated during the fight.
    pub messages: Vec<String>,
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
            // Autorun is handled at a higher level (main loop / MCP act).
            GameCommand::Autorun { .. } | GameCommand::Quit => false,
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

    /// Run in a direction until something interesting happens.
    ///
    /// Repeatedly calls `step(Move{dx,dy})`, stopping when:
    /// - Wall ahead (can't move forward)
    /// - A new monster enters FOV
    /// - Player takes damage
    /// - Game over
    /// - Corridor branches or room entered (neighbor count changes)
    /// - Safety cap reached
    pub fn autorun(&mut self, dx: i32, dy: i32) -> AutorunResult {
        let max_steps = data::CONFIG.max_autorun_steps;
        let mut steps_taken = 0;
        let mut all_messages = Vec::new();

        // Snapshot the initial walkable-neighbor count (excluding "behind").
        // This lets autorun work when started in a room — it only stops
        // when the topology *changes*.
        let initial_neighbor_count =
            self.map
                .open_neighbors_excluding(self.entities[0].x, self.entities[0].y, -dx, -dy);

        loop {
            if steps_taken >= max_steps {
                return AutorunResult {
                    steps_taken,
                    stop_reason: AutorunStopReason::MaxSteps,
                    messages: all_messages,
                };
            }

            // Don't auto-attack: stop if any living monster is adjacent.
            if self.has_adjacent_monster() {
                return AutorunResult {
                    steps_taken,
                    stop_reason: AutorunStopReason::MonsterSpotted,
                    messages: all_messages,
                };
            }

            // Snapshot state before the step.
            let hp_before = self.entities[0].hp;
            let visible_monsters_before = self.visible_monster_ids();

            let result = self.step(GameCommand::Move { dx, dy });
            all_messages.extend(result.new_messages);

            if !result.action_taken {
                return AutorunResult {
                    steps_taken,
                    stop_reason: AutorunStopReason::WallReached,
                    messages: all_messages,
                };
            }

            steps_taken += 1;

            if result.game_over {
                return AutorunResult {
                    steps_taken,
                    stop_reason: AutorunStopReason::GameOver,
                    messages: all_messages,
                };
            }

            // Damage check — stop immediately so the player can react.
            if self.entities[0].hp < hp_before {
                return AutorunResult {
                    steps_taken,
                    stop_reason: AutorunStopReason::DamageTaken,
                    messages: all_messages,
                };
            }

            // New monster check.
            let visible_monsters_after = self.visible_monster_ids();
            if visible_monsters_after
                .difference(&visible_monsters_before)
                .next()
                .is_some()
            {
                return AutorunResult {
                    steps_taken,
                    stop_reason: AutorunStopReason::MonsterSpotted,
                    messages: all_messages,
                };
            }

            // Forward tile blocked? We've reached the end of the road.
            let px = self.entities[0].x;
            let py = self.entities[0].y;
            if !self.map.is_walkable(px + dx, py + dy) {
                return AutorunResult {
                    steps_taken,
                    stop_reason: AutorunStopReason::WallReached,
                    messages: all_messages,
                };
            }

            // Corridor topology check — stop when surroundings change.
            let current_neighbor_count = self.map.open_neighbors_excluding(px, py, -dx, -dy);
            if current_neighbor_count != initial_neighbor_count {
                return AutorunResult {
                    steps_taken,
                    stop_reason: AutorunStopReason::CorridorBranches,
                    messages: all_messages,
                };
            }
        }
    }

    /// Fight an adjacent monster to the death in one call.
    ///
    /// Picks the adjacent monster with the lowest HP (quickest kill).
    /// Each round is a full `step()`, so other monsters still act.
    /// Stops when the target dies, the player dies, or the target moves away.
    pub fn auto_fight(&mut self) -> Result<AutoFightResult, String> {
        let px = self.entities[0].x;
        let py = self.entities[0].y;

        let target_idx = self
            .entities
            .iter()
            .enumerate()
            .filter(|(i, e)| *i != 0 && e.alive && (e.x - px).abs() <= 1 && (e.y - py).abs() <= 1)
            .min_by_key(|(_, e)| e.hp)
            .map(|(i, _)| i)
            .ok_or_else(|| "No adjacent monster to fight.".to_string())?;

        let hp_before = self.entities[0].hp;
        let target_name = self.entities[target_idx].name.clone();
        let msg_count_before = self.log.len();
        let mut rounds = 0;

        loop {
            if !self.entities[target_idx].alive {
                break;
            }

            // Recompute direction to target each round (safe if target moves)
            let tx = self.entities[target_idx].x;
            let ty = self.entities[target_idx].y;
            let dx = (tx - self.entities[0].x).signum();
            let dy = (ty - self.entities[0].y).signum();

            // Target moved out of melee range — stop
            if (tx - self.entities[0].x).abs() > 1 || (ty - self.entities[0].y).abs() > 1 {
                break;
            }

            let result = self.step(GameCommand::Move { dx, dy });
            rounds += 1;

            if result.game_over {
                break;
            }
        }

        Ok(AutoFightResult {
            rounds,
            target_name,
            target_killed: !self.entities[target_idx].alive,
            player_hp_lost: hp_before - self.entities[0].hp,
            messages: self.log.messages_since(msg_count_before),
        })
    }

    /// True if any living monster is adjacent to the player (within 1 tile).
    pub fn has_adjacent_monster(&self) -> bool {
        let px = self.entities[0].x;
        let py = self.entities[0].y;
        self.entities
            .iter()
            .skip(1)
            .any(|e| e.alive && (e.x - px).abs() <= 1 && (e.y - py).abs() <= 1)
    }

    /// Set of entity indices for living, visible monsters.
    pub fn visible_monster_ids(&self) -> HashSet<usize> {
        self.entities
            .iter()
            .enumerate()
            .filter(|(i, e)| *i != 0 && e.alive && self.visible.contains(&(e.x, e.y)))
            .map(|(i, _)| i)
            .collect()
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

        // --- game stats ---
        let kills = self.entities.iter().skip(1).filter(|e| !e.alive).count() as i32;
        let total_monsters = (self.entities.len() - 1) as i32;
        let rooms_found = self
            .map
            .rooms
            .iter()
            .filter(|r| self.explored.contains(&r.center()))
            .count() as i32;
        let total_rooms = self.map.rooms.len() as i32;
        let floor_count = self.map.floor_count();
        let explored_pct = if floor_count > 0 {
            (self.explored.len() as i32 * 100) / floor_count
        } else {
            0
        };

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
            kills,
            total_monsters,
            rooms_found,
            total_rooms,
            explored_pct,
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

    // --- autorun() tests ---

    /// Build a horizontal corridor: floor from (1, 5) to (18, 5), walls everywhere else.
    fn corridor_game() -> GameState {
        let mut m = Map::new(20, 10);
        for x in 1..=18 {
            let idx = m.idx(x, 5);
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
        }
    }

    #[test]
    fn autorun_stops_at_wall() {
        let mut gs = corridor_game();
        // Player at (5,5), corridor ends at x=18. Running east should reach x=18
        // and stop because the tile ahead (x=19) is a wall.
        let result = gs.autorun(1, 0);
        assert_eq!(result.stop_reason, AutorunStopReason::WallReached);
        assert_eq!(gs.entities[0].x, 18);
        assert_eq!(result.steps_taken, 13);
    }

    #[test]
    fn autorun_stops_when_monster_spotted() {
        let mut gs = corridor_game();
        // Place a goblin at x=14, just outside FOV radius of 8 from (5,5).
        // After moving east a few tiles, the goblin enters FOV.
        let monster = Entity::from_template(&data::GOBLIN, 14, 5);
        gs.entities.push(monster);
        let result = gs.autorun(1, 0);
        assert_eq!(result.stop_reason, AutorunStopReason::MonsterSpotted);
        assert!(gs.entities[0].x < 14); // stopped before reaching monster
    }

    #[test]
    fn autorun_stops_when_adjacent_to_monster() {
        let mut gs = corridor_game();
        // Place a goblin adjacent at (6, 5). Autorun should stop immediately
        // because a monster is right next to us — don't auto-attack.
        let monster = Entity::from_template(&data::GOBLIN, 6, 5);
        gs.entities.push(monster);
        gs.update_fov();
        let result = gs.autorun(1, 0);
        assert_eq!(result.stop_reason, AutorunStopReason::MonsterSpotted);
        assert_eq!(result.steps_taken, 0);
        assert_eq!(gs.entities[0].x, 5); // didn't move
    }

    #[test]
    fn autorun_stops_at_corridor_branch() {
        let mut m = Map::new(20, 10);
        // Horizontal corridor
        for x in 1..=18 {
            let idx = m.idx(x, 5);
            m.tiles[idx] = Tile::Floor;
        }
        // Add a branch going north at x=10
        for y in 1..=4 {
            let idx = m.idx(10, y);
            m.tiles[idx] = Tile::Floor;
        }

        let player = Entity::player(5, 5);
        let visible = fov::compute_fov(&m, 5, 5, 8);
        let explored = visible.clone();

        let mut gs = GameState {
            map: m,
            entities: vec![player],
            fov_radius: 8,
            visible,
            explored,
            log: MessageLog::new(),
            game_over: false,
        };

        let result = gs.autorun(1, 0);
        assert_eq!(result.stop_reason, AutorunStopReason::CorridorBranches);
        // Topology change detected when the NE neighbor (10,4) becomes visible
        // from position (9,5), so the player stops at x=9.
        assert_eq!(gs.entities[0].x, 9);
    }

    #[test]
    fn autorun_respects_max_steps() {
        // Create a very long corridor
        let mut m = Map::new(200, 10);
        for x in 1..=198 {
            let idx = m.idx(x, 5);
            m.tiles[idx] = Tile::Floor;
        }

        let player = Entity::player(5, 5);
        let visible = fov::compute_fov(&m, 5, 5, 8);
        let explored = visible.clone();

        let mut gs = GameState {
            map: m,
            entities: vec![player],
            fov_radius: 8,
            visible,
            explored,
            log: MessageLog::new(),
            game_over: false,
        };

        let result = gs.autorun(1, 0);
        assert_eq!(result.stop_reason, AutorunStopReason::MaxSteps);
        assert_eq!(result.steps_taken, data::CONFIG.max_autorun_steps);
    }

    #[test]
    fn autorun_zero_steps_into_wall() {
        let mut gs = corridor_game();
        // Player at (5,5), run north into wall
        let result = gs.autorun(0, -1);
        assert_eq!(result.stop_reason, AutorunStopReason::WallReached);
        assert_eq!(result.steps_taken, 0);
        assert_eq!(gs.entities[0].x, 5);
        assert_eq!(gs.entities[0].y, 5);
    }

    // --- observe() game stats tests ---

    #[test]
    fn observe_stats_no_monsters() {
        let mut gs = test_game();
        gs.update_fov();
        let obs = gs.observe();
        assert_eq!(obs.kills, 0);
        assert_eq!(obs.total_monsters, 0);
        assert!(obs.explored_pct > 0);
    }

    #[test]
    fn observe_stats_with_kills() {
        let mut gs = test_game();
        let mut dead = Entity::from_template(&data::GOBLIN, 6, 5);
        dead.alive = false;
        gs.entities.push(dead);
        let alive = Entity::from_template(&data::ORC, 7, 5);
        gs.entities.push(alive);
        gs.update_fov();
        let obs = gs.observe();
        assert_eq!(obs.kills, 1);
        assert_eq!(obs.total_monsters, 2);
    }

    #[test]
    fn observe_stats_rooms_found() {
        // Use a real generated map so we have rooms
        let gs = GameState::new(80, 40);
        let obs = gs.observe();
        assert!(obs.total_rooms > 0);
        // Player starts in first room, so at least 1 room found
        assert!(obs.rooms_found >= 1);
        assert!(obs.rooms_found <= obs.total_rooms);
    }

    #[test]
    fn observe_stats_explored_pct_range() {
        let gs = GameState::new(80, 40);
        let obs = gs.observe();
        assert!(obs.explored_pct > 0);
        assert!(obs.explored_pct <= 100);
    }

    // --- auto_fight() tests ---

    #[test]
    fn auto_fight_kills_goblin() {
        let mut gs = test_game();
        let goblin = Entity::from_template(&data::GOBLIN, 6, 5);
        gs.entities.push(goblin);
        gs.update_fov();
        let result = gs.auto_fight().unwrap();
        assert!(result.target_killed);
        assert_eq!(result.target_name, "Goblin");
        // Player ATK=5, Goblin DEF=0 → 5 dmg/hit, Goblin HP=6 → 2 hits to kill
        assert_eq!(result.rounds, 2);
        // Goblin ATK=3, Player DEF=2 → 1 dmg/hit, 1 hit taken (dies on round 2)
        assert_eq!(result.player_hp_lost, 1);
        assert!(!result.messages.is_empty());
    }

    #[test]
    fn auto_fight_kills_orc() {
        let mut gs = test_game();
        let orc = Entity::from_template(&data::ORC, 6, 5);
        gs.entities.push(orc);
        gs.update_fov();
        let result = gs.auto_fight().unwrap();
        assert!(result.target_killed);
        assert_eq!(result.target_name, "Orc");
        // Player ATK=5, Orc DEF=1 → 4 dmg/hit, Orc HP=12 → 3 hits to kill
        assert_eq!(result.rounds, 3);
        // Orc ATK=4, Player DEF=2 → 2 dmg/hit, 2 hits taken (dies on round 3)
        assert_eq!(result.player_hp_lost, 4);
    }

    #[test]
    fn auto_fight_no_adjacent_monster_errors() {
        let mut gs = test_game();
        gs.update_fov();
        let result = gs.auto_fight();
        assert!(result.is_err());
    }

    #[test]
    fn auto_fight_picks_weakest_target() {
        let mut gs = test_game();
        // Place an orc (12 HP) and a goblin (6 HP) adjacent
        let orc = Entity::from_template(&data::ORC, 4, 5);
        gs.entities.push(orc);
        let goblin = Entity::from_template(&data::GOBLIN, 6, 5);
        gs.entities.push(goblin);
        gs.update_fov();
        let result = gs.auto_fight().unwrap();
        // Should fight the goblin (lower HP) first
        assert_eq!(result.target_name, "Goblin");
        assert!(result.target_killed);
    }

    #[test]
    fn auto_fight_player_dies_to_troll() {
        let mut gs = test_game();
        gs.entities[0].hp = 5; // low HP
        gs.entities[0].defense = 0;
        let troll = Entity::from_template(&data::TROLL, 6, 5);
        gs.entities.push(troll);
        gs.update_fov();
        let result = gs.auto_fight().unwrap();
        assert!(!result.target_killed);
        assert!(gs.game_over);
    }
}
