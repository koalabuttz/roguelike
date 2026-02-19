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
// Raw USB submodule — GIP protocol for Xbox controllers
// ---------------------------------------------------------------------------

#[cfg(feature = "raw-usb")]
mod raw_usb {
    use rusb::{Context, DeviceHandle, UsbContext};
    use std::time::Duration;

    // Xbox Series controller USB IDs.
    const VENDOR_ID: u16 = 0x045e;
    const PRODUCT_ID: u16 = 0x0b12;

    // GIP interface descriptor values.
    const GIP_IFACE_CLASS: u8 = 0xFF;
    const GIP_IFACE_SUBCLASS: u8 = 0x47;
    const GIP_IFACE_PROTOCOL: u8 = 0xD0;

    // GIP command byte for input reports.
    const GIP_CMD_INPUT: u8 = 0x20;

    // GIP init packets.
    const GIP_INIT_IDENTIFY: &[u8] = &[0x04, 0x20, 0x01, 0x00];
    const GIP_INIT_ACTIVE: &[u8] = &[0x05, 0x20, 0x03, 0x01, 0x00];

    // Button bit positions in the u16 bitmask (bytes 4-5 of the report).
    pub const BTN_MENU: u16 = 1 << 2;
    #[cfg(test)] // only used in test iteration over all buttons
    pub const BTN_VIEW: u16 = 1 << 3;
    pub const BTN_A: u16 = 1 << 4;
    pub const BTN_B: u16 = 1 << 5;
    pub const BTN_X: u16 = 1 << 6;
    pub const BTN_Y: u16 = 1 << 7;
    pub const BTN_DPAD_UP: u16 = 1 << 8;
    pub const BTN_DPAD_DOWN: u16 = 1 << 9;
    pub const BTN_DPAD_LEFT: u16 = 1 << 10;
    pub const BTN_DPAD_RIGHT: u16 = 1 << 11;
    pub const BTN_LB: u16 = 1 << 12;
    pub const BTN_RB: u16 = 1 << 13;

    /// Parsed GIP input report (18 bytes from the controller).
    ///
    /// Layout: `[header(4)] [buttons(2)] [LT(2)] [RT(2)] [LX(2)] [LY(2)] [RX(2)] [RY(2)]`
    #[derive(Debug, Clone, Copy, Default, PartialEq)]
    pub struct GipInputReport {
        pub buttons: u16,
        pub left_trigger: u16,
        pub right_trigger: u16,
        pub left_stick_x: i16,
        pub left_stick_y: i16,
        pub right_stick_x: i16,
        pub right_stick_y: i16,
    }

    impl GipInputReport {
        /// Parse from an 18+ byte buffer. Caller must verify `buf[0] == 0x20`
        /// and `len >= 18`.
        pub fn parse(buf: &[u8]) -> Self {
            Self {
                buttons: u16::from_le_bytes([buf[4], buf[5]]),
                left_trigger: u16::from_le_bytes([buf[6], buf[7]]),
                right_trigger: u16::from_le_bytes([buf[8], buf[9]]),
                left_stick_x: i16::from_le_bytes([buf[10], buf[11]]),
                left_stick_y: i16::from_le_bytes([buf[12], buf[13]]),
                right_stick_x: i16::from_le_bytes([buf[14], buf[15]]),
                right_stick_y: i16::from_le_bytes([buf[16], buf[17]]),
            }
        }

        /// Test whether a button bit is set.
        pub fn has(&self, bit: u16) -> bool {
            self.buttons & bit != 0
        }
    }

    /// Edge detection: button pressed in `curr` but not in `prev`.
    pub fn newly_pressed(curr: &GipInputReport, prev: &GipInputReport, bit: u16) -> bool {
        curr.has(bit) && !prev.has(bit)
    }

    /// USB device handle for a GIP (Xbox) controller.
    pub struct RawUsbState {
        handle: DeviceHandle<Context>,
        ep_in: u8,
        #[allow(dead_code)] // stored for potential future output (rumble)
        ep_out: u8,
    }

