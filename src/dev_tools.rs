use serde::{Deserialize, Serialize};

use crate::data;
use crate::entity::Entity;
use crate::fov;
use crate::game::GameState;
use crate::map::Tile;
use crate::types::{Coord, Stat};

/// Commands available only during development / debug builds.
///
/// These bypass normal gameplay rules to let developers set up specific
/// scenarios quickly. None of these should be accessible in release builds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DevCommand {
    /// Teleport the player to (x, y). Fails silently if target is a wall.
    Teleport { x: Coord, y: Coord },
    /// Set the player's HP to the given value (clamped to 1..=max_hp).
    SetHp { hp: Stat },
    /// Set the player's attack stat.
    SetAttack { attack: Stat },
    /// Set the player's defense stat.
    SetDefense { defense: Stat },
    /// Spawn a monster by name at (x, y). Recognized names: goblin, orc, troll.
    Spawn { name: String, x: Coord, y: Coord },
    /// Reveal the entire map (add all tiles to explored set).
    RevealMap,
    /// Toggle FOV — when disabled, all tiles are visible.
    ToggleFov,
    /// Kill all living monsters on the map.
    KillAll,
    /// Print a summary of the current game state to the message log.
    DumpStats,
    /// Toggle god mode — player takes no damage.
    ToggleGodMode,
}

/// Mutable flags that control debug behavior, stored on GameState.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DevFlags {
    /// When true, all tiles are always visible (FOV disabled).
    pub fov_disabled: bool,
    /// When true, player takes no damage.
    pub god_mode: bool,
}

impl GameState {
    /// Execute a dev command. Returns a message describing what happened.
    pub fn exec_dev(&mut self, cmd: DevCommand) -> String {
        match cmd {
            DevCommand::Teleport { x, y } => self.dev_teleport(x, y),
            DevCommand::SetHp { hp } => self.dev_set_hp(hp),
            DevCommand::SetAttack { attack } => self.dev_set_attack(attack),
            DevCommand::SetDefense { defense } => self.dev_set_defense(defense),
            DevCommand::Spawn { name, x, y } => self.dev_spawn(&name, x, y),
            DevCommand::RevealMap => self.dev_reveal_map(),
            DevCommand::ToggleFov => self.dev_toggle_fov(),
            DevCommand::KillAll => self.dev_kill_all(),
            DevCommand::DumpStats => self.dev_dump_stats(),
            DevCommand::ToggleGodMode => self.dev_toggle_god_mode(),
        }
    }

    fn dev_teleport(&mut self, x: Coord, y: Coord) -> String {
        if !self.map.is_walkable(x, y) {
            return format!("Cannot teleport to ({}, {}): not walkable.", x, y);
        }
        self.entities[0].x = x;
        self.entities[0].y = y;
        self.update_fov();
        format!("Teleported to ({}, {}).", x, y)
    }

    fn dev_set_hp(&mut self, hp: Stat) -> String {
        let clamped = hp.clamp(1, self.entities[0].max_hp);
        self.entities[0].hp = clamped;
        format!("HP set to {}.", clamped)
    }

    fn dev_set_attack(&mut self, attack: Stat) -> String {
        self.entities[0].attack = attack;
        format!("Attack set to {}.", attack)
    }

    fn dev_set_defense(&mut self, defense: Stat) -> String {
        self.entities[0].defense = defense;
        format!("Defense set to {}.", defense)
    }

    fn dev_spawn(&mut self, name: &str, x: Coord, y: Coord) -> String {
        if !self.map.is_walkable(x, y) {
            return format!("Cannot spawn at ({}, {}): not walkable.", x, y);
        }
        let template = match name.to_lowercase().as_str() {
            "goblin" => &data::GOBLIN,
            "orc" => &data::ORC,
            "troll" => &data::TROLL,
            _ => return format!("Unknown monster: '{}'. Use goblin, orc, or troll.", name),
        };
        self.entities.push(Entity::from_template(template, x, y));
        format!("Spawned {} at ({}, {}).", template.name, x, y)
    }

    fn dev_reveal_map(&mut self) -> String {
        for y in 0..self.map.height {
            for x in 0..self.map.width {
                self.explored.insert((x, y));
            }
        }
        "Entire map revealed.".to_string()
    }

