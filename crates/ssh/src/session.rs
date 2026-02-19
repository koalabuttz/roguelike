use std::io::Write;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyModifiers};

use roguelike_core::{
    command::GameCommand,
    data,
    game::{self, LookOptions},
    help,
    look::{LookAction, LookCursor},
    menu::{self, MenuAction},
    message_history::{MessageHistoryViewer, ViewerAction},
    platform::Renderer,
    seed_code,
    settings::{self, Platform},
    types::Coord,
};
use roguelike_tui::{input, render};

use crate::ansi_input::AnsiParser;
use crate::lobby::wait_for_key;
use crate::saves::SaveManager;

const MIN_WIDTH: i32 = 60;
const MIN_HEIGHT: i32 = 20;

/// Top-level application state machine (same as terminal, without gamepad).
enum AppState {
    Title(menu::Menu),
    Playing,
    Paused(menu::Menu),
}

/// Run the full game session for a logged-in user.
///
/// This runs on a blocking thread (via `spawn_blocking`). Communication
/// with the async SSH handler is via `rx` (input bytes) and `writer`
/// (output to SSH channel). `size_rx` provides terminal resize events.
pub fn run_session<W: Write>(
    writer: &mut W,
    rx: &Receiver<Vec<u8>>,
    size_rx: &mut tokio::sync::watch::Receiver<(u32, u32)>,
    parser: &mut AnsiParser,
    saves: &SaveManager,
    username: &str,
) -> std::io::Result<()> {
    let (mut cols, mut rows) = {
        let (w, h) = *size_rx.borrow();
        (w as i32, h as i32)
    };

    // Enforce minimum terminal size
    if cols < MIN_WIDTH || rows < MIN_HEIGHT {
        show_resize_prompt(writer, cols, rows)?;
        loop {
            if size_rx.has_changed().unwrap_or(false) {
                let (w, h) = *size_rx.borrow_and_update();
                cols = w as i32;
                rows = h as i32;
                if cols >= MIN_WIDTH && rows >= MIN_HEIGHT {
                    break;
                }
                show_resize_prompt(writer, cols, rows)?;
            }
            // Wait for input or timeout to re-check
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(_) => {} // Discard input while waiting for resize
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                Err(_) => {}
            }
        }
    }

    let mut renderer = render::CrosstermRenderer::new(writer, cols, rows);

    let mut settings = saves.load_settings();
    let game_data = data::load_game_data();
    let map_height = rows - 1 - settings.message_log_lines as i32;
    let mut game_state: Option<game::GameState> = None;
    let mut autosave_buf: Option<String> = None;
    let has_save = saves.has_save_for_title(settings.casual_mode);
    let mut app_state = AppState::Title(menu::title_menu(has_save, settings.casual_mode));

    'app: loop {
        // Check for resize events
        if size_rx.has_changed().unwrap_or(false) {
            let (w, h) = *size_rx.borrow_and_update();
            cols = w as i32;
            rows = h as i32;
            renderer.set_size(cols, rows);
        }

        match &mut app_state {
            AppState::Title(title) => {
                match run_menu(title, &mut renderer, rx, parser)? {
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
                            match run_menu(&mut confirm, &mut renderer, rx, parser)? {
                                Some(MenuAction::Confirm) => {
                                    saves.delete_autosave();
                                }
                                _ => {
                                    let has_save = saves.has_save_for_title(settings.casual_mode);
                                    app_state = AppState::Title(menu::title_menu(
                                        has_save,
                                        settings.casual_mode,
                                    ));
                                    continue;
                                }
                            }
                        }
                        game_state =
                            Some(game::GameState::new_with_data(cols, map_height, &game_data));
                        autosave_buf = None;
                        app_state = AppState::Playing;
                    }
                    Some(MenuAction::EnterSeed) => {
                        match text_input_dialog(
                            renderer.writer(),
                            rx,
                            parser,
                            "Enter Seed Code",
                            "e.g. r7z3kq or r7z3kq-120x60a | Esc to cancel",
                            cols,
                            rows,
                        )? {
                            Some(code) if !code.is_empty() => match seed_code::decode(&code) {
                                Ok(params) => {
                                    let h = rows - 1 - settings.message_log_lines as i32;
                                    let gs = if let Some(preset) = params.preset {
                                        game::GameState::with_preset_data(
                                            params.width,
                                            params.height.min(h),
                                            params.seed,
                                            preset,
                                            &game_data,
                                        )
                                    } else {
                                        game::GameState::with_data(
                                            params.width,
                                            params.height.min(h),
                                            params.seed,
                                            &game_data,
                                        )
                                    };
                                    game_state = Some(gs);
                                    autosave_buf = None;
                                    app_state = AppState::Playing;
                                }
                                Err(msg) => {
                                    let mut err_menu = menu::confirm_menu(&format!("Error: {msg}"));
                                    err_menu.selected = 0;
                                    err_menu.items = vec![menu::MenuItem {
                                        label: "OK".to_string(),
                                        action: MenuAction::Back,
                                        enabled: true,
                                    }];
                                    let _ = run_menu(&mut err_menu, &mut renderer, rx, parser)?;
                                    let has_save = saves.has_save_for_title(settings.casual_mode);
                                    app_state = AppState::Title(menu::title_menu(
                                        has_save,
                                        settings.casual_mode,
                                    ));
                                }
                            },
                            _ => {
                                let has_save = saves.has_save_for_title(settings.casual_mode);
                                app_state = AppState::Title(menu::title_menu(
                                    has_save,
                                    settings.casual_mode,
                                ));
                            }
                        }
                    }
                    Some(MenuAction::LoadGame) => {
                        if settings.casual_mode {
                            let slots = saves.load_all_slot_metadata();
                            let auto_meta = saves.load_autosave_metadata();
                            let has_auto = saves.has_autosave();
                            let mut load_menu = menu::load_slot_menu(
                                has_auto,
                                &auto_meta,
                                &slots,
                                settings.show_explored_pct,
                            );
                            match run_menu(&mut load_menu, &mut renderer, rx, parser)? {
                                Some(MenuAction::LoadGame) => {
                                    menu::draw_loading(&mut renderer);
                                    match saves.load_autosave() {
                                        Ok(mut loaded) => {
                                            loaded.log.add("Game loaded.");
                                            game_state = Some(loaded);
                                            autosave_buf = None;
                                            app_state = AppState::Playing;
                                        }
                                        Err(_) => {
                                            let has_save =
                                                saves.has_save_for_title(settings.casual_mode);
                                            app_state = AppState::Title(menu::title_menu(
                                                has_save,
                                                settings.casual_mode,
                                            ));
                                        }
                                    }
                                }
                                Some(MenuAction::SelectSlot(slot)) => {
                                    menu::draw_loading(&mut renderer);
                                    match saves.load_from_slot(slot) {
                                        Ok(mut loaded) => {
                                            loaded.log.add("Game loaded.");
                                            game_state = Some(loaded);
                                            autosave_buf = None;
                                            app_state = AppState::Playing;
                                        }
                                        Err(_) => {
                                            let has_save =
                                                saves.has_save_for_title(settings.casual_mode);
                                            app_state = AppState::Title(menu::title_menu(
                                                has_save,
                                                settings.casual_mode,
                                            ));
                                        }
                                    }
                                }
                                _ => {
                                    let has_save = saves.has_save_for_title(settings.casual_mode);
                                    app_state = AppState::Title(menu::title_menu(
                                        has_save,
                                        settings.casual_mode,
                                    ));
                                }
                            }
                        } else {
                            menu::draw_loading(&mut renderer);
                            match saves.load_autosave() {
                                Ok(mut loaded) => {
                                    loaded.log.add("Game loaded.");
                                    game_state = Some(loaded);
                                    autosave_buf = None;
                                    app_state = AppState::Playing;
                                }
                                Err(_) => {
                                    let has_save = saves.has_save_for_title(settings.casual_mode);
                                    app_state = AppState::Title(menu::title_menu(
                                        has_save,
                                        settings.casual_mode,
                                    ));
                                }
                            }
                        }
                    }
                    Some(MenuAction::Settings) => {
                        loop {
                            let mut settings_m = menu::settings_menu(&settings, Platform::Ssh);
                            match run_menu(&mut settings_m, &mut renderer, rx, parser)? {
                                Some(MenuAction::ToggleCasualMode) => {
                                    settings.casual_mode = !settings.casual_mode;
                                    saves.save_settings(&settings);
                                }
                                Some(MenuAction::ToggleShowExploredPct) => {
                                    settings.show_explored_pct = !settings.show_explored_pct;
                                    saves.save_settings(&settings);
                                }
                                Some(MenuAction::ToggleShowCoordinates) => {
                                    settings.show_coordinates = !settings.show_coordinates;
                                    saves.save_settings(&settings);
                                }
                                Some(MenuAction::ToggleShowKeybindHints) => {
                                    settings.show_keybind_hints = !settings.show_keybind_hints;
                                    saves.save_settings(&settings);
                                }
                                Some(MenuAction::ToggleShowCorpses) => {
                                    settings.show_corpses = !settings.show_corpses;
                                    saves.save_settings(&settings);
                                }
                                Some(MenuAction::ToggleViKeys) => {
                                    settings.vi_keys = !settings.vi_keys;
                                    saves.save_settings(&settings);
                                }
                                Some(MenuAction::ToggleNumpad) => {
                                    settings.numpad = !settings.numpad;
                                    saves.save_settings(&settings);
                                }
                                Some(MenuAction::CycleAnimationSpeed) => {
                                    settings.animation_speed_ms = match settings.animation_speed_ms
                                    {
                                        0 => 25,
                                        25 => 50,
                                        50 => 100,
                                        100 => 200,
                                        _ => 0,
                                    };
                                    saves.save_settings(&settings);
                                }
                                Some(MenuAction::CycleAutosaveFrequency) => {
                                    settings.autosave_frequency = match settings.autosave_frequency
                                    {
                                        1 => 5,
                                        5 => 10,
                                        10 => 25,
                                        _ => 1,
                                    };
                                    saves.save_settings(&settings);
                                }
                                Some(MenuAction::CycleMessageLogLines) => {
                                    settings.message_log_lines = match settings.message_log_lines {
                                        2 => 4,
                                        4 => 6,
                                        6 => 8,
                                        _ => 2,
                                    };
                                    saves.save_settings(&settings);
                                }
                                Some(MenuAction::CycleColorPalette) => {
                                    settings.color_palette = settings.color_palette.next();
                                    saves.save_settings(&settings);
                                }
                                Some(MenuAction::CycleLeftHandLayout) => {
                                    settings.left_hand_layout = settings.left_hand_layout.next();
                                    saves.save_settings(&settings);
                                }
                                Some(MenuAction::EditPlayerName) => {
                                    if let Some(name) = text_input_dialog(
                                        renderer.writer(),
                                        rx,
                                        parser,
                                        "Enter Your Name",
                                        "Letters, digits, hyphens, spaces, underscores | Esc to cancel",
                                        cols,
                                        rows,
                                    )? {
                                        settings.player_name = name;
                                        saves.save_settings(&settings);
                                    }
                                }
                                Some(MenuAction::CyclePronouns) => {
                                    settings.pronouns = settings.pronouns.next();
                                    saves.save_settings(&settings);
                                }
                                _ => break,
                            }
                        }
                        let has_save = saves.has_save_for_title(settings.casual_mode);
                        app_state =
                            AppState::Title(menu::title_menu(has_save, settings.casual_mode));
                    }
                    Some(MenuAction::Quit | MenuAction::Back) => break 'app,
                    _ => {}
                }
            }

            AppState::Playing => {
                let state = game_state.as_mut().expect("no game state while playing");

                // Flush autosave buffer
                if let Some(ref buf) = autosave_buf {
                    let mut meta = state.extract_metadata();
                    if !settings.player_name.is_empty() {
                        meta.player_name = Some(settings.player_name.clone());
                    }
                    saves.write_autosave(buf, &meta);
                    autosave_buf = None;
                }

                render::render(renderer.writer(), state, cols, rows, &settings)?;

                if state.game_over {
                    let _ = wait_for_key(rx, parser)?;
                    saves.delete_autosave();
                    game_state = None;
                    autosave_buf = None;
                    let has_save = saves.has_save_for_title(settings.casual_mode);
                    app_state = AppState::Title(menu::title_menu(has_save, settings.casual_mode));
                    continue;
                }

                let key = match wait_for_key(rx, parser)? {
                    Some(k) => k,
                    None => break 'app,
                };

                // Message history (Ctrl+P)
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('p') {
                    run_message_history(state.log.all(), &mut renderer, rx, parser)?;
                    continue;
                }

                let cmd = input::translate_key(key, &settings);

                if let Some(cmd) = cmd {
                    match cmd {
                        GameCommand::Quit => {
                            app_state = AppState::Paused(menu::pause_menu(settings.casual_mode));
                        }
                        GameCommand::Look => {
                            let look_opts = LookOptions::default();
                            run_look_mode(
                                state,
                                &mut renderer,
                                cols,
                                rows,
                                &settings,
                                &look_opts,
                                rx,
                                parser,
                            )?;
                        }
                        GameCommand::Help => {
                            let lines = help::help_lines(&settings, &game_data);
                            run_message_history(&lines, &mut renderer, rx, parser)?;
                        }
                        GameCommand::Autorun { dx, dy } => {
                            let stepper = state.start_autorun(dx, dy);
                            animate_stepper(
                                renderer.writer(),
                                state,
                                stepper,
                                cols,
                                rows,
                                &settings,
                                rx,
                                parser,
                            )?;
                            if state.dirty
                                && let Ok(json) = state.save_to_json()
                            {
                                state.dirty = false;
                                autosave_buf = Some(json);
                            }
                        }
                        GameCommand::AutoExplore => match state.start_auto_explore() {
                            Ok((stepper, _tx, _ty)) => {
                                animate_stepper(
                                    renderer.writer(),
                                    state,
                                    stepper,
                                    cols,
                                    rows,
                                    &settings,
                                    rx,
                                    parser,
                                )?;
                                if state.dirty
                                    && let Ok(json) = state.save_to_json()
                                {
                                    state.dirty = false;
                                    autosave_buf = Some(json);
                                }
                            }
                            Err(_) => {
                                state.log.add("No unexplored areas reachable.");
                            }
                        },
                        _ => {
                            state.step(cmd);
                            if state.dirty
                                && (state.turn_count as u32)
                                    .is_multiple_of(settings.autosave_frequency)
                                && let Ok(json) = state.save_to_json()
                            {
                                state.dirty = false;
                                autosave_buf = Some(json);
                            }
                        }
                    }
                }
            }

            AppState::Paused(pause) => {
                let action = run_menu(pause, &mut renderer, rx, parser)?;
                match action {
                    Some(MenuAction::ResumeGame | MenuAction::Back) | None => {
                        if action.is_none() {
                            break 'app; // Disconnected
                        }
                        app_state = AppState::Playing;
                    }
                    Some(MenuAction::SaveGame) => {
                        if let Some(ref state) = game_state {
                            let slots = saves.load_all_slot_metadata();
                            let mut slot_menu =
                                menu::save_slot_menu(&slots, settings.show_explored_pct);
                            match run_menu(&mut slot_menu, &mut renderer, rx, parser)? {
                                Some(MenuAction::SelectSlot(slot)) => {
                                    let msg =
                                        saves.save_to_slot(state, slot, &settings.player_name);
                                    let mut new_pause = menu::pause_menu(settings.casual_mode);
                                    new_pause.selected = 1;
                                    app_state = AppState::Paused(new_pause);
                                    if let Some(ref mut state) = game_state {
                                        state.log.add(&msg);
                                    }
                                }
                                _ => {
                                    let mut new_pause = menu::pause_menu(settings.casual_mode);
                                    new_pause.selected = 1;
                                    app_state = AppState::Paused(new_pause);
                                }
                            }
                        }
                    }
                    Some(MenuAction::LoadGame) => {
                        let slots = saves.load_all_slot_metadata();
                        let auto_meta = saves.load_autosave_metadata();
                        let has_auto = saves.has_autosave();
                        let mut load_m = menu::load_slot_menu(
                            has_auto,
                            &auto_meta,
                            &slots,
                            settings.show_explored_pct,
                        );
                        match run_menu(&mut load_m, &mut renderer, rx, parser)? {
                            Some(MenuAction::LoadGame) => {
                                menu::draw_loading(&mut renderer);
                                match saves.load_autosave() {
                                    Ok(mut loaded) => {
                                        loaded.log.add("Game loaded.");
                                        game_state = Some(loaded);
                                        autosave_buf = None;
                                        app_state = AppState::Playing;
                                    }
                                    Err(msg) => {
                                        if let Some(ref mut state) = game_state {
                                            state.log.add(&msg);
                                        }
                                        let mut new_pause = menu::pause_menu(settings.casual_mode);
                                        new_pause.selected = 2;
                                        app_state = AppState::Paused(new_pause);
                                    }
                                }
                            }
                            Some(MenuAction::SelectSlot(slot)) => {
                                menu::draw_loading(&mut renderer);
                                match saves.load_from_slot(slot) {
                                    Ok(mut loaded) => {
                                        loaded.log.add("Game loaded.");
                                        game_state = Some(loaded);
                                        autosave_buf = None;
                                        app_state = AppState::Playing;
                                    }
                                    Err(msg) => {
                                        if let Some(ref mut state) = game_state {
                                            state.log.add(&msg);
                                        }
                                        let mut new_pause = menu::pause_menu(settings.casual_mode);
                                        new_pause.selected = 2;
                                        app_state = AppState::Paused(new_pause);
                                    }
                                }
                            }
                            _ => {
                                let mut new_pause = menu::pause_menu(settings.casual_mode);
                                new_pause.selected = 2;
                                app_state = AppState::Paused(new_pause);
                            }
                        }
                    }
                    Some(MenuAction::TitleScreen) => {
                        game_state = None;
                        autosave_buf = None;
                        let has_save = saves.has_save_for_title(settings.casual_mode);
                        app_state =
                            AppState::Title(menu::title_menu(has_save, settings.casual_mode));
                    }
                    Some(MenuAction::Quit) => break 'app,
                    _ => {}
                }
            }
        }
    }

    tracing::info!(username, "Session ended");
    Ok(())
}

