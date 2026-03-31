//! GBA game loop: state machine driving CompactGameState.
//!
//! States: Playing → Looking → GameOver → (restart)
//! Game state lives in EWRAM via static MaybeUninit.

use core::mem::MaybeUninit;

use gba::prelude::*;

use roguelike_core::command::GameCommand;
use roguelike_core::rules::balance;
use roguelike_core::rules::color::GameColor;
use roguelike_core::rules::health::{self, HealthTier};
use roguelike_core::rules::items as rules_items;
use roguelike_core::rules::message::GameEvent;
use roguelike_core::rules::monster_table;
use roguelike_core::tier_compact::game::CompactGameState;
use roguelike_core::tier_compact::map::{TILE_FLOOR, TILE_STAIRS_DOWN, TILE_STRUCTURAL};
use roguelike_core::tier_compact::types::{MAP_HEIGHT, MAP_WIDTH, NO_ENTITY, NO_ITEM, PLAYER_IDX};

use crate::display;
use crate::input;
use crate::palette::PALBANK_HIGHLIGHT;
use crate::render;

/// Game state in EWRAM (too large for IWRAM stack).
#[link_section = ".ewram"]
static mut GAME: MaybeUninit<CompactGameState> = MaybeUninit::uninit();

enum AppState {
    Playing,
    Looking { cx: i32, cy: i32 },
    GameOver,
}

/// Start a new game directly in EWRAM, avoiding large stack allocation.
fn start_game(seed: u32) {
    let ptr = unsafe { (&raw mut GAME).cast::<CompactGameState>() };
    unsafe { CompactGameState::new_into(ptr, seed, MAP_WIDTH, MAP_HEIGHT) };
    crate::debug::debug_log!("Game initialized: seed={}, map={}x{}", seed, MAP_WIDTH, MAP_HEIGHT);
}

/// Get a reference to the active game state.
/// Safety: must only be called after start_game().
fn game() -> &'static mut CompactGameState {
    unsafe { &mut *(&raw mut GAME).cast::<CompactGameState>() }
}

/// What the title screen resolved to.
enum TitleResult {
    /// Start a fresh game with this seed.
    NewGame(u32),
    /// Continue from SRAM save.
    Continue,
}

/// Run the title screen and return the chosen action.
fn run_title_screen() -> TitleResult {
    let has_save = crate::saves::has_save();
    loop {
        match crate::title_screen::run_title(has_save) {
            crate::title_screen::TitleAction::NewGame => return TitleResult::NewGame(read_timer_seed()),
            crate::title_screen::TitleAction::Seed(s) => return TitleResult::NewGame(s),
            crate::title_screen::TitleAction::Continue => return TitleResult::Continue,
        }
    }
}

/// Load a saved game from SRAM into the EWRAM game state.
/// Returns true on success.
fn load_game() -> bool {
    let state = game();
    if crate::saves::load_from_sram(state).is_ok() {
        // Recompute FOV from player position (visible bitfield not saved).
        let px = state.entities.x[PLAYER_IDX as usize];
        let py = state.entities.y[PLAYER_IDX as usize];
        state.fov.compute_fov(px, py, balance::FOV_RADIUS, &state.map);
        state.log.add(GameEvent::Welcome);
        true
    } else {
        false
    }
}

/// Read the GBA's free-running timer as a seed source.
fn read_timer_seed() -> u32 {
    TIMER0_RELOAD.write(0);
    TIMER0_CONTROL.write(TimerControl::new().with_enabled(true));

    let lo = TIMER0_COUNT.read() as u32;
    display::vblank_wait();
    let hi = TIMER0_COUNT.read() as u32;

    // Combine into a non-zero 32-bit seed (LFSR requires non-zero)
    ((hi << 16) | lo) | 1
}

/// Wait until any key is newly pressed (rising edge detection).
fn wait_for_key() {
    let mut prev: u16 = 0;
    loop {
        display::vblank_wait();
        let pressed = !KEYINPUT.read().to_u16() & 0x03FF;
        let edges = pressed & !prev;
        prev = pressed;
        if edges != 0 {
            break;
        }
    }
}

