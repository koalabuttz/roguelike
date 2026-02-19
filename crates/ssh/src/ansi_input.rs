use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use std::time::{Duration, Instant};

/// Timeout for distinguishing a bare Esc from the start of an escape sequence.
const ESC_TIMEOUT: Duration = Duration::from_millis(50);

/// Parser state machine for ANSI escape sequences.
///
/// Converts raw bytes from an SSH channel into crossterm `KeyEvent` structs
/// so the tui crate's `translate_key()` / `translate_menu_key()` /
/// `translate_look_key()` work without modification.
#[derive(Debug)]
pub struct AnsiParser {
    state: State,
    params: Vec<u8>,
    esc_time: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Normal,
    Escape,
    CsiParam,
}

impl AnsiParser {
    pub fn new() -> Self {
        Self {
            state: State::Normal,
            params: Vec::new(),
            esc_time: None,
        }
    }

    /// Feed a byte into the parser and return any resulting key events.
    pub fn feed(&mut self, byte: u8) -> Vec<KeyEvent> {
        let mut events = Vec::new();
        match self.state {
            State::Normal => self.handle_normal(byte, &mut events),
            State::Escape => self.handle_escape(byte, &mut events),
            State::CsiParam => self.handle_csi(byte, &mut events),
        }
        events
    }

    /// Check for a bare Esc timeout. Call this periodically when no data
    /// arrives. Returns `Some(KeyEvent)` if a bare Esc should be emitted.
    pub fn check_timeout(&mut self) -> Option<KeyEvent> {
        if self.state == State::Escape
            && let Some(t) = self.esc_time
            && t.elapsed() >= ESC_TIMEOUT
        {
            self.state = State::Normal;
            self.esc_time = None;
            return Some(key(KeyCode::Esc, KeyModifiers::NONE));
        }
        None
    }

    /// Returns true if the parser is waiting for more bytes of an escape
    /// sequence (i.e. in Escape or CsiParam state).
    pub fn pending(&self) -> bool {
        self.state != State::Normal
    }

    fn handle_normal(&mut self, byte: u8, events: &mut Vec<KeyEvent>) {
        match byte {
            0x1b => {
                self.state = State::Escape;
                self.esc_time = Some(Instant::now());
            }
            b'\r' | b'\n' => events.push(key(KeyCode::Enter, KeyModifiers::NONE)),
            b'\t' => events.push(key(KeyCode::Tab, KeyModifiers::NONE)),
            0x01..=0x1a => {
                // Ctrl+A through Ctrl+Z (excluding Tab/CR/LF matched above)
                let ch = (b'a' + byte - 1) as char;
                events.push(key(KeyCode::Char(ch), KeyModifiers::CONTROL));
            }
            0x7f => events.push(key(KeyCode::Backspace, KeyModifiers::NONE)),
            0x20..=0x7e => {
                let ch = byte as char;
                let mods = if ch.is_ascii_uppercase() {
                    KeyModifiers::SHIFT
                } else {
                    KeyModifiers::NONE
                };
                events.push(key(KeyCode::Char(ch), mods));
            }
            _ => {} // Ignore high bytes / non-printable
        }
    }

    fn handle_escape(&mut self, byte: u8, events: &mut Vec<KeyEvent>) {
        self.esc_time = None;
        match byte {
            b'[' => {
                self.state = State::CsiParam;
                self.params.clear();
            }
            b'O' => {
                // SS3 sequences (some terminals send these for F1-F4)
                self.state = State::CsiParam;
                self.params.clear();
                self.params.push(b'O'); // Tag so CsiParam knows it's SS3
            }
            0x1b => {
                // Double-Esc: emit Esc and stay in Escape state
                events.push(key(KeyCode::Esc, KeyModifiers::NONE));
                self.esc_time = Some(Instant::now());
            }
            _ => {
                // Alt+key or unrecognized — emit Esc then reprocess byte
                self.state = State::Normal;
                events.push(key(KeyCode::Esc, KeyModifiers::NONE));
                self.handle_normal(byte, events);
            }
        }
    }

