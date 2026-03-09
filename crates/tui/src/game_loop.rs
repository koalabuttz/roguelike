use std::io::{self, Write};
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crossterm::{cursor, queue, style, terminal};

use roguelike_core::command::GameCommand;
use roguelike_core::data::{self, GameData};
use roguelike_core::game::{self, GameState, LookOptions};
use roguelike_core::game_step::{self, GameStep, MicroGameStateAdapter};
use roguelike_core::help;
use roguelike_core::look::{LookAction, LookCursor};
use roguelike_core::menu::{self, MenuAction};
use roguelike_core::message_history::{MessageHistoryViewer, ViewerAction};
use roguelike_core::platform::Renderer;
use roguelike_core::rules::balance;
use roguelike_core::seed_code;
use roguelike_core::settings::{self, Platform, Settings};
use roguelike_core::spectate::FrameSink;
use roguelike_core::types::Coord;

use crate::input_provider::{GameInput, HistoryInput, InputProvider, InputResult};
use crate::render::{self, CrosstermRenderer, Viewport};
use crate::saves::SaveBackend;

/// Top-level application state machine.
enum AppState {
    Title(menu::Menu),
    Playing,
    Paused(menu::Menu),
}

/// What caused the game loop to exit.
pub enum GameLoopResult {
    /// Normal quit or disconnect.
    Quit,
    /// User chose "Lobby" — return to the server lobby (SSH).
    Lobby,
}

/// Optional dev-tools callbacks for debug builds.
///
/// All methods have default no-op implementations so `NoDevHooks` (or any
/// struct that doesn't need dev-tools) can simply `impl DevHooks for T {}`.
///
/// Methods accept `&mut dyn GameStep` instead of `&mut GameState` so the
/// trait works with any capability tier.  Implementations that need
/// standard-tier state downcast internally via `as_any_mut()`.
pub trait DevHooks {
    /// Handle a debug key press (F1-F12, overlay cursor movement, etc.).
    /// Returns `true` if the key was consumed and should not be passed
    /// to the normal game command handler.
    fn handle_dev_key(
        &mut self,
        _key: KeyEvent,
        _state: &mut dyn GameStep,
        _game_data: &mut GameData,
    ) -> bool {
        false
    }

    /// Called after each game step (move, wait, etc.) for recording/god-mode.
    fn after_step(&mut self, _state: &mut dyn GameStep, _cmd: GameCommand) {}

    /// Whether field-of-view is currently disabled (all tiles visible).
    fn fov_disabled(&self) -> bool {
        false
    }

    /// Make all tiles visible (called during animations when FOV is disabled).
    fn apply_fov_override(&self, _state: &mut dyn GameStep) {}

    /// Look-mode options (e.g. whether to reveal monsters outside FOV).
    fn look_options(&self) -> LookOptions {
        LookOptions::default()
    }

    /// Render a debug overlay on top of the game view.
    fn render_overlay<W2: Write>(
        &self,
        _w: &mut W2,
        _state: &dyn GameStep,
        _pal: settings::ColorPalette,
    ) -> io::Result<()> {
        Ok(())
    }
}

/// No-op dev-hooks for release builds and SSH.
pub struct NoDevHooks;
impl DevHooks for NoDevHooks {}

/// Try to downcast a `&dyn GameStep` to `&GameState`.
fn standard_state(game: &dyn GameStep) -> Option<&GameState> {
    game.as_any().downcast_ref::<GameState>()
}

/// Try to downcast a `&mut dyn GameStep` to `&mut GameState`.
fn standard_state_mut(game: &mut dyn GameStep) -> Option<&mut GameState> {
    game.as_any_mut().downcast_mut::<GameState>()
}

/// Render any game tier through the unified `RenderSource` trait.
///
/// Downcasts `&dyn GameStep` to either `GameState` or `MicroGameStateAdapter`
/// and renders via `render::render(&dyn RenderSource, …)`. Falls back to the
/// observation-based renderer for unknown tiers.
fn render_game<W: Write>(
    w: &mut W,
    game: &dyn GameStep,
    cols: Coord,
    rows: Coord,
    settings: &Settings,
) -> io::Result<Viewport> {
    if let Some(gs) = game.as_any().downcast_ref::<GameState>() {
        render::render(w, gs, cols, rows, settings)
    } else if let Some(adapter) = game.as_any().downcast_ref::<MicroGameStateAdapter>() {
        render::render(w, adapter, cols, rows, settings)
    } else {
        // Unknown tier: fall back to observation-based render.
        render::render_observation(
            w,
            &game.observe(),
            cols,
            rows,
            settings.message_log_lines as Coord,
        )?;
        Ok(Viewport { x: 0, y: 0 })
    }
}

