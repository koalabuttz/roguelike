use roguelike_core::rules::color::GameColor;
use roguelike_core::rules::game_view::GameView;
use roguelike_core::rules::tiles::{self, TileKind};

use crate::color_map::game_color_to_rgb555;
use crate::math::{Fixed16, Vec3};

/// Height of wall blocks in world units.
pub const WALL_HEIGHT: Fixed16 = Fixed16::ONE;

/// Surface normals for axis-aligned faces.
const NORMAL_UP: Vec3 = Vec3::new(Fixed16::ZERO, Fixed16::ONE, Fixed16::ZERO);
const NORMAL_SOUTH: Vec3 = Vec3::new(Fixed16::ZERO, Fixed16::ZERO, Fixed16::ONE);
const NORMAL_NORTH: Vec3 = Vec3::new(Fixed16::ZERO, Fixed16::ZERO, Fixed16::NEG_ONE);
const NORMAL_EAST: Vec3 = Vec3::new(Fixed16::ONE, Fixed16::ZERO, Fixed16::ZERO);
const NORMAL_WEST: Vec3 = Vec3::new(Fixed16::NEG_ONE, Fixed16::ZERO, Fixed16::ZERO);

/// Consumer of triangles produced by geometry generation.
///
/// The streaming pattern: geometry is generated, emitted into the sink,
/// transformed, rasterized, and forgotten — zero intermediate allocation.
pub trait TriangleSink {
    /// Receive a triangle with its surface normal and a pre-computed RGB555 color.
    /// The normal is used for per-vertex lighting (Lambert's cosine law).
    fn emit(&mut self, v0: Vec3, v1: Vec3, v2: Vec3, normal: Vec3, color: u16);
}

/// Subdivision level per axis. Each tile face becomes SUBDIV×SUBDIV quads.
/// 1 = no subdivision (original), 2 = 4 quads/face, 4 = 16 quads/face.
/// Higher values reduce Gouraud faceting by sampling the non-linear lighting
/// (Lambert × inverse-square) at more vertices.
const SUBDIV: i32 = 1;

/// Emit a quad as two triangles with reversed winding.
///
/// Callers specify vertices in CCW order when viewed from the front face
/// (the natural convention). This function emits them in CW order to
/// compensate for the viewport y-flip in project_vertex, which negates
/// the screen-space cross product. CW-in-world → CCW-in-screen → front-facing.
#[inline]
fn emit_quad(
    sink: &mut dyn TriangleSink,
    v0: Vec3,
    v1: Vec3,
    v2: Vec3,
    v3: Vec3,
    normal: Vec3,
    color: u16,
) {
    sink.emit(v0, v2, v1, normal, color);
    sink.emit(v0, v3, v2, normal, color);
}

/// Linearly interpolate between two Vec3 at parameter t (0..SUBDIV maps to 0..1).
#[inline]
fn lerp(a: Vec3, b: Vec3, num: i32, den: i32) -> Vec3 {
    // a + (b - a) * num / den, using Fixed16 arithmetic
    let t_num = Fixed16::from_int(num);
    let t_den = Fixed16::from_int(den);
    let t = t_num / t_den;
    Vec3::new(
        a.x + (b.x - a.x) * t,
        a.y + (b.y - a.y) * t,
        a.z + (b.z - a.z) * t,
    )
}

/// Emit a subdivided quad as SUBDIV×SUBDIV sub-quads.
///
/// Corners v0..v3 are in CCW order when viewed from the front face:
/// ```text
///   v0 ------- v3
///   |           |
///   |           |
///   v1 ------- v2
/// ```
/// The quad is subdivided by bilinear interpolation: each sub-quad's corners
/// are lerped from the original 4 corners, giving Gouraud shading more
/// sample points for smoother lighting gradients.
fn emit_subdivided_quad(
    sink: &mut dyn TriangleSink,
    v0: Vec3,
    v1: Vec3,
    v2: Vec3,
    v3: Vec3,
    normal: Vec3,
    color: u16,
) {
    if SUBDIV <= 1 {
        emit_quad(sink, v0, v1, v2, v3, normal, color);
        return;
    }

    let n = SUBDIV;
    for row in 0..n {
        for col in 0..n {
            // Bilinear interpolation of the 4 corners for sub-quad (row, col).
            // Top edge: lerp(v0, v3) at col/n and (col+1)/n
            // Bottom edge: lerp(v1, v2) at col/n and (col+1)/n
            // Then lerp vertically between top and bottom edges.
            let top_l = lerp(v0, v3, col, n);
            let top_r = lerp(v0, v3, col + 1, n);
            let bot_l = lerp(v1, v2, col, n);
            let bot_r = lerp(v1, v2, col + 1, n);

            let sv0 = lerp(top_l, bot_l, row, n);
            let sv1 = lerp(top_l, bot_l, row + 1, n);
            let sv2 = lerp(top_r, bot_r, row + 1, n);
            let sv3 = lerp(top_r, bot_r, row, n);

            emit_quad(sink, sv0, sv1, sv2, sv3, normal, color);
        }
    }
}

