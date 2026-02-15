use std::io::stdout;
use std::time::Duration;

use roguelike::{
    data, game, input, menu, menu::MenuAction, platform::Renderer, render, saves::SlotMetadata,
    settings, types::Coord,
};

use crossterm::{
    cursor,
    event::{self, Event, KeyEvent, KeyEventKind},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};

const SAVE_FILE: &str = "savegame.json";
const AUTOSAVE_META_FILE: &str = "savegame.meta.json";
const SETTINGS_FILE: &str = "settings.json";
const NUM_SLOTS: u8 = 5;

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
    show_explored_pct: bool,
) -> std::io::Result<()> {
    loop {
        match stepper.next_step(state) {
            game::StepOutcome::Continue => {
                render::render(stdout, state, cols, rows, show_explored_pct)?;
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

// --- Save-slot helpers (casual mode) ---

fn slot_save_path(slot: u8) -> String {
    format!("savegame_{}.json", slot + 1)
}

fn slot_meta_path(slot: u8) -> String {
    format!("savegame_{}.meta.json", slot + 1)
}

fn write_metadata(path: &str, meta: &SlotMetadata) {
    if let Ok(json) = serde_json::to_string(meta) {
        let _ = std::fs::write(path, json);
    }
}

fn save_to_slot(state: &game::GameState, slot: u8) -> String {
    match state.save_to_json() {
        Ok(json) => match std::fs::write(slot_save_path(slot), json) {
            Ok(()) => {
                write_metadata(&slot_meta_path(slot), &state.extract_metadata());
                "Game saved.".to_string()
            }
            Err(e) => format!("Save failed: {e}"),
        },
        Err(e) => format!("Save failed: {e}"),
    }
}

fn load_from_slot(slot: u8) -> Result<game::GameState, String> {
    let json =
        std::fs::read_to_string(slot_save_path(slot)).map_err(|e| format!("Load failed: {e}"))?;
    game::GameState::load_from_json(&json).map_err(|e| format!("Load failed: {e}"))
}

fn load_slot_metadata(slot: u8) -> Option<SlotMetadata> {
    std::fs::read_to_string(slot_meta_path(slot))
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
}

fn load_all_slot_metadata() -> [Option<SlotMetadata>; 5] {
    [
        load_slot_metadata(0),
        load_slot_metadata(1),
        load_slot_metadata(2),
        load_slot_metadata(3),
        load_slot_metadata(4),
    ]
}

fn load_autosave_metadata() -> Option<SlotMetadata> {
    std::fs::read_to_string(AUTOSAVE_META_FILE)
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
}

/// Check if any save exists: autosave or any slot.
fn has_any_save() -> bool {
    if std::path::Path::new(SAVE_FILE).exists() {
        return true;
    }
    (0..NUM_SLOTS).any(|i| std::path::Path::new(&slot_save_path(i)).exists())
}

/// Check if a save exists for the title "Load Game" button.
/// Classic: only autosave. Casual: autosave or any slot.
fn has_save_for_title(casual_mode: bool) -> bool {
    if casual_mode {
        has_any_save()
    } else {
        std::path::Path::new(SAVE_FILE).exists()
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
    let has_save = has_save_for_title(settings.casual_mode);
    let mut app_state = AppState::Title(menu::title_menu(has_save, settings.casual_mode));

    'app: loop {
        match &mut app_state {
            AppState::Title(title) => match run_menu(title, &mut renderer)? {
                MenuAction::NewGame => {
                    let autosave_exists = std::path::Path::new(SAVE_FILE).exists();
                    if autosave_exists {
                        let msg = if settings.casual_mode {
                            "Start new game? Manual saves will be kept."
                        } else {
                            "This will abandon your saved game."
                        };
                        let mut confirm = menu::confirm_menu(msg);
                        match run_menu(&mut confirm, &mut renderer)? {
                            MenuAction::Confirm => {
                                let _ = std::fs::remove_file(SAVE_FILE);
                                let _ = std::fs::remove_file(AUTOSAVE_META_FILE);
                            }
                            _ => {
                                let has_save = has_save_for_title(settings.casual_mode);
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
                    if settings.casual_mode {
                        // Show load-slot picker.
                        let slots = load_all_slot_metadata();
                        let auto_meta = load_autosave_metadata();
                        let has_auto = std::path::Path::new(SAVE_FILE).exists();
                        let mut load_menu = menu::load_slot_menu(
                            has_auto,
                            &auto_meta,
                            &slots,
                            settings.show_explored_pct,
                        );
                        match run_menu(&mut load_menu, &mut renderer)? {
                            MenuAction::LoadGame => {
                                // Load autosave.
                                menu::draw_loading(&mut renderer);
                                match load_game() {
                                    Ok(mut loaded) => {
                                        loaded.log.add("Game loaded.");
                                        game_state = Some(loaded);
                                        autosave_buf = None;
                                        app_state = AppState::Playing;
                                    }
                                    Err(_) => {
                                        let has_save = has_save_for_title(settings.casual_mode);
                                        app_state = AppState::Title(menu::title_menu(
                                            has_save,
                                            settings.casual_mode,
                                        ));
                                    }
                                }
                            }
                            MenuAction::SelectSlot(slot) => {
                                menu::draw_loading(&mut renderer);
                                match load_from_slot(slot) {
                                    Ok(mut loaded) => {
                                        loaded.log.add("Game loaded.");
                                        game_state = Some(loaded);
                                        autosave_buf = None;
                                        app_state = AppState::Playing;
                                    }
                                    Err(_) => {
                                        let has_save = has_save_for_title(settings.casual_mode);
                                        app_state = AppState::Title(menu::title_menu(
                                            has_save,
                                            settings.casual_mode,
                                        ));
                                    }
                                }
                            }
                            _ => {
                                // Back — return to title.
                                let has_save = has_save_for_title(settings.casual_mode);
                                app_state = AppState::Title(menu::title_menu(
                                    has_save,
                                    settings.casual_mode,
                                ));
                            }
                        }
                    } else {
                        // Classic mode — load autosave directly.
                        menu::draw_loading(&mut renderer);
                        match load_game() {
                            Ok(mut loaded) => {
                                loaded.log.add("Game loaded.");
                                game_state = Some(loaded);
                                autosave_buf = None;
                                app_state = AppState::Playing;
                            }
                            Err(_) => {
                                let has_save = has_save_for_title(settings.casual_mode);
                                app_state = AppState::Title(menu::title_menu(
                                    has_save,
                                    settings.casual_mode,
                                ));
                            }
                        }
                    }
                }
                MenuAction::Settings => {
                    loop {
                        let mut settings_m =
                            menu::settings_menu(settings.casual_mode, settings.show_explored_pct);
                        match run_menu(&mut settings_m, &mut renderer)? {
                            MenuAction::ToggleCasualMode => {
                                settings.casual_mode = !settings.casual_mode;
                                save_settings(&settings);
                            }
                            MenuAction::ToggleShowExploredPct => {
                                settings.show_explored_pct = !settings.show_explored_pct;
                                save_settings(&settings);
                            }
                            _ => break,
                        }
                    }
                    let has_save = has_save_for_title(settings.casual_mode);
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
                    // Write sidecar metadata (both modes — so switching to casual
                    // has metadata immediately available).
                    write_metadata(AUTOSAVE_META_FILE, &state.extract_metadata());
                    autosave_buf = None;
                }

                render::render(
                    &mut stdout,
                    state,
                    cols as i32,
                    rows as i32,
                    settings.show_explored_pct,
                )?;

                if state.game_over {
                    // Game-over: any key returns to title.
                    wait_for_keypress()?;
                    // Dead game shouldn't be resumable — delete autosave + meta.
                    // Slot saves are preserved (casual mode can load from them).
                    let _ = std::fs::remove_file(SAVE_FILE);
                    let _ = std::fs::remove_file(AUTOSAVE_META_FILE);
                    game_state = None;
                    autosave_buf = None;
                    let has_save = has_save_for_title(settings.casual_mode);
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
                            animate_stepper(
                                &mut stdout,
                                state,
                                stepper,
                                cols as i32,
                                rows as i32,
                                settings.show_explored_pct,
                            )?;
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
                                    settings.show_explored_pct,
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
                            // Show save-slot picker.
                            let slots = load_all_slot_metadata();
                            let mut slot_menu =
                                menu::save_slot_menu(&slots, settings.show_explored_pct);
                            match run_menu(&mut slot_menu, &mut renderer)? {
                                MenuAction::SelectSlot(slot) => {
                                    let msg = save_to_slot(state, slot);
                                    let mut new_pause = menu::pause_menu(settings.casual_mode);
                                    new_pause.selected = 1;
                                    app_state = AppState::Paused(new_pause);
                                    if let Some(ref mut state) = game_state {
                                        state.log.add(&msg);
                                    }
                                }
                                _ => {
                                    // Back — return to pause menu.
                                    let mut new_pause = menu::pause_menu(settings.casual_mode);
                                    new_pause.selected = 1;
                                    app_state = AppState::Paused(new_pause);
                                }
                            }
                        }
                    }
                    MenuAction::LoadGame => {
                        // Show load-slot picker (casual mode only reaches here).
                        let slots = load_all_slot_metadata();
                        let auto_meta = load_autosave_metadata();
                        let has_auto = std::path::Path::new(SAVE_FILE).exists();
                        let mut load_m = menu::load_slot_menu(
                            has_auto,
                            &auto_meta,
                            &slots,
                            settings.show_explored_pct,
                        );
                        match run_menu(&mut load_m, &mut renderer)? {
                            MenuAction::LoadGame => {
                                // Load autosave.
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
                            MenuAction::SelectSlot(slot) => {
                                menu::draw_loading(&mut renderer);
                                match load_from_slot(slot) {
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
                                // Back — return to pause menu.
                                let mut new_pause = menu::pause_menu(settings.casual_mode);
                                new_pause.selected = 2;
                                app_state = AppState::Paused(new_pause);
                            }
                        }
                    }
                    MenuAction::TitleScreen => {
                        game_state = None;
                        autosave_buf = None;
                        let has_save = has_save_for_title(settings.casual_mode);
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
