//! Cross-tier game state interface for rendering and interaction.
//!
//! `GameView` provides a uniform, `no_std` view of game state that works
//! identically for micro (u8 coords) and compact (i32 coords) tiers.
//! All coordinates are normalized to `i32`. The GBA uses `impl GameView`
//! for monomorphized zero-overhead dispatch; future platforms can use
//! `dyn GameView` if heap allocation is available.

use super::balance;
use super::color::GameColor;
use super::items::{self as rules_items, Equipment, Inventory, ItemKind};
use super::message::GameEvent;
use super::monster_table::{self, MonsterKind};
use super::tiles as tile_rules;
use crate::command::GameCommand;

/// Unified step result across tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameViewStep {
    pub action_taken: bool,
    pub game_over: bool,
    pub game_won: bool,
}

/// Tile visibility state for rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileVisibility {
    /// Not yet seen by the player.
    Unexplored,
    /// Previously seen but not currently in FOV.
    Explored,
    /// Currently in the player's field of view.
    Visible,
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
    /// Percentage of floor tiles explored (0–100).
    fn explored_pct(&self) -> u8;
    /// Target depth (win condition). 0 if not applicable.
    fn target_depth(&self) -> u8;

    // -- Messages --
    fn recent_message(&self, n: u8) -> Option<GameEvent>;

    // -- Stepping --
    /// Execute one command. Returns a no_std-compatible result.
    /// Named `step_view` to avoid conflict with `GameStep::step` which
    /// returns `StepResult` (includes `Vec<String>` messages).
    fn step_view(&mut self, cmd: GameCommand) -> GameViewStep;

    /// Whether the game has ended (player died or won).
    fn is_terminal(&self) -> bool {
        self.game_over() || self.game_won()
    }

    // -- Render-ready methods (default implementations) --

    /// Tile visibility state at (x, y).
    fn tile_visibility(&self, x: i32, y: i32) -> TileVisibility {
        if self.is_visible(x, y) {
            TileVisibility::Visible
        } else if self.is_explored(x, y) {
            TileVisibility::Explored
        } else {
            TileVisibility::Unexplored
        }
    }

    /// Whether the tile at (x, y) is a structural wall (visible wall adjacent to floor).
    fn tile_is_structural(&self, x: i32, y: i32) -> bool {
        self.tile_at(x, y) == 1 // TILE_STRUCTURAL
    }

    /// Render-ready tile: (glyph, color). Returns (' ', Black) for unknown tiles.
    fn render_tile(&self, x: i32, y: i32) -> (char, GameColor) {
        match tile_rules::from_micro(self.tile_at(x, y)) {
            Some(kind) => (tile_rules::glyph(kind), tile_rules::color(kind)),
            None => (' ', GameColor::Black),
        }
    }

    /// Render-ready entity: (glyph, color). Handles player, corpses, monsters.
    fn render_entity(&self, i: usize) -> (char, GameColor) {
        if i == 0 {
            (balance::PLAYER_GLYPH, GameColor::Green)
        } else if !self.entity_alive(i) {
            ('%', GameColor::DarkRed)
        } else {
            match self.entity_kind(i) {
                Some(kind) => (monster_table::glyph(kind), monster_table::color(kind)),
                None => ('?', GameColor::White),
            }
        }
    }

    /// Render-ready item: (glyph, color).
    fn render_item(&self, i: usize) -> (char, GameColor) {
        let kind = self.item_kind_at(i);
        (rules_items::glyph(kind), rules_items::color(kind))
    }
}
