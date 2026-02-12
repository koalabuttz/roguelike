use crossterm::style::Color;

use crate::data::MonsterTemplate;

#[derive(Clone, Copy, PartialEq)]
pub enum EntityKind {
    Player,
    Monster,
}

#[derive(Clone, Copy, PartialEq)]
pub enum AiBehavior {
    None,  // Player — no automatic AI
    Chase, // Greedy chase toward player
}

pub struct Entity {
    pub x: i32,
    pub y: i32,
    pub glyph: char,
    pub color: Color,
    pub name: String,
    pub kind: EntityKind,
    pub ai: AiBehavior,
    pub hp: i32,
    pub max_hp: i32,
    pub attack: i32,
    pub defense: i32,
    pub alive: bool,
}

impl Entity {
    pub fn player(x: i32, y: i32) -> Self {
        Entity {
            x,
            y,
            glyph: '@',
            color: Color::Yellow,
            name: "Player".into(),
            kind: EntityKind::Player,
            ai: AiBehavior::None,
            hp: 30,
            max_hp: 30,
            attack: 5,
            defense: 2,
            alive: true,
        }
    }

    pub fn from_template(template: &MonsterTemplate, x: i32, y: i32) -> Self {
        Entity {
            x,
            y,
            glyph: template.glyph,
            color: template.color,
            name: template.name.into(),
            kind: EntityKind::Monster,
            ai: template.ai,
            hp: template.hp,
            max_hp: template.hp,
            attack: template.attack,
            defense: template.defense,
            alive: true,
        }
    }
}
