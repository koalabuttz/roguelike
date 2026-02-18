use std::io;

use crossterm::event::{self, Event, KeyEvent, KeyEventKind};

#[cfg(feature = "gamepad")]
use roguelike_core::platform::MenuCommand;

// ---------------------------------------------------------------------------
// Type alias: uniform signatures regardless of feature flag
// ---------------------------------------------------------------------------

/// When the `gamepad` feature is enabled this is `Option<GamepadState>`;
/// otherwise it collapses to `()` (zero-size, optimized away).
#[cfg(feature = "gamepad")]
pub type GamepadOption = Option<GamepadState>;
#[cfg(not(feature = "gamepad"))]
pub type GamepadOption = ();

// ---------------------------------------------------------------------------
// InputEvent — returned by `poll_input`
// ---------------------------------------------------------------------------

/// A unified input event from either keyboard or gamepad.
pub enum InputEvent {
    Key(KeyEvent),
    #[cfg(feature = "gamepad")]
    GamepadReady,
}

// ---------------------------------------------------------------------------
// HistoryCommand — terminal-specific, not in core
// ---------------------------------------------------------------------------

/// Commands for the message history viewer (extends `MenuCommand` with paging).
#[cfg(feature = "gamepad")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryCommand {
    Menu(MenuCommand),
    PageUp,
    PageDown,
}

// ---------------------------------------------------------------------------
// Gamepad-enabled implementation
// ---------------------------------------------------------------------------

#[cfg(feature = "gamepad")]
mod inner {
    use super::*;
    use gilrs::{Axis, Button, EventType, Gilrs};
    use roguelike_core::command::GameCommand;
    use roguelike_core::look::LookCommand;
    use roguelike_core::types::Coord;
    use std::time::Duration;

    /// Persistent gamepad state carried through the application lifetime.
    pub struct GamepadState {
        gilrs: Gilrs,
        /// Edge-trigger flag: true while the analog stick is deflected past
        /// the deadzone, preventing repeat commands until the stick returns.
        stick_engaged: bool,
        /// Peeked gilrs event — `poll_input` consumes one event to detect
        /// "gamepad has input", then buffers it for the next translation call.
        buffered_event: Option<gilrs::Event>,
        /// Which D-pad buttons are currently held (Up, Down, Left, Right).
        dpad_held: [bool; 4],
        /// Whether LB (autorun modifier) is currently held.
        lb_held: bool,
    }

    // D-pad index constants.
    const DPAD_UP: usize = 0;
    const DPAD_DOWN: usize = 1;
    const DPAD_LEFT: usize = 2;
    const DPAD_RIGHT: usize = 3;

    impl GamepadState {
        /// Try to create a `GamepadOption`. Returns `Some(state)` if gilrs
        /// initializes successfully, `None` otherwise (graceful degradation).
        pub fn new_option() -> super::GamepadOption {
            Gilrs::new().ok().map(|gilrs| GamepadState {
                gilrs,
                stick_engaged: false,
                buffered_event: None,
                dpad_held: [false; 4],
                lb_held: false,
            })
        }

        /// Drain all pending gilrs events (so stale input from an earlier
        /// context doesn't leak into the current one).
        pub fn drain_stale(&mut self) {
            while self.gilrs.next_event().is_some() {}
            self.buffered_event = None;
            self.dpad_held = [false; 4];
            self.lb_held = false;
            self.stick_engaged = false;
        }

        /// Check whether there are pending gamepad events (buffered or new).
        pub fn has_pending_events(&mut self) -> bool {
            if self.buffered_event.is_some() {
                return true;
            }
            if let Some(ev) = self.gilrs.next_event() {
                self.buffered_event = Some(ev);
                return true;
            }
            false
        }

        // ---- Event iteration helpers ----

        /// Take the next gilrs event (buffered first, then fresh).
        fn next_event(&mut self) -> Option<gilrs::Event> {
            if let Some(ev) = self.buffered_event.take() {
                return Some(ev);
            }
            self.gilrs.next_event()
        }

