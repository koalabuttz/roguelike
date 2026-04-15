//! Pure dungeon layout helpers shared by all capability tiers.
//!
//! The generation loop stays per-tier (coupled to RNG and tile storage),
//! but room geometry and corridor routing are shared here.

/// A corridor segment to carve into tile storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorridorSegment {
    /// Horizontal tunnel at `y` from `min(x1,x2)` to `max(x1,x2)`.
    Horizontal { x1: i32, x2: i32, y: i32 },
    /// Vertical tunnel at `x` from `min(y1,y2)` to `max(y1,y2)`.
    Vertical { y1: i32, y2: i32, x: i32 },
}

/// Check if two rooms overlap (with 1-tile wall padding).
/// All arguments in (x, y, w, h) form.
#[allow(clippy::too_many_arguments)]
pub const fn rooms_intersect(
    ax: i32,
    ay: i32,
    aw: i32,
    ah: i32,
    bx: i32,
    by: i32,
    bw: i32,
    bh: i32,
) -> bool {
    ax <= bx + bw && ax + aw >= bx && ay <= by + bh && ay + ah >= by
}

/// Room center from (x, y, w, h).
pub const fn room_center(x: i32, y: i32, w: i32, h: i32) -> (i32, i32) {
    (x + w / 2, y + h / 2)
}

/// Compute the L-shaped corridor between two room centers.
///
/// `h_first`: if true, carve horizontal then vertical; otherwise vertical
/// then horizontal. The caller uses their RNG's coin flip for this.
pub const fn corridor_between(
    prev_cx: i32,
    prev_cy: i32,
    new_cx: i32,
    new_cy: i32,
    h_first: bool,
) -> [CorridorSegment; 2] {
    if h_first {
        [
            CorridorSegment::Horizontal {
                x1: prev_cx,
                x2: new_cx,
                y: prev_cy,
            },
            CorridorSegment::Vertical {
                y1: prev_cy,
                y2: new_cy,
                x: new_cx,
            },
        ]
    } else {
        [
            CorridorSegment::Vertical {
                y1: prev_cy,
                y2: new_cy,
                x: prev_cx,
            },
            CorridorSegment::Horizontal {
                x1: prev_cx,
                x2: new_cx,
                y: new_cy,
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- rooms_intersect --

    #[test]
    fn overlapping_rooms() {
        assert!(rooms_intersect(0, 0, 5, 5, 3, 3, 5, 5));
    }

    #[test]
    fn non_overlapping_rooms() {
        assert!(!rooms_intersect(0, 0, 5, 5, 10, 10, 5, 5));
    }

    #[test]
    fn adjacent_rooms_overlap() {
        // With wall padding, adjacent rooms are considered overlapping.
        assert!(rooms_intersect(0, 0, 5, 5, 5, 0, 5, 5));
    }

    #[test]
    fn one_tile_gap() {
        assert!(!rooms_intersect(0, 0, 5, 5, 6, 0, 5, 5));
    }

    #[test]
    fn identical_rooms() {
        assert!(rooms_intersect(3, 3, 4, 4, 3, 3, 4, 4));
    }

    // -- room_center --

    #[test]
    fn even_dimensions() {
        assert_eq!(room_center(10, 20, 6, 8), (13, 24));
    }

    #[test]
    fn odd_dimensions() {
        assert_eq!(room_center(10, 20, 5, 7), (12, 23));
    }

    // -- corridor_between --

    #[test]
    fn h_first_corridor() {
        let segs = corridor_between(5, 10, 20, 30, true);
        assert_eq!(
            segs[0],
            CorridorSegment::Horizontal {
                x1: 5,
                x2: 20,
                y: 10
            }
        );
        assert_eq!(
            segs[1],
            CorridorSegment::Vertical {
                y1: 10,
                y2: 30,
                x: 20
            }
        );
    }

    #[test]
    fn v_first_corridor() {
        let segs = corridor_between(5, 10, 20, 30, false);
        assert_eq!(
            segs[0],
            CorridorSegment::Vertical {
                y1: 10,
                y2: 30,
                x: 5
            }
        );
        assert_eq!(
            segs[1],
            CorridorSegment::Horizontal {
                x1: 5,
                x2: 20,
                y: 30
            }
        );
    }

    #[test]
    fn same_center_corridor() {
        let segs = corridor_between(10, 10, 10, 10, true);
        assert_eq!(
            segs[0],
            CorridorSegment::Horizontal {
                x1: 10,
                x2: 10,
                y: 10
            }
        );
    }
}
