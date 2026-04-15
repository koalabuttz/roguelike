//! PS Vita frontend entry point.
//!
//! Phase 1: vita2d initialized, static dungeon renders using colored
//! rectangles. No input yet (Phase 2). No saves (Phase 3).
//!
//! The game uses a fixed compact-tier seed (100000) which routes through
//! `create_game` to `CompactGameStateAdapter` (80×40 map, i32 coords).
//! Phase 2 will add seed selection via the title screen and analog input.

mod render;
mod vita2d;

use roguelike_core::data::GameData;
use roguelike_core::game_step::create_game;
use roguelike_core::rules::balance::{COMPACT_MAP_HEIGHT, COMPACT_MAP_WIDTH};

fn main() {
    // Initialize vita2d. Keeps SceGxm context alive for the process lifetime.
    let vita = vita2d::Vita2d::init();
    vita.set_clear_color(vita2d::BLACK);

    // Create a compact-tier game (seed 100000 > 0xFFFF → compact tier).
    // GameData::defaults() uses compiled-in balance constants (no game.toml
    // needed — data-files feature is disabled for this crate).
    let game_data = GameData::defaults();
    let mut game = create_game(
        100_000,
        COMPACT_MAP_WIDTH as i32,
        COMPACT_MAP_HEIGHT as i32,
        None,
        game_data,
    )
    .expect("failed to create game");

    // Render loop — static for Phase 1 (no input).
    // Phase 2 will poll SceCtrl here and call game.step_view(cmd).
    loop {
        vita.start_frame();
        vita.clear();
        render::render_frame(&vita, game.as_mut());
        vita.end_frame();
    }
}
