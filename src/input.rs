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
fn resolve_direction(
    code: KeyCode,
    shift: bool,
    vi_keys: bool,
    numpad: bool,
) -> Option<(Coord, Coord, bool)> {
    match code {
        // Arrow keys: shift = autorun (always available)
        KeyCode::Up => Some((0, -1, shift)),
        KeyCode::Down => Some((0, 1, shift)),
        KeyCode::Left => Some((-1, 0, shift)),
        KeyCode::Right => Some((1, 0, shift)),

        // Vi keys: uppercase = autorun
        KeyCode::Char('k') if vi_keys => Some((0, -1, false)),
        KeyCode::Char('K') if vi_keys => Some((0, -1, true)),
        KeyCode::Char('j') if vi_keys => Some((0, 1, false)),
        KeyCode::Char('J') if vi_keys => Some((0, 1, true)),
        KeyCode::Char('h') if vi_keys => Some((-1, 0, false)),
        KeyCode::Char('H') if vi_keys => Some((-1, 0, true)),
        KeyCode::Char('l') if vi_keys => Some((1, 0, false)),
        KeyCode::Char('L') if vi_keys => Some((1, 0, true)),
        KeyCode::Char('y') if vi_keys => Some((-1, -1, false)),
        KeyCode::Char('Y') if vi_keys => Some((-1, -1, true)),
        KeyCode::Char('u') if vi_keys => Some((1, -1, false)),
        KeyCode::Char('U') if vi_keys => Some((1, -1, true)),
        KeyCode::Char('b') if vi_keys => Some((-1, 1, false)),
        KeyCode::Char('B') if vi_keys => Some((-1, 1, true)),
        KeyCode::Char('n') if vi_keys => Some((1, 1, false)),
        KeyCode::Char('N') if vi_keys => Some((1, 1, true)),

        // Numpad: shift = autorun
        KeyCode::Char('8') if numpad => Some((0, -1, shift)),
        KeyCode::Char('2') if numpad => Some((0, 1, shift)),
        KeyCode::Char('4') if numpad => Some((-1, 0, shift)),
        KeyCode::Char('6') if numpad => Some((1, 0, shift)),
        KeyCode::Char('7') if numpad => Some((-1, -1, shift)),
        KeyCode::Char('9') if numpad => Some((1, -1, shift)),
        KeyCode::Char('1') if numpad => Some((-1, 1, shift)),
        KeyCode::Char('3') if numpad => Some((1, 1, shift)),

        _ => None,
    }
}