/// Configuration for the game loop.
pub struct GameLoopConfig {
    /// Platform identifier (Terminal or Ssh) — controls which settings
    /// are shown in the settings menu.
    pub platform: Platform,
    /// Initial terminal width in character cells.
    pub cols: i32,
    /// Initial terminal height in character cells.
    pub rows: i32,
}

/// Run the unified game loop.
///
/// This is the shared entry point for both terminal and SSH frontends.
/// All platform-specific behavior is injected through the trait parameters.
pub fn run_game_loop<W: Write, D: DevHooks>(
    renderer: &mut CrosstermRenderer<W>,
    input: &mut dyn InputProvider,
    saves: &dyn SaveBackend,
    dev: &mut D,
    config: GameLoopConfig,
    frame_sink: &dyn FrameSink,
) -> io::Result<GameLoopResult> {
    let mut cols = config.cols;
    let mut rows = config.rows;
    let platform = config.platform;

    let mut settings = saves.load_settings();
    let mut game_data = data::load_game_data();
    let mut game_state: Option<Box<dyn GameStep>> = None;
    let mut autosave_buf: Option<String> = None;
    let has_save = saves.has_save_for_title(settings.casual_mode);
    let mut app_state = AppState::Title(menu::title_menu(has_save, settings.casual_mode, platform));

    'app: loop {
        // Check for terminal resize (SSH only; terminal returns None).
        if let Some((w, h)) = input.check_resize() {
            cols = w;
            rows = h;
            renderer.set_size(cols, rows);
        }

        match &mut app_state {
            AppState::Title(title) => {
                match run_menu(title, renderer, input)? {
                    None => break 'app, // Disconnected
                    Some(MenuAction::NewGame) => {
                        let autosave_exists = saves.has_autosave();
                        if autosave_exists {
                            let msg = if settings.casual_mode {
                                "Start new game? Manual saves will be kept."
                            } else {
                                "This will abandon your saved game."
                            };
                            let mut confirm = menu::confirm_menu(msg);
                            match run_menu(&mut confirm, renderer, input)? {
                                Some(MenuAction::Confirm) => {
                                    saves.delete_autosave();
                                }
                                _ => {
                                    let has_save = saves.has_save_for_title(settings.casual_mode);
                                    app_state = AppState::Title(menu::title_menu(
                                        has_save,
                                        settings.casual_mode,
                                        platform,
                                    ));
                                    continue;
                                }
                            }
                        }
                        match game_step::create_random_game(
                            balance::STANDARD_MAP_WIDTH as i32,
                            balance::STANDARD_MAP_HEIGHT as i32,
                            &game_data,
                        ) {
                            Ok(g) => {
                                game_state = Some(g);
                                autosave_buf = None;
                                app_state = AppState::Playing;
                            }
                            Err(msg) => {
                                run_error_dialog(&msg, renderer, input)?;
                                let has_save = saves.has_save_for_title(settings.casual_mode);
                                app_state = AppState::Title(menu::title_menu(
                                    has_save,
                                    settings.casual_mode,
                                    platform,
                                ));
                            }
                        }
                    }
                    Some(MenuAction::EnterSeed) => {
                        match text_input_dialog(
                            renderer,
                            input,
                            "Enter Seed Code",
                            "e.g. r7z3kq or r7z3kq-120x60a | Esc to cancel",
                        )? {
                            Some(code) if !code.is_empty() => match seed_code::decode(&code) {
                                Ok(params) => {
                                    match game_step::create_game(
                                        params.seed,
                                        params.width,
                                        params.height,
                                        params.preset,
                                        &game_data,
                                    ) {
                                        Ok(g) => {
                                            game_state = Some(g);
                                            autosave_buf = None;
                                            app_state = AppState::Playing;
                                        }
                                        Err(msg) => {
                                            run_error_dialog(&msg, renderer, input)?;
                                            let has_save =
                                                saves.has_save_for_title(settings.casual_mode);
                                            app_state = AppState::Title(menu::title_menu(
                                                has_save,
                                                settings.casual_mode,
                                                platform,
                                            ));
                                        }
                                    }
                                }
                                Err(msg) => {
                                    run_error_dialog(&msg, renderer, input)?;
                                    let has_save = saves.has_save_for_title(settings.casual_mode);
                                    app_state = AppState::Title(menu::title_menu(
                                        has_save,
                                        settings.casual_mode,
                                        platform,
                                    ));
                                }
                            },
                            _ => {
                                let has_save = saves.has_save_for_title(settings.casual_mode);
                                app_state = AppState::Title(menu::title_menu(
                                    has_save,
                                    settings.casual_mode,
                                    platform,
                                ));
                            }
                        }
                    }
                    Some(MenuAction::LoadGame) => {
                        handle_load_game(
                            &mut app_state,
                            &mut game_state,
                            &mut autosave_buf,
                            renderer,
                            input,
                            saves,
                            &settings,
                            None, // no pause menu index
                            platform,
                        )?;
                    }
                    Some(MenuAction::Settings) => {
                        run_settings_loop(renderer, input, saves, &mut settings, config.platform)?;
                        let has_save = saves.has_save_for_title(settings.casual_mode);
                        app_state = AppState::Title(menu::title_menu(
                            has_save,
                            settings.casual_mode,
                            platform,
                        ));
                    }
                    Some(MenuAction::Quit) => break 'app,
                    Some(MenuAction::Back) => {
                        if platform == Platform::Ssh {
                            return Ok(GameLoopResult::Lobby);
                        }
                        break 'app;
                    }
                    Some(MenuAction::Lobby) => return Ok(GameLoopResult::Lobby),
                    _ => {}
                }
            }

            AppState::Playing => {
                let game = game_state.as_mut().expect("no game state while playing");

                // Flush autosave buffer to disk (standard tier only).
                if let Some(ref buf) = autosave_buf {
                    if let Some(gs) = standard_state_mut(game.as_mut()) {
                        let mut meta = gs.extract_metadata();
                        if !settings.player_name.is_empty() {
                            meta.player_name = Some(settings.player_name.clone());
                        }
                        saves.write_autosave(buf, &meta);
                    }
                    autosave_buf = None;
                }

                // Render via unified RenderSource trait.
                render_game(renderer.writer(), game.as_ref(), cols, rows, &settings)?;
                dev.render_overlay(renderer.writer(), game.as_ref(), settings.color_palette)?;

                if game.is_game_over() {
                    // Game-over or victory: any input returns to title.
                    let _ = input.wait_for_key()?;
                    saves.delete_autosave();
                    game_state = None;
                    autosave_buf = None;
                    let has_save = saves.has_save_for_title(settings.casual_mode);
                    app_state =
                        AppState::Title(menu::title_menu(has_save, settings.casual_mode, platform));
                    continue;
                }

                let game_input = input.wait_for_game_input(&settings)?;

                let cmd = match game_input {
                    GameInput::Key { key, command } => {
                        // Dev-tools key handling (F1-F12, overlay cursor, etc.).
                        if dev.handle_dev_key(key, game.as_mut(), &mut game_data) {
                            continue;
                        }

                        // Message history viewer (Ctrl+P).
                        if key.modifiers.contains(KeyModifiers::CONTROL)
                            && key.code == KeyCode::Char('p')
                        {
                            if let Some(gs) = standard_state(game.as_ref()) {
                                run_message_history(gs.log.all(), renderer, input)?;
                            } else {
                                let obs = game.observe();
                                run_message_history(&obs.recent_messages, renderer, input)?;
                            }
                            continue;
                        }

                        command
                    }
                    GameInput::GamepadCommand(cmd) => Some(cmd),
                    GameInput::Disconnected => break 'app,
                };

                if let Some(cmd) = cmd {
                    match cmd {
                        GameCommand::Quit => {
                            app_state =
                                AppState::Paused(menu::pause_menu(settings.casual_mode, platform));
                        }
                        GameCommand::OpenInventory => {
                            run_inventory_modal(game.as_mut(), renderer, input)?;
                        }
                        GameCommand::Look => {
                            let look_opts = dev.look_options();
                            run_look_mode(
                                game.as_ref(),
                                renderer,
                                cols,
                                rows,
                                &settings,
                                &look_opts,
                                input,
                            )?;
                        }
                        GameCommand::Help => {
                            let lines = help::help_lines(&settings, &game_data);
                            run_message_history(&lines, renderer, input)?;
                        }
                        GameCommand::Autorun(dir) => {
                            if standard_state(game.as_ref()).is_some() {
                                let gs = standard_state_mut(game.as_mut()).unwrap();
                                let stepper = gs.start_autorun(dir);
                                animate_stepper(
                                    renderer.writer(),
                                    gs,
                                    stepper,
                                    cols,
                                    rows,
                                    &settings,
                                    dev,
                                    input,
                                )?;
                            } else if let Some(adapter) =
                                game.as_any_mut().downcast_mut::<MicroGameStateAdapter>()
                            {
                                let stepper = adapter.start_autorun(dir);
                                animate_micro_stepper(
                                    renderer.writer(),
                                    adapter,
                                    stepper,
                                    cols,
                                    rows,
                                    &settings,
                                    input,
                                )?;
                            } else {
                                // Unknown tier: single move fallback.
                                game.step(GameCommand::Move(dir));
                            }
                            dev.after_step(game.as_mut(), cmd);
                            frame_sink.write_frame(&game.observe());
                            if let Some(gs) = standard_state_mut(game.as_mut())
                                && gs.dirty
                                && let Ok(json) = gs.save_to_json()
                            {
                                gs.dirty = false;
                                autosave_buf = Some(json);
                            }
                        }
                        GameCommand::AutoExplore => {
                            // Auto-explore is standard-tier only.
                            let mut explored = false;
                            if let Some(gs) = standard_state_mut(game.as_mut()) {
                                match gs.start_auto_explore() {
                                    Ok((stepper, _tx, _ty)) => {
                                        animate_stepper(
                                            renderer.writer(),
                                            gs,
                                            stepper,
                                            cols,
                                            rows,
                                            &settings,
                                            dev,
                                            input,
                                        )?;
                                        explored = true;
                                    }
                                    Err(_) => {
                                        gs.log.add("No unexplored areas reachable.");
                                    }
                                }
                            }
                            if explored {
                                dev.after_step(game.as_mut(), cmd);
                                frame_sink.write_frame(&game.observe());
                                if let Some(gs) = standard_state_mut(game.as_mut())
                                    && gs.dirty
                                    && let Ok(json) = gs.save_to_json()
                                {
                                    gs.dirty = false;
                                    autosave_buf = Some(json);
                                }
                            }
                        }
                        _ => {
                            game.step(cmd);
                            dev.after_step(game.as_mut(), cmd);
                            frame_sink.write_frame(&game.observe());
                            if let Some(gs) = standard_state_mut(game.as_mut())
                                && gs.dirty
                                && (gs.turn_count as u32)
                                    .is_multiple_of(settings.autosave_frequency)
                                && let Ok(json) = gs.save_to_json()
                            {
                                gs.dirty = false;
                                autosave_buf = Some(json);
                            }
                        }
                    }
                }
            }

            AppState::Paused(pause) => {
                let action = run_menu(pause, renderer, input)?;
                match action {
                    Some(MenuAction::ResumeGame | MenuAction::Back) => {
                        app_state = AppState::Playing;
                    }
                    None => {
                        // Disconnected
                        break 'app;
                    }
                    Some(MenuAction::SaveGame) => {
                        // Saving is standard-tier only.
                        if let Some(ref game) = game_state
                            && let Some(gs) = standard_state(game.as_ref())
                        {
                            let slots = saves.load_all_slot_metadata();
                            let mut slot_menu =
                                menu::save_slot_menu(&slots, settings.show_explored_pct);
                            match run_menu(&mut slot_menu, renderer, input)? {
                                Some(MenuAction::SelectSlot(slot)) => {
                                    let msg = saves.save_to_slot(gs, slot, &settings.player_name);
                                    let mut new_pause =
                                        menu::pause_menu(settings.casual_mode, platform);
                                    new_pause.selected = 1;
                                    app_state = AppState::Paused(new_pause);
                                    if let Some(ref mut game) = game_state
                                        && let Some(gs) = standard_state_mut(game.as_mut())
                                    {
                                        gs.log.add(&msg);
                                    }
                                }
                                _ => {
                                    let mut new_pause =
                                        menu::pause_menu(settings.casual_mode, platform);
                                    new_pause.selected = 1;
                                    app_state = AppState::Paused(new_pause);
                                }
                            }
                        }
                    }
                    Some(MenuAction::LoadGame) => {
                        handle_load_game(
                            &mut app_state,
                            &mut game_state,
                            &mut autosave_buf,
                            renderer,
                            input,
                            saves,
                            &settings,
                            Some(2), // pause menu "Load Game" index
                            platform,
                        )?;
                    }
                    Some(MenuAction::TitleScreen) => {
                        game_state = None;
                        autosave_buf = None;
                        let has_save = saves.has_save_for_title(settings.casual_mode);
                        app_state = AppState::Title(menu::title_menu(
                            has_save,
                            settings.casual_mode,
                            platform,
                        ));
                    }
                    Some(MenuAction::Quit) => break 'app,
                    Some(MenuAction::Lobby) => return Ok(GameLoopResult::Lobby),
                    _ => {}
                }
            }
        }
    }

    Ok(GameLoopResult::Quit)
}

