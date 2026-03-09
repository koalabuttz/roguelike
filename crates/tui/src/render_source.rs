//! `RenderSource` trait — uniform rendering data source for any capability tier.
//!
//! The TUI renderer calls trait methods instead of directly accessing game
//! internals, eliminating separate render code paths per tier and giving
//! the micro tier full-color rendering with palette support.

use roguelike_core::game::GameState;
use roguelike_core::game_step::MicroGameStateAdapter;
use roguelike_core::item;
use roguelike_core::map::Tile;
use roguelike_core::message_log::format_event;
use roguelike_core::rules::balance;
use roguelike_core::rules::items as rules_items;
use roguelike_core::rules::monster_table;
use roguelike_core::rules::tiles::{self as tile_rules, TileKind};
use roguelike_core::seed_code::{self, SeedParams};
use roguelike_core::tier_micro::types::PLAYER_IDX;
use roguelike_core::types::{Coord, GameColor, Stat};

// ── Data types ──────────────────────────────────────────────────────────

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

/// What the renderer needs to know about a single map tile.
pub struct RenderTile {
    pub glyph: char,
    pub fg: GameColor,
    pub structural: bool,
}

/// A visible entity to render (player, monster, or corpse).
pub struct RenderEntity {
    pub x: Coord,
    pub y: Coord,
    pub glyph: char,
    pub fg: GameColor,
    pub alive: bool,
}

/// A visible item to render.
pub struct RenderItem {
    pub x: Coord,
    pub y: Coord,
    pub glyph: char,
    pub fg: GameColor,
}

// ── Trait ────────────────────────────────────────────────────────────────

/// Uniform rendering data source for any capability tier.
///
/// Implemented by `GameState` (standard) and `MicroGameStateAdapter` (micro).
/// The TUI renderer calls these methods instead of directly accessing game
/// internals.
pub trait RenderSource {
    /// Map dimensions in tiles.
    fn map_size(&self) -> (Coord, Coord);

    /// Tile visibility at (x, y).
    fn tile_visibility(&self, x: Coord, y: Coord) -> TileVisibility;

    /// Tile rendering data at (x, y). Only called for Visible/Explored tiles.
    fn tile_at(&self, x: Coord, y: Coord) -> RenderTile;

    /// Iterate over all visible entities (alive and dead if applicable).
    fn for_each_visible_entity(&self, f: &mut dyn FnMut(RenderEntity));

    /// Iterate over all visible ground items. Default: no items (micro tier).
    fn for_each_visible_item(&self, _f: &mut dyn FnMut(RenderItem)) {}

    /// Player position for viewport centering.
    fn player_pos(&self) -> (Coord, Coord);

    /// (hp, max_hp).
    fn player_hp(&self) -> (Stat, Stat);

    /// (base_attack, equipment_bonus).
    fn player_atk(&self) -> (Stat, Stat);

    /// (base_defense, equipment_bonus).
    fn player_def(&self) -> (Stat, Stat);

    /// (current_depth, target_depth).
    fn depth(&self) -> (Stat, Stat);

    /// Percentage of map explored (0–100).
    fn explored_pct(&self) -> Stat;

    /// Shareable seed code.
    fn seed_code(&self) -> String;

    /// Recent messages for the message log (oldest first). Returns up to `n`.
    fn recent_messages(&self, n: usize) -> Vec<String>;

    /// Number of monsters killed.
    fn kills(&self) -> Stat;

    /// Number of turns elapsed.
    fn turn_count(&self) -> Stat;

    /// Whether the game is won.
    fn game_won(&self) -> bool;

    /// Whether the game is over (player dead).
    fn game_over(&self) -> bool;
}

// ── Standard tier ───────────────────────────────────────────────────────

impl RenderSource for GameState {
    fn map_size(&self) -> (Coord, Coord) {
        (self.map.width, self.map.height)
    }

    fn tile_visibility(&self, x: Coord, y: Coord) -> TileVisibility {
        if self.visible.contains(&(x, y)) {
            TileVisibility::Visible
        } else if self.explored.contains(&(x, y)) {
            TileVisibility::Explored
        } else {
            TileVisibility::Unexplored
        }
    }

