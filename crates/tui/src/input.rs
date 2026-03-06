use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use roguelike_core::command::{Direction, GameCommand};
use roguelike_core::look::LookCommand;
use roguelike_core::platform::MenuCommand;
use roguelike_core::settings::{LeftHandLayout, Settings};

/// Resolve a key to a `Direction` and whether it triggers autorun.
///
/// Each input method uses its own autorun convention:
/// - Arrow keys: Shift = autorun
/// - Vi keys: uppercase = autorun
/// - Numpad: Shift = autorun
/// - Left-hand layouts: uppercase = autorun
pub fn resolve_direction(
    code: KeyCode,
    shift: bool,
    vi_keys: bool,
    numpad: bool,
    left_hand: LeftHandLayout,
) -> Option<(Direction, bool)> {
    use Direction::*;

    // Left-hand layouts (checked first — overrides vi/numpad for these keys).
    if left_hand != LeftHandLayout::Off {
        let result = match code {
            KeyCode::Char('q') => Some((NorthWest, false)),
            KeyCode::Char('Q') => Some((NorthWest, true)),
            KeyCode::Char('w') => Some((North, false)),
            KeyCode::Char('W') => Some((North, true)),
            KeyCode::Char('e') => Some((NorthEast, false)),
            KeyCode::Char('E') => Some((NorthEast, true)),
            KeyCode::Char('a') => Some((West, false)),
            KeyCode::Char('A') => Some((West, true)),
            KeyCode::Char('d') => Some((East, false)),
            KeyCode::Char('D') => Some((East, true)),
            KeyCode::Char('z') => Some((SouthWest, false)),
            KeyCode::Char('Z') => Some((SouthWest, true)),
            KeyCode::Char('x') => Some((South, false)),
            KeyCode::Char('X') => Some((South, true)),
            KeyCode::Char('c') => Some((SouthEast, false)),
            KeyCode::Char('C') => Some((SouthEast, true)),
            _ => None,
        };
        if result.is_some() {
            return result;
        }
    }

    match code {
        // Arrow keys: shift = autorun (always available)
        KeyCode::Up => Some((North, shift)),
        KeyCode::Down => Some((South, shift)),
        KeyCode::Left => Some((West, shift)),
        KeyCode::Right => Some((East, shift)),

        // Vi keys: uppercase = autorun
        KeyCode::Char('k') if vi_keys => Some((North, false)),
        KeyCode::Char('K') if vi_keys => Some((North, true)),
        KeyCode::Char('j') if vi_keys => Some((South, false)),
        KeyCode::Char('J') if vi_keys => Some((South, true)),
        KeyCode::Char('h') if vi_keys => Some((West, false)),
        KeyCode::Char('H') if vi_keys => Some((West, true)),
        KeyCode::Char('l') if vi_keys => Some((East, false)),
        KeyCode::Char('L') if vi_keys => Some((East, true)),
        KeyCode::Char('y') if vi_keys => Some((NorthWest, false)),
        KeyCode::Char('Y') if vi_keys => Some((NorthWest, true)),
        KeyCode::Char('u') if vi_keys => Some((NorthEast, false)),
        KeyCode::Char('U') if vi_keys => Some((NorthEast, true)),
        KeyCode::Char('b') if vi_keys => Some((SouthWest, false)),
        KeyCode::Char('B') if vi_keys => Some((SouthWest, true)),
        KeyCode::Char('n') if vi_keys => Some((SouthEast, false)),
        KeyCode::Char('N') if vi_keys => Some((SouthEast, true)),

        // Numpad: shift = autorun
        KeyCode::Char('8') if numpad => Some((North, shift)),
        KeyCode::Char('2') if numpad => Some((South, shift)),
        KeyCode::Char('4') if numpad => Some((West, shift)),
        KeyCode::Char('6') if numpad => Some((East, shift)),
        KeyCode::Char('7') if numpad => Some((NorthWest, shift)),
        KeyCode::Char('9') if numpad => Some((NorthEast, shift)),
        KeyCode::Char('1') if numpad => Some((SouthWest, shift)),
        KeyCode::Char('3') if numpad => Some((SouthEast, shift)),

        _ => None,
    }
}

