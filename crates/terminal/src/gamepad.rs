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
// Raw HID submodule — generic USB HID gamepad support
// ---------------------------------------------------------------------------

#[cfg(feature = "raw-usb")]
mod raw_hid {
    use rusb::{Context, DeviceHandle, UsbContext};
    use std::time::Duration;

    // USB HID interface class code.
    const HID_IFACE_CLASS: u8 = 0x03;

    // buttons1 bits — 8BitDo SN30 Pro DirectInput (D mode).
    // Button numbering follows USB HID report order.
    // Names use Xbox/positional convention (A=South, B=East, X=West, Y=North).
    // Verify experimentally with `usb_hid_test` if using a different controller.
    pub const HID_BTN_A: u8 = 0x02; // South face button
    pub const HID_BTN_B: u8 = 0x01; // East face button
    pub const HID_BTN_X: u8 = 0x08; // West face button
    pub const HID_BTN_Y: u8 = 0x04; // North face button
    pub const HID_BTN_LB: u8 = 0x10; // Left shoulder
    pub const HID_BTN_RB: u8 = 0x20; // Right shoulder

    // buttons2 bits
    pub const HID_BTN_SELECT: u8 = 0x01;
    pub const HID_BTN_START: u8 = 0x02;

    /// Parsed HID input report from a DirectInput gamepad.
    ///
    /// Layout: `[buttons1] [buttons2] [hat] [LX] [LY] [RX] [RY] [extra...]`
    /// Sticks are unsigned 0x00-0xFF with 0x80 as center.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct HidInputReport {
        pub buttons1: u8,
        pub buttons2: u8,
        pub hat: u8,
        pub left_stick_x: u8,
        pub left_stick_y: u8,
        pub right_stick_x: u8,
        pub right_stick_y: u8,
    }

    impl Default for HidInputReport {
        fn default() -> Self {
            Self {
                buttons1: 0,
                buttons2: 0,
                hat: 0x08, // centered
                left_stick_x: 0x80,
                left_stick_y: 0x80,
                right_stick_x: 0x80,
                right_stick_y: 0x80,
            }
        }
    }

    impl HidInputReport {
        /// Parse from a 7+ byte buffer.
        pub fn parse(buf: &[u8]) -> Self {
            Self {
                buttons1: buf[0],
                buttons2: buf[1],
                hat: buf[2],
                left_stick_x: buf[3],
                left_stick_y: buf[4],
                right_stick_x: buf[5],
                right_stick_y: buf[6],
            }
        }

        /// Test whether a button bit is set in buttons1.
        pub fn has1(&self, bit: u8) -> bool {
            self.buttons1 & bit != 0
        }

        /// Test whether a button bit is set in buttons2.
        #[cfg(test)]
        pub fn has2(&self, bit: u8) -> bool {
            self.buttons2 & bit != 0
        }
    }

    /// Hat switch value to D-pad booleans: `[Up, Down, Left, Right]`.
    ///
    /// Standard USB HID hat encoding: 0=N, 1=NE, 2=E, 3=SE, 4=S, 5=SW,
    /// 6=W, 7=NW, >=8=centered.
    pub fn hat_to_dpad(hat: u8) -> [bool; 4] {
        match hat {
            0 => [true, false, false, false], // N
            1 => [true, false, false, true],  // NE
            2 => [false, false, false, true], // E
            3 => [false, true, false, true],  // SE
            4 => [false, true, false, false], // S
            5 => [false, true, true, false],  // SW
            6 => [false, false, true, false], // W
            7 => [true, false, true, false],  // NW
            _ => [false; 4],                  // centered
        }
    }

    /// Edge detection for buttons1: pressed in `curr` but not in `prev`.
    pub fn hid_newly_pressed1(curr: &HidInputReport, prev: &HidInputReport, bit: u8) -> bool {
        (curr.buttons1 & bit != 0) && (prev.buttons1 & bit == 0)
    }

    /// Edge detection for buttons2: pressed in `curr` but not in `prev`.
    pub fn hid_newly_pressed2(curr: &HidInputReport, prev: &HidInputReport, bit: u8) -> bool {
        (curr.buttons2 & bit != 0) && (prev.buttons2 & bit == 0)
    }

    /// USB device handle for a generic HID gamepad.
    pub struct RawHidState {
        handle: DeviceHandle<Context>,
        ep_in: u8,
    }

    impl RawHidState {
        /// Find and initialize any USB HID gamepad (class 0x03).
        /// Returns `None` if not found or init fails.
        pub fn try_init() -> Option<Self> {
            let ctx = Context::new().ok()?;
            let devices = ctx.devices().ok()?;

            for device in devices.iter() {
                let config = match device.active_config_descriptor() {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                for iface in config.interfaces() {
                    for iface_desc in iface.descriptors() {
                        if iface_desc.class_code() != HID_IFACE_CLASS {
                            continue;
                        }

                        let iface_num = iface_desc.interface_number();
                        let mut ep_in = None;

                        for ep in iface_desc.endpoint_descriptors() {
                            if ep.direction() == rusb::Direction::In
                                && ep.transfer_type() == rusb::TransferType::Interrupt
                            {
                                ep_in = Some(ep.address());
                            }
                        }

                        let ep_in = match ep_in {
                            Some(e) => e,
                            None => continue,
                        };

                        let handle = match device.open() {
                            Ok(h) => h,
                            Err(_) => continue,
                        };

                        let _ = handle.set_auto_detach_kernel_driver(true);

                        if handle.claim_interface(iface_num).is_err() {
                            continue;
                        }

                        // Workaround: some USB stacks (e.g. Crostini/xHCI
                        // passthrough) require a control transfer before
                        // interrupt endpoint transfers will succeed.
                        let _ = handle.read_control(
                            0x80, // GET_STATUS: device-to-host, standard, device
                            0x00, // GET_STATUS request
                            0,
                            0,
                            &mut [0u8; 2],
                            Duration::from_millis(100),
                        );

                        // No init packets needed — HID devices are ready
                        // immediately after the kernel driver initializes them.

                        return Some(RawHidState { handle, ep_in });
                    }
                }
            }
            None
        }

        /// Non-blocking read of an HID input report (4ms timeout).
        pub fn read_input(&self) -> Option<HidInputReport> {
            let mut buf = [0u8; 64];
            let timeout = Duration::from_millis(4);
            match self.handle.read_interrupt(self.ep_in, &mut buf, timeout) {
                Ok(len) if len >= 7 => Some(HidInputReport::parse(&buf)),
                _ => None,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Gamepad-enabled implementation
// ---------------------------------------------------------------------------

#[cfg(feature = "gamepad")]
mod inner {
    use super::*;
    use gilrs::{Axis, Button, EventType, Gilrs};
    use roguelike_core::command::{Direction, GameCommand};
    use roguelike_core::look::LookCommand;
    use std::time::Duration;

    // D-pad index constants.
    const DPAD_UP: usize = 0;
    const DPAD_DOWN: usize = 1;
    const DPAD_LEFT: usize = 2;
    const DPAD_RIGHT: usize = 3;

    /// Backend-specific state for gamepad input.
    enum Backend {
        /// Standard gamepad via gilrs (evdev/XInput/HID).
        Gilrs {
            gilrs: Gilrs,
            buffered_event: Option<gilrs::Event>,
        },
        /// Generic USB HID gamepad (e.g. 8BitDo SN30 Pro in DirectInput mode),
        /// for environments where gilrs/evdev don't see the device.
        #[cfg(feature = "raw-usb")]
        RawHid {
            hid: super::raw_hid::RawHidState,
            prev_report: super::raw_hid::HidInputReport,
            pending_report: Option<super::raw_hid::HidInputReport>,
        },
    }

    /// Persistent gamepad state carried through the application lifetime.
    pub struct GamepadState {
        backend: Backend,
        /// Edge-trigger flag: true while the analog stick is deflected past
        /// the deadzone, preventing repeat commands until the stick returns.
        stick_engaged: bool,
        /// Which D-pad buttons are currently held (Up, Down, Left, Right).
        dpad_held: [bool; 4],
        /// Whether LB (autorun modifier) is currently held.
        lb_held: bool,
    }

    // -- Free helper functions (avoid borrow conflicts with `&mut self`) --

    /// Compute composite D-pad direction from currently held buttons.
    fn dpad_direction(held: &[bool; 4]) -> Option<Direction> {
        let dx = match (held[DPAD_LEFT], held[DPAD_RIGHT]) {
            (true, false) => -1,
            (false, true) => 1,
            _ => 0,
        };
        let dy = match (held[DPAD_UP], held[DPAD_DOWN]) {
            (true, false) => -1,
            (false, true) => 1,
            _ => 0,
        };
        Direction::from_offset(dx, dy)
    }

    /// Update held-state tracking from a gilrs event.
    fn track_gilrs_held_state(ev: &gilrs::Event, dpad_held: &mut [bool; 4], lb_held: &mut bool) {
        match ev.event {
            EventType::ButtonPressed(btn, _) => match btn {
                Button::DPadUp => dpad_held[DPAD_UP] = true,
                Button::DPadDown => dpad_held[DPAD_DOWN] = true,
                Button::DPadLeft => dpad_held[DPAD_LEFT] = true,
                Button::DPadRight => dpad_held[DPAD_RIGHT] = true,
                Button::LeftTrigger => *lb_held = true,
                _ => {}
            },
            EventType::ButtonReleased(btn, _) => match btn {
                Button::DPadUp => dpad_held[DPAD_UP] = false,
                Button::DPadDown => dpad_held[DPAD_DOWN] = false,
                Button::DPadLeft => dpad_held[DPAD_LEFT] = false,
                Button::DPadRight => dpad_held[DPAD_RIGHT] = false,
                Button::LeftTrigger => *lb_held = false,
                _ => {}
            },
            _ => {}
        }
    }

    /// Check gilrs analog stick for a new direction past the deadzone.
    fn check_gilrs_stick(gilrs: &Gilrs, stick_engaged: &mut bool) -> Option<Direction> {
        let gp_id = gilrs.gamepads().next().map(|(id, _)| id)?;
        let gp = gilrs.gamepad(gp_id);
        let sx = gp
            .axis_data(Axis::LeftStickX)
            .map(|a| a.value())
            .unwrap_or(0.0);
        let sy = gp
            .axis_data(Axis::LeftStickY)
            .map(|a| a.value())
            .unwrap_or(0.0);
        check_stick_common(sx, sy, stick_engaged)
    }

    /// Shared stick edge-trigger logic (works for both gilrs and raw HID).
    fn check_stick_common(sx: f32, sy: f32, stick_engaged: &mut bool) -> Option<Direction> {
        let magnitude = (sx * sx + sy * sy).sqrt();
        if magnitude < 0.3 {
            *stick_engaged = false;
            return None;
        }
        if *stick_engaged {
            return None;
        }
        *stick_engaged = true;
        Some(analog_to_direction(sx, sy))
    }

    // -- Raw-HID dispatch helpers (pure functions for testability) ------------

    /// Update shared state from an HID hat switch and shoulder buttons.
    #[cfg(feature = "raw-usb")]
    fn update_held_from_hid_report(
        report: &super::raw_hid::HidInputReport,
        dpad_held: &mut [bool; 4],
        lb_held: &mut bool,
    ) {
        *dpad_held = super::raw_hid::hat_to_dpad(report.hat);
        *lb_held = report.has1(super::raw_hid::HID_BTN_LB);
    }

    /// Consume a pending or new HID report, updating held state.
    /// Returns `(current, previous)` for edge detection.
    #[cfg(feature = "raw-usb")]
    fn consume_hid_report(
        hid: &super::raw_hid::RawHidState,
        prev_report: &mut super::raw_hid::HidInputReport,
        pending_report: &mut Option<super::raw_hid::HidInputReport>,
        dpad_held: &mut [bool; 4],
        lb_held: &mut bool,
    ) -> Option<(
        super::raw_hid::HidInputReport,
        super::raw_hid::HidInputReport,
    )> {
        let report = pending_report.take().or_else(|| hid.read_input())?;
        let prev = *prev_report;
        *prev_report = report;
        update_held_from_hid_report(&report, dpad_held, lb_held);
        Some((report, prev))
    }

    /// Normalize HID stick axes (u8 0-255, center 128) and apply edge-trigger.
    /// HID Y axis: 0x00=up, 0xFF=down — inverted vs gilrs convention, so we
    /// negate Y before passing to `check_stick_common`.
    #[cfg(feature = "raw-usb")]
    fn check_hid_stick(
        report: &super::raw_hid::HidInputReport,
        stick_engaged: &mut bool,
    ) -> Option<Direction> {
        let sx = (report.left_stick_x as f32 - 128.0) / 128.0;
        // Negate Y: HID 0x00=up → -1.0, but gilrs/check_stick_common expects
        // positive Y = up, so invert.
        let sy = -((report.left_stick_y as f32 - 128.0) / 128.0);
        check_stick_common(sx, sy, stick_engaged)
    }

    /// Detect hat switch change representing a new d-pad press.
    #[cfg(feature = "raw-usb")]
    pub fn hid_dpad_newly_pressed(
        report: &super::raw_hid::HidInputReport,
        prev: &super::raw_hid::HidInputReport,
    ) -> bool {
        // Centered → direction, or direction → different direction.
        report.hat < 8 && (prev.hat >= 8 || prev.hat != report.hat)
    }

    /// Map HID face-button edges to game commands (A/B/X/Y/Start only).
    #[cfg(feature = "raw-usb")]
    pub fn hid_face_to_game_cmd(
        report: &super::raw_hid::HidInputReport,
        prev: &super::raw_hid::HidInputReport,
    ) -> Option<GameCommand> {
        use super::raw_hid::*;
        if hid_newly_pressed1(report, prev, HID_BTN_A) {
            return Some(GameCommand::Wait);
        }
        if hid_newly_pressed1(report, prev, HID_BTN_B)
            || hid_newly_pressed2(report, prev, HID_BTN_START)
        {
            return Some(GameCommand::Quit);
        }
        if hid_newly_pressed1(report, prev, HID_BTN_X) {
            return Some(GameCommand::AutoExplore);
        }
        if hid_newly_pressed1(report, prev, HID_BTN_Y) {
            return Some(GameCommand::Look);
        }
        if hid_newly_pressed1(report, prev, HID_BTN_RB) {
            return Some(GameCommand::Pickup);
        }
        if hid_newly_pressed2(report, prev, HID_BTN_SELECT) {
            return Some(GameCommand::OpenInventory);
        }
        None
    }

    /// Map HID button edges to menu commands (hat + face buttons).
    #[cfg(feature = "raw-usb")]
    pub fn hid_to_menu_cmd(
        report: &super::raw_hid::HidInputReport,
        prev: &super::raw_hid::HidInputReport,
    ) -> Option<MenuCommand> {
        use super::raw_hid::*;
        // Hat switch edge detection for menu navigation.
        if hid_dpad_newly_pressed(report, prev) {
            let dpad = hat_to_dpad(report.hat);
            if dpad[DPAD_UP] {
                return Some(MenuCommand::Up);
            }
            if dpad[DPAD_DOWN] {
                return Some(MenuCommand::Down);
            }
        }
        if hid_newly_pressed1(report, prev, HID_BTN_A)
            || hid_newly_pressed2(report, prev, HID_BTN_START)
        {
            return Some(MenuCommand::Select);
        }
        if hid_newly_pressed1(report, prev, HID_BTN_B) {
            return Some(MenuCommand::Back);
        }
        None
    }

    /// Map HID button edges to look-mode commands (close only).
    #[cfg(feature = "raw-usb")]
    pub fn hid_to_look_cmd(
        report: &super::raw_hid::HidInputReport,
        prev: &super::raw_hid::HidInputReport,
    ) -> Option<LookCommand> {
        use super::raw_hid::*;
        if hid_newly_pressed1(report, prev, HID_BTN_B)
            || hid_newly_pressed2(report, prev, HID_BTN_START)
        {
            return Some(LookCommand::Close);
        }
        None
    }

    /// Map HID button edges to history-viewer commands.
    #[cfg(feature = "raw-usb")]
    pub fn hid_to_history_cmd(
        report: &super::raw_hid::HidInputReport,
        prev: &super::raw_hid::HidInputReport,
    ) -> Option<HistoryCommand> {
        use super::raw_hid::*;
        if hid_dpad_newly_pressed(report, prev) {
            let dpad = hat_to_dpad(report.hat);
            if dpad[DPAD_UP] {
                return Some(HistoryCommand::Menu(MenuCommand::Up));
            }
            if dpad[DPAD_DOWN] {
                return Some(HistoryCommand::Menu(MenuCommand::Down));
            }
        }
        if hid_newly_pressed1(report, prev, HID_BTN_LB) {
            return Some(HistoryCommand::PageUp);
        }
        if hid_newly_pressed1(report, prev, HID_BTN_RB) {
            return Some(HistoryCommand::PageDown);
        }
        if hid_newly_pressed1(report, prev, HID_BTN_A)
            || hid_newly_pressed1(report, prev, HID_BTN_B)
            || hid_newly_pressed2(report, prev, HID_BTN_START)
        {
            return Some(HistoryCommand::Menu(MenuCommand::Back));
        }
        None
    }

    impl GamepadState {
        /// Try to create a `GamepadOption`. Tries gilrs first; if gilrs finds
        /// zero gamepads and `raw-usb` is enabled, falls back to raw HID.
        pub fn new_option() -> super::GamepadOption {
            match Gilrs::new() {
                Ok(gilrs) => {
                    #[cfg(feature = "raw-usb")]
                    if gilrs.gamepads().count() == 0
                        && let Some(hid) = super::raw_hid::RawHidState::try_init()
                    {
                        return Some(GamepadState {
                            backend: Backend::RawHid {
                                hid,
                                prev_report: super::raw_hid::HidInputReport::default(),
                                pending_report: None,
                            },
                            stick_engaged: false,
                            dpad_held: [false; 4],
                            lb_held: false,
                        });
                    }
                    Some(GamepadState {
                        backend: Backend::Gilrs {
                            gilrs,
                            buffered_event: None,
                        },
                        stick_engaged: false,
                        dpad_held: [false; 4],
                        lb_held: false,
                    })
                }
                Err(_) => {
                    #[cfg(feature = "raw-usb")]
                    if let Some(hid) = super::raw_hid::RawHidState::try_init() {
                        return Some(GamepadState {
                            backend: Backend::RawHid {
                                hid,
                                prev_report: super::raw_hid::HidInputReport::default(),
                                pending_report: None,
                            },
                            stick_engaged: false,
                            dpad_held: [false; 4],
                            lb_held: false,
                        });
                    }
                    None
                }
            }
        }

        /// Drain all pending events/reports (so stale input from an earlier
        /// context doesn't leak into the current one).
        pub fn drain_stale(&mut self) {
            match &mut self.backend {
                Backend::Gilrs {
                    gilrs,
                    buffered_event,
                } => {
                    while gilrs.next_event().is_some() {}
                    *buffered_event = None;
                }
                #[cfg(feature = "raw-usb")]
                Backend::RawHid {
                    hid,
                    prev_report,
                    pending_report,
                } => {
                    while hid.read_input().is_some() {}
                    *prev_report = super::raw_hid::HidInputReport::default();
                    *pending_report = None;
                }
            }
            self.dpad_held = [false; 4];
            self.lb_held = false;
            self.stick_engaged = false;
        }

        /// Check whether there are pending gamepad events (buffered or new).
        pub fn has_pending_events(&mut self) -> bool {
            match &mut self.backend {
                Backend::Gilrs {
                    gilrs,
                    buffered_event,
                } => {
                    if buffered_event.is_some() {
                        return true;
                    }
                    if let Some(ev) = gilrs.next_event() {
                        *buffered_event = Some(ev);
                        return true;
                    }
                    false
                }
                #[cfg(feature = "raw-usb")]
                Backend::RawHid {
                    hid,
                    pending_report,
                    ..
                } => {
                    if pending_report.is_some() {
                        return true;
                    }
                    if let Some(report) = hid.read_input() {
                        *pending_report = Some(report);
                        return true;
                    }
                    false
                }
            }
        }

        // ---- Translation functions ----

        /// Drain events and return the first actionable game command.
        pub fn next_game_command(&mut self) -> Option<GameCommand> {
            match &mut self.backend {
                Backend::Gilrs {
                    gilrs,
                    buffered_event,
                } => loop {
                    let ev = buffered_event.take().or_else(|| gilrs.next_event())?;
                    track_gilrs_held_state(&ev, &mut self.dpad_held, &mut self.lb_held);
                    match ev.event {
                        EventType::ButtonPressed(btn, _) => match btn {
                            Button::DPadUp
                            | Button::DPadDown
                            | Button::DPadLeft
                            | Button::DPadRight => {
                                if let Some(dir) = dpad_direction(&self.dpad_held) {
                                    return Some(if self.lb_held {
                                        GameCommand::Autorun(dir)
                                    } else {
                                        GameCommand::Move(dir)
                                    });
                                }
                            }
                            Button::South => return Some(GameCommand::Wait),
                            Button::East | Button::Start => return Some(GameCommand::Quit),
                            Button::West => return Some(GameCommand::AutoExplore),
                            Button::North => return Some(GameCommand::Look),
                            Button::RightTrigger => return Some(GameCommand::Pickup),
                            Button::Select => return Some(GameCommand::OpenInventory),
                            _ => {}
                        },
                        EventType::AxisChanged(Axis::LeftStickX | Axis::LeftStickY, _, _) => {
                            if let Some(dir) = check_gilrs_stick(gilrs, &mut self.stick_engaged) {
                                return Some(if self.lb_held {
                                    GameCommand::Autorun(dir)
                                } else {
                                    GameCommand::Move(dir)
                                });
                            }
                        }
                        _ => {}
                    }
                },
                #[cfg(feature = "raw-usb")]
                Backend::RawHid {
                    hid,
                    prev_report,
                    pending_report,
                } => {
                    let (report, prev) = consume_hid_report(
                        hid,
                        prev_report,
                        pending_report,
                        &mut self.dpad_held,
                        &mut self.lb_held,
                    )?;

                    if let Some(cmd) = hid_face_to_game_cmd(&report, &prev) {
                        return Some(cmd);
                    }

                    if hid_dpad_newly_pressed(&report, &prev)
                        && let Some(dir) = dpad_direction(&self.dpad_held)
                    {
                        return Some(if self.lb_held {
                            GameCommand::Autorun(dir)
                        } else {
                            GameCommand::Move(dir)
                        });
                    }

                    if let Some(dir) = check_hid_stick(&report, &mut self.stick_engaged) {
                        return Some(if self.lb_held {
                            GameCommand::Autorun(dir)
                        } else {
                            GameCommand::Move(dir)
                        });
                    }

                    None
                }
            }
        }

        /// Drain events and return the first actionable menu command.
        pub fn next_menu_command(&mut self) -> Option<MenuCommand> {
            match &mut self.backend {
                Backend::Gilrs {
                    gilrs,
                    buffered_event,
                } => loop {
                    let ev = buffered_event.take().or_else(|| gilrs.next_event())?;
                    track_gilrs_held_state(&ev, &mut self.dpad_held, &mut self.lb_held);
                    if let EventType::ButtonPressed(btn, _) = ev.event {
                        match btn {
                            Button::DPadUp => return Some(MenuCommand::Up),
                            Button::DPadDown => return Some(MenuCommand::Down),
                            Button::South | Button::Start => {
                                return Some(MenuCommand::Select);
                            }
                            Button::East => return Some(MenuCommand::Back),
                            _ => {}
                        }
                    }
                    if let EventType::AxisChanged(Axis::LeftStickX | Axis::LeftStickY, _, _) =
                        ev.event
                        && let Some(dir) = check_gilrs_stick(gilrs, &mut self.stick_engaged)
                    {
                        let (_, dy) = dir.to_offset();
                        if dy < 0 {
                            return Some(MenuCommand::Up);
                        }
                        if dy > 0 {
                            return Some(MenuCommand::Down);
                        }
                    }
                },
                #[cfg(feature = "raw-usb")]
                Backend::RawHid {
                    hid,
                    prev_report,
                    pending_report,
                } => {
                    let (report, prev) = consume_hid_report(
                        hid,
                        prev_report,
                        pending_report,
                        &mut self.dpad_held,
                        &mut self.lb_held,
                    )?;

                    if let Some(cmd) = hid_to_menu_cmd(&report, &prev) {
                        return Some(cmd);
                    }

                    if let Some(dir) = check_hid_stick(&report, &mut self.stick_engaged) {
                        let (_, dy) = dir.to_offset();
                        if dy < 0 {
                            return Some(MenuCommand::Up);
                        }
                        if dy > 0 {
                            return Some(MenuCommand::Down);
                        }
                    }

                    None
                }
            }
        }

        /// Drain events and return the first actionable look-mode command.
        pub fn next_look_command(&mut self) -> Option<LookCommand> {
            match &mut self.backend {
                Backend::Gilrs {
                    gilrs,
                    buffered_event,
                } => loop {
                    let ev = buffered_event.take().or_else(|| gilrs.next_event())?;
                    track_gilrs_held_state(&ev, &mut self.dpad_held, &mut self.lb_held);
                    match ev.event {
                        EventType::ButtonPressed(btn, _) => match btn {
                            Button::DPadUp
                            | Button::DPadDown
                            | Button::DPadLeft
                            | Button::DPadRight => {
                                if let Some(dir) = dpad_direction(&self.dpad_held) {
                                    return Some(LookCommand::Move(dir));
                                }
                            }
                            Button::East | Button::Start => {
                                return Some(LookCommand::Close);
                            }
                            _ => {}
                        },
                        EventType::AxisChanged(Axis::LeftStickX | Axis::LeftStickY, _, _) => {
                            if let Some(dir) = check_gilrs_stick(gilrs, &mut self.stick_engaged) {
                                return Some(LookCommand::Move(dir));
                            }
                        }
                        _ => {}
                    }
                },
                #[cfg(feature = "raw-usb")]
                Backend::RawHid {
                    hid,
                    prev_report,
                    pending_report,
                } => {
                    let (report, prev) = consume_hid_report(
                        hid,
                        prev_report,
                        pending_report,
                        &mut self.dpad_held,
                        &mut self.lb_held,
                    )?;

                    if let Some(cmd) = hid_to_look_cmd(&report, &prev) {
                        return Some(cmd);
                    }

                    if hid_dpad_newly_pressed(&report, &prev)
                        && let Some(dir) = dpad_direction(&self.dpad_held)
                    {
                        return Some(LookCommand::Move(dir));
                    }

                    if let Some(dir) = check_hid_stick(&report, &mut self.stick_engaged) {
                        return Some(LookCommand::Move(dir));
                    }

                    None
                }
            }
        }

        /// Drain events and return the first actionable history command.
        pub fn next_history_command(&mut self) -> Option<HistoryCommand> {
            match &mut self.backend {
                Backend::Gilrs {
                    gilrs,
                    buffered_event,
                } => loop {
                    let ev = buffered_event.take().or_else(|| gilrs.next_event())?;
                    track_gilrs_held_state(&ev, &mut self.dpad_held, &mut self.lb_held);
                    if let EventType::ButtonPressed(btn, _) = ev.event {
                        match btn {
                            Button::DPadUp => {
                                return Some(HistoryCommand::Menu(MenuCommand::Up));
                            }
                            Button::DPadDown => {
                                return Some(HistoryCommand::Menu(MenuCommand::Down));
                            }
                            Button::LeftTrigger => return Some(HistoryCommand::PageUp),
                            Button::RightTrigger => return Some(HistoryCommand::PageDown),
                            Button::South | Button::East | Button::Start => {
                                return Some(HistoryCommand::Menu(MenuCommand::Back));
                            }
                            _ => {}
                        }
                    }
                    if let EventType::AxisChanged(Axis::LeftStickX | Axis::LeftStickY, _, _) =
                        ev.event
                        && let Some(dir) = check_gilrs_stick(gilrs, &mut self.stick_engaged)
                    {
                        let (_, dy) = dir.to_offset();
                        if dy < 0 {
                            return Some(HistoryCommand::Menu(MenuCommand::Up));
                        }
                        if dy > 0 {
                            return Some(HistoryCommand::Menu(MenuCommand::Down));
                        }
                    }
                },
                #[cfg(feature = "raw-usb")]
                Backend::RawHid {
                    hid,
                    prev_report,
                    pending_report,
                } => {
                    let (report, prev) = consume_hid_report(
                        hid,
                        prev_report,
                        pending_report,
                        &mut self.dpad_held,
                        &mut self.lb_held,
                    )?;

                    if let Some(cmd) = hid_to_history_cmd(&report, &prev) {
                        return Some(cmd);
                    }

                    if let Some(dir) = check_hid_stick(&report, &mut self.stick_engaged) {
                        let (_, dy) = dir.to_offset();
                        if dy < 0 {
                            return Some(HistoryCommand::Menu(MenuCommand::Up));
                        }
                        if dy > 0 {
                            return Some(HistoryCommand::Menu(MenuCommand::Down));
                        }
                    }

                    None
                }
            }
        }
    }

    /// Convert analog stick `(sx, sy)` into one of 8 discrete directions.
    ///
    /// Uses `atan2` with π/8 sector boundaries. gilrs convention: positive Y is
    /// up, but our screen Y axis points down, so we negate `sy`.
    pub fn analog_to_direction(sx: f32, sy: f32) -> Direction {
        let angle = (-sy).atan2(sx); // negate Y for screen coords
        // Divide the circle into 8 sectors of π/4 each, centered on each
        // cardinal/diagonal direction.
        let sector = ((angle + std::f32::consts::PI + std::f32::consts::FRAC_PI_8)
            / std::f32::consts::FRAC_PI_4) as i32
            % 8;
        match sector {
            0 => Direction::West,
            1 => Direction::NorthWest,
            2 => Direction::North,
            3 => Direction::NorthEast,
            4 => Direction::East,
            5 => Direction::SouthEast,
            6 => Direction::South,
            7 => Direction::SouthWest,
            _ => Direction::East, // unreachable: sector is mod 8
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
        use roguelike_core::command::Direction;

        #[test]
        fn right() {
            assert_eq!(analog_to_direction(1.0, 0.0), Direction::East);
        }

        #[test]
        fn left() {
            assert_eq!(analog_to_direction(-1.0, 0.0), Direction::West);
        }

        #[test]
        fn up() {
            // gilrs: positive Y = up, our function negates it → screen up
            assert_eq!(analog_to_direction(0.0, 1.0), Direction::North);
        }

        #[test]
        fn down() {
            assert_eq!(analog_to_direction(0.0, -1.0), Direction::South);
        }

        #[test]
        fn up_right() {
            assert_eq!(analog_to_direction(0.7, 0.7), Direction::NorthEast);
        }

        #[test]
        fn up_left() {
            assert_eq!(analog_to_direction(-0.7, 0.7), Direction::NorthWest);
        }

        #[test]
        fn down_right() {
            assert_eq!(analog_to_direction(0.7, -0.7), Direction::SouthEast);
        }

        #[test]
        fn down_left() {
            assert_eq!(analog_to_direction(-0.7, -0.7), Direction::SouthWest);
        }

        #[test]
        fn near_axis_snaps_to_cardinal() {
            // Slightly off-axis should still snap to cardinal direction.
            assert_eq!(analog_to_direction(1.0, 0.1), Direction::East);
            assert_eq!(analog_to_direction(1.0, -0.1), Direction::East);
            assert_eq!(analog_to_direction(0.1, 1.0), Direction::North);
            assert_eq!(analog_to_direction(-0.1, 1.0), Direction::North);
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
                Button::RightTrigger => Some(GameCommand::Pickup),
                Button::Select => Some(GameCommand::OpenInventory),
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
            assert_eq!(
                face_button_to_game_cmd(Button::RightTrigger, false),
                Some(GameCommand::Pickup)
            );
            assert_eq!(
                face_button_to_game_cmd(Button::Select, false),
                Some(GameCommand::OpenInventory)
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

    // -- Raw HID report parsing tests (no hardware required) --

    #[cfg(feature = "raw-usb")]
    mod hid_tests {
        use super::super::raw_hid::*;

        /// Build a 9-byte HID input report buffer from field values.
        fn make_hid_report(
            buttons1: u8,
            buttons2: u8,
            hat: u8,
            lx: u8,
            ly: u8,
            rx: u8,
            ry: u8,
        ) -> [u8; 9] {
            [buttons1, buttons2, hat, lx, ly, rx, ry, 0, 0]
        }

        #[test]
        fn parse_idle_report() {
            let buf = make_hid_report(0, 0, 0x08, 0x80, 0x80, 0x80, 0x80);
            let report = HidInputReport::parse(&buf);
            assert_eq!(report, HidInputReport::default());
        }

        #[test]
        fn parse_each_button1() {
            let buttons = [
                HID_BTN_A, HID_BTN_B, HID_BTN_X, HID_BTN_Y, HID_BTN_LB, HID_BTN_RB,
            ];
            for &btn in &buttons {
                let buf = make_hid_report(btn, 0, 0x08, 0x80, 0x80, 0x80, 0x80);
                let report = HidInputReport::parse(&buf);
                assert!(report.has1(btn), "button 0x{btn:02x} not set");
                for &other in &buttons {
                    if other != btn {
                        assert!(!report.has1(other), "button 0x{other:02x} should be off");
                    }
                }
            }
        }

        #[test]
        fn parse_each_button2() {
            let buttons = [HID_BTN_SELECT, HID_BTN_START];
            for &btn in &buttons {
                let buf = make_hid_report(0, btn, 0x08, 0x80, 0x80, 0x80, 0x80);
                let report = HidInputReport::parse(&buf);
                assert!(report.has2(btn), "button2 0x{btn:02x} not set");
                for &other in &buttons {
                    if other != btn {
                        assert!(!report.has2(other), "button2 0x{other:02x} should be off");
                    }
                }
            }
        }

        #[test]
        fn hat_to_dpad_all_values() {
            // [Up, Down, Left, Right]
            assert_eq!(hat_to_dpad(0), [true, false, false, false]); // N
            assert_eq!(hat_to_dpad(1), [true, false, false, true]); // NE
            assert_eq!(hat_to_dpad(2), [false, false, false, true]); // E
            assert_eq!(hat_to_dpad(3), [false, true, false, true]); // SE
            assert_eq!(hat_to_dpad(4), [false, true, false, false]); // S
            assert_eq!(hat_to_dpad(5), [false, true, true, false]); // SW
            assert_eq!(hat_to_dpad(6), [false, false, true, false]); // W
            assert_eq!(hat_to_dpad(7), [true, false, true, false]); // NW
        }

        #[test]
        fn hat_to_dpad_centered_variants() {
            // Any value >= 8 should produce centered (all false).
            for v in 8..=15 {
                assert_eq!(hat_to_dpad(v), [false; 4], "hat={v} should be centered");
            }
            assert_eq!(hat_to_dpad(0xFF), [false; 4]);
        }

        #[test]
        fn hid_newly_pressed_fires_once() {
            let idle = HidInputReport::default();
            let pressed = HidInputReport {
                buttons1: HID_BTN_A,
                ..Default::default()
            };
            // Press edge: fires.
            assert!(hid_newly_pressed1(&pressed, &idle, HID_BTN_A));
            // Hold: does not fire.
            assert!(!hid_newly_pressed1(&pressed, &pressed, HID_BTN_A));
            // Release: does not fire.
            assert!(!hid_newly_pressed1(&idle, &pressed, HID_BTN_A));
        }

        #[test]
        fn hid_newly_pressed2_fires_once() {
            let idle = HidInputReport::default();
            let pressed = HidInputReport {
                buttons2: HID_BTN_START,
                ..Default::default()
            };
            assert!(hid_newly_pressed2(&pressed, &idle, HID_BTN_START));
            assert!(!hid_newly_pressed2(&pressed, &pressed, HID_BTN_START));
            assert!(!hid_newly_pressed2(&idle, &pressed, HID_BTN_START));
        }

        #[test]
        fn stick_normalization() {
            use super::super::inner::analog_to_direction;
            // HID: 0x00 = full left/up, 0x80 = center, 0xFF = full right/down.
            // After normalization: X maps directly, Y is negated.
            // left_stick_x=0xFF → sx = (255-128)/128 = ~1.0 → right
            // left_stick_y=0x00 → sy_raw = (0-128)/128 = -1.0,
            //   negated → sy = 1.0 → up → screen (0,-1)
            let sx = (0xFFu8 as f32 - 128.0) / 128.0;
            let sy = -((0x00u8 as f32 - 128.0) / 128.0);
            assert_eq!(analog_to_direction(sx, sy), (1, -1)); // right + up = NE

            // Full down: y=0xFF → sy_raw = (255-128)/128 ≈ 1.0,
            //   negated → sy = -1.0 → down → screen (0, 1)
            let sx = (0x80u8 as f32 - 128.0) / 128.0; // ~0
            let sy = -((0xFFu8 as f32 - 128.0) / 128.0);
            assert_eq!(analog_to_direction(sx, sy), (0, 1)); // down
        }
    }

    #[cfg(feature = "raw-usb")]
    mod hid_command_mapping_tests {
        use super::super::inner::{
            hid_dpad_newly_pressed, hid_face_to_game_cmd, hid_to_history_cmd, hid_to_look_cmd,
            hid_to_menu_cmd,
        };
        use super::super::raw_hid::*;
        use roguelike_core::command::GameCommand;
        use roguelike_core::look::LookCommand;
        use roguelike_core::platform::MenuCommand;

        use super::super::HistoryCommand;

        /// Build a (current, previous) report pair for a fresh buttons1 press.
        fn press1(btn: u8) -> (HidInputReport, HidInputReport) {
            let curr = HidInputReport {
                buttons1: btn,
                ..Default::default()
            };
            let prev = HidInputReport::default();
            (curr, prev)
        }

        /// Build a (current, previous) report pair for a fresh buttons2 press.
        fn press2(btn: u8) -> (HidInputReport, HidInputReport) {
            let curr = HidInputReport {
                buttons2: btn,
                ..Default::default()
            };
            let prev = HidInputReport::default();
            (curr, prev)
        }

        /// Build a (current, previous) report pair for a fresh hat press.
        fn press_hat(hat: u8) -> (HidInputReport, HidInputReport) {
            let curr = HidInputReport {
                hat,
                ..Default::default()
            };
            let prev = HidInputReport::default(); // hat=0x08 (centered)
            (curr, prev)
        }

        #[test]
        fn game_face_buttons() {
            let (r, p) = press1(HID_BTN_A);
            assert_eq!(hid_face_to_game_cmd(&r, &p), Some(GameCommand::Wait));
            let (r, p) = press1(HID_BTN_B);
            assert_eq!(hid_face_to_game_cmd(&r, &p), Some(GameCommand::Quit));
            let (r, p) = press1(HID_BTN_X);
            assert_eq!(hid_face_to_game_cmd(&r, &p), Some(GameCommand::AutoExplore));
            let (r, p) = press1(HID_BTN_Y);
            assert_eq!(hid_face_to_game_cmd(&r, &p), Some(GameCommand::Look));
            let (r, p) = press2(HID_BTN_START);
            assert_eq!(hid_face_to_game_cmd(&r, &p), Some(GameCommand::Quit));
            let (r, p) = press1(HID_BTN_RB);
            assert_eq!(hid_face_to_game_cmd(&r, &p), Some(GameCommand::Pickup));
            let (r, p) = press2(HID_BTN_SELECT);
            assert_eq!(
                hid_face_to_game_cmd(&r, &p),
                Some(GameCommand::OpenInventory)
            );
        }

        #[test]
        fn game_held_button_does_not_fire() {
            let held = HidInputReport {
                buttons1: HID_BTN_A,
                ..Default::default()
            };
            assert_eq!(hid_face_to_game_cmd(&held, &held), None);
        }

        #[test]
        fn menu_buttons() {
            let (r, p) = press_hat(0); // N = Up
            assert_eq!(hid_to_menu_cmd(&r, &p), Some(MenuCommand::Up));
            let (r, p) = press_hat(4); // S = Down
            assert_eq!(hid_to_menu_cmd(&r, &p), Some(MenuCommand::Down));
            let (r, p) = press1(HID_BTN_A);
            assert_eq!(hid_to_menu_cmd(&r, &p), Some(MenuCommand::Select));
            let (r, p) = press2(HID_BTN_START);
            assert_eq!(hid_to_menu_cmd(&r, &p), Some(MenuCommand::Select));
            let (r, p) = press1(HID_BTN_B);
            assert_eq!(hid_to_menu_cmd(&r, &p), Some(MenuCommand::Back));
            let (r, p) = press1(HID_BTN_X);
            assert_eq!(hid_to_menu_cmd(&r, &p), None);
        }

        #[test]
        fn look_buttons() {
            let (r, p) = press1(HID_BTN_B);
            assert_eq!(hid_to_look_cmd(&r, &p), Some(LookCommand::Close));
            let (r, p) = press2(HID_BTN_START);
            assert_eq!(hid_to_look_cmd(&r, &p), Some(LookCommand::Close));
            let (r, p) = press1(HID_BTN_A);
            assert_eq!(hid_to_look_cmd(&r, &p), None);
        }

        #[test]
        fn history_buttons() {
            let (r, p) = press_hat(0); // N = Up
            assert_eq!(
                hid_to_history_cmd(&r, &p),
                Some(HistoryCommand::Menu(MenuCommand::Up))
            );
            let (r, p) = press_hat(4); // S = Down
            assert_eq!(
                hid_to_history_cmd(&r, &p),
                Some(HistoryCommand::Menu(MenuCommand::Down))
            );
            let (r, p) = press1(HID_BTN_LB);
            assert_eq!(hid_to_history_cmd(&r, &p), Some(HistoryCommand::PageUp));
            let (r, p) = press1(HID_BTN_RB);
            assert_eq!(hid_to_history_cmd(&r, &p), Some(HistoryCommand::PageDown));
            let (r, p) = press1(HID_BTN_A);
            assert_eq!(
                hid_to_history_cmd(&r, &p),
                Some(HistoryCommand::Menu(MenuCommand::Back))
            );
            let (r, p) = press1(HID_BTN_B);
            assert_eq!(
                hid_to_history_cmd(&r, &p),
                Some(HistoryCommand::Menu(MenuCommand::Back))
            );
        }

        #[test]
        fn dpad_hat_edge_detection() {
            let idle = HidInputReport::default(); // hat=0x08
            let north = HidInputReport {
                hat: 0,
                ..Default::default()
            };
            let east = HidInputReport {
                hat: 2,
                ..Default::default()
            };
            // Centered → direction = newly pressed.
            assert!(hid_dpad_newly_pressed(&north, &idle));
            // Direction → same direction = not newly pressed.
            assert!(!hid_dpad_newly_pressed(&north, &north));
            // Direction → different direction = newly pressed.
            assert!(hid_dpad_newly_pressed(&east, &north));
            // Direction → centered = not newly pressed.
            assert!(!hid_dpad_newly_pressed(&idle, &north));
        }
    }
}
