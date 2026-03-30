//! mGBA debug console logging (feature-gated).
//!
//! When running in mGBA, messages appear in Tools > View Logs.
//! When running on hardware or non-mGBA emulators, logging silently no-ops
//! (MgbaBufferedLogger::try_new returns Err).
//!
//! - `debug_log!` — Debug level, compiles to nothing without `dev` feature
//! - `debug_log_fatal!` — Error+Fatal, compiles to nothing without `dev` feature

/// Log a message to the mGBA debug console at Debug level.
/// Compiles to nothing when the `dev` feature is disabled.
#[cfg(feature = "dev")]
macro_rules! debug_log {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        if let Ok(mut logger) = gba::mgba::MgbaBufferedLogger::try_new(
            gba::mgba::MgbaMessageLevel::Debug,
        ) {
            let _ = write!(logger, $($arg)*);
        }
    }};
}

#[cfg(not(feature = "dev"))]
macro_rules! debug_log {
    ($($arg:tt)*) => {};
}

/// Log at Error level (full message) then Fatal (halts mGBA emulation).
/// Feature-gated to `dev` — uses `core::fmt` which adds ~1.7 KB to ROM.
/// Fatal truncates at 256 bytes, so we log the full message at Error first.
#[cfg(feature = "dev")]
macro_rules! debug_log_fatal {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        if let Ok(mut logger) = gba::mgba::MgbaBufferedLogger::try_new(
            gba::mgba::MgbaMessageLevel::Error,
        ) {
            let _ = write!(logger, $($arg)*);
        }
        if let Ok(mut logger) = gba::mgba::MgbaBufferedLogger::try_new(
            gba::mgba::MgbaMessageLevel::Fatal,
        ) {
            let _ = write!(logger, "FATAL — see Error log above");
        }
    }};
}

#[cfg(not(feature = "dev"))]
macro_rules! debug_log_fatal {
    ($($arg:tt)*) => {};
}

pub(crate) use debug_log;
pub(crate) use debug_log_fatal;
