//! Touch-to-GameCommand translation for Android.
//!
//! Phase 1 — minimal tap input:
//! - Tap adjacent tile → Move(direction)
//! - Tap player tile → Wait
//! - Tap distant tile → ignored (Phase 2 adds pathfind-to)
//! - Tap status/message area → ignored (Phase 2 adds button bar)

use roguelike_core::rules::command::GameCommand;
use roguelike_core::rules::direction::Direction;
use roguelike_core::rules::game_view::GameView;

use crate::render;

/// Translate a touch/click event into a GameCommand.
///
/// `touch_x`/`touch_y` are pixel coordinates within the window.
/// Returns `None` if the tap doesn't map to a valid command.
pub fn touch_to_command(
    touch_x: f64,
    touch_y: f64,
    window_w: u32,
    window_h: u32,
    state: &dyn GameView,
) -> Option<GameCommand> {
    // Convert pixel to viewport tile coordinates.
    let (tile_x, tile_y) = render::pixel_to_tile(touch_x, touch_y, window_w, window_h)?;

    // Convert viewport tile to world coordinates.
    let (vx, vy) = state.viewport_origin(render::VP_COLS as i32, render::VP_ROWS as i32);
    let world_x = vx + tile_x as i32;
    let world_y = vy + tile_y as i32;

    // Delta from player position.
    let (px, py) = state.player_xy();
    let dx = world_x - px;
    let dy = world_y - py;

    if dx == 0 && dy == 0 {
        // Tap on self → wait.
        return Some(GameCommand::Wait);
    }

    // Only handle adjacent taps (Chebyshev distance 1) in Phase 1.
    if dx.abs() <= 1 && dy.abs() <= 1 {
        Direction::from_offset(dx, dy).map(GameCommand::Move)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roguelike_core::rules::game_view::GameViewStep;
    use roguelike_core::rules::items::{Equipment, Inventory, ItemKind};
    use roguelike_core::rules::message::GameEvent;
    use roguelike_core::rules::monster_table::MonsterKind;

    /// Minimal mock for testing touch input translation.
    struct MockView {
        player_x: i32,
        player_y: i32,
        equipment: Equipment,
        inventory: Inventory,
    }

    impl MockView {
        fn at(x: i32, y: i32) -> Self {
            Self {
                player_x: x,
                player_y: y,
                equipment: Equipment::default(),
                inventory: Inventory::default(),
            }
        }
    }

    impl GameView for MockView {
        fn map_dims(&self) -> (i32, i32) {
            (80, 40)
        }
        fn map_in_bounds(&self, _x: i32, _y: i32) -> bool {
            true
        }
        fn tile_at(&self, _x: i32, _y: i32) -> u8 {
            2
        }
        fn is_visible(&self, _x: i32, _y: i32) -> bool {
            true
        }
        fn is_explored(&self, _x: i32, _y: i32) -> bool {
            true
        }
        fn player_xy(&self) -> (i32, i32) {
            (self.player_x, self.player_y)
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
            1
        }
        fn entity_xy(&self, _i: usize) -> (i32, i32) {
            (self.player_x, self.player_y)
        }
        fn entity_alive(&self, _i: usize) -> bool {
            true
        }
        fn entity_kind(&self, _i: usize) -> Option<MonsterKind> {
            None
        }
        fn entity_hp(&self, _i: usize) -> (u8, u8) {
            (10, 10)
        }
        fn entity_at(&self, _x: i32, _y: i32) -> Option<u8> {
            None
        }
        fn item_count(&self) -> usize {
            0
        }
        fn item_xy(&self, _i: usize) -> (i32, i32) {
            (0, 0)
        }
        fn item_alive(&self, _i: usize) -> bool {
            false
        }
        fn item_kind_at(&self, _i: usize) -> ItemKind {
            ItemKind::HealthPotion
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
            false
        }
        fn game_won(&self) -> bool {
            false
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

    // Window 800x440 → cell_w=20, cell_h=20 (440/22=20).
    // Player at (20, 10): viewport_origin(40, 20) = (0, 0) since
    // 20-20=0, 10-10=0 (clamped to 0).
    const W: u32 = 800;
    const H: u32 = 440;

    #[test]
    fn tap_on_player_is_wait() {
        let view = MockView::at(20, 10);
        // Player at (20,10), viewport at (0,0). Pixel for tile (20,10) = (400, 200).
        let cmd = touch_to_command(400.0, 200.0, W, H, &view);
        assert_eq!(cmd, Some(GameCommand::Wait));
    }

    #[test]
    fn tap_adjacent_east_is_move() {
        let view = MockView::at(20, 10);
        // Tile (21,10) = pixel (420, 200).
        let cmd = touch_to_command(420.0, 200.0, W, H, &view);
        assert_eq!(cmd, Some(GameCommand::Move(Direction::East)));
    }

    #[test]
    fn tap_adjacent_northwest_is_move() {
        let view = MockView::at(20, 10);
        // Tile (19,9) = pixel (380, 180).
        let cmd = touch_to_command(380.0, 180.0, W, H, &view);
        assert_eq!(cmd, Some(GameCommand::Move(Direction::NorthWest)));
    }

    #[test]
    fn tap_distant_is_none() {
        let view = MockView::at(20, 10);
        // Tile (25,10) = pixel (500, 200), dx=5 > 1.
        let cmd = touch_to_command(500.0, 200.0, W, H, &view);
        assert_eq!(cmd, None);
    }

    #[test]
    fn tap_status_bar_is_none() {
        let view = MockView::at(20, 10);
        // Status bar is at y >= VP_ROWS * cell_h = 400.
        let cmd = touch_to_command(100.0, 410.0, W, H, &view);
        assert_eq!(cmd, None);
    }
}
