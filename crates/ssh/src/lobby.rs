use std::io::Write;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use crossterm::{
    cursor, queue,
    style::{self, Color, SetBackgroundColor, SetForegroundColor},
    terminal,
};

use crate::accounts::AccountStore;
use crate::ansi_input::AnsiParser;

/// Result of the lobby flow.
pub enum LobbyResult {
    /// User logged in with this username.
    LoggedIn(String),
    /// User chose to quit.
    Quit,
}

/// Lobby menu items.
#[derive(Clone, Copy, PartialEq)]
enum LobbyItem {
    Login,
    Register,
    Watch,
    Quit,
}

const LOBBY_ITEMS: &[LobbyItem] = &[
    LobbyItem::Login,
    LobbyItem::Register,
    LobbyItem::Watch,
    LobbyItem::Quit,
];

/// Run the dgamelaunch-style lobby. Blocks until the user logs in or quits.
pub fn run_lobby<W: Write>(
    w: &mut W,
    rx: &Receiver<Vec<u8>>,
    parser: &mut AnsiParser,
    accounts: &AccountStore,
    width: i32,
    height: i32,
    active_sessions: usize,
) -> std::io::Result<LobbyResult> {
    let mut selected: usize = 0;

    loop {
        draw_lobby(w, width, height, selected, active_sessions)?;

        let key = match wait_for_key(rx, parser)? {
            Some(k) => k,
            None => return Ok(LobbyResult::Quit),
        };

        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if selected > 0 {
                    selected -= 1;
                } else {
                    selected = LOBBY_ITEMS.len() - 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                selected = (selected + 1) % LOBBY_ITEMS.len();
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                match LOBBY_ITEMS[selected] {
                    LobbyItem::Login => {
                        if let Some(username) = run_login(w, rx, parser, accounts, width, height)? {
                            return Ok(LobbyResult::LoggedIn(username));
                        }
                    }
                    LobbyItem::Register => {
                        if let Some(username) =
                            run_register(w, rx, parser, accounts, width, height)?
                        {
                            return Ok(LobbyResult::LoggedIn(username));
                        }
                    }
                    LobbyItem::Watch => {
                        // Coming soon — show a message and return to lobby
                        show_message(w, width, height, "Spectating coming soon!", Color::Yellow)?;
                        let _ = wait_for_key(rx, parser)?;
                    }
                    LobbyItem::Quit => return Ok(LobbyResult::Quit),
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => return Ok(LobbyResult::Quit),
            _ => {}
        }
    }
}

fn draw_lobby<W: Write>(
    w: &mut W,
    width: i32,
    height: i32,
    selected: usize,
    active_sessions: usize,
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
        SetForegroundColor(Color::Cyan),
        SetBackgroundColor(Color::Black),
        style::Print(title)
    )?;

    // Version
    let version = concat!("v", env!("CARGO_PKG_VERSION"));
    let vx = (cx - version.len() as i32 / 2).max(0);
    queue!(
        w,
        cursor::MoveTo(vx as u16, (top + 1) as u16),
        SetForegroundColor(Color::DarkGrey),
        SetBackgroundColor(Color::Black),
        style::Print(version)
    )?;

    // Menu items
    let items = [
        ("Login", true),
        ("Register", true),
        ("Watch a Game  (coming soon)", false),
        ("Quit", true),
    ];

    let items_y = top + 4;
    for (i, (label, enabled)) in items.iter().enumerate() {
        let y = items_y + i as i32;
        let prefix = if i == selected { "> " } else { "  " };
        let text = format!("{}{}", prefix, label);
        let ix = (cx - text.len() as i32 / 2).max(0);

        let fg = if !enabled {
            Color::DarkGrey
        } else if i == selected {
            Color::Yellow
        } else {
            Color::White
        };

        queue!(
            w,
            cursor::MoveTo(ix as u16, y as u16),
            SetForegroundColor(fg),
            SetBackgroundColor(Color::Black),
            style::Print(&text)
        )?;
    }

    // Player count
    let count_msg = if active_sessions == 1 {
        "1 player online".to_string()
    } else {
        format!("{} players online", active_sessions)
    };
    let count_x = (cx - count_msg.len() as i32 / 2).max(0);
    let count_y = items_y + items.len() as i32 + 2;
    queue!(
        w,
        cursor::MoveTo(count_x as u16, count_y as u16),
        SetForegroundColor(Color::DarkGrey),
        SetBackgroundColor(Color::Black),
        style::Print(&count_msg)
    )?;

    w.flush()?;
    Ok(())
}

