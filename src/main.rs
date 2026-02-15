use std::io::stdout;
use std::time::Duration;

use roguelike::{
    data, game, input, menu, menu::MenuAction, platform::Renderer, render, types::Coord,
};

use crossterm::{
    cursor,
    event::{self, Event, KeyEvent, KeyEventKind},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};

const SAVE_FILE: &str = "savegame.json";

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

fn main() -> std::io::Result<()> {
    let mut stdout = stdout();

    terminal::enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, cursor::Hide)?;

    let mut renderer = render::CrosstermRenderer::new(std::io::stdout());
    let (cols, rows) = terminal::size()?;
    let map_height = (rows as i32) - data::CONFIG.ui_bottom_rows;

    let mut game_state: Option<game::GameState> = None;
    let has_save = std::path::Path::new(SAVE_FILE).exists();
    let mut app_state = AppState::Title(menu::title_menu(has_save));

    'app: loop {
        match &mut app_state {
            AppState::Title(title) => match run_menu(title, &mut renderer)? {
                MenuAction::NewGame => {
                    game_state = Some(game::GameState::new(cols as i32, map_height));
                    app_state = AppState::Playing;
                }
                MenuAction::LoadGame => {
                    menu::draw_loading(&mut renderer);
                    match load_game() {
                        Ok(mut loaded) => {
                            loaded.log.add("Game loaded.");
                            game_state = Some(loaded);
                            app_state = AppState::Playing;
                        }
                        Err(msg) => {
                            // Shouldn't normally happen (button is disabled when
                            // no save exists), but handle gracefully.
                            let _ = msg;
                            let has_save = std::path::Path::new(SAVE_FILE).exists();
                            app_state = AppState::Title(menu::title_menu(has_save));
                        }
                    }
                }
                MenuAction::Quit | MenuAction::Back => break 'app,
                _ => {}
            },

            AppState::Playing => {
                let state = game_state.as_mut().expect("no game state while playing");

                render::render(&mut stdout, state, cols as i32, rows as i32)?;

                if state.game_over {
                    // Game-over: any key returns to title.
                    wait_for_keypress()?;
                    game_state = None;
                    let has_save = std::path::Path::new(SAVE_FILE).exists();
                    app_state = AppState::Title(menu::title_menu(has_save));
                    continue;
                }

                let key = wait_for_keypress()?;
                if let Some(cmd) = input::translate_key(key) {
                    match cmd {
                        input::GameCommand::Quit => {
                            app_state = AppState::Paused(menu::pause_menu());
                        }

                        input::GameCommand::Autorun { dx, dy } => {
                            let stepper = state.start_autorun(dx, dy);
                            animate_stepper(&mut stdout, state, stepper, cols as i32, rows as i32)?;
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
                            }
                            Err(_) => {
                                state.log.add("No unexplored areas reachable.");
                            }
                        },

                        _ => {
                            state.step(cmd);
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
                            // Show save result briefly on the pause menu.
                            // Re-open pause menu so the player sees the result.
                            let mut new_pause = menu::pause_menu();
                            // Keep selection on Save Game so the context is clear.
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
                                app_state = AppState::Playing;
                            }
                            Err(msg) => {
                                if let Some(ref mut state) = game_state {
                                    state.log.add(&msg);
                                }
                                let mut new_pause = menu::pause_menu();
                                new_pause.selected = 2;
                                app_state = AppState::Paused(new_pause);
                            }
                        }
                    }
                    MenuAction::MainMenu => {
                        game_state = None;
                        let has_save = std::path::Path::new(SAVE_FILE).exists();
                        app_state = AppState::Title(menu::title_menu(has_save));
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
