use std::io;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyModifiers};

use roguelike_core::look::LookCommand;
use roguelike_core::platform::MenuCommand;
use roguelike_core::settings::Settings;

use roguelike_tui::input;
use roguelike_tui::input_provider::{GameInput, HistoryInput, InputProvider, InputResult};

use crate::ansi_input::AnsiParser;
use crate::lobby::wait_for_key;

/// SSH input provider: mpsc channel + ANSI escape parser + resize watcher.
pub struct SshInput<'a> {
    pub rx: &'a Receiver<Vec<u8>>,
    pub parser: &'a mut AnsiParser,
    pub size_rx: &'a mut tokio::sync::watch::Receiver<(u32, u32)>,
}

impl<'a> InputProvider for SshInput<'a> {
    fn wait_for_key(&mut self) -> io::Result<InputResult<crossterm::event::KeyEvent>> {
        match wait_for_key(self.rx, self.parser)? {
            Some(key) => Ok(InputResult::Command(key)),
            None => Ok(InputResult::Disconnected),
        }
    }

    fn wait_for_game_input(&mut self, settings: &Settings) -> io::Result<GameInput> {
        match wait_for_key(self.rx, self.parser)? {
            Some(key) => {
                let command = input::translate_key(key, settings);
                Ok(GameInput::Key { key, command })
            }
            None => Ok(GameInput::Disconnected),
        }
    }

    fn wait_for_menu_command(&mut self) -> io::Result<InputResult<MenuCommand>> {
        match wait_for_key(self.rx, self.parser)? {
            Some(key) => match input::translate_menu_key(key) {
                Some(cmd) => Ok(InputResult::Command(cmd)),
                None => Ok(InputResult::NoCommand),
            },
            None => Ok(InputResult::Disconnected),
        }
    }

    fn wait_for_look_command(
        &mut self,
        settings: &Settings,
    ) -> io::Result<InputResult<LookCommand>> {
        match wait_for_key(self.rx, self.parser)? {
            Some(key) => match input::translate_look_key(key, settings) {
                Some(cmd) => Ok(InputResult::Command(cmd)),
                None => Ok(InputResult::NoCommand),
            },
            None => Ok(InputResult::Disconnected),
        }
    }

    fn wait_for_history_input(&mut self) -> io::Result<InputResult<HistoryInput>> {
        match wait_for_key(self.rx, self.parser)? {
            Some(key) => {
                match key.code {
                    KeyCode::PageUp => return Ok(InputResult::Command(HistoryInput::PageUp)),
                    KeyCode::PageDown => return Ok(InputResult::Command(HistoryInput::PageDown)),
                    KeyCode::Home => return Ok(InputResult::Command(HistoryInput::ScrollToTop)),
                    KeyCode::End => return Ok(InputResult::Command(HistoryInput::ScrollToBottom)),
                    KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(InputResult::Command(HistoryInput::HalfPageUp));
                    }
                    KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(InputResult::Command(HistoryInput::HalfPageDown));
                    }
                    _ => {}
                }
                match input::translate_menu_key(key) {
                    Some(cmd) => Ok(InputResult::Command(HistoryInput::Menu(cmd))),
                    None => Ok(InputResult::NoCommand),
                }
            }
            None => Ok(InputResult::Disconnected),
        }
    }

    fn poll_animation_interrupt(&mut self, timeout: Duration) -> io::Result<bool> {
        match self.rx.recv_timeout(timeout) {
            Ok(data) => {
                for &byte in &data {
                    let events = self.parser.feed(byte);
                    if !events.is_empty() {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Ok(false),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Ok(true),
        }
    }

    fn check_resize(&mut self) -> Option<(i32, i32)> {
        if self.size_rx.has_changed().unwrap_or(false) {
            let (w, h) = *self.size_rx.borrow_and_update();
            Some((w as i32, h as i32))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn make_input<'a>(
        rx: &'a Receiver<Vec<u8>>,
        parser: &'a mut AnsiParser,
        size_rx: &'a mut tokio::sync::watch::Receiver<(u32, u32)>,
    ) -> SshInput<'a> {
        SshInput {
            rx,
            parser,
            size_rx,
        }
    }

    #[test]
    fn check_resize_returns_none_when_unchanged() {
        let (_size_tx, mut size_rx) = tokio::sync::watch::channel((80, 24));
        // Mark initial value as seen.
        size_rx.borrow_and_update();
        let (_, rx) = mpsc::channel::<Vec<u8>>();
        let mut parser = AnsiParser::new();
        let mut input = make_input(&rx, &mut parser, &mut size_rx);

        assert_eq!(input.check_resize(), None);
    }

    #[test]
    fn check_resize_returns_new_size() {
        let (size_tx, mut size_rx) = tokio::sync::watch::channel((80, 24));
        // Mark initial value as seen.
        size_rx.borrow_and_update();
        let (_, rx) = mpsc::channel::<Vec<u8>>();
        let mut parser = AnsiParser::new();

        size_tx.send((120, 40)).unwrap();

        let mut input = make_input(&rx, &mut parser, &mut size_rx);
        assert_eq!(input.check_resize(), Some((120, 40)));
    }

    #[test]
    fn check_resize_clears_after_read() {
        let (size_tx, mut size_rx) = tokio::sync::watch::channel((80, 24));
        size_rx.borrow_and_update();
        let (_, rx) = mpsc::channel::<Vec<u8>>();
        let mut parser = AnsiParser::new();

        size_tx.send((132, 50)).unwrap();

        let mut input = make_input(&rx, &mut parser, &mut size_rx);
        assert!(input.check_resize().is_some());
        // Second call should return None — change was consumed.
        assert_eq!(input.check_resize(), None);
    }

    #[test]
    fn poll_animation_interrupt_timeout_returns_false() {
        let (_tx, rx) = mpsc::channel::<Vec<u8>>();
        let (_size_tx, mut size_rx) = tokio::sync::watch::channel((80, 24));
        let mut parser = AnsiParser::new();
        let mut input = make_input(&rx, &mut parser, &mut size_rx);

        // With no data sent, poll should time out and return false.
        let result = input.poll_animation_interrupt(Duration::from_millis(10)).unwrap();
        assert!(!result);
    }

    #[test]
    fn poll_animation_interrupt_disconnected_returns_true() {
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let (_size_tx, mut size_rx) = tokio::sync::watch::channel((80, 24));
        let mut parser = AnsiParser::new();

        // Drop sender to simulate disconnect.
        drop(tx);

        let mut input = make_input(&rx, &mut parser, &mut size_rx);
        let result = input.poll_animation_interrupt(Duration::from_millis(10)).unwrap();
        assert!(result);
    }
}