/// Text input dialog. Returns None on Esc.
fn text_input<W: Write>(
    w: &mut W,
    rx: &Receiver<Vec<u8>>,
    parser: &mut AnsiParser,
    prompt: &str,
    width: i32,
    height: i32,
    hidden: bool,
) -> std::io::Result<Option<String>> {
    let mut input = String::new();
    loop {
        queue!(
            w,
            terminal::Clear(terminal::ClearType::All),
            cursor::MoveTo(0, 0)
        )?;

        let cx = width / 2;
        let cy = height / 2;

        // Prompt
        let px = (cx - prompt.len() as i32 / 2).max(0);
        queue!(
            w,
            cursor::MoveTo(px as u16, (cy - 2) as u16),
            SetForegroundColor(Color::Cyan),
            SetBackgroundColor(Color::Black),
            style::Print(prompt)
        )?;

        // Input field
        let display_text = if hidden {
            "*".repeat(input.len())
        } else {
            input.clone()
        };
        let display = format!("> {}_", display_text);
        let ix = (cx - display.len() as i32 / 2).max(0);
        queue!(
            w,
            cursor::MoveTo(ix as u16, cy as u16),
            SetForegroundColor(Color::Yellow),
            SetBackgroundColor(Color::Black),
            style::Print(&display)
        )?;

        // Hint
        let hint = "Esc to cancel";
        let hx = (cx - hint.len() as i32 / 2).max(0);
        queue!(
            w,
            cursor::MoveTo(hx as u16, (cy + 2) as u16),
            SetForegroundColor(Color::DarkGrey),
            SetBackgroundColor(Color::Black),
            style::Print(hint)
        )?;

        w.flush()?;

        let key = match wait_for_key(rx, parser)? {
            Some(k) => k,
            None => return Ok(None),
        };

        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Esc => return Ok(None),
            KeyCode::Enter => return Ok(Some(input)),
            KeyCode::Backspace => {
                input.pop();
            }
            KeyCode::Char(c) if c.is_ascii_graphic() || c == ' ' => {
                // For hidden fields, don't allow very long passwords
                if input.len() < 128 {
                    input.push(c);
                }
            }
            _ => {}
        }
    }
}

fn run_login<W: Write>(
    w: &mut W,
    rx: &Receiver<Vec<u8>>,
    parser: &mut AnsiParser,
    accounts: &AccountStore,
    width: i32,
    height: i32,
) -> std::io::Result<Option<String>> {
    let username = match text_input(w, rx, parser, "Username", width, height, false)? {
        Some(u) if !u.is_empty() => u,
        _ => return Ok(None),
    };

    let password = match text_input(w, rx, parser, "Password", width, height, true)? {
        Some(p) => p,
        None => return Ok(None),
    };

    match accounts.login(&username, &password) {
        Ok(()) => Ok(Some(username)),
        Err(msg) => {
            show_message(w, width, height, &msg, Color::Red)?;
            let _ = wait_for_key(rx, parser)?;
            Ok(None)
        }
    }
}

fn run_register<W: Write>(
    w: &mut W,
    rx: &Receiver<Vec<u8>>,
    parser: &mut AnsiParser,
    accounts: &AccountStore,
    width: i32,
    height: i32,
) -> std::io::Result<Option<String>> {
    let username = match text_input(w, rx, parser, "Choose a Username", width, height, false)? {
        Some(u) if !u.is_empty() => u,
        _ => return Ok(None),
    };

    let password = match text_input(w, rx, parser, "Choose a Password", width, height, true)? {
        Some(p) if !p.is_empty() => p,
        _ => return Ok(None),
    };

    let confirm = match text_input(w, rx, parser, "Confirm Password", width, height, true)? {
        Some(c) => c,
        None => return Ok(None),
    };

    if password != confirm {
        show_message(w, width, height, "Passwords do not match.", Color::Red)?;
        let _ = wait_for_key(rx, parser)?;
        return Ok(None);
    }

    match accounts.register(&username, &password) {
        Ok(()) => {
            show_message(
                w,
                width,
                height,
                &format!("Account '{}' created! Logging in...", username),
                Color::Green,
            )?;
            let _ = wait_for_key(rx, parser)?;
            Ok(Some(username))
        }
        Err(msg) => {
            show_message(w, width, height, &msg, Color::Red)?;
            let _ = wait_for_key(rx, parser)?;
            Ok(None)
        }
    }
}

fn show_message<W: Write>(
    w: &mut W,
    width: i32,
    height: i32,
    msg: &str,
    color: Color,
) -> std::io::Result<()> {
    queue!(
        w,
        terminal::Clear(terminal::ClearType::All),
        cursor::MoveTo(0, 0)
    )?;

    let cx = width / 2;
    let cy = height / 2;
    let mx = (cx - msg.len() as i32 / 2).max(0);
    queue!(
        w,
        cursor::MoveTo(mx as u16, cy as u16),
        SetForegroundColor(color),
        SetBackgroundColor(Color::Black),
        style::Print(msg)
    )?;

    let hint = "Press any key...";
    let hx = (cx - hint.len() as i32 / 2).max(0);
    queue!(
        w,
        cursor::MoveTo(hx as u16, (cy + 2) as u16),
        SetForegroundColor(Color::DarkGrey),
        SetBackgroundColor(Color::Black),
        style::Print(hint)
    )?;

    w.flush()?;
    Ok(())
}

/// Wait for a key event from the SSH channel. Returns None on channel close.
pub fn wait_for_key(
    rx: &Receiver<Vec<u8>>,
    parser: &mut AnsiParser,
) -> std::io::Result<Option<crossterm::event::KeyEvent>> {
    loop {
        // If parser has a pending escape, poll with a short timeout
        let timeout = if parser.pending() {
            Duration::from_millis(50)
        } else {
            Duration::from_secs(300) // 5 min idle timeout for lobby
        };

        match rx.recv_timeout(timeout) {
            Ok(data) => {
                for &byte in &data {
                    let events = parser.feed(byte);
                    if let Some(ev) = events.into_iter().next() {
                        return Ok(Some(ev));
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if let Some(ev) = parser.check_timeout() {
                    return Ok(Some(ev));
                }
                if !parser.pending() {
                    // Real timeout — disconnect
                    return Ok(None);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Ok(None);
            }
        }
    }
}
