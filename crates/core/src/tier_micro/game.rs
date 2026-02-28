//! Top-level micro-tier game state and step API.
//!
//! `MicroGameState` owns all game data — map, entities, FOV, messages, RNG.
//! The `step()` method processes one player command and runs a full game tick.

use super::ai;
use super::combat;
use super::entity::EntityStore;
use super::fov::MicroFov;
use super::item_store::ItemStore;
use super::map::{MicroMap, TILE_STAIRS_DOWN};
use super::msglog::MicroMessageLog;
use super::prng::LfsrRng16;
use super::spawn;
use super::types::*;
use crate::command::GameCommand;
use crate::rules::balance;
use crate::rules::damage;
use crate::rules::items::{self as rules_items, Equipment};
use crate::rules::message::{GameEvent, SoundDistance};
use crate::rules::monster_table::AiBehavior;

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
    pub items: ItemStore,
    pub equipment: Equipment,
    pub log: MicroMessageLog,
    pub rng: LfsrRng16,
    /// Original seed used to create this game (for display/sharing).
    pub seed: u16,
    pub turn_count: u16,
    pub kills: u8,
    pub depth: u8,
    pub game_over: bool,
    pub game_won: bool,
    /// Consecutive Wait commands (resets on any non-wait action).
    pub idle_count: u8,
    /// Total wandering monsters spawned this game (for analytics).
    pub wandering_spawned: u8,
    /// Counts down each turn; triggers regen at zero and resets.
    /// Avoids modulo on 6502 where division is expensive.
    regen_counter: u8,
    /// Counts down each turn; triggers wandering spawn check at zero.
    /// Uses the same decrement pattern as regen_counter to avoid modulo.
    wandering_counter: u8,
    /// Counts down each turn; triggers ambient sound check at zero.
    ambient_sound_counter: u8,
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

        let mut items = ItemStore::new();
        spawn::spawn_items(&mut items, &map, &mut rng);

        let mut fov = MicroFov::new(width, height);
        fov.compute_fov(sx, sy, &map);

        let mut log = MicroMessageLog::new();
        log.add(GameEvent::Welcome);

        Self {
            map,
            fov,
            entities,
            items,
            equipment: Equipment::default(),
            log,
            rng,
            seed,
            turn_count: 0,
            kills: 0,
            depth: 1,
            game_over: false,
            game_won: false,
            idle_count: 0,
            wandering_spawned: 0,
            regen_counter: balance::REGEN_INTERVAL,
            wandering_counter: balance::WANDERING_GRACE_PERIOD - 1,
            ambient_sound_counter: balance::WANDERING_AMBIENT_SOUND_INTERVAL - 1,
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

        let is_wait = matches!(cmd, GameCommand::Wait);
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
            if is_wait {
                self.idle_count = self.idle_count.saturating_add(1);
            } else {
                self.idle_count = 0;
            }

            let px = self.entities.x[PLAYER_IDX as usize];
            let py = self.entities.y[PLAYER_IDX as usize];
            self.fov.compute_fov(px, py, &self.map);

            let player_def = self.effective_defense();
            let player_died = ai::run_monster_turns(
                &mut self.entities,
                &self.map,
                &mut self.rng,
                &mut self.log,
                player_def,
            );
            if player_died {
                self.game_over = true;
                self.log.add(GameEvent::PlayerDeath);
            }

            self.turn_count += 1;
            if !self.game_won {
                self.try_spawn_wandering();
                self.emit_ambient_sound_cues();
            }
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

        // Spawn and scale monsters, spawn items
        spawn::spawn_monsters(&mut self.entities, &self.map, &mut self.rng);
        spawn::apply_depth_scaling(&mut self.entities, self.depth);
        self.items = ItemStore::new();
        spawn::spawn_items(&mut self.items, &self.map, &mut self.rng);

        // Reset FOV
        self.fov = MicroFov::new(w, h);
        self.fov.compute_fov(sx, sy, &self.map);

        // Reset wandering state for new floor.
        self.idle_count = 0;
        self.wandering_spawned = 0;
        self.wandering_counter = balance::WANDERING_GRACE_PERIOD - 1;
        self.ambient_sound_counter = balance::WANDERING_AMBIENT_SOUND_INTERVAL - 1;

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
            let atk = self.effective_attack();
            let def = self.entities.def[target as usize];
            let killed = combat::melee_attack(
                PLAYER_IDX,
                target,
                atk,
                def,
                &mut self.entities,
                &mut self.log,
            );
            if killed {
                self.kills += 1;
            }
            return true;
        }

        // Try to move
        if self.map.is_walkable(nx, ny) {
            self.entities.x[PLAYER_IDX as usize] = nx;
            self.entities.y[PLAYER_IDX as usize] = ny;
            self.try_pickup_items(nx, ny);
            return true;
        }

        false
    }

    /// Player's effective attack (base + weapon bonus).
    pub fn effective_attack(&self) -> u8 {
        let base = self.entities.atk[PLAYER_IDX as usize];
        damage::effective_attack(base, self.equipment.attack_bonus())
    }

    /// Player's effective defense (base + armor bonus).
    pub fn effective_defense(&self) -> u8 {
        let base = self.entities.def[PLAYER_IDX as usize];
        damage::effective_defense(base, self.equipment.defense_bonus())
    }

    /// Try to pick up items at position. Mirrors standard tier's auto-pickup.
    fn try_pickup_items(&mut self, x: u8, y: u8) {
        let mut i = self.items.count as usize;
        while i > 0 {
            i -= 1;
            if !self.items.alive[i] || self.items.x[i] != x || self.items.y[i] != y {
                continue;
            }
            let kind = self.items.kind[i];

            if rules_items::is_consumable(kind) {
                let heal = rules_items::heal_amount(kind);
                let pi = PLAYER_IDX as usize;
                let hp = self.entities.hp[pi];
                let max_hp = self.entities.max_hp[pi];
                if heal > 0 && hp >= max_hp {
                    continue; // full HP — leave potion on ground
                }
                self.items.remove(i as u8);
                if heal > 0 {
                    let healed = heal.min(max_hp - hp);
                    self.entities.hp[pi] = hp + healed;
                    self.log.add(GameEvent::DrinkPotion { kind, healed });
                }
            } else if rules_items::is_weapon(kind)
                && rules_items::is_better_weapon(kind, self.equipment.weapon)
            {
                self.items.remove(i as u8);
                self.equipment.weapon = Some(kind);
                self.log.add(GameEvent::EquipWeapon {
                    kind,
                    bonus: rules_items::attack_bonus(kind),
                });
            } else if rules_items::is_armor(kind)
                && rules_items::is_better_armor(kind, self.equipment.armor)
            {
                self.items.remove(i as u8);
                self.equipment.armor = Some(kind);
                self.log.add(GameEvent::EquipArmor {
                    kind,
                    bonus: rules_items::defense_bonus(kind),
                });
            }
        }
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

    /// Attempt to spawn a wandering monster offscreen if conditions are met.
    ///
    /// Mirrors the standard tier's `try_spawn_wandering`: grace period, spawn
    /// interval (with idle acceleration), random chance, wandering cap, entity
    /// budget. Spawns in a random room the player isn't in, outside FOV.
    ///
    /// Uses a countdown counter (like `regen_counter`) to avoid modulo on 6502.
    /// The counter starts at `WANDERING_GRACE_PERIOD`, counts down each turn,
    /// then reloads at the spawn interval (halved when idle).
    fn try_spawn_wandering(&mut self) {
        if self.wandering_counter > 0 {
            self.wandering_counter -= 1;
            return;
        }

        // Reload counter: base interval, halved if idle.
        // Uses a right-shift instead of division — safe for 6502 as long as
        // WANDERING_IDLE_ACCELERATION is a power of 2 (enforced at compile time).
        let base = balance::WANDERING_SPAWN_INTERVAL;
        let interval = if self.idle_count >= balance::WANDERING_IDLE_THRESHOLD {
            (base >> balance::WANDERING_IDLE_ACCEL_SHIFT).max(1)
        } else {
            base
        };
        self.wandering_counter = interval - 1;

        if self.rng.range_u8(0, 99) >= balance::WANDERING_SPAWN_CHANCE {
            return;
        }

        if self.entities.count as usize >= MAX_ENTITIES {
            return;
        }

        // Cap alive wandering monsters.
        let mut wander_alive: u8 = 0;
        for i in 1..self.entities.count as usize {
            if self.entities.alive[i] && self.entities.ai[i] == AiBehavior::Wander {
                wander_alive += 1;
            }
        }
        if wander_alive >= balance::WANDERING_MAX_ACTIVE {
            return;
        }

        if let Some((sx, sy)) = self.pick_offscreen_spawn_pos() {
            let kind = spawn::pick_monster_kind(&mut self.rng);
            if self
                .entities
                .spawn_monster(kind, sx, sy, AiBehavior::Wander)
            {
                let idx = (self.entities.count - 1) as usize;
                spawn::scale_monster(&mut self.entities, idx, self.depth);
                self.wandering_spawned += 1;
                self.emit_spawn_sound_cue(sx, sy);
            }
        }
    }

    /// Pick a random floor tile in a room the player isn't in,
    /// outside the player's FOV and not occupied by another entity.
    fn pick_offscreen_spawn_pos(&mut self) -> Option<(u8, u8)> {
        if self.map.room_count == 0 {
            return None;
        }

        let px = self.entities.x[PLAYER_IDX as usize];
        let py = self.entities.y[PLAYER_IDX as usize];

        for _ in 0..10 {
            let room_idx = self.rng.range_u8(0, self.map.room_count - 1) as usize;
            let room = self.map.rooms[room_idx];

            // Skip rooms the player is standing in.
            if room.contains_interior(px, py) {
                continue;
            }

            // Pick a random floor tile inside the room interior.
            if room.w < 3 || room.h < 3 {
                continue;
            }
            let sx = self.rng.range_u8(room.x + 1, room.x + room.w - 1);
            let sy = self.rng.range_u8(room.y + 1, room.y + room.h - 1);

            if !self.map.is_walkable(sx, sy) {
                continue;
            }
            if self.fov.is_visible(sx, sy) {
                continue;
            }
            if self.entities.entity_at(sx, sy) != NO_ENTITY {
                continue;
            }
            return Some((sx, sy));
        }
        None
    }

    /// Emit a distance-based sound cue when a wandering monster spawns.
    fn emit_spawn_sound_cue(&mut self, sx: u8, sy: u8) {
        let px = self.entities.x[PLAYER_IDX as usize];
        let py = self.entities.y[PLAYER_IDX as usize];
        let dist = px.abs_diff(sx) + py.abs_diff(sy);

        let distance = if dist <= balance::WANDERING_SOUND_NEAR {
            Some(SoundDistance::Near)
        } else if dist <= balance::WANDERING_SOUND_MEDIUM {
            Some(SoundDistance::Medium)
        } else if dist <= balance::WANDERING_SOUND_FAR {
            Some(SoundDistance::Far)
        } else {
            None
        };
        if let Some(distance) = distance {
            self.log.add(GameEvent::SoundCue { distance });
        }
    }

    /// Emit ambient sound cues for nearby wandering monsters.
    ///
    /// Uses a countdown counter to avoid modulo. Bails early if a Near
    /// monster is found — no need to scan the rest.
    fn emit_ambient_sound_cues(&mut self) {
        if self.ambient_sound_counter > 0 {
            self.ambient_sound_counter -= 1;
            return;
        }
        self.ambient_sound_counter = balance::WANDERING_AMBIENT_SOUND_INTERVAL - 1;

        let px = self.entities.x[PLAYER_IDX as usize];
        let py = self.entities.y[PLAYER_IDX as usize];

        let mut closest_dist: u8 = u8::MAX;
        for i in 1..self.entities.count as usize {
            if self.entities.alive[i] && self.entities.ai[i] == AiBehavior::Wander {
                let dist = px.abs_diff(self.entities.x[i]) + py.abs_diff(self.entities.y[i]);
                if dist < closest_dist {
                    closest_dist = dist;
                    // Near is the tightest threshold — can't improve, bail.
                    if closest_dist <= balance::WANDERING_SOUND_NEAR {
                        break;
                    }
                }
            }
        }

        let distance = if closest_dist <= balance::WANDERING_SOUND_NEAR {
            Some(SoundDistance::Near)
        } else if closest_dist <= balance::WANDERING_SOUND_MEDIUM {
            Some(SoundDistance::Medium)
        } else if closest_dist <= balance::WANDERING_SOUND_FAR {
            Some(SoundDistance::Far)
        } else {
            None
        };
        if let Some(distance) = distance {
            self.log.add(GameEvent::SoundCue { distance });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Direction;
    use crate::rules::items::ItemKind;

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
        let packed_size = ((a.map.width as usize) * (a.map.height as usize)).div_ceil(2);
        assert_eq!(a.map.tiles[..packed_size], b.map.tiles[..packed_size]);

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

        let packed_size = ((a.map.width as usize) * (a.map.height as usize)).div_ceil(2);
        assert_eq!(a.map.tiles[..packed_size], b.map.tiles[..packed_size]);
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
        let packed_size = ((a.map.width as usize) * (a.map.height as usize)).div_ceil(2);
        assert_ne!(
            a.map.tiles[..packed_size],
            b.map.tiles[..packed_size],
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

    // ── Item and equipment tests ──────────────────────────────────────

    /// Place an item under the player and step onto it.
    fn place_item_at_player(g: &mut MicroGameState, kind: ItemKind) {
        let px = g.entities.x[0];
        let py = g.entities.y[0];
        g.items.spawn(px, py, kind);
    }

    #[test]
    fn items_spawn_on_new_game() {
        let g = MicroGameState::new_default(42);
        assert!(g.items.count > 0, "should have spawned items");
    }

    #[test]
    fn potion_heals_when_hurt() {
        let mut g = MicroGameState::new_default(42);
        let pi = PLAYER_IDX as usize;
        g.entities.hp[pi] = g.entities.max_hp[pi] - 5;
        let hp_before = g.entities.hp[pi];

        place_item_at_player(&mut g, ItemKind::HealthPotion);
        let px = g.entities.x[0];
        let py = g.entities.y[0];
        g.try_pickup_items(px, py);

        assert!(
            g.entities.hp[pi] > hp_before,
            "potion should heal the player"
        );
    }

    #[test]
    fn potion_skipped_at_full_hp() {
        let mut g = MicroGameState::new_default(42);
        // Clear existing items to avoid interference
        g.items = ItemStore::new();
        place_item_at_player(&mut g, ItemKind::HealthPotion);
        let items_before = g.items.count;
        let px = g.entities.x[0];
        let py = g.entities.y[0];
        g.try_pickup_items(px, py);

        // Item should still be alive (not consumed)
        assert!(
            g.items.alive[items_before as usize - 1],
            "potion should stay on ground at full HP"
        );
    }

    #[test]
    fn weapon_auto_equips() {
        let mut g = MicroGameState::new_default(42);
        g.items = ItemStore::new();
        assert_eq!(g.equipment.weapon, None);

        place_item_at_player(&mut g, ItemKind::ShortSword);
        let px = g.entities.x[0];
        let py = g.entities.y[0];
        g.try_pickup_items(px, py);

        assert_eq!(g.equipment.weapon, Some(ItemKind::ShortSword));
    }

    #[test]
    fn armor_auto_equips() {
        let mut g = MicroGameState::new_default(42);
        g.items = ItemStore::new();
        assert_eq!(g.equipment.armor, None);

        place_item_at_player(&mut g, ItemKind::LeatherArmor);
        let px = g.entities.x[0];
        let py = g.entities.y[0];
        g.try_pickup_items(px, py);

        assert_eq!(g.equipment.armor, Some(ItemKind::LeatherArmor));
    }

    #[test]
    fn same_weapon_not_picked_up() {
        let mut g = MicroGameState::new_default(42);
        g.items = ItemStore::new();
        g.equipment.weapon = Some(ItemKind::ShortSword);

        place_item_at_player(&mut g, ItemKind::ShortSword);
        let px = g.entities.x[0];
        let py = g.entities.y[0];
        g.try_pickup_items(px, py);

        // Item should still be on ground
        assert!(g.items.alive[0], "same weapon should not be picked up");
    }

    #[test]
    fn effective_attack_with_weapon() {
        let mut g = MicroGameState::new_default(42);
        let base = g.entities.atk[PLAYER_IDX as usize];
        assert_eq!(g.effective_attack(), base);

        g.equipment.weapon = Some(ItemKind::ShortSword);
        assert_eq!(
            g.effective_attack(),
            base + rules_items::attack_bonus(ItemKind::ShortSword)
        );
    }

    #[test]
    fn effective_defense_with_armor() {
        let mut g = MicroGameState::new_default(42);
        let base = g.entities.def[PLAYER_IDX as usize];
        assert_eq!(g.effective_defense(), base);

        g.equipment.armor = Some(ItemKind::LeatherArmor);
        assert_eq!(
            g.effective_defense(),
            base + rules_items::defense_bonus(ItemKind::LeatherArmor)
        );
    }

    #[test]
    fn equipment_persists_across_descent() {
        let mut g = MicroGameState::new_default(42);
        g.equipment.weapon = Some(ItemKind::ShortSword);
        g.equipment.armor = Some(ItemKind::LeatherArmor);

        let last = g.map.rooms[(g.map.room_count - 1) as usize];
        g.entities.x[0] = last.cx();
        g.entities.y[0] = last.cy();
        g.step(GameCommand::Descend);

        assert_eq!(g.equipment.weapon, Some(ItemKind::ShortSword));
        assert_eq!(g.equipment.armor, Some(ItemKind::LeatherArmor));
    }

    #[test]
    fn items_spawn_on_new_floor() {
        let mut g = MicroGameState::new_default(42);
        let last = g.map.rooms[(g.map.room_count - 1) as usize];
        g.entities.x[0] = last.cx();
        g.entities.y[0] = last.cy();
        g.step(GameCommand::Descend);

        assert!(g.items.count > 0, "new floor should have items");
    }

    // ── Wandering monster tests ──────────────────────────────────────

    /// Helper: run Wait commands until a wandering monster spawns or limit is hit.
    /// Returns the count of alive entities with AiBehavior::Wander.
    fn count_wanderers(g: &MicroGameState) -> u8 {
        let mut count = 0u8;
        for i in 1..g.entities.count as usize {
            if g.entities.alive[i] && g.entities.ai[i] == AiBehavior::Wander {
                count += 1;
            }
        }
        count
    }

    #[test]
    fn no_wandering_before_grace_period() {
        let mut g = MicroGameState::new_default(42);
        let initial_count = g.entities.count;

        // Run turns up to (but not including) the grace period.
        for _ in 0..balance::WANDERING_GRACE_PERIOD {
            if g.game_over {
                break;
            }
            g.step(GameCommand::Wait);
        }

        // No wandering spawns should have occurred — entity count should only
        // decrease (monster kills) or stay the same, never increase beyond
        // what initial room-based spawning created.
        assert!(
            count_wanderers(&g) == 0,
            "no wanderers should spawn before grace period"
        );
        assert!(
            g.wandering_spawned == 0,
            "wandering_spawned counter should be 0"
        );
        // Entity count can't have grown (only killed, never spawned).
        assert!(
            g.entities.count <= initial_count,
            "entity count should not grow before grace period"
        );
    }

    #[test]
    fn wandering_spawns_after_grace_period() {
        let mut g = MicroGameState::new_default(42);
        // Kill all initial monsters to make room and avoid interference.
        for i in 1..g.entities.count {
            g.entities.kill(i);
        }

        // Run well past grace period — enough turns for spawns to happen.
        let target_turns = (balance::WANDERING_GRACE_PERIOD as u16) * 4;
        for _ in 0..target_turns {
            if g.game_over {
                break;
            }
            g.step(GameCommand::Wait);
        }

        assert!(
            g.wandering_spawned > 0,
            "at least one wandering monster should have spawned after {} turns",
            target_turns
        );
    }

    #[test]
    fn wandering_cap_respected() {
        let mut g = MicroGameState::new_default(42);
        // Kill all initial monsters.
        for i in 1..g.entities.count {
            g.entities.kill(i);
        }

        // Run a large number of turns.
        for _ in 0..500 {
            if g.game_over {
                break;
            }
            g.step(GameCommand::Wait);
        }

        let wanderers = count_wanderers(&g);
        assert!(
            wanderers <= balance::WANDERING_MAX_ACTIVE,
            "wanderer count {} exceeds cap {}",
            wanderers,
            balance::WANDERING_MAX_ACTIVE
        );
    }

    #[test]
    fn idle_count_tracks_waits() {
        let mut g = MicroGameState::new_default(42);
        assert_eq!(g.idle_count, 0);

        g.step(GameCommand::Wait);
        assert_eq!(g.idle_count, 1);
        g.step(GameCommand::Wait);
        assert_eq!(g.idle_count, 2);

        // Move resets idle count (find a walkable direction).
        let dirs = [
            Direction::North,
            Direction::South,
            Direction::East,
            Direction::West,
        ];
        for dir in dirs {
            let (dx, dy) = dir.to_offset();
            let px = g.entities.x[0];
            let py = g.entities.y[0];
            let nx = (px as i8 + dx as i8) as u8;
            let ny = (py as i8 + dy as i8) as u8;
            if g.map.is_walkable(nx, ny) && g.entities.monster_at(nx, ny) == NO_ENTITY {
                g.step(GameCommand::Move(dir));
                break;
            }
        }
        assert_eq!(g.idle_count, 0, "move should reset idle count");
    }

    #[test]
    fn wandering_state_resets_on_descent() {
        let mut g = MicroGameState::new_default(42);
        g.idle_count = 10;
        g.wandering_spawned = 3;

        let last = g.map.rooms[(g.map.room_count - 1) as usize];
        g.entities.x[0] = last.cx();
        g.entities.y[0] = last.cy();
        g.step(GameCommand::Descend);

        assert_eq!(g.idle_count, 0, "idle_count should reset on descent");
        assert_eq!(
            g.wandering_spawned, 0,
            "wandering_spawned should reset on descent"
        );
    }
}
