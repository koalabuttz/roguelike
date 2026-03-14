// Roguelike Dungeon Crawler — Commodore 64 Edition
//
// Thin C64 frontend over roguelike-core::tier_micro. All game logic
// (map gen, FOV, entities, combat, AI, spawning, messages) comes from
// the shared core crate. This file handles hardware init, seed reading,
// and the main loop state machine.

#![no_std]
#![no_main]
#![allow(static_mut_refs)] // Single-threaded bare metal — static mut is safe

mod c64;
mod render;
mod input;

use core::mem::MaybeUninit;
use core::panic::PanicInfo;
use input::{InventoryInput, LookInput, MenuInput};
use roguelike_core::command::{Direction, GameCommand};
use roguelike_core::rules::message::{Combatant, GameEvent};
use roguelike_core::rules::{balance, seed_code};
use roguelike_core::tier_micro::autorun::{MicroAutorunStop, MicroAutorunStepper, MicroStepOutcome};
use roguelike_core::tier_micro::game::MicroGameState;
use roguelike_core::tier_micro::map::TILE_STAIRS_DOWN;
use roguelike_core::tier_micro::types::{DEFAULT_MAP_HEIGHT, DEFAULT_MAP_WIDTH, PLAYER_IDX};

/// Panic handler — flash the border red (classic C64 crash indicator).
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        c64::poke(c64::VIC_BORDER, c64::COLOR_RED);
    }
}

/// Game state stored in a static — too large for the 6502 hardware stack
/// (256 bytes) but fine in main RAM. MaybeUninit avoids requiring Default.
/// Explicit link_section keeps it in main ram so the linker can overflow
/// smaller compiler-generated statics to the freed KERNAL region.
#[link_section = ".noinit.state"]
static mut STATE: MaybeUninit<MicroGameState> = MaybeUninit::uninit();

/// Previous-frame snapshot for differential rendering (~810 bytes).
/// Placed in freed KERNAL region alongside STATE.
#[link_section = ".noinit.state"]
static mut DIFF: render::DiffState = render::DiffState::new();

/// Application states for the main loop.
enum AppState {
    Title,
    Playing,
    Looking,
    Inventory,
    Help,
    MessageHistory,
    Paused,
    GameOver,
}

/// Read a 16-bit seed from the CIA1 timer (human timing jitter = entropy).
fn read_cia_seed() -> u16 {
    let lo = c64::peek(c64::CIA1_TIMER_LO) as u16;
    let hi = c64::peek(unsafe { c64::CIA1_TIMER_LO.add(1) }) as u16;
    lo | (hi << 8)
}

/// Start a new game with the given seed and dimensions.
/// Writes to the global STATE and returns a mutable reference.
fn start_game(seed: u16, width: u8, height: u8) -> &'static mut MicroGameState {
    c64::io_bank_out();
    unsafe {
        STATE.write(MicroGameState::new(seed, width, height));
    }
    c64::io_bank_in();
    unsafe { STATE.assume_init_mut() }
}

/// Run the title menu. Returns the seed and dimensions for the new game.
fn run_title() -> (u16, u8, u8) {
    let mut selected: u8 = 0;
    render::render_title(selected);

    loop {
        match input::wait_for_menu_input() {
            MenuInput::Up => {
                if selected > 0 {
                    selected -= 1;
                    render::render_title(selected);
                }
            }
            MenuInput::Down => {
                if selected < 1 {
                    selected += 1;
                    render::render_title(selected);
                }
            }
            MenuInput::Select => {
                match selected {
                    0 => {
                        // New Game — random seed, default dims
                        return (read_cia_seed(), DEFAULT_MAP_WIDTH, DEFAULT_MAP_HEIGHT);
                    }
                    1 => {
                        // Enter Seed
                        if let Some((seed, w, h)) = run_seed_input() {
                            return (seed, w, h);
                        }
                        // Cancelled — redraw title
                        render::render_title(selected);
                    }
                    _ => {}
                }
            }
            MenuInput::Back => {
                // No back action on title — nowhere to go
            }
            MenuInput::Left | MenuInput::Right => {}
        }
    }
}

