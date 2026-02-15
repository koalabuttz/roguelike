use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::platform::{InputSource, MenuCommand};
use crate::types::Coord;

/// A platform-independent game command.
///
/// Input adapters (keyboard, controller, replay, network) produce these;
/// game logic consumes them. No module outside `input` should match on
/// raw key events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameCommand {
    Move {
        dx: Coord,
        dy: Coord,
    },
    /// Keep moving in a direction until something interesting happens.
    Autorun {
        dx: Coord,
        dy: Coord,
    },
    AutoExplore,
    Wait,
    Quit,
}

/// Resolve a key to a direction `(dx, dy)` and whether it triggers autorun.
///
/// Each input method uses its own autorun convention:
/// - Arrow keys: Shift = autorun
/// - Vi keys: uppercase = autorun
/// - Numpad: Shift = autorun
fn resolve_direction(code: KeyCode, shift: bool) -> Option<(Coord, Coord, bool)> {
    match code {
        // Arrow keys: shift = autorun
        KeyCode::Up => Some((0, -1, shift)),
        KeyCode::Down => Some((0, 1, shift)),
        KeyCode::Left => Some((-1, 0, shift)),
        KeyCode::Right => Some((1, 0, shift)),

        // Vi keys: uppercase = autorun
        KeyCode::Char('k') => Some((0, -1, false)),
        KeyCode::Char('K') => Some((0, -1, true)),
        KeyCode::Char('j') => Some((0, 1, false)),
        KeyCode::Char('J') => Some((0, 1, true)),
        KeyCode::Char('h') => Some((-1, 0, false)),
        KeyCode::Char('H') => Some((-1, 0, true)),
        KeyCode::Char('l') => Some((1, 0, false)),
        KeyCode::Char('L') => Some((1, 0, true)),
        KeyCode::Char('y') => Some((-1, -1, false)),
        KeyCode::Char('Y') => Some((-1, -1, true)),
        KeyCode::Char('u') => Some((1, -1, false)),
        KeyCode::Char('U') => Some((1, -1, true)),
        KeyCode::Char('b') => Some((-1, 1, false)),
        KeyCode::Char('B') => Some((-1, 1, true)),
        KeyCode::Char('n') => Some((1, 1, false)),
        KeyCode::Char('N') => Some((1, 1, true)),

        // Numpad: shift = autorun
        KeyCode::Char('8') => Some((0, -1, shift)),
        KeyCode::Char('2') => Some((0, 1, shift)),
        KeyCode::Char('4') => Some((-1, 0, shift)),
        KeyCode::Char('6') => Some((1, 0, shift)),
        KeyCode::Char('7') => Some((-1, -1, shift)),
        KeyCode::Char('9') => Some((1, -1, shift)),
        KeyCode::Char('1') => Some((-1, 1, shift)),
        KeyCode::Char('3') => Some((1, 1, shift)),

        _ => None,
    }
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

    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    if let Some((dx, dy, autorun)) = resolve_direction(key.code, shift) {
        return Some(if autorun {
            GameCommand::Autorun { dx, dy }
        } else {
            GameCommand::Move { dx, dy }
        });
    }

    match key.code {
        KeyCode::Char('o') => Some(GameCommand::AutoExplore),
        KeyCode::Char('.') | KeyCode::Char('5') => Some(GameCommand::Wait),
        KeyCode::Char('q') | KeyCode::Esc => Some(GameCommand::Quit),
        _ => None,
    }
}

/// Translate a crossterm key event into a menu command.
///
/// Returns `None` for keys that have no menu binding.
pub fn translate_menu_key(key: KeyEvent) -> Option<MenuCommand> {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('8') => Some(MenuCommand::Up),
        KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('2') => Some(MenuCommand::Down),
        KeyCode::Enter | KeyCode::Char(' ') => Some(MenuCommand::Select),
        KeyCode::Esc | KeyCode::Char('q') => Some(MenuCommand::Back),
        _ => None,
    }
}

/// Terminal input source backed by crossterm.
///
/// Blocks on `crossterm::event::read()` and translates key events into
/// platform-independent commands.
pub struct CrosstermInput;

impl InputSource for CrosstermInput {
    fn next_command(&mut self) -> Option<GameCommand> {
        loop {
            if let Ok(Event::Key(
                key @ KeyEvent {
                    kind: KeyEventKind::Press,
                    ..
                },
            )) = event::read()
            {
                return translate_key(key);
            }
        }
    }