/// Main entry point — runs forever.
pub fn run() -> ! {
    loop {
        match run_title_screen() {
            TitleResult::NewGame(seed) => {
                start_game(seed);
            }
            TitleResult::Continue => {
                // load_game writes into the EWRAM static directly.
                if !load_game() {
                    // Save was corrupt — fall back to new game.
                    crate::saves::erase_save();
                    continue; // Re-show title (now without Continue)
                }
                crate::debug::debug_log!("Game loaded from SRAM");
            }
        }

        render::render_game(game());
        run_play_loop();
    }
}

/// The inner play loop — returns when the player quits or game ends.
fn run_play_loop() {
    let mut app_state = AppState::Playing;

    loop {
        display::vblank_wait();

        #[cfg(feature = "dev")]
        if !crate::stack_check::check_canary() {
            crate::display::write_map_string(0, 0, "STACK OVERFLOW", 4);
            crate::debug::debug_log_fatal!("Stack canary corrupted — overflow detected");
            loop {}
        }

        match app_state {
            AppState::Playing => {
                let cmd = match input::read_game_input() {
                    Some(c) => c,
                    None => continue,
                };

                let state = game();

                // UI commands — don't step the game
                match cmd {
                    GameCommand::Look => {
                        let cx = state.entities.x[PLAYER_IDX as usize];
                        let cy = state.entities.y[PLAYER_IDX as usize];
                        render_look_cursor(state, cx, cy);
                        render_look_description(state, cx, cy);
                        app_state = AppState::Looking { cx, cy };
                        continue;
                    }
                    GameCommand::Quit => {
                        // Save before returning to title.
                        crate::saves::save_to_sram(state);
                        crate::debug::debug_log!("Game saved to SRAM (quit)");
                        return;
                    }
                    GameCommand::OpenInventory => {
                        crate::inventory_ui::run_inventory(game());
                        render::render_game(game());
                        continue;
                    }
                    _ => {}
                }

                let depth_before = state.depth;
                let mut result = state.step(cmd);

                // A button sends Pickup; if nothing to pick up, try Descend.
                if !result.action_taken && matches!(cmd, GameCommand::Pickup) {
                    result = state.step(GameCommand::Descend);
                }

                if !result.action_taken {
                    continue;
                }

                // Auto-save after descending stairs (depth increased).
                if state.depth > depth_before {
                    crate::saves::save_to_sram(state);
                    crate::debug::debug_log!("Auto-saved to SRAM (depth {})", state.depth);
                }

                render::render_game(state);

                if state.game_over || state.game_won {
                    app_state = AppState::GameOver;
                }
            }

            AppState::Looking { ref mut cx, ref mut cy } => {
                match input::read_look_input() {
                    Some(input::LookCommand::Move(dir)) => {
                        let (dx, dy) = dir.to_offset();
                        let state = game();
                        let nx = (*cx + dx).clamp(0, state.map.width - 1);
                        let ny = (*cy + dy).clamp(0, state.map.height - 1);
                        *cx = nx;
                        *cy = ny;

                        render::render_game(state);
                        render_look_cursor(state, nx, ny);
                        render_look_description(state, nx, ny);
                    }
                    Some(input::LookCommand::Close) => {
                        render::render_game(game());
                        app_state = AppState::Playing;
                    }
                    None => {}
                }
            }

            AppState::GameOver => {
                // Erase save — permadeath, no reloading after death.
                crate::saves::erase_save();
                crate::debug::debug_log!("Save erased (game over)");

                show_game_over(game());
                wait_for_key();
                return;
            }
        }
    }
}

/// Show the look cursor on the map layer.
fn render_look_cursor(state: &CompactGameState, wx: i32, wy: i32) {
    let px = state.entities.x[PLAYER_IDX as usize];
    let py = state.entities.y[PLAYER_IDX as usize];
    let vp_w = display::SCREEN_COLS as i32;
    let vp_h = display::MAP_ROWS as i32;
    let vx = (px - vp_w / 2).clamp(0, (state.map.width - vp_w).max(0));
    let vy = (py - vp_h / 2).clamp(0, (state.map.height - vp_h).max(0));

    let sx = wx - vx;
    let sy = wy - vy;
    if sx >= 0 && sx < vp_w && sy >= 0 && sy < vp_h {
        display::write_map_tile(sx as usize, sy as usize, b'X', PALBANK_HIGHLIGHT);
    }
}

