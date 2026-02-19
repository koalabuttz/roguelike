use std::io::stdout;

mod dev_hooks;
mod gamepad;
mod local_saves;
mod terminal_input;

use crossterm::{cursor, execute, terminal};

use roguelike_core::settings::Platform;
use roguelike_core::spectate::NullFrameSink;
use roguelike_tui::game_loop::{self, GameLoopConfig};

fn main() -> std::io::Result<()> {
    let mut stdout = stdout();

    terminal::enable_raw_mode()?;
    execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;

    let (cols, rows) = terminal::size()?;
    let mut renderer =
        roguelike_tui::render::CrosstermRenderer::new(std::io::stdout(), cols as i32, rows as i32);

    let mut input = terminal_input::TerminalInput {
        gp: gamepad::new_gamepad_option(),
    };
    let saves = local_saves::LocalSaveBackend;

    #[cfg(all(debug_assertions, feature = "dev-tools"))]
    let mut dev = dev_hooks::TerminalDevHooks::new();
    #[cfg(not(all(debug_assertions, feature = "dev-tools")))]
    let mut dev = roguelike_tui::game_loop::NoDevHooks;

    let config = GameLoopConfig {
        platform: Platform::Terminal,
        cols: cols as i32,
        rows: rows as i32,
    };

    let result = game_loop::run_game_loop(
        &mut renderer,
        &mut input,
        &saves,
        &mut dev,
        config,
        &NullFrameSink,
    );

    execute!(stdout, terminal::LeaveAlternateScreen, cursor::Show)?;
    terminal::disable_raw_mode()?;

    result.map(|_| ())
}
