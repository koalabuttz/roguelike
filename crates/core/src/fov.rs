use std::collections::HashSet;

use crate::map::Map;
use crate::types::{Coord, Pos};

/// Octant multipliers for recursive shadowcasting.
/// Each column represents one of 8 octants around the origin.
const MULT_XX: [i32; 8] = [1, 0, 0, -1, -1, 0, 0, 1];
const MULT_XY: [i32; 8] = [0, 1, -1, 0, 0, -1, 1, 0];
const MULT_YX: [i32; 8] = [0, 1, 1, 0, 0, -1, -1, 0];
const MULT_YY: [i32; 8] = [1, 0, 0, 1, -1, 0, 0, -1];

/// Compute field of view from (ox, oy) with the given radius.
/// Returns a set of all visible (x, y) positions.
pub fn compute_fov(map: &Map, ox: Coord, oy: Coord, radius: Coord) -> HashSet<Pos> {
    let mut visible = HashSet::new();
    visible.insert((ox, oy));

    for octant in 0..8 {
        cast_light_cb(
            map,
            &mut |x, y| {
                visible.insert((x, y));
                true // continue scanning
            },
            ox,
            oy,
            radius,
            1,
            1.0,
            0.0,
            MULT_XX[octant],
            MULT_XY[octant],
            MULT_YX[octant],
            MULT_YY[octant],
        );
    }

    visible
}

/// Check whether (tx, ty) is visible from (ox, oy) within the given radius.
///
/// Uses the same shadowcasting algorithm as `compute_fov` but with early exit:
/// returns `true` as soon as the target tile is confirmed visible, without
/// computing the full FOV set. Zero allocation.
pub fn can_see(map: &Map, ox: Coord, oy: Coord, tx: Coord, ty: Coord, radius: Coord) -> bool {
    // Origin always sees itself.
    if ox == tx && oy == ty {
        return true;
    }

    // Chebyshev distance pre-check: if target is beyond radius, it's never visible.
    if (tx - ox).abs().max((ty - oy).abs()) > radius {
        return false;
    }

    let mut found = false;

    for octant in 0..8 {
        let stop = !cast_light_cb(
            map,
            &mut |x, y| {
                if x == tx && y == ty {
                    found = true;
                    false // stop scanning — target found
                } else {
                    true // continue
                }
            },
            ox,
            oy,
            radius,
            1,
            1.0,
            0.0,
            MULT_XX[octant],
            MULT_XY[octant],
            MULT_YX[octant],
            MULT_YY[octant],
        );
        if stop {
            break;
        }
    }

    found
}

fn is_blocking(map: &Map, x: Coord, y: Coord) -> bool {
    !map.in_bounds(x, y) || !map.tiles[map.idx(x, y)].is_walkable()
}

/// Generic shadowcasting with a callback.
///
/// Calls `on_visible(map_x, map_y)` for each visible tile. The callback returns
/// `true` to continue scanning or `false` to request early exit. The function
/// itself returns `false` if an early exit was requested.
#[allow(clippy::too_many_arguments)]
fn cast_light_cb<F>(
    map: &Map,
    on_visible: &mut F,
    ox: i32,
    oy: i32,
    radius: i32,
    row: i32,
    mut start_slope: f64,
    end_slope: f64,
    xx: i32,
    xy: i32,
    yx: i32,
    yy: i32,
) -> bool
where
    F: FnMut(Coord, Coord) -> bool,
{
    if start_slope < end_slope {
        return true;
    }

    let radius_sq = radius * radius;
    let mut next_start_slope = start_slope;

    for j in row..=radius {
        let dy = -j;
        let mut blocked = false;

        for dx in -j..=0 {
            let l_slope = (dx as f64 - 0.5) / (dy as f64 + 0.5);
            let r_slope = (dx as f64 + 0.5) / (dy as f64 - 0.5);

            if start_slope < r_slope {
                continue;
            }
            if end_slope > l_slope {
                break;
            }

            let map_x = ox + dx * xx + dy * xy;
            let map_y = oy + dx * yx + dy * yy;

            // Only mark visible if within the circular radius
            if (dx * dx + dy * dy) < radius_sq
                && map.in_bounds(map_x, map_y)
                && !on_visible(map_x, map_y)
            {
                return false; // early exit requested
            }

            if blocked {
                if is_blocking(map, map_x, map_y) {
                    next_start_slope = r_slope;
                    continue;
                } else {
                    blocked = false;
                    start_slope = next_start_slope;
                }
            } else if is_blocking(map, map_x, map_y) && j < radius {
                blocked = true;
                if !cast_light_cb(
                    map,
                    on_visible,
                    ox,
                    oy,
                    radius,
                    j + 1,
                    start_slope,
                    l_slope,
                    xx,
                    xy,
                    yx,
                    yy,
                ) {
                    return false; // propagate early exit
                }
                next_start_slope = r_slope;
            }
        }

        if blocked {
            break;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Map, Tile};

    /// Create a small open map (all floor except border walls).
    fn open_map(w: i32, h: i32) -> Map {
        let mut m = Map::new(w, h);
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                let idx = m.idx(x, y);
                m.tiles[idx] = Tile::Floor;
            }
        }
        m
    }

    #[test]
    fn can_see_clear_line() {
        let map = open_map(20, 20);
        // Target within radius on open map → visible
        assert!(can_see(&map, 5, 5, 8, 5, 8));
        assert!(can_see(&map, 5, 5, 5, 8, 8));
        assert!(can_see(&map, 5, 5, 8, 8, 8)); // diagonal
    }

    #[test]
    fn can_see_blocked_by_wall() {
        let mut map = open_map(20, 20);
        // Place a wall between origin and target
        let idx = map.idx(7, 5);
        map.tiles[idx] = Tile::Wall;
        // Target at (9, 5) is behind the wall from (5, 5)
        assert!(!can_see(&map, 5, 5, 9, 5, 8));
    }

    #[test]
    fn can_see_out_of_range() {
        let map = open_map(30, 30);
        // Target beyond radius → not visible (Chebyshev pre-check)
        assert!(!can_see(&map, 5, 5, 20, 5, 8));
        assert!(!can_see(&map, 5, 5, 5, 20, 8));
    }

    #[test]
    fn can_see_self() {
        let map = open_map(10, 10);
        assert!(can_see(&map, 5, 5, 5, 5, 8));
    }

    #[test]
    fn compute_fov_unchanged() {
        // Verify refactored compute_fov produces consistent results with can_see
        let map = open_map(20, 20);
        let visible = compute_fov(&map, 10, 10, 8);
        // Every tile in the visible set should be can_see-able
        for &(x, y) in &visible {
            assert!(
                can_see(&map, 10, 10, x, y, 8),
                "({},{}) in compute_fov but not in can_see",
                x,
                y
            );
        }
        // Origin should always be visible
        assert!(visible.contains(&(10, 10)));
    }
}
