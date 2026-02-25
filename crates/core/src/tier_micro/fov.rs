//! Field of view — Bresenham raycasting for the micro tier.
//!
//! Casts ~40 rays from the player to the perimeter of a circle (radius 6),
//! using Bresenham's line algorithm (integer-only). Each ray marks tiles
//! visible until it hits a wall.

use super::map::{MicroMap, TILE_WALL};
use super::types::*;
use crate::rules::balance;

pub const FOV_RADIUS: u8 = balance::MICRO_FOV_RADIUS;

fn bit_idx(x: u8, y: u8) -> (usize, u8) {
    let i = (y as usize) * (MAP_WIDTH as usize) + (x as usize);
    (i / 8, 1u8 << (i % 8))
}

/// Precomputed perimeter offsets for FOV_RADIUS=6.
const PERIMETER: [(i8, i8); 40] = [
    // Top half, left to right
    (-6, 0),
    (-6, -1),
    (-5, -2),
    (-5, -3),
    (-4, -4),
    (-3, -5),
    (-2, -5),
    (-1, -6),
    (0, -6),
    (1, -6),
    (2, -5),
    (3, -5),
    (4, -4),
    (5, -3),
    (5, -2),
    (6, -1),
    (6, 0),
    // Bottom half
    (6, 1),
    (5, 2),
    (5, 3),
    (4, 4),
    (3, 5),
    (2, 5),
    (1, 6),
    (0, 6),
    (-1, 6),
    (-2, 5),
    (-3, 5),
    (-4, 4),
    (-5, 3),
    (-5, 2),
    (-6, 1),
    // Fill corners for coverage
    (-4, -5),
    (4, -5),
    (4, 5),
    (-4, 5),
    (-3, -6),
    (3, -6),
    (3, 6),
    (-3, 6),
];

pub struct MicroFov {
    visible: [u8; BITFIELD_SIZE],
    explored: [u8; BITFIELD_SIZE],
}

impl Default for MicroFov {
    fn default() -> Self {
        Self::new()
    }
}

impl MicroFov {
    pub fn new() -> Self {
        Self {
            visible: [0; BITFIELD_SIZE],
            explored: [0; BITFIELD_SIZE],
        }
    }

    pub fn is_visible(&self, x: u8, y: u8) -> bool {
        if !MicroMap::in_bounds(x, y) {
            return false;
        }
        let (byte, bit) = bit_idx(x, y);
        self.visible[byte] & bit != 0
    }

    pub fn is_explored(&self, x: u8, y: u8) -> bool {
        if !MicroMap::in_bounds(x, y) {
            return false;
        }
        let (byte, bit) = bit_idx(x, y);
        self.explored[byte] & bit != 0
    }

