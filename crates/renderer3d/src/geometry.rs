use roguelike_core::rules::color::GameColor;
use roguelike_core::rules::game_view::{GameView, TileVisibility};
use roguelike_core::rules::tiles::{self, TileKind};

use crate::color_map::{Face, tile_color};
use crate::math::{Fixed16, Vec3};

/// Height of wall blocks in world units.
pub const WALL_HEIGHT: Fixed16 = Fixed16::ONE;

/// Size of entity/item marker quads as a fraction of a tile.
const MARKER_PAD: Fixed16 = Fixed16::from_raw(0x4CCC); // ~0.3
const MARKER_SIZE: Fixed16 = Fixed16::from_raw(0x6666); // ~0.4

/// Consumer of triangles produced by geometry generation.
///
/// The streaming pattern: geometry is generated, emitted into the sink,
/// transformed, rasterized, and forgotten — zero intermediate allocation.
pub trait TriangleSink {
    /// Receive a triangle with CCW winding and a pre-computed RGB555 color.
    fn emit(&mut self, v0: Vec3, v1: Vec3, v2: Vec3, color: u16);
}

/// Emit a quad as two triangles with reversed winding.
///
/// Callers specify vertices in CCW order when viewed from the front face
/// (the natural convention). This function emits them in CW order to
/// compensate for the viewport y-flip in project_vertex, which negates
/// the screen-space cross product. CW-in-world → CCW-in-screen → front-facing.
#[inline]
fn emit_quad(sink: &mut dyn TriangleSink, v0: Vec3, v1: Vec3, v2: Vec3, v3: Vec3, color: u16) {
    sink.emit(v0, v2, v1, color);
    sink.emit(v0, v3, v2, color);
}

/// Helper to create a Vec3 from grid coordinates at a given height.
#[inline]
fn gv(x: i32, y_height: Fixed16, z: i32) -> Vec3 {
    Vec3::new(Fixed16::from_int(x), y_height, Fixed16::from_int(z))
}

/// Emit a floor quad at y=0 for grid position (gx, gz).
/// CCW when viewed from above (+y direction).
fn emit_floor(sink: &mut dyn TriangleSink, gx: i32, gz: i32, color: u16) {
    let y = Fixed16::ZERO;
    //  (gx, gz) --- (gx+1, gz)
    //     |              |
    //  (gx, gz+1) - (gx+1, gz+1)
    // CCW from above: go counter-clockwise
    let v0 = gv(gx, y, gz);
    let v1 = gv(gx, y, gz + 1);
    let v2 = gv(gx + 1, y, gz + 1);
    let v3 = gv(gx + 1, y, gz);
    emit_quad(sink, v0, v1, v2, v3, color);
}

/// Emit a wall top quad at y=WALL_HEIGHT for grid position (gx, gz).
/// CCW when viewed from above (+y direction).
fn emit_wall_top(sink: &mut dyn TriangleSink, gx: i32, gz: i32, color: u16) {
    let y = WALL_HEIGHT;
    let v0 = gv(gx, y, gz);
    let v1 = gv(gx, y, gz + 1);
    let v2 = gv(gx + 1, y, gz + 1);
    let v3 = gv(gx + 1, y, gz);
    emit_quad(sink, v0, v1, v2, v3, color);
}

/// Emit south-facing wall (at z=gz+1, facing +z direction).
/// CCW when viewed from +z (outside the block, looking north).
fn emit_wall_south(sink: &mut dyn TriangleSink, gx: i32, gz: i32, color: u16) {
    let z = gz + 1;
    let v0 = gv(gx, Fixed16::ZERO, z);
    let v1 = gv(gx + 1, Fixed16::ZERO, z);
    let v2 = gv(gx + 1, WALL_HEIGHT, z);
    let v3 = gv(gx, WALL_HEIGHT, z);
    emit_quad(sink, v0, v1, v2, v3, color);
}

/// Emit north-facing wall (at z=gz, facing -z direction).
/// CCW when viewed from -z (outside the block, looking south).
fn emit_wall_north(sink: &mut dyn TriangleSink, gx: i32, gz: i32, color: u16) {
    let z = gz;
    let v0 = gv(gx + 1, Fixed16::ZERO, z);
    let v1 = gv(gx, Fixed16::ZERO, z);
    let v2 = gv(gx, WALL_HEIGHT, z);
    let v3 = gv(gx + 1, WALL_HEIGHT, z);
    emit_quad(sink, v0, v1, v2, v3, color);
}

