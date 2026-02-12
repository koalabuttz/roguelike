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
