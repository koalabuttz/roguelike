//! Cross-tier game state interface for rendering and interaction.
//!
//! `GameView` provides a uniform, `no_std` view of game state that works
//! identically for micro (u8 coords) and compact (i32 coords) tiers.
//! All coordinates are normalized to `i32`. The GBA uses `impl GameView`
//! for monomorphized zero-overhead dispatch; future platforms can use
//! `dyn GameView` if heap allocation is available.

use super::balance;
use super::color::GameColor;
use super::command::GameCommand;
use super::items::{self as rules_items, Equipment, Inventory, ItemKind};
use super::message::GameEvent;
use super::monster_table::{self, MonsterKind};
use super::tiles as tile_rules;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::command::GameCommand;

    /// Minimal mock for testing `GameView` default implementations.
    /// Only fields used by the defaults need real values.
    struct MockView {
        visible: bool,
        explored: bool,
        tile: u8,
        game_over: bool,
        game_won: bool,
        entity_alive: bool,
        entity_kind: Option<MonsterKind>,
        item_kind: ItemKind,
        equipment: Equipment,
        inventory: Inventory,
    }

    impl Default for MockView {
        fn default() -> Self {
            Self {
                visible: false,
                explored: false,
                tile: 0,
                game_over: false,
                game_won: false,
                entity_alive: true,
                entity_kind: Some(MonsterKind::Goblin),
                item_kind: ItemKind::HealthPotion,
                equipment: Equipment::default(),
                inventory: Inventory::default(),
            }
        }
    }

    impl GameView for MockView {
        fn map_dims(&self) -> (i32, i32) {
            (10, 10)
        }
        fn map_in_bounds(&self, _x: i32, _y: i32) -> bool {
            true
        }
        fn tile_at(&self, _x: i32, _y: i32) -> u8 {
            self.tile
        }
        fn is_visible(&self, _x: i32, _y: i32) -> bool {
            self.visible
        }
        fn is_explored(&self, _x: i32, _y: i32) -> bool {
            self.explored
        }
        fn player_xy(&self) -> (i32, i32) {
            (5, 5)
        }
        fn player_hp(&self) -> (u8, u8) {
            (10, 10)
        }
        fn effective_attack(&self) -> u8 {
            3
        }
        fn effective_defense(&self) -> u8 {
            1
        }
        fn entity_count(&self) -> usize {
            2
        }
        fn entity_xy(&self, _i: usize) -> (i32, i32) {
            (1, 1)
        }
        fn entity_alive(&self, _i: usize) -> bool {
            self.entity_alive
        }
        fn entity_kind(&self, _i: usize) -> Option<MonsterKind> {
            self.entity_kind
        }
        fn entity_hp(&self, _i: usize) -> (u8, u8) {
            (5, 5)
        }
        fn entity_at(&self, _x: i32, _y: i32) -> Option<u8> {
            None
        }
        fn item_count(&self) -> usize {
            1
        }
        fn item_xy(&self, _i: usize) -> (i32, i32) {
            (3, 3)
        }
        fn item_alive(&self, _i: usize) -> bool {
            true
        }
        fn item_kind_at(&self, _i: usize) -> ItemKind {
            self.item_kind
        }
        fn item_at(&self, _x: i32, _y: i32) -> Option<u8> {
            None
        }
        fn equipment(&self) -> &Equipment {
            &self.equipment
        }
        fn inventory(&self) -> &Inventory {
            &self.inventory
        }
        fn depth(&self) -> u8 {
            1
        }
        fn kills(&self) -> u8 {
            0
        }
        fn turn_count(&self) -> u16 {
            0
        }
        fn game_over(&self) -> bool {
            self.game_over
        }
        fn game_won(&self) -> bool {
            self.game_won
        }
        fn seed_u32(&self) -> u32 {
            42
        }
        fn explored_pct(&self) -> u8 {
            50
        }
        fn target_depth(&self) -> u8 {
            5
        }
        fn recent_message(&self, _n: u8) -> Option<GameEvent> {
            None
        }
        fn step_view(&mut self, _cmd: GameCommand) -> GameViewStep {
            GameViewStep {
                action_taken: false,
                game_over: false,
                game_won: false,
            }
        }
    }

    #[test]
    fn is_terminal_both_false() {
        let v = MockView::default();
        assert!(!v.is_terminal());
    }

    #[test]
    fn is_terminal_game_over() {
        let v = MockView {
            game_over: true,
            ..Default::default()
        };
        assert!(v.is_terminal());
    }

    #[test]
    fn is_terminal_game_won() {
        let v = MockView {
            game_won: true,
            ..Default::default()
        };
        assert!(v.is_terminal());
    }

    #[test]
    fn tile_visibility_visible_wins() {
        let v = MockView {
            visible: true,
            explored: true,
            ..Default::default()
        };
        assert_eq!(v.tile_visibility(0, 0), TileVisibility::Visible);
    }

    #[test]
    fn tile_visibility_explored() {
        let v = MockView {
            visible: false,
            explored: true,
            ..Default::default()
        };
        assert_eq!(v.tile_visibility(0, 0), TileVisibility::Explored);
    }

    #[test]
    fn tile_visibility_unexplored() {
        let v = MockView::default();
        assert_eq!(v.tile_visibility(0, 0), TileVisibility::Unexplored);
    }

    #[test]
    fn tile_is_structural_true() {
        let v = MockView {
            tile: 1,
            ..Default::default()
        };
        assert!(v.tile_is_structural(0, 0));
    }

    #[test]
    fn tile_is_structural_false() {
        let v = MockView {
            tile: 0,
            ..Default::default()
        };
        assert!(!v.tile_is_structural(0, 0));
    }

    #[test]
    fn render_tile_floor() {
        // tile_at=2 is TileKind::Floor
        let v = MockView {
            tile: 2,
            ..Default::default()
        };
        let (glyph, color) = v.render_tile(0, 0);
        assert_eq!(glyph, tile_rules::glyph(tile_rules::TileKind::Floor));
        assert_eq!(color, tile_rules::color(tile_rules::TileKind::Floor));
    }

    #[test]
    fn render_tile_unknown() {
        let v = MockView {
            tile: 255,
            ..Default::default()
        };
        let (glyph, color) = v.render_tile(0, 0);
        assert_eq!(glyph, ' ');
        assert_eq!(color, GameColor::Black);
    }

    #[test]
    fn render_entity_player() {
        let v = MockView::default();
        let (glyph, color) = v.render_entity(0);
        assert_eq!(glyph, balance::PLAYER_GLYPH);
        assert_eq!(color, GameColor::Green);
    }

    #[test]
    fn render_entity_corpse() {
        let v = MockView {
            entity_alive: false,
            ..Default::default()
        };
        let (glyph, color) = v.render_entity(1);
        assert_eq!(glyph, '%');
        assert_eq!(color, GameColor::DarkRed);
    }

    #[test]
    fn render_entity_monster() {
        let v = MockView {
            entity_kind: Some(MonsterKind::Goblin),
            ..Default::default()
        };
        let (glyph, color) = v.render_entity(1);
        assert_eq!(glyph, monster_table::glyph(MonsterKind::Goblin));
        assert_eq!(color, monster_table::color(MonsterKind::Goblin));
    }

    #[test]
    fn render_item_delegates() {
        let v = MockView {
            item_kind: ItemKind::HealthPotion,
            ..Default::default()
        };
        let (glyph, color) = v.render_item(0);
        assert_eq!(glyph, rules_items::glyph(ItemKind::HealthPotion));
        assert_eq!(color, rules_items::color(ItemKind::HealthPotion));
    }
}
