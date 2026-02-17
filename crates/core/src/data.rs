use serde::Deserialize;

use crate::entity::AiBehavior;
use crate::types::{Coord, GameColor, Stat};

/// Top-level game data — player stats, config knobs, and monster definitions.
#[derive(Debug, Clone, Deserialize)]
pub struct GameData {
    pub player: PlayerDef,
    pub config: GameConfig,
    pub monsters: Vec<MonsterDef>,
}

/// Player template — starting stats and appearance.
#[derive(Debug, Clone, Deserialize)]
pub struct PlayerDef {
    pub hp: Stat,
    pub attack: Stat,
    pub defense: Stat,
    pub glyph: String,
    pub color: String,
}

/// Defines a type of monster — all stats, appearance, AI, and spawn weight.
#[derive(Debug, Clone, Deserialize)]
pub struct MonsterDef {
    pub name: String,
    pub glyph: String,
    pub color: String,
    pub hp: Stat,
    pub attack: Stat,
    pub defense: Stat,
    pub ai: String,
    pub spawn_weight: u32,
}

/// Game-wide tuning knobs — change these to rebalance without touching logic.
#[derive(Debug, Clone, Deserialize)]
pub struct GameConfig {
    pub fov_radius: Coord,
    pub max_rooms: i32,
    pub room_size_min: Coord,
    pub room_size_max: Coord,
    pub max_monsters_per_room: i32,
    pub ui_bottom_rows: i32,
    pub max_autorun_steps: i32,
    pub regen_interval: i32,
}

impl GameData {
    /// Find a monster definition by name (case-insensitive).
    pub fn monster_by_name(&self, name: &str) -> Option<&MonsterDef> {
        let lower = name.to_lowercase();
        self.monsters
            .iter()
            .find(|m| m.name.to_lowercase() == lower)
    }
}

impl MonsterDef {
    /// Parse the glyph string into a char.
    pub fn glyph_char(&self) -> char {
        self.glyph.chars().next().unwrap_or('?')
    }

    /// Parse the color string into a GameColor.
    pub fn game_color(&self) -> GameColor {
        parse_color(&self.color)
    }

    /// Parse the AI string into an AiBehavior.
    pub fn ai_behavior(&self) -> AiBehavior {
        match self.ai.to_lowercase().as_str() {
            "chase" => AiBehavior::Chase,
            _ => AiBehavior::None,
        }
    }
}

impl PlayerDef {
    /// Parse the glyph string into a char.
    pub fn glyph_char(&self) -> char {
        self.glyph.chars().next().unwrap_or('@')
    }

    /// Parse the color string into a GameColor.
    pub fn game_color(&self) -> GameColor {
        parse_color(&self.color)
    }
}

fn parse_color(s: &str) -> GameColor {
    match s {
        "Black" => GameColor::Black,
        "White" => GameColor::White,
        "Grey" => GameColor::Grey,
        "DarkGrey" => GameColor::DarkGrey,
        "Red" => GameColor::Red,
        "DarkRed" => GameColor::DarkRed,
        "Green" => GameColor::Green,
        "DarkGreen" => GameColor::DarkGreen,
        "Yellow" => GameColor::Yellow,
        "DarkBlue" => GameColor::DarkBlue,
        "Cyan" => GameColor::Cyan,
        _ => GameColor::White,
    }
}

// --- Feature-gated: data-files ---

#[cfg(feature = "data-files")]
mod data_files {
    use std::sync::LazyLock;

    use super::*;

    const DEFAULT_TOML: &str = include_str!("../data/game.toml");

    static DEFAULT_DATA: LazyLock<GameData> =
        LazyLock::new(|| parse_game_data(DEFAULT_TOML).expect("embedded game.toml is invalid"));

    /// Access the default game data (parsed once from embedded TOML).
    pub fn defaults() -> &'static GameData {
        &DEFAULT_DATA
    }

    /// Parse a TOML string into GameData.
    pub fn parse_game_data(toml_str: &str) -> Result<GameData, String> {
        toml::from_str(toml_str).map_err(|e| format!("TOML parse error: {e}"))
    }

    /// Convenience: access the default game config.
    pub fn config() -> &'static GameConfig {
        &defaults().config
    }

    /// Convenience: access the Goblin definition.
    pub fn goblin() -> &'static MonsterDef {
        defaults()
            .monster_by_name("Goblin")
            .expect("Goblin not found in game data")
    }

    /// Convenience: access the Orc definition.
    pub fn orc() -> &'static MonsterDef {
        defaults()
            .monster_by_name("Orc")
            .expect("Orc not found in game data")
    }

    /// Convenience: access the Troll definition.
    pub fn troll() -> &'static MonsterDef {
        defaults()
            .monster_by_name("Troll")
            .expect("Troll not found in game data")
    }
}

#[cfg(feature = "data-files")]
pub use data_files::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_embedded_toml() {
        let data = defaults();
        assert_eq!(data.player.hp, 30);
        assert_eq!(data.player.attack, 5);
        assert_eq!(data.player.defense, 2);
        assert_eq!(data.config.fov_radius, 8);
        assert_eq!(data.config.regen_interval, 3);
        assert_eq!(data.monsters.len(), 3);
    }

    #[test]
    fn monster_by_name_case_insensitive() {
        let data = defaults();
        assert!(data.monster_by_name("goblin").is_some());
        assert!(data.monster_by_name("GOBLIN").is_some());
        assert!(data.monster_by_name("Goblin").is_some());
        assert!(data.monster_by_name("dragon").is_none());
    }

    #[test]
    fn convenience_accessors() {
        assert_eq!(goblin().name, "Goblin");
        assert_eq!(goblin().hp, 6);
        assert_eq!(orc().name, "Orc");
        assert_eq!(orc().hp, 12);
        assert_eq!(troll().name, "Troll");
        assert_eq!(troll().hp, 20);
    }

    #[test]
    fn config_accessor() {
        let cfg = config();
        assert_eq!(cfg.fov_radius, 8);
        assert_eq!(cfg.max_rooms, 30);
        assert_eq!(cfg.max_autorun_steps, 100);
    }

    #[test]
    fn glyph_and_color_parsing() {
        assert_eq!(goblin().glyph_char(), 'g');
        assert_eq!(goblin().game_color(), GameColor::Green);
        assert_eq!(goblin().ai_behavior(), AiBehavior::Chase);
        assert_eq!(defaults().player.glyph_char(), '@');
        assert_eq!(defaults().player.game_color(), GameColor::Yellow);
    }

    #[test]
    fn spawn_weights_sum_to_100() {
        let total: u32 = defaults().monsters.iter().map(|m| m.spawn_weight).sum();
        assert_eq!(total, 100);
    }
}
