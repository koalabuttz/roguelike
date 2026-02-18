use crate::platform::{MenuCommand, Renderer};
use crate::types::GameColor;

/// Result of handling a viewer input event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerAction {
    /// The viewer should remain open.
    Continue,
    /// The viewer should close.
    Close,
}

/// Full-screen scrollable viewer for the message log.
///
/// Borrows a slice of messages and provides scroll navigation.
/// Rendering uses the `Renderer` trait so all platforms get this for free.
pub struct MessageHistoryViewer<'a> {
    messages: &'a [String],
    /// Index of the first visible message (0 = oldest).
    scroll_offset: usize,
}

impl<'a> MessageHistoryViewer<'a> {
    pub fn new(messages: &'a [String]) -> Self {
        // Start scrolled to the bottom (most recent messages visible).
        Self {
            messages,
            scroll_offset: messages.len(),
        }
    }

    /// Scroll up (toward older messages) by `n` lines.
    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    /// Scroll down (toward newer messages) by `n` lines.
    pub fn scroll_down(&mut self, n: usize) {
        self.scroll_offset = (self.scroll_offset + n).min(self.messages.len());
    }

    /// Page up by `n` lines (typically screen height - 2 for header/footer).
    pub fn page_up(&mut self, n: usize) {
        self.scroll_up(n);
    }

    /// Page down by `n` lines.
    pub fn page_down(&mut self, n: usize) {
        self.scroll_down(n);
    }

    /// Handle a menu-style input command.
    pub fn handle_input(&mut self, cmd: MenuCommand) -> ViewerAction {
        match cmd {
            MenuCommand::Up => {
                self.scroll_up(1);
                ViewerAction::Continue
            }
            MenuCommand::Down => {
                self.scroll_down(1);
                ViewerAction::Continue
            }
            MenuCommand::Back | MenuCommand::Select => ViewerAction::Close,
        }
    }