/// Emit east-facing wall (at x=gx+1, facing +x direction).
/// CCW when viewed from +x (outside the block, looking west).
fn emit_wall_east(sink: &mut dyn TriangleSink, gx: i32, gz: i32, color: u16) {
    let x = gx + 1;
    let v0 = gv(x, Fixed16::ZERO, gz + 1);
    let v1 = gv(x, Fixed16::ZERO, gz);
    let v2 = gv(x, WALL_HEIGHT, gz);
    let v3 = gv(x, WALL_HEIGHT, gz + 1);
    emit_quad(sink, v0, v1, v2, v3, color);
}

/// Emit west-facing wall (at x=gx, facing -x direction).
/// CCW when viewed from -x (outside the block, looking east).
fn emit_wall_west(sink: &mut dyn TriangleSink, gx: i32, gz: i32, color: u16) {
    let x = gx;
    let v0 = gv(x, Fixed16::ZERO, gz);
    let v1 = gv(x, Fixed16::ZERO, gz + 1);
    let v2 = gv(x, WALL_HEIGHT, gz + 1);
    let v3 = gv(x, WALL_HEIGHT, gz);
    emit_quad(sink, v0, v1, v2, v3, color);
}

/// Emit a small colored quad on the floor for an entity or item marker.
fn emit_marker(sink: &mut dyn TriangleSink, gx: i32, gz: i32, color: u16) {
    let base_x = Fixed16::from_int(gx) + MARKER_PAD;
    let base_z = Fixed16::from_int(gz) + MARKER_PAD;
    let y = Fixed16::from_raw(0x0100); // slightly above floor to avoid z-fighting

    let v0 = Vec3::new(base_x, y, base_z);
    let v1 = Vec3::new(base_x, y, base_z + MARKER_SIZE);
    let v2 = Vec3::new(base_x + MARKER_SIZE, y, base_z + MARKER_SIZE);
    let v3 = Vec3::new(base_x + MARKER_SIZE, y, base_z);
    emit_quad(sink, v0, v1, v2, v3, color);
}

/// Check if a tile at (x, z) is a Structural wall.
fn is_structural(view: &dyn GameView, x: i32, z: i32) -> bool {
    let (w, h) = view.map_dims();
    if x < 0 || z < 0 || x >= w || z >= h {
        return false;
    }
    tiles::from_micro(view.tile_at(x, z)) == Some(TileKind::Structural)
}