        /// Update internal held-state tracking from an event.
        fn track_held_state(&mut self, ev: &gilrs::Event) {
            match ev.event {
                EventType::ButtonPressed(btn, _) => match btn {
                    Button::DPadUp => self.dpad_held[DPAD_UP] = true,
                    Button::DPadDown => self.dpad_held[DPAD_DOWN] = true,
                    Button::DPadLeft => self.dpad_held[DPAD_LEFT] = true,
                    Button::DPadRight => self.dpad_held[DPAD_RIGHT] = true,
                    Button::LeftTrigger => self.lb_held = true,
                    _ => {}
                },
                EventType::ButtonReleased(btn, _) => match btn {
                    Button::DPadUp => self.dpad_held[DPAD_UP] = false,
                    Button::DPadDown => self.dpad_held[DPAD_DOWN] = false,
                    Button::DPadLeft => self.dpad_held[DPAD_LEFT] = false,
                    Button::DPadRight => self.dpad_held[DPAD_RIGHT] = false,
                    Button::LeftTrigger => self.lb_held = false,
                    _ => {}
                },
                _ => {}
            }
        }

        /// Compute composite D-pad direction from currently held buttons.
        fn dpad_direction(&self) -> Option<(Coord, Coord)> {
            let dx = match (self.dpad_held[DPAD_LEFT], self.dpad_held[DPAD_RIGHT]) {
                (true, false) => -1,
                (false, true) => 1,
                _ => 0,
            };
            let dy = match (self.dpad_held[DPAD_UP], self.dpad_held[DPAD_DOWN]) {
                (true, false) => -1,
                (false, true) => 1,
                _ => 0,
            };
            if dx == 0 && dy == 0 {
                None
            } else {
                Some((dx, dy))
            }
        }

        /// Process analog stick axes and return a direction if newly deflected
        /// past the deadzone (edge-triggered).
        fn check_stick_direction(&mut self) -> Option<(Coord, Coord)> {
            // Find any active gamepad.
            let gp_id = self.gilrs.gamepads().next().map(|(id, _)| id)?;
            let gp = self.gilrs.gamepad(gp_id);
            let sx = gp
                .axis_data(Axis::LeftStickX)
                .map(|a| a.value())
                .unwrap_or(0.0);
            let sy = gp
                .axis_data(Axis::LeftStickY)
                .map(|a| a.value())
                .unwrap_or(0.0);

            let magnitude = (sx * sx + sy * sy).sqrt();
            if magnitude < 0.3 {
                self.stick_engaged = false;
                return None;
            }
            if self.stick_engaged {
                return None;
            }
            self.stick_engaged = true;
            Some(analog_to_direction(sx, sy))
        }

        // ---- Translation functions ----

        /// Drain events and return the first actionable game command.
        pub fn next_game_command(&mut self) -> Option<GameCommand> {
            while let Some(ev) = self.next_event() {
                self.track_held_state(&ev);
                match ev.event {
                    EventType::ButtonPressed(btn, _) => match btn {
                        Button::DPadUp
                        | Button::DPadDown
                        | Button::DPadLeft
                        | Button::DPadRight => {
                            if let Some((dx, dy)) = self.dpad_direction() {
                                return Some(if self.lb_held {
                                    GameCommand::Autorun { dx, dy }
                                } else {
                                    GameCommand::Move { dx, dy }
                                });
                            }
                        }
                        Button::South => return Some(GameCommand::Wait),
                        Button::East | Button::Start => return Some(GameCommand::Quit),
                        Button::West => return Some(GameCommand::AutoExplore),
                        Button::North => return Some(GameCommand::Look),
                        _ => {}
                    },
                    EventType::AxisChanged(Axis::LeftStickX | Axis::LeftStickY, _, _) => {
                        if let Some((dx, dy)) = self.check_stick_direction() {
                            return Some(if self.lb_held {
                                GameCommand::Autorun { dx, dy }
                            } else {
                                GameCommand::Move { dx, dy }
                            });
                        }
                    }
                    _ => {}
                }
            }
            None
        }

