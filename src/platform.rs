use crate::input::GameCommand;
use crate::types::{Coord, GameColor};

/// A platform-independent command for menu navigation.
///
/// Separate from `GameCommand` so menu logic doesn't depend on gameplay
/// concepts like movement deltas or autorun.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuCommand {
    Up,
    Down,
    Select,
    Back,
}

/// Abstraction over rendering output.
///
/// Implementations map `GameColor` to platform-native colors and handle
/// cursor positioning. All coordinates are in character-cell units.
pub trait Renderer {
    /// Clear the entire screen.
    fn clear(&mut self);

    /// Draw a character at `(x, y)` with foreground and background color.
    fn draw_char(&mut self, x: Coord, y: Coord, ch: char, fg: GameColor, bg: GameColor);

    /// Draw a string starting at `(x, y)` with foreground and background color.
    fn draw_str(&mut self, x: Coord, y: Coord, text: &str, fg: GameColor, bg: GameColor);

    /// Flush all pending draws to the screen.
    fn flush(&mut self);

    /// Screen dimensions `(width, height)` in character cells.
    fn screen_size(&self) -> (Coord, Coord);
}

/// Abstraction over input sources.
///
/// Implementations translate platform-native events (keyboard, gamepad,
/// touch) into game or menu commands.
pub trait InputSource {
    /// Block until the next game command is available.
    ///
    /// Returns `None` for key events that have no binding.
    fn next_command(&mut self) -> Option<GameCommand>;

    /// Block until the next menu command is available.
    ///
    /// Returns `None` for key events that have no binding.
    fn next_menu_command(&mut self) -> Option<MenuCommand>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mock renderer that records draw calls for testing.
    struct MockRenderer {
        chars: Vec<(Coord, Coord, char, GameColor, GameColor)>,
        strings: Vec<(Coord, Coord, String, GameColor, GameColor)>,
        cleared: bool,
        flushed: bool,
        width: Coord,
        height: Coord,
    }

    impl MockRenderer {
        fn new(width: Coord, height: Coord) -> Self {
            Self {
                chars: Vec::new(),
                strings: Vec::new(),
                cleared: false,
                flushed: false,
                width,
                height,
            }
        }
    }

    impl Renderer for MockRenderer {
        fn clear(&mut self) {
            self.cleared = true;
            self.chars.clear();
            self.strings.clear();
        }

        fn draw_char(&mut self, x: Coord, y: Coord, ch: char, fg: GameColor, bg: GameColor) {
            self.chars.push((x, y, ch, fg, bg));
        }

        fn draw_str(&mut self, x: Coord, y: Coord, text: &str, fg: GameColor, bg: GameColor) {
            self.strings.push((x, y, text.to_string(), fg, bg));
        }

        fn flush(&mut self) {
            self.flushed = true;
        }

        fn screen_size(&self) -> (Coord, Coord) {
            (self.width, self.height)
        }
    }

    #[test]
    fn mock_renderer_tracks_draw_calls() {
        let mut r = MockRenderer::new(80, 24);
        r.draw_char(5, 10, '@', GameColor::Yellow, GameColor::Black);
        r.draw_str(0, 0, "Hello", GameColor::White, GameColor::Black);
        r.flush();

        assert_eq!(r.chars.len(), 1);
        assert_eq!(
            r.chars[0],
            (5, 10, '@', GameColor::Yellow, GameColor::Black)
        );
        assert_eq!(r.strings.len(), 1);
        assert_eq!(r.strings[0].2, "Hello");
        assert!(r.flushed);
    }

    #[test]
    fn clear_resets_draw_state() {
        let mut r = MockRenderer::new(80, 24);
        r.draw_char(0, 0, 'x', GameColor::White, GameColor::Black);
        r.clear();

        assert!(r.cleared);
        assert!(r.chars.is_empty());
        assert!(r.strings.is_empty());
    }

    #[test]
    fn screen_size_returns_dimensions() {
        let r = MockRenderer::new(120, 40);
        assert_eq!(r.screen_size(), (120, 40));
    }
}
