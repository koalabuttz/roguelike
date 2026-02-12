mod ai;
mod combat;
mod data;
mod entity;
mod fov;
mod game;
mod map;
mod message_log;
mod render;
mod spawn;

use std::io::stdout;

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
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

        if let Event::Key(KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            ..
        }) = event::read()?
        {
            if modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
                break;
            }

            let player_took_action = match code {
                KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('8') => {
                    state.player_move_or_attack(0, -1)
                }
                KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('2') => {
                    state.player_move_or_attack(0, 1)
                }
                KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('4') => {
                    state.player_move_or_attack(-1, 0)
                }
                KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('6') => {
                    state.player_move_or_attack(1, 0)
                }

                KeyCode::Char('y') | KeyCode::Char('7') => state.player_move_or_attack(-1, -1),
                KeyCode::Char('u') | KeyCode::Char('9') => state.player_move_or_attack(1, -1),
                KeyCode::Char('b') | KeyCode::Char('1') => state.player_move_or_attack(-1, 1),
                KeyCode::Char('n') | KeyCode::Char('3') => state.player_move_or_attack(1, 1),

                KeyCode::Char('.') | KeyCode::Char('5') => true,

                KeyCode::Char('q') | KeyCode::Esc => break,

                _ => false,
            };

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