/// Run the seed input dialog. Returns decoded params or None if cancelled.
fn run_seed_input() -> Option<(u16, u8, u8)> {
    let mut buf = [0u8; 12];

    render::render_seed_input(&[], 0);

    let len = input::read_seed_input(&mut buf, |typed, len| {
        render::render_seed_input(typed, len);
    })?;

    // Try to decode the seed code
    match seed_code::decode_micro_from_bytes(&buf[..len as usize]) {
        Ok(params) => Some((params.seed, params.width, params.height)),
        Err(_) => {
            render::render_seed_error();
            input::wait_for_menu_input(); // wait for any key
            None
        }
    }
}

/// Run the pause menu. Returns the next AppState.
fn run_pause(state: &MicroGameState) -> AppState {
    let mut selected: u8 = 0;
    render::render_pause(state, selected);

    loop {
        match input::wait_for_menu_input() {
            MenuInput::Up => {
                if selected > 0 {
                    selected -= 1;
                    render::render_pause(state, selected);
                }
            }
            MenuInput::Down => {
                if selected < 1 {
                    selected += 1;
                    render::render_pause(state, selected);
                }
            }
            MenuInput::Select => {
                return match selected {
                    0 => AppState::Playing, // Resume
                    _ => AppState::Title,   // New Game → go to title
                };
            }
            MenuInput::Back => {
                return AppState::Playing; // Back = Resume
            }
            MenuInput::Left | MenuInput::Right => {}
        }
    }
}

/// Run the end-of-game screen (death or victory). Returns the next AppState.
fn run_end_screen(state: &MicroGameState) -> AppState {
    let mut selected: u8 = 0;
    let render = |s: &MicroGameState, sel: u8| {
        if s.game_won {
            render::render_victory(s, sel);
        } else {
            render::render_game_over(s, sel);
        }
    };
    render(state, selected);

    loop {
        match input::wait_for_menu_input() {
            MenuInput::Up => {
                if selected > 0 {
                    selected -= 1;
                    render(state, selected);
                }
            }
            MenuInput::Down => {
                if selected < 1 {
                    selected += 1;
                    render(state, selected);
                }
            }
            MenuInput::Select => {
                return match selected {
                    0 => AppState::Playing,  // Play Again
                    _ => AppState::Title,    // Title Screen
                };
            }
            MenuInput::Back => {} // No back from end screen
            MenuInput::Left | MenuInput::Right => {}
        }
    }
}

/// Combat events detected this turn, used to select screen shake + SFX.
struct CombatInfo {
    /// Player attacked or killed a monster.
    player_attacked: bool,
    /// Player was hit by a monster (or took damage).
    player_hurt: bool,
}

/// Scan events added this turn for combat involving the player.
/// `old_total` is the log total from before `step()`.
fn detect_combat(state: &MicroGameState, old_total: u16) -> CombatInfo {
    let new_events = state.log.total().wrapping_sub(old_total);
    let limit = if new_events > 8 { 8 } else { new_events as u8 };
    let mut info = CombatInfo { player_attacked: false, player_hurt: false };
    let mut i: u8 = 0;
    while i < limit {
        match state.log.recent(i) {
            Some(GameEvent::Attack { attacker: Combatant::Player, .. })
            | Some(GameEvent::Kill { attacker: Combatant::Player, .. })
            | Some(GameEvent::NoDamage { attacker: Combatant::Player, .. }) => {
                info.player_attacked = true;
            }
            Some(GameEvent::Attack { defender: Combatant::Player, .. })
            | Some(GameEvent::NoDamage { defender: Combatant::Player, .. })
            | Some(GameEvent::Kill { victim: Combatant::Player, .. }) => {
                info.player_hurt = true;
            }
            _ => {}
        }
        i += 1;
    }
    info
}

/// (dx, dy) offsets indexed by Direction discriminant. i8 for sign,
/// applied via u8 checked arithmetic — no widening to i32.
const DIR_OFFSETS: [(i8, i8); 8] = [
    ( 0, -1), // North     = 0
    ( 0,  1), // South     = 1
    ( 1,  0), // East      = 2
    (-1,  0), // West      = 3
    ( 1, -1), // NorthEast = 4
    (-1, -1), // NorthWest = 5
    ( 1,  1), // SouthEast = 6
    (-1,  1), // SouthWest = 7
];

/// Apply a signed offset to a u8 coordinate, clamping to [0, max).
fn apply_offset(pos: u8, delta: i8, max: u8) -> u8 {
    if delta > 0 {
        if pos + 1 < max { pos + 1 } else { pos }
    } else if delta < 0 {
        if pos > 0 { pos - 1 } else { pos }
    } else {
        pos
    }
}