    fn handle_csi(&mut self, byte: u8, events: &mut Vec<KeyEvent>) {
        match byte {
            b'0'..=b'9' | b';' => {
                self.params.push(byte);
            }
            b'~' => {
                // Extended key: CSI <number> ~
                let num = parse_param(&self.params);
                let ev = match num {
                    1 | 7 => Some(key(KeyCode::Home, KeyModifiers::NONE)),
                    4 | 8 => Some(key(KeyCode::End, KeyModifiers::NONE)),
                    5 => Some(key(KeyCode::PageUp, KeyModifiers::NONE)),
                    6 => Some(key(KeyCode::PageDown, KeyModifiers::NONE)),
                    _ => None,
                };
                if let Some(ev) = ev {
                    events.push(ev);
                }
                self.state = State::Normal;
            }
            b'A' => {
                let mods = modifier_from_params(&self.params);
                events.push(key(KeyCode::Up, mods));
                self.state = State::Normal;
            }
            b'B' => {
                let mods = modifier_from_params(&self.params);
                events.push(key(KeyCode::Down, mods));
                self.state = State::Normal;
            }
            b'C' => {
                let mods = modifier_from_params(&self.params);
                events.push(key(KeyCode::Right, mods));
                self.state = State::Normal;
            }
            b'D' => {
                let mods = modifier_from_params(&self.params);
                events.push(key(KeyCode::Left, mods));
                self.state = State::Normal;
            }
            b'H' => {
                events.push(key(KeyCode::Home, KeyModifiers::NONE));
                self.state = State::Normal;
            }
            b'F' => {
                events.push(key(KeyCode::End, KeyModifiers::NONE));
                self.state = State::Normal;
            }
            b'Z' => {
                // Shift+Tab
                events.push(key(KeyCode::BackTab, KeyModifiers::SHIFT));
                self.state = State::Normal;
            }
            _ => {
                // Unrecognized CSI sequence — discard
                self.state = State::Normal;
            }
        }
    }
}

/// Extract the modifier from CSI params like "1;2" (where 2 = Shift).
fn modifier_from_params(params: &[u8]) -> KeyModifiers {
    // Look for a semicolon — the number after it is the modifier code.
    let s = std::str::from_utf8(params).unwrap_or("");
    if let Some((_prefix, suffix)) = s.split_once(';') {
        match suffix.parse::<u8>() {
            Ok(2) => KeyModifiers::SHIFT,
            Ok(3) => KeyModifiers::ALT,
            Ok(5) => KeyModifiers::CONTROL,
            Ok(6) => KeyModifiers::SHIFT | KeyModifiers::CONTROL,
            _ => KeyModifiers::NONE,
        }
    } else {
        KeyModifiers::NONE
    }
}

/// Parse the first numeric parameter from CSI params.
fn parse_param(params: &[u8]) -> u16 {
    let s = std::str::from_utf8(params).unwrap_or("");
    let num_str = s.split(';').next().unwrap_or("");
    num_str.parse().unwrap_or(0)
}

