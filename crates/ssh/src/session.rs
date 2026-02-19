use std::io::Write;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use crossterm::{cursor, queue, style, terminal};

use roguelike_core::settings::Platform;

use roguelike_tui::game_loop::{self, GameLoopConfig, NoDevHooks};
use roguelike_tui::render;

use crate::ansi_input::AnsiParser;
use crate::saves::SaveManager;
use crate::ssh_input::SshInput;

const MIN_WIDTH: i32 = 60;
const MIN_HEIGHT: i32 = 20;

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
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
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

    tracing::info!(username, "Session ended");
    result
}

fn show_resize_prompt<W: Write>(w: &mut W, cols: i32, rows: i32) -> std::io::Result<()> {
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