// ---------------------------------------------------------------------------
// Sub-loop helpers
// ---------------------------------------------------------------------------

/// Show a single-button "Error: {msg}" dialog and wait for the user to dismiss it.
fn run_error_dialog<W: Write>(
    msg: &str,
    renderer: &mut CrosstermRenderer<W>,
    input: &mut dyn InputProvider,
) -> io::Result<()> {
    let mut err_menu = menu::confirm_menu(&format!("Error: {msg}"));
    err_menu.selected = 0;
    err_menu.items = vec![menu::MenuItem {
        label: "OK".to_string(),
        action: MenuAction::Back,
        enabled: true,
    }];
    let _ = run_menu(&mut err_menu, renderer, input)?;
    Ok(())
}

/// Run a menu loop. Returns `None` if the input was disconnected.
fn run_menu<W: Write>(
    menu: &mut menu::Menu,
    renderer: &mut CrosstermRenderer<W>,
    input: &mut dyn InputProvider,
) -> io::Result<Option<MenuAction>> {
    loop {
        menu.draw(renderer);

        match input.wait_for_menu_command()? {
            InputResult::Command(cmd) => {
                if let Some(action) = menu.handle_input(cmd) {
                    return Ok(Some(action));
                }
            }
            InputResult::NoCommand => {}
            InputResult::Disconnected => return Ok(None),
        }
    }
}

