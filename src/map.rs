use rand::Rng;

#[derive(Clone, Copy, PartialEq)]
pub enum Tile {
    Wall,
    Floor,
}

pub struct Rect {
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect {
            x1: x,
            y1: y,
            x2: x + w,
            y2: y + h,
        }
    }

    pub fn center(&self) -> (i32, i32) {
        ((self.x1 + self.x2) / 2, (self.y1 + self.y2) / 2)
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x1 <= other.x2 && self.x2 >= other.x1 && self.y1 <= other.y2 && self.y2 >= other.y1
    }
}

pub struct Map {
    pub width: i32,
    pub height: i32,
    pub tiles: Vec<Tile>,
    pub rooms: Vec<Rect>,
}

impl Map {
    pub fn new(width: i32, height: i32) -> Self {
        Map {
            width,
            height,
            tiles: vec![Tile::Wall; (width * height) as usize],
            rooms: Vec::new(),
        }
    }

    pub fn idx(&self, x: i32, y: i32) -> usize {
        (y * self.width + x) as usize
    }

    pub fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && x < self.width && y >= 0 && y < self.height
    }

    pub fn is_walkable(&self, x: i32, y: i32) -> bool {
        self.in_bounds(x, y) && self.tiles[self.idx(x, y)] == Tile::Floor
    }

    /// Count walkable tiles adjacent to (x, y), excluding one direction.
    ///
    /// Useful for corridor topology detection: in a straight 1-wide corridor,
    /// the count excluding "behind" is exactly 1. At junctions or room
    /// entrances, it's 2+. At dead ends, it's 0.
    pub fn open_neighbors_excluding(
        &self,
        x: i32,
        y: i32,
        exclude_dx: i32,
        exclude_dy: i32,
    ) -> i32 {
        let mut count = 0;
        for ny in -1..=1 {
            for nx in -1..=1 {
                if nx == 0 && ny == 0 {
                    continue;
                }
                if nx == exclude_dx && ny == exclude_dy {
                    continue;
                }
                if self.is_walkable(x + nx, y + ny) {
                    count += 1;
                }
            }
        }
        count
    }

    fn carve_room(&mut self, room: &Rect) {
        for y in (room.y1 + 1)..room.y2 {
            for x in (room.x1 + 1)..room.x2 {
                if self.in_bounds(x, y) {
                    let idx = self.idx(x, y);
                    self.tiles[idx] = Tile::Floor;
                }
            }
        }
    }

    fn carve_h_tunnel(&mut self, x1: i32, x2: i32, y: i32) {
        for x in x1.min(x2)..=x1.max(x2) {
            if self.in_bounds(x, y) {
                let idx = self.idx(x, y);
                self.tiles[idx] = Tile::Floor;
            }
        }
    }

    fn carve_v_tunnel(&mut self, y1: i32, y2: i32, x: i32) {
        for y in y1.min(y2)..=y1.max(y2) {
            if self.in_bounds(x, y) {
                let idx = self.idx(x, y);
                self.tiles[idx] = Tile::Floor;
            }
        }
    }

    /// Generate a dungeon with random rooms connected by corridors.
    /// Returns the player's starting position (center of the first room).
    pub fn generate(&mut self, max_rooms: i32, min_size: i32, max_size: i32) -> (i32, i32) {
        let mut rng = rand::thread_rng();
        let mut player_start = (0, 0);

        for _ in 0..max_rooms {
            let w = rng.gen_range(min_size..=max_size);
            let h = rng.gen_range(min_size..=max_size);
            let x = rng.gen_range(1..self.width - w - 1);
            let y = rng.gen_range(1..self.height - h - 1);

            let new_room = Rect::new(x, y, w, h);

            if self.rooms.iter().any(|r| r.intersects(&new_room)) {
                continue;
            }

            self.carve_room(&new_room);

            let (new_cx, new_cy) = new_room.center();

            if self.rooms.is_empty() {
                player_start = (new_cx, new_cy);
            } else {
                let (prev_cx, prev_cy) = self.rooms.last().unwrap().center();
                if rng.gen_bool(0.5) {
                    self.carve_h_tunnel(prev_cx, new_cx, prev_cy);
                    self.carve_v_tunnel(prev_cy, new_cy, new_cx);
                } else {
                    self.carve_v_tunnel(prev_cy, new_cy, prev_cx);
                    self.carve_h_tunnel(prev_cx, new_cx, new_cy);
                }
            }

            self.rooms.push(new_room);
        }

        player_start
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_center() {
        let r = Rect::new(0, 0, 10, 6);
        assert_eq!(r.center(), (5, 3));
    }

    #[test]
    fn rect_center_odd_dimensions() {
        let r = Rect::new(1, 1, 5, 5);
        // x1=1, x2=6, y1=1, y2=6 → center (3,3)
        assert_eq!(r.center(), (3, 3));
    }

    #[test]
    fn rect_intersects_overlapping() {
        let a = Rect::new(0, 0, 5, 5);
        let b = Rect::new(3, 3, 5, 5);
        assert!(a.intersects(&b));
        assert!(b.intersects(&a));
    }

    #[test]
    fn rect_intersects_adjacent() {
        // Rects sharing an edge — x2 of a == x1 of b
        let a = Rect::new(0, 0, 5, 5);
        let b = Rect::new(5, 0, 5, 5);
        assert!(a.intersects(&b)); // touching counts as intersecting
    }

    #[test]
    fn rect_no_intersect_disjoint() {
        let a = Rect::new(0, 0, 3, 3);
        let b = Rect::new(10, 10, 3, 3);
        assert!(!a.intersects(&b));
    }

    #[test]
    fn map_new_all_walls() {
        let m = Map::new(10, 10);
        assert!(m.tiles.iter().all(|t| *t == Tile::Wall));
        assert_eq!(m.tiles.len(), 100);
        assert!(m.rooms.is_empty());
    }

    #[test]
    fn map_idx_correct() {
        let m = Map::new(80, 50);
        assert_eq!(m.idx(0, 0), 0);
        assert_eq!(m.idx(1, 0), 1);
        assert_eq!(m.idx(0, 1), 80);
        assert_eq!(m.idx(5, 3), 3 * 80 + 5);
    }

    #[test]
    fn in_bounds_interior() {
        let m = Map::new(80, 50);
        assert!(m.in_bounds(0, 0));
        assert!(m.in_bounds(79, 49));
        assert!(m.in_bounds(40, 25));
    }

    #[test]
    fn in_bounds_out_of_range() {
        let m = Map::new(80, 50);
        assert!(!m.in_bounds(-1, 0));
        assert!(!m.in_bounds(0, -1));
        assert!(!m.in_bounds(80, 0));
        assert!(!m.in_bounds(0, 50));
    }

    #[test]
    fn is_walkable_wall() {
        let m = Map::new(10, 10);
        assert!(!m.is_walkable(5, 5)); // all walls
    }

    #[test]
    fn is_walkable_floor() {
        let mut m = Map::new(10, 10);
        let idx = m.idx(5, 5);
        m.tiles[idx] = Tile::Floor;
        assert!(m.is_walkable(5, 5));
    }

    #[test]
    fn is_walkable_out_of_bounds() {
        let m = Map::new(10, 10);
        assert!(!m.is_walkable(-1, -1));
        assert!(!m.is_walkable(100, 100));
    }

    #[test]
    fn generate_creates_at_least_one_room() {
        let mut m = Map::new(80, 50);
        m.generate(30, 4, 10);
        assert!(!m.rooms.is_empty());
    }

    #[test]
    fn generate_player_start_is_walkable() {
        let mut m = Map::new(80, 50);
        let (px, py) = m.generate(30, 4, 10);
        assert!(m.is_walkable(px, py));
    }

    #[test]
    fn open_neighbors_in_corridor() {
        // Horizontal corridor at y=5
        let mut m = Map::new(10, 10);
        for x in 1..=8 {
            let idx = m.idx(x, 5);
            m.tiles[idx] = Tile::Floor;
        }
        // Middle of corridor: excluding west (behind), only east is open → 1
        assert_eq!(m.open_neighbors_excluding(5, 5, -1, 0), 1);
        // Dead end at x=8: excluding west, east is wall → 0
        assert_eq!(m.open_neighbors_excluding(8, 5, -1, 0), 0);
    }

    #[test]
    fn open_neighbors_at_junction() {
        let mut m = Map::new(10, 10);
        // Horizontal corridor at y=5
        for x in 1..=8 {
            let idx = m.idx(x, 5);
            m.tiles[idx] = Tile::Floor;
        }
        // Branch going north at x=5
        for y in 1..=4 {
            let idx = m.idx(5, y);
            m.tiles[idx] = Tile::Floor;
        }
        // At the junction: excluding west (behind), east and north are open → 2
        assert_eq!(m.open_neighbors_excluding(5, 5, -1, 0), 2);
    }

    #[test]
    fn generate_rooms_dont_overlap() {
        let mut m = Map::new(80, 50);
        m.generate(30, 4, 10);
        for i in 0..m.rooms.len() {
            for j in (i + 1)..m.rooms.len() {
                assert!(
                    !m.rooms[i].intersects(&m.rooms[j]),
                    "rooms {} and {} overlap",
                    i,
                    j
                );
            }
        }
    }
}