/// Show a description of the tile at (cx, cy) on the HUD message row.
fn render_look_description(state: &CompactGameState, cx: i32, cy: i32) {
    let mut buf = [b' '; 30];
    let mut p = crate::format::write_str(&mut buf, 0, "[L] ");

    if !state.map.in_bounds(cx, cy) || !state.fov.is_explored(cx, cy) {
        p = crate::format::write_str(&mut buf, p, "Unexplored");
    } else {
        let visible = state.fov.is_visible(cx, cy);

        // Terrain
        let tile = state.map.tile_at(cx, cy);
        p = match tile {
            TILE_FLOOR => crate::format::write_str(&mut buf, p, "Floor"),
            TILE_STAIRS_DOWN => crate::format::write_str(&mut buf, p, "Stairs down"),
            TILE_STRUCTURAL => crate::format::write_str(&mut buf, p, "Wall"),
            _ => crate::format::write_str(&mut buf, p, "Wall"),
        };

        if !visible {
            p = crate::format::write_str(&mut buf, p, " (remembered)");
        } else {
            // Entity on tile
            let eidx = state.entities.entity_at(cx, cy);
            if eidx != NO_ENTITY {
                if p < 30 {
                    buf[p] = b' ';
                    p += 1;
                }
                if eidx == PLAYER_IDX {
                    p = crate::format::write_str(&mut buf, p, "Player");
                } else if let Some(kind) = state.entities.kind[eidx as usize] {
                    p = crate::format::write_str(&mut buf, p, monster_table::name(kind));
                    let tier = health::health_tier(
                        state.entities.hp[eidx as usize],
                        state.entities.max_hp[eidx as usize],
                    );
                    let desc = match tier {
                        HealthTier::Healthy => "",
                        HealthTier::Moderate => " (damaged)",
                        HealthTier::Severe => " (wounded)",
                        HealthTier::AlmostDead => " (dying)",
                    };
                    p = crate::format::write_str(&mut buf, p, desc);
                }
            }

            // Item on tile (show first one)
            let iidx = state.items.item_at(cx, cy);
            if iidx != NO_ITEM {
                if p < 30 {
                    buf[p] = b' ';
                    p += 1;
                }
                if p < 30 {
                    buf[p] = b'[';
                    p += 1;
                }
                p = crate::format::write_str(&mut buf, p, rules_items::name(state.items.kind[iidx as usize]));
                if p < 30 {
                    buf[p] = b']';
                    p += 1;
                }
            }
        }
    }
    let _ = p;

    // Clear message row and write description
    display::clear_hud_row(display::MSG_ROW);
    if let Ok(s) = core::str::from_utf8(&buf) {
        display::write_hud_string(0, display::MSG_ROW, s.trim_end(), crate::palette::PALBANK_MSG);
    }
}

/// Show game over screen.
fn show_game_over(state: &CompactGameState) {
    let msg = if state.game_won {
        "You escaped!"
    } else {
        "You have been slain..."
    };
    display::write_map_centered(8, msg, GameColor::Red as u16);

    let mut buf = [b' '; 30];
    let mut p = 0;
    p = crate::format::write_str(&mut buf, p, "Depth:");
    p = crate::format::write_u16(&mut buf, p, state.depth as u16);
    p = crate::format::write_str(&mut buf, p, " Kills:");
    p = crate::format::write_u16(&mut buf, p, state.kills as u16);
    p = crate::format::write_str(&mut buf, p, " Turns:");
    let _ = crate::format::write_u16(&mut buf, p, state.turn_count);

    let stats = core::str::from_utf8(&buf).unwrap_or("");
    display::write_map_centered(10, stats.trim_end(), GameColor::Grey as u16);

    display::write_map_string(4, 13, "Press any key to continue", GameColor::DarkGrey as u16);
}
