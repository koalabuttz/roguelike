//! Field of view — Bresenham raycasting for the micro tier.
//!
//! Casts rays from the player to the perimeter of a square (Chebyshev radius),
//! using Bresenham's line algorithm (integer-only). Each ray marks tiles
//! visible until it hits a wall.
//!
//! The perimeter is computed at runtime from `FOV_RADIUS`, so any radius works.

use super::map::{MicroMap, TILE_WALL};
use super::types::*;
use crate::rules::balance;

pub const FOV_RADIUS: u8 = balance::MICRO_FOV_RADIUS;

pub struct MicroFov {
    visible: [u8; MAX_BITFIELD_SIZE],
    explored: [u8; MAX_BITFIELD_SIZE],
    pub width: u8,
    pub height: u8,
}

impl MicroFov {
    pub fn new(width: u8, height: u8) -> Self {
        Self {
            visible: [0; MAX_BITFIELD_SIZE],
            explored: [0; MAX_BITFIELD_SIZE],
            width,
            height,
        }
    }

    /// Create FOV with C64 default dimensions (64×48).
    pub fn new_default() -> Self {
        Self::new(DEFAULT_MAP_WIDTH, DEFAULT_MAP_HEIGHT)
    }

    fn bit_idx(&self, x: u8, y: u8) -> (usize, u8) {
        let i = (y as usize) * (self.width as usize) + (x as usize);
        (i / 8, 1u8 << (i % 8))
    }

    pub fn is_visible(&self, x: u8, y: u8) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let (byte, bit) = self.bit_idx(x, y);
        self.visible[byte] & bit != 0
    }

    pub fn is_explored(&self, x: u8, y: u8) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let (byte, bit) = self.bit_idx(x, y);
        self.explored[byte] & bit != 0
    }

    pub fn explored_floor_count(&self, map: &MicroMap) -> u16 {
        let mut count: u16 = 0;
        for y in 0..self.height {
            for x in 0..self.width {
                if self.is_explored(x, y) && map.is_walkable(x, y) {
                    count += 1;
                }
            }
        }
        count
    }

    fn mark_visible(&mut self, x: u8, y: u8) {
        let (byte, bit) = self.bit_idx(x, y);
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

            if !map.in_bounds(ux, uy) {
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

        let r = FOV_RADIUS as i8;
        for dy in -r..=r {
            for dx in -r..=r {
                // Only cast to perimeter points (Chebyshev distance == radius).
                if dx.unsigned_abs().max(dy.unsigned_abs()) != FOV_RADIUS {
                    continue;
                }
                let tx = (ox as i8).wrapping_add(dx);
                let ty = (oy as i8).wrapping_add(dy);
                if tx < 0 || ty < 0 || tx >= self.width as i8 || ty >= self.height as i8 {
                    continue;
                }
                self.cast_ray(ox, oy, tx as u8, ty as u8, map);
            }
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
        if !map.in_bounds(ux, uy) {
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
        let mut map = MicroMap::new_default();
        map.generate(&mut rng);
        map
    }

    #[test]
    fn origin_always_visible() {
        let map = make_test_map();
        let mut fov = MicroFov::new(map.width, map.height);
        fov.compute_fov(map.rooms[0].cx(), map.rooms[0].cy(), &map);
        assert!(fov.is_visible(map.rooms[0].cx(), map.rooms[0].cy()));
    }

    #[test]
    fn explored_persists_across_recompute() {
        let map = make_test_map();
        let mut fov = MicroFov::new(map.width, map.height);

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
        if map.in_bounds(far_x, far_y) {
            assert!(!can_see(cx, cy, far_x, far_y, FOV_RADIUS, &map));
        }
    }

    #[test]
    fn can_see_through_open_space() {
        // Build a small all-floor map to test open LOS
        let mut map = MicroMap::new_default();
        for y in 5..15 {
            for x in 5..15 {
                map.tiles[map.idx(x, y)] = super::super::map::TILE_FLOOR;
            }
        }
        assert!(can_see(10, 10, 12, 12, 6, &map));
    }

    #[test]
    fn can_see_blocked_by_wall() {
        // Open area with a wall blocking LOS
        let mut map = MicroMap::new_default();
        for y in 5..15 {
            for x in 5..15 {
                map.tiles[map.idx(x, y)] = super::super::map::TILE_FLOOR;
            }
        }
        // Place wall between (8,10) and (12,10)
        map.tiles[map.idx(10, 10)] = TILE_WALL;
        assert!(!can_see(8, 10, 12, 10, 6, &map));
    }

    #[test]
    fn out_of_bounds_not_visible() {
        let fov = MicroFov::new_default();
        assert!(!fov.is_visible(255, 255));
        assert!(!fov.is_explored(DEFAULT_MAP_WIDTH, 0));
    }

    #[test]
    fn computed_perimeter_count() {
        // For radius R, Chebyshev perimeter has 8*R points.
        let r = FOV_RADIUS as i8;
        let mut count = 0u32;
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.unsigned_abs().max(dy.unsigned_abs()) == FOV_RADIUS {
                    count += 1;
                }
            }
        }
        assert_eq!(count, 8 * FOV_RADIUS as u32);
    }
}