    fn next_menu_command(&mut self) -> Option<MenuCommand> {
        loop {
            if let Ok(Event::Key(
                key @ KeyEvent {
                    kind: KeyEventKind::Press,
                    ..
                },
            )) = event::read()
            {
                return translate_menu_key(key);
            }
        }
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
    fn shift_numpad_produces_autorun() {
        assert_eq!(
            translate_key(press_with(KeyCode::Char('8'), KeyModifiers::SHIFT)),
            Some(GameCommand::Autorun { dx: 0, dy: -1 })
        );
        assert_eq!(
            translate_key(press_with(KeyCode::Char('2'), KeyModifiers::SHIFT)),
            Some(GameCommand::Autorun { dx: 0, dy: 1 })
        );
        assert_eq!(
            translate_key(press_with(KeyCode::Char('4'), KeyModifiers::SHIFT)),
            Some(GameCommand::Autorun { dx: -1, dy: 0 })
        );
        assert_eq!(
            translate_key(press_with(KeyCode::Char('6'), KeyModifiers::SHIFT)),
            Some(GameCommand::Autorun { dx: 1, dy: 0 })
        );
        assert_eq!(
            translate_key(press_with(KeyCode::Char('7'), KeyModifiers::SHIFT)),
            Some(GameCommand::Autorun { dx: -1, dy: -1 })
        );
        assert_eq!(
            translate_key(press_with(KeyCode::Char('9'), KeyModifiers::SHIFT)),
            Some(GameCommand::Autorun { dx: 1, dy: -1 })
        );
        assert_eq!(
            translate_key(press_with(KeyCode::Char('1'), KeyModifiers::SHIFT)),
            Some(GameCommand::Autorun { dx: -1, dy: 1 })
        );
        assert_eq!(
            translate_key(press_with(KeyCode::Char('3'), KeyModifiers::SHIFT)),
            Some(GameCommand::Autorun { dx: 1, dy: 1 })
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

    #[test]
    fn uppercase_vi_keys_produce_autorun() {
        assert_eq!(
            translate_key(press(KeyCode::Char('K'))),
            Some(GameCommand::Autorun { dx: 0, dy: -1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('J'))),
            Some(GameCommand::Autorun { dx: 0, dy: 1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('H'))),
            Some(GameCommand::Autorun { dx: -1, dy: 0 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('L'))),
            Some(GameCommand::Autorun { dx: 1, dy: 0 })
        );
    }

    #[test]
    fn uppercase_vi_diagonal_keys_produce_autorun() {
        assert_eq!(
            translate_key(press(KeyCode::Char('Y'))),
            Some(GameCommand::Autorun { dx: -1, dy: -1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('U'))),
            Some(GameCommand::Autorun { dx: 1, dy: -1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('B'))),
            Some(GameCommand::Autorun { dx: -1, dy: 1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('N'))),
            Some(GameCommand::Autorun { dx: 1, dy: 1 })
        );
    }

    #[test]
    fn o_key_produces_auto_explore() {
        assert_eq!(
            translate_key(press(KeyCode::Char('o'))),
            Some(GameCommand::AutoExplore)
        );
    }

    #[test]
    fn shift_arrow_keys_produce_autorun() {
        assert_eq!(
            translate_key(press_with(KeyCode::Up, KeyModifiers::SHIFT)),
            Some(GameCommand::Autorun { dx: 0, dy: -1 })
        );
        assert_eq!(
            translate_key(press_with(KeyCode::Down, KeyModifiers::SHIFT)),
            Some(GameCommand::Autorun { dx: 0, dy: 1 })
        );
        assert_eq!(
            translate_key(press_with(KeyCode::Left, KeyModifiers::SHIFT)),
            Some(GameCommand::Autorun { dx: -1, dy: 0 })
        );
        assert_eq!(
            translate_key(press_with(KeyCode::Right, KeyModifiers::SHIFT)),
            Some(GameCommand::Autorun { dx: 1, dy: 0 })
        );
    }

    // --- Menu command tests ---

    #[test]
    fn menu_up_keys() {
        assert_eq!(
            translate_menu_key(press(KeyCode::Up)),
            Some(MenuCommand::Up)
        );
        assert_eq!(
            translate_menu_key(press(KeyCode::Char('k'))),
            Some(MenuCommand::Up)
        );
        assert_eq!(
            translate_menu_key(press(KeyCode::Char('8'))),
            Some(MenuCommand::Up)
        );
    }

    #[test]
    fn menu_down_keys() {
        assert_eq!(
            translate_menu_key(press(KeyCode::Down)),
            Some(MenuCommand::Down)
        );
        assert_eq!(
            translate_menu_key(press(KeyCode::Char('j'))),
            Some(MenuCommand::Down)
        );
        assert_eq!(
            translate_menu_key(press(KeyCode::Char('2'))),
            Some(MenuCommand::Down)
        );
    }

    #[test]
    fn menu_select_keys() {
        assert_eq!(
            translate_menu_key(press(KeyCode::Enter)),
            Some(MenuCommand::Select)
        );
        assert_eq!(
            translate_menu_key(press(KeyCode::Char(' '))),
            Some(MenuCommand::Select)
        );
    }

    #[test]
    fn menu_back_keys() {
        assert_eq!(
            translate_menu_key(press(KeyCode::Esc)),
            Some(MenuCommand::Back)
        );
        assert_eq!(
            translate_menu_key(press(KeyCode::Char('q'))),
            Some(MenuCommand::Back)
        );
    }

    #[test]
    fn menu_unbound_key_returns_none() {
        assert_eq!(translate_menu_key(press(KeyCode::Char('x'))), None);
        assert_eq!(translate_menu_key(press(KeyCode::F(1))), None);
    }
}