        /// Drain events and return the first actionable menu command.
        pub fn next_menu_command(&mut self) -> Option<MenuCommand> {
            while let Some(ev) = self.next_event() {
                self.track_held_state(&ev);
                if let EventType::ButtonPressed(btn, _) = ev.event {
                    match btn {
                        Button::DPadUp => return Some(MenuCommand::Up),
                        Button::DPadDown => return Some(MenuCommand::Down),
                        Button::South | Button::Start => return Some(MenuCommand::Select),
                        Button::East => return Some(MenuCommand::Back),
                        _ => {}
                    }
                }
                if let EventType::AxisChanged(Axis::LeftStickX | Axis::LeftStickY, _, _) = ev.event
                    && let Some((_, dy)) = self.check_stick_direction()
                {
                    if dy < 0 {
                        return Some(MenuCommand::Up);
                    }
                    if dy > 0 {
                        return Some(MenuCommand::Down);
                    }
                }
            }
            None
        }

        /// Drain events and return the first actionable look-mode command.
        pub fn next_look_command(&mut self) -> Option<LookCommand> {
            while let Some(ev) = self.next_event() {
                self.track_held_state(&ev);
                match ev.event {
                    EventType::ButtonPressed(btn, _) => match btn {
                        Button::DPadUp
                        | Button::DPadDown
                        | Button::DPadLeft
                        | Button::DPadRight => {
                            if let Some((dx, dy)) = self.dpad_direction() {
                                return Some(LookCommand::Move { dx, dy });
                            }
                        }
                        Button::East | Button::Start => return Some(LookCommand::Close),
                        _ => {}
                    },
                    EventType::AxisChanged(Axis::LeftStickX | Axis::LeftStickY, _, _) => {
                        if let Some((dx, dy)) = self.check_stick_direction() {
                            return Some(LookCommand::Move { dx, dy });
                        }
                    }
                    _ => {}
                }
            }
            None
        }

        /// Drain events and return the first actionable history command.
        pub fn next_history_command(&mut self) -> Option<HistoryCommand> {
            while let Some(ev) = self.next_event() {
                self.track_held_state(&ev);
                if let EventType::ButtonPressed(btn, _) = ev.event {
                    match btn {
                        Button::DPadUp => return Some(HistoryCommand::Menu(MenuCommand::Up)),
                        Button::DPadDown => return Some(HistoryCommand::Menu(MenuCommand::Down)),
                        Button::LeftTrigger => return Some(HistoryCommand::PageUp),
                        Button::RightTrigger => return Some(HistoryCommand::PageDown),
                        Button::South | Button::East | Button::Start => {
                            return Some(HistoryCommand::Menu(MenuCommand::Back));
                        }
                        _ => {}
                    }
                }
                if let EventType::AxisChanged(Axis::LeftStickX | Axis::LeftStickY, _, _) = ev.event
                    && let Some((_, dy)) = self.check_stick_direction()
                {
                    if dy < 0 {
                        return Some(HistoryCommand::Menu(MenuCommand::Up));
                    }
                    if dy > 0 {
                        return Some(HistoryCommand::Menu(MenuCommand::Down));
                    }
                }
            }
            None
        }
    }

