mod ai;
mod combat;
mod data;
mod entity;
mod fov;
mod game;
mod input;
mod map;
mod message_log;
mod render;
mod spawn;

use std::io::stdout;

use crossterm::{
    cursor,
    event::{self, Event, KeyEvent, KeyEventKind},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};

fn main() -> std::io::Result<()> {
    let mut stdout = stdout();

    terminal::enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, cursor::Hide)?;

    let (cols, rows) = terminal::size()?;
    let map_height = (rows as i32) - data::CONFIG.ui_bottom_rows;
    let mut state = game::GameState::new(cols as i32, map_height);

    loop {
        render::render(&mut stdout, &state, cols as i32, rows as i32)?;

        if state.game_over {
            // Game-over screen: any key dismisses
            loop {
                if let Event::Key(KeyEvent {
                    kind: KeyEventKind::Press,
                    ..
                }) = event::read()?
                {
                    break;
                }
            }
            break;
        }

        if let Event::Key(
            key_event @ KeyEvent {
                kind: KeyEventKind::Press,
                ..
            },
        ) = event::read()?
            && let Some(cmd) = input::translate_key(key_event)
        {
            if matches!(cmd, input::GameCommand::Quit) {
                break;
            }

            let player_took_action = state.handle_command(cmd);

            if player_took_action {
                state.update_fov();
                if ai::run_monster_turns(
                    &mut state.entities,
                    &state.map,
                    &state.visible,
                    &mut state.log,
                ) {
                    state.game_over = true;
                }
            }
        }
    }

    execute!(stdout, LeaveAlternateScreen, cursor::Show)?;
    terminal::disable_raw_mode()?;

    Ok(())
}
