use std::collections::HashSet;

use crate::map::{Map, Tile};

/// Octant multipliers for recursive shadowcasting.
/// Each column represents one of 8 octants around the origin.
const MULT_XX: [i32; 8] = [1, 0, 0, -1, -1, 0, 0, 1];
const MULT_XY: [i32; 8] = [0, 1, -1, 0, 0, -1, 1, 0];
const MULT_YX: [i32; 8] = [0, 1, 1, 0, 0, -1, -1, 0];
const MULT_YY: [i32; 8] = [1, 0, 0, 1, -1, 0, 0, -1];

/// Compute field of view from (ox, oy) with the given radius.
/// Returns a set of all visible (x, y) positions.
pub fn compute_fov(map: &Map, ox: i32, oy: i32, radius: i32) -> HashSet<(i32, i32)> {
    let mut visible = HashSet::new();
    visible.insert((ox, oy));

    for octant in 0..8 {
        cast_light(
            map,
            &mut visible,
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

fn is_blocking(map: &Map, x: i32, y: i32) -> bool {
    !map.in_bounds(x, y) || map.tiles[map.idx(x, y)] == Tile::Wall
}

#[allow(clippy::too_many_arguments)]
fn cast_light(
    map: &Map,
    visible: &mut HashSet<(i32, i32)>,
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
) {
    if start_slope < end_slope {
        return;
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
            if (dx * dx + dy * dy) < radius_sq && map.in_bounds(map_x, map_y) {
                visible.insert((map_x, map_y));
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
                cast_light(
                    map,
                    visible,
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
                );
                next_start_slope = r_slope;
            }
        }

        if blocked {
            break;
        }
    }
}
