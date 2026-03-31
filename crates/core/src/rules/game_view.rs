//! Cross-tier game state interface for rendering and interaction.
//!
//! `GameView` provides a uniform, `no_std` view of game state that works
//! identically for micro (u8 coords) and compact (i32 coords) tiers.
//! All coordinates are normalized to `i32`. The GBA uses `impl GameView`
//! for monomorphized zero-overhead dispatch; future platforms can use
//! `dyn GameView` if heap allocation is available.

use super::items::{Equipment, Inventory, ItemKind};
use super::message::GameEvent;
use super::monster_table::MonsterKind;
use crate::command::GameCommand;

/// Unified step result across tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameViewStep {
    pub action_taken: bool,
    pub game_over: bool,
    pub game_won: bool,
}

/// Cross-tier read/write interface for game state.
///
/// Implemented by `CompactGameState` and `MicroGameState`. Coords are
/// always `i32` — micro implementations widen `u8` on return and narrow
/// on input.
pub trait GameView {
    // -- Map --
    fn map_dims(&self) -> (i32, i32);
    fn map_in_bounds(&self, x: i32, y: i32) -> bool;
    fn tile_at(&self, x: i32, y: i32) -> u8;

    // -- FOV --
    fn is_visible(&self, x: i32, y: i32) -> bool;
    fn is_explored(&self, x: i32, y: i32) -> bool;

    // -- Player --
    fn player_xy(&self) -> (i32, i32);
    fn player_hp(&self) -> (u8, u8);
    fn effective_attack(&self) -> u8;
    fn effective_defense(&self) -> u8;

    // -- Entities --
    fn entity_count(&self) -> usize;
    fn entity_xy(&self, i: usize) -> (i32, i32);
    fn entity_alive(&self, i: usize) -> bool;
    fn entity_kind(&self, i: usize) -> Option<MonsterKind>;
    fn entity_hp(&self, i: usize) -> (u8, u8);
    /// Find the first alive entity at (x, y). Returns slot index or None.
    fn entity_at(&self, x: i32, y: i32) -> Option<u8>;

    // -- Items --
    fn item_count(&self) -> usize;
    fn item_xy(&self, i: usize) -> (i32, i32);
    fn item_alive(&self, i: usize) -> bool;
    fn item_kind_at(&self, i: usize) -> ItemKind;
    /// Find the first alive item at (x, y). Returns slot index or None.
    fn item_at(&self, x: i32, y: i32) -> Option<u8>;

    // -- Inventory & equipment --
    fn equipment(&self) -> &Equipment;
    fn inventory(&self) -> &Inventory;

    // -- Game state --
    fn depth(&self) -> u8;
    fn kills(&self) -> u8;
    fn turn_count(&self) -> u16;
    fn game_over(&self) -> bool;
    fn game_won(&self) -> bool;
    fn seed_u32(&self) -> u32;

    // -- Messages --
    fn recent_message(&self, n: u8) -> Option<GameEvent>;

    // -- Stepping --
    fn step(&mut self, cmd: GameCommand) -> GameViewStep;
}
