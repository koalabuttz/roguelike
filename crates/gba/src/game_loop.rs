//! GBA game loop: state machine driving CompactGameState or MicroGameState.
//!
//! States: Playing → Looking → GameOver → (restart)
//! Game state lives in EWRAM via a union (CompactGameState or MicroGameState).

use core::mem::MaybeUninit;

use gba::prelude::*;

use roguelike_core::command::GameCommand;
use roguelike_core::rules::balance;
use roguelike_core::rules::color::GameColor;
use roguelike_core::rules::game_view::GameView;
use roguelike_core::rules::health::{self, HealthTier};
use roguelike_core::rules::items as rules_items;
use roguelike_core::rules::message::GameEvent;
use roguelike_core::rules::monster_table;
use roguelike_core::rules::seed_code;
use roguelike_core::tier_compact::game::CompactGameState;
use roguelike_core::tier_compact::types::{MAP_HEIGHT, MAP_WIDTH};
use roguelike_core::tier_micro::game::MicroGameState;
use roguelike_core::tier_micro::types::{
    DEFAULT_MAP_HEIGHT as MICRO_MAP_HEIGHT, DEFAULT_MAP_WIDTH as MICRO_MAP_WIDTH,
};

use crate::display;
use crate::input;
use crate::palette::PALBANK_HIGHLIGHT;
use crate::render;

// ---------------------------------------------------------------------------
// EWRAM game state — union holds either tier
// ---------------------------------------------------------------------------

use core::mem::ManuallyDrop;

#[repr(C)]
union GameSlot {
    compact: ManuallyDrop<MaybeUninit<CompactGameState>>,
    micro: ManuallyDrop<MaybeUninit<MicroGameState>>,
}

#[link_section = ".ewram"]
static mut GAME_SLOT: GameSlot = GameSlot {
    compact: ManuallyDrop::new(MaybeUninit::uninit()),
};

/// Which tier is currently active.
static mut IS_MICRO: bool = false;

pub(crate) fn is_micro() -> bool {
    unsafe { IS_MICRO }
}

/// # Safety
/// Only call before initializing the corresponding union variant.
pub(crate) unsafe fn set_micro(val: bool) {
    IS_MICRO = val;
}

pub(crate) fn game_compact() -> &'static mut CompactGameState {
    unsafe { &mut *(*GAME_SLOT.compact).as_mut_ptr() }
}

pub(crate) fn game_micro() -> &'static mut MicroGameState {
    unsafe { &mut *(*GAME_SLOT.micro).as_mut_ptr() }
}

// ---------------------------------------------------------------------------
// Game initialization
// ---------------------------------------------------------------------------

fn start_game_compact(seed: u32, width: i32, height: i32) {
    unsafe {
        IS_MICRO = false;
        CompactGameState::new_into((*GAME_SLOT.compact).as_mut_ptr(), seed, width, height);
    }
    crate::debug::debug_log!("Compact game: seed={}, map={}x{}", seed, width, height);
}

fn start_game_micro(seed: u16, width: u8, height: u8) {
    unsafe {
        IS_MICRO = true;
        MicroGameState::new_into((*GAME_SLOT.micro).as_mut_ptr(), seed, width, height);
    }
    crate::debug::debug_log!("Micro game: seed={}, map={}x{}", seed, width, height);
}

// ---------------------------------------------------------------------------
// Title screen integration
// ---------------------------------------------------------------------------

enum TitleResult {
    NewGameCompact(u32),
    NewGameMicro(u16, u8, u8),
    Continue,
}

