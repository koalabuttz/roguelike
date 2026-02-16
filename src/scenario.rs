//! Fluent scenario builder for composing specific game states and asserting
//! outcomes.
//!
//! Uses `DevCommand`s from `dev_tools.rs` to set up game state, then runs
//! turns and checks results. This keeps scenario setup using the same
//! primitives as interactive dev tools.
//!
//! # Example
//!
//! ```rust,ignore
//! Scenario::new(20, 20, 42)
//!     .preset(MapPreset::SingleRoom)
//!     .kill_all()
//!     .spawn("troll", 4, 5)
//!     .set_player_hp(10)
//!     .run_turns(50)
//!     .assert_dead();
//! ```

use crate::analytics::{self, GameAnalytics};
use crate::dev_tools::{DevCommand, DevSession, after_step, exec_dev};
use crate::game::GameState;
use crate::input::GameCommand;
use crate::map::MapPreset;
use crate::types::{Coord, Stat};

/// Builder for constructing a test scenario.
///
/// Accumulates dev commands and mutations to apply at build time.
pub struct Scenario {
    width: Coord,
    height: Coord,
    seed: u64,
    preset: Option<MapPreset>,
    commands: Vec<DevCommand>,
    mutations: Vec<Box<dyn FnOnce(&mut GameState, &mut DevSession)>>,
}

impl Scenario {
    /// Create a new scenario builder with the given map dimensions and seed.
    pub fn new(width: Coord, height: Coord, seed: u64) -> Self {
        Scenario {
            width,
            height,
            seed,
            preset: None,
            commands: Vec::new(),
            mutations: Vec::new(),
        }
    }

    /// Use a specific map preset.
    pub fn preset(mut self, preset: MapPreset) -> Self {
        self.preset = Some(preset);
        self
    }

    /// Kill all monsters currently on the map.
    pub fn kill_all(mut self) -> Self {
        self.commands.push(DevCommand::KillAll);
        self
    }

    /// Spawn a monster by name at (x, y).
    pub fn spawn(mut self, name: &str, x: Coord, y: Coord) -> Self {
        self.commands.push(DevCommand::Spawn {
            name: name.to_string(),
            x,
            y,
        });
        self
    }

    /// Set the player's HP.
    pub fn set_player_hp(mut self, hp: Stat) -> Self {
        self.commands.push(DevCommand::SetHp { hp });
        self
    }

    /// Set the player's attack stat.
    pub fn set_player_attack(mut self, attack: Stat) -> Self {
        self.commands.push(DevCommand::SetAttack { attack });
        self
    }

    /// Set the player's defense stat.
    pub fn set_player_defense(mut self, defense: Stat) -> Self {
        self.commands.push(DevCommand::SetDefense { defense });
        self
    }

    /// Enable god mode (player takes no damage).
    pub fn god_mode(mut self) -> Self {
        self.commands.push(DevCommand::ToggleGodMode);
        self
    }

    /// Teleport the player to (x, y).
    pub fn teleport(mut self, x: Coord, y: Coord) -> Self {
        self.commands.push(DevCommand::Teleport { x, y });
        self
    }

    /// Reveal the entire map.
    pub fn reveal_map(mut self) -> Self {
        self.commands.push(DevCommand::RevealMap);
        self
    }

    /// Add a custom mutation applied after dev commands.
    pub fn mutate(mut self, f: impl FnOnce(&mut GameState, &mut DevSession) + 'static) -> Self {
        self.mutations.push(Box::new(f));
        self
    }

    /// Build the scenario into a prepared state ready for execution.
    pub fn build(self) -> PreparedScenario {
        let mut gs = match self.preset {
            Some(preset) => GameState::with_preset(self.width, self.height, self.seed, preset),
            None => GameState::with_seed(self.width, self.height, self.seed),
        };

        let mut session = DevSession {
            recording: true,
            ..DevSession::default()
        };

        // Apply dev commands.
        for cmd in self.commands {
            exec_dev(&mut gs, &mut session, cmd);
        }

        // Apply custom mutations.
        for mutation in self.mutations {
            mutation(&mut gs, &mut session);
        }

        PreparedScenario {
            gs,
            session,
            preset: self.preset,
        }
    }

    /// Build and run for N turns using auto-fight AI, returning the result.
    ///
    /// This is the most common usage — set up a scenario and see what happens
    /// after N turns of automated play.
    pub fn run_turns(self, max_turns: Stat) -> ScenarioResult {
        self.build().run_turns(max_turns)
    }