    impl RawUsbState {
        /// Find and initialize an Xbox Series controller via raw USB.
        /// Returns `None` if not found or init fails.
        pub fn try_init() -> Option<Self> {
            let ctx = Context::new().ok()?;
            let devices = ctx.devices().ok()?;

            for device in devices.iter() {
                let desc = match device.device_descriptor() {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                if desc.vendor_id() != VENDOR_ID || desc.product_id() != PRODUCT_ID {
                    continue;
                }

                let config = match device.active_config_descriptor() {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                for iface in config.interfaces() {
                    for iface_desc in iface.descriptors() {
                        if iface_desc.class_code() != GIP_IFACE_CLASS
                            || iface_desc.sub_class_code() != GIP_IFACE_SUBCLASS
                            || iface_desc.protocol_code() != GIP_IFACE_PROTOCOL
                        {
                            continue;
                        }

                        let iface_num = iface_desc.interface_number();
                        let mut ep_in = None;
                        let mut ep_out = None;

                        for ep in iface_desc.endpoint_descriptors() {
                            match ep.direction() {
                                rusb::Direction::In => ep_in = Some(ep.address()),
                                rusb::Direction::Out => ep_out = Some(ep.address()),
                            }
                        }

                        let (ep_in, ep_out) = match (ep_in, ep_out) {
                            (Some(i), Some(o)) => (i, o),
                            _ => continue,
                        };

                        let handle = match device.open() {
                            Ok(h) => h,
                            Err(_) => continue,
                        };

                        let _ = handle.set_auto_detach_kernel_driver(true);

                        if handle.claim_interface(iface_num).is_err() {
                            continue;
                        }

                        // Send GIP init packets.
                        let timeout = Duration::from_millis(100);
                        if handle
                            .write_interrupt(ep_out, GIP_INIT_IDENTIFY, timeout)
                            .is_err()
                        {
                            continue;
                        }
                        if handle
                            .write_interrupt(ep_out, GIP_INIT_ACTIVE, timeout)
                            .is_err()
                        {
                            continue;
                        }

                        return Some(RawUsbState {
                            handle,
                            ep_in,
                            ep_out,
                        });
                    }
                }
            }
            None
        }

        /// Non-blocking read of a GIP input report (4ms timeout).
        pub fn read_input(&self) -> Option<GipInputReport> {
            let mut buf = [0u8; 64];
            let timeout = Duration::from_millis(4);
            match self.handle.read_interrupt(self.ep_in, &mut buf, timeout) {
                Ok(len) if len >= 18 && buf[0] == GIP_CMD_INPUT => {
                    Some(GipInputReport::parse(&buf))
                }
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
    use roguelike_core::command::GameCommand;
    use roguelike_core::look::LookCommand;
    use roguelike_core::types::Coord;
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
        /// Xbox controller via raw USB (GIP protocol), for environments
        /// where the kernel lacks the xpad driver (e.g. Crostini).
        #[cfg(feature = "raw-usb")]
        RawUsb {
            usb: super::raw_usb::RawUsbState,
            prev_report: super::raw_usb::GipInputReport,
            pending_report: Option<super::raw_usb::GipInputReport>,
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
    fn dpad_direction(held: &[bool; 4]) -> Option<(Coord, Coord)> {
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
        if dx == 0 && dy == 0 {
            None
        } else {
            Some((dx, dy))
        }
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
    fn check_gilrs_stick(gilrs: &Gilrs, stick_engaged: &mut bool) -> Option<(Coord, Coord)> {
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

    /// Shared stick edge-trigger logic (works for both gilrs and raw-usb).
    fn check_stick_common(sx: f32, sy: f32, stick_engaged: &mut bool) -> Option<(Coord, Coord)> {
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

    /// Update shared state from a raw-usb GIP input report.
    #[cfg(feature = "raw-usb")]
    fn update_held_from_report(
        report: &super::raw_usb::GipInputReport,
        dpad_held: &mut [bool; 4],
        lb_held: &mut bool,
    ) {
        use super::raw_usb::*;
        dpad_held[DPAD_UP] = report.has(BTN_DPAD_UP);
        dpad_held[DPAD_DOWN] = report.has(BTN_DPAD_DOWN);
        dpad_held[DPAD_LEFT] = report.has(BTN_DPAD_LEFT);
        dpad_held[DPAD_RIGHT] = report.has(BTN_DPAD_RIGHT);
        *lb_held = report.has(BTN_LB);
    }

    impl GamepadState {
        /// Try to create a `GamepadOption`. Tries gilrs first; if gilrs finds
        /// zero gamepads and `raw-usb` is enabled, falls back to raw USB.
        pub fn new_option() -> super::GamepadOption {
            match Gilrs::new() {
                Ok(gilrs) => {
                    #[cfg(feature = "raw-usb")]
                    if gilrs.gamepads().count() == 0
                        && let Some(usb) = super::raw_usb::RawUsbState::try_init()
                    {
                        return Some(GamepadState {
                            backend: Backend::RawUsb {
                                usb,
                                prev_report: super::raw_usb::GipInputReport::default(),
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
                    if let Some(usb) = super::raw_usb::RawUsbState::try_init() {
                        return Some(GamepadState {
                            backend: Backend::RawUsb {
                                usb,
                                prev_report: super::raw_usb::GipInputReport::default(),
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
                Backend::RawUsb {
                    usb,
                    prev_report,
                    pending_report,
                } => {
                    while usb.read_input().is_some() {}
                    *prev_report = super::raw_usb::GipInputReport::default();
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
                Backend::RawUsb {
                    usb,
                    pending_report,
                    ..
                } => {
                    if pending_report.is_some() {
                        return true;
                    }
                    if let Some(report) = usb.read_input() {
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
                                if let Some((dx, dy)) = dpad_direction(&self.dpad_held) {
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
                            if let Some((dx, dy)) =
                                check_gilrs_stick(gilrs, &mut self.stick_engaged)
                            {
                                return Some(if self.lb_held {
                                    GameCommand::Autorun { dx, dy }
                                } else {
                                    GameCommand::Move { dx, dy }
                                });
                            }
                        }
                        _ => {}
                    }
                },
                #[cfg(feature = "raw-usb")]
                Backend::RawUsb {
                    usb,
                    prev_report,
                    pending_report,
                } => {
                    use super::raw_usb::*;
                    let report = pending_report.take().or_else(|| usb.read_input())?;
                    let prev = *prev_report;
                    *prev_report = report;

                    update_held_from_report(&report, &mut self.dpad_held, &mut self.lb_held);

                    // Edge-detect face buttons.
                    if newly_pressed(&report, &prev, BTN_A) {
                        return Some(GameCommand::Wait);
                    }
                    if newly_pressed(&report, &prev, BTN_B)
                        || newly_pressed(&report, &prev, BTN_MENU)
                    {
                        return Some(GameCommand::Quit);
                    }
                    if newly_pressed(&report, &prev, BTN_X) {
                        return Some(GameCommand::AutoExplore);
                    }
                    if newly_pressed(&report, &prev, BTN_Y) {
                        return Some(GameCommand::Look);
                    }

                    // D-pad: fire on any newly pressed d-pad button.
                    if (newly_pressed(&report, &prev, BTN_DPAD_UP)
                        || newly_pressed(&report, &prev, BTN_DPAD_DOWN)
                        || newly_pressed(&report, &prev, BTN_DPAD_LEFT)
                        || newly_pressed(&report, &prev, BTN_DPAD_RIGHT))
                        && let Some((dx, dy)) = dpad_direction(&self.dpad_held)
                    {
                        return Some(if self.lb_held {
                            GameCommand::Autorun { dx, dy }
                        } else {
                            GameCommand::Move { dx, dy }
                        });
                    }

                    // Stick: normalize and edge-trigger.
                    let sx = report.left_stick_x as f32 / 32768.0;
                    let sy = report.left_stick_y as f32 / 32768.0;
                    if let Some((dx, dy)) = check_stick_common(sx, sy, &mut self.stick_engaged) {
                        return Some(if self.lb_held {
                            GameCommand::Autorun { dx, dy }
                        } else {
                            GameCommand::Move { dx, dy }
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
                        && let Some((_, dy)) = check_gilrs_stick(gilrs, &mut self.stick_engaged)
                    {
                        if dy < 0 {
                            return Some(MenuCommand::Up);
                        }
                        if dy > 0 {
                            return Some(MenuCommand::Down);
                        }
                    }
                },
                #[cfg(feature = "raw-usb")]
                Backend::RawUsb {
                    usb,
                    prev_report,
                    pending_report,
                } => {
                    use super::raw_usb::*;
                    let report = pending_report.take().or_else(|| usb.read_input())?;
                    let prev = *prev_report;
                    *prev_report = report;

                    update_held_from_report(&report, &mut self.dpad_held, &mut self.lb_held);

                    if newly_pressed(&report, &prev, BTN_DPAD_UP) {
                        return Some(MenuCommand::Up);
                    }
                    if newly_pressed(&report, &prev, BTN_DPAD_DOWN) {
                        return Some(MenuCommand::Down);
                    }
                    if newly_pressed(&report, &prev, BTN_A)
                        || newly_pressed(&report, &prev, BTN_MENU)
                    {
                        return Some(MenuCommand::Select);
                    }
                    if newly_pressed(&report, &prev, BTN_B) {
                        return Some(MenuCommand::Back);
                    }

                    // Stick: up/down only.
                    let sx = report.left_stick_x as f32 / 32768.0;
                    let sy = report.left_stick_y as f32 / 32768.0;
                    if let Some((_, dy)) = check_stick_common(sx, sy, &mut self.stick_engaged) {
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
                                if let Some((dx, dy)) = dpad_direction(&self.dpad_held) {
                                    return Some(LookCommand::Move { dx, dy });
                                }
                            }
                            Button::East | Button::Start => {
                                return Some(LookCommand::Close);
                            }
                            _ => {}
                        },
                        EventType::AxisChanged(Axis::LeftStickX | Axis::LeftStickY, _, _) => {
                            if let Some((dx, dy)) =
                                check_gilrs_stick(gilrs, &mut self.stick_engaged)
                            {
                                return Some(LookCommand::Move { dx, dy });
                            }
                        }
                        _ => {}
                    }
                },
                #[cfg(feature = "raw-usb")]
                Backend::RawUsb {
                    usb,
                    prev_report,
                    pending_report,
                } => {
                    use super::raw_usb::*;
                    let report = pending_report.take().or_else(|| usb.read_input())?;
                    let prev = *prev_report;
                    *prev_report = report;

                    update_held_from_report(&report, &mut self.dpad_held, &mut self.lb_held);

                    if newly_pressed(&report, &prev, BTN_B)
                        || newly_pressed(&report, &prev, BTN_MENU)
                    {
                        return Some(LookCommand::Close);
                    }

                    // D-pad movement.
                    if (newly_pressed(&report, &prev, BTN_DPAD_UP)
                        || newly_pressed(&report, &prev, BTN_DPAD_DOWN)
                        || newly_pressed(&report, &prev, BTN_DPAD_LEFT)
                        || newly_pressed(&report, &prev, BTN_DPAD_RIGHT))
                        && let Some((dx, dy)) = dpad_direction(&self.dpad_held)
                    {
                        return Some(LookCommand::Move { dx, dy });
                    }

                    // Stick.
                    let sx = report.left_stick_x as f32 / 32768.0;
                    let sy = report.left_stick_y as f32 / 32768.0;
                    if let Some((dx, dy)) = check_stick_common(sx, sy, &mut self.stick_engaged) {
                        return Some(LookCommand::Move { dx, dy });
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
                        && let Some((_, dy)) = check_gilrs_stick(gilrs, &mut self.stick_engaged)
                    {
                        if dy < 0 {
                            return Some(HistoryCommand::Menu(MenuCommand::Up));
                        }
                        if dy > 0 {
                            return Some(HistoryCommand::Menu(MenuCommand::Down));
                        }
                    }
                },
                #[cfg(feature = "raw-usb")]
                Backend::RawUsb {
                    usb,
                    prev_report,
                    pending_report,
                } => {
                    use super::raw_usb::*;
                    let report = pending_report.take().or_else(|| usb.read_input())?;
                    let prev = *prev_report;
                    *prev_report = report;

                    update_held_from_report(&report, &mut self.dpad_held, &mut self.lb_held);

                    if newly_pressed(&report, &prev, BTN_DPAD_UP) {
                        return Some(HistoryCommand::Menu(MenuCommand::Up));
                    }
                    if newly_pressed(&report, &prev, BTN_DPAD_DOWN) {
                        return Some(HistoryCommand::Menu(MenuCommand::Down));
                    }
                    if newly_pressed(&report, &prev, BTN_LB) {
                        return Some(HistoryCommand::PageUp);
                    }
                    if newly_pressed(&report, &prev, BTN_RB) {
                        return Some(HistoryCommand::PageDown);
                    }
                    if newly_pressed(&report, &prev, BTN_A)
                        || newly_pressed(&report, &prev, BTN_B)
                        || newly_pressed(&report, &prev, BTN_MENU)
                    {
                        return Some(HistoryCommand::Menu(MenuCommand::Back));
                    }

                    // Stick: up/down only.
                    let sx = report.left_stick_x as f32 / 32768.0;
                    let sy = report.left_stick_y as f32 / 32768.0;
                    if let Some((_, dy)) = check_stick_common(sx, sy, &mut self.stick_engaged) {
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

    // -- Raw USB / GIP protocol tests (no hardware required) --

    #[cfg(feature = "raw-usb")]
    mod gip_tests {
        use super::super::raw_usb::*;

        /// Build an 18-byte GIP input report buffer from field values.
        fn make_report(
            buttons: u16,
            lt: u16,
            rt: u16,
            lx: i16,
            ly: i16,
            rx: i16,
            ry: i16,
        ) -> [u8; 18] {
            let mut buf = [0u8; 18];
            buf[0] = 0x20; // command byte
            buf[1] = 0x00; // flags
            buf[2] = 0x01; // sequence
            buf[3] = 0x0E; // length
            buf[4..6].copy_from_slice(&buttons.to_le_bytes());
            buf[6..8].copy_from_slice(&lt.to_le_bytes());
            buf[8..10].copy_from_slice(&rt.to_le_bytes());
            buf[10..12].copy_from_slice(&lx.to_le_bytes());
            buf[12..14].copy_from_slice(&ly.to_le_bytes());
            buf[14..16].copy_from_slice(&rx.to_le_bytes());
            buf[16..18].copy_from_slice(&ry.to_le_bytes());
            buf
        }

        #[test]
        fn parse_idle_report() {
            let buf = make_report(0, 0, 0, 0, 0, 0, 0);
            let report = GipInputReport::parse(&buf);
            assert_eq!(report, GipInputReport::default());
        }

        #[test]
        fn parse_button_a() {
            let buf = make_report(BTN_A, 0, 0, 0, 0, 0, 0);
            let report = GipInputReport::parse(&buf);
            assert!(report.has(BTN_A));
            assert!(!report.has(BTN_B));
            assert!(!report.has(BTN_X));
        }

        #[test]
        fn parse_each_button() {
            let buttons = [
                BTN_A,
                BTN_B,
                BTN_X,
                BTN_Y,
                BTN_MENU,
                BTN_VIEW,
                BTN_LB,
                BTN_RB,
                BTN_DPAD_UP,
                BTN_DPAD_DOWN,
                BTN_DPAD_LEFT,
                BTN_DPAD_RIGHT,
            ];
            for &btn in &buttons {
                let buf = make_report(btn, 0, 0, 0, 0, 0, 0);
                let report = GipInputReport::parse(&buf);
                assert!(report.has(btn), "button 0x{btn:04x} not set");
                // Every other button should be off.
                for &other in &buttons {
                    if other != btn {
                        assert!(!report.has(other), "button 0x{other:04x} should be off");
                    }
                }
            }
        }

        #[test]
        fn parse_multi_button() {
            let bits = BTN_A | BTN_LB | BTN_DPAD_UP;
            let buf = make_report(bits, 0, 0, 0, 0, 0, 0);
            let report = GipInputReport::parse(&buf);
            assert!(report.has(BTN_A));
            assert!(report.has(BTN_LB));
            assert!(report.has(BTN_DPAD_UP));
            assert!(!report.has(BTN_B));
        }

        #[test]
        fn parse_dpad_combos() {
            let diagonal = BTN_DPAD_UP | BTN_DPAD_RIGHT;
            let buf = make_report(diagonal, 0, 0, 0, 0, 0, 0);
            let report = GipInputReport::parse(&buf);
            assert!(report.has(BTN_DPAD_UP));
            assert!(report.has(BTN_DPAD_RIGHT));
            assert!(!report.has(BTN_DPAD_DOWN));
            assert!(!report.has(BTN_DPAD_LEFT));
        }

        #[test]
        fn parse_triggers() {
            let buf = make_report(0, 512, 1023, 0, 0, 0, 0);
            let report = GipInputReport::parse(&buf);
            assert_eq!(report.left_trigger, 512);
            assert_eq!(report.right_trigger, 1023);
        }

        #[test]
        fn parse_stick_extremes() {
            // Full left-down on left stick, full right-up on right stick.
            let buf = make_report(0, 0, 0, -32768, -32768, 32767, 32767);
            let report = GipInputReport::parse(&buf);
            assert_eq!(report.left_stick_x, -32768);
            assert_eq!(report.left_stick_y, -32768);
            assert_eq!(report.right_stick_x, 32767);
            assert_eq!(report.right_stick_y, 32767);
        }

        #[test]
        fn newly_pressed_fires_once() {
            let idle = GipInputReport::default();
            let pressed = GipInputReport {
                buttons: BTN_A,
                ..Default::default()
            };
            // Press edge: fires.
            assert!(newly_pressed(&pressed, &idle, BTN_A));
            // Hold: does not fire.
            assert!(!newly_pressed(&pressed, &pressed, BTN_A));
            // Release: does not fire.
            assert!(!newly_pressed(&idle, &pressed, BTN_A));
        }

        #[test]
        fn newly_pressed_independent_buttons() {
            let prev = GipInputReport {
                buttons: BTN_A,
                ..Default::default()
            };
            let curr = GipInputReport {
                buttons: BTN_A | BTN_B,
                ..Default::default()
            };
            // B is newly pressed, A is held.
            assert!(newly_pressed(&curr, &prev, BTN_B));
            assert!(!newly_pressed(&curr, &prev, BTN_A));
        }

        #[test]
        fn stick_normalization_positive_y_is_up() {
            // GIP: positive Y = up. Same convention as gilrs.
            // analog_to_direction expects positive Y = up and negates it for screen.
            use super::super::inner::analog_to_direction;
            // Full up: i16 max = 32767 → 32767/32768 ≈ 1.0 → screen up = (0, -1)
            let sy = 32767i16 as f32 / 32768.0;
            assert_eq!(analog_to_direction(0.0, sy), (0, -1));
            // Full down: i16 min = -32768 → -32768/32768 = -1.0 → screen down = (0, 1)
            let sy = -32768i16 as f32 / 32768.0;
            assert_eq!(analog_to_direction(0.0, sy), (0, 1));
        }
    }

    #[cfg(feature = "raw-usb")]
    mod gip_command_mapping_tests {
        use super::super::raw_usb::*;
        use roguelike_core::command::GameCommand;
        use roguelike_core::look::LookCommand;
        use roguelike_core::platform::MenuCommand;

        /// Pure function: map a GIP face button to a game command via edge detection.
        fn gip_face_to_game_cmd(btn: u16) -> Option<GameCommand> {
            match btn {
                b if b == BTN_A => Some(GameCommand::Wait),
                b if b == BTN_B || b == BTN_MENU => Some(GameCommand::Quit),
                b if b == BTN_X => Some(GameCommand::AutoExplore),
                b if b == BTN_Y => Some(GameCommand::Look),
                _ => None,
            }
        }

        fn gip_to_menu_cmd(btn: u16) -> Option<MenuCommand> {
            match btn {
                b if b == BTN_DPAD_UP => Some(MenuCommand::Up),
                b if b == BTN_DPAD_DOWN => Some(MenuCommand::Down),
                b if b == BTN_A || b == BTN_MENU => Some(MenuCommand::Select),
                b if b == BTN_B => Some(MenuCommand::Back),
                _ => None,
            }
        }

        fn gip_to_look_cmd(btn: u16) -> Option<LookCommand> {
            match btn {
                b if b == BTN_B || b == BTN_MENU => Some(LookCommand::Close),
                _ => None,
            }
        }

        #[test]
        fn game_face_buttons() {
            assert_eq!(gip_face_to_game_cmd(BTN_A), Some(GameCommand::Wait));
            assert_eq!(gip_face_to_game_cmd(BTN_B), Some(GameCommand::Quit));
            assert_eq!(gip_face_to_game_cmd(BTN_X), Some(GameCommand::AutoExplore));
            assert_eq!(gip_face_to_game_cmd(BTN_Y), Some(GameCommand::Look));
            assert_eq!(gip_face_to_game_cmd(BTN_MENU), Some(GameCommand::Quit));
        }

        #[test]
        fn menu_buttons() {
            assert_eq!(gip_to_menu_cmd(BTN_DPAD_UP), Some(MenuCommand::Up));
            assert_eq!(gip_to_menu_cmd(BTN_DPAD_DOWN), Some(MenuCommand::Down));
            assert_eq!(gip_to_menu_cmd(BTN_A), Some(MenuCommand::Select));
            assert_eq!(gip_to_menu_cmd(BTN_MENU), Some(MenuCommand::Select));
            assert_eq!(gip_to_menu_cmd(BTN_B), Some(MenuCommand::Back));
            assert_eq!(gip_to_menu_cmd(BTN_X), None);
        }

        #[test]
        fn look_buttons() {
            assert_eq!(gip_to_look_cmd(BTN_B), Some(LookCommand::Close));
            assert_eq!(gip_to_look_cmd(BTN_MENU), Some(LookCommand::Close));
            assert_eq!(gip_to_look_cmd(BTN_A), None);
        }
    }
}