/// Run the full-screen message history viewer.
fn run_message_history<W: Write>(
    messages: &[String],
    renderer: &mut CrosstermRenderer<W>,
    input: &mut dyn InputProvider,
) -> io::Result<()> {
    let mut viewer = MessageHistoryViewer::new(messages);
    loop {
        viewer.draw(renderer);

        let (_, screen_h) = renderer.screen_size();
        let page_size = (screen_h - 2).max(1) as usize;

        match input.wait_for_history_input()? {
            InputResult::Command(cmd) => match cmd {
                HistoryInput::PageUp => viewer.page_up(page_size),
                HistoryInput::PageDown => viewer.page_down(page_size),
                HistoryInput::HalfPageUp => viewer.page_up(page_size / 2),
                HistoryInput::HalfPageDown => viewer.page_down(page_size / 2),
                HistoryInput::ScrollToTop => viewer.scroll_up(usize::MAX),
                HistoryInput::ScrollToBottom => viewer.scroll_down(usize::MAX),
                HistoryInput::Menu(cmd) => {
                    if viewer.handle_input(cmd) == ViewerAction::Close {
                        return Ok(());
                    }
                }
            },
            InputResult::NoCommand => {}
            InputResult::Disconnected => return Ok(()),
        }
    }
}

/// Run look mode: move a cursor around to examine tiles.
#[allow(clippy::too_many_arguments)]
fn run_look_mode<W: Write>(
    game: &dyn GameStep,
    renderer: &mut CrosstermRenderer<W>,
    cols: Coord,
    rows: Coord,
    settings: &Settings,
    look_opts: &LookOptions,
    input: &mut dyn InputProvider,
) -> io::Result<()> {
    let (px, py) = game.player_pos();
    let mut cursor = LookCursor::new(px, py);

    // Standard tier: LookCursor with map-bounds checking.
    // Other tiers: manual cursor movement (no bounds check).
    if let Some(gs) = standard_state(game) {
        loop {
            let vp = render::render(renderer.writer(), gs, cols, rows, settings)?;
            let info = cursor.current_info_with(gs, look_opts);
            cursor.draw_overlay(
                renderer,
                &info,
                rows,
                settings.message_log_lines as Coord,
                (vp.x, vp.y),
            );
            renderer.flush();

            match input.wait_for_look_command(settings)? {
                InputResult::Command(cmd) => {
                    if cursor.handle_input(cmd, gs) == LookAction::Close {
                        return Ok(());
                    }
                }
                InputResult::NoCommand => {}
                InputResult::Disconnected => return Ok(()),
            }
        }
    } else {
        loop {
            let vp = render_game(renderer.writer(), game, cols, rows, settings)?;
            let info = game.look_at(cursor.cursor_x, cursor.cursor_y);
            cursor.draw_overlay(
                renderer,
                &info,
                rows,
                settings.message_log_lines as Coord,
                (vp.x, vp.y),
            );
            renderer.flush();

            match input.wait_for_look_command(settings)? {
                InputResult::Command(cmd) => match cmd {
                    roguelike_core::look::LookCommand::Move(dir) => {
                        let (dx, dy) = dir.to_offset();
                        cursor.cursor_x += dx;
                        cursor.cursor_y += dy;
                    }
                    roguelike_core::look::LookCommand::Close => return Ok(()),
                },
                InputResult::NoCommand => {}
                InputResult::Disconnected => return Ok(()),
            }
        }
    }
}

