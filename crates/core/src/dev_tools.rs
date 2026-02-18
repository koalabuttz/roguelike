use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::command::GameCommand;
use crate::data;
use crate::entity::Entity;
use crate::game::GameState;
use crate::map::Tile;
use crate::pathfinding;
use crate::types::{Coord, GameColor, Pos, Stat};

/// Overlay layers for the debug visualization system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OverlayLayer {
    /// FOV boundary — tiles at the edge of the visible set.
    Fov,
    /// Monster AI targets — greedy chase candidates for each visible monster.
    MonsterTargets,
    /// A* pathfinding — path to nearest frontier or to a movable cursor.
    Pathfinding,
    /// Exploration frontiers — tiles at the boundary of explored territory.
    Frontiers,
}

/// A single overlay cell to draw on top of the normal map.
#[derive(Debug, Clone)]
pub struct OverlayCell {
    pub x: Coord,
    pub y: Coord,
    pub ch: char,
    pub color: GameColor,
}

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
    /// Toggle a debug overlay layer.
    ToggleOverlay(OverlayLayer),
    /// Reload game data from CWD `game.toml`.
    ReloadData,
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
    /// Bitfield of active overlay layers (bit 0=Fov, 1=MonsterTargets,
    /// 2=Pathfinding, 3=Frontiers).
    pub overlay_flags: u8,
    /// Cursor position for pathfinding overlay cursor mode.
    /// `None` = frontier mode, `Some(pos)` = cursor mode.
    pub overlay_cursor: Option<Pos>,
    /// Custom game data loaded from disk (used by Spawn and hot reload).
    pub game_data: Option<data::GameData>,
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
            let game_data: &data::GameData = match &session.game_data {
                Some(d) => d,
                None => data::defaults(),
            };
            let template = match game_data.monster_by_name(&name) {
                Some(t) => t,
                None => {
                    let known: Vec<&str> =
                        game_data.monsters.iter().map(|m| m.name.as_str()).collect();
                    return format!("Unknown monster: '{}'. Known: {}.", name, known.join(", "));
                }
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
        DevCommand::ToggleOverlay(layer) => {
            let bit = match layer {
                OverlayLayer::Fov => 0,
                OverlayLayer::MonsterTargets => 1,
                OverlayLayer::Pathfinding => 2,
                OverlayLayer::Frontiers => 3,
            };

            if layer == OverlayLayer::Pathfinding {
                let is_on = session.overlay_flags & (1 << bit) != 0;
                if !is_on {
                    // OFF -> frontier mode.
                    session.overlay_flags |= 1 << bit;
                    session.overlay_cursor = None;
                    "Pathfinding overlay: frontier mode.".to_string()
                } else if session.overlay_cursor.is_none() {
                    // Frontier mode -> cursor mode.
                    session.overlay_cursor = Some((gs.entities[0].x, gs.entities[0].y));
                    "Pathfinding overlay: cursor mode (arrows to move, Esc to exit).".to_string()
                } else {
                    // Cursor mode -> OFF.
                    session.overlay_flags &= !(1 << bit);
                    session.overlay_cursor = None;
                    "Pathfinding overlay off.".to_string()
                }
            } else {
                session.overlay_flags ^= 1 << bit;
                let name = match layer {
                    OverlayLayer::Fov => "FOV boundary",
                    OverlayLayer::MonsterTargets => "Monster targets",
                    OverlayLayer::Pathfinding => unreachable!(),
                    OverlayLayer::Frontiers => "Frontiers",
                };
                if session.overlay_flags & (1 << bit) != 0 {
                    format!("{} overlay on.", name)
                } else {
                    format!("{} overlay off.", name)
                }
            }
        }
        DevCommand::ReloadData => {
            let old_data = session
                .game_data
                .clone()
                .unwrap_or_else(|| data::defaults().clone());
            let new_data = data::load_game_data();

            // Apply config changes to game state.
            // Note: only global config values are patched here. Monster stat
            // changes (HP, attack, etc.) apply to newly spawned entities only —
            // existing live monsters keep their original stats.
            gs.regen_interval = new_data.config.regen_interval;
            gs.max_autorun_steps = new_data.config.max_autorun_steps;
            let fov_changed = gs.fov_radius != new_data.config.fov_radius;
            gs.fov_radius = new_data.config.fov_radius;
            if fov_changed {
                gs.update_fov();
                if session.fov_disabled {
                    apply_fov_override(gs);
                }
            }

            // Generate diff report.
            let diffs = data::diff_game_data(&old_data, &new_data);
            session.game_data = Some(new_data);

            if diffs.is_empty() {
                "Data reloaded (no changes detected).".to_string()
            } else {
                format!("Data reloaded: {}", diffs.join("; "))
            }
        }
    }
}

