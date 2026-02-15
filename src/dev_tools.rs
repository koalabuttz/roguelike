use serde::{Deserialize, Serialize};

use crate::data;
use crate::entity::Entity;
use crate::game::GameState;
use crate::input::GameCommand;
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

/// Debug state owned by the caller (main loop, headless runner), not by
/// GameState. This keeps the core game struct free of dev-only fields.
#[derive(Debug, Clone, Default)]
pub struct DevSession {
    /// When true, all tiles are always visible (FOV disabled).
    pub fov_disabled: bool,
    /// When true, player takes no damage.
    pub god_mode: bool,
    /// Recorded commands for replay export.
    pub command_log: Vec<GameCommand>,
    /// Whether recording is active.
    pub recording: bool,
}

/// Execute a dev command against the game state. Returns a user-facing message.
///
/// This is a free function — it operates *on* GameState through its public
/// fields rather than being an inherent method, keeping GameState's interface
/// defined entirely in `game.rs` per the method placement rule.
pub fn exec_dev(gs: &mut GameState, session: &mut DevSession, cmd: DevCommand) -> String {
    match cmd {
        DevCommand::Teleport { x, y } => {
            if !gs.map.is_walkable(x, y) {
                return format!("Cannot teleport to ({}, {}): not walkable.", x, y);
            }
            gs.entities[0].x = x;
            gs.entities[0].y = y;
            gs.update_fov();
            if session.fov_disabled {
                apply_fov_override(gs);
            }
            format!("Teleported to ({}, {}).", x, y)
        }
        DevCommand::SetHp { hp } => {
            let clamped = hp.clamp(1, gs.entities[0].max_hp);
            gs.entities[0].hp = clamped;
            format!("HP set to {}.", clamped)
        }
        DevCommand::SetAttack { attack } => {
            gs.entities[0].attack = attack;
            format!("Attack set to {}.", attack)
        }
        DevCommand::SetDefense { defense } => {
            gs.entities[0].defense = defense;
            format!("Defense set to {}.", defense)
        }
        DevCommand::Spawn { name, x, y } => {
            if !gs.map.is_walkable(x, y) {
                return format!("Cannot spawn at ({}, {}): not walkable.", x, y);
            }
            let template = match name.to_lowercase().as_str() {
                "goblin" => &data::GOBLIN,
                "orc" => &data::ORC,
                "troll" => &data::TROLL,
                _ => return format!("Unknown monster: '{}'. Use goblin, orc, or troll.", name),
            };
            gs.entities.push(Entity::from_template(template, x, y));
            format!("Spawned {} at ({}, {}).", template.name, x, y)
        }
        DevCommand::RevealMap => {
            for y in 0..gs.map.height {
                for x in 0..gs.map.width {
                    gs.explored.insert((x, y));
                }
            }
            "Entire map revealed.".to_string()
        }
        DevCommand::ToggleFov => {
            session.fov_disabled = !session.fov_disabled;
            if session.fov_disabled {
                apply_fov_override(gs);
                "FOV disabled (all tiles visible).".to_string()
            } else {
                gs.update_fov();
                "FOV re-enabled.".to_string()
            }
        }
        DevCommand::KillAll => {
            let mut killed = 0;
            for entity in gs.entities.iter_mut().skip(1) {
                if entity.alive {
                    entity.alive = false;
                    entity.hp = 0;
                    killed += 1;
                }
            }
            format!("Killed {} monsters.", killed)
        }
        DevCommand::DumpStats => dump_stats(gs, session),
        DevCommand::ToggleGodMode => {
            session.god_mode = !session.god_mode;
            if session.god_mode {
                "God mode enabled (invulnerable).".to_string()
            } else {
                "God mode disabled.".to_string()
            }
        }
    }
}

/// Make all tiles visible — called after FOV toggle or after step() when
/// FOV is disabled.
fn apply_fov_override(gs: &mut GameState) {
    for y in 0..gs.map.height {
        for x in 0..gs.map.width {
            gs.visible.insert((x, y));
            gs.explored.insert((x, y));
        }
    }
}