/// Run the inventory modal. Blocks until the user closes it.
///
/// Displays inventory contents from the game observation. The user selects
/// a slot by letter (a-z), then presses u/e/d to use/equip/drop.
fn run_inventory_modal<W: Write>(
    game: &mut dyn GameStep,
    renderer: &mut CrosstermRenderer<W>,
    input: &mut dyn InputProvider,
) -> io::Result<()> {
    let (screen_w, screen_h) = renderer.screen_size();
    let mut selected: Option<usize> = None;

    loop {
        let obs = game.observe();
        let hint = match selected {
            Some(idx) => {
                let letter = (b'a' + idx as u8) as char;
                format!("[u]se  [e]quip  [d]rop  slot '{}'  [Esc] back", letter)
            }
            None => {
                let mut parts = Vec::new();
                if obs.weapon.is_some() {
                    parts.push("[W] unequip weapon");
                }
                if obs.armor.is_some() {
                    parts.push("[A] unequip armor");
                }
                parts.join("  ")
            }
        };
        render::render_inventory(
            renderer.writer(),
            &obs.inventory,
            &obs.inventory_colors,
            obs.weapon.as_deref(),
            obs.armor.as_deref(),
            screen_w,
            screen_h,
            selected,
            &hint,
        )?;

        match input.wait_for_key()? {
            InputResult::Command(key) => match key.code {
                KeyCode::Esc => {
                    if selected.is_some() {
                        selected = None;
                    } else {
                        return Ok(());
                    }
                }
                KeyCode::Char('i') | KeyCode::Char('q') if selected.is_none() => {
                    return Ok(());
                }
                KeyCode::Char('W') if selected.is_none() => {
                    game.step(GameCommand::UnequipWeapon);
                }
                KeyCode::Char('A') if selected.is_none() => {
                    game.step(GameCommand::UnequipArmor);
                }
                KeyCode::Char(c @ 'a'..='z') if selected.is_none() => {
                    let idx = (c as u8 - b'a') as usize;
                    // Only select if the slot is occupied (check against inventory labels).
                    let slot_exists = obs
                        .inventory
                        .iter()
                        .any(|s| s.starts_with(&format!("{})", c)));
                    if slot_exists {
                        selected = Some(idx);
                    }
                }
                KeyCode::Char('u') if selected.is_some() => {
                    game.step(GameCommand::UseItem(selected.unwrap() as u8));
                    selected = None;
                }
                KeyCode::Char('e') if selected.is_some() => {
                    game.step(GameCommand::EquipItem(selected.unwrap() as u8));
                    selected = None;
                }
                KeyCode::Char('d') if selected.is_some() => {
                    game.step(GameCommand::DropItem(selected.unwrap() as u8));
                    selected = None;
                }
                _ => {}
            },
            InputResult::NoCommand => {}
            InputResult::Disconnected => return Ok(()),
        }
    }
}

