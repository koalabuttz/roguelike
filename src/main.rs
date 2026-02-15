use std::io::stdout;
use std::time::Duration;

use roguelike::{
    data, game, input, menu, menu::MenuAction, platform::Renderer, render, settings, types::Coord,
};

use crossterm::{
    cursor,
    event::{self, Event, KeyEvent, KeyEventKind},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};

const SAVE_FILE: &str = "savegame.json";
const SETTINGS_FILE: &str = "settings.json";

/// Top-level application state machine.
enum AppState {
    Title(menu::Menu),
    Playing,
    Paused(menu::Menu),
}

/// Run a multi-step sequence with animation: render each frame and
/// check for keypress interrupts between steps.
fn animate_stepper(
    stdout: &mut impl std::io::Write,
    state: &mut game::GameState,
    mut stepper: game::AutorunStepper,
    cols: Coord,
    rows: Coord,
) -> std::io::Result<()> {
    loop {
        match stepper.next_step(state) {
            game::StepOutcome::Continue => {
                render::render(stdout, state, cols, rows)?;
                // 50ms frame pacing + interrupt detection.
                if event::poll(Duration::from_millis(50))? {
                    let _ = event::read()?;
                    return Ok(());
                }
            }
            game::StepOutcome::Done(_) => return Ok(()),
        }
    }
}

/// Wait for a key-press event, ignoring releases and non-key events.
fn wait_for_keypress() -> std::io::Result<KeyEvent> {
    loop {
        if let Event::Key(
            key @ KeyEvent {
                kind: KeyEventKind::Press,
                ..
            },
        ) = event::read()?
        {
            return Ok(key);
        }
    }
}

/// Run the title or pause menu loop. Returns the selected action.
fn run_menu(menu: &mut menu::Menu, renderer: &mut dyn Renderer) -> std::io::Result<MenuAction> {
    loop {
        menu.draw(renderer);

        let key = wait_for_keypress()?;
        if let Some(cmd) = input::translate_menu_key(key)
            && let Some(action) = menu.handle_input(cmd)
        {
            return Ok(action);
        }
    }
}

/// Save the game state to disk. Returns a message for the log.
fn save_game(state: &game::GameState) -> String {
    match state.save_to_json() {
        Ok(json) => match std::fs::write(SAVE_FILE, json) {
            Ok(()) => "Game saved.".to_string(),
            Err(e) => format!("Save failed: {e}"),
        },
        Err(e) => format!("Save failed: {e}"),
    }
}

/// Load a game state from disk.
fn load_game() -> Result<game::GameState, String> {
    let json = std::fs::read_to_string(SAVE_FILE).map_err(|e| format!("Load failed: {e}"))?;
    game::GameState::load_from_json(&json).map_err(|e| format!("Load failed: {e}"))
}

/// Load settings from disk, falling back to defaults if the file is missing
/// or malformed.
fn load_settings() -> settings::Settings {
    std::fs::read_to_string(SETTINGS_FILE)
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

/// Persist settings to disk. Errors are silently ignored — settings are
/// non-critical and will fall back to defaults on next launch.
fn save_settings(settings: &settings::Settings) {
    if let Ok(json) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(SETTINGS_FILE, json);
    }
}

