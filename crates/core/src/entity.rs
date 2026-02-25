use serde::{Deserialize, Serialize};

use crate::data::{MonsterDef, PlayerDef};
use crate::rules::message::Combatant;
use crate::rules::monster_table::MonsterKind;
use crate::types::{Coord, GameColor, Stat};

// AiBehavior is defined in rules::monster_table, re-exported here for
// backwards compatibility — existing `use crate::entity::AiBehavior` keeps working.
pub use crate::rules::monster_table::AiBehavior;

fn default_entity_sight_radius() -> Coord {
    8
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum EntityKind {
    Player,
    Monster,
}

#[derive(Serialize, Deserialize)]
pub struct Entity {
    pub x: Coord,
    pub y: Coord,
    pub glyph: char,
    pub color: GameColor,
    pub name: String,
    #[allow(dead_code)]
    pub kind: EntityKind,
    pub ai: AiBehavior,
    pub hp: Stat,
    pub max_hp: Stat,
    pub attack: Stat,
    pub defense: Stat,
    pub alive: bool,
    #[serde(default = "default_entity_sight_radius")]
    pub sight_radius: Coord,
    /// The canonical monster type, if this entity is a known monster kind.
    /// `None` for the player or custom/modded monsters without a matching kind.
    #[serde(default)]
    pub monster_kind: Option<MonsterKind>,
}

impl Entity {
    /// Create a player entity from a PlayerDef.
    pub fn player_from_def(def: &PlayerDef, x: Coord, y: Coord) -> Self {
        Entity {
            x,
            y,
            glyph: def.glyph_char(),
            color: def.game_color(),
            name: "Player".into(),
            kind: EntityKind::Player,
            ai: AiBehavior::None,
            hp: def.hp,
            max_hp: def.hp,
            attack: def.attack,
            defense: def.defense,
            alive: true,
            sight_radius: 0, // Player FOV managed by GameState.fov_radius
            monster_kind: None,
        }
    }

    /// Create a player entity with default stats.
    #[cfg(feature = "data-files")]
    pub fn player(x: Coord, y: Coord) -> Self {
        use crate::data;
        Self::player_from_def(&data::defaults().player, x, y)
    }

    /// Convert this entity to a `Combatant` for structured game events.
    pub fn combatant(&self) -> Combatant {
        match (self.kind, self.monster_kind) {
            (EntityKind::Player, _) => Combatant::Player,
            (EntityKind::Monster, Some(kind)) => Combatant::Monster(kind),
            (EntityKind::Monster, None) => Combatant::UnknownMonster,
        }
    }

    pub fn from_template(template: &MonsterDef, x: Coord, y: Coord) -> Self {
        Entity {
            x,
            y,
            glyph: template.glyph_char(),
            color: template.game_color(),
            name: template.name.clone(),
            kind: EntityKind::Monster,
            ai: template.ai_behavior(),
            hp: template.hp,
            max_hp: template.hp,
            attack: template.attack,
            defense: template.defense,
            alive: true,
            sight_radius: template.sight_radius,
            monster_kind: template.monster_kind(),
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
        let e = Entity::from_template(data::goblin(), 3, 7);
        assert_eq!(e.x, 3);
        assert_eq!(e.y, 7);
        assert_eq!(e.glyph, data::goblin().glyph_char());
        assert_eq!(e.name, data::goblin().name);
        assert_eq!(e.hp, data::goblin().hp);
        assert_eq!(e.max_hp, data::goblin().hp);
        assert_eq!(e.attack, data::goblin().attack);
        assert_eq!(e.defense, data::goblin().defense);
        assert_eq!(e.kind, EntityKind::Monster);
        assert_eq!(e.ai, data::goblin().ai_behavior());
        assert!(e.alive);
    }

    #[test]
    fn from_template_sets_hp_to_max_hp() {
        let e = Entity::from_template(data::troll(), 0, 0);
        assert_eq!(e.hp, e.max_hp);
    }

    #[test]
    fn from_template_copies_sight_radius() {
        let e = Entity::from_template(data::goblin(), 0, 0);
        assert_eq!(e.sight_radius, data::goblin().sight_radius);
        let e2 = Entity::from_template(data::troll(), 0, 0);
        assert_eq!(e2.sight_radius, data::troll().sight_radius);
    }

    #[test]
    fn player_sight_radius_is_zero() {
        let p = Entity::player(5, 5);
        assert_eq!(p.sight_radius, 0);
    }
}
