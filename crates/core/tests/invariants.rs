//! Property-based tests for core game invariants.
//!
//! Uses `proptest` to generate random `GameCommand` sequences and verifies
//! that fundamental invariants hold after every `step()`. These tests catch
//! edge cases in the game logic that deterministic tests miss.

use proptest::prelude::*;

use roguelike_core::command::GameCommand;
use roguelike_core::game::GameState;

/// Generate a random GameCommand: Move in any direction or Wait.
fn arb_game_command() -> impl Strategy<Value = GameCommand> {
    prop_oneof![
        8 => (-1..=1i32, -1..=1i32)
            .prop_filter("not zero move", |(dx, dy)| *dx != 0 || *dy != 0)
            .prop_map(|(dx, dy)| GameCommand::Move { dx, dy }),
        2 => Just(GameCommand::Wait),
    ]
}

/// Generate a sequence of random commands with a random seed.
fn arb_game_scenario() -> impl Strategy<Value = (u64, Vec<GameCommand>)> {
    (
        any::<u64>(),
        proptest::collection::vec(arb_game_command(), 50..=200),
    )
}

/// Track which entities have been seen dead so we can verify they stay dead.
struct DeathTracker {
    /// (entity_index, x, y) for entities seen with alive == false.
    dead_entities: Vec<usize>,
}

impl DeathTracker {
    fn new() -> Self {
        Self {
            dead_entities: Vec::new(),
        }
    }

    fn record_deaths(&mut self, state: &GameState) {
        for (i, entity) in state.entities.iter().enumerate() {
            if !entity.alive && !self.dead_entities.contains(&i) {
                self.dead_entities.push(i);
            }
        }
    }

    fn verify_dead_stay_dead(&self, state: &GameState) {
        for &idx in &self.dead_entities {
            let entity = &state.entities[idx];
            assert!(
                !entity.alive,
                "Entity {} ('{}') at ({}, {}) was dead but came back alive!",
                idx, entity.name, entity.x, entity.y,
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn game_invariants_hold_under_random_commands((seed, commands) in arb_game_scenario()) {
        let mut state = GameState::with_seed(80, 40, seed);
        state.update_fov();

        let mut prev_explored_size = state.explored.len();
        let mut death_tracker = DeathTracker::new();

        for cmd in &commands {
            // Record pre-step dead entities.
            death_tracker.record_deaths(&state);

            let _result = state.step(*cmd);

            // Invariant 1: HP never exceeds max_hp.
            let player = &state.entities[0];
            prop_assert!(
                player.hp <= player.max_hp,
                "HP {} exceeded max_hp {} (seed={}, turn={})",
                player.hp, player.max_hp, seed, state.turn_count,
            );

            // Invariant 2: game_over == true iff player.hp <= 0.
            if state.game_over {
                prop_assert!(
                    player.hp <= 0,
                    "Game over but HP is {} (seed={}, turn={})",
                    player.hp, seed, state.turn_count,
                );
            }
            if player.hp <= 0 {
                prop_assert!(
                    state.game_over,
                    "HP is {} but game_over is false (seed={}, turn={})",
                    player.hp, seed, state.turn_count,
                );
            }

            // Invariant 3: Explored set never shrinks.
            let current_explored_size = state.explored.len();
            prop_assert!(
                current_explored_size >= prev_explored_size,
                "Explored set shrank from {} to {} (seed={}, turn={})",
                prev_explored_size, current_explored_size, seed, state.turn_count,
            );
            prev_explored_size = current_explored_size;

            // Invariant 4: Dead entities stay dead.
            death_tracker.verify_dead_stay_dead(&state);

            // Invariant 5: Player is on a walkable tile (while alive).
            if player.alive {
                let px = player.x;
                let py = player.y;
                prop_assert!(
                    state.map.in_bounds(px, py),
                    "Player at ({}, {}) is out of bounds (seed={}, turn={})",
                    px, py, seed, state.turn_count,
                );
                let tile = state.map.tiles[state.map.idx(px, py)];
                prop_assert!(
                    tile.is_walkable(),
                    "Player at ({}, {}) is on {:?}, not walkable (seed={}, turn={})",
                    px, py, tile, seed, state.turn_count,
                );
            }

            // Invariant 6: observe() doesn't panic.
            let _obs = state.observe();

            // Invariant 7: Entity count stays within budget.
            // max_wandering (default 5) + initial monsters + player.
            // Allow generous headroom — the real cap is enforced by try_spawn_wandering().
            let alive_count = state.entities.iter().filter(|e| e.alive).count();
            prop_assert!(
                alive_count <= 200,
                "Alive entity count {} exceeds budget (seed={}, turn={})",
                alive_count, seed, state.turn_count,
            );

            // Stop stepping if game is over (commands after death are no-ops).
            if state.game_over {
                break;
            }
        }
    }

    #[test]
    fn save_load_roundtrip_preserves_state((seed, commands) in arb_game_scenario()) {
        let mut state = GameState::with_seed(80, 40, seed);
        state.update_fov();

        // Run some commands.
        let steps_to_take = commands.len().min(50);
        for cmd in &commands[..steps_to_take] {
            state.step(*cmd);
            if state.game_over {
                break;
            }
        }

        // Save and reload.
        let json = state.save_to_json().expect("save_to_json failed");
        let loaded = GameState::load_from_json(&json).expect("load_from_json failed");

        // Core state must match.
        prop_assert_eq!(state.entities[0].hp, loaded.entities[0].hp);
        prop_assert_eq!(state.entities[0].x, loaded.entities[0].x);
        prop_assert_eq!(state.entities[0].y, loaded.entities[0].y);
        prop_assert_eq!(state.turn_count, loaded.turn_count);
        prop_assert_eq!(state.game_over, loaded.game_over);
        prop_assert_eq!(state.seed, loaded.seed);
        prop_assert_eq!(state.entities.len(), loaded.entities.len());
        prop_assert_eq!(state.explored.len(), loaded.explored.len());
        prop_assert_eq!(state.wandering_seed, loaded.wandering_seed);
        prop_assert_eq!(state.wandering_spawned, loaded.wandering_spawned);
        prop_assert_eq!(state.idle_count, loaded.idle_count);
    }
}
