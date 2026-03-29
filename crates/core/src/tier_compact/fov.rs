//! Field of view — iterative shadowcasting for the compact tier (GBA).
//!
//! Same octant-based algorithm as the micro tier, simplified for ARM7:
//! - i32 slopes and coordinates (native word size, no overflow concerns)
//! - Direct multiply for slope comparison (hardware multiplier)
//! - No quarter-square table or BIT lookup (barrel shifter handles shifts)

use super::map::{CompactMap, TILE_FLOOR};
use super::types::*;

/// Maximum iterative stack depth for shadowcasting sub-wedges.
const MAX_STACK: usize = 16;

/// Octant transform multipliers for shadowcasting.
const OCT_XX: [i32; 8] = [1, 0, 0, -1, -1, 0, 0, 1];
const OCT_XY: [i32; 8] = [0, 1, -1, 0, 0, -1, 1, 0];
const OCT_YX: [i32; 8] = [0, 1, 1, 0, 0, -1, -1, 0];
const OCT_YY: [i32; 8] = [1, 0, 0, 1, -1, 0, 0, -1];

/// One pending scan wedge for iterative shadowcasting.
/// Slopes are rational numbers: slope = num / den (den always > 0).
#[derive(Copy, Clone)]
struct ScanJob {
    row: i32,
    start_num: i32,
    start_den: i32,
    end_num: i32,
    end_den: i32,
}

/// Returns true if slope a/a_den < b/b_den (both denominators positive).
fn slope_lt(a_num: i32, a_den: i32, b_num: i32, b_den: i32) -> bool {
    a_num * b_den < b_num * a_den
}

/// Returns true if slope a/a_den > b/b_den (both denominators positive).
fn slope_gt(a_num: i32, a_den: i32, b_num: i32, b_den: i32) -> bool {
    a_num * b_den > b_num * a_den
}

pub struct CompactFov {
    visible: [u8; BITFIELD_SIZE],
    explored: [u8; BITFIELD_SIZE],
    pub width: Coord,
    pub height: Coord,
}

impl CompactFov {
    pub fn new(width: Coord, height: Coord) -> Self {
        debug_assert!(width > 0 && height > 0);
        debug_assert!((width as usize) * (height as usize) <= MAP_SIZE);
        Self {
            visible: [0; BITFIELD_SIZE],
            explored: [0; BITFIELD_SIZE],
            width,
            height,
        }
    }

    pub fn visible_bytes(&self) -> &[u8; BITFIELD_SIZE] {
        &self.visible
    }

    pub fn explored_bytes(&self) -> &[u8; BITFIELD_SIZE] {
        &self.explored
    }

    pub fn explored_bytes_mut(&mut self) -> &mut [u8; BITFIELD_SIZE] {
        &mut self.explored
    }

    fn bit_idx(&self, x: Coord, y: Coord) -> (usize, u8) {
        let i = (y * self.width + x) as usize;
        (i >> 3, 1u8 << (i & 7))
    }

    pub fn is_visible(&self, x: Coord, y: Coord) -> bool {
        if x < 0 || x >= self.width || y < 0 || y >= self.height {
            return false;
        }
        let (byte, bit) = self.bit_idx(x, y);
        self.visible[byte] & bit != 0
    }

    pub fn is_explored(&self, x: Coord, y: Coord) -> bool {
        if x < 0 || x >= self.width || y < 0 || y >= self.height {
            return false;
        }
        let (byte, bit) = self.bit_idx(x, y);
        self.explored[byte] & bit != 0
    }