fn run_title_screen() -> TitleResult {
    let has_save = crate::saves::has_save();
    loop {
        match crate::title_screen::run_title(has_save) {
            crate::title_screen::TitleAction::NewGame => {
                return TitleResult::NewGameCompact(read_timer_seed());
            }
            crate::title_screen::TitleAction::Seed(s) => {
                // Check if seed is in micro range
                if seed_code::tier_from_seed(s as u64) == seed_code::Tier::Micro {
                    return TitleResult::NewGameMicro(
                        s as u16,
                        MICRO_MAP_WIDTH,
                        MICRO_MAP_HEIGHT,
                    );
                }
                return TitleResult::NewGameCompact(s);
            }
            crate::title_screen::TitleAction::Continue => {
                return TitleResult::Continue;
            }
        }
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

/// Load a saved game from SRAM. Returns true on success.
fn load_game() -> bool {
    // Peek at the tier byte to know which variant to load into
    let tier_ok = crate::saves::load_dispatch();
    if !tier_ok {
        return false;
    }

    // Recompute FOV and add welcome message
    if is_micro() {
        let state = game_micro();
        let px = state.entities.x[0];
        let py = state.entities.y[0];
        state.fov.compute_fov(px, py, &state.map);
        state.log.add(GameEvent::Welcome);
    } else {
        let state = game_compact();
        let px = state.entities.x[0];
        let py = state.entities.y[0];
        state
            .fov
            .compute_fov(px, py, balance::FOV_RADIUS, &state.map);
        state.log.add(GameEvent::Welcome);
    }
    crate::debug::debug_log!("Game loaded from SRAM (micro={})", is_micro());
    true
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

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

enum AppState {
    Playing,
    Looking { cx: i32, cy: i32 },
    GameOver,
}

/// Main entry point — runs forever.
pub fn run() -> ! {
    // Load persistent settings from SRAM (survives death + restarts).
    crate::saves::load_settings();

    loop {
        match run_title_screen() {
            TitleResult::NewGameCompact(seed) => {
                start_game_compact(seed, MAP_WIDTH, MAP_HEIGHT);
            }
            TitleResult::NewGameMicro(seed, w, h) => {
                start_game_micro(seed, w, h);
            }
            TitleResult::Continue => {
                if !load_game() {
                    crate::saves::erase_save();
                    continue;
                }
            }
        }

        // Apply persistent settings to the active game state.
        crate::saves::apply_settings_to_game();

        // Render initial frame and run the play loop
        if is_micro() {
            render::render_game(game_micro());
            run_play_loop(game_micro());
        } else {
            render::render_game(game_compact());
            run_play_loop(game_compact());
        }
    }
}

// ---------------------------------------------------------------------------
// Pause flow — loops between pause menu and settings
// ---------------------------------------------------------------------------

enum PauseAction {
    Resume,
    SaveAndQuit,
    TitleScreen,
}

/// Run the pause menu, looping back when sub-menus (settings) return via B.
/// START from any sub-menu resumes gameplay directly.
fn run_pause_flow() -> PauseAction {
    loop {
        match crate::pause_menu::run_pause() {
            crate::pause_menu::PauseResult::Resume => return PauseAction::Resume,
            crate::pause_menu::PauseResult::Settings => {
                match crate::settings_menu::run_settings() {
                    crate::settings_menu::SettingsResult::Back => {} // loop to pause menu
                    crate::settings_menu::SettingsResult::Resume => return PauseAction::Resume,
                }
            }
            crate::pause_menu::PauseResult::SaveAndQuit => return PauseAction::SaveAndQuit,
            crate::pause_menu::PauseResult::TitleScreen => return PauseAction::TitleScreen,
        }
    }
}

// ---------------------------------------------------------------------------
// Play loop — generic over GameView
// ---------------------------------------------------------------------------

fn run_play_loop(state: &mut impl GameView) {
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

                // UI commands — don't step the game
                match cmd {
                    GameCommand::Look => {
                        let (cx, cy) = state.player_xy();
                        render_look_cursor(state, cx, cy);
                        render_look_description(state, cx, cy);
                        app_state = AppState::Looking { cx, cy };
                        continue;
                    }
                    GameCommand::Quit => {
                        match run_pause_flow() {
                            PauseAction::Resume => {
                                render::render_game(state);
                                continue;
                            }
                            PauseAction::SaveAndQuit => {
                                crate::saves::save_dispatch();
                                crate::debug::debug_log!("Game saved to SRAM (pause menu)");
                                return;
                            }
                            PauseAction::TitleScreen => {
                                return;
                            }
                        }
                    }
                    GameCommand::OpenInventory => {
                        crate::inventory_ui::run_inventory(state);
                        render::render_game(state);
                        continue;
                    }
                    _ => {}
                }

                let depth_before = state.depth();
                let result = state.step_view(cmd);

                if !result.action_taken {
                    continue;
                }

                // Auto-save after descending stairs (depth increased).
                if state.depth() > depth_before {
                    crate::saves::save_dispatch();
                    crate::debug::debug_log!("Auto-saved to SRAM (depth {})", state.depth());
                }

                render::render_game(state);

                if state.game_over() || state.game_won() {
                    app_state = AppState::GameOver;
                }
            }

            AppState::Looking {
                ref mut cx,
                ref mut cy,
            } => match input::read_look_input() {
                Some(input::LookCommand::Move(dir)) => {
                    let (dx, dy) = dir.to_offset();
                    let (mw, mh) = state.map_dims();
                    let nx = (*cx + dx).clamp(0, mw - 1);
                    let ny = (*cy + dy).clamp(0, mh - 1);
                    *cx = nx;
                    *cy = ny;

                    render::render_game(state);
                    render_look_cursor(state, nx, ny);
                    render_look_description(state, nx, ny);
                }
                Some(input::LookCommand::Close) => {
                    render::render_game(state);
                    app_state = AppState::Playing;
                }
                None => {}
            },

            AppState::GameOver => {
                crate::saves::erase_save();
                crate::debug::debug_log!("Save erased (game over)");

                show_game_over(state);
                wait_for_key();
                return;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Look mode helpers — generic over GameView
// ---------------------------------------------------------------------------

/// Show the look cursor on the map layer.
fn render_look_cursor(state: &impl GameView, wx: i32, wy: i32) {
    let (px, py) = state.player_xy();
    let (mw, mh) = state.map_dims();
    let vp_w = display::SCREEN_COLS as i32;
    let vp_h = display::MAP_ROWS as i32;
    let vx = (px - vp_w / 2).clamp(0, (mw - vp_w).max(0));
    let vy = (py - vp_h / 2).clamp(0, (mh - vp_h).max(0));

    let sx = wx - vx;
    let sy = wy - vy;
    if sx >= 0 && sx < vp_w && sy >= 0 && sy < vp_h {
        display::write_map_tile(sx as usize, sy as usize, b'X', PALBANK_HIGHLIGHT);
    }
}

/// Show a description of the tile at (cx, cy) on the HUD message row.
fn render_look_description(state: &impl GameView, cx: i32, cy: i32) {
    use roguelike_core::tier_compact::map::{TILE_FLOOR, TILE_STAIRS_DOWN, TILE_STRUCTURAL};

    let mut buf = [b' '; 30];
    let mut p = crate::format::write_str(&mut buf, 0, "[L] ");

    if !state.map_in_bounds(cx, cy) || !state.is_explored(cx, cy) {
        p = crate::format::write_str(&mut buf, p, "Unexplored");
    } else {
        let visible = state.is_visible(cx, cy);

        // Terrain
        let tile = state.tile_at(cx, cy);
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
            if let Some(eidx) = state.entity_at(cx, cy) {
                if p < 30 {
                    buf[p] = b' ';
                    p += 1;
                }
                if eidx == 0 {
                    p = crate::format::write_str(&mut buf, p, "Player");
                } else if let Some(kind) = state.entity_kind(eidx as usize) {
                    p = crate::format::write_str(&mut buf, p, monster_table::name(kind));
                    let (hp, max_hp) = state.entity_hp(eidx as usize);
                    let tier = health::health_tier(hp, max_hp);
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
            if let Some(iidx) = state.item_at(cx, cy) {
                if p < 30 {
                    buf[p] = b' ';
                    p += 1;
                }
                if p < 30 {
                    buf[p] = b'[';
                    p += 1;
                }
                p = crate::format::write_str(
                    &mut buf,
                    p,
                    rules_items::name(state.item_kind_at(iidx as usize)),
                );
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
        display::write_hud_string(
            0,
            display::MSG_ROW,
            s.trim_end(),
            crate::palette::PALBANK_MSG,
        );
    }
}

// ---------------------------------------------------------------------------
// Game over screen — generic over GameView
// ---------------------------------------------------------------------------

fn show_game_over(state: &impl GameView) {
    let msg = if state.game_won() {
        "You escaped!"
    } else {
        "You have been slain..."
    };
    display::write_map_centered(8, msg, GameColor::Red as u16);

    let mut buf = [b' '; 30];
    let mut p = 0;
    p = crate::format::write_str(&mut buf, p, "Depth:");
    p = crate::format::write_u16(&mut buf, p, state.depth() as u16);
    p = crate::format::write_str(&mut buf, p, " Kills:");
    p = crate::format::write_u16(&mut buf, p, state.kills() as u16);
    p = crate::format::write_str(&mut buf, p, " Turns:");
    let _ = crate::format::write_u16(&mut buf, p, state.turn_count());

    let stats = core::str::from_utf8(&buf).unwrap_or("");
    display::write_map_centered(10, stats.trim_end(), GameColor::Grey as u16);

    display::write_map_string(4, 13, "Press any key to continue", GameColor::DarkGrey as u16);
}
