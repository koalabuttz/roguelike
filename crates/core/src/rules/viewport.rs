//! Player-centered viewport math shared by all port renderers.
//!
//! Pure `no_std` functions. Two widths coexist:
//! - [`player_centered_i32`] for standard and compact tiers (and the
//!   [`GameView::viewport_origin`](super::game_view::GameView::viewport_origin)
//!   default impl).
//! - [`player_centered_u8`] for the micro tier / C64, which avoids
//!   `u8 → i32 → u8` conversions on 6502.
//!
//! Both functions compute the same thing: the top-left world tile of a
//! viewport centered on the player, clamped so the viewport never extends
//! past the map boundary. If the map is smaller than the viewport in
//! either axis, that axis pins to 0.
//!
//! Keep it `const fn` — callers may compute viewports at compile time.

/// Compute the top-left world tile of a player-centered viewport, clamped
/// to map bounds.
///
/// All arguments and return values are `i32` so the math accommodates
/// negative intermediate values without overflow or underflow.
pub const fn player_centered_i32(
    px: i32,
    py: i32,
    map_w: i32,
    map_h: i32,
    viewport_w: i32,
    viewport_h: i32,
) -> (i32, i32) {
    let max_vx = if map_w > viewport_w {
        map_w - viewport_w
    } else {
        0
    };
    let max_vy = if map_h > viewport_h {
        map_h - viewport_h
    } else {
        0
    };
    let ideal_vx = px - viewport_w / 2;
    let ideal_vy = py - viewport_h / 2;
    let vx = if ideal_vx < 0 {
        0
    } else if ideal_vx > max_vx {
        max_vx
    } else {
        ideal_vx
    };
    let vy = if ideal_vy < 0 {
        0
    } else if ideal_vy > max_vy {
        max_vy
    } else {
        ideal_vy
    };
    (vx, vy)
}

/// Compute the top-left world tile of a player-centered viewport, clamped
/// to map bounds. `u8`-native for micro tier / 6502 hot paths.
///
/// Uses `saturating_sub` and `min` to stay in `u8` throughout. The u8
/// codomain means every value is implicitly `>= 0`, so no explicit
/// lower-bound clamp is needed.
pub const fn player_centered_u8(
    px: u8,
    py: u8,
    map_w: u8,
    map_h: u8,
    viewport_w: u8,
    viewport_h: u8,
) -> (u8, u8) {
    let max_vx = map_w.saturating_sub(viewport_w);
    let max_vy = map_h.saturating_sub(viewport_h);
    let ideal_vx = px.saturating_sub(viewport_w / 2);
    let ideal_vy = py.saturating_sub(viewport_h / 2);
    let vx = if ideal_vx < max_vx { ideal_vx } else { max_vx };
    let vy = if ideal_vy < max_vy { ideal_vy } else { max_vy };
    (vx, vy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i32_centers_on_player() {
        // Large map, player in middle, viewport centers.
        let (vx, vy) = player_centered_i32(50, 30, 100, 60, 40, 20);
        assert_eq!((vx, vy), (30, 20));
    }

    #[test]
    fn i32_clamps_to_zero_near_top_left() {
        let (vx, vy) = player_centered_i32(2, 1, 100, 60, 40, 20);
        assert_eq!((vx, vy), (0, 0));
    }

    #[test]
    fn i32_clamps_to_max_near_bottom_right() {
        let (vx, vy) = player_centered_i32(99, 59, 100, 60, 40, 20);
        assert_eq!((vx, vy), (60, 40));
    }

    #[test]
    fn i32_pins_to_zero_when_map_smaller_than_viewport() {
        let (vx, vy) = player_centered_i32(5, 5, 20, 10, 40, 20);
        assert_eq!((vx, vy), (0, 0));
    }

    #[test]
    fn i32_handles_negative_player_coords() {
        let (vx, vy) = player_centered_i32(-5, -5, 100, 60, 40, 20);
        assert_eq!((vx, vy), (0, 0));
    }

    #[test]
    fn u8_centers_on_player() {
        let (vx, vy) = player_centered_u8(50, 30, 100, 60, 40, 20);
        assert_eq!((vx, vy), (30, 20));
    }

    #[test]
    fn u8_clamps_to_zero_near_top_left() {
        let (vx, vy) = player_centered_u8(2, 1, 100, 60, 40, 20);
        assert_eq!((vx, vy), (0, 0));
    }

    #[test]
    fn u8_clamps_to_max_near_bottom_right() {
        let (vx, vy) = player_centered_u8(99, 59, 100, 60, 40, 20);
        assert_eq!((vx, vy), (60, 40));
    }

    #[test]
    fn u8_pins_to_zero_when_map_smaller_than_viewport() {
        let (vx, vy) = player_centered_u8(5, 5, 20, 10, 40, 20);
        assert_eq!((vx, vy), (0, 0));
    }

    #[test]
    fn u8_and_i32_agree_on_common_inputs() {
        let cases = [
            (50, 30, 100, 60, 40, 20),
            (2, 1, 100, 60, 40, 20),
            (99, 59, 100, 60, 40, 20),
            (0, 0, 64, 48, 40, 22),
        ];
        for (px, py, mw, mh, vw, vh) in cases {
            let (i32_vx, i32_vy) = player_centered_i32(px, py, mw, mh, vw, vh);
            let (u8_vx, u8_vy) =
                player_centered_u8(px as u8, py as u8, mw as u8, mh as u8, vw as u8, vh as u8);
            assert_eq!(
                (i32_vx as u8, i32_vy as u8),
                (u8_vx, u8_vy),
                "inputs: {:?}",
                (px, py, mw, mh, vw, vh)
            );
        }
    }

    #[test]
    fn u8_viewport_equal_to_map_pins_zero() {
        let (vx, vy) = player_centered_u8(5, 5, 40, 20, 40, 20);
        assert_eq!((vx, vy), (0, 0));
    }

    #[test]
    fn const_usable_at_compile_time() {
        const VP: (i32, i32) = player_centered_i32(50, 30, 100, 60, 40, 20);
        const VPU: (u8, u8) = player_centered_u8(50, 30, 100, 60, 40, 20);
        assert_eq!(VP, (30, 20));
        assert_eq!(VPU, (30, 20));
    }
}