/// Helper to create a Vec3 from grid coordinates at a given height.
#[inline]
fn gv(x: i32, y_height: Fixed16, z: i32) -> Vec3 {
    Vec3::new(Fixed16::from_int(x), y_height, Fixed16::from_int(z))
}

/// Emit a subdivided floor quad at y=0 for grid position (gx, gz).
fn emit_floor(sink: &mut dyn TriangleSink, gx: i32, gz: i32, color: u16) {
    let y = Fixed16::ZERO;
    let v0 = gv(gx, y, gz);
    let v1 = gv(gx, y, gz + 1);
    let v2 = gv(gx + 1, y, gz + 1);
    let v3 = gv(gx + 1, y, gz);
    emit_subdivided_quad(sink, v0, v1, v2, v3, NORMAL_UP, color);
}

/// Emit a subdivided wall top quad at y=WALL_HEIGHT for grid position (gx, gz).
fn emit_wall_top(sink: &mut dyn TriangleSink, gx: i32, gz: i32, color: u16) {
    let y = WALL_HEIGHT;
    let v0 = gv(gx, y, gz);
    let v1 = gv(gx, y, gz + 1);
    let v2 = gv(gx + 1, y, gz + 1);
    let v3 = gv(gx + 1, y, gz);
    emit_subdivided_quad(sink, v0, v1, v2, v3, NORMAL_UP, color);
}

/// Emit subdivided south-facing wall (at z=gz+1, facing +z direction).
fn emit_wall_south(sink: &mut dyn TriangleSink, gx: i32, gz: i32, color: u16) {
    let z = gz + 1;
    let v0 = gv(gx, WALL_HEIGHT, z);
    let v1 = gv(gx, Fixed16::ZERO, z);
    let v2 = gv(gx + 1, Fixed16::ZERO, z);
    let v3 = gv(gx + 1, WALL_HEIGHT, z);
    emit_subdivided_quad(sink, v0, v1, v2, v3, NORMAL_SOUTH, color);
}

/// Emit subdivided north-facing wall (at z=gz, facing -z direction).
fn emit_wall_north(sink: &mut dyn TriangleSink, gx: i32, gz: i32, color: u16) {
    let z = gz;
    let v0 = gv(gx + 1, WALL_HEIGHT, z);
    let v1 = gv(gx + 1, Fixed16::ZERO, z);
    let v2 = gv(gx, Fixed16::ZERO, z);
    let v3 = gv(gx, WALL_HEIGHT, z);
    emit_subdivided_quad(sink, v0, v1, v2, v3, NORMAL_NORTH, color);
}

/// Emit subdivided east-facing wall (at x=gx+1, facing +x direction).
fn emit_wall_east(sink: &mut dyn TriangleSink, gx: i32, gz: i32, color: u16) {
    let x = gx + 1;
    let v0 = gv(x, WALL_HEIGHT, gz + 1);
    let v1 = gv(x, Fixed16::ZERO, gz + 1);
    let v2 = gv(x, Fixed16::ZERO, gz);
    let v3 = gv(x, WALL_HEIGHT, gz);
    emit_subdivided_quad(sink, v0, v1, v2, v3, NORMAL_EAST, color);
}

/// Emit subdivided west-facing wall (at x=gx, facing -x direction).
fn emit_wall_west(sink: &mut dyn TriangleSink, gx: i32, gz: i32, color: u16) {
    let x = gx;
    let v0 = gv(x, WALL_HEIGHT, gz);
    let v1 = gv(x, Fixed16::ZERO, gz);
    let v2 = gv(x, Fixed16::ZERO, gz + 1);
    let v3 = gv(x, WALL_HEIGHT, gz + 1);
    emit_subdivided_quad(sink, v0, v1, v2, v3, NORMAL_WEST, color);
}

/// Check if a tile at (x, z) is a Structural wall.
fn is_structural(view: &dyn GameView, x: i32, z: i32) -> bool {
    let (w, h) = view.map_dims();
    if x < 0 || z < 0 || x >= w || z >= h {
        return false;
    }
    tiles::from_micro(view.tile_at(x, z)) == Some(TileKind::Structural)
}

