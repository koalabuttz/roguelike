//! Platform-independent color enum shared by all capability tiers.
//!
//! `GameColor` is the color vocabulary for the entire engine — game rules,
//! entity data, UI chrome, and renderer traits all speak this type. It lives
//! in `rules/` because it must be available to every tier, including `no_std`
//! constrained platforms (C64, GBA).
//!
//! Each platform maps `GameColor` to its native palette in the renderer.
//! The terminal renderer uses `palette_color()` in `tui/render.rs`; a C64
//! frontend would map discriminants to PETSCII color codes via a lookup table.

/// Platform-independent color for game rendering.
///
/// `#[repr(u8)]` gives each variant a stable discriminant for serialization
/// and cross-platform save compatibility. The discriminant values are
/// sequential and internal — they do **not** correspond to any hardware
/// palette (C64 PETSCII, GBA, etc.). Every platform needs its own mapping.
///
/// **Size:** 4 bytes due to the `Rgb` payload, not 1. Constrained platforms
/// that need single-byte colors should avoid the `Rgb` variant (which is
/// standard-tier only) and rely on the named variants, whose discriminants
/// fit in a `u8`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum GameColor {
    Black = 0,
    White = 1,
    Grey = 2,
    DarkGrey = 3,
    Red = 4,
    DarkRed = 5,
    Green = 6,
    DarkGreen = 7,
    Yellow = 8,
    DarkBlue = 9,
    Cyan = 10,
    /// Arbitrary RGB color for dev-tool overlays and accessibility palettes.
    /// Standard-tier only — constrained platforms never construct this.
    Rgb(u8, u8, u8),
}
