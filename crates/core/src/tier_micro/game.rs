//! Top-level micro-tier game state and step API.
//!
//! `MicroGameState` owns all game data — map, entities, FOV, messages, RNG.
//! The `step()` method processes one player command and runs a full game tick.

use super::ai;
use super::combat;
use super::entity::EntityStore;
use super::fov::MicroFov;
use super::map::{MicroMap, TILE_STAIRS_DOWN};
use super::msglog::MicroMessageLog;
use super::prng::LfsrRng16;
use super::spawn;
use super::types::*;
use crate::command::GameCommand;
use crate::rules::balance;
use crate::rules::message::GameEvent;

/// Result of a single step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MicroStepResult {
    pub action_taken: bool,
    pub game_over: bool,
    pub game_won: bool,
}

pub struct MicroGameState {
    pub map: MicroMap,
    pub fov: MicroFov,
    pub entities: EntityStore,
    pub log: MicroMessageLog,
    pub rng: LfsrRng16,
    /// Original seed used to create this game (for display/sharing).
    pub seed: u16,
    pub turn_count: u16,
    pub kills: u8,
    pub depth: u8,
    pub game_over: bool,
    pub game_won: bool,
    /// Counts down each turn; triggers regen at zero and resets.
    /// Avoids modulo on 6502 where division is expensive.
    regen_counter: u8,
}

impl MicroGameState {
    /// Create a new game with the given seed and map dimensions.
    pub fn new(seed: u16, width: u8, height: u8) -> Self {
        let mut rng = LfsrRng16::new(seed);
        let mut map = MicroMap::new(width, height);
        let (sx, sy) = map.generate(&mut rng);

        let mut entities = EntityStore::new();
        entities.spawn_player(sx, sy);
        spawn::spawn_monsters(&mut entities, &map, &mut rng);

        let mut fov = MicroFov::new(width, height);
        fov.compute_fov(sx, sy, &map);

        let mut log = MicroMessageLog::new();
        log.add(GameEvent::Welcome);

        Self {
            map,
            fov,
            entities,
            log,
            rng,
            seed,
            turn_count: 0,
            kills: 0,
            depth: 1,
            game_over: false,
            game_won: false,
            regen_counter: balance::REGEN_INTERVAL,
        }
    }

    /// Returns true if the game has reached a terminal state (death or victory).
    pub fn is_terminal(&self) -> bool {
        self.game_over || self.game_won
    }

    /// Create a new game with C64 default dimensions (64×48).
    pub fn new_default(seed: u16) -> Self {
        Self::new(seed, DEFAULT_MAP_WIDTH, DEFAULT_MAP_HEIGHT)
    }

    /// Execute one player command + monster turns + regen.
    ///
    /// Accepts the full `GameCommand` enum. Variants the micro tier doesn't
    /// support (Autorun, AutoExplore, Look, etc.) are silently ignored.
    pub fn step(&mut self, cmd: GameCommand) -> MicroStepResult {
        if self.is_terminal() {
            return MicroStepResult {
                action_taken: false,
                game_over: self.game_over,
                game_won: self.game_won,
            };
        }

        // Descent is handled separately — it rebuilds the level and FOV.
        if matches!(cmd, GameCommand::Descend) {
            let descended = self.descend();
            return MicroStepResult {
                action_taken: descended,
                game_over: self.game_over,
                game_won: self.game_won,
            };
        }

        let action_taken = match cmd {
            GameCommand::Wait => true,
            GameCommand::Move(dir) => {
                let (dx, dy) = dir.to_offset();
                self.player_move_or_attack(dx as i8, dy as i8)
            }
            // Unsupported variants — no action taken
            _ => false,
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
            game_won: self.game_won,
        }
    }