/// Run animated autorun/auto-explore with frame pacing.
#[allow(clippy::too_many_arguments)]
fn animate_stepper<W: Write, D: DevHooks>(
    w: &mut W,
    state: &mut GameState,
    mut stepper: game::AutorunStepper,
    cols: Coord,
    rows: Coord,
    settings: &Settings,
    dev: &D,
    input: &mut dyn InputProvider,
) -> io::Result<()> {
    loop {
        match stepper.next_step(state) {
            game::StepOutcome::Continue => {
                if dev.fov_disabled() {
                    dev.apply_fov_override(state);
                }
                render::render(
                    w,
                    state as &dyn crate::render_source::RenderSource,
                    cols,
                    rows,
                    settings,
                )?;
                let timeout = Duration::from_millis(settings.animation_speed_ms as u64);
                if input.poll_animation_interrupt(timeout)? {
                    return Ok(());
                }
            }
            game::StepOutcome::Done(_) => return Ok(()),
        }
    }
}

/// Run animated micro-tier autorun with frame pacing.
#[allow(clippy::too_many_arguments)]
fn animate_micro_stepper<W: Write>(
    w: &mut W,
    adapter: &mut MicroGameStateAdapter,
    mut stepper: game_step::MicroAutorunStepper,
    cols: Coord,
    rows: Coord,
    settings: &Settings,
    input: &mut dyn InputProvider,
) -> io::Result<()> {
    loop {
        match stepper.next_step(adapter) {
            game::StepOutcome::Continue => {
                render::render(
                    w,
                    adapter as &dyn crate::render_source::RenderSource,
                    cols,
                    rows,
                    settings,
                )?;
                let timeout = Duration::from_millis(settings.animation_speed_ms as u64);
                if input.poll_animation_interrupt(timeout)? {
                    return Ok(());
                }
            }
            game::StepOutcome::Done(_) => return Ok(()),
        }
    }
}