/// Helper to construct a `KeyEvent`.
fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_bytes(parser: &mut AnsiParser, bytes: &[u8]) -> Vec<KeyEvent> {
        let mut all = Vec::new();
        for &b in bytes {
            all.extend(parser.feed(b));
        }
        all
    }

    fn assert_key(events: &[KeyEvent], code: KeyCode, mods: KeyModifiers) {
        assert_eq!(events.len(), 1, "Expected 1 event, got {:?}", events);
        assert_eq!(events[0].code, code);
        assert_eq!(events[0].modifiers, mods);
    }

    #[test]
    fn printable_ascii() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"a");
        assert_key(&events, KeyCode::Char('a'), KeyModifiers::NONE);
    }

    #[test]
    fn uppercase_has_shift_modifier() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"A");
        assert_key(&events, KeyCode::Char('A'), KeyModifiers::SHIFT);
    }

    #[test]
    fn enter_key() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\r");
        assert_key(&events, KeyCode::Enter, KeyModifiers::NONE);
    }

    #[test]
    fn tab_key() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\t");
        assert_key(&events, KeyCode::Tab, KeyModifiers::NONE);
    }

    #[test]
    fn backspace() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\x7f");
        assert_key(&events, KeyCode::Backspace, KeyModifiers::NONE);
    }

    #[test]
    fn ctrl_c() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\x03");
        assert_key(&events, KeyCode::Char('c'), KeyModifiers::CONTROL);
    }

    #[test]
    fn ctrl_p() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\x10");
        assert_key(&events, KeyCode::Char('p'), KeyModifiers::CONTROL);
    }

    #[test]
    fn ctrl_u() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\x15");
        assert_key(&events, KeyCode::Char('u'), KeyModifiers::CONTROL);
    }

    #[test]
    fn ctrl_d() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\x04");
        assert_key(&events, KeyCode::Char('d'), KeyModifiers::CONTROL);
    }

    #[test]
    fn arrow_up() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\x1b[A");
        assert_key(&events, KeyCode::Up, KeyModifiers::NONE);
    }

    #[test]
    fn arrow_down() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\x1b[B");
        assert_key(&events, KeyCode::Down, KeyModifiers::NONE);
    }

    #[test]
    fn arrow_right() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\x1b[C");
        assert_key(&events, KeyCode::Right, KeyModifiers::NONE);
    }

    #[test]
    fn arrow_left() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\x1b[D");
        assert_key(&events, KeyCode::Left, KeyModifiers::NONE);
    }

    #[test]
    fn shift_arrow_up() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\x1b[1;2A");
        assert_key(&events, KeyCode::Up, KeyModifiers::SHIFT);
    }

    #[test]
    fn shift_arrow_down() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\x1b[1;2B");
        assert_key(&events, KeyCode::Down, KeyModifiers::SHIFT);
    }

    #[test]
    fn shift_arrow_right() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\x1b[1;2C");
        assert_key(&events, KeyCode::Right, KeyModifiers::SHIFT);
    }

    #[test]
    fn shift_arrow_left() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\x1b[1;2D");
        assert_key(&events, KeyCode::Left, KeyModifiers::SHIFT);
    }

    #[test]
    fn home_key() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\x1b[H");
        assert_key(&events, KeyCode::Home, KeyModifiers::NONE);
    }

    #[test]
    fn end_key() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\x1b[F");
        assert_key(&events, KeyCode::End, KeyModifiers::NONE);
    }

    #[test]
    fn home_tilde() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\x1b[1~");
        assert_key(&events, KeyCode::Home, KeyModifiers::NONE);
    }

    #[test]
    fn end_tilde() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\x1b[4~");
        assert_key(&events, KeyCode::End, KeyModifiers::NONE);
    }

    #[test]
    fn page_up() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\x1b[5~");
        assert_key(&events, KeyCode::PageUp, KeyModifiers::NONE);
    }

    #[test]
    fn page_down() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\x1b[6~");
        assert_key(&events, KeyCode::PageDown, KeyModifiers::NONE);
    }

    #[test]
    fn bare_esc_timeout() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\x1b");
        assert!(events.is_empty());
        assert!(p.pending());

        // Simulate timeout
        p.esc_time = Some(Instant::now() - ESC_TIMEOUT);
        let ev = p.check_timeout();
        assert!(ev.is_some());
        assert_eq!(ev.unwrap().code, KeyCode::Esc);
        assert!(!p.pending());
    }

    #[test]
    fn multiple_keys_in_sequence() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"abc");
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].code, KeyCode::Char('a'));
        assert_eq!(events[1].code, KeyCode::Char('b'));
        assert_eq!(events[2].code, KeyCode::Char('c'));
    }

    #[test]
    fn vi_keys() {
        let mut p = AnsiParser::new();
        for &(byte, expected_char) in &[
            (b'h', 'h'),
            (b'j', 'j'),
            (b'k', 'k'),
            (b'l', 'l'),
            (b'y', 'y'),
            (b'u', 'u'),
            (b'b', 'b'),
            (b'n', 'n'),
        ] {
            let events = feed_bytes(&mut p, &[byte]);
            assert_key(&events, KeyCode::Char(expected_char), KeyModifiers::NONE);
        }
    }

    #[test]
    fn uppercase_vi_keys() {
        let mut p = AnsiParser::new();
        for &(byte, expected_char) in &[(b'H', 'H'), (b'J', 'J'), (b'K', 'K'), (b'L', 'L')] {
            let events = feed_bytes(&mut p, &[byte]);
            assert_key(&events, KeyCode::Char(expected_char), KeyModifiers::SHIFT);
        }
    }

    #[test]
    fn digits() {
        let mut p = AnsiParser::new();
        for digit in b'0'..=b'9' {
            let events = feed_bytes(&mut p, &[digit]);
            assert_key(&events, KeyCode::Char(digit as char), KeyModifiers::NONE);
        }
    }

    #[test]
    fn special_chars() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b".");
        assert_key(&events, KeyCode::Char('.'), KeyModifiers::NONE);

        let events = feed_bytes(&mut p, b"?");
        assert_key(&events, KeyCode::Char('?'), KeyModifiers::NONE);

        let events = feed_bytes(&mut p, b" ");
        assert_key(&events, KeyCode::Char(' '), KeyModifiers::NONE);
    }

    #[test]
    fn double_esc_emits_esc_and_stays_in_escape_state() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\x1b\x1b");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].code, KeyCode::Esc);
        assert!(p.pending()); // Still in Escape state waiting for next byte
    }

    #[test]
    fn shift_tab() {
        let mut p = AnsiParser::new();
        let events = feed_bytes(&mut p, b"\x1b[Z");
        assert_key(&events, KeyCode::BackTab, KeyModifiers::SHIFT);
    }

    #[test]
    fn esc_followed_by_regular_char() {
        let mut p = AnsiParser::new();
        // Esc followed by 'a' — should emit Esc then 'a'
        let events = feed_bytes(&mut p, b"\x1ba");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].code, KeyCode::Esc);
        assert_eq!(events[1].code, KeyCode::Char('a'));
    }
}
