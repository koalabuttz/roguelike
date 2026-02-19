use std::io::Write;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use crossterm::event::KeyCode;
use crossterm::style::Color;
use crossterm::{cursor, queue, style, terminal};

use roguelike_core::settings::Platform;

use roguelike_tui::game_loop::{self, GameLoopConfig, GameLoopResult, NoDevHooks};
use roguelike_tui::render;

use crate::ansi_input::AnsiParser;
use crate::lobby::wait_for_key;
use crate::saves::SaveManager;
use crate::ssh_input::SshInput;

const MIN_WIDTH: i32 = 60;
const MIN_HEIGHT: i32 = 20;

/// What caused the session to end.
pub enum SessionResult {
    /// Normal quit or disconnect — close the connection.
    Quit,
    /// User chose "Log Out" — return to the pre-login lobby.
    LogOut,
}

/// Run the full game session for a logged-in user.
///
/// Shows a server menu (Play, Watch, Log Out) and loops back to it when the
/// user selects "Lobby" from the game's title/pause menu. Returns `LogOut`
/// when the user wants to return to the pre-login lobby.
pub fn run_session<W: Write>(
    writer: &mut W,
    rx: &Receiver<Vec<u8>>,
    size_rx: &mut tokio::sync::watch::Receiver<(u32, u32)>,
    parser: &mut AnsiParser,
    saves: &SaveManager,
    username: &str,
) -> std::io::Result<SessionResult> {
    loop {
        let (cols, rows) = {
            let (w, h) = *size_rx.borrow();
            (w as i32, h as i32)
        };

        match run_server_menu(writer, rx, parser, username, cols, rows)? {
            ServerMenuChoice::Play => {
                match run_game(writer, rx, size_rx, parser, saves, username)? {
                    GameLoopResult::Lobby => continue, // Back to server menu
                    GameLoopResult::Quit => return Ok(SessionResult::Quit),
                }
            }
            ServerMenuChoice::LogOut => return Ok(SessionResult::LogOut),
            ServerMenuChoice::Disconnected => return Ok(SessionResult::Quit),
        }
    }
}

// -- Server menu (post-login) ------------------------------------------------

enum ServerMenuChoice {
    Play,
    LogOut,
    Disconnected,
}

#[derive(Clone, Copy, PartialEq)]
enum ServerMenuItem {
    Play,
    Watch,
    LogOut,
}

const SERVER_MENU_ITEMS: &[(ServerMenuItem, &str, bool)] = &[
    (ServerMenuItem::Play, "Play", true),
    (ServerMenuItem::Watch, "Watch a Game  (coming soon)", false),
    (ServerMenuItem::LogOut, "Log Out", true),
];

fn run_server_menu<W: Write>(
    w: &mut W,
    rx: &Receiver<Vec<u8>>,
    parser: &mut AnsiParser,
    username: &str,
    width: i32,
    height: i32,
) -> std::io::Result<ServerMenuChoice> {
    let mut selected: usize = 0;

    loop {
        draw_server_menu(w, width, height, selected, username)?;

        let key = match wait_for_key(rx, parser)? {
            Some(k) => k,
            None => return Ok(ServerMenuChoice::Disconnected),
        };

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if selected > 0 {
                    selected -= 1;
                } else {
                    selected = SERVER_MENU_ITEMS.len() - 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                selected = (selected + 1) % SERVER_MENU_ITEMS.len();
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                let (item, _, enabled) = SERVER_MENU_ITEMS[selected];
                if !enabled {
                    continue;
                }
                match item {
                    ServerMenuItem::Play => return Ok(ServerMenuChoice::Play),
                    ServerMenuItem::Watch => {} // coming soon
                    ServerMenuItem::LogOut => return Ok(ServerMenuChoice::LogOut),
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => return Ok(ServerMenuChoice::LogOut),
            _ => {}
        }
    }
}

fn draw_server_menu<W: Write>(
    w: &mut W,
    width: i32,
    height: i32,
    selected: usize,
    username: &str,
) -> std::io::Result<()> {
    queue!(
        w,
        terminal::Clear(terminal::ClearType::All),
        cursor::MoveTo(0, 0)
    )?;

    let cx = width / 2;
    let top = height / 4;

    // Title
    let title = "ROGUELIKE SSH SERVER";
    let tx = (cx - title.len() as i32 / 2).max(0);
    queue!(
        w,
        cursor::MoveTo(tx as u16, top as u16),
        style::SetForegroundColor(Color::Cyan),
        style::SetBackgroundColor(Color::Black),
        style::Print(title)
    )?;

    // Welcome message
    let welcome = format!("Welcome, {}!", username);
    let wx = (cx - welcome.len() as i32 / 2).max(0);
    queue!(
        w,
        cursor::MoveTo(wx as u16, (top + 2) as u16),
        style::SetForegroundColor(Color::White),
        style::SetBackgroundColor(Color::Black),
        style::Print(&welcome)
    )?;

    // Menu items — left-justified as a block, block centered on screen.
    let max_item_width = SERVER_MENU_ITEMS
        .iter()
        .map(|(_, label, _)| label.len() as i32 + 2)
        .max()
        .unwrap_or(0);
    let items_x = (cx - max_item_width / 2).max(0);
    let items_y = top + 5;

    for (i, (_, label, enabled)) in SERVER_MENU_ITEMS.iter().enumerate() {
        let y = items_y + i as i32;
        let prefix = if i == selected { "> " } else { "  " };
        let text = format!("{}{}", prefix, label);

        let fg = if !enabled {
            Color::DarkGrey
        } else if i == selected {
            Color::Yellow
        } else {
            Color::White
        };

        queue!(
            w,
            cursor::MoveTo(items_x as u16, y as u16),
            style::SetForegroundColor(fg),
            style::SetBackgroundColor(Color::Black),
            style::Print(&text)
        )?;
    }

    w.flush()?;
    Ok(())
}

// -- Game session ------------------------------------------------------------

fn run_game<W: Write>(
    writer: &mut W,
    rx: &Receiver<Vec<u8>>,
    size_rx: &mut tokio::sync::watch::Receiver<(u32, u32)>,
    parser: &mut AnsiParser,
    saves: &SaveManager,
    username: &str,
) -> std::io::Result<GameLoopResult> {
    let (mut cols, mut rows) = {
        let (w, h) = *size_rx.borrow();
        (w as i32, h as i32)
    };

    // Enforce minimum terminal size.
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
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Ok(GameLoopResult::Quit);
                }
                Err(_) => {}
            }
        }
    }

    let mut renderer = render::CrosstermRenderer::new(writer, cols, rows);

    let mut input = SshInput {
        rx,
        parser,
        size_rx,
    };

    let mut dev = NoDevHooks;

    let config = GameLoopConfig {
        platform: Platform::Ssh,
        cols,
        rows,
    };

    let result = game_loop::run_game_loop(&mut renderer, &mut input, saves, &mut dev, config);

    tracing::info!(username, "Game ended");
    result
}

fn show_resize_prompt<W: Write>(w: &mut W, cols: i32, rows: i32) -> std::io::Result<()> {
    queue!(
        w,
        terminal::Clear(terminal::ClearType::All),
        cursor::MoveTo(0, 0),
        style::SetForegroundColor(Color::Yellow),
        style::SetBackgroundColor(Color::Black),
        style::Print(format!(
            "Terminal too small: {}x{} (need {}x{}). Please resize.",
            cols, rows, MIN_WIDTH, MIN_HEIGHT
        ))
    )?;
    w.flush()?;
    Ok(())
}
