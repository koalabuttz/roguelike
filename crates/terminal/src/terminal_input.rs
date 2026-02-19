use std::io;
use std::time::Duration;

use crossterm::event::{self, KeyCode, KeyModifiers};

use roguelike_core::look::LookCommand;
use roguelike_core::platform::MenuCommand;
use roguelike_core::settings::Settings;

use roguelike_tui::input;
use roguelike_tui::input_provider::{GameInput, HistoryInput, InputProvider, InputResult};

use crate::gamepad;

/// Terminal input provider: crossterm keyboard + optional gamepad.
pub struct TerminalInput {
    pub gp: gamepad::GamepadOption,
}

impl InputProvider for TerminalInput {
    fn wait_for_key(&mut self) -> io::Result<InputResult<crossterm::event::KeyEvent>> {
        match gamepad::poll_input(&mut self.gp)? {
            gamepad::InputEvent::Key(key) => Ok(InputResult::Command(key)),
            // Gamepad input during key-wait contexts (game-over, text input):
            // treat as "no command" for keyboard-only contexts, or handle
            // special cases (B button = cancel in text input).
            #[cfg(feature = "gamepad")]
            gamepad::InputEvent::GamepadReady => {
                // Check for B button (Back) which maps to Esc-like behavior.
                if let Some(g) = self.gp.as_mut()
                    && let Some(gamepad::HistoryCommand::Menu(MenuCommand::Back)) =
                        g.next_history_command()
                {
                    // Synthesize an Esc key event.
                    Ok(InputResult::Command(crossterm::event::KeyEvent::new(
                        KeyCode::Esc,
                        KeyModifiers::NONE,
                    )))
                } else {
                    Ok(InputResult::NoCommand)
                }
            }
        }
    }

    fn wait_for_game_input(&mut self, settings: &Settings) -> io::Result<GameInput> {
        match gamepad::poll_input(&mut self.gp)? {
            gamepad::InputEvent::Key(key) => {
                let command = input::translate_key(key, settings);
                Ok(GameInput::Key { key, command })
            }
            #[cfg(feature = "gamepad")]
            gamepad::InputEvent::GamepadReady => {
                let cmd = self.gp.as_mut().and_then(|g| g.next_game_command());
                match cmd {
                    Some(cmd) => Ok(GameInput::GamepadCommand(cmd)),
                    None => {
                        // Gamepad event was not translatable to a game command.
                        // Return a synthetic no-op key.
                        Ok(GameInput::Key {
                            key: crossterm::event::KeyEvent::new(KeyCode::Null, KeyModifiers::NONE),
                            command: None,
                        })
                    }
                }
            }
        }
    }

    fn wait_for_menu_command(&mut self) -> io::Result<InputResult<MenuCommand>> {
        match gamepad::poll_input(&mut self.gp)? {
            gamepad::InputEvent::Key(key) => match input::translate_menu_key(key) {
                Some(cmd) => Ok(InputResult::Command(cmd)),
                None => Ok(InputResult::NoCommand),
            },
            #[cfg(feature = "gamepad")]
            gamepad::InputEvent::GamepadReady => {
                match self.gp.as_mut().and_then(|g| g.next_menu_command()) {
                    Some(cmd) => Ok(InputResult::Command(cmd)),
                    None => Ok(InputResult::NoCommand),
                }
            }
        }
    }

    fn wait_for_look_command(
        &mut self,
        settings: &Settings,
    ) -> io::Result<InputResult<LookCommand>> {
        match gamepad::poll_input(&mut self.gp)? {
            gamepad::InputEvent::Key(key) => match input::translate_look_key(key, settings) {
                Some(cmd) => Ok(InputResult::Command(cmd)),
                None => Ok(InputResult::NoCommand),
            },
            #[cfg(feature = "gamepad")]
            gamepad::InputEvent::GamepadReady => {
                match self.gp.as_mut().and_then(|g| g.next_look_command()) {
                    Some(cmd) => Ok(InputResult::Command(cmd)),
                    None => Ok(InputResult::NoCommand),
                }
            }
        }
    }

    fn wait_for_history_input(&mut self) -> io::Result<InputResult<HistoryInput>> {
        match gamepad::poll_input(&mut self.gp)? {
            gamepad::InputEvent::Key(key) => {
                // Check paging keys first.
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
                // Fall through to menu key translation.
                match input::translate_menu_key(key) {
                    Some(cmd) => Ok(InputResult::Command(HistoryInput::Menu(cmd))),
                    None => Ok(InputResult::NoCommand),
                }
            }
            #[cfg(feature = "gamepad")]
            gamepad::InputEvent::GamepadReady => {
                match self.gp.as_mut().and_then(|g| g.next_history_command()) {
                    Some(gamepad::HistoryCommand::PageUp) => {
                        Ok(InputResult::Command(HistoryInput::PageUp))
                    }
                    Some(gamepad::HistoryCommand::PageDown) => {
                        Ok(InputResult::Command(HistoryInput::PageDown))
                    }
                    Some(gamepad::HistoryCommand::Menu(cmd)) => {
                        Ok(InputResult::Command(HistoryInput::Menu(cmd)))
                    }
                    None => Ok(InputResult::NoCommand),
                }
            }
        }
    }

    fn poll_animation_interrupt(&mut self, timeout: Duration) -> io::Result<bool> {
        // Check crossterm keyboard events.
        if event::poll(timeout)? {
            let _ = event::read()?;
            return Ok(true);
        }
        // Check gamepad.
        Ok(gamepad::check_animation_interrupt(&mut self.gp))
    }
}
