use crossterm::style::Color;

use crate::entity::AiBehavior;

/// Defines a type of monster — all stats, appearance, and AI in one place.
pub struct MonsterTemplate {
    pub name: &'static str,
    pub glyph: char,
    pub color: Color,
    pub hp: i32,
    pub attack: i32,
    pub defense: i32,
    pub ai: AiBehavior,
}

pub const GOBLIN: MonsterTemplate = MonsterTemplate {
    name: "Goblin",
    glyph: 'g',
    color: Color::Green,
    hp: 6,
    attack: 3,
    defense: 0,
    ai: AiBehavior::Chase,
};

pub const ORC: MonsterTemplate = MonsterTemplate {
    name: "Orc",
    glyph: 'o',
    color: Color::DarkGreen,
    hp: 12,
    attack: 4,
    defense: 1,
    ai: AiBehavior::Chase,
};

pub const TROLL: MonsterTemplate = MonsterTemplate {
    name: "Troll",
    glyph: 'T',
    color: Color::DarkRed,
    hp: 20,
    attack: 6,
    defense: 3,
    ai: AiBehavior::Chase,
};

/// Weighted spawn entry — higher weight means more common.
pub struct SpawnEntry {
    pub template: &'static MonsterTemplate,
    pub weight: u32,
}

pub const SPAWN_TABLE: &[SpawnEntry] = &[
    SpawnEntry {
        template: &GOBLIN,
        weight: 60,
    },
    SpawnEntry {
        template: &ORC,
        weight: 30,
    },
    SpawnEntry {
        template: &TROLL,
        weight: 10,
    },
];

/// Game-wide tuning knobs — change these to rebalance without touching logic.
pub struct GameConfig {
    pub fov_radius: i32,
    pub max_rooms: i32,
    pub room_size_min: i32,
    pub room_size_max: i32,
    pub max_monsters_per_room: i32,
    pub ui_bottom_rows: i32,
    pub max_autorun_steps: i32,
    pub regen_interval: i32,
}

pub const CONFIG: GameConfig = GameConfig {
    fov_radius: 8,
    max_rooms: 30,
    room_size_min: 4,
    room_size_max: 10,
    max_monsters_per_room: 2,
    ui_bottom_rows: 5,
    max_autorun_steps: 100,
    regen_interval: 3,
};
