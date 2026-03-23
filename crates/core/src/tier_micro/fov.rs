//! Field of view — iterative shadowcasting for the micro tier.
//!
//! Uses octant-based shadowcasting with integer rational slopes and an
//! explicit stack (no recursion, no alloc). Visits every tile in the FOV
//! area exactly once per octant, eliminating the coverage gaps that
//! Bresenham raycasting had between adjacent rays.
//!
//! The `can_see` free function still uses Bresenham for fast point-to-point
//! line-of-sight checks (O(R) vs O(R²) for full shadowcasting).

use super::map::{MicroMap, TILE_FLOOR};
use super::types::*;
use crate::rules::balance;

pub const FOV_RADIUS: u8 = balance::MICRO_FOV_RADIUS;

/// Maximum iterative stack depth for shadowcasting sub-wedges.
const MAX_STACK: usize = 16;

/// Octant transform multipliers for shadowcasting.
/// Maps abstract (dx, dy) to map (x, y) offsets for each of 8 octants.
const OCT_XX: [i8; 8] = [1, 0, 0, -1, -1, 0, 0, 1];
const OCT_XY: [i8; 8] = [0, 1, -1, 0, 0, -1, 1, 0];
const OCT_YX: [i8; 8] = [0, 1, 1, 0, 0, -1, -1, 0];
const OCT_YY: [i8; 8] = [1, 0, 0, 1, -1, 0, 0, -1];

/// One pending scan wedge for iterative shadowcasting.
/// Slopes are rational numbers: slope = num / den (den always > 0).
#[derive(Copy, Clone)]
struct ScanJob {
    row: u8,
    start_num: i8,
    start_den: i8,
    end_num: i8,
    end_den: i8,
}

// ---------------------------------------------------------------------------
// Quarter-square multiplication table for fast slope comparison.
//
// On 6502, i16 multiply (__mulhi3) costs ~50-60 cycles. Each slope comparison
// does two of them. The quarter-square identity replaces each multiply with
// two table lookups and a subtract: a * b = QS[a+b] - QS[|a-b|].
//
// Table size = 2 * (1 + 2*MAX_FOV_RADIUS) + 1 entries of u16.
// At MAX_FOV_RADIUS=16: 67 entries = 134 bytes.
// ---------------------------------------------------------------------------

/// Max slope operand value: 1 + 2 * MAX_FOV_RADIUS (from tile edge formulas).
const MAX_SLOPE_VAL: usize = 1 + 2 * balance::MAX_FOV_RADIUS as usize;

/// Quarter-square table: QS[x] = floor(x² / 4).
/// Used for fast multiply: a * b = QS[a+b] - QS[|a-b|].
const QS: [u16; 2 * MAX_SLOPE_VAL + 1] = {
    let len = 2 * MAX_SLOPE_VAL + 1;
    let mut t = [0u16; 2 * MAX_SLOPE_VAL + 1];
    let mut i = 0;
    while i < len {
        t[i] = ((i * i) / 4) as u16;
        i += 1;
    }
    t
};

const _: () = assert!(
    balance::FOV_RADIUS <= balance::MAX_FOV_RADIUS,
    "FOV_RADIUS exceeds MAX_FOV_RADIUS — increase MAX_FOV_RADIUS in rules/balance.rs"
);

/// Fast unsigned multiply via quarter-square lookup.
/// Valid for a, b in 0..=MAX_SLOPE_VAL.
fn fast_mul(a: u8, b: u8) -> u16 {
    let sum = a as usize + b as usize;
    let diff = if a >= b {
        (a - b) as usize
    } else {
        (b - a) as usize
    };
    QS[sum] - QS[diff]
}

/// Signed cross-product for slope comparison: num * den where den > 0.
/// Handles negative numerators (only -1 in practice) via branch + negate.
fn signed_cross(num: i8, den: i8) -> i16 {
    if num >= 0 {
        fast_mul(num as u8, den as u8) as i16
    } else {
        -(fast_mul((-num) as u8, den as u8) as i16)
    }
}

/// Returns true if slope a/a_den < b/b_den (both denominators positive).
fn slope_lt(a_num: i8, a_den: i8, b_num: i8, b_den: i8) -> bool {
    signed_cross(a_num, b_den) < signed_cross(b_num, a_den)
}

/// Returns true if slope a/a_den > b/b_den (both denominators positive).
fn slope_gt(a_num: i8, a_den: i8, b_num: i8, b_den: i8) -> bool {
    signed_cross(a_num, b_den) > signed_cross(b_num, a_den)
}