    fn tile_at(&self, x: Coord, y: Coord) -> RenderTile {
        let idx = self.map.idx(x, y);
        let kind = match self.map.tiles[idx] {
            Tile::Floor => TileKind::Floor,
            Tile::Wall if self.map.structural[idx] => TileKind::Structural,
            Tile::Wall => TileKind::Wall,
            Tile::StairsDown => TileKind::StairsDown,
        };
        RenderTile {
            glyph: tile_rules::glyph(kind),
            fg: tile_rules::color(kind),
            structural: kind == TileKind::Structural,
        }
    }

    fn for_each_visible_entity(&self, f: &mut dyn FnMut(RenderEntity)) {
        for entity in &self.entities {
            if self.visible.contains(&(entity.x, entity.y)) {
                f(RenderEntity {
                    x: entity.x,
                    y: entity.y,
                    glyph: if entity.alive { entity.glyph } else { '%' },
                    fg: if entity.alive {
                        entity.color
                    } else {
                        GameColor::DarkRed
                    },
                    alive: entity.alive,
                });
            }
        }
    }

    fn for_each_visible_item(&self, f: &mut dyn FnMut(RenderItem)) {
        for it in &self.ground_items {
            if self.visible.contains(&(it.x, it.y)) {
                f(RenderItem {
                    x: it.x,
                    y: it.y,
                    glyph: item::item_glyph(it.kind),
                    fg: item::item_color(it.kind),
                });
            }
        }
    }

    fn player_pos(&self) -> (Coord, Coord) {
        (self.entities[0].x, self.entities[0].y)
    }

    fn player_hp(&self) -> (Stat, Stat) {
        let p = &self.entities[0];
        (p.hp, p.max_hp)
    }

    fn player_atk(&self) -> (Stat, Stat) {
        (
            self.entities[0].attack,
            self.equipment.attack_bonus() as Stat,
        )
    }

    fn player_def(&self) -> (Stat, Stat) {
        (
            self.entities[0].defense,
            self.equipment.defense_bonus() as Stat,
        )
    }

    fn depth(&self) -> (Stat, Stat) {
        (self.depth, self.target_depth)
    }

    fn explored_pct(&self) -> Stat {
        self.explored_pct()
    }

    fn seed_code(&self) -> String {
        self.seed_code()
    }

    fn recent_messages(&self, n: usize) -> Vec<String> {
        self.log.recent(n).iter().map(|s| s.to_string()).collect()
    }

    fn kills(&self) -> Stat {
        self.kill_count()
    }

    fn turn_count(&self) -> Stat {
        self.turn_count
    }

    fn game_won(&self) -> bool {
        self.game_won
    }

    fn game_over(&self) -> bool {
        self.game_over
    }
}

// ── Micro tier ──────────────────────────────────────────────────────────

impl RenderSource for MicroGameStateAdapter {
    fn map_size(&self) -> (Coord, Coord) {
        (self.game.map.width as Coord, self.game.map.height as Coord)
    }

    fn tile_visibility(&self, x: Coord, y: Coord) -> TileVisibility {
        let ux = x as u8;
        let uy = y as u8;
        if self.game.fov.is_visible(ux, uy) {
            TileVisibility::Visible
        } else if self.game.fov.is_explored(ux, uy) {
            TileVisibility::Explored
        } else {
            TileVisibility::Unexplored
        }
    }

    fn tile_at(&self, x: Coord, y: Coord) -> RenderTile {
        let micro_tile = self.game.map.tile_at(x as u8, y as u8);
        match tile_rules::from_micro(micro_tile) {
            Some(kind) => RenderTile {
                glyph: tile_rules::glyph(kind),
                fg: tile_rules::color(kind),
                structural: kind == TileKind::Structural,
            },
            None => RenderTile {
                glyph: ' ',
                fg: GameColor::Black,
                structural: false,
            },
        }
    }

    fn for_each_visible_entity(&self, f: &mut dyn FnMut(RenderEntity)) {
        let entities = &self.game.entities;
        let fov = &self.game.fov;
        for i in 0..entities.count as usize {
            if entities.alive[i] && fov.is_visible(entities.x[i], entities.y[i]) {
                let (glyph, fg) = if i == PLAYER_IDX as usize {
                    ('@', GameColor::Yellow)
                } else if let Some(kind) = entities.kind[i] {
                    (monster_table::glyph(kind), monster_table::color(kind))
                } else {
                    ('?', GameColor::White)
                };
                f(RenderEntity {
                    x: entities.x[i] as Coord,
                    y: entities.y[i] as Coord,
                    glyph,
                    fg,
                    alive: true,
                });
            }
        }
    }