/// Make all tiles visible — called after FOV toggle or after step() when
/// FOV is disabled.
pub fn apply_fov_override(gs: &mut GameState) {
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
            (explored_floors as Stat * 100) / floor_count
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

/// Compute overlay cells for all active overlay layers.
///
/// Returns an empty vec when no overlays are enabled. The caller (renderer)
/// draws each cell at its position with its color on top of the normal map.
pub fn compute_overlay(gs: &GameState, session: &DevSession) -> Vec<OverlayCell> {
    let mut cells = Vec::new();

    if session.overlay_flags == 0 {
        return cells;
    }

    // Layer 0: FOV boundary — tiles at the edge of the visible set.
    if session.overlay_flags & (1 << 0) != 0 {
        for &(x, y) in &gs.visible {
            let at_boundary = (-1..=1i32).any(|dy| {
                (-1..=1i32)
                    .any(|dx| (dx != 0 || dy != 0) && !gs.visible.contains(&(x + dx, y + dy)))
            });
            if at_boundary {
                cells.push(OverlayCell {
                    x,
                    y,
                    ch: '*',
                    color: GameColor::Cyan,
                });
            }
        }
    }

    // Layer 1: Monster targets — greedy chase candidates for each visible monster.
    if session.overlay_flags & (1 << 1) != 0 {
        let px = gs.entities[0].x;
        let py = gs.entities[0].y;
        for entity in gs.entities.iter().skip(1) {
            if !entity.alive || !gs.visible.contains(&(entity.x, entity.y)) {
                continue;
            }
            let mx = entity.x;
            let my = entity.y;
            let step_x = (px - mx).signum();
            let step_y = (py - my).signum();
            // Same candidate logic as chase_ai in ai.rs.
            let candidates = [
                (mx + step_x, my + step_y),
                (mx + step_x, my),
                (mx, my + step_y),
            ];
            for (cx, cy) in candidates {
                if gs.map.is_walkable(cx, cy) {
                    cells.push(OverlayCell {
                        x: cx,
                        y: cy,
                        ch: '.',
                        color: GameColor::Rgb(255, 0, 255), // magenta
                    });
                }
            }
        }
    }

    // Layer 2: Pathfinding — A* path to cursor or nearest frontier.
    if session.overlay_flags & (1 << 2) != 0 {
        let px = gs.entities[0].x;
        let py = gs.entities[0].y;

        let path = if let Some((tx, ty)) = session.overlay_cursor {
            // Cursor mode: path to cursor position.
            pathfinding::find_path(&gs.map, px, py, tx, ty, &gs.explored)
        } else {
            // Frontier mode: path to nearest frontier.
            let frontiers = gs.frontier_tiles();
            if !frontiers.is_empty() {
                let frontier_set: HashSet<Pos> = frontiers.into_iter().collect();
                if let Some((tx, ty)) =
                    pathfinding::nearest_by_cost(&gs.map, px, py, &frontier_set, &gs.explored)
                {
                    pathfinding::find_path(&gs.map, px, py, tx, ty, &gs.explored)
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(path) = path {
            for (px, py) in path {
                cells.push(OverlayCell {
                    x: px,
                    y: py,
                    ch: '+',
                    color: GameColor::Rgb(80, 130, 255), // bright blue
                });
            }
        }

        // Draw cursor marker if in cursor mode.
        if let Some((cx, cy)) = session.overlay_cursor {
            cells.push(OverlayCell {
                x: cx,
                y: cy,
                ch: 'X',
                color: GameColor::Rgb(255, 255, 0), // bright yellow
            });
        }
    }

    // Layer 3: Frontiers — exploration boundary tiles.
    if session.overlay_flags & (1 << 3) != 0 {
        for (fx, fy) in gs.frontier_tiles() {
            cells.push(OverlayCell {
                x: fx,
                y: fy,
                ch: '~',
                color: GameColor::Yellow,
            });
        }
    }

    cells
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
    pub fn from_session(
        gs: &GameState,
        session: &DevSession,
        preset: Option<crate::map::MapPreset>,
    ) -> Self {
        Replay {
            seed: gs.seed,
            width: gs.map.width,
            height: gs.map.height,
            commands: session.command_log.clone(),
            preset,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayResult {
    pub turns_played: Stat,
    pub game_over: bool,
    pub final_hp: Stat,
    pub final_turn: Stat,
    pub kills: Stat,
}

/// A golden replay: a recorded game with its expected outcome.
///
/// Used for regression testing — re-execute the replay after code changes
/// and verify the result hasn't diverged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoldenReplay {
    pub name: String,
    pub description: String,
    pub replay: Replay,
    pub expected: ReplayResult,
}

impl GoldenReplay {
    /// Execute the replay and compare against the expected result.
    ///
    /// Returns `Ok(())` if the result matches, or `Err(message)` describing
    /// the divergence.
    pub fn verify(&self) -> Result<(), String> {
        let actual = self.replay.execute();
        if actual == self.expected {
            Ok(())
        } else {
            Err(format!(
                "Golden replay '{}' diverged!\n  Expected: {:?}\n  Actual:   {:?}",
                self.name, self.expected, actual
            ))
        }
    }
}

/// Create a golden replay from a completed game session.
///
/// Captures the current replay and its result as a golden snapshot.
pub fn golden_from_session(
    name: &str,
    description: &str,
    gs: &GameState,
    session: &DevSession,
    preset: Option<crate::map::MapPreset>,
) -> GoldenReplay {
    let replay = Replay::from_session(gs, session, preset);
    let expected = replay.execute();
    GoldenReplay {
        name: name.to_string(),
        description: description.to_string(),
        replay,
        expected,
    }
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
            preset: None,
            dirty: false,
            regen_interval: data::config().regen_interval,
            max_autorun_steps: data::config().max_autorun_steps,
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
        gs.entities
            .push(Entity::from_template(data::goblin(), 3, 3));
        gs.entities.push(Entity::from_template(data::orc(), 4, 4));
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
        let mut session = DevSession {
            god_mode: true,
            ..Default::default()
        };
        gs.entities[0].hp = 1;
        gs.entities[0].attack = 0;
        let mut monster = Entity::from_template(data::troll(), 6, 5);
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
        let replay = Replay::from_session(&gs, &session, None);
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
        let replay = Replay::from_session(&gs, &session, None);
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
        let mut monster = Entity::from_template(data::troll(), 6, 5);
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

    // --- Golden replay tests ---

    #[test]
    fn replay_result_partial_eq() {
        let a = ReplayResult {
            turns_played: 10,
            game_over: false,
            final_hp: 25,
            final_turn: 10,
            kills: 3,
        };
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn golden_replay_verify_passes() {
        let mut gs = test_game();
        let mut session = DevSession {
            recording: true,
            ..DevSession::default()
        };
        for _ in 0..3 {
            gs.step(GameCommand::Move { dx: 1, dy: 0 });
            after_step(&mut gs, &mut session, GameCommand::Move { dx: 1, dy: 0 });
        }
        let golden = golden_from_session("test", "test golden", &gs, &session, None);
        assert!(golden.verify().is_ok());
    }

    #[test]
    fn golden_replay_verify_detects_mismatch() {
        let golden = GoldenReplay {
            name: "bad".to_string(),
            description: "intentionally wrong".to_string(),
            replay: Replay {
                seed: 42,
                width: 40,
                height: 30,
                commands: vec![GameCommand::Wait, GameCommand::Wait],
                preset: None,
            },
            expected: ReplayResult {
                turns_played: 999,
                game_over: true,
                final_hp: -1,
                final_turn: 999,
                kills: 100,
            },
        };
        assert!(golden.verify().is_err());
    }

    #[test]
    fn golden_from_session_roundtrip() {
        let mut gs = GameState::with_seed(40, 30, 42);
        let mut session = DevSession {
            recording: true,
            ..DevSession::default()
        };
        for _ in 0..5 {
            gs.step(GameCommand::Wait);
            after_step(&mut gs, &mut session, GameCommand::Wait);
        }
        let golden =
            golden_from_session("seed_42", "Standard dungeon, 5 waits", &gs, &session, None);
        // Serialize and deserialize.
        let json = serde_json::to_string_pretty(&golden).unwrap();
        let loaded: GoldenReplay = serde_json::from_str(&json).unwrap();
        assert!(loaded.verify().is_ok());
        assert_eq!(loaded.name, "seed_42");
    }

    // --- Overlay tests ---

    #[test]
    fn toggle_overlay_sets_and_clears_flag() {
        let mut gs = test_game();
        let mut session = DevSession::default();
        assert_eq!(session.overlay_flags, 0);

        exec_dev(
            &mut gs,
            &mut session,
            DevCommand::ToggleOverlay(OverlayLayer::Fov),
        );
        assert_ne!(session.overlay_flags & 1, 0);

        exec_dev(
            &mut gs,
            &mut session,
            DevCommand::ToggleOverlay(OverlayLayer::Fov),
        );
        assert_eq!(session.overlay_flags & 1, 0);
    }

    #[test]
    fn compute_overlay_empty_when_disabled() {
        let gs = test_game();
        let session = DevSession::default();
        let cells = compute_overlay(&gs, &session);
        assert!(cells.is_empty());
    }

    #[test]
    fn compute_overlay_fov_boundary_only_at_edges() {
        let gs = test_game();
        let mut session = DevSession::default();
        session.overlay_flags = 1 << 0; // FOV layer
        let cells = compute_overlay(&gs, &session);
        assert!(!cells.is_empty());
        // Every cell must be visible and have at least one non-visible neighbor.
        for cell in &cells {
            assert_eq!(cell.ch, '*');
            assert!(gs.visible.contains(&(cell.x, cell.y)));
            let has_non_visible = (-1..=1i32).any(|dy| {
                (-1..=1i32).any(|dx| {
                    (dx != 0 || dy != 0) && !gs.visible.contains(&(cell.x + dx, cell.y + dy))
                })
            });
            assert!(
                has_non_visible,
                "({},{}) is not at FOV boundary",
                cell.x, cell.y
            );
        }
    }

    #[test]
    fn compute_overlay_monster_targets_near_monsters() {
        let mut gs = test_game();
        let monster = Entity::from_template(data::goblin(), 3, 3);
        gs.entities.push(monster);
        gs.update_fov();
        let mut session = DevSession::default();
        session.overlay_flags = 1 << 1; // Monster targets layer
        let cells = compute_overlay(&gs, &session);
        // Should have target cells near the monster.
        assert!(!cells.is_empty());
        for cell in &cells {
            assert_eq!(cell.ch, '.');
            // Target cells should be near the monster (within 1 tile).
            let near = (cell.x - 3).abs() <= 1 && (cell.y - 3).abs() <= 1;
            assert!(
                near,
                "target ({},{}) not near monster (3,3)",
                cell.x, cell.y
            );
        }
    }

    #[test]
    fn compute_overlay_frontiers_matches_frontier_tiles() {
        let gs = test_game();
        let frontiers = gs.frontier_tiles();
        let mut session = DevSession::default();
        session.overlay_flags = 1 << 3; // Frontiers layer
        let cells = compute_overlay(&gs, &session);
        assert_eq!(cells.len(), frontiers.len());
        for cell in &cells {
            assert_eq!(cell.ch, '~');
            assert!(frontiers.contains(&(cell.x, cell.y)));
        }
    }

    #[test]
    fn compute_overlay_pathfinding_with_frontier() {
        let mut gs = test_game();
        gs.update_fov();
        let mut session = DevSession::default();
        session.overlay_flags = 1 << 2; // Pathfinding layer
        // No cursor → frontier mode.
        let frontiers = gs.frontier_tiles();
        if !frontiers.is_empty() {
            let cells = compute_overlay(&gs, &session);
            // Should have path cells ('+') leading toward a frontier.
            assert!(cells.iter().any(|c| c.ch == '+'));
        }
    }

    #[test]
    fn compute_overlay_pathfinding_with_cursor() {
        let mut gs = test_game();
        gs.update_fov();
        let mut session = DevSession::default();
        session.overlay_flags = 1 << 2; // Pathfinding layer
        session.overlay_cursor = Some((8, 5)); // Cursor at (8, 5)
        let cells = compute_overlay(&gs, &session);
        // Should have path cells and a cursor marker 'X'.
        assert!(cells.iter().any(|c| c.ch == '+'));
        assert!(cells.iter().any(|c| c.ch == 'X' && c.x == 8 && c.y == 5));
    }

    // --- Reload data tests ---

    #[test]
    fn reload_data_updates_config_fields() {
        let mut gs = test_game();
        let mut session = DevSession::default();
        // Pre-load custom data with different config values.
        let mut custom = data::defaults().clone();
        custom.config.regen_interval = 10;
        custom.config.max_autorun_steps = 50;
        custom.config.fov_radius = 5;
        session.game_data = Some(custom);
        // ReloadData reads from disk (no file → defaults), but we verify that
        // exec_dev applies the new data's config fields to gs.
        let msg = exec_dev(&mut gs, &mut session, DevCommand::ReloadData);
        // After reload (no CWD file → defaults), gs should have default values.
        assert_eq!(gs.regen_interval, data::config().regen_interval);
        assert_eq!(gs.max_autorun_steps, data::config().max_autorun_steps);
        assert_eq!(gs.fov_radius, data::config().fov_radius);
        assert!(msg.contains("reloaded"));
    }

    #[test]
    fn reload_data_fov_updates_on_radius_change() {
        let mut gs = test_game();
        let mut session = DevSession::default();
        // Set a different fov_radius so reload triggers update_fov.
        gs.fov_radius = 3;
        gs.update_fov();
        let visible_before = gs.visible.len();
        let msg = exec_dev(&mut gs, &mut session, DevCommand::ReloadData);
        // After reload, fov_radius should be restored to default (8).
        assert_eq!(gs.fov_radius, data::config().fov_radius);
        // With a larger FOV radius, more tiles should be visible.
        assert!(gs.visible.len() >= visible_before);
        assert!(msg.contains("reloaded"));
    }

    #[test]
    fn reload_data_spawn_uses_reloaded_data() {
        let mut gs = test_game();
        let mut session = DevSession::default();
        // Load custom data with a different Goblin HP.
        let mut custom = data::defaults().clone();
        if let Some(goblin) = custom.monsters.iter_mut().find(|m| m.name == "Goblin") {
            goblin.hp = 99;
        }
        session.game_data = Some(custom);
        // Spawn should use session data.
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
        assert_eq!(gs.entities.last().unwrap().hp, 99);
    }
}