    fn dev_toggle_fov(&mut self) -> String {
        self.dev_flags.fov_disabled = !self.dev_flags.fov_disabled;
        if self.dev_flags.fov_disabled {
            // Make all tiles visible.
            for y in 0..self.map.height {
                for x in 0..self.map.width {
                    self.visible.insert((x, y));
                    self.explored.insert((x, y));
                }
            }
            "FOV disabled (all tiles visible).".to_string()
        } else {
            self.update_fov();
            "FOV re-enabled.".to_string()
        }
    }

    fn dev_kill_all(&mut self) -> String {
        let mut killed = 0;
        for entity in self.entities.iter_mut().skip(1) {
            if entity.alive {
                entity.alive = false;
                entity.hp = 0;
                killed += 1;
            }
        }
        format!("Killed {} monsters.", killed)
    }

    fn dev_dump_stats(&self) -> String {
        let living = self.entities.iter().skip(1).filter(|e| e.alive).count();
        let dead = self.entities.iter().skip(1).filter(|e| !e.alive).count();
        let floor_count = self.map.floor_count();
        let explored_floors = self
            .explored
            .iter()
            .filter(|&&(x, y)| {
                self.map.in_bounds(x, y) && self.map.tiles[self.map.idx(x, y)] == Tile::Floor
            })
            .count();
        let p = &self.entities[0];
        format!(
            "Turn {} | Seed {} | HP {}/{} | ATK {} DEF {} | \
             Monsters: {} alive, {} dead | Rooms: {} | \
             Explored: {}/{} floor tiles ({}%) | Flags: fov_off={} god={}",
            self.turn_count,
            self.seed,
            p.hp,
            p.max_hp,
            p.attack,
            p.defense,
            living,
            dead,
            self.map.rooms.len(),
            explored_floors,
            floor_count,
            if floor_count > 0 {
                (explored_floors as i32 * 100) / floor_count
            } else {
                0
            },
            self.dev_flags.fov_disabled,
            self.dev_flags.god_mode,
        )
    }

    fn dev_toggle_god_mode(&mut self) -> String {
        self.dev_flags.god_mode = !self.dev_flags.god_mode;
        if self.dev_flags.god_mode {
            "God mode enabled (invulnerable).".to_string()
        } else {
            "God mode disabled.".to_string()
        }
    }

    /// Override update_fov when FOV is disabled via dev tools.
    pub fn dev_update_fov(&mut self) {
        if self.dev_flags.fov_disabled {
            for y in 0..self.map.height {
                for x in 0..self.map.width {
                    self.visible.insert((x, y));
                }
            }
        } else {
            let px = self.entities[0].x;
            let py = self.entities[0].y;
            self.visible = fov::compute_fov(&self.map, px, py, self.fov_radius);
            self.explored.extend(&self.visible);
        }
    }

    /// Start recording commands (clears any existing log).
    pub fn start_recording(&mut self) {
        self.command_log.clear();
    }

    /// Export the command log as a Replay.
    pub fn export_replay(&self) -> Replay {
        Replay {
            seed: self.seed,
            width: self.map.width,
            height: self.map.height,
            commands: self.command_log.clone(),
            preset: None,
        }
    }

    /// Replay a sequence of commands on this game state.
    /// Returns the number of commands successfully executed.
    pub fn replay_commands(&mut self, commands: &[crate::input::GameCommand]) -> ReplayResult {
        let mut turns_played = 0;
        for &cmd in commands {
            if self.game_over {
                break;
            }
            let result = self.step(cmd);
            if result.action_taken {
                turns_played += 1;
            }
        }
        ReplayResult {
            turns_played,
            game_over: self.game_over,
            final_hp: self.entities[0].hp,
            final_turn: self.turn_count,
            kills: self.entities.iter().skip(1).filter(|e| !e.alive).count() as i32,
        }
    }
}

/// A recorded game session that can be replayed deterministically.
///
/// Contains the seed and dimensions needed to recreate the same map,
/// plus the sequence of commands the player issued.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Replay {
    pub seed: u64,
    pub width: Coord,
    pub height: Coord,
    pub commands: Vec<crate::input::GameCommand>,
    /// Optional map preset used (None = standard generation).
    pub preset: Option<crate::map::MapPreset>,
}

