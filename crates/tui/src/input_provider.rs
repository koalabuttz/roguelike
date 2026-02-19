use std::io;
use std::time::Duration;

use crossterm::event::KeyEvent;

use roguelike_core::command::GameCommand;
use roguelike_core::look::LookCommand;
use roguelike_core::platform::MenuCommand;
use roguelike_core::settings::Settings;

/// Result of waiting for input, with disconnection support.
///
/// `Disconnected` is used by the SSH backend when the channel closes.
/// The terminal backend never returns it (keyboard input blocks forever).
pub enum InputResult<T> {
    /// A translated command was received.
    Command(T),
    /// An event was received but had no binding (unrecognized key).
    NoCommand,
    /// The input channel was closed (SSH disconnect).
    Disconnected,
}

/// Input for the main gameplay loop.
///
/// Separates keyboard events (which carry the raw key for dev-tool and
/// Ctrl+P interception) from gamepad events (which produce commands directly).
pub enum GameInput {
    /// A keyboard event with an optionally translated game command.
    Key {
        key: KeyEvent,
        command: Option<GameCommand>,
    },
    /// A gamepad-produced command (no raw key event available).
    GamepadCommand(GameCommand),
    /// The input channel was closed.
    Disconnected,
}

/// Commands for the full-screen message history viewer.
///
/// Extends `MenuCommand` with paging (PageUp/PageDown/half-page/scroll-to-end)
/// since the viewer needs scroll navigation that menus don't.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryInput {
    PageUp,
    PageDown,
    HalfPageUp,
    HalfPageDown,
    ScrollToTop,
    ScrollToBottom,
    Menu(MenuCommand),
}

/// Abstraction over input sources (keyboard+gamepad vs SSH channel).
///
/// Each method corresponds to a sub-loop in the game. The implementation
/// is responsible for polling the appropriate device(s) and translating
/// raw events into the typed commands that sub-loop expects.
pub trait InputProvider {
    /// Wait for a raw key event. Used for game-over "press any key" and
    /// text input dialogs where the caller needs character-level access.
    fn wait_for_key(&mut self) -> io::Result<InputResult<KeyEvent>>;

    /// Wait for gameplay input. Translates keyboard/gamepad events into
    /// `GameCommand`s using the provided settings for key binding lookup.
    fn wait_for_game_input(&mut self, settings: &Settings) -> io::Result<GameInput>;

    /// Wait for a menu navigation command (Up/Down/Select/Back).
    fn wait_for_menu_command(&mut self) -> io::Result<InputResult<MenuCommand>>;

    /// Wait for a look-mode command (cursor movement or close).
    fn wait_for_look_command(
        &mut self,
        settings: &Settings,
    ) -> io::Result<InputResult<LookCommand>>;

    /// Wait for a message history viewer command (paging or close).
    fn wait_for_history_input(&mut self) -> io::Result<InputResult<HistoryInput>>;

    /// Poll for input that should interrupt an animation.
    /// Returns `true` if any input was received within `timeout`.
    fn poll_animation_interrupt(&mut self, timeout: Duration) -> io::Result<bool>;

    /// Check for terminal resize. Returns `Some((cols, rows))` if the
    /// terminal was resized since the last check. Default: no resize support.
    fn check_resize(&mut self) -> Option<(i32, i32)> {
        None
    }
}