fn main() -> std::io::Result<()> {
    let mut stdout = stdout();

    terminal::enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, cursor::Hide)?;

    let mut renderer = render::CrosstermRenderer::new(std::io::stdout());
    let (cols, rows) = terminal::size()?;
    let map_height = (rows as i32) - data::CONFIG.ui_bottom_rows;

    let mut settings = load_settings();
    let mut game_state: Option<game::GameState> = None;
    let mut autosave_buf: Option<String> = None;
    let has_save = std::path::Path::new(SAVE_FILE).exists();
    let mut app_state = AppState::Title(menu::title_menu(has_save, settings.casual_mode));

    'app: loop {
        match &mut app_state {
            AppState::Title(title) => match run_menu(title, &mut renderer)? {
                MenuAction::NewGame => {
                    let has_save = std::path::Path::new(SAVE_FILE).exists();
                    if has_save {
                        let mut confirm = menu::confirm_menu("This will abandon your saved game.");
                        match run_menu(&mut confirm, &mut renderer)? {
                            MenuAction::Confirm => {
                                let _ = std::fs::remove_file(SAVE_FILE);
                            }
                            _ => {
                                let has_save = std::path::Path::new(SAVE_FILE).exists();
                                app_state = AppState::Title(menu::title_menu(
                                    has_save,
                                    settings.casual_mode,
                                ));
                                continue;
                            }
                        }
                    }
                    game_state = Some(game::GameState::new(cols as i32, map_height));
                    autosave_buf = None;
                    app_state = AppState::Playing;
                }
                MenuAction::LoadGame => {
                    menu::draw_loading(&mut renderer);
                    match load_game() {
                        Ok(mut loaded) => {
                            if !settings.casual_mode {
                                let _ = std::fs::remove_file(SAVE_FILE);
                            }
                            loaded.log.add("Game loaded.");
                            game_state = Some(loaded);
                            autosave_buf = None;
                            app_state = AppState::Playing;
                        }
                        Err(msg) => {
                            let _ = msg;
                            let has_save = std::path::Path::new(SAVE_FILE).exists();
                            app_state =
                                AppState::Title(menu::title_menu(has_save, settings.casual_mode));
                        }
                    }
                }
                MenuAction::Settings => {
                    loop {
                        let mut settings_m = menu::settings_menu(settings.casual_mode);
                        match run_menu(&mut settings_m, &mut renderer)? {
                            MenuAction::ToggleCasualMode => {
                                settings.casual_mode = !settings.casual_mode;
                                save_settings(&settings);
                            }
                            _ => break,
                        }
                    }
                    let has_save = std::path::Path::new(SAVE_FILE).exists();
                    app_state = AppState::Title(menu::title_menu(has_save, settings.casual_mode));
                }
                MenuAction::Quit | MenuAction::Back => break 'app,
                _ => {}
            },

            AppState::Playing => {
                let state = game_state.as_mut().expect("no game state while playing");

                // Flush autosave buffer to disk during input wait.
                if let Some(ref buf) = autosave_buf {
                    let _ = std::fs::write(SAVE_FILE, buf);
                    autosave_buf = None;
                }

                render::render(&mut stdout, state, cols as i32, rows as i32)?;

                if state.game_over {
                    // Game-over: any key returns to title.
                    wait_for_keypress()?;
                    // Dead game shouldn't be resumable — delete the save.
                    let _ = std::fs::remove_file(SAVE_FILE);
                    game_state = None;
                    autosave_buf = None;
                    let has_save = std::path::Path::new(SAVE_FILE).exists();
                    app_state = AppState::Title(menu::title_menu(has_save, settings.casual_mode));
                    continue;
                }

                let key = wait_for_keypress()?;
                if let Some(cmd) = input::translate_key(key) {
                    match cmd {
                        input::GameCommand::Quit => {
                            app_state = AppState::Paused(menu::pause_menu(settings.casual_mode));
                        }

                        input::GameCommand::Autorun { dx, dy } => {
                            let stepper = state.start_autorun(dx, dy);
                            animate_stepper(&mut stdout, state, stepper, cols as i32, rows as i32)?;
                            // Autosave after autorun completes.
                            if let Ok(json) = state.save_to_json() {
                                autosave_buf = Some(json);
                            }
                        }

                        input::GameCommand::AutoExplore => match state.start_auto_explore() {
                            Ok((stepper, _tx, _ty)) => {
                                animate_stepper(
                                    &mut stdout,
                                    state,
                                    stepper,
                                    cols as i32,
                                    rows as i32,
                                )?;
                                // Autosave after auto-explore completes.
                                if let Ok(json) = state.save_to_json() {
                                    autosave_buf = Some(json);
                                }
                            }
                            Err(_) => {
                                state.log.add("No unexplored areas reachable.");
                            }
                        },

                        _ => {
                            state.step(cmd);
                            // Autosave after each turn.
                            if let Ok(json) = state.save_to_json() {
                                autosave_buf = Some(json);
                            }
                        }
                    }
                }
            }

            AppState::Paused(pause) => {
                let action = run_menu(pause, &mut renderer)?;
                match action {
                    MenuAction::ResumeGame | MenuAction::Back => {
                        app_state = AppState::Playing;
                    }
                    MenuAction::SaveGame => {
                        if let Some(ref state) = game_state {
                            let msg = save_game(state);
                            let mut new_pause = menu::pause_menu(settings.casual_mode);
                            new_pause.selected = 1;
                            app_state = AppState::Paused(new_pause);
                            if let Some(ref mut state) = game_state {
                                state.log.add(&msg);
                            }
                        }
                    }
                    MenuAction::LoadGame => {
                        menu::draw_loading(&mut renderer);
                        match load_game() {
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
                    MenuAction::TitleScreen => {
                        game_state = None;
                        autosave_buf = None;
                        let has_save = std::path::Path::new(SAVE_FILE).exists();
                        app_state =
                            AppState::Title(menu::title_menu(has_save, settings.casual_mode));
                    }
                    MenuAction::Quit => break 'app,
                    _ => {}
                }
            }
        }
    }

    execute!(stdout, LeaveAlternateScreen, cursor::Show)?;
    terminal::disable_raw_mode()?;

    Ok(())
}