/// Run look mode: move a cursor around the map to examine tiles.
/// Viewport follows the cursor. Does not consume game turns.
/// Uses differential rendering — only redraws the old/new cursor tiles
/// and the status bar, unless the viewport scrolls.
fn run_look_mode(state: &MicroGameState) {
    let pi = PLAYER_IDX as usize;
    let mut cx = state.entities.x[pi];
    let mut cy = state.entities.y[pi];

    // Initial full render
    let mut vx: u8;
    let mut vy: u8;
    (vx, vy) = render::look_viewport(state, cx, cy);
    render::render_look(state, vx, vy, cx, cy);

    loop {
        match input::wait_for_look_input() {
            LookInput::Move(dir) => {
                let (dx, dy) = DIR_OFFSETS[dir as usize];
                let nx = apply_offset(cx, dx, state.map.width);
                let ny = apply_offset(cy, dy, state.map.height);
                if nx == cx && ny == cy {
                    continue;
                }

                let (nvx, nvy) = render::look_viewport(state, nx, ny);
                if nvx != vx || nvy != vy {
                    // Viewport scrolled — full redraw
                    vx = nvx;
                    vy = nvy;
                    cx = nx;
                    cy = ny;
                    render::render_look(state, vx, vy, cx, cy);
                } else {
                    // Same viewport — differential update
                    render::restore_tile(state, vx, vy, cx, cy);
                    cx = nx;
                    cy = ny;
                    render::draw_cursor(vx, vy, cx, cy);
                    render::render_look_status(state, cx, cy);
                }
            }
            LookInput::Close => return,
        }
    }
}

/// Run the inventory modal. Two-phase: Browse (cursor) + Act (action bar).
/// Keyboard shortcuts U/E/D work directly in Browse mode.
/// Joystick: fire enters Act mode, left/right cycles actions, fire confirms.
fn run_inventory(state: &mut MicroGameState) {
    use render::InvAction;

    let mut selected: u8 = 0;
    // None = Browse mode, Some(idx) = Act mode with action bar selection
    let mut action_sel: Option<u8> = None;

    render::render_inventory(state, selected, None);

    loop {
        let ec = render::equip_count(state);
        let total = ec + state.inventory.len() as u8;
        let inp = input::wait_for_inventory_input();

        match inp {
            InventoryInput::Close => return,

            // --- Browse mode: cursor movement ---
            InventoryInput::Up => {
                if action_sel.is_some() {
                    // In act mode, up exits back to browse
                    action_sel = None;
                } else if selected > 0 {
                    selected -= 1;
                }
            }
            InventoryInput::Down => {
                if action_sel.is_some() {
                    action_sel = None;
                } else if total > 0 && selected < total - 1 {
                    selected += 1;
                }
            }

            // --- Direct keyboard shortcuts (work on inventory items) ---
            InventoryInput::Use => {
                if selected >= ec {
                    execute_inventory_action(state, selected - ec, InvAction::Use);
                    action_sel = None;
                    clamp_selected(state, &mut selected);
                }
            }
            InventoryInput::Equip => {
                if selected < ec {
                    // Selected item is equipped — unequip it
                    execute_equip_action(state, selected);
                    action_sel = None;
                    clamp_selected(state, &mut selected);
                } else {
                    execute_inventory_action(state, selected - ec, InvAction::Equip);
                    action_sel = None;
                    clamp_selected(state, &mut selected);
                }
            }
            InventoryInput::Drop => {
                if selected < ec {
                    drop_equipped(state, selected);
                } else {
                    execute_inventory_action(state, selected - ec, InvAction::Drop);
                }
                action_sel = None;
                clamp_selected(state, &mut selected);
            }

            // --- Action bar navigation ---
            InventoryInput::Left => {
                if let Some(ref mut sel) = action_sel {
                    if *sel > 0 {
                        *sel -= 1;
                    }
                }
            }
            InventoryInput::Right => {
                if let Some(ref mut sel) = action_sel {
                    let actions = current_actions(state, selected);
                    let max = actions.len() as u8;
                    if *sel + 1 < max {
                        *sel += 1;
                    }
                }
            }

            // --- Confirm: enter Act mode or execute selected action ---
            InventoryInput::Confirm => {
                if total == 0 {
                    continue;
                }
                match action_sel {
                    None => {
                        // Enter Act mode with default action (index 0)
                        action_sel = Some(0);
                    }
                    Some(sel) => {
                        let actions = current_actions(state, selected);
                        if (sel as usize) < actions.len() {
                            let action = actions[sel as usize];
                            if action == InvAction::Back {
                                action_sel = None;
                            } else if selected < ec {
                                match action {
                                    InvAction::Unequip => execute_equip_action(state, selected),
                                    InvAction::Drop => drop_equipped(state, selected),
                                    _ => {}
                                }
                                action_sel = None;
                                clamp_selected(state, &mut selected);
                            } else {
                                execute_inventory_action(state, selected - ec, action);
                                action_sel = None;
                                clamp_selected(state, &mut selected);
                            }
                        }
                    }
                }
            }
        }

        // If inventory is empty after an action, exit
        if state.inventory.len() == 0 && state.equipment.weapon.is_none() && state.equipment.armor.is_none() {
            render::render_inventory(state, selected, None);
            return;
        }

        // Build action bar for rendering
        let bar = action_sel.map(|sel| {
            let actions = current_actions(state, selected);
            (actions, sel)
        });
        render::render_inventory(state, selected, bar);
    }
}