/// Show a text input dialog. Returns `Some(text)` on Enter, `None` on Esc
/// or disconnect.
fn text_input_dialog<W: Write>(
    renderer: &mut CrosstermRenderer<W>,
    input: &mut dyn InputProvider,
    prompt: &str,
    hint: &str,
) -> io::Result<Option<String>> {
    let mut buf = String::new();
    loop {
        let (screen_w, screen_h) = renderer.screen_size();
        let cx = screen_w / 2;
        let cy = screen_h / 2;

        let w = renderer.writer();

        queue!(
            w,
            terminal::Clear(terminal::ClearType::All),
            cursor::MoveTo(0, 0)
        )?;

        // Prompt.
        let px = (cx - prompt.len() as i32 / 2).max(0);
        queue!(
            w,
            cursor::MoveTo(px as u16, (cy - 2) as u16),
            style::SetForegroundColor(style::Color::Cyan),
            style::SetBackgroundColor(style::Color::Black),
            style::Print(prompt)
        )?;

        // Input field with cursor.
        let display = format!("> {}_", buf);
        let ix = (cx - display.len() as i32 / 2).max(0);
        queue!(
            w,
            cursor::MoveTo(ix as u16, cy as u16),
            style::SetForegroundColor(style::Color::Yellow),
            style::SetBackgroundColor(style::Color::Black),
            style::Print(&display)
        )?;

        // Hint.
        let hx = (cx - hint.len() as i32 / 2).max(0);
        queue!(
            w,
            cursor::MoveTo(hx as u16, (cy + 2) as u16),
            style::SetForegroundColor(style::Color::DarkGrey),
            style::SetBackgroundColor(style::Color::Black),
            style::Print(hint)
        )?;

        w.flush()?;

        match input.wait_for_key()? {
            InputResult::Command(key) => match key.code {
                KeyCode::Esc => return Ok(None),
                KeyCode::Enter => return Ok(Some(buf)),
                KeyCode::Backspace => {
                    buf.pop();
                }
                KeyCode::Char(c)
                    if c.is_ascii_alphanumeric() || c == '-' || c == ' ' || c == '_' =>
                {
                    buf.push(c);
                }
                _ => {}
            },
            InputResult::NoCommand => {}
            InputResult::Disconnected => return Ok(None),
        }
    }
}

/// Handle the LoadGame action (used from both title and pause menus).
///
/// `pause_selected` is `Some(index)` when called from the pause menu (to
/// restore cursor position on error), `None` when called from the title menu.
#[allow(clippy::too_many_arguments)]
fn handle_load_game<W: Write>(
    app_state: &mut AppState,
    game_state: &mut Option<Box<dyn GameStep>>,
    autosave_buf: &mut Option<String>,
    renderer: &mut CrosstermRenderer<W>,
    input: &mut dyn InputProvider,
    saves: &dyn SaveBackend,
    settings: &Settings,
    pause_selected: Option<usize>,
    platform: Platform,
) -> io::Result<()> {
    if settings.casual_mode {
        // Show load-slot picker.
        let slots = saves.load_all_slot_metadata();
        let auto_meta = saves.load_autosave_metadata();
        let has_auto = saves.has_autosave();
        let mut load_menu =
            menu::load_slot_menu(has_auto, &auto_meta, &slots, settings.show_explored_pct);
        match run_menu(&mut load_menu, renderer, input)? {
            Some(MenuAction::LoadGame) => {
                // Load autosave (always standard tier).
                menu::draw_loading(renderer);
                match saves.load_autosave() {
                    Ok(mut loaded) => {
                        loaded.log.add("Game loaded.");
                        *game_state = Some(Box::new(loaded));
                        *autosave_buf = None;
                        *app_state = AppState::Playing;
                    }
                    Err(msg) => {
                        load_failed(
                            app_state,
                            game_state,
                            saves,
                            settings,
                            pause_selected,
                            &msg,
                            platform,
                        );
                    }
                }
            }
            Some(MenuAction::SelectSlot(slot)) => {
                menu::draw_loading(renderer);
                match saves.load_from_slot(slot) {
                    Ok(mut loaded) => {
                        loaded.log.add("Game loaded.");
                        *game_state = Some(Box::new(loaded));
                        *autosave_buf = None;
                        *app_state = AppState::Playing;
                    }
                    Err(msg) => {
                        load_failed(
                            app_state,
                            game_state,
                            saves,
                            settings,
                            pause_selected,
                            &msg,
                            platform,
                        );
                    }
                }
            }
            _ => {
                // Back — return to previous menu.
                load_back(app_state, saves, settings, pause_selected, platform);
            }
        }
    } else {
        // Classic mode — load autosave directly.
        menu::draw_loading(renderer);
        match saves.load_autosave() {
            Ok(mut loaded) => {
                loaded.log.add("Game loaded.");
                *game_state = Some(Box::new(loaded));
                *autosave_buf = None;
                *app_state = AppState::Playing;
            }
            Err(msg) => {
                load_failed(
                    app_state,
                    game_state,
                    saves,
                    settings,
                    pause_selected,
                    &msg,
                    platform,
                );
            }
        }
    }
    Ok(())
}