/// Render radius² — tiles beyond this distance from the player are not rendered.
/// Set larger than FOV_RADIUS to provide a border of dark geometry beyond the FOV.
const RENDER_RADIUS_SQ: i32 = 100; // 10 tiles

/// Generate 3D triangles for all tiles within the render radius.
///
/// Renders ALL tiles near the player (not just FOV-visible ones).
/// The per-tile fog map in the scene module handles lighting:
/// visible tiles get distance-based falloff, non-visible tiles are fully dark.
/// This eliminates hard edges between lit geometry and void.
pub fn generate_map_geometry(view: &dyn GameView, sink: &mut dyn TriangleSink) {
    let (w, h) = view.map_dims();
    let (px, py) = view.player_xy();

    // Only iterate tiles within the render radius of the player
    let r = 10i32; // sqrt(RENDER_RADIUS_SQ)
    let min_x = (px - r).max(0);
    let max_x = (px + r).min(w - 1);
    let min_z = (py - r).max(0);
    let max_z = (py + r).min(h - 1);

    for gz in min_z..=max_z {
        for gx in min_x..=max_x {
            let dx = gx - px;
            let dz = gz - py;
            if dx * dx + dz * dz > RENDER_RADIUS_SQ {
                continue;
            }

            let tile = match tiles::from_micro(view.tile_at(gx, gz)) {
                Some(t) => t,
                None => continue,
            };

            match tile {
                TileKind::Wall => {
                    // Void tile — no geometry
                }
                TileKind::Floor | TileKind::StairsDown => {
                    // Use a brighter albedo than the 2D terminal color (DarkGrey).
                    // In 3D, the lighting system handles brightness — the surface
                    // color is the material reflectivity, not the final appearance.
                    let color = game_color_to_rgb555(GameColor::Grey);
                    emit_floor(sink, gx, gz, color);
                }
                TileKind::Structural => {
                    let color = game_color_to_rgb555(GameColor::White);
                    emit_wall_top(sink, gx, gz, color);

                    if !is_structural(view, gx, gz - 1) {
                        emit_wall_north(sink, gx, gz, color);
                    }
                    if !is_structural(view, gx, gz + 1) {
                        emit_wall_south(sink, gx, gz, color);
                    }
                    if !is_structural(view, gx - 1, gz) {
                        emit_wall_west(sink, gx, gz, color);
                    }
                    if !is_structural(view, gx + 1, gz) {
                        emit_wall_east(sink, gx, gz, color);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framebuffer::rgb555;

    /// Collects emitted triangles for inspection.
    struct VecSink {
        tris: Vec<(Vec3, Vec3, Vec3, Vec3, u16)>,
    }

    impl VecSink {
        fn new() -> Self {
            Self { tris: Vec::new() }
        }
    }

    impl TriangleSink for VecSink {
        fn emit(&mut self, v0: Vec3, v1: Vec3, v2: Vec3, normal: Vec3, color: u16) {
            self.tris.push((v0, v1, v2, normal, color));
        }
    }

    fn f(n: i32) -> Fixed16 {
        Fixed16::from_int(n)
    }

    /// Check that a triangle has CW winding when viewed from above (+y direction).
    /// We emit CW-from-above so that the viewport y-flip produces CCW-in-screen.
    /// The y-component of cross(v1-v0, v2-v0) should be negative.
    fn cross_y_negative(v0: Vec3, v1: Vec3, v2: Vec3) -> bool {
        let e1x = v1.x - v0.x;
        let e1z = v1.z - v0.z;
        let e2x = v2.x - v0.x;
        let e2z = v2.z - v0.z;
        let y_component = e1z * e2x - e1x * e2z;
        y_component < Fixed16::ZERO
    }

    /// Expected triangle count per face: SUBDIV² sub-quads × 2 tris each.
    const EXPECTED_TRIS: usize = (SUBDIV * SUBDIV * 2) as usize;

    #[test]
    fn floor_emits_subdivided_triangles() {
        let mut sink = VecSink::new();
        let color = rgb555(10, 10, 10);
        emit_floor(&mut sink, 3, 5, color);

        assert_eq!(
            sink.tris.len(),
            EXPECTED_TRIS,
            "floor should emit {EXPECTED_TRIS} triangles"
        );

        // All vertices at y=0
        for (v0, v1, v2, _, c) in &sink.tris {
            assert_eq!(v0.y, f(0));
            assert_eq!(v1.y, f(0));
            assert_eq!(v2.y, f(0));
            assert_eq!(*c, color);
        }
    }

    #[test]
    fn floor_winding_for_yflip() {
        let mut sink = VecSink::new();
        emit_floor(&mut sink, 0, 0, 0);

        // Triangles are CW-from-above → CCW-in-screen after viewport y-flip
        for (v0, v1, v2, _, _) in &sink.tris {
            assert!(
                cross_y_negative(*v0, *v1, *v2),
                "floor triangle should be CW from above (CCW after y-flip)"
            );
        }
    }

    #[test]
    fn wall_top_at_wall_height() {
        let mut sink = VecSink::new();
        emit_wall_top(&mut sink, 2, 3, 0);

        assert_eq!(sink.tris.len(), EXPECTED_TRIS);
        for (v0, v1, v2, _, _) in &sink.tris {
            assert_eq!(v0.y, WALL_HEIGHT);
            assert_eq!(v1.y, WALL_HEIGHT);
            assert_eq!(v2.y, WALL_HEIGHT);
        }
    }

    #[test]
    fn wall_top_winding_for_yflip() {
        let mut sink = VecSink::new();
        emit_wall_top(&mut sink, 0, 0, 0);

        for (v0, v1, v2, _, _) in &sink.tris {
            assert!(
                cross_y_negative(*v0, *v1, *v2),
                "wall top should be CW from above (CCW after y-flip)"
            );
        }
    }

    #[test]
    fn wall_south_vertices() {
        let mut sink = VecSink::new();
        emit_wall_south(&mut sink, 1, 2, 0);

        assert_eq!(sink.tris.len(), EXPECTED_TRIS);
        // All vertices should be at z = gz+1 = 3
        for (v0, v1, v2, _, _) in &sink.tris {
            assert_eq!(v0.z, f(3));
            assert_eq!(v1.z, f(3));
            assert_eq!(v2.z, f(3));
        }
    }

    #[test]
    fn wall_north_vertices() {
        let mut sink = VecSink::new();
        emit_wall_north(&mut sink, 1, 2, 0);

        assert_eq!(sink.tris.len(), EXPECTED_TRIS);
        // All vertices should be at z = gz = 2
        for (v0, v1, v2, _, _) in &sink.tris {
            assert_eq!(v0.z, f(2));
            assert_eq!(v1.z, f(2));
            assert_eq!(v2.z, f(2));
        }
    }

    #[test]
    fn wall_east_vertices() {
        let mut sink = VecSink::new();
        emit_wall_east(&mut sink, 1, 2, 0);

        assert_eq!(sink.tris.len(), EXPECTED_TRIS);
        // All vertices at x = gx+1 = 2
        for (v0, v1, v2, _, _) in &sink.tris {
            assert_eq!(v0.x, f(2));
            assert_eq!(v1.x, f(2));
            assert_eq!(v2.x, f(2));
        }
    }

    #[test]
    fn wall_west_vertices() {
        let mut sink = VecSink::new();
        emit_wall_west(&mut sink, 1, 2, 0);

        assert_eq!(sink.tris.len(), EXPECTED_TRIS);
        // All vertices at x = gx = 1
        for (v0, v1, v2, _, _) in &sink.tris {
            assert_eq!(v0.x, f(1));
            assert_eq!(v1.x, f(1));
            assert_eq!(v2.x, f(1));
        }
    }

    #[test]
    fn wall_faces_span_full_height() {
        let mut sink = VecSink::new();
        emit_wall_south(&mut sink, 0, 0, 0);

        // Should have vertices at both y=0 and y=WALL_HEIGHT
        let mut has_floor = false;
        let mut has_top = false;
        for (v0, v1, v2, _, _) in &sink.tris {
            for v in [v0, v1, v2] {
                if v.y == f(0) {
                    has_floor = true;
                }
                if v.y == WALL_HEIGHT {
                    has_top = true;
                }
            }
        }
        assert!(has_floor, "wall face should touch y=0");
        assert!(has_top, "wall face should reach WALL_HEIGHT");
    }

    #[test]
    fn subdivision_creates_midpoint_vertices() {
        let mut sink = VecSink::new();
        emit_floor(&mut sink, 0, 0, 0);

        // With SUBDIV=2, should have vertices at 0.0, 0.5, and 1.0
        let half = Fixed16::HALF;
        let mut has_half_x = false;
        let mut has_half_z = false;
        for (v0, v1, v2, _, _) in &sink.tris {
            for v in [v0, v1, v2] {
                if v.x == half {
                    has_half_x = true;
                }
                if v.z == half {
                    has_half_z = true;
                }
            }
        }
        if SUBDIV >= 2 {
            assert!(has_half_x, "subdivision should create x=0.5 midpoints");
            assert!(has_half_z, "subdivision should create z=0.5 midpoints");
        }
    }
}