/// Get the context-sensitive action list for the currently selected item.
fn current_actions(state: &MicroGameState, selected: u8) -> &'static [render::InvAction] {
    let ec = render::equip_count(state);
    if selected < ec {
        render::actions_for_equipped()
    } else {
        state
            .inventory
            .iter()
            .nth((selected - ec) as usize)
            .map(|(_, slot)| render::actions_for_kind(slot.kind))
            .unwrap_or(&[render::InvAction::Back])
    }
}

/// Execute an unequip action on the selected equipped item.
fn execute_equip_action(state: &mut MicroGameState, equip_sel: u8) {
    // equip_sel 0 = weapon (if present), else armor.
    // equip_sel 1 = armor (when weapon is also present).
    let cmd = if state.equipment.weapon.is_some() && equip_sel == 0 {
        GameCommand::UnequipWeapon
    } else {
        GameCommand::UnequipArmor
    };
    c64::io_bank_out();
    state.step(cmd);
    c64::io_bank_in();
}

/// Drop an equipped item directly to the ground (bypasses inventory).
fn drop_equipped(state: &mut MicroGameState, equip_sel: u8) {
    let cmd = if state.equipment.weapon.is_some() && equip_sel == 0 {
        GameCommand::DropEquippedWeapon
    } else {
        GameCommand::DropEquippedArmor
    };
    c64::io_bank_out();
    state.step(cmd);
    c64::io_bank_in();
}

/// Execute an inventory action on a bag item (selected relative to inventory, not equip).
fn execute_inventory_action(
    state: &mut MicroGameState,
    inv_selected: u8,
    action: render::InvAction,
) {
    let slot_idx = state
        .inventory
        .iter()
        .nth(inv_selected as usize)
        .map(|(i, _)| i as u8);
    if let Some(idx) = slot_idx {
        let cmd = match action {
            render::InvAction::Use => GameCommand::UseItem(idx),
            render::InvAction::Equip => GameCommand::EquipItem(idx),
            render::InvAction::Drop => GameCommand::DropItem(idx),
            render::InvAction::Unequip | render::InvAction::Back => return,
        };
        c64::io_bank_out();
        state.step(cmd);
        c64::io_bank_in();
    }
}

/// Clamp the cursor position after inventory changes.
fn clamp_selected(state: &MicroGameState, selected: &mut u8) {
    let total = render::equip_count(state) + state.inventory.len() as u8;
    if total > 0 && *selected >= total {
        *selected = total - 1;
    }
}

/// Run the multi-page help screen. Left/Right flips pages, Back/Select exits.
fn run_help() {
    let mut page: u8 = 0;
    render::render_help_page(page);

    loop {
        match input::wait_for_menu_input() {
            MenuInput::Right => {
                if page + 1 < render::HELP_PAGES {
                    page += 1;
                    render::render_help_page(page);
                }
            }
            MenuInput::Left => {
                if page > 0 {
                    page -= 1;
                    render::render_help_page(page);
                }
            }
            MenuInput::Back | MenuInput::Select => return,
            MenuInput::Up | MenuInput::Down => {}
        }
    }
}

