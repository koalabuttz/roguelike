use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// A platform-independent game command.
///
/// Input adapters (keyboard, controller, replay, network) produce these;
/// game logic consumes them. No module outside `input` should match on
/// raw key events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameCommand {
    Move { dx: i32, dy: i32 },
    Wait,
    Quit,
}

/// Translate a crossterm key event into a game command.
///
/// Returns `None` for keys that have no binding, so the caller can
/// silently ignore them.
pub fn translate_key(key: KeyEvent) -> Option<GameCommand> {
    // Ctrl+C always quits, checked before any character bindings
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Some(GameCommand::Quit);
    }

    match key.code {
        // Cardinal movement — arrows, vi keys, numpad
        KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('8') => {
            Some(GameCommand::Move { dx: 0, dy: -1 })
        }
        KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('2') => {
            Some(GameCommand::Move { dx: 0, dy: 1 })
        }
        KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('4') => {
            Some(GameCommand::Move { dx: -1, dy: 0 })
        }
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('6') => {
            Some(GameCommand::Move { dx: 1, dy: 0 })
        }

        // Diagonal movement — vi keys, numpad
        KeyCode::Char('y') | KeyCode::Char('7') => Some(GameCommand::Move { dx: -1, dy: -1 }),
        KeyCode::Char('u') | KeyCode::Char('9') => Some(GameCommand::Move { dx: 1, dy: -1 }),
        KeyCode::Char('b') | KeyCode::Char('1') => Some(GameCommand::Move { dx: -1, dy: 1 }),
        KeyCode::Char('n') | KeyCode::Char('3') => Some(GameCommand::Move { dx: 1, dy: 1 }),

        // Wait
        KeyCode::Char('.') | KeyCode::Char('5') => Some(GameCommand::Wait),

        // Quit
        KeyCode::Char('q') | KeyCode::Esc => Some(GameCommand::Quit),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};

    /// Helper to build a key-press event with no modifiers.
    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    /// Helper to build a key-press event with specific modifiers.
    fn press_with(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn arrow_keys_produce_cardinal_moves() {
        assert_eq!(
            translate_key(press(KeyCode::Up)),
            Some(GameCommand::Move { dx: 0, dy: -1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Down)),
            Some(GameCommand::Move { dx: 0, dy: 1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Left)),
            Some(GameCommand::Move { dx: -1, dy: 0 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Right)),
            Some(GameCommand::Move { dx: 1, dy: 0 })
        );
    }

    #[test]
    fn vi_keys_produce_cardinal_moves() {
        assert_eq!(
            translate_key(press(KeyCode::Char('k'))),
            Some(GameCommand::Move { dx: 0, dy: -1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('j'))),
            Some(GameCommand::Move { dx: 0, dy: 1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('h'))),
            Some(GameCommand::Move { dx: -1, dy: 0 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('l'))),
            Some(GameCommand::Move { dx: 1, dy: 0 })
        );
    }

    #[test]
    fn vi_diagonal_keys_produce_diagonal_moves() {
        assert_eq!(
            translate_key(press(KeyCode::Char('y'))),
            Some(GameCommand::Move { dx: -1, dy: -1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('u'))),
            Some(GameCommand::Move { dx: 1, dy: -1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('b'))),
            Some(GameCommand::Move { dx: -1, dy: 1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('n'))),
            Some(GameCommand::Move { dx: 1, dy: 1 })
        );
    }

    #[test]
    fn numpad_produces_all_eight_directions() {
        assert_eq!(
            translate_key(press(KeyCode::Char('8'))),
            Some(GameCommand::Move { dx: 0, dy: -1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('2'))),
            Some(GameCommand::Move { dx: 0, dy: 1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('4'))),
            Some(GameCommand::Move { dx: -1, dy: 0 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('6'))),
            Some(GameCommand::Move { dx: 1, dy: 0 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('7'))),
            Some(GameCommand::Move { dx: -1, dy: -1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('9'))),
            Some(GameCommand::Move { dx: 1, dy: -1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('1'))),
            Some(GameCommand::Move { dx: -1, dy: 1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('3'))),
            Some(GameCommand::Move { dx: 1, dy: 1 })
        );
    }

    #[test]
    fn wait_keys() {
        assert_eq!(
            translate_key(press(KeyCode::Char('.'))),
            Some(GameCommand::Wait)
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('5'))),
            Some(GameCommand::Wait)
        );
    }

    #[test]
    fn quit_keys() {
        assert_eq!(
            translate_key(press(KeyCode::Char('q'))),
            Some(GameCommand::Quit)
        );
        assert_eq!(translate_key(press(KeyCode::Esc)), Some(GameCommand::Quit));
    }

    #[test]
    fn ctrl_c_quits() {
        assert_eq!(
            translate_key(press_with(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(GameCommand::Quit)
        );
    }

    #[test]
    fn unbound_key_returns_none() {
        assert_eq!(translate_key(press(KeyCode::Char('x'))), None);
        assert_eq!(translate_key(press(KeyCode::Char('z'))), None);
        assert_eq!(translate_key(press(KeyCode::F(1))), None);
    }

    #[test]
    fn ctrl_c_takes_priority_over_character_bindings() {
        // 'c' alone is unbound, but Ctrl+C should still quit
        assert_eq!(translate_key(press(KeyCode::Char('c'))), None);
        assert_eq!(
            translate_key(press_with(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(GameCommand::Quit)
        );
    }
}