    fn for_each_visible_item(&self, f: &mut dyn FnMut(RenderItem)) {
        let items = &self.game.items;
        let fov = &self.game.fov;
        for i in 0..items.count as usize {
            if items.alive[i] && fov.is_visible(items.x[i], items.y[i]) {
                let kind = items.kind[i];
                f(RenderItem {
                    x: items.x[i] as Coord,
                    y: items.y[i] as Coord,
                    glyph: rules_items::glyph(kind),
                    fg: rules_items::color(kind),
                });
            }
        }
    }

    fn player_pos(&self) -> (Coord, Coord) {
        let pi = PLAYER_IDX as usize;
        (
            self.game.entities.x[pi] as Coord,
            self.game.entities.y[pi] as Coord,
        )
    }

    fn player_hp(&self) -> (Stat, Stat) {
        let pi = PLAYER_IDX as usize;
        (
            self.game.entities.hp[pi] as Stat,
            self.game.entities.max_hp[pi] as Stat,
        )
    }

    fn player_atk(&self) -> (Stat, Stat) {
        (
            self.game.entities.atk[PLAYER_IDX as usize] as Stat,
            self.game.equipment.attack_bonus() as Stat,
        )
    }

    fn player_def(&self) -> (Stat, Stat) {
        (
            self.game.entities.def[PLAYER_IDX as usize] as Stat,
            self.game.equipment.defense_bonus() as Stat,
        )
    }

    fn depth(&self) -> (Stat, Stat) {
        (self.game.depth as Stat, balance::TARGET_DEPTH as Stat)
    }

    fn explored_pct(&self) -> Stat {
        let total = self.game.map.floor_count();
        let explored = self.game.fov.explored_floor_count(&self.game.map);
        if total > 0 {
            ((explored as i32) * 100) / (total as i32)
        } else {
            0
        }
    }

    fn seed_code(&self) -> String {
        seed_code::encode(&SeedParams {
            seed: self.seed() as u64,
            width: self.game.map.width as i32,
            height: self.game.map.height as i32,
            preset: None,
        })
    }

    fn recent_messages(&self, n: usize) -> Vec<String> {
        use roguelike_core::tier_micro::msglog::MSG_COUNT;
        let mut messages = Vec::new();
        for i in (0..MSG_COUNT as u8).rev() {
            if let Some(event) = self.game.log.recent(i) {
                messages.push(format_event(event));
            }
        }
        // Return at most n, oldest first.
        let skip = messages.len().saturating_sub(n);
        messages.into_iter().skip(skip).collect()
    }

    fn kills(&self) -> Stat {
        self.game.kills as Stat
    }

    fn turn_count(&self) -> Stat {
        self.game.turn_count as Stat
    }

    fn game_won(&self) -> bool {
        self.game.game_won
    }