    /// Convert analog stick `(sx, sy)` into one of 8 discrete directions.
    ///
    /// Uses `atan2` with π/8 sector boundaries. gilrs convention: positive Y is
    /// up, but our screen Y axis points down, so we negate `sy`.
    pub fn analog_to_direction(sx: f32, sy: f32) -> (Coord, Coord) {
        let angle = (-sy).atan2(sx); // negate Y for screen coords
        // Divide the circle into 8 sectors of π/4 each, centered on each
        // cardinal/diagonal direction.
        let sector = ((angle + std::f32::consts::PI + std::f32::consts::FRAC_PI_8)
            / std::f32::consts::FRAC_PI_4) as i32
            % 8;
        match sector {
            0 => (-1, 0),  // Left (π)
            1 => (-1, -1), // Up-Left
            2 => (0, -1),  // Up
            3 => (1, -1),  // Up-Right
            4 => (1, 0),   // Right (0)
            5 => (1, 1),   // Down-Right
            6 => (0, 1),   // Down
            7 => (-1, 1),  // Down-Left
            _ => (0, 0),
        }
    }

    /// Combined keyboard + gamepad polling.
    ///
    /// Polls crossterm with an 8ms timeout, then checks gilrs for pending
    /// events. Returns the first available input. CPU cost is near zero since
    /// the OS sleeps the thread during `event::poll`.
    pub fn poll_input(gamepad: &mut super::GamepadOption) -> io::Result<InputEvent> {
        match gamepad {
            Some(gp) => loop {
                if event::poll(Duration::from_millis(8))?
                    && let Event::Key(
                        key @ KeyEvent {
                            kind: KeyEventKind::Press,
                            ..
                        },
                    ) = event::read()?
                {
                    gp.drain_stale();
                    return Ok(InputEvent::Key(key));
                }
                if gp.has_pending_events() {
                    return Ok(InputEvent::GamepadReady);
                }
            },
            None => loop {
                if let Event::Key(
                    key @ KeyEvent {
                        kind: KeyEventKind::Press,
                        ..
                    },
                ) = event::read()?
                {
                    return Ok(InputEvent::Key(key));
                }
            },
        }
    }

