use std::io::{self, Write};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::terminal;

use roguelike_core::rules::command::GameCommand;
use roguelike_core::rules::direction::Direction;
use roguelike_core::rules::game_view::GameView;
use roguelike_core::tier_micro::game::MicroGameState;
use roguelike_renderer3d::framebuffer::Framebuffer;
use roguelike_renderer3d::scene::render_scene;

/// Map crossterm key events to game commands.
fn get_command(key: KeyEvent) -> Option<GameCommand> {
    match key.code {
        KeyCode::Up => Some(GameCommand::Move(Direction::North)),
        KeyCode::Down => Some(GameCommand::Move(Direction::South)),
        KeyCode::Left => Some(GameCommand::Move(Direction::West)),
        KeyCode::Right => Some(GameCommand::Move(Direction::East)),
        KeyCode::Char('.') => Some(GameCommand::Descend),
        KeyCode::Char(' ') => Some(GameCommand::Wait),
        KeyCode::Char('g') => Some(GameCommand::Pickup),
        _ => None,
    }
}

fn main() -> io::Result<()> {
    let seed: u16 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(42);

    let mut game = MicroGameState::new_default(seed);
    let mut stdout = io::stdout();

    // Enter raw mode + alternate screen
    terminal::enable_raw_mode()?;
    crossterm::execute!(
        stdout,
        terminal::EnterAlternateScreen,
        crossterm::cursor::Hide
    )?;

    // Get terminal size — framebuffer width = columns, height = rows * 2 (half-block)
    let (cols, rows) = terminal::size()?;
    let fb_w = cols as u32;
    let fb_h = (rows as u32) * 2;

    let mut fb = Framebuffer::new(fb_w, fb_h);
    let mut frame: u32 = 0;

    let target_frame_time = Duration::from_millis(16); // ~60 fps

    loop {
        let frame_start = Instant::now();

        // Render the scene
        render_scene(&game, &mut fb, frame);

        // Build the full frame into a buffer, then write all at once
        // to avoid tearing/stutter from partial flushes.
        let mut buf: Vec<u8> = Vec::with_capacity((fb_w as usize) * (rows as usize) * 30);
        let half_rows = fb_h / 2;

        for row in 0..half_rows {
            // Cursor position per row (no newlines — avoids auto-wrap blank lines)
            std::write!(buf, "\x1b[{};1H", row + 1)?;
            let mut prev_fg: u16 = u16::MAX;
            let mut prev_bg: u16 = u16::MAX;

            let y_top = row * 2;
            let y_bot = y_top + 1;

            for x in 0..fb_w {
                let top = fb.get_pixel(x, y_top);
                let bot = fb.get_pixel(x, y_bot);

                if top != prev_fg {
                    let (r, g, b) = roguelike_renderer3d::framebuffer::unpack_rgb555(top);
                    std::write!(buf, "\x1b[38;2;{r};{g};{b}m")?;
                    prev_fg = top;
                }
                if bot != prev_bg {
                    let (r, g, b) = roguelike_renderer3d::framebuffer::unpack_rgb555(bot);
                    std::write!(buf, "\x1b[48;2;{r};{g};{b}m")?;
                    prev_bg = bot;
                }
                buf.extend_from_slice("▀".as_bytes());
            }
            buf.extend_from_slice(b"\x1b[0m");
        }

        stdout.write_all(&buf)?;
        stdout.flush()?;

        frame = frame.wrapping_add(1);

        // Poll for input with remaining frame time
        let elapsed = frame_start.elapsed();
        let poll_time = target_frame_time.saturating_sub(elapsed);

        if event::poll(poll_time)? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if key.code == KeyCode::Esc || key.code == KeyCode::Char('q') {
                    break;
                }
                if let Some(cmd) = get_command(key) {
                    game.step_view(cmd);
                }
            }
        }
    }

    // Restore terminal
    crossterm::execute!(
        stdout,
        crossterm::cursor::Show,
        terminal::LeaveAlternateScreen
    )?;
    terminal::disable_raw_mode()?;

    Ok(())
}