/// Translate a crossterm key event into a game command.
///
/// Returns `None` for keys that have no binding, so the caller can
/// silently ignore them.
pub fn translate_key(key: KeyEvent, vi_keys: bool, numpad: bool) -> Option<GameCommand> {
    // Ctrl+C always quits, checked before any character bindings
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Some(GameCommand::Quit);
    }

    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    if let Some((dx, dy, autorun)) = resolve_direction(key.code, shift, vi_keys, numpad) {
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
pub struct CrosstermInput {
    pub vi_keys: bool,
    pub numpad: bool,
}

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
                return translate_key(key, self.vi_keys, self.numpad);
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
            translate_key(press(KeyCode::Up), true, true),
            Some(GameCommand::Move { dx: 0, dy: -1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Down), true, true),
            Some(GameCommand::Move { dx: 0, dy: 1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Left), true, true),
            Some(GameCommand::Move { dx: -1, dy: 0 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Right), true, true),
            Some(GameCommand::Move { dx: 1, dy: 0 })
        );
    }

    #[test]
    fn vi_keys_produce_cardinal_moves() {
        assert_eq!(
            translate_key(press(KeyCode::Char('k')), true, true),
            Some(GameCommand::Move { dx: 0, dy: -1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('j')), true, true),
            Some(GameCommand::Move { dx: 0, dy: 1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('h')), true, true),
            Some(GameCommand::Move { dx: -1, dy: 0 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('l')), true, true),
            Some(GameCommand::Move { dx: 1, dy: 0 })
        );
    }

    #[test]
    fn vi_diagonal_keys_produce_diagonal_moves() {
        assert_eq!(
            translate_key(press(KeyCode::Char('y')), true, true),
            Some(GameCommand::Move { dx: -1, dy: -1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('u')), true, true),
            Some(GameCommand::Move { dx: 1, dy: -1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('b')), true, true),
            Some(GameCommand::Move { dx: -1, dy: 1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('n')), true, true),
            Some(GameCommand::Move { dx: 1, dy: 1 })
        );
    }

    #[test]
    fn numpad_produces_all_eight_directions() {
        assert_eq!(
            translate_key(press(KeyCode::Char('8')), true, true),
            Some(GameCommand::Move { dx: 0, dy: -1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('2')), true, true),
            Some(GameCommand::Move { dx: 0, dy: 1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('4')), true, true),
            Some(GameCommand::Move { dx: -1, dy: 0 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('6')), true, true),
            Some(GameCommand::Move { dx: 1, dy: 0 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('7')), true, true),
            Some(GameCommand::Move { dx: -1, dy: -1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('9')), true, true),
            Some(GameCommand::Move { dx: 1, dy: -1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('1')), true, true),
            Some(GameCommand::Move { dx: -1, dy: 1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('3')), true, true),
            Some(GameCommand::Move { dx: 1, dy: 1 })
        );
    }

    #[test]
    fn shift_numpad_produces_autorun() {
        assert_eq!(
            translate_key(
                press_with(KeyCode::Char('8'), KeyModifiers::SHIFT),
                true,
                true
            ),
            Some(GameCommand::Autorun { dx: 0, dy: -1 })
        );
        assert_eq!(
            translate_key(
                press_with(KeyCode::Char('2'), KeyModifiers::SHIFT),
                true,
                true
            ),
            Some(GameCommand::Autorun { dx: 0, dy: 1 })
        );
        assert_eq!(
            translate_key(
                press_with(KeyCode::Char('4'), KeyModifiers::SHIFT),
                true,
                true
            ),
            Some(GameCommand::Autorun { dx: -1, dy: 0 })
        );
        assert_eq!(
            translate_key(
                press_with(KeyCode::Char('6'), KeyModifiers::SHIFT),
                true,
                true
            ),
            Some(GameCommand::Autorun { dx: 1, dy: 0 })
        );
        assert_eq!(
            translate_key(
                press_with(KeyCode::Char('7'), KeyModifiers::SHIFT),
                true,
                true
            ),
            Some(GameCommand::Autorun { dx: -1, dy: -1 })
        );
        assert_eq!(
            translate_key(
                press_with(KeyCode::Char('9'), KeyModifiers::SHIFT),
                true,
                true
            ),
            Some(GameCommand::Autorun { dx: 1, dy: -1 })
        );
        assert_eq!(
            translate_key(
                press_with(KeyCode::Char('1'), KeyModifiers::SHIFT),
                true,
                true
            ),
            Some(GameCommand::Autorun { dx: -1, dy: 1 })
        );
        assert_eq!(
            translate_key(
                press_with(KeyCode::Char('3'), KeyModifiers::SHIFT),
                true,
                true
            ),
            Some(GameCommand::Autorun { dx: 1, dy: 1 })
        );
    }

    #[test]
    fn wait_keys() {
        assert_eq!(
            translate_key(press(KeyCode::Char('.')), true, true),
            Some(GameCommand::Wait)
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('5')), true, true),
            Some(GameCommand::Wait)
        );
    }

    #[test]
    fn quit_keys() {
        assert_eq!(
            translate_key(press(KeyCode::Char('q')), true, true),
            Some(GameCommand::Quit)
        );
        assert_eq!(
            translate_key(press(KeyCode::Esc), true, true),
            Some(GameCommand::Quit)
        );
    }

    #[test]
    fn ctrl_c_quits() {
        assert_eq!(
            translate_key(
                press_with(KeyCode::Char('c'), KeyModifiers::CONTROL),
                true,
                true
            ),
            Some(GameCommand::Quit)
        );
    }

    #[test]
    fn unbound_key_returns_none() {
        assert_eq!(translate_key(press(KeyCode::Char('x')), true, true), None);
        assert_eq!(translate_key(press(KeyCode::Char('z')), true, true), None);
        assert_eq!(translate_key(press(KeyCode::F(1)), true, true), None);
    }

    #[test]
    fn ctrl_c_takes_priority_over_character_bindings() {
        // 'c' alone is unbound, but Ctrl+C should still quit
        assert_eq!(translate_key(press(KeyCode::Char('c')), true, true), None);
        assert_eq!(
            translate_key(
                press_with(KeyCode::Char('c'), KeyModifiers::CONTROL),
                true,
                true
            ),
            Some(GameCommand::Quit)
        );
    }

    #[test]
    fn uppercase_vi_keys_produce_autorun() {
        assert_eq!(
            translate_key(press(KeyCode::Char('K')), true, true),
            Some(GameCommand::Autorun { dx: 0, dy: -1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('J')), true, true),
            Some(GameCommand::Autorun { dx: 0, dy: 1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('H')), true, true),
            Some(GameCommand::Autorun { dx: -1, dy: 0 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('L')), true, true),
            Some(GameCommand::Autorun { dx: 1, dy: 0 })
        );
    }

    #[test]
    fn uppercase_vi_diagonal_keys_produce_autorun() {
        assert_eq!(
            translate_key(press(KeyCode::Char('Y')), true, true),
            Some(GameCommand::Autorun { dx: -1, dy: -1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('U')), true, true),
            Some(GameCommand::Autorun { dx: 1, dy: -1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('B')), true, true),
            Some(GameCommand::Autorun { dx: -1, dy: 1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('N')), true, true),
            Some(GameCommand::Autorun { dx: 1, dy: 1 })
        );
    }

    #[test]
    fn o_key_produces_auto_explore() {
        assert_eq!(
            translate_key(press(KeyCode::Char('o')), true, true),
            Some(GameCommand::AutoExplore)
        );
    }

    #[test]
    fn shift_arrow_keys_produce_autorun() {
        assert_eq!(
            translate_key(press_with(KeyCode::Up, KeyModifiers::SHIFT), true, true),
            Some(GameCommand::Autorun { dx: 0, dy: -1 })
        );
        assert_eq!(
            translate_key(press_with(KeyCode::Down, KeyModifiers::SHIFT), true, true),
            Some(GameCommand::Autorun { dx: 0, dy: 1 })
        );
        assert_eq!(
            translate_key(press_with(KeyCode::Left, KeyModifiers::SHIFT), true, true),
            Some(GameCommand::Autorun { dx: -1, dy: 0 })
        );
        assert_eq!(
            translate_key(press_with(KeyCode::Right, KeyModifiers::SHIFT), true, true),
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

    // --- vi_keys / numpad toggle tests ---

    #[test]
    fn vi_keys_disabled_ignores_hjkl() {
        assert_eq!(translate_key(press(KeyCode::Char('h')), false, true), None);
        assert_eq!(translate_key(press(KeyCode::Char('j')), false, true), None);
        assert_eq!(translate_key(press(KeyCode::Char('k')), false, true), None);
        assert_eq!(translate_key(press(KeyCode::Char('l')), false, true), None);
    }

    #[test]
    fn vi_keys_disabled_ignores_diagonal() {
        assert_eq!(translate_key(press(KeyCode::Char('y')), false, true), None);
        assert_eq!(translate_key(press(KeyCode::Char('u')), false, true), None);
        assert_eq!(translate_key(press(KeyCode::Char('b')), false, true), None);
        assert_eq!(translate_key(press(KeyCode::Char('n')), false, true), None);
    }

    #[test]
    fn vi_keys_disabled_ignores_autorun() {
        assert_eq!(translate_key(press(KeyCode::Char('H')), false, true), None);
        assert_eq!(translate_key(press(KeyCode::Char('L')), false, true), None);
    }

    #[test]
    fn numpad_disabled_ignores_digits() {
        assert_eq!(translate_key(press(KeyCode::Char('8')), true, false), None);
        assert_eq!(translate_key(press(KeyCode::Char('2')), true, false), None);
        assert_eq!(translate_key(press(KeyCode::Char('4')), true, false), None);
        assert_eq!(translate_key(press(KeyCode::Char('6')), true, false), None);
    }

    #[test]
    fn numpad_disabled_ignores_diagonals() {
        assert_eq!(translate_key(press(KeyCode::Char('7')), true, false), None);
        assert_eq!(translate_key(press(KeyCode::Char('9')), true, false), None);
        assert_eq!(translate_key(press(KeyCode::Char('1')), true, false), None);
        assert_eq!(translate_key(press(KeyCode::Char('3')), true, false), None);
    }

    #[test]
    fn arrows_work_with_both_disabled() {
        assert_eq!(
            translate_key(press(KeyCode::Up), false, false),
            Some(GameCommand::Move { dx: 0, dy: -1 })
        );
        assert_eq!(
            translate_key(press(KeyCode::Down), false, false),
            Some(GameCommand::Move { dx: 0, dy: 1 })
        );
    }

    #[test]
    fn wait_and_quit_work_with_both_disabled() {
        assert_eq!(
            translate_key(press(KeyCode::Char('.')), false, false),
            Some(GameCommand::Wait)
        );
        assert_eq!(
            translate_key(press(KeyCode::Esc), false, false),
            Some(GameCommand::Quit)
        );
    }

    #[test]
    fn numpad_disabled_wait_on_5_still_works() {
        // '5' is the wait key AND a numpad key. With numpad off,
        // '5' should NOT be treated as numpad movement — it falls through
        // to the wait match.
        assert_eq!(
            translate_key(press(KeyCode::Char('5')), true, false),
            Some(GameCommand::Wait)
        );
    }
}