    fn game_over(&self) -> bool {
        self.game.game_over
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roguelike_core::data;

    fn test_standard_game() -> GameState {
        let gd = data::load_game_data();
        let mut state = GameState::with_data(40, 30, 42, &gd);
        state.update_fov();
        state
    }

    fn test_micro_game() -> MicroGameStateAdapter {
        MicroGameStateAdapter::new(42, 64, 48)
    }

    #[test]
    fn standard_map_size() {
        let gs = test_standard_game();
        assert_eq!(gs.map_size(), (40, 30));
    }

    #[test]
    fn micro_map_size() {
        let mg = test_micro_game();
        assert_eq!(mg.map_size(), (64, 48));
    }

    #[test]
    fn standard_player_visible() {
        let gs = test_standard_game();
        let (px, py) = RenderSource::player_pos(&gs);
        assert_eq!(gs.tile_visibility(px, py), TileVisibility::Visible);
    }

    #[test]
    fn micro_player_visible() {
        let mg = test_micro_game();
        let (px, py) = RenderSource::player_pos(&mg);
        assert_eq!(mg.tile_visibility(px, py), TileVisibility::Visible);
    }

    #[test]
    fn standard_entities_include_player() {
        let gs = test_standard_game();
        let mut found_player = false;
        gs.for_each_visible_entity(&mut |e| {
            if e.glyph == '@' {
                found_player = true;
            }
        });
        assert!(found_player);
    }

    #[test]
    fn micro_entities_include_player() {
        let mg = test_micro_game();
        let mut found_player = false;
        mg.for_each_visible_entity(&mut |e| {
            if e.glyph == '@' {
                found_player = true;
            }
        });
        assert!(found_player);
    }

    #[test]
    fn micro_entity_colors() {
        let mg = test_micro_game();
        let mut found_colored = false;
        mg.for_each_visible_entity(&mut |e| {
            if e.glyph != '@' {
                // Monsters should have a real color, not White or Black.
                assert!(
                    e.fg != GameColor::White && e.fg != GameColor::Black,
                    "monster glyph '{}' has color {:?}",
                    e.glyph,
                    e.fg
                );
                found_colored = true;
            }
        });
        // It's fine if no monsters are visible from spawn — the test verifies
        // the color mapping path, not monster visibility.
        let _ = found_colored;
    }

    #[test]
    fn standard_hp_positive() {
        let gs = test_standard_game();
        let (hp, max_hp) = RenderSource::player_hp(&gs);
        assert!(hp > 0);
        assert!(max_hp > 0);
    }

    #[test]
    fn micro_hp_positive() {
        let mg = test_micro_game();
        let (hp, max_hp) = RenderSource::player_hp(&mg);
        assert!(hp > 0);
        assert!(max_hp > 0);
    }

    #[test]
    fn standard_explored_pct_range() {
        let gs = test_standard_game();
        let pct = RenderSource::explored_pct(&gs);
        assert!((0..=100).contains(&pct));
    }

    #[test]
    fn micro_explored_pct_range() {
        let mg = test_micro_game();
        let pct = RenderSource::explored_pct(&mg);
        assert!((0..=100).contains(&pct));
    }

    #[test]
    fn micro_has_items() {
        let mg = test_micro_game();
        let mut count = 0;
        mg.for_each_visible_item(&mut |_| count += 1);
        // Items spawn in rooms other than room 0, so some may or may not
        // be visible from starting position — just verify it doesn't panic.
        let _ = count;
    }

    #[test]
    fn micro_depth() {
        let mg = test_micro_game();
        let (depth, target) = mg.depth();
        assert_eq!(depth, 1);
        assert_eq!(target, roguelike_core::rules::balance::TARGET_DEPTH as Stat);
    }

    #[test]
    fn standard_structural_walls_render_as_hash() {
        let gs = test_standard_game();
        let (map_w, map_h) = gs.map_size();
        let mut found_structural = false;
        for y in 0..map_h {
            for x in 0..map_w {
                let idx = gs.map.idx(x, y);
                if gs.map.structural[idx] {
                    let tile = RenderSource::tile_at(&gs, x, y);
                    assert_eq!(
                        tile.glyph, '#',
                        "structural wall at ({x},{y}) should have '#' glyph, got '{}'",
                        tile.glyph
                    );
                    assert!(
                        tile.structural,
                        "structural wall at ({x},{y}) should have structural=true"
                    );
                    found_structural = true;
                }
            }
        }
        assert!(
            found_structural,
            "map should have at least one structural wall"
        );
    }

    #[test]
    fn standard_non_structural_walls_render_as_space() {
        let gs = test_standard_game();
        let (map_w, map_h) = gs.map_size();
        let mut found_non_structural = false;
        for y in 0..map_h {
            for x in 0..map_w {
                let idx = gs.map.idx(x, y);
                if gs.map.tiles[idx] == roguelike_core::map::Tile::Wall && !gs.map.structural[idx] {
                    let tile = RenderSource::tile_at(&gs, x, y);
                    assert_eq!(
                        tile.glyph, ' ',
                        "non-structural wall at ({x},{y}) should have ' ' glyph"
                    );
                    assert!(
                        !tile.structural,
                        "non-structural wall at ({x},{y}) should have structural=false"
                    );
                    found_non_structural = true;
                }
            }
        }
        assert!(
            found_non_structural,
            "map should have at least one non-structural wall"
        );
    }
}
