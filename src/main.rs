use std::io::stdout;
use std::time::Duration;

use roguelike::{data, game, input, render, types::Coord};

use crossterm::{
    cursor,
    event::{self, Event, KeyEvent, KeyEventKind},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};

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

            // Autorun: animate step-by-step with interrupt support.
            if let input::GameCommand::Autorun { dx, dy } = cmd {
                let stepper = state.start_autorun(dx, dy);
                animate_stepper(&mut stdout, &mut state, stepper, cols as i32, rows as i32)?;
                continue;
            }

            // Auto-explore: animate step-by-step with interrupt support.
            if matches!(cmd, input::GameCommand::AutoExplore) {
                match state.start_auto_explore() {
                    Ok((stepper, _tx, _ty)) => {
                        animate_stepper(
                            &mut stdout,
                            &mut state,
                            stepper,
                            cols as i32,
                            rows as i32,
                        )?;
                    }
                    Err(_) => {
                        state.log.add("No unexplored areas reachable.");
                    }
                }
                continue;
            }

            state.step(cmd);
        }
    }

    execute!(stdout, LeaveAlternateScreen, cursor::Show)?;
    terminal::disable_raw_mode()?;

    Ok(())
}