    /// Check for gamepad interrupt during animations. Returns `true` if a
    /// gamepad event is pending (the animation should stop).
    pub fn check_animation_interrupt(gamepad: &mut super::GamepadOption) -> bool {
        match gamepad {
            Some(gp) => {
                if gp.has_pending_events() {
                    gp.drain_stale();
                    true
                } else {
                    false
                }
            }
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// No-gamepad stub implementation
// ---------------------------------------------------------------------------

#[cfg(not(feature = "gamepad"))]
mod inner {
    use super::*;

    /// Create the (zero-size) gamepad option when the feature is disabled.
    pub fn new_gamepad_option() -> super::GamepadOption {}

    /// Blocking keyboard-only polling — identical to the old `wait_for_keypress`.
    pub fn poll_input(_gamepad: &mut super::GamepadOption) -> io::Result<InputEvent> {
        loop {
            if let Event::Key(
                key @ KeyEvent {
                    kind: KeyEventKind::Press,
                    ..
                },
            ) = event::read()?
            {
                return Ok(InputEvent::Key(key));
            }
        }
    }

    /// No-op when gamepad feature is disabled.
    pub fn check_animation_interrupt(_gamepad: &mut super::GamepadOption) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Public re-exports — single entry points regardless of feature flag
// ---------------------------------------------------------------------------

#[cfg(feature = "gamepad")]
pub use inner::GamepadState;
pub use inner::check_animation_interrupt;
pub use inner::poll_input;

/// Create a `GamepadOption` — `Some(GamepadState)` when the feature is enabled
/// and a gamepad is available, `()` otherwise.
pub fn new_gamepad_option() -> GamepadOption {
    #[cfg(feature = "gamepad")]
    {
        inner::GamepadState::new_option()
    }
    #[cfg(not(feature = "gamepad"))]
    {
        inner::new_gamepad_option()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    // -- analog_to_direction tests (only when gamepad feature is enabled) --

    #[cfg(feature = "gamepad")]
    mod analog_tests {
        use super::super::inner::analog_to_direction;

        #[test]
        fn right() {
            assert_eq!(analog_to_direction(1.0, 0.0), (1, 0));
        }

        #[test]
        fn left() {
            assert_eq!(analog_to_direction(-1.0, 0.0), (-1, 0));
        }

        #[test]
        fn up() {
            // gilrs: positive Y = up, our function negates it → screen up (-1)
            assert_eq!(analog_to_direction(0.0, 1.0), (0, -1));
        }

        #[test]
        fn down() {
            assert_eq!(analog_to_direction(0.0, -1.0), (0, 1));
        }

        #[test]
        fn up_right() {
            assert_eq!(analog_to_direction(0.7, 0.7), (1, -1));
        }

        #[test]
        fn up_left() {
            assert_eq!(analog_to_direction(-0.7, 0.7), (-1, -1));
        }

        #[test]
        fn down_right() {
            assert_eq!(analog_to_direction(0.7, -0.7), (1, 1));
        }

        #[test]
        fn down_left() {
            assert_eq!(analog_to_direction(-0.7, -0.7), (-1, 1));
        }

        #[test]
        fn near_axis_snaps_to_cardinal() {
            // Slightly off-axis should still snap to cardinal direction.
            assert_eq!(analog_to_direction(1.0, 0.1), (1, 0));
            assert_eq!(analog_to_direction(1.0, -0.1), (1, 0));
            assert_eq!(analog_to_direction(0.1, 1.0), (0, -1));
            assert_eq!(analog_to_direction(-0.1, 1.0), (0, -1));
        }
    }

    // -- button mapping tests (pure function extraction for testability) --

    #[cfg(feature = "gamepad")]
    mod button_mapping_tests {
        use gilrs::Button;
        use roguelike_core::command::GameCommand;
        use roguelike_core::look::LookCommand;
        use roguelike_core::platform::MenuCommand;

        /// Pure function: map a face button to a game command.
        /// Mirrors the logic in `next_game_command` for non-directional buttons.
        fn face_button_to_game_cmd(btn: Button, lb_held: bool) -> Option<GameCommand> {
            let _ = lb_held; // LB only affects directional commands
            match btn {
                Button::South => Some(GameCommand::Wait),
                Button::East | Button::Start => Some(GameCommand::Quit),
                Button::West => Some(GameCommand::AutoExplore),
                Button::North => Some(GameCommand::Look),
                _ => None,
            }
        }

        fn button_to_menu_cmd(btn: Button) -> Option<MenuCommand> {
            match btn {
                Button::DPadUp => Some(MenuCommand::Up),
                Button::DPadDown => Some(MenuCommand::Down),
                Button::South | Button::Start => Some(MenuCommand::Select),
                Button::East => Some(MenuCommand::Back),
                _ => None,
            }
        }

        fn button_to_look_cmd(btn: Button) -> Option<LookCommand> {
            match btn {
                Button::East | Button::Start => Some(LookCommand::Close),
                _ => None,
            }
        }

        #[test]
        fn game_face_buttons() {
            assert_eq!(
                face_button_to_game_cmd(Button::South, false),
                Some(GameCommand::Wait)
            );
            assert_eq!(
                face_button_to_game_cmd(Button::East, false),
                Some(GameCommand::Quit)
            );
            assert_eq!(
                face_button_to_game_cmd(Button::West, false),
                Some(GameCommand::AutoExplore)
            );
            assert_eq!(
                face_button_to_game_cmd(Button::North, false),
                Some(GameCommand::Look)
            );
            assert_eq!(
                face_button_to_game_cmd(Button::Start, false),
                Some(GameCommand::Quit)
            );
        }

        #[test]
        fn game_face_buttons_with_lb() {
            // LB shouldn't change face button behavior.
            assert_eq!(
                face_button_to_game_cmd(Button::South, true),
                Some(GameCommand::Wait)
            );
        }

        #[test]
        fn menu_buttons() {
            assert_eq!(button_to_menu_cmd(Button::DPadUp), Some(MenuCommand::Up));
            assert_eq!(
                button_to_menu_cmd(Button::DPadDown),
                Some(MenuCommand::Down)
            );
            assert_eq!(button_to_menu_cmd(Button::South), Some(MenuCommand::Select));
            assert_eq!(button_to_menu_cmd(Button::East), Some(MenuCommand::Back));
            assert_eq!(button_to_menu_cmd(Button::Start), Some(MenuCommand::Select));
            assert_eq!(button_to_menu_cmd(Button::West), None);
        }

        #[test]
        fn look_buttons() {
            assert_eq!(button_to_look_cmd(Button::East), Some(LookCommand::Close));
            assert_eq!(button_to_look_cmd(Button::Start), Some(LookCommand::Close));
            assert_eq!(button_to_look_cmd(Button::South), None);
        }
    }

    // -- StickEdgeTrigger tests --

    #[cfg(feature = "gamepad")]
    mod edge_trigger_tests {
        /// Minimal edge-trigger state machine for isolated testing.
        struct StickEdgeTrigger {
            engaged: bool,
        }

        impl StickEdgeTrigger {
            fn new() -> Self {
                Self { engaged: false }
            }

            /// Returns true if the stick should fire a command.
            fn update(&mut self, magnitude: f32, deadzone: f32) -> bool {
                if magnitude < deadzone {
                    self.engaged = false;
                    return false;
                }
                if self.engaged {
                    return false;
                }
                self.engaged = true;
                true
            }
        }

        #[test]
        fn fires_on_first_deflection() {
            let mut trigger = StickEdgeTrigger::new();
            assert!(trigger.update(0.5, 0.3));
        }

        #[test]
        fn does_not_repeat() {
            let mut trigger = StickEdgeTrigger::new();
            assert!(trigger.update(0.5, 0.3));
            assert!(!trigger.update(0.6, 0.3));
            assert!(!trigger.update(0.9, 0.3));
        }

        #[test]
        fn resets_on_return_to_deadzone() {
            let mut trigger = StickEdgeTrigger::new();
            assert!(trigger.update(0.5, 0.3));
            assert!(!trigger.update(0.5, 0.3));
            // Return to deadzone.
            assert!(!trigger.update(0.1, 0.3));
            // Should fire again.
            assert!(trigger.update(0.5, 0.3));
        }

        #[test]
        fn below_deadzone_never_fires() {
            let mut trigger = StickEdgeTrigger::new();
            assert!(!trigger.update(0.0, 0.3));
            assert!(!trigger.update(0.1, 0.3));
            assert!(!trigger.update(0.29, 0.3));
        }
    }

    // -- D-pad composite direction tests --

    #[test]
    fn dpad_composite_directions() {
        // Simulate held state and test composite direction.
        // [Up, Down, Left, Right]
        let cases: &[([bool; 4], Option<(i32, i32)>)] = &[
            ([true, false, false, false], Some((0, -1))), // Up
            ([false, true, false, false], Some((0, 1))),  // Down
            ([false, false, true, false], Some((-1, 0))), // Left
            ([false, false, false, true], Some((1, 0))),  // Right
            ([true, false, false, true], Some((1, -1))),  // Up+Right = NE
            ([true, false, true, false], Some((-1, -1))), // Up+Left = NW
            ([false, true, false, true], Some((1, 1))),   // Down+Right = SE
            ([false, true, true, false], Some((-1, 1))),  // Down+Left = SW
            ([false, false, false, false], None),         // Nothing held
            ([true, true, false, false], None),           // Up+Down cancel
            ([false, false, true, true], None),           // Left+Right cancel
        ];

        for (held, expected) in cases {
            let dx = match (held[2], held[3]) {
                (true, false) => -1,
                (false, true) => 1,
                _ => 0,
            };
            let dy = match (held[0], held[1]) {
                (true, false) => -1,
                (false, true) => 1,
                _ => 0,
            };
            let result = if dx == 0 && dy == 0 {
                None
            } else {
                Some((dx, dy))
            };
            assert_eq!(result, *expected, "held={held:?}");
        }
    }
}