    pub fn explored_floor_count(&self, map: &CompactMap) -> u16 {
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

    fn mark_visible(&mut self, x: Coord, y: Coord) {
        let (byte, bit) = self.bit_idx(x, y);
        self.visible[byte] |= bit;
        self.explored[byte] |= bit;
    }

    fn clear_visible(&mut self) {
        let total_tiles = (self.height * self.width) as usize;
        let used = (total_tiles + 7) >> 3;
        self.visible[..used].fill(0);
    }

    /// Scan one octant using iterative shadowcasting with integer slopes.
    #[allow(clippy::too_many_arguments)]
    fn scan_octant(
        &mut self,
        ox: Coord,
        oy: Coord,
        radius: i32,
        map: &CompactMap,
        xx: i32,
        xy: i32,
        yx: i32,
        yy: i32,
        stack: &mut [ScanJob; MAX_STACK],
    ) {
        let mut sp: usize = 1;
        stack[0] = ScanJob {
            row: 1,
            start_num: 1,
            start_den: 1,
            end_num: 0,
            end_den: 1,
        };

        while sp > 0 {
            sp -= 1;
            let job = stack[sp];
            let mut start_num = job.start_num;
            let mut start_den = job.start_den;
            let end_num = job.end_num;
            let end_den = job.end_den;

            if slope_lt(start_num, start_den, end_num, end_den) {
                continue;
            }

            let mut next_start_num = start_num;
            let mut next_start_den = start_den;

            let mut j = job.row;
            'row_loop: while j <= radius {
                let dy = -j;
                let mut blocked = false;

                let j2 = j * 2;
                let l_den = j2 - 1;
                let r_den = j2 + 1;

                let mut dx = -j;
                while dx <= 0 {
                    let dx2 = dx * 2;
                    let l_num = 1 - dx2;
                    let r_num = -1 - dx2;

                    if slope_lt(start_num, start_den, r_num, r_den) {
                        dx += 1;
                        continue;
                    }
                    if slope_gt(end_num, end_den, l_num, l_den) {
                        break;
                    }

                    let map_x = ox + dx * xx + dy * xy;
                    let map_y = oy + dx * yx + dy * yy;

                    let in_bounds =
                        map_x >= 0 && map_x < self.width && map_y >= 0 && map_y < self.height;

                    if in_bounds {
                        self.mark_visible(map_x, map_y);
                    }

                    let tile_blocks = !in_bounds || map.tile_at(map_x, map_y) < TILE_FLOOR;

                    if blocked {
                        if tile_blocks {
                            next_start_num = r_num;
                            next_start_den = r_den;
                        } else {
                            blocked = false;
                            start_num = next_start_num;
                            start_den = next_start_den;
                        }
                    } else if tile_blocks && j < radius {
                        blocked = true;
                        if sp < MAX_STACK {
                            stack[sp] = ScanJob {
                                row: j + 1,
                                start_num,
                                start_den,
                                end_num: l_num,
                                end_den: l_den,
                            };
                            sp += 1;
                        }
                        next_start_num = r_num;
                        next_start_den = r_den;
                    }

                    dx += 1;
                }

                if blocked {
                    break 'row_loop;
                }

                j += 1;
            }
        }
    }

    /// Compute field of view from (ox, oy) with given radius. Clears previous visible set.
    pub fn compute_fov(&mut self, ox: Coord, oy: Coord, radius: u8, map: &CompactMap) {
        self.clear_visible();
        self.mark_visible(ox, oy);

        let r = radius as i32;
        let mut stack = [ScanJob {
            row: 0,
            start_num: 0,
            start_den: 1,
            end_num: 0,
            end_den: 1,
        }; MAX_STACK];

        for octant in 0..8usize {
            self.scan_octant(
                ox,
                oy,
                r,
                map,
                OCT_XX[octant],
                OCT_XY[octant],
                OCT_YX[octant],
                OCT_YY[octant],
                &mut stack,
            );
        }
    }
}