fn show_resize_prompt<W: Write>(w: &mut W, cols: i32, rows: i32) -> std::io::Result<()> {
    use crossterm::{cursor, queue, style, terminal};
    queue!(
        w,
        terminal::Clear(terminal::ClearType::All),
        cursor::MoveTo(0, 0),
        style::SetForegroundColor(crossterm::style::Color::Yellow),
        style::SetBackgroundColor(crossterm::style::Color::Black),
        style::Print(format!(
            "Terminal too small: {}x{} (need {}x{}). Please resize.",
            cols, rows, MIN_WIDTH, MIN_HEIGHT
        ))
    )?;
    w.flush()?;
    Ok(())
}

fn run_menu(
    menu: &mut menu::Menu,
    renderer: &mut dyn Renderer,
    rx: &Receiver<Vec<u8>>,
    parser: &mut AnsiParser,
) -> std::io::Result<Option<MenuAction>> {
    loop {
        menu.draw(renderer);

        let key = match wait_for_key(rx, parser)? {
            Some(k) => k,
            None => return Ok(None),
        };

        if let Some(cmd) = input::translate_menu_key(key)
            && let Some(action) = menu.handle_input(cmd)
        {
            return Ok(Some(action));
        }
    }
}

fn run_message_history(
    messages: &[String],
    renderer: &mut dyn Renderer,
    rx: &Receiver<Vec<u8>>,
    parser: &mut AnsiParser,
) -> std::io::Result<()> {
    let mut viewer = MessageHistoryViewer::new(messages);
    loop {
        viewer.draw(renderer);

        let (_, screen_h) = renderer.screen_size();
        let page_size = (screen_h - 2).max(1) as usize;

        let key = match wait_for_key(rx, parser)? {
            Some(k) => k,
            None => return Ok(()),
        };

        match key.code {
            KeyCode::PageUp => viewer.page_up(page_size),
            KeyCode::PageDown => viewer.page_down(page_size),
            KeyCode::Home => viewer.scroll_up(usize::MAX),
            KeyCode::End => viewer.scroll_down(usize::MAX),
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                viewer.page_up(page_size / 2);
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                viewer.page_down(page_size / 2);
            }
            _ => {
                if let Some(cmd) = input::translate_menu_key(key)
                    && viewer.handle_input(cmd) == ViewerAction::Close
                {
                    return Ok(());
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_look_mode<W: Write>(
    state: &game::GameState,
    renderer: &mut render::CrosstermRenderer<W>,
    cols: Coord,
    rows: Coord,
    settings: &settings::Settings,
    look_opts: &LookOptions,
    rx: &Receiver<Vec<u8>>,
    parser: &mut AnsiParser,
) -> std::io::Result<()> {
    let player = &state.entities[0];
    let mut cursor = LookCursor::new(player.x, player.y);

    loop {
        render::render(renderer.writer(), state, cols, rows, settings)?;
        let info = cursor.current_info_with(state, look_opts);
        cursor.draw_overlay(renderer, &info, rows, settings.message_log_lines as Coord);
        renderer.flush();

        let key = match wait_for_key(rx, parser)? {
            Some(k) => k,
            None => return Ok(()),
        };

        if let Some(cmd) = input::translate_look_key(key, settings)
            && cursor.handle_input(cmd, state) == LookAction::Close
        {
            return Ok(());
        }
    }
}

/// Animated autorun/auto-explore with frame pacing.
#[allow(clippy::too_many_arguments)]
fn animate_stepper<W: Write>(
    stdout: &mut W,
    state: &mut game::GameState,
    mut stepper: game::AutorunStepper,
    cols: Coord,
    rows: Coord,
    settings: &settings::Settings,
    rx: &Receiver<Vec<u8>>,
    parser: &mut AnsiParser,
) -> std::io::Result<()> {
    loop {
        match stepper.next_step(state) {
            game::StepOutcome::Continue => {
                render::render(stdout, state, cols, rows, settings)?;
                // Frame pacing + interrupt detection via timeout recv.
                let timeout = Duration::from_millis(settings.animation_speed_ms as u64);
                match rx.recv_timeout(timeout) {
                    Ok(data) => {
                        // Any input interrupts the animation
                        for &byte in &data {
                            let events = parser.feed(byte);
                            if !events.is_empty() {
                                return Ok(());
                            }
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                }
            }
            game::StepOutcome::Done(_) => return Ok(()),
        }
    }
}

/// Text input dialog (same as terminal's but using SSH input).
fn text_input_dialog<W: Write>(
    w: &mut W,
    rx: &Receiver<Vec<u8>>,
    parser: &mut AnsiParser,
    prompt: &str,
    hint: &str,
    width: Coord,
    height: Coord,
) -> std::io::Result<Option<String>> {
    use crossterm::{cursor, queue, style, terminal};
    let mut input_buf = String::new();
    loop {
        let cx = width / 2;
        let cy = height / 2;

        queue!(
            w,
            terminal::Clear(terminal::ClearType::All),
            cursor::MoveTo(0, 0)
        )?;

        let px = (cx - prompt.len() as i32 / 2).max(0);
        queue!(
            w,
            cursor::MoveTo(px as u16, (cy - 2) as u16),
            style::SetForegroundColor(crossterm::style::Color::Cyan),
            style::SetBackgroundColor(crossterm::style::Color::Black),
            style::Print(prompt)
        )?;

        let display = format!("> {}_", input_buf);
        let ix = (cx - display.len() as i32 / 2).max(0);
        queue!(
            w,
            cursor::MoveTo(ix as u16, cy as u16),
            style::SetForegroundColor(crossterm::style::Color::Yellow),
            style::SetBackgroundColor(crossterm::style::Color::Black),
            style::Print(&display)
        )?;

        let hx = (cx - hint.len() as i32 / 2).max(0);
        queue!(
            w,
            cursor::MoveTo(hx as u16, (cy + 2) as u16),
            style::SetForegroundColor(crossterm::style::Color::DarkGrey),
            style::SetBackgroundColor(crossterm::style::Color::Black),
            style::Print(hint)
        )?;

        w.flush()?;

        let key = match wait_for_key(rx, parser)? {
            Some(k) => k,
            None => return Ok(None),
        };

        match key.code {
            KeyCode::Esc => return Ok(None),
            KeyCode::Enter => return Ok(Some(input_buf)),
            KeyCode::Backspace => {
                input_buf.pop();
            }
            KeyCode::Char(c) if c.is_ascii_alphanumeric() || c == '-' || c == ' ' || c == '_' => {
                input_buf.push(c);
            }
            _ => {}
        }
    }
}