/// Format a debug summary of the game state.
fn dump_stats(gs: &GameState, session: &DevSession) -> String {
    let living = gs.entities.iter().skip(1).filter(|e| e.alive).count();
    let dead = gs.entities.iter().skip(1).filter(|e| !e.alive).count();
    let floor_count = gs.map.floor_count();
    let explored_floors = gs
        .explored
        .iter()
        .filter(|&&(x, y)| gs.map.in_bounds(x, y) && gs.map.tiles[gs.map.idx(x, y)] == Tile::Floor)
        .count();
    let p = &gs.entities[0];
    format!(
        "Turn {} | Seed {} | HP {}/{} | ATK {} DEF {} | \
         Monsters: {} alive, {} dead | Rooms: {} | \
         Explored: {}/{} floor tiles ({}%) | Flags: fov_off={} god={}",
        gs.turn_count,
        gs.seed,
        p.hp,
        p.max_hp,
        p.attack,
        p.defense,
        living,
        dead,
        gs.map.rooms.len(),
        explored_floors,
        floor_count,
        if floor_count > 0 {
            (explored_floors as i32 * 100) / floor_count
        } else {
            0
        },
        session.fov_disabled,
        session.god_mode,
    )
}

/// Apply dev-session overrides after a normal `gs.step()` call.
///
/// Call this in the game loop after each `step()` to enforce god mode and
/// FOV disable without polluting GameState's core logic.
pub fn after_step(gs: &mut GameState, session: &mut DevSession, cmd: GameCommand) {
    // Record command if recording is active.
    if session.recording {
        session.command_log.push(cmd);
    }
    // God mode: undo death.
    if session.god_mode && gs.game_over {
        gs.entities[0].hp = 1;
        gs.entities[0].alive = true;
        gs.game_over = false;
    }
    // FOV override: make all tiles visible.
    if session.fov_disabled {
        apply_fov_override(gs);
    }
}

/// Replay a sequence of commands on a game state. Returns a summary.
///
/// This is the core replay engine — it drives `gs.step()` for each command
/// and collects statistics.
pub fn replay_commands(gs: &mut GameState, commands: &[GameCommand]) -> ReplayResult {
    let mut turns_played = 0;
    for &cmd in commands {
        if gs.game_over {
            break;
        }
        let result = gs.step(cmd);
        if result.action_taken {
            turns_played += 1;
        }
    }
    ReplayResult {
        turns_played,
        game_over: gs.game_over,
        final_hp: gs.entities[0].hp,
        final_turn: gs.turn_count,
        kills: gs.entities.iter().skip(1).filter(|e| !e.alive).count() as Stat,
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
    pub commands: Vec<GameCommand>,
    /// Optional map preset used (None = standard generation).
    pub preset: Option<crate::map::MapPreset>,
}

impl Replay {
    /// Build a replay from the current game + recorded commands.
    pub fn from_session(gs: &GameState, session: &DevSession) -> Self {
        Replay {
            seed: gs.seed,
            width: gs.map.width,
            height: gs.map.height,
            commands: session.command_log.clone(),
            preset: None,
        }
    }

    /// Execute this replay and return the result.
    pub fn execute(&self) -> ReplayResult {
        let mut gs = match self.preset {
            Some(preset) => GameState::with_preset(self.width, self.height, self.seed, preset),
            None => GameState::with_seed(self.width, self.height, self.seed),
        };
        replay_commands(&mut gs, &self.commands)
    }
}

/// Summary of a replay execution or headless run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayResult {
    pub turns_played: Stat,
    pub game_over: bool,
    pub final_hp: Stat,
    pub final_turn: Stat,
    pub kills: Stat,
}