impl Replay {
    /// Serialize to JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserialize from JSON.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Execute this replay and return the result.
    pub fn execute(&self) -> ReplayResult {
        let mut gs = match self.preset {
            Some(preset) => GameState::with_preset(self.width, self.height, self.seed, preset),
            None => GameState::with_seed(self.width, self.height, self.seed),
        };
        gs.replay_commands(&self.commands)
    }
}

/// Summary of a replay execution or headless run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayResult {
    pub turns_played: i32,
    pub game_over: bool,
    pub final_hp: Stat,
    pub final_turn: i32,
    pub kills: i32,
}

/// Result of a headless batch run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRunStats {
    pub games_played: i32,
    pub games_won: i32,
    pub games_lost: i32,
    pub total_turns: i32,
    pub total_kills: i32,
    pub avg_turns_per_game: f64,
    pub avg_kills_per_game: f64,
    pub seeds_used: Vec<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::GameState;
    use crate::map::{Map, Tile};
    use crate::message_log::MessageLog;

    fn test_game() -> GameState {
        let mut m = Map::new(20, 20);
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
            turn_count: 0,
            seed: 0,
            dirty: false,
            dev_flags: DevFlags::default(),
            command_log: Vec::new(),
        }
    }

    #[test]
    fn teleport_to_walkable() {
        let mut gs = test_game();
        let msg = gs.exec_dev(DevCommand::Teleport { x: 8, y: 8 });
        assert!(msg.contains("Teleported"));
        assert_eq!(gs.entities[0].x, 8);
        assert_eq!(gs.entities[0].y, 8);
    }

    #[test]
    fn teleport_to_wall_fails() {
        let mut gs = test_game();
        let msg = gs.exec_dev(DevCommand::Teleport { x: 0, y: 0 });
        assert!(msg.contains("not walkable"));
        assert_eq!(gs.entities[0].x, 5);
    }

    #[test]
    fn set_hp_clamps() {
        let mut gs = test_game();
        gs.exec_dev(DevCommand::SetHp { hp: 999 });
        assert_eq!(gs.entities[0].hp, gs.entities[0].max_hp);
        gs.exec_dev(DevCommand::SetHp { hp: -5 });
        assert_eq!(gs.entities[0].hp, 1);
    }

    #[test]
    fn set_attack() {
        let mut gs = test_game();
        gs.exec_dev(DevCommand::SetAttack { attack: 99 });
        assert_eq!(gs.entities[0].attack, 99);
    }

    #[test]
    fn set_defense() {
        let mut gs = test_game();
        gs.exec_dev(DevCommand::SetDefense { defense: 50 });
        assert_eq!(gs.entities[0].defense, 50);
    }

    #[test]
    fn spawn_known_monster() {
        let mut gs = test_game();
        let msg = gs.exec_dev(DevCommand::Spawn {
            name: "goblin".to_string(),
            x: 3,
            y: 3,
        });
        assert!(msg.contains("Spawned Goblin"));
        assert_eq!(gs.entities.len(), 2);
        assert_eq!(gs.entities[1].name, "Goblin");
    }

    #[test]
    fn spawn_unknown_monster_fails() {
        let mut gs = test_game();
        let msg = gs.exec_dev(DevCommand::Spawn {
            name: "dragon".to_string(),
            x: 3,
            y: 3,
        });
        assert!(msg.contains("Unknown monster"));
        assert_eq!(gs.entities.len(), 1);
    }

    #[test]
    fn spawn_on_wall_fails() {
        let mut gs = test_game();
        let msg = gs.exec_dev(DevCommand::Spawn {
            name: "orc".to_string(),
            x: 0,
            y: 0,
        });
        assert!(msg.contains("not walkable"));
    }

    #[test]
    fn reveal_map_explores_everything() {
        let mut gs = test_game();
        let before = gs.explored.len();
        gs.exec_dev(DevCommand::RevealMap);
        assert!(gs.explored.len() > before);
        // All in-bounds tiles should be explored.
        for y in 0..gs.map.height {
            for x in 0..gs.map.width {
                assert!(gs.explored.contains(&(x, y)));
            }
        }
    }

    #[test]
    fn toggle_fov_disables_and_enables() {
        let mut gs = test_game();
        assert!(!gs.dev_flags.fov_disabled);
        gs.exec_dev(DevCommand::ToggleFov);
        assert!(gs.dev_flags.fov_disabled);
        // All tiles visible.
        assert!(gs.visible.contains(&(0, 0)));
        gs.exec_dev(DevCommand::ToggleFov);
        assert!(!gs.dev_flags.fov_disabled);
    }

    #[test]
    fn kill_all_monsters() {
        let mut gs = test_game();
        gs.entities.push(Entity::from_template(&data::GOBLIN, 3, 3));
        gs.entities.push(Entity::from_template(&data::ORC, 4, 4));
        let msg = gs.exec_dev(DevCommand::KillAll);
        assert!(msg.contains("Killed 2"));
        assert!(gs.entities.iter().skip(1).all(|e| !e.alive));
    }

    #[test]
    fn dump_stats_includes_key_info() {
        let mut gs = test_game();
        let msg = gs.exec_dev(DevCommand::DumpStats);
        assert!(msg.contains("Turn 0"));
        assert!(msg.contains("Seed 0"));
        assert!(msg.contains("HP"));
        assert!(msg.contains("ATK"));
    }

    #[test]
    fn toggle_god_mode() {
        let mut gs = test_game();
        assert!(!gs.dev_flags.god_mode);
        gs.exec_dev(DevCommand::ToggleGodMode);
        assert!(gs.dev_flags.god_mode);
        gs.exec_dev(DevCommand::ToggleGodMode);
        assert!(!gs.dev_flags.god_mode);
    }

    // --- Replay tests ---

    #[test]
    fn recording_captures_commands() {
        let mut gs = test_game();
        gs.start_recording();
        gs.step(crate::input::GameCommand::Move { dx: 1, dy: 0 });
        gs.step(crate::input::GameCommand::Wait);
        // In debug builds, command_log always records; check it has entries.
        assert!(gs.command_log.len() >= 2);
    }

    #[test]
    fn export_replay_roundtrip() {
        let mut gs = test_game();
        gs.start_recording();
        gs.step(crate::input::GameCommand::Move { dx: 1, dy: 0 });
        let replay = gs.export_replay();
        let json = replay.to_json().unwrap();
        let loaded = Replay::from_json(&json).unwrap();
        assert_eq!(loaded.seed, 0);
        assert!(!loaded.commands.is_empty());
    }

    #[test]
    fn replay_deterministic_same_outcome() {
        // Create a game, record some moves, export, then replay and compare.
        let mut gs = GameState::with_seed(40, 30, 42);
        gs.start_recording();
        for _ in 0..5 {
            gs.step(crate::input::GameCommand::Wait);
        }
        let replay = gs.export_replay();
        let result = replay.execute();
        assert_eq!(result.final_turn, gs.turn_count);
    }

    #[test]
    fn replay_with_preset() {
        let replay = Replay {
            seed: 42,
            width: 40,
            height: 30,
            commands: vec![
                crate::input::GameCommand::Wait,
                crate::input::GameCommand::Wait,
            ],
            preset: Some(crate::map::MapPreset::Arena),
        };
        let result = replay.execute();
        assert_eq!(result.turns_played, 2);
    }

    #[test]
    fn replay_commands_stops_on_game_over() {
        let mut gs = test_game();
        gs.entities[0].hp = 1;
        gs.entities[0].attack = 0;
        // Place a strong monster adjacent.
        let mut monster = Entity::from_template(&data::TROLL, 6, 5);
        monster.attack = 100;
        gs.entities.push(monster);
        // Replaying many waits — should stop after player dies.
        let commands: Vec<_> = (0..100).map(|_| crate::input::GameCommand::Wait).collect();
        let result = gs.replay_commands(&commands);
        assert!(result.game_over);
        assert!(result.turns_played < 100);
    }

    // --- with_preset constructor test ---

    #[test]
    fn with_preset_creates_valid_game() {
        let gs = GameState::with_preset(40, 30, 42, crate::map::MapPreset::Arena);
        assert!(gs.entities[0].alive);
        assert!(gs.map.is_walkable(gs.entities[0].x, gs.entities[0].y));
        assert!(gs.log.recent(1)[0].contains("Arena"));
    }
}