/// Apply an octant transform multiplier (-1, 0, or 1) to a value.
///
/// Replaces `val * mult` which generates a `__mulqi3` call on 6502 (~27
/// cycles).  A two-branch conditional is ~6-10 cycles.
fn apply_octant(val: i8, mult: i8) -> i8 {
    if mult == 1 {
        val
    } else if mult == -1 {
        -val
    } else {
        0
    }
}

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

    /// Raw visible bitfield bytes (read-only). Used for frame-to-frame
    /// diffing and direct row-major rendering on constrained platforms.
    pub fn visible_bytes(&self) -> &[u8; MAX_BITFIELD_SIZE] {
        &self.visible
    }

    /// Raw explored bitfield bytes (read-only). Used for direct row-major
    /// rendering on constrained platforms.
    pub fn explored_bytes(&self) -> &[u8; MAX_BITFIELD_SIZE] {
        &self.explored
    }

    /// Mutable access to explored bitfield bytes. Used by save/load to
    /// restore the explored state without recomputing.
    pub fn explored_bytes_mut(&mut self) -> &mut [u8; MAX_BITFIELD_SIZE] {
        &mut self.explored
    }

    fn bit_idx(&self, x: u8, y: u8) -> (usize, u8) {
        let i = row_col_idx(y, x, self.width);
        (i >> 3, BIT[i & 7])
    }

    pub fn is_visible(&self, x: u8, y: u8) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let (byte, bit) = self.bit_idx(x, y);
        // Safety: bounds checked above, so byte < MAX_BITFIELD_SIZE.
        unsafe { *self.visible.get_unchecked(byte) & bit != 0 }
    }

    pub fn is_explored(&self, x: u8, y: u8) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let (byte, bit) = self.bit_idx(x, y);
        // Safety: bounds checked above, so byte < MAX_BITFIELD_SIZE.
        unsafe { *self.explored.get_unchecked(byte) & bit != 0 }
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
        // Safety: callers ensure x < width && y < height, so byte < MAX_BITFIELD_SIZE.
        unsafe {
            *self.visible.get_unchecked_mut(byte) |= bit;
            *self.explored.get_unchecked_mut(byte) |= bit;
        }
    }

    fn clear_visible(&mut self) {
        // Only clear bytes covering the actual map dimensions instead of the
        // full MAX_BITFIELD_SIZE.  For 64×48 maps this zeros 384 bytes vs 600,
        // saving ~36% of the __memset cost per FOV call.
        let total_tiles = row_col_idx(self.height, 0, self.width);
        let used = (total_tiles + 7) >> 3;
        for i in 0..used {
            // Safety: used <= MAX_BITFIELD_SIZE by construction.
            unsafe {
                *self.visible.get_unchecked_mut(i) = 0;
            }
        }
    }

    /// Scan one octant using iterative shadowcasting with integer slopes.
    ///
    /// Scans row by row outward from the origin, tracking start/end slope
    /// boundaries. When a wall is found, the sub-wedge beyond it is pushed
    /// to an explicit stack for later processing.
    #[allow(clippy::too_many_arguments)]
    fn scan_octant(
        &mut self,
        ox: u8,
        oy: u8,
        map: &MicroMap,
        xx: i8,
        xy: i8,
        yx: i8,
        yy: i8,
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

        let w = self.width as i8;
        let h = self.height as i8;

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

            // Manual while loop avoids RangeInclusive::next iterator overhead.
            let mut j = job.row;
            'row_loop: while j <= FOV_RADIUS {
                let dy = -(j as i8);
                let mut blocked = false;

                // Pre-compute row-invariant denominator terms outside the
                // per-tile loop.  Uses self-add (j_i8 + j_i8) instead of
                // `2 * j` to avoid __mulqi3 on 6502.
                let j_i8 = j as i8;
                let j2 = j_i8 + j_i8;
                let l_den: i8 = j2 - 1;
                let r_den: i8 = 1 + j2;

                // Scan from dx=-j to dx=0 (matching the standard tier's
                // top-to-bottom order for correct slope boundary tracking).
                let mut dx = -(j as i8);
                while dx <= 0 {
                    // Tile slope boundaries (positive-denominator convention).
                    // Uses self-add for numerators to avoid __mulqi3.
                    let dx2 = dx + dx;
                    let l_num: i8 = 1 - dx2;
                    let r_num: i8 = -1 - dx2;

                    if slope_lt(start_num, start_den, r_num, r_den) {
                        dx += 1;
                        continue;
                    }
                    if slope_gt(end_num, end_den, l_num, l_den) {
                        break;
                    }

                    // Octant coordinate transform.  The multipliers xx/xy/yx/yy
                    // are always -1, 0, or 1.  Branching avoids __mulqi3 (~27
                    // cycles) for what is effectively a conditional negate.
                    let map_x = (ox as i8) + apply_octant(dx, xx) + apply_octant(dy, xy);
                    let map_y = (oy as i8) + apply_octant(dx, yx) + apply_octant(dy, yy);

                    let in_bounds = map_x >= 0 && map_x < w && map_y >= 0 && map_y < h;

                    if in_bounds {
                        self.mark_visible(map_x as u8, map_y as u8);
                    }

                    let tile_blocks =
                        !in_bounds || map.tile_at(map_x as u8, map_y as u8) < TILE_FLOOR;

                    if blocked {
                        if tile_blocks {
                            next_start_num = r_num;
                            next_start_den = r_den;
                        } else {
                            blocked = false;
                            start_num = next_start_num;
                            start_den = next_start_den;
                        }
                    } else if tile_blocks && j < FOV_RADIUS {
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

    /// Compute field of view from (ox, oy). Clears previous visible set.
    #[cfg_attr(feature = "c64-overlay", unsafe(link_section = ".overlay"))]
    pub fn compute_fov(&mut self, ox: u8, oy: u8, map: &MicroMap) {
        self.clear_visible();
        self.mark_visible(ox, oy);

        // Allocate the scan stack once and reuse across all 8 octants.
        // Each octant only writes stack[0] + push/pop; stale data in
        // higher slots is never read.  This saves 7 redundant 80-byte
        // array initializations per compute_fov call.
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
        if map.tile_at(ux, uy) < TILE_FLOOR {
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
                map.set_tile(x, y, TILE_FLOOR);
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
                map.set_tile(x, y, TILE_FLOOR);
            }
        }
        // Place wall between (8,10) and (12,10)
        map.set_tile(10, 10, super::super::map::TILE_WALL);
        assert!(!can_see(8, 10, 12, 10, 6, &map));
    }

    #[test]
    fn out_of_bounds_not_visible() {
        let fov = MicroFov::new_default();
        assert!(!fov.is_visible(255, 255));
        assert!(!fov.is_explored(DEFAULT_MAP_WIDTH, 0));
    }

    #[test]
    fn open_room_full_coverage() {
        // Every floor tile within FOV_RADIUS must be visible in an open room.
        // This is the bug the Bresenham algorithm had.
        let mut map = MicroMap::new_default();
        let cx: u8 = 20;
        let cy: u8 = 20;
        let r = FOV_RADIUS;
        for y in (cy - r - 2)..=(cy + r + 2) {
            for x in (cx - r - 2)..=(cx + r + 2) {
                if map.in_bounds(x, y) {
                    map.set_tile(x, y, TILE_FLOOR);
                }
            }
        }

        let mut fov = MicroFov::new(map.width, map.height);
        fov.compute_fov(cx, cy, &map);

        for dy in -(r as i8)..=(r as i8) {
            for dx in -(r as i8)..=(r as i8) {
                let dist = (dx.unsigned_abs()).max(dy.unsigned_abs());
                if dist > r {
                    continue;
                }
                let x = (cx as i8 + dx) as u8;
                let y = (cy as i8 + dy) as u8;
                assert!(
                    fov.is_visible(x, y),
                    "tile ({},{}) at Chebyshev distance {} should be visible in open room",
                    x,
                    y,
                    dist
                );
            }
        }
    }

    #[test]
    fn wall_blocks_behind() {
        let mut map = MicroMap::new_default();
        let cx: u8 = 20;
        let cy: u8 = 20;
        for y in 15..26 {
            for x in 15..26 {
                map.set_tile(x, y, TILE_FLOOR);
            }
        }
        // Wall 3 tiles east of origin
        map.set_tile(23, 20, super::super::map::TILE_WALL);

        let mut fov = MicroFov::new(map.width, map.height);
        fov.compute_fov(cx, cy, &map);

        assert!(fov.is_visible(23, 20), "wall should be visible");
        assert!(
            !fov.is_visible(24, 20),
            "tile behind wall should not be visible"
        );
    }

    #[test]
    fn corridor_visibility() {
        let mut map = MicroMap::new_default();
        // Horizontal corridor at y=20, from x=10 to x=30
        for x in 10..31 {
            map.set_tile(x, 20, TILE_FLOOR);
        }

        let mut fov = MicroFov::new(map.width, map.height);
        fov.compute_fov(20, 20, &map);

        assert!(fov.is_visible(20, 20));
        // Along corridor within radius
        assert!(fov.is_visible(20 + FOV_RADIUS, 20));
        assert!(fov.is_visible(20 - FOV_RADIUS, 20));
        // Wall adjacent to corridor should be visible
        assert!(
            fov.is_visible(20, 19),
            "wall adjacent to corridor should be visible"
        );
        // Behind corridor wall should not be visible
        assert!(
            !fov.is_visible(20, 18),
            "tile behind corridor wall should not be visible"
        );
    }
}