/// Run autorun: skip to destination instantly, then render once.
/// Combat SFX/shake fires if the final step involved combat.
fn run_autorun(state: &mut MicroGameState, dir: Direction) {
    // Immediate feedback: show "Running..." via the game log.
    state.log.add(GameEvent::Autorun);
    render::render_messages(state);

    let mut stepper = MicroAutorunStepper::new(dir);
    let mut last_msg_total;

    // Bank I/O out per-step rather than for the entire loop.
    // The stepper's step calls may invoke compute_fov (overlay at $D000),
    // which needs I/O banked out. Banking per-step ensures I/O is restored
    // between iterations so the loop works regardless of IRQ state.
    let stop_reason = loop {
        last_msg_total = state.log.total();
        c64::io_bank_out();
        let outcome = stepper.next_step(state);
        c64::io_bank_in();
        match outcome {
            MicroStepOutcome::Continue => continue,
            MicroStepOutcome::Done(reason) => break reason,
        }
    };

    // Ensure FOV is current (last step may have skipped it).
    let pi = PLAYER_IDX as usize;
    c64::io_bank_out();
    state
        .fov
        .compute_fov(state.entities.x[pi], state.entities.y[pi], &state.map);
    c64::io_bank_in();

    // Log why autorun stopped (unless combat/death events already explain it).
    match stop_reason {
        MicroAutorunStop::DamageTaken | MicroAutorunStop::GameOver => {}
        reason => {
            state.log.add(GameEvent::AutorunStop {
                cause: reason.to_cause(),
            });
        }
    }

    // Combat feedback for the final step.
    apply_combat_feedback(state, last_msg_total);
}

/// Full render + diff snapshot. Deduplicates the render_all + snapshot
/// sequence used after modal exits and state transitions.
#[inline(never)]
fn render_and_snapshot(state: &MicroGameState) {
    render::render_all(state);
    let diff = unsafe { &mut DIFF };
    diff.snapshot(state, render::viewport_pos(state));
}

/// Apply combat screen effects (shake + SFX) based on recent log events.
#[inline(never)]
fn apply_combat_feedback(state: &MicroGameState, old_msg_total: u16) {
    let combat = detect_combat(state, old_msg_total);
    if combat.player_attacked || combat.player_hurt {
        c64::shake_start();
    }
    if combat.player_attacked {
        c64::sfx_attack();
    }
    if combat.player_hurt {
        c64::sfx_hurt();
    }
}

/// Start a new game with spinner, render, and begin music.
#[inline(never)]
fn start_and_present_game(seed: u16, width: u8, height: u8) {
    render::render_loading();
    c64::spinner_start();
    let state = start_game(seed, width, height);
    c64::spinner_stop();
    render_and_snapshot(state);
    c64::music_start();
}

/// Post-step rendering: full redraw on descent, viewport scroll, or diff render.
#[inline(never)]
fn render_after_step(state: &MicroGameState, old_depth: u8) {
    let diff = unsafe { &mut DIFF };
    if state.depth != old_depth {
        let vp = render::viewport_pos(state);
        render::render_all(state);
        diff.snapshot(state, vp);
    } else {
        let (old_vx, old_vy) = diff.viewport;
        let (vx, vy) = render::viewport_pos_lazy(state, old_vx, old_vy);
        if (vx, vy) != (old_vx, old_vy) {
            render::render_viewport_scroll(state, diff, vx, vy, old_vx, old_vy);
        } else {
            render::draw_player_immediate(state, diff, vx, vy);
            render::render_diff(state, diff, vx, vy);
        }
        diff.snapshot(state, (vx, vy));
    }
}

/// Thin wrapper — must have NO local state so the compiler allocates no
/// static stacks here.  copy_code_to_ram() must run first: it copies
/// overlay + HIRAM code from LMA to VMA before init_hardware()'s
/// callee-save prologue can overwrite the LMA data in .noinit.
#[no_mangle]
pub extern "C" fn main() -> isize {
    c64::copy_code_to_ram();
    c64::init_hardware();
    game_loop()
}