/// Handle a load failure: log the error and return to the appropriate menu.
fn load_failed(
    app_state: &mut AppState,
    game_state: &mut Option<Box<dyn GameStep>>,
    saves: &dyn SaveBackend,
    settings: &Settings,
    pause_selected: Option<usize>,
    msg: &str,
    platform: Platform,
) {
    if let Some(selected) = pause_selected {
        // From pause menu — log error and return.
        if let Some(game) = game_state
            && let Some(gs) = standard_state_mut(game.as_mut())
        {
            gs.log.add(msg);
        }
        let mut new_pause = menu::pause_menu(settings.casual_mode, platform);
        new_pause.selected = selected;
        *app_state = AppState::Paused(new_pause);
    } else {
        // From title menu — return to title.
        let has_save = saves.has_save_for_title(settings.casual_mode);
        *app_state = AppState::Title(menu::title_menu(has_save, settings.casual_mode, platform));
    }
}

/// Return to the previous menu after cancelling a load dialog.
fn load_back(
    app_state: &mut AppState,
    saves: &dyn SaveBackend,
    settings: &Settings,
    pause_selected: Option<usize>,
    platform: Platform,
) {
    if let Some(selected) = pause_selected {
        let mut new_pause = menu::pause_menu(settings.casual_mode, platform);
        new_pause.selected = selected;
        *app_state = AppState::Paused(new_pause);
    } else {
        let has_save = saves.has_save_for_title(settings.casual_mode);
        *app_state = AppState::Title(menu::title_menu(has_save, settings.casual_mode, platform));
    }
}

/// Run the settings submenu loop, applying changes and persisting them.
fn run_settings_loop<W: Write>(
    renderer: &mut CrosstermRenderer<W>,
    input: &mut dyn InputProvider,
    saves: &dyn SaveBackend,
    settings: &mut Settings,
    platform: Platform,
) -> io::Result<()> {
    loop {
        let mut settings_m = menu::settings_menu(settings, platform);
        match run_menu(&mut settings_m, renderer, input)? {
            Some(action) => {
                if !apply_settings_action(action, settings, renderer, input) {
                    break;
                }
                saves.save_settings(settings);
            }
            None => break, // Disconnected
        }
    }
    Ok(())
}

/// Apply a single settings menu action. Returns `true` if the settings loop
/// should continue, `false` if it should break (Back/unrecognized action).
fn apply_settings_action<W: Write>(
    action: MenuAction,
    settings: &mut Settings,
    renderer: &mut CrosstermRenderer<W>,
    input: &mut dyn InputProvider,
) -> bool {
    match action {
        MenuAction::ToggleCasualMode => settings.casual_mode = !settings.casual_mode,
        MenuAction::ToggleShowExploredPct => {
            settings.show_explored_pct = !settings.show_explored_pct;
        }
        MenuAction::ToggleShowCoordinates => {
            settings.show_coordinates = !settings.show_coordinates;
        }
        MenuAction::ToggleShowKeybindHints => {
            settings.show_keybind_hints = !settings.show_keybind_hints;
        }
        MenuAction::ToggleShowCorpses => settings.show_corpses = !settings.show_corpses,
        MenuAction::ToggleViKeys => settings.vi_keys = !settings.vi_keys,
        MenuAction::ToggleNumpad => settings.numpad = !settings.numpad,
        MenuAction::CycleAnimationSpeed => {
            settings.animation_speed_ms = match settings.animation_speed_ms {
                0 => 25,
                25 => 50,
                50 => 100,
                100 => 200,
                _ => 0,
            };
        }
        MenuAction::CycleAutosaveFrequency => {
            settings.autosave_frequency = match settings.autosave_frequency {
                1 => 5,
                5 => 10,
                10 => 25,
                _ => 1,
            };
        }
        MenuAction::CycleMessageLogLines => {
            settings.message_log_lines = match settings.message_log_lines {
                2 => 4,
                4 => 6,
                6 => 8,
                _ => 2,
            };
        }
        MenuAction::CycleColorPalette => {
            settings.color_palette = settings.color_palette.next();
        }
        MenuAction::CycleLeftHandLayout => {
            settings.left_hand_layout = settings.left_hand_layout.next();
        }
        MenuAction::EditPlayerName => {
            // text_input_dialog can fail or return None; either way we continue.
            if let Ok(Some(name)) = text_input_dialog(
                renderer,
                input,
                "Enter Your Name",
                "Letters, digits, hyphens, spaces, underscores | Esc to cancel",
            ) {
                settings.player_name = name;
            }
            // Return true but the caller should still save_settings (harmless if
            // the name didn't change — settings are small).
        }
        MenuAction::CyclePronouns => {
            settings.pronouns = settings.pronouns.next();
        }
        _ => return false,
    }
    true
}