/// Check line-of-sight between two points using Bresenham's algorithm.
/// Does NOT modify any FOV state. Used for monster awareness checks.
pub fn can_see(ox: Coord, oy: Coord, tx: Coord, ty: Coord, radius: u8, map: &CompactMap) -> bool {
    // Chebyshev distance check
    let dx = (tx - ox).abs();
    let dy = (ty - oy).abs();
    let dist = dx.max(dy);
    if dist > radius as Coord {
        return false;
    }

    let mut x = ox;
    let mut y = oy;

    let ddx = dx;
    let ddy = dy;
    let sx: Coord = if tx > x { 1 } else { -1 };
    let sy: Coord = if ty > y { 1 } else { -1 };
    let mut err = ddx - ddy;

    loop {
        if x == tx && y == ty {
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

        if !map.in_bounds(x, y) {
            return false;
        }
        if map.tile_at(x, y) < TILE_FLOOR {
            return x == tx && y == ty;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::balance;
    use crate::tier_compact::prng::LfsrRng32;

    const R: u8 = balance::FOV_RADIUS;

    fn make_test_map() -> CompactMap {
        let mut rng = LfsrRng32::new(42);
        let mut map = CompactMap::new(MAP_WIDTH, MAP_HEIGHT);
        map.generate(&mut rng);
        map
    }

    #[test]
    fn origin_always_visible() {
        let map = make_test_map();
        let mut fov = CompactFov::new(map.width, map.height);
        let cx = map.rooms[0].cx();
        let cy = map.rooms[0].cy();
        fov.compute_fov(cx, cy, R, &map);
        assert!(fov.is_visible(cx, cy));
    }

    #[test]
    fn explored_persists_across_recompute() {
        let map = make_test_map();
        let mut fov = CompactFov::new(map.width, map.height);

        let (x1, y1) = (map.rooms[0].cx(), map.rooms[0].cy());
        fov.compute_fov(x1, y1, R, &map);
        assert!(fov.is_explored(x1, y1));

        if map.room_count > 1 {
            let (x2, y2) = (map.rooms[1].cx(), map.rooms[1].cy());
            fov.compute_fov(x2, y2, R, &map);
            assert!(fov.is_explored(x1, y1), "explored should persist");
        }
    }

    #[test]
    fn can_see_same_point() {
        let map = make_test_map();
        let cx = map.rooms[0].cx();
        let cy = map.rooms[0].cy();
        assert!(can_see(cx, cy, cx, cy, R, &map));
    }

    #[test]
    fn can_see_respects_radius() {
        let map = make_test_map();
        let cx = map.rooms[0].cx();
        let cy = map.rooms[0].cy();
        let far_x = cx + R as Coord + 5;
        if map.in_bounds(far_x, cy) {
            assert!(!can_see(cx, cy, far_x, cy, R, &map));
        }
    }

    #[test]
    fn can_see_through_open_space() {
        let mut map = CompactMap::new(MAP_WIDTH, MAP_HEIGHT);
        for y in 5..15 {
            for x in 5..15 {
                map.set_tile(x, y, TILE_FLOOR);
            }
        }
        assert!(can_see(10, 10, 12, 12, 6, &map));
    }

    #[test]
    fn can_see_blocked_by_wall() {
        let mut map = CompactMap::new(MAP_WIDTH, MAP_HEIGHT);
        for y in 5..15 {
            for x in 5..15 {
                map.set_tile(x, y, TILE_FLOOR);
            }
        }
        use super::super::map::TILE_WALL;
        map.set_tile(10, 10, TILE_WALL);
        assert!(!can_see(8, 10, 12, 10, 6, &map));
    }

    #[test]
    fn out_of_bounds_not_visible() {
        let fov = CompactFov::new(MAP_WIDTH, MAP_HEIGHT);
        assert!(!fov.is_visible(-1, -1));
        assert!(!fov.is_visible(MAP_WIDTH, 0));
        assert!(!fov.is_explored(0, MAP_HEIGHT));
    }

    #[test]
    fn open_room_full_coverage() {
        let mut map = CompactMap::new(MAP_WIDTH, MAP_HEIGHT);
        let cx: Coord = 20;
        let cy: Coord = 20;
        let r = R as Coord;
        for y in (cy - r - 2)..=(cy + r + 2) {
            for x in (cx - r - 2)..=(cx + r + 2) {
                if map.in_bounds(x, y) {
                    map.set_tile(x, y, TILE_FLOOR);
                }
            }
        }

        let mut fov = CompactFov::new(map.width, map.height);
        fov.compute_fov(cx, cy, R, &map);

        for dy in -r..=r {
            for dx in -r..=r {
                let dist = dx.abs().max(dy.abs());
                if dist > r {
                    continue;
                }
                let x = cx + dx;
                let y = cy + dy;
                assert!(
                    fov.is_visible(x, y),
                    "tile ({x},{y}) at Chebyshev distance {dist} should be visible in open room",
                );
            }
        }
    }

    #[test]
    fn wall_blocks_behind() {
        let mut map = CompactMap::new(MAP_WIDTH, MAP_HEIGHT);
        let cx: Coord = 20;
        let cy: Coord = 20;
        for y in 15..26 {
            for x in 15..26 {
                map.set_tile(x, y, TILE_FLOOR);
            }
        }
        use super::super::map::TILE_WALL;
        map.set_tile(23, 20, TILE_WALL);

        let mut fov = CompactFov::new(map.width, map.height);
        fov.compute_fov(cx, cy, R, &map);

        assert!(fov.is_visible(23, 20), "wall should be visible");
        assert!(
            !fov.is_visible(24, 20),
            "tile behind wall should not be visible"
        );
    }

    #[test]
    fn corridor_visibility() {
        let mut map = CompactMap::new(MAP_WIDTH, MAP_HEIGHT);
        for x in 10..31 {
            map.set_tile(x, 20, TILE_FLOOR);
        }

        let mut fov = CompactFov::new(map.width, map.height);
        fov.compute_fov(20, 20, R, &map);

        assert!(fov.is_visible(20, 20));
        assert!(fov.is_visible(20 + R as Coord, 20));
        assert!(fov.is_visible(20 - R as Coord, 20));
        assert!(
            fov.is_visible(20, 19),
            "wall adjacent to corridor should be visible"
        );
        assert!(
            !fov.is_visible(20, 18),
            "tile behind corridor wall should not be visible"
        );
    }

    #[test]
    fn explored_floor_count_accuracy() {
        let map = make_test_map();
        let mut fov = CompactFov::new(map.width, map.height);
        fov.compute_fov(map.rooms[0].cx(), map.rooms[0].cy(), R, &map);

        let mut manual: u16 = 0;
        for y in 0..map.height {
            for x in 0..map.width {
                if fov.is_explored(x, y) && map.is_walkable(x, y) {
                    manual += 1;
                }
            }
        }
        assert_eq!(fov.explored_floor_count(&map), manual);
    }
}