/// Generate 3D triangles for all visible/explored tiles in the map.
///
/// Walks the tile grid via `GameView`, emitting geometry into the sink:
/// - Floor/StairsDown → flat quad at y=0
/// - Structural → extruded block (top + adjacency-culled side faces)
/// - Entities/items at visible positions → small colored floor markers
///
/// Unexplored tiles and Wall (void) tiles produce no geometry.
pub fn generate_map_geometry(view: &dyn GameView, sink: &mut dyn TriangleSink) {
    let (w, h) = view.map_dims();

    for gz in 0..h {
        for gx in 0..w {
            let vis = view.tile_visibility(gx, gz);
            if vis == TileVisibility::Unexplored {
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
                    let color = tile_color(tiles::color(tile), Face::Top, vis);
                    emit_floor(sink, gx, gz, color);
                }
                TileKind::Structural => {
                    let wall_color = GameColor::White; // structural walls are white
                    let top_color = tile_color(wall_color, Face::Top, vis);
                    let side_color = tile_color(wall_color, Face::Side, vis);

                    emit_wall_top(sink, gx, gz, top_color);

                    // Emit side faces only where bordering non-structural tiles
                    if !is_structural(view, gx, gz - 1) {
                        emit_wall_north(sink, gx, gz, side_color);
                    }
                    if !is_structural(view, gx, gz + 1) {
                        emit_wall_south(sink, gx, gz, side_color);
                    }
                    if !is_structural(view, gx - 1, gz) {
                        emit_wall_west(sink, gx, gz, side_color);
                    }
                    if !is_structural(view, gx + 1, gz) {
                        emit_wall_east(sink, gx, gz, side_color);
                    }
                }
            }
        }
    }

    // Entity markers (visible only)
    for i in 0..view.entity_count() {
        if !view.entity_alive(i) {
            continue;
        }
        let (ex, ey) = view.entity_xy(i);
        if view.tile_visibility(ex, ey) != TileVisibility::Visible {
            continue;
        }
        let (_, gc) = view.render_entity(i);
        let color = crate::color_map::game_color_to_rgb555(gc);
        emit_marker(sink, ex, ey, color);
    }

    // Item markers (visible only)
    for i in 0..view.item_count() {
        if !view.item_alive(i) {
            continue;
        }
        let (ix, iy) = view.item_xy(i);
        if view.tile_visibility(ix, iy) != TileVisibility::Visible {
            continue;
        }
        let (_, gc) = view.render_item(i);
        let color = crate::color_map::game_color_to_rgb555(gc);
        emit_marker(sink, ix, iy, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framebuffer::rgb555;

    /// Collects emitted triangles for inspection.
    struct VecSink {
        tris: Vec<(Vec3, Vec3, Vec3, u16)>,
    }

    impl VecSink {
        fn new() -> Self {
            Self { tris: Vec::new() }
        }
    }

    impl TriangleSink for VecSink {
        fn emit(&mut self, v0: Vec3, v1: Vec3, v2: Vec3, color: u16) {
            self.tris.push((v0, v1, v2, color));
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

    #[test]
    fn floor_emits_two_triangles() {
        let mut sink = VecSink::new();
        let color = rgb555(10, 10, 10);
        emit_floor(&mut sink, 3, 5, color);

        assert_eq!(sink.tris.len(), 2, "floor quad should emit 2 triangles");

        // All vertices at y=0
        for (v0, v1, v2, c) in &sink.tris {
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
        for (v0, v1, v2, _) in &sink.tris {
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

        assert_eq!(sink.tris.len(), 2);
        for (v0, v1, v2, _) in &sink.tris {
            assert_eq!(v0.y, WALL_HEIGHT);
            assert_eq!(v1.y, WALL_HEIGHT);
            assert_eq!(v2.y, WALL_HEIGHT);
        }
    }

    #[test]
    fn wall_top_winding_for_yflip() {
        let mut sink = VecSink::new();
        emit_wall_top(&mut sink, 0, 0, 0);

        for (v0, v1, v2, _) in &sink.tris {
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

        assert_eq!(sink.tris.len(), 2);
        // All vertices should be at z = gz+1 = 3
        for (v0, v1, v2, _) in &sink.tris {
            assert_eq!(v0.z, f(3));
            assert_eq!(v1.z, f(3));
            assert_eq!(v2.z, f(3));
        }
    }

    #[test]
    fn wall_north_vertices() {
        let mut sink = VecSink::new();
        emit_wall_north(&mut sink, 1, 2, 0);

        assert_eq!(sink.tris.len(), 2);
        // All vertices should be at z = gz = 2
        for (v0, v1, v2, _) in &sink.tris {
            assert_eq!(v0.z, f(2));
            assert_eq!(v1.z, f(2));
            assert_eq!(v2.z, f(2));
        }
    }

    #[test]
    fn wall_east_vertices() {
        let mut sink = VecSink::new();
        emit_wall_east(&mut sink, 1, 2, 0);

        assert_eq!(sink.tris.len(), 2);
        // All vertices at x = gx+1 = 2
        for (v0, v1, v2, _) in &sink.tris {
            assert_eq!(v0.x, f(2));
            assert_eq!(v1.x, f(2));
            assert_eq!(v2.x, f(2));
        }
    }

    #[test]
    fn wall_west_vertices() {
        let mut sink = VecSink::new();
        emit_wall_west(&mut sink, 1, 2, 0);

        assert_eq!(sink.tris.len(), 2);
        // All vertices at x = gx = 1
        for (v0, v1, v2, _) in &sink.tris {
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
        for (v0, v1, v2, _) in &sink.tris {
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
    fn marker_slightly_above_floor() {
        let mut sink = VecSink::new();
        emit_marker(&mut sink, 5, 5, rgb555(0, 31, 0));

        assert_eq!(sink.tris.len(), 2);
        // All vertices should be slightly above zero
        for (v0, v1, v2, _) in &sink.tris {
            assert!(v0.y > Fixed16::ZERO);
            assert!(v1.y > Fixed16::ZERO);
            assert!(v2.y > Fixed16::ZERO);
            // But well below wall height
            assert!(v0.y < WALL_HEIGHT);
        }
    }
}