/// Result of a headless batch run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRunStats {
    pub games_played: Stat,
    pub games_won: Stat,
    pub games_lost: Stat,
    pub total_turns: Stat,
    pub total_kills: Stat,
    pub avg_turns_per_game: f64,
    pub avg_kills_per_game: f64,
    pub seeds_used: Vec<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fov;
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
        }
    }

    #[test]
    fn teleport_to_walkable() {
        let mut gs = test_game();
        let mut session = DevSession::default();
        let msg = exec_dev(&mut gs, &mut session, DevCommand::Teleport { x: 8, y: 8 });
        assert!(msg.contains("Teleported"));
        assert_eq!(gs.entities[0].x, 8);
        assert_eq!(gs.entities[0].y, 8);
    }

    #[test]
    fn teleport_to_wall_fails() {
        let mut gs = test_game();
        let mut session = DevSession::default();
        let msg = exec_dev(&mut gs, &mut session, DevCommand::Teleport { x: 0, y: 0 });
        assert!(msg.contains("not walkable"));
        assert_eq!(gs.entities[0].x, 5);
    }

    #[test]
    fn set_hp_clamps() {
        let mut gs = test_game();
        let mut session = DevSession::default();
        exec_dev(&mut gs, &mut session, DevCommand::SetHp { hp: 999 });
        assert_eq!(gs.entities[0].hp, gs.entities[0].max_hp);
        exec_dev(&mut gs, &mut session, DevCommand::SetHp { hp: -5 });
        assert_eq!(gs.entities[0].hp, 1);
    }

    #[test]
    fn set_attack() {
        let mut gs = test_game();
        let mut session = DevSession::default();
        exec_dev(&mut gs, &mut session, DevCommand::SetAttack { attack: 99 });
        assert_eq!(gs.entities[0].attack, 99);
    }

    #[test]
    fn set_defense() {
        let mut gs = test_game();
        let mut session = DevSession::default();
        exec_dev(
            &mut gs,
            &mut session,
            DevCommand::SetDefense { defense: 50 },
        );
        assert_eq!(gs.entities[0].defense, 50);
    }

    #[test]
    fn spawn_known_monster() {
        let mut gs = test_game();
        let mut session = DevSession::default();
        let msg = exec_dev(
            &mut gs,
            &mut session,
            DevCommand::Spawn {
                name: "goblin".to_string(),
                x: 3,
                y: 3,
            },
        );
        assert!(msg.contains("Spawned Goblin"));
        assert_eq!(gs.entities.len(), 2);
        assert_eq!(gs.entities[1].name, "Goblin");
    }

    #[test]
    fn spawn_unknown_monster_fails() {
        let mut gs = test_game();
        let mut session = DevSession::default();
        let msg = exec_dev(
            &mut gs,
            &mut session,
            DevCommand::Spawn {
                name: "dragon".to_string(),
                x: 3,
                y: 3,
            },
        );
        assert!(msg.contains("Unknown monster"));
        assert_eq!(gs.entities.len(), 1);
    }

    #[test]
    fn spawn_on_wall_fails() {
        let mut gs = test_game();
        let mut session = DevSession::default();
        let msg = exec_dev(
            &mut gs,
            &mut session,
            DevCommand::Spawn {
                name: "orc".to_string(),
                x: 0,
                y: 0,
            },
        );
        assert!(msg.contains("not walkable"));
    }

    #[test]
    fn reveal_map_explores_everything() {
        let mut gs = test_game();
        let mut session = DevSession::default();
        let before = gs.explored.len();
        exec_dev(&mut gs, &mut session, DevCommand::RevealMap);
        assert!(gs.explored.len() > before);
        for y in 0..gs.map.height {
            for x in 0..gs.map.width {
                assert!(gs.explored.contains(&(x, y)));
            }
        }
    }

    #[test]
    fn toggle_fov_disables_and_enables() {
        let mut gs = test_game();
        let mut session = DevSession::default();
        assert!(!session.fov_disabled);
        exec_dev(&mut gs, &mut session, DevCommand::ToggleFov);
        assert!(session.fov_disabled);
        assert!(gs.visible.contains(&(0, 0)));
        exec_dev(&mut gs, &mut session, DevCommand::ToggleFov);
        assert!(!session.fov_disabled);
    }

    #[test]
    fn kill_all_monsters() {
        let mut gs = test_game();
        let mut session = DevSession::default();
        gs.entities.push(Entity::from_template(&data::GOBLIN, 3, 3));
        gs.entities.push(Entity::from_template(&data::ORC, 4, 4));
        let msg = exec_dev(&mut gs, &mut session, DevCommand::KillAll);
        assert!(msg.contains("Killed 2"));
        assert!(gs.entities.iter().skip(1).all(|e| !e.alive));
    }

    #[test]
    fn dump_stats_includes_key_info() {
        let gs = test_game();
        let session = DevSession::default();
        let msg = dump_stats(&gs, &session);
        assert!(msg.contains("Turn 0"));
        assert!(msg.contains("Seed 0"));
        assert!(msg.contains("HP"));
        assert!(msg.contains("ATK"));
    }

    #[test]
    fn toggle_god_mode() {
        let mut gs = test_game();
        let mut session = DevSession::default();
        assert!(!session.god_mode);
        exec_dev(&mut gs, &mut session, DevCommand::ToggleGodMode);
        assert!(session.god_mode);
        exec_dev(&mut gs, &mut session, DevCommand::ToggleGodMode);
        assert!(!session.god_mode);
    }

    #[test]
    fn god_mode_prevents_death_via_after_step() {
        let mut gs = test_game();
        let mut session = DevSession::default();
        session.god_mode = true;
        gs.entities[0].hp = 1;
        gs.entities[0].attack = 0;
        let mut monster = Entity::from_template(&data::TROLL, 6, 5);
        monster.attack = 100;
        gs.entities.push(monster);
        gs.step(GameCommand::Wait);
        after_step(&mut gs, &mut session, GameCommand::Wait);
        // God mode should have undone the death.
        assert!(!gs.game_over);
        assert!(gs.entities[0].alive);
        assert_eq!(gs.entities[0].hp, 1);
    }

    // --- Replay tests ---

    #[test]
    fn recording_captures_commands() {
        let mut gs = test_game();
        let mut session = DevSession {
            recording: true,
            ..DevSession::default()
        };
        gs.step(GameCommand::Move { dx: 1, dy: 0 });
        after_step(&mut gs, &mut session, GameCommand::Move { dx: 1, dy: 0 });
        gs.step(GameCommand::Wait);
        after_step(&mut gs, &mut session, GameCommand::Wait);
        assert_eq!(session.command_log.len(), 2);
    }

    #[test]
    fn export_replay_roundtrip() {
        let mut gs = test_game();
        let mut session = DevSession {
            recording: true,
            ..DevSession::default()
        };
        gs.step(GameCommand::Move { dx: 1, dy: 0 });
        after_step(&mut gs, &mut session, GameCommand::Move { dx: 1, dy: 0 });
        let replay = Replay::from_session(&gs, &session);
        let json = serde_json::to_string_pretty(&replay).unwrap();
        let loaded: Replay = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.seed, 0);
        assert!(!loaded.commands.is_empty());
    }

    #[test]
    fn replay_deterministic_same_outcome() {
        let mut gs = GameState::with_seed(40, 30, 42);
        let mut session = DevSession {
            recording: true,
            ..DevSession::default()
        };
        for _ in 0..5 {
            gs.step(GameCommand::Wait);
            after_step(&mut gs, &mut session, GameCommand::Wait);
        }
        let replay = Replay::from_session(&gs, &session);
        let result = replay.execute();
        assert_eq!(result.final_turn, gs.turn_count);
    }

    #[test]
    fn replay_with_preset() {
        let replay = Replay {
            seed: 42,
            width: 40,
            height: 30,
            commands: vec![GameCommand::Wait, GameCommand::Wait],
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
        let mut monster = Entity::from_template(&data::TROLL, 6, 5);
        monster.attack = 100;
        gs.entities.push(monster);
        let commands: Vec<_> = (0..100).map(|_| GameCommand::Wait).collect();
        let result = replay_commands(&mut gs, &commands);
        assert!(result.game_over);
        assert!(result.turns_played < 100);
    }

    #[test]
    fn with_preset_creates_valid_game() {
        let gs = GameState::with_preset(40, 30, 42, crate::map::MapPreset::Arena);
        assert!(gs.entities[0].alive);
        assert!(gs.map.is_walkable(gs.entities[0].x, gs.entities[0].y));
        assert!(gs.log.recent(1)[0].contains("Arena"));
    }
}