    /// Render the viewer full-screen.
    ///
    /// Layout:
    /// - Row 0: title bar with position indicator
    /// - Rows 1..h-1: messages (newest at bottom, matching game log convention)
    /// - Most recent message is white, older messages are grey
    pub fn draw(&self, renderer: &mut dyn Renderer) {
        let (screen_w, screen_h) = renderer.screen_size();
        let content_height = (screen_h - 1).max(0) as usize; // rows available for messages

        renderer.clear();

        // Title bar.
        let position = if self.messages.is_empty() {
            "empty".to_string()
        } else {
            format!("{}/{}", self.scroll_offset, self.messages.len())
        };
        let title = format!(" Message History [{}]", position);
        let padded: String = title
            .chars()
            .chain(std::iter::repeat(' '))
            .take(screen_w as usize)
            .collect();
        renderer.draw_str(0, 0, &padded, GameColor::Cyan, GameColor::DarkBlue);

        if self.messages.is_empty() || content_height == 0 {
            renderer.flush();
            return;
        }

        // We display messages such that scroll_offset points to the bottom
        // of the visible area (the last visible message index + 1).
        // This means messages[scroll_offset - content_height .. scroll_offset]
        // are shown, with the newest at the bottom row.
        let end = self.scroll_offset.min(self.messages.len());
        let start = end.saturating_sub(content_height);
        let visible = &self.messages[start..end];

        // Render messages bottom-aligned: if fewer messages than content_height,
        // they appear at the bottom of the screen.
        let empty_rows = content_height - visible.len();

        for (i, msg) in visible.iter().enumerate() {
            let row = (1 + empty_rows + i) as i32;
            let is_newest = start + i == self.messages.len() - 1;
            let fg = if is_newest {
                GameColor::White
            } else {
                GameColor::Grey
            };

            // Truncate to screen width (word wrapping can be added later).
            let display: String = format!(" {}", msg)
                .chars()
                .take(screen_w as usize)
                .collect();
            renderer.draw_str(0, row, &display, fg, GameColor::Black);
        }

        renderer.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Coord;

    /// A mock renderer that records draw calls for testing.
    struct MockRenderer {
        strings: Vec<(Coord, Coord, String, GameColor, GameColor)>,
        cleared: bool,
        flushed: bool,
        width: Coord,
        height: Coord,
    }

    impl MockRenderer {
        fn new(width: Coord, height: Coord) -> Self {
            Self {
                strings: Vec::new(),
                cleared: false,
                flushed: false,
                width,
                height,
            }
        }

        fn find_str(&self, needle: &str) -> Option<&(Coord, Coord, String, GameColor, GameColor)> {
            self.strings
                .iter()
                .find(|(_, _, text, _, _)| text.contains(needle))
        }
    }

    impl Renderer for MockRenderer {
        fn clear(&mut self) {
            self.cleared = true;
            self.strings.clear();
        }

        fn draw_char(&mut self, _x: Coord, _y: Coord, _ch: char, _fg: GameColor, _bg: GameColor) {}

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

    fn sample_messages(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("Message {}", i)).collect()
    }

    #[test]
    fn new_viewer_starts_at_bottom() {
        let msgs = sample_messages(20);
        let viewer = MessageHistoryViewer::new(&msgs);
        assert_eq!(viewer.scroll_offset, 20);
    }

    #[test]
    fn scroll_up_moves_toward_older() {
        let msgs = sample_messages(20);
        let mut viewer = MessageHistoryViewer::new(&msgs);
        viewer.scroll_up(5);
        assert_eq!(viewer.scroll_offset, 15);
    }

    #[test]
    fn scroll_up_clamps_at_zero() {
        let msgs = sample_messages(5);
        let mut viewer = MessageHistoryViewer::new(&msgs);
        viewer.scroll_up(100);
        assert_eq!(viewer.scroll_offset, 0);
    }

    #[test]
    fn scroll_down_moves_toward_newer() {
        let msgs = sample_messages(20);
        let mut viewer = MessageHistoryViewer::new(&msgs);
        viewer.scroll_up(10); // offset = 10
        viewer.scroll_down(3);
        assert_eq!(viewer.scroll_offset, 13);
    }

    #[test]
    fn scroll_down_clamps_at_len() {
        let msgs = sample_messages(10);
        let mut viewer = MessageHistoryViewer::new(&msgs);
        viewer.scroll_down(100);
        assert_eq!(viewer.scroll_offset, 10);
    }

    #[test]
    fn page_up_and_down() {
        let msgs = sample_messages(50);
        let mut viewer = MessageHistoryViewer::new(&msgs);
        viewer.page_up(20);
        assert_eq!(viewer.scroll_offset, 30);
        viewer.page_down(10);
        assert_eq!(viewer.scroll_offset, 40);
    }

    #[test]
    fn handle_input_up_scrolls_up() {
        let msgs = sample_messages(10);
        let mut viewer = MessageHistoryViewer::new(&msgs);
        let action = viewer.handle_input(MenuCommand::Up);
        assert_eq!(action, ViewerAction::Continue);
        assert_eq!(viewer.scroll_offset, 9);
    }

    #[test]
    fn handle_input_down_scrolls_down() {
        let msgs = sample_messages(10);
        let mut viewer = MessageHistoryViewer::new(&msgs);
        viewer.scroll_up(5);
        let action = viewer.handle_input(MenuCommand::Down);
        assert_eq!(action, ViewerAction::Continue);
        assert_eq!(viewer.scroll_offset, 6);
    }

    #[test]
    fn handle_input_back_closes() {
        let msgs = sample_messages(5);
        let mut viewer = MessageHistoryViewer::new(&msgs);
        assert_eq!(viewer.handle_input(MenuCommand::Back), ViewerAction::Close);
    }

    #[test]
    fn handle_input_select_closes() {
        let msgs = sample_messages(5);
        let mut viewer = MessageHistoryViewer::new(&msgs);
        assert_eq!(
            viewer.handle_input(MenuCommand::Select),
            ViewerAction::Close
        );
    }

    #[test]
    fn draw_empty_log() {
        let msgs: Vec<String> = Vec::new();
        let viewer = MessageHistoryViewer::new(&msgs);
        let mut r = MockRenderer::new(80, 24);
        viewer.draw(&mut r);

        assert!(r.cleared);
        assert!(r.flushed);
        assert!(r.find_str("empty").is_some());
    }

    #[test]
    fn draw_shows_title_bar() {
        let msgs = sample_messages(10);
        let viewer = MessageHistoryViewer::new(&msgs);
        let mut r = MockRenderer::new(80, 24);
        viewer.draw(&mut r);

        let title = r.find_str("Message History").unwrap();
        assert_eq!(title.1, 0); // row 0
        assert_eq!(title.3, GameColor::Cyan);
        assert_eq!(title.4, GameColor::DarkBlue);
    }

    #[test]
    fn draw_shows_position_indicator() {
        let msgs = sample_messages(10);
        let viewer = MessageHistoryViewer::new(&msgs);
        let mut r = MockRenderer::new(80, 24);
        viewer.draw(&mut r);

        assert!(r.find_str("10/10").is_some());
    }

    #[test]
    fn draw_newest_message_is_white() {
        let msgs = sample_messages(5);
        let viewer = MessageHistoryViewer::new(&msgs);
        let mut r = MockRenderer::new(80, 24);
        viewer.draw(&mut r);

        let newest = r.find_str("Message 4").unwrap();
        assert_eq!(newest.3, GameColor::White);
    }

    #[test]
    fn draw_older_messages_are_grey() {
        let msgs = sample_messages(5);
        let viewer = MessageHistoryViewer::new(&msgs);
        let mut r = MockRenderer::new(80, 24);
        viewer.draw(&mut r);

        let older = r.find_str("Message 0").unwrap();
        assert_eq!(older.3, GameColor::Grey);
    }

    #[test]
    fn draw_messages_bottom_aligned() {
        // With 3 messages on a 24-row screen (23 content rows),
        // messages should appear at rows 21, 22, 23 (bottom).
        let msgs = sample_messages(3);
        let viewer = MessageHistoryViewer::new(&msgs);
        let mut r = MockRenderer::new(80, 24);
        viewer.draw(&mut r);

        let m0 = r.find_str("Message 0").unwrap();
        let m1 = r.find_str("Message 1").unwrap();
        let m2 = r.find_str("Message 2").unwrap();
        // content_height = 23, empty_rows = 20, so first msg at row 1+20=21
        assert_eq!(m0.1, 21);
        assert_eq!(m1.1, 22);
        assert_eq!(m2.1, 23);
    }
}