    /// Build and run auto-fight against adjacent monsters for up to N turns.
    pub fn run_auto_fight(self, max_turns: Stat) -> ScenarioResult {
        self.build().run_auto_fight(max_turns)
    }
}

/// A built scenario ready for step-by-step or batch execution.
pub struct PreparedScenario {
    pub gs: GameState,
    pub session: DevSession,
    pub preset: Option<MapPreset>,
}

impl PreparedScenario {
    /// Run for up to `max_turns` turns using the headless AI strategy:
    /// fight adjacent monsters, otherwise wait.
    pub fn run_turns(mut self, max_turns: Stat) -> ScenarioResult {
        let mut analytics = analytics::new_analytics(self.gs.seed);
        let mut turns_run = 0;

        while !self.gs.game_over && turns_run < max_turns {
            let before = analytics::snapshot_entities(&self.gs);

            let cmd = if self.gs.has_adjacent_monster() {
                fight_command(&self.gs)
            } else {
                GameCommand::Wait
            };

            let result = self.gs.step(cmd);
            after_step(&mut self.gs, &mut self.session, cmd);

            if result.action_taken {
                analytics::diff_combat(&before, &self.gs, self.gs.turn_count, &mut analytics);
                turns_run += 1;
            }
        }

        analytics::finalize_analytics(&mut analytics, &self.gs);

        ScenarioResult {
            gs: self.gs,
            turns_run,
            analytics: Some(analytics),
        }
    }

    /// Run auto-fight: repeatedly attack the weakest adjacent monster until
    /// all are dead, player dies, or max_turns reached.
    pub fn run_auto_fight(mut self, max_turns: Stat) -> ScenarioResult {
        let mut analytics = analytics::new_analytics(self.gs.seed);
        let mut turns_run = 0;

        while !self.gs.game_over && turns_run < max_turns && self.gs.has_adjacent_monster() {
            let before = analytics::snapshot_entities(&self.gs);
            let cmd = fight_command(&self.gs);
            let result = self.gs.step(cmd);
            after_step(&mut self.gs, &mut self.session, cmd);

            if result.action_taken {
                analytics::diff_combat(&before, &self.gs, self.gs.turn_count, &mut analytics);
                turns_run += 1;
            }
        }

        analytics::finalize_analytics(&mut analytics, &self.gs);

        ScenarioResult {
            gs: self.gs,
            turns_run,
            analytics: Some(analytics),
        }
    }
}

/// Pick the move command to attack the weakest adjacent monster.
fn fight_command(gs: &GameState) -> GameCommand {
    let px = gs.entities[0].x;
    let py = gs.entities[0].y;
    gs.entities
        .iter()
        .enumerate()
        .filter(|(i, e)| *i != 0 && e.alive && (e.x - px).abs() <= 1 && (e.y - py).abs() <= 1)
        .min_by_key(|(_, e)| e.hp)
        .map(|(_, e)| GameCommand::Move {
            dx: (e.x - px).signum(),
            dy: (e.y - py).signum(),
        })
        .unwrap_or(GameCommand::Wait)
}

/// The outcome of running a scenario.
pub struct ScenarioResult {
    pub gs: GameState,
    pub turns_run: Stat,
    pub analytics: Option<GameAnalytics>,
}

// --- Chainable assertions on ScenarioResult ---

impl ScenarioResult {
    /// Assert the player died.
    pub fn assert_dead(self) -> Self {
        assert!(
            self.gs.game_over,
            "Expected player to be dead, but they survived with {} HP after {} turns",
            self.gs.entities[0].hp,
            self.turns_run,
        );
        self
    }

    /// Assert the player is alive.
    pub fn assert_alive(self) -> Self {
        assert!(
            !self.gs.game_over,
            "Expected player to be alive, but they died after {} turns",
            self.turns_run,
        );
        self
    }

    /// Assert player HP equals exactly `expected`.
    pub fn assert_hp(self, expected: Stat) -> Self {
        assert_eq!(
            self.gs.entities[0].hp, expected,
            "Expected HP={}, got HP={}",
            expected, self.gs.entities[0].hp,
        );
        self
    }

    /// Assert player HP is in [min, max].
    pub fn assert_hp_between(self, min: Stat, max: Stat) -> Self {
        let hp = self.gs.entities[0].hp;
        assert!(
            hp >= min && hp <= max,
            "Expected HP in [{}, {}], got HP={}",
            min,
            max,
            hp,
        );
        self
    }