    /// Descend to the next dungeon level. Returns true if descent succeeded.
    fn descend(&mut self) -> bool {
        let pi = PLAYER_IDX as usize;
        let px = self.entities.x[pi];
        let py = self.entities.y[pi];

        // Must be standing on stairs
        if self.map.tile_at(px, py) != TILE_STAIRS_DOWN {
            self.log.add(GameEvent::NoStairs);
            return false;
        }

        // Victory condition: descending from the final floor
        if self.depth >= balance::TARGET_DEPTH {
            self.game_won = true;
            self.log.add(GameEvent::Victory {
                depth: balance::TARGET_DEPTH,
            });
            return true;
        }

        self.depth += 1;

        // Derive deterministic seed for this floor
        let floor_seed = self.seed ^ (self.depth as u16).wrapping_mul(0x9E37);
        self.rng = LfsrRng16::new(floor_seed);

        // Save player stats
        let hp = self.entities.hp[pi];
        let max_hp = self.entities.max_hp[pi];
        let atk = self.entities.atk[pi];
        let def = self.entities.def[pi];

        // Generate new map
        let w = self.map.width;
        let h = self.map.height;
        self.map = MicroMap::new(w, h);
        let (sx, sy) = self.map.generate(&mut self.rng);

        // Reset entities — player keeps stats
        self.entities = EntityStore::new();
        self.entities.spawn_player(sx, sy);
        self.entities.hp[pi] = hp;
        self.entities.max_hp[pi] = max_hp;
        self.entities.atk[pi] = atk;
        self.entities.def[pi] = def;

        // Spawn and scale monsters
        spawn::spawn_monsters(&mut self.entities, &self.map, &mut self.rng);
        spawn::apply_depth_scaling(&mut self.entities, self.depth);

        // Reset FOV
        self.fov = MicroFov::new(w, h);
        self.fov.compute_fov(sx, sy, &self.map);

        // Reset message log for new floor
        self.log.reset();
        self.log.add(GameEvent::Descend {
            depth: self.depth,
            target: balance::TARGET_DEPTH,
        });

        true
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
        self.regen_counter -= 1;
        if self.regen_counter == 0 {
            self.regen_counter = balance::REGEN_INTERVAL;
            let pi = PLAYER_IDX as usize;
            let hp = self.entities.hp[pi];
            let max_hp = self.entities.max_hp[pi];
            if hp < max_hp {
                self.entities.hp[pi] = hp + 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Direction;

    #[test]
    fn new_game_is_playable() {
        let g = MicroGameState::new_default(42);
        assert!(!g.game_over);
        assert!(g.entities.alive[PLAYER_IDX as usize]);
        assert!(g.entities.hp[PLAYER_IDX as usize] > 0);
        assert!(g.entities.count > 1, "should have monsters");
    }

    #[test]
    fn move_changes_position() {
        let mut g = MicroGameState::new_default(42);
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
                let result = g.step(GameCommand::Move(dir));
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
        let mut g = MicroGameState::new_default(42);
        let result = g.step(GameCommand::Wait);
        assert!(result.action_taken);
        assert_eq!(g.turn_count, 1);
    }

    #[test]
    fn game_over_blocks_step() {
        let mut g = MicroGameState::new_default(42);
        g.game_over = true;
        let result = g.step(GameCommand::Wait);
        assert!(!result.action_taken);
        assert!(result.game_over);
    }

    #[test]
    fn deterministic_with_same_seed() {
        let mut a = MicroGameState::new_default(1234);
        let mut b = MicroGameState::new_default(1234);

        // Same initial state
        assert_eq!(a.entities.count, b.entities.count);
        let size = (a.map.width as usize) * (a.map.height as usize);
        assert_eq!(a.map.tiles[..size], b.map.tiles[..size]);

        // Run same commands
        for _ in 0..10 {
            a.step(GameCommand::Wait);
            b.step(GameCommand::Wait);
        }

        assert_eq!(a.turn_count, b.turn_count);
        assert_eq!(a.kills, b.kills);
        assert_eq!(a.rng.state(), b.rng.state());
        assert_eq!(a.entities.hp[0], b.entities.hp[0]);
    }

    #[test]
    fn regen_heals_player() {
        let mut g = MicroGameState::new_default(42);
        // Damage the player
        let pi = PLAYER_IDX as usize;
        g.entities.hp[pi] = g.entities.max_hp[pi] - 5;
        let hp_after_damage = g.entities.hp[pi];

        // Step enough turns for regen to kick in
        for _ in 0..(balance::REGEN_INTERVAL as u16 * 3) {
            if g.game_over {
                break;
            }
            g.step(GameCommand::Wait);
        }

        if !g.game_over {
            assert!(
                g.entities.hp[pi] > hp_after_damage,
                "player should have regenerated HP"
            );
        }
    }

    #[test]
    fn custom_dimensions() {
        let g = MicroGameState::new(42, 80, 40);
        assert_eq!(g.map.width, 80);
        assert_eq!(g.map.height, 40);
        assert!(!g.game_over);
        assert!(g.entities.count > 1);
    }

    #[test]
    fn descend_on_stairs_succeeds() {
        let mut g = MicroGameState::new_default(42);
        // Teleport player to stairs (last room center)
        let last = g.map.rooms[(g.map.room_count - 1) as usize];
        g.entities.x[0] = last.cx();
        g.entities.y[0] = last.cy();

        let result = g.step(GameCommand::Descend);
        assert!(result.action_taken);
        assert_eq!(g.depth, 2);
        assert!(!result.game_won);
    }

    #[test]
    fn descend_not_on_stairs_fails() {
        let mut g = MicroGameState::new_default(42);
        // Player starts on floor in room 0, not on stairs
        let result = g.step(GameCommand::Descend);
        assert!(!result.action_taken);
        assert_eq!(g.depth, 1);
        // Should have logged NoStairs
        assert_eq!(g.log.recent(0), Some(GameEvent::NoStairs));
    }

    #[test]
    fn victory_after_target_depth() {
        let mut g = MicroGameState::new_default(42);
        for _ in 0..balance::TARGET_DEPTH {
            // Teleport to stairs and descend
            let last = g.map.rooms[(g.map.room_count - 1) as usize];
            g.entities.x[0] = last.cx();
            g.entities.y[0] = last.cy();
            g.step(GameCommand::Descend);
        }
        assert!(g.game_won);
        assert_eq!(g.depth, balance::TARGET_DEPTH);
    }

    #[test]
    fn player_hp_carries_over() {
        let mut g = MicroGameState::new_default(42);
        g.entities.hp[0] = 15; // damage player
        let last = g.map.rooms[(g.map.room_count - 1) as usize];
        g.entities.x[0] = last.cx();
        g.entities.y[0] = last.cy();

        g.step(GameCommand::Descend);
        assert_eq!(g.entities.hp[0], 15, "HP should carry over");
        assert_eq!(g.entities.max_hp[0], balance::PLAYER_HP);
    }

    #[test]
    fn deterministic_floor_generation() {
        // Two games with same seed should produce identical level 2
        let mut a = MicroGameState::new_default(100);
        let mut b = MicroGameState::new_default(100);

        // Descend both to level 2
        for g in [&mut a, &mut b] {
            let last = g.map.rooms[(g.map.room_count - 1) as usize];
            g.entities.x[0] = last.cx();
            g.entities.y[0] = last.cy();
            g.step(GameCommand::Descend);
        }

        let size = (a.map.width as usize) * (a.map.height as usize);
        assert_eq!(a.map.tiles[..size], b.map.tiles[..size]);
        assert_eq!(a.entities.count, b.entities.count);
        assert_eq!(a.depth, b.depth);
    }

    #[test]
    fn floor_seeds_decorrelated() {
        // Seed N at depth 2 must NOT produce the same map as seed N+1 at depth 1
        let mut a = MicroGameState::new_default(100);
        let b = MicroGameState::new_default(101);

        // Descend game A to depth 2
        let last = a.map.rooms[(a.map.room_count - 1) as usize];
        a.entities.x[0] = last.cx();
        a.entities.y[0] = last.cy();
        a.step(GameCommand::Descend);
        assert_eq!(a.depth, 2);

        // Game B stays at depth 1 — compare maps
        let size = (a.map.width as usize) * (a.map.height as usize);
        assert_ne!(
            a.map.tiles[..size],
            b.map.tiles[..size],
            "seed 100 depth 2 should differ from seed 101 depth 1"
        );
    }

    #[test]
    fn monsters_scaled_on_deeper_floors() {
        let mut g = MicroGameState::new_default(42);
        let last = g.map.rooms[(g.map.room_count - 1) as usize];
        g.entities.x[0] = last.cx();
        g.entities.y[0] = last.cy();

        g.step(GameCommand::Descend);
        assert_eq!(g.depth, 2);

        // Check that at least one monster has scaled stats
        if g.entities.count > 1 {
            let kind = g.entities.kind[1].unwrap();
            let base_hp = crate::rules::monster_table::max_hp(kind);
            assert_eq!(
                g.entities.hp[1],
                base_hp + balance::MONSTER_HP_PER_FLOOR,
                "monster HP should be scaled for depth 2"
            );
        }
    }
}