    pub fn explored_floor_count(&self, map: &MicroMap) -> u16 {
        let mut count: u16 = 0;
        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                if self.is_explored(x, y) && map.is_walkable(x, y) {
                    count += 1;
                }
            }
        }
        count
    }

    fn mark_visible(&mut self, x: u8, y: u8) {
        let (byte, bit) = bit_idx(x, y);
        self.visible[byte] |= bit;
        self.explored[byte] |= bit;
    }

    fn clear_visible(&mut self) {
        for b in self.visible.iter_mut() {
            *b = 0;
        }
    }

    /// Cast a single ray, marking tiles visible until hitting a wall or leaving bounds.
    fn cast_ray(&mut self, ox: u8, oy: u8, tx: u8, ty: u8, map: &MicroMap) {
        let mut x = ox as i8;
        let mut y = oy as i8;
        let target_x = tx as i8;
        let target_y = ty as i8;

        let dx = if target_x > x {
            target_x - x
        } else {
            x - target_x
        };
        let dy = if target_y > y {
            target_y - y
        } else {
            y - target_y
        };
        let sx: i8 = if target_x > x { 1 } else { -1 };
        let sy: i8 = if target_y > y { 1 } else { -1 };
        let mut err = dx - dy;

        loop {
            let ux = x as u8;
            let uy = y as u8;

            if !MicroMap::in_bounds(ux, uy) {
                break;
            }

            self.mark_visible(ux, uy);

            if map.tile_at(ux, uy) == TILE_WALL {
                break;
            }

            if x == target_x && y == target_y {
                break;
            }

            let e2 = err * 2;
            if e2 > -dy {
                err -= dy;
                x += sx;
            }
            if e2 < dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Compute field of view from (ox, oy). Clears previous visible set.
    pub fn compute_fov(&mut self, ox: u8, oy: u8, map: &MicroMap) {
        self.clear_visible();
        self.mark_visible(ox, oy);

        for &(dx, dy) in PERIMETER.iter() {
            let tx = (ox as i8 + dx) as u8;
            let ty = (oy as i8 + dy) as u8;
            if tx >= MAP_WIDTH || ty >= MAP_HEIGHT {
                continue;
            }
            self.cast_ray(ox, oy, tx, ty, map);
        }
    }
}

/// Check line-of-sight between two points (for monster awareness).
/// Casts a single Bresenham ray — does NOT modify any FOV state.
pub fn can_see(ox: u8, oy: u8, tx: u8, ty: u8, radius: u8, map: &MicroMap) -> bool {
    // Chebyshev distance check
    let dx = tx.abs_diff(ox);
    let dy = ty.abs_diff(oy);
    let dist = if dx > dy { dx } else { dy };
    if dist > radius {
        return false;
    }

    let mut x = ox as i8;
    let mut y = oy as i8;
    let target_x = tx as i8;
    let target_y = ty as i8;

    let ddx = if target_x > x {
        target_x - x
    } else {
        x - target_x
    };
    let ddy = if target_y > y {
        target_y - y
    } else {
        y - target_y
    };
    let sx: i8 = if target_x > x { 1 } else { -1 };
    let sy: i8 = if target_y > y { 1 } else { -1 };
    let mut err = ddx - ddy;

    loop {
        if x == target_x && y == target_y {
            return true;
        }

        let e2 = err * 2;
        if e2 > -ddy {
            err -= ddy;
            x += sx;
        }
        if e2 < ddx {
            err += ddx;
            y += sy;
        }

        let ux = x as u8;
        let uy = y as u8;
        if !MicroMap::in_bounds(ux, uy) {
            return false;
        }
        if map.tile_at(ux, uy) == TILE_WALL {
            return x == target_x && y == target_y;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tier_micro::prng::LfsrRng16;

    fn make_test_map() -> MicroMap {
        let mut rng = LfsrRng16::new(42);
        let mut map = MicroMap::new();
        map.generate(&mut rng);
        map
    }

    #[test]
    fn origin_always_visible() {
        let map = make_test_map();
        let mut fov = MicroFov::new();
        fov.compute_fov(map.rooms[0].cx(), map.rooms[0].cy(), &map);
        assert!(fov.is_visible(map.rooms[0].cx(), map.rooms[0].cy()));
    }

    #[test]
    fn explored_persists_across_recompute() {
        let map = make_test_map();
        let mut fov = MicroFov::new();

        // Compute from first room
        let (x1, y1) = (map.rooms[0].cx(), map.rooms[0].cy());
        fov.compute_fov(x1, y1, &map);
        assert!(fov.is_explored(x1, y1));

        // Recompute from different position — origin should still be explored
        if map.room_count > 1 {
            let (x2, y2) = (map.rooms[1].cx(), map.rooms[1].cy());
            fov.compute_fov(x2, y2, &map);
            assert!(fov.is_explored(x1, y1), "explored should persist");
        }
    }

    #[test]
    fn can_see_same_point() {
        let map = make_test_map();
        let cx = map.rooms[0].cx();
        let cy = map.rooms[0].cy();
        assert!(can_see(cx, cy, cx, cy, FOV_RADIUS, &map));
    }

    #[test]
    fn can_see_respects_radius() {
        let map = make_test_map();
        let cx = map.rooms[0].cx();
        let cy = map.rooms[0].cy();
        // A point far beyond radius should not be visible
        let far_x = cx.wrapping_add(FOV_RADIUS + 5);
        let far_y = cy;
        if MicroMap::in_bounds(far_x, far_y) {
            assert!(!can_see(cx, cy, far_x, far_y, FOV_RADIUS, &map));
        }
    }

    #[test]
    fn can_see_through_open_space() {
        // Build a small all-floor map to test open LOS
        let mut map = MicroMap::new();
        for y in 5..15 {
            for x in 5..15 {
                map.tiles[(y as usize) * MAP_WIDTH as usize + x as usize] =
                    super::super::map::TILE_FLOOR;
            }
        }
        assert!(can_see(10, 10, 12, 12, 6, &map));
    }

    #[test]
    fn can_see_blocked_by_wall() {
        // Open area with a wall blocking LOS
        let mut map = MicroMap::new();
        for y in 5..15 {
            for x in 5..15 {
                map.tiles[(y as usize) * MAP_WIDTH as usize + x as usize] =
                    super::super::map::TILE_FLOOR;
            }
        }
        // Place wall between (8,10) and (12,10)
        map.tiles[10usize * MAP_WIDTH as usize + 10] = TILE_WALL;
        assert!(!can_see(8, 10, 12, 10, 6, &map));
    }

    #[test]
    fn out_of_bounds_not_visible() {
        let fov = MicroFov::new();
        assert!(!fov.is_visible(255, 255));
        assert!(!fov.is_explored(MAP_WIDTH, 0));
    }

    #[test]
    fn perimeter_points_within_radius() {
        for &(dx, dy) in PERIMETER.iter() {
            let dist = dx.unsigned_abs().max(dy.unsigned_abs());
            assert!(
                dist <= FOV_RADIUS,
                "perimeter point ({dx},{dy}) has Chebyshev distance {dist} > {FOV_RADIUS}"
            );
        }
    }
}