/// Translate a crossterm key event into a game command.
///
/// Returns `None` for keys that have no binding, so the caller can
/// silently ignore them.
pub fn translate_key(key: KeyEvent, settings: &Settings) -> Option<GameCommand> {
    // Ctrl+C always quits, checked before any character bindings
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Some(GameCommand::Quit);
    }

    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    if let Some((dir, autorun)) = resolve_direction(
        key.code,
        shift,
        settings.vi_keys,
        settings.numpad,
        settings.left_hand_layout,
    ) {
        return Some(if autorun {
            GameCommand::Autorun(dir)
        } else {
            GameCommand::Move(dir)
        });
    }

    // Left-hand layout: 's' = wait (both QWEASDZXC and WEASDZXCR)
    if settings.left_hand_layout != LeftHandLayout::Off
        && matches!(key.code, KeyCode::Char('s') | KeyCode::Char('S'))
    {
        return Some(GameCommand::Wait);
    }

    match key.code {
        KeyCode::Char('g') | KeyCode::Char(',') => Some(GameCommand::Pickup),
        KeyCode::Char('i') => Some(GameCommand::OpenInventory),
        KeyCode::Char('o') => Some(GameCommand::AutoExplore),
        KeyCode::Char('>') => Some(GameCommand::Descend),
        KeyCode::Char('x') | KeyCode::Tab => Some(GameCommand::Look),
        KeyCode::Char('?') => Some(GameCommand::Help),
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

/// Translate a crossterm key event into a look-mode command.
///
/// Uses directional keys for cursor movement (ignoring autorun flag).
/// Esc, Tab, x, and q close look mode. When a left-hand layout is active,
/// x is a direction key, so Tab and Esc are the primary close keys.
pub fn translate_look_key(key: KeyEvent, settings: &Settings) -> Option<LookCommand> {
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    // Directional keys move the cursor (autorun flag is ignored).
    if let Some((dir, _)) = resolve_direction(
        key.code,
        shift,
        settings.vi_keys,
        settings.numpad,
        settings.left_hand_layout,
    ) {
        return Some(LookCommand::Move(dir));
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('x') | KeyCode::Char('q') | KeyCode::Tab => {
            Some(LookCommand::Close)
        }
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

    /// Build a Settings with vi_keys and numpad toggles, left-hand off.
    fn settings(vi_keys: bool, numpad: bool) -> Settings {
        Settings {
            vi_keys,
            numpad,
            ..Settings::default()
        }
    }

    /// Build a Settings with a specific left-hand layout.
    fn settings_left_hand(layout: LeftHandLayout) -> Settings {
        Settings {
            left_hand_layout: layout,
            ..Settings::default()
        }
    }

    #[test]
    fn arrow_keys_produce_cardinal_moves() {
        let s = settings(true, true);
        assert_eq!(
            translate_key(press(KeyCode::Up), &s),
            Some(GameCommand::Move(Direction::North))
        );
        assert_eq!(
            translate_key(press(KeyCode::Down), &s),
            Some(GameCommand::Move(Direction::South))
        );
        assert_eq!(
            translate_key(press(KeyCode::Left), &s),
            Some(GameCommand::Move(Direction::West))
        );
        assert_eq!(
            translate_key(press(KeyCode::Right), &s),
            Some(GameCommand::Move(Direction::East))
        );
    }

    #[test]
    fn vi_keys_produce_cardinal_moves() {
        let s = settings(true, true);
        assert_eq!(
            translate_key(press(KeyCode::Char('k')), &s),
            Some(GameCommand::Move(Direction::North))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('j')), &s),
            Some(GameCommand::Move(Direction::South))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('h')), &s),
            Some(GameCommand::Move(Direction::West))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('l')), &s),
            Some(GameCommand::Move(Direction::East))
        );
    }

    #[test]
    fn vi_diagonal_keys_produce_diagonal_moves() {
        let s = settings(true, true);
        assert_eq!(
            translate_key(press(KeyCode::Char('y')), &s),
            Some(GameCommand::Move(Direction::NorthWest))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('u')), &s),
            Some(GameCommand::Move(Direction::NorthEast))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('b')), &s),
            Some(GameCommand::Move(Direction::SouthWest))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('n')), &s),
            Some(GameCommand::Move(Direction::SouthEast))
        );
    }

    #[test]
    fn numpad_produces_all_eight_directions() {
        let s = settings(true, true);
        assert_eq!(
            translate_key(press(KeyCode::Char('8')), &s),
            Some(GameCommand::Move(Direction::North))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('2')), &s),
            Some(GameCommand::Move(Direction::South))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('4')), &s),
            Some(GameCommand::Move(Direction::West))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('6')), &s),
            Some(GameCommand::Move(Direction::East))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('7')), &s),
            Some(GameCommand::Move(Direction::NorthWest))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('9')), &s),
            Some(GameCommand::Move(Direction::NorthEast))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('1')), &s),
            Some(GameCommand::Move(Direction::SouthWest))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('3')), &s),
            Some(GameCommand::Move(Direction::SouthEast))
        );
    }

    #[test]
    fn shift_numpad_produces_autorun() {
        let s = settings(true, true);
        assert_eq!(
            translate_key(press_with(KeyCode::Char('8'), KeyModifiers::SHIFT), &s),
            Some(GameCommand::Autorun(Direction::North))
        );
        assert_eq!(
            translate_key(press_with(KeyCode::Char('2'), KeyModifiers::SHIFT), &s),
            Some(GameCommand::Autorun(Direction::South))
        );
        assert_eq!(
            translate_key(press_with(KeyCode::Char('4'), KeyModifiers::SHIFT), &s),
            Some(GameCommand::Autorun(Direction::West))
        );
        assert_eq!(
            translate_key(press_with(KeyCode::Char('6'), KeyModifiers::SHIFT), &s),
            Some(GameCommand::Autorun(Direction::East))
        );
        assert_eq!(
            translate_key(press_with(KeyCode::Char('7'), KeyModifiers::SHIFT), &s),
            Some(GameCommand::Autorun(Direction::NorthWest))
        );
        assert_eq!(
            translate_key(press_with(KeyCode::Char('9'), KeyModifiers::SHIFT), &s),
            Some(GameCommand::Autorun(Direction::NorthEast))
        );
        assert_eq!(
            translate_key(press_with(KeyCode::Char('1'), KeyModifiers::SHIFT), &s),
            Some(GameCommand::Autorun(Direction::SouthWest))
        );
        assert_eq!(
            translate_key(press_with(KeyCode::Char('3'), KeyModifiers::SHIFT), &s),
            Some(GameCommand::Autorun(Direction::SouthEast))
        );
    }

    #[test]
    fn wait_keys() {
        let s = settings(true, true);
        assert_eq!(
            translate_key(press(KeyCode::Char('.')), &s),
            Some(GameCommand::Wait)
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('5')), &s),
            Some(GameCommand::Wait)
        );
    }

    #[test]
    fn quit_keys() {
        let s = settings(true, true);
        assert_eq!(
            translate_key(press(KeyCode::Char('q')), &s),
            Some(GameCommand::Quit)
        );
        assert_eq!(
            translate_key(press(KeyCode::Esc), &s),
            Some(GameCommand::Quit)
        );
    }

    #[test]
    fn ctrl_c_quits() {
        let s = settings(true, true);
        assert_eq!(
            translate_key(press_with(KeyCode::Char('c'), KeyModifiers::CONTROL), &s),
            Some(GameCommand::Quit)
        );
    }

    #[test]
    fn x_key_produces_look() {
        let s = settings(true, true);
        assert_eq!(
            translate_key(press(KeyCode::Char('x')), &s),
            Some(GameCommand::Look)
        );
    }

    #[test]
    fn question_mark_produces_help() {
        let s = settings(true, true);
        assert_eq!(
            translate_key(press(KeyCode::Char('?')), &s),
            Some(GameCommand::Help)
        );
    }

    #[test]
    fn tab_key_produces_look() {
        let s = settings(true, true);
        assert_eq!(
            translate_key(press(KeyCode::Tab), &s),
            Some(GameCommand::Look)
        );
    }

    #[test]
    fn unbound_key_returns_none() {
        let s = settings(true, true);
        assert_eq!(translate_key(press(KeyCode::Char('z')), &s), None);
        assert_eq!(translate_key(press(KeyCode::F(1)), &s), None);
    }

    #[test]
    fn ctrl_c_takes_priority_over_character_bindings() {
        let s = settings(true, true);
        // 'c' alone is unbound, but Ctrl+C should still quit
        assert_eq!(translate_key(press(KeyCode::Char('c')), &s), None);
        assert_eq!(
            translate_key(press_with(KeyCode::Char('c'), KeyModifiers::CONTROL), &s),
            Some(GameCommand::Quit)
        );
    }

    #[test]
    fn uppercase_vi_keys_produce_autorun() {
        let s = settings(true, true);
        assert_eq!(
            translate_key(press(KeyCode::Char('K')), &s),
            Some(GameCommand::Autorun(Direction::North))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('J')), &s),
            Some(GameCommand::Autorun(Direction::South))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('H')), &s),
            Some(GameCommand::Autorun(Direction::West))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('L')), &s),
            Some(GameCommand::Autorun(Direction::East))
        );
    }

    #[test]
    fn uppercase_vi_diagonal_keys_produce_autorun() {
        let s = settings(true, true);
        assert_eq!(
            translate_key(press(KeyCode::Char('Y')), &s),
            Some(GameCommand::Autorun(Direction::NorthWest))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('U')), &s),
            Some(GameCommand::Autorun(Direction::NorthEast))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('B')), &s),
            Some(GameCommand::Autorun(Direction::SouthWest))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('N')), &s),
            Some(GameCommand::Autorun(Direction::SouthEast))
        );
    }

    #[test]
    fn o_key_produces_auto_explore() {
        let s = settings(true, true);
        assert_eq!(
            translate_key(press(KeyCode::Char('o')), &s),
            Some(GameCommand::AutoExplore)
        );
    }

    #[test]
    fn shift_arrow_keys_produce_autorun() {
        let s = settings(true, true);
        assert_eq!(
            translate_key(press_with(KeyCode::Up, KeyModifiers::SHIFT), &s),
            Some(GameCommand::Autorun(Direction::North))
        );
        assert_eq!(
            translate_key(press_with(KeyCode::Down, KeyModifiers::SHIFT), &s),
            Some(GameCommand::Autorun(Direction::South))
        );
        assert_eq!(
            translate_key(press_with(KeyCode::Left, KeyModifiers::SHIFT), &s),
            Some(GameCommand::Autorun(Direction::West))
        );
        assert_eq!(
            translate_key(press_with(KeyCode::Right, KeyModifiers::SHIFT), &s),
            Some(GameCommand::Autorun(Direction::East))
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
        let s = settings(false, true);
        assert_eq!(translate_key(press(KeyCode::Char('h')), &s), None);
        assert_eq!(translate_key(press(KeyCode::Char('j')), &s), None);
        assert_eq!(translate_key(press(KeyCode::Char('k')), &s), None);
        assert_eq!(translate_key(press(KeyCode::Char('l')), &s), None);
    }

    #[test]
    fn vi_keys_disabled_ignores_diagonal() {
        let s = settings(false, true);
        assert_eq!(translate_key(press(KeyCode::Char('y')), &s), None);
        assert_eq!(translate_key(press(KeyCode::Char('u')), &s), None);
        assert_eq!(translate_key(press(KeyCode::Char('b')), &s), None);
        assert_eq!(translate_key(press(KeyCode::Char('n')), &s), None);
    }

    #[test]
    fn vi_keys_disabled_ignores_autorun() {
        let s = settings(false, true);
        assert_eq!(translate_key(press(KeyCode::Char('H')), &s), None);
        assert_eq!(translate_key(press(KeyCode::Char('L')), &s), None);
    }

    #[test]
    fn numpad_disabled_ignores_digits() {
        let s = settings(true, false);
        assert_eq!(translate_key(press(KeyCode::Char('8')), &s), None);
        assert_eq!(translate_key(press(KeyCode::Char('2')), &s), None);
        assert_eq!(translate_key(press(KeyCode::Char('4')), &s), None);
        assert_eq!(translate_key(press(KeyCode::Char('6')), &s), None);
    }

    #[test]
    fn numpad_disabled_ignores_diagonals() {
        let s = settings(true, false);
        assert_eq!(translate_key(press(KeyCode::Char('7')), &s), None);
        assert_eq!(translate_key(press(KeyCode::Char('9')), &s), None);
        assert_eq!(translate_key(press(KeyCode::Char('1')), &s), None);
        assert_eq!(translate_key(press(KeyCode::Char('3')), &s), None);
    }

    #[test]
    fn arrows_work_with_both_disabled() {
        let s = settings(false, false);
        assert_eq!(
            translate_key(press(KeyCode::Up), &s),
            Some(GameCommand::Move(Direction::North))
        );
        assert_eq!(
            translate_key(press(KeyCode::Down), &s),
            Some(GameCommand::Move(Direction::South))
        );
    }

    #[test]
    fn wait_and_quit_work_with_both_disabled() {
        let s = settings(false, false);
        assert_eq!(
            translate_key(press(KeyCode::Char('.')), &s),
            Some(GameCommand::Wait)
        );
        assert_eq!(
            translate_key(press(KeyCode::Esc), &s),
            Some(GameCommand::Quit)
        );
    }

    #[test]
    fn numpad_disabled_wait_on_5_still_works() {
        let s = settings(true, false);
        assert_eq!(
            translate_key(press(KeyCode::Char('5')), &s),
            Some(GameCommand::Wait)
        );
    }

    // --- Left-hand layout tests ---

    #[test]
    fn left_hand_qweasdzxc_directions() {
        let s = settings_left_hand(LeftHandLayout::Qweasdzxc);
        assert_eq!(
            translate_key(press(KeyCode::Char('q')), &s),
            Some(GameCommand::Move(Direction::NorthWest))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('w')), &s),
            Some(GameCommand::Move(Direction::North))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('e')), &s),
            Some(GameCommand::Move(Direction::NorthEast))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('a')), &s),
            Some(GameCommand::Move(Direction::West))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('d')), &s),
            Some(GameCommand::Move(Direction::East))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('z')), &s),
            Some(GameCommand::Move(Direction::SouthWest))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('x')), &s),
            Some(GameCommand::Move(Direction::South))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('c')), &s),
            Some(GameCommand::Move(Direction::SouthEast))
        );
    }

    #[test]
    fn left_hand_s_is_wait() {
        let s = settings_left_hand(LeftHandLayout::Qweasdzxc);
        assert_eq!(
            translate_key(press(KeyCode::Char('s')), &s),
            Some(GameCommand::Wait)
        );
    }

    #[test]
    fn left_hand_uppercase_is_autorun() {
        let s = settings_left_hand(LeftHandLayout::Qweasdzxc);
        assert_eq!(
            translate_key(press(KeyCode::Char('Q')), &s),
            Some(GameCommand::Autorun(Direction::NorthWest))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('W')), &s),
            Some(GameCommand::Autorun(Direction::North))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('D')), &s),
            Some(GameCommand::Autorun(Direction::East))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('X')), &s),
            Some(GameCommand::Autorun(Direction::South))
        );
    }

    #[test]
    fn left_hand_q_overrides_quit() {
        // With left-hand active, 'q' is NW direction, not quit.
        let s = settings_left_hand(LeftHandLayout::Qweasdzxc);
        assert_eq!(
            translate_key(press(KeyCode::Char('q')), &s),
            Some(GameCommand::Move(Direction::NorthWest))
        );
        // Esc still quits.
        assert_eq!(
            translate_key(press(KeyCode::Esc), &s),
            Some(GameCommand::Quit)
        );
    }

    #[test]
    fn left_hand_x_overrides_look() {
        // With left-hand active, 'x' is South, not look.
        let s = settings_left_hand(LeftHandLayout::Qweasdzxc);
        assert_eq!(
            translate_key(press(KeyCode::Char('x')), &s),
            Some(GameCommand::Move(Direction::South))
        );
        // Tab still works for look.
        assert_eq!(
            translate_key(press(KeyCode::Tab), &s),
            Some(GameCommand::Look)
        );
    }

    #[test]
    fn left_hand_weasdzxcr_directions() {
        let s = settings_left_hand(LeftHandLayout::Weasdzxcr);
        assert_eq!(
            translate_key(press(KeyCode::Char('w')), &s),
            Some(GameCommand::Move(Direction::North))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('a')), &s),
            Some(GameCommand::Move(Direction::West))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('d')), &s),
            Some(GameCommand::Move(Direction::East))
        );
        assert_eq!(
            translate_key(press(KeyCode::Char('x')), &s),
            Some(GameCommand::Move(Direction::South))
        );
    }

    #[test]
    fn left_hand_off_q_is_quit() {
        // With left-hand off, 'q' is quit as usual.
        let s = settings_left_hand(LeftHandLayout::Off);
        assert_eq!(
            translate_key(press(KeyCode::Char('q')), &s),
            Some(GameCommand::Quit)
        );
    }

    // --- Look mode key tests ---

    #[test]
    fn look_directional_keys() {
        let s = settings(true, true);
        assert_eq!(
            translate_look_key(press(KeyCode::Up), &s),
            Some(LookCommand::Move(Direction::North))
        );
        assert_eq!(
            translate_look_key(press(KeyCode::Right), &s),
            Some(LookCommand::Move(Direction::East))
        );
        assert_eq!(
            translate_look_key(press(KeyCode::Char('h')), &s),
            Some(LookCommand::Move(Direction::West))
        );
        assert_eq!(
            translate_look_key(press(KeyCode::Char('7')), &s),
            Some(LookCommand::Move(Direction::NorthWest))
        );
    }

    #[test]
    fn look_shift_direction_moves_one_tile() {
        let s = settings(true, true);
        assert_eq!(
            translate_look_key(press_with(KeyCode::Up, KeyModifiers::SHIFT), &s),
            Some(LookCommand::Move(Direction::North))
        );
        assert_eq!(
            translate_look_key(press(KeyCode::Char('H')), &s),
            Some(LookCommand::Move(Direction::West))
        );
    }

    #[test]
    fn look_close_keys() {
        let s = settings(true, true);
        assert_eq!(
            translate_look_key(press(KeyCode::Esc), &s),
            Some(LookCommand::Close)
        );
        assert_eq!(
            translate_look_key(press(KeyCode::Char('x')), &s),
            Some(LookCommand::Close)
        );
        assert_eq!(
            translate_look_key(press(KeyCode::Char('q')), &s),
            Some(LookCommand::Close)
        );
        assert_eq!(
            translate_look_key(press(KeyCode::Tab), &s),
            Some(LookCommand::Close)
        );
    }

    #[test]
    fn look_unbound_key_returns_none() {
        let s = settings(true, true);
        assert_eq!(translate_look_key(press(KeyCode::Char('z')), &s), None);
    }

    #[test]
    fn g_key_produces_pickup() {
        let s = settings(true, true);
        assert_eq!(
            translate_key(press(KeyCode::Char('g')), &s),
            Some(GameCommand::Pickup)
        );
    }

    #[test]
    fn comma_key_produces_pickup() {
        let s = settings(true, true);
        assert_eq!(
            translate_key(press(KeyCode::Char(',')), &s),
            Some(GameCommand::Pickup)
        );
    }

    #[test]
    fn i_key_produces_open_inventory() {
        let s = settings(true, true);
        assert_eq!(
            translate_key(press(KeyCode::Char('i')), &s),
            Some(GameCommand::OpenInventory)
        );
    }

    #[test]
    fn look_left_hand_uses_directions() {
        let s = settings_left_hand(LeftHandLayout::Qweasdzxc);
        // 'x' should move south in look mode with left-hand, not close.
        assert_eq!(
            translate_look_key(press(KeyCode::Char('x')), &s),
            Some(LookCommand::Move(Direction::South))
        );
        // Tab closes look mode.
        assert_eq!(
            translate_look_key(press(KeyCode::Tab), &s),
            Some(LookCommand::Close)
        );
    }
}