#[inline(never)]
fn game_loop() -> ! {
    let mut app_state = AppState::Title;
    let mut current_width: u8 = DEFAULT_MAP_WIDTH;
    let mut current_height: u8 = DEFAULT_MAP_HEIGHT;

    loop {
        match app_state {
            AppState::Title => {
                let (seed, w, h) = run_title();
                current_width = w;
                current_height = h;
                start_and_present_game(seed, w, h);
                app_state = AppState::Playing;
            }
            AppState::Playing => {
                let state = unsafe { STATE.assume_init_mut() };

                if state.is_terminal() {
                    c64::music_stop();
                    render::render_all(state);
                    app_state = AppState::GameOver;
                    continue;
                }

                let cmd = input::wait_for_input();

                if cmd == GameCommand::Quit {
                    c64::music_stop();
                    app_state = AppState::Paused;
                    continue;
                }

                if cmd == GameCommand::Look {
                    app_state = AppState::Looking;
                    continue;
                }

                if cmd == GameCommand::OpenInventory {
                    app_state = AppState::Inventory;
                    continue;
                }

                if cmd == GameCommand::Help {
                    app_state = AppState::Help;
                    continue;
                }

                if cmd == GameCommand::MessageHistory {
                    app_state = AppState::MessageHistory;
                    continue;
                }

                if let GameCommand::Autorun(dir) = cmd {
                    run_autorun(state, dir);
                    // Full re-render after autorun to ensure clean state.
                    render_and_snapshot(state);
                    if state.is_terminal() {
                        c64::music_stop();
                        app_state = AppState::GameOver;
                    }
                    continue;
                }

                // Show loading spinner during descent (map generation is slow)
                let will_generate = cmd == GameCommand::Descend
                    && state.depth < balance::TARGET_DEPTH
                    && {
                        let pi = PLAYER_IDX as usize;
                        state.map.tile_at(state.entities.x[pi], state.entities.y[pi])
                            == TILE_STAIRS_DOWN
                    };
                if will_generate {
                    c64::music_fade_for_descent();
                    render::render_loading();
                    c64::spinner_start();
                    c64::sfx_descent();
                }

                let old_depth = state.depth;
                let msg_total = state.log.total();
                c64::io_bank_out();
                let result = state.step(cmd);
                c64::io_bank_in();

                if will_generate {
                    c64::spinner_stop();
                    c64::music_resume();
                }

                if !result.action_taken {
                    continue; // nothing changed, skip rendering
                }

                // Combat feedback: screen shake + SID sound effects.
                // IRQ-driven shake runs asynchronously during rendering.
                apply_combat_feedback(state, msg_total);

                render_after_step(state, old_depth);

                if state.is_terminal() {
                    c64::music_stop();
                    app_state = AppState::GameOver;
                }
            }
            AppState::Looking => {
                let state = unsafe { STATE.assume_init_mut() };
                run_look_mode(state);
                render_and_snapshot(state);
                app_state = AppState::Playing;
            }
            AppState::Inventory => {
                let state = unsafe { STATE.assume_init_mut() };
                run_inventory(state);
                render_and_snapshot(state);
                app_state = AppState::Playing;
            }
            AppState::Help => {
                run_help();
                let state = unsafe { STATE.assume_init_mut() };
                render_and_snapshot(state);
                app_state = AppState::Playing;
            }
            AppState::MessageHistory => {
                let state = unsafe { STATE.assume_init_mut() };
                render::render_message_history(state);
                input::wait_for_menu_input(); // any key dismisses
                render_and_snapshot(state);
                app_state = AppState::Playing;
            }
            AppState::Paused => {
                let state = unsafe { STATE.assume_init_mut() };
                match run_pause(state) {
                    AppState::Playing => {
                        render_and_snapshot(state);
                        c64::music_start();
                        app_state = AppState::Playing;
                    }
                    AppState::Title => {
                        app_state = AppState::Title;
                    }
                    other => app_state = other,
                }
            }
            AppState::GameOver => {
                let state = unsafe { STATE.assume_init_mut() };
                match run_end_screen(state) {
                    AppState::Playing => {
                        // Play Again — new random seed, same dimensions
                        start_and_present_game(
                            read_cia_seed(),
                            current_width,
                            current_height,
                        );
                        app_state = AppState::Playing;
                    }
                    AppState::Title => {
                        app_state = AppState::Title;
                    }
                    other => app_state = other,
                }
            }
        }
    }
}