    /// Assert exactly `expected` monsters were killed.
    pub fn assert_kills(self, expected: Stat) -> Self {
        let kills = self
            .gs
            .entities
            .iter()
            .skip(1)
            .filter(|e| !e.alive)
            .count() as Stat;
        assert_eq!(
            kills, expected,
            "Expected {} kills, got {}",
            expected, kills,
        );
        self
    }

    /// Assert exactly `expected` monsters are still alive.
    pub fn assert_monsters_alive(self, expected: Stat) -> Self {
        let alive = self
            .gs
            .entities
            .iter()
            .skip(1)
            .filter(|e| e.alive)
            .count() as Stat;
        assert_eq!(
            alive, expected,
            "Expected {} monsters alive, got {}",
            expected, alive,
        );
        self
    }

    /// Assert the scenario ran for exactly `expected` turns.
    pub fn assert_turns(self, expected: Stat) -> Self {
        assert_eq!(
            self.turns_run, expected,
            "Expected {} turns, got {}",
            expected, self.turns_run,
        );
        self
    }

    /// Assert the scenario ran for fewer than `max` turns.
    pub fn assert_turns_less_than(self, max: Stat) -> Self {
        assert!(
            self.turns_run < max,
            "Expected fewer than {} turns, got {}",
            max, self.turns_run,
        );
        self
    }

    /// Get a reference to the final game state for custom assertions.
    pub fn game_state(&self) -> &GameState {
        &self.gs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn troll_kills_weak_player() {
        Scenario::new(20, 20, 42)
            .preset(MapPreset::SingleRoom)
            .kill_all()
            .set_player_hp(5)
            .set_player_defense(0)
            .spawn("troll", 4, 5)
            .run_turns(50)
            .assert_dead();
    }

    #[test]
    fn god_mode_survives_everything() {
        Scenario::new(20, 20, 42)
            .preset(MapPreset::Arena)
            .god_mode()
            .set_player_hp(1)
            .run_turns(100)
            .assert_alive();
    }

    #[test]
    fn strong_player_kills_goblin() {
        Scenario::new(20, 20, 42)
            .preset(MapPreset::SingleRoom)
            .kill_all()
            .set_player_attack(100)
            .spawn("goblin", 4, 5)
            .run_turns(50)
            .assert_alive()
            .assert_kills(1);
    }

    #[test]
    fn scenario_with_no_monsters_survives() {
        Scenario::new(20, 20, 42)
            .preset(MapPreset::SingleRoom)
            .kill_all()
            .run_turns(10)
            .assert_alive()
            .assert_kills(0);
    }

    #[test]
    fn auto_fight_resolves_combat() {
        let result = Scenario::new(20, 20, 42)
            .preset(MapPreset::SingleRoom)
            .kill_all()
            .spawn("goblin", 4, 5)
            .set_player_attack(100)
            .run_auto_fight(50);

        result.assert_alive().assert_kills(1);
    }

    #[test]
    fn scenario_has_analytics() {
        let result = Scenario::new(20, 20, 42)
            .preset(MapPreset::SingleRoom)
            .kill_all()
            .spawn("goblin", 4, 5)
            .set_player_attack(100)
            .run_turns(50);

        assert!(result.analytics.is_some());
        let analytics = result.analytics.as_ref().unwrap();
        assert_eq!(analytics.seed, 42);
    }

    #[test]
    fn assert_hp_between_passes() {
        Scenario::new(20, 20, 42)
            .preset(MapPreset::SingleRoom)
            .kill_all()
            .set_player_hp(15)
            .run_turns(1)
            .assert_hp_between(1, 30);
    }

    #[test]
    fn assert_monsters_alive() {
        Scenario::new(20, 20, 42)
            .preset(MapPreset::SingleRoom)
            .kill_all()
            .spawn("goblin", 3, 3)
            .spawn("orc", 7, 7)
            .run_turns(0)
            .assert_monsters_alive(2);
    }

    #[test]
    fn custom_mutation() {
        let result = Scenario::new(20, 20, 42)
            .preset(MapPreset::SingleRoom)
            .kill_all()
            .mutate(|gs, _session| {
                gs.entities[0].hp = 7;
            })
            .run_turns(0);

        result.assert_hp(7);
    }

    #[test]
    fn assert_turns_less_than() {
        // With no monsters, waiting 5 turns should complete in exactly 5.
        Scenario::new(20, 20, 42)
            .preset(MapPreset::SingleRoom)
            .kill_all()
            .run_turns(5)
            .assert_turns(5)
            .assert_turns_less_than(10);
    }
}
