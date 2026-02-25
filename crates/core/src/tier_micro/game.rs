//! Top-level micro-tier game state and step API.
//!
//! `MicroGameState` owns all game data — map, entities, FOV, messages, RNG.
//! The `step()` method processes one player command and runs a full game tick.

use super::ai;
use super::combat;
use super::entity::EntityStore;
use super::fov::MicroFov;
use super::map::MicroMap;
use super::msglog::MicroMessageLog;
use super::prng::LfsrRng16;
use super::spawn;
use super::types::*;
use crate::rules::balance;
use crate::rules::direction::Direction;
use crate::rules::message::GameEvent;

/// Commands the micro tier understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicroCommand {
    Move(Direction),
    Wait,
}

/// Result of a single step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MicroStepResult {
    pub action_taken: bool,
    pub game_over: bool,
}

pub struct MicroGameState {
    pub map: MicroMap,
    pub fov: MicroFov,
    pub entities: EntityStore,
    pub log: MicroMessageLog,
    pub rng: LfsrRng16,
    pub turn_count: u16,
    pub kills: u8,
    pub game_over: bool,
}

impl MicroGameState {
    /// Create a new game with the given seed.
    pub fn new(seed: u16) -> Self {
        let mut rng = LfsrRng16::new(seed);
        let mut map = MicroMap::new();
        let (sx, sy) = map.generate(&mut rng);

        let mut entities = EntityStore::new();
        entities.spawn_player(sx, sy);
        spawn::spawn_monsters(&mut entities, &map, &mut rng);

        let mut fov = MicroFov::new();
        fov.compute_fov(sx, sy, &map);

        let mut log = MicroMessageLog::new();
        log.add(GameEvent::Welcome);

        Self {
            map,
            fov,
            entities,
            log,
            rng,
            turn_count: 0,
            kills: 0,
            game_over: false,
        }
    }

    /// Execute one player command + monster turns + regen.
    pub fn step(&mut self, cmd: MicroCommand) -> MicroStepResult {
        if self.game_over {
            return MicroStepResult {
                action_taken: false,
                game_over: true,
            };
        }

        let action_taken = match cmd {
            MicroCommand::Wait => true,
            MicroCommand::Move(dir) => {
                let (dx, dy) = dir.to_offset();
                self.player_move_or_attack(dx as i8, dy as i8)
            }
        };

        if action_taken {
            let px = self.entities.x[PLAYER_IDX as usize];
            let py = self.entities.y[PLAYER_IDX as usize];
            self.fov.compute_fov(px, py, &self.map);

            let player_died =
                ai::run_monster_turns(&mut self.entities, &self.map, &mut self.rng, &mut self.log);
            if player_died {
                self.game_over = true;
                self.log.add(GameEvent::PlayerDeath);
            }

            self.turn_count += 1;
            self.apply_regen();
        }

        MicroStepResult {
            action_taken,
            game_over: self.game_over,
        }
    }

    fn player_move_or_attack(&mut self, dx: i8, dy: i8) -> bool {
        let px = self.entities.x[PLAYER_IDX as usize];
        let py = self.entities.y[PLAYER_IDX as usize];
        let nx = (px as i8 + dx) as u8;
        let ny = (py as i8 + dy) as u8;

        // Check for monster at target position
        let target = self.entities.monster_at(nx, ny);
        if target != NO_ENTITY {
            let killed =
                combat::melee_attack(PLAYER_IDX, target, &mut self.entities, &mut self.log);
            if killed {
                self.kills += 1;
            }
            return true;
        }

        // Try to move
        if self.map.is_walkable(nx, ny) {
            self.entities.x[PLAYER_IDX as usize] = nx;
            self.entities.y[PLAYER_IDX as usize] = ny;
            return true;
        }

        false
    }

    fn apply_regen(&mut self) {
        if self.game_over {
            return;
        }
        let pi = PLAYER_IDX as usize;
        let hp = self.entities.hp[pi];
        let max_hp = self.entities.max_hp[pi];
        if hp < max_hp
            && self
                .turn_count
                .is_multiple_of(balance::REGEN_INTERVAL as u16)
        {
            self.entities.hp[pi] = hp + 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_game_is_playable() {
        let g = MicroGameState::new(42);
        assert!(!g.game_over);
        assert!(g.entities.alive[PLAYER_IDX as usize]);
        assert!(g.entities.hp[PLAYER_IDX as usize] > 0);
        assert!(g.entities.count > 1, "should have monsters");
    }

    #[test]
    fn move_changes_position() {
        let mut g = MicroGameState::new(42);
        let px = g.entities.x[0];
        let py = g.entities.y[0];

        // Try all 8 directions until one succeeds
        let dirs = [
            Direction::North,
            Direction::South,
            Direction::East,
            Direction::West,
            Direction::NorthEast,
            Direction::NorthWest,
            Direction::SouthEast,
            Direction::SouthWest,
        ];
        let mut moved = false;
        for dir in dirs {
            let (dx, dy) = dir.to_offset();
            let nx = (px as i8 + dx as i8) as u8;
            let ny = (py as i8 + dy as i8) as u8;
            if g.map.is_walkable(nx, ny) && g.entities.monster_at(nx, ny) == NO_ENTITY {
                let result = g.step(MicroCommand::Move(dir));
                assert!(result.action_taken);
                assert_ne!((g.entities.x[0], g.entities.y[0]), (px, py));
                moved = true;
                break;
            }
        }
        assert!(
            moved,
            "player should be able to move in at least one direction"
        );
    }

    #[test]
    fn wait_passes_turn() {
        let mut g = MicroGameState::new(42);
        let result = g.step(MicroCommand::Wait);
        assert!(result.action_taken);
        assert_eq!(g.turn_count, 1);
    }

    #[test]
    fn game_over_blocks_step() {
        let mut g = MicroGameState::new(42);
        g.game_over = true;
        let result = g.step(MicroCommand::Wait);
        assert!(!result.action_taken);
        assert!(result.game_over);
    }

    #[test]
    fn deterministic_with_same_seed() {
        let mut a = MicroGameState::new(1234);
        let mut b = MicroGameState::new(1234);

        // Same initial state
        assert_eq!(a.entities.count, b.entities.count);
        assert_eq!(a.map.tiles, b.map.tiles);

        // Run same commands
        for _ in 0..10 {
            a.step(MicroCommand::Wait);
            b.step(MicroCommand::Wait);
        }

        assert_eq!(a.turn_count, b.turn_count);
        assert_eq!(a.kills, b.kills);
        assert_eq!(a.rng.state(), b.rng.state());
        assert_eq!(a.entities.hp[0], b.entities.hp[0]);
    }

    #[test]
    fn regen_heals_player() {
        let mut g = MicroGameState::new(42);
        // Damage the player
        let pi = PLAYER_IDX as usize;
        g.entities.hp[pi] = g.entities.max_hp[pi] - 5;
        let hp_after_damage = g.entities.hp[pi];

        // Step enough turns for regen to kick in
        for _ in 0..(balance::REGEN_INTERVAL as u16 * 3) {
            if g.game_over {
                break;
            }
            g.step(MicroCommand::Wait);
        }

        if !g.game_over {
            assert!(
                g.entities.hp[pi] > hp_after_damage,
                "player should have regenerated HP"
            );
        }
    }
}
