use crossterm::style::Color;

use crate::data::MonsterTemplate;
use crate::types::{Coord, Stat};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EntityKind {
    Player,
    Monster,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AiBehavior {
    None,  // Player — no automatic AI
    Chase, // Greedy chase toward player
}

pub struct Entity {
    pub x: Coord,
    pub y: Coord,
    pub glyph: char,
    pub color: Color,
    pub name: String,
    #[allow(dead_code)]
    pub kind: EntityKind,
    pub ai: AiBehavior,
    pub hp: Stat,
    pub max_hp: Stat,
    pub attack: Stat,
    pub defense: Stat,
    pub alive: bool,
}

impl Entity {
    pub fn player(x: Coord, y: Coord) -> Self {
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

    pub fn from_template(template: &MonsterTemplate, x: Coord, y: Coord) -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data;

    #[test]
    fn player_has_correct_stats() {
        let p = Entity::player(5, 10);
        assert_eq!(p.x, 5);
        assert_eq!(p.y, 10);
        assert_eq!(p.glyph, '@');
        assert_eq!(p.hp, 30);
        assert_eq!(p.max_hp, 30);
        assert_eq!(p.attack, 5);
        assert_eq!(p.defense, 2);
        assert!(p.alive);
        assert_eq!(p.kind, EntityKind::Player);
        assert_eq!(p.ai, AiBehavior::None);
    }

    #[test]
    fn from_template_copies_all_fields() {
        let e = Entity::from_template(&data::GOBLIN, 3, 7);
        assert_eq!(e.x, 3);
        assert_eq!(e.y, 7);
        assert_eq!(e.glyph, data::GOBLIN.glyph);
        assert_eq!(e.name, data::GOBLIN.name);
        assert_eq!(e.hp, data::GOBLIN.hp);
        assert_eq!(e.max_hp, data::GOBLIN.hp);
        assert_eq!(e.attack, data::GOBLIN.attack);
        assert_eq!(e.defense, data::GOBLIN.defense);
        assert_eq!(e.kind, EntityKind::Monster);
        assert_eq!(e.ai, data::GOBLIN.ai);
        assert!(e.alive);
    }

    #[test]
    fn from_template_sets_hp_to_max_hp() {
        let e = Entity::from_template(&data::TROLL, 0, 0);
        assert_eq!(e.hp, e.max_hp);
    }
}
