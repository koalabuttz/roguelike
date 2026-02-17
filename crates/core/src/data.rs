use serde::Deserialize;

use crate::entity::AiBehavior;
use crate::types::{Coord, GameColor, Stat};

/// Top-level game data — player stats, config knobs, and monster definitions.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GameData {
    pub player: PlayerDef,
    pub config: GameConfig,
    pub monsters: Vec<MonsterDef>,
}

/// Player template — starting stats and appearance.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PlayerDef {
    pub hp: Stat,
    pub attack: Stat,
    pub defense: Stat,
    pub glyph: String,
    pub color: String,
}

/// Defines a type of monster — all stats, appearance, AI, and spawn weight.
#[derive(Debug, Clone, PartialEq, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Deserialize)]
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
    use std::collections::HashSet;
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

    /// Load game data from CWD `game.toml`, falling back to compiled-in defaults.
    ///
    /// This is the modding entry point: players can drop a `game.toml` in the
    /// working directory to override all balance values, monster definitions,
    /// and player stats.
    pub fn load_game_data() -> GameData {
        match std::fs::read_to_string("game.toml") {
            Ok(content) => match parse_game_data(&content) {
                Ok(data) => {
                    eprintln!("[data] Loaded game.toml from current directory.");
                    data
                }
                Err(e) => {
                    eprintln!(
                        "[data] Warning: game.toml parse error: {}. Using defaults.",
                        e
                    );
                    defaults().clone()
                }
            },
            Err(_) => defaults().clone(),
        }
    }

    /// Compare two `GameData` instances and return human-readable diff lines.
    ///
    /// Reports config field changes, player stat changes, monster stat changes,
    /// and added/removed monsters.
    pub fn diff_game_data(old: &GameData, new: &GameData) -> Vec<String> {
        let mut diffs = Vec::new();

        // Config changes.
        macro_rules! diff_config {
            ($field:ident) => {
                if old.config.$field != new.config.$field {
                    diffs.push(format!(
                        "{} {} -> {}",
                        stringify!($field),
                        old.config.$field,
                        new.config.$field
                    ));
                }
            };
        }
        diff_config!(fov_radius);
        diff_config!(max_rooms);
        diff_config!(room_size_min);
        diff_config!(room_size_max);
        diff_config!(max_monsters_per_room);
        diff_config!(ui_bottom_rows);
        diff_config!(max_autorun_steps);
        diff_config!(regen_interval);

        // Player stat changes.
        if old.player.hp != new.player.hp {
            diffs.push(format!("Player HP {} -> {}", old.player.hp, new.player.hp));
        }
        if old.player.attack != new.player.attack {
            diffs.push(format!(
                "Player ATK {} -> {}",
                old.player.attack, new.player.attack
            ));
        }
        if old.player.defense != new.player.defense {
            diffs.push(format!(
                "Player DEF {} -> {}",
                old.player.defense, new.player.defense
            ));
        }

        // Added/removed monsters.
        let old_names: HashSet<&str> = old.monsters.iter().map(|m| m.name.as_str()).collect();
        let new_names: HashSet<&str> = new.monsters.iter().map(|m| m.name.as_str()).collect();

        let mut added: Vec<&str> = new_names.difference(&old_names).copied().collect();
        added.sort();
        let mut removed: Vec<&str> = old_names.difference(&new_names).copied().collect();
        removed.sort();

        if !added.is_empty() {
            diffs.push(format!("Added: {}", added.join(", ")));
        }
        if !removed.is_empty() {
            diffs.push(format!("Removed: {}", removed.join(", ")));
        }

        // Monster stat changes for existing monsters.
        for new_m in &new.monsters {
            if let Some(old_m) = old.monster_by_name(&new_m.name) {
                let mut changes = Vec::new();
                if old_m.hp != new_m.hp {
                    changes.push(format!("HP {} -> {}", old_m.hp, new_m.hp));
                }
                if old_m.attack != new_m.attack {
                    changes.push(format!("ATK {} -> {}", old_m.attack, new_m.attack));
                }
                if old_m.defense != new_m.defense {
                    changes.push(format!("DEF {} -> {}", old_m.defense, new_m.defense));
                }
                if !changes.is_empty() {
                    diffs.push(format!("{} {}", new_m.name, changes.join(", ")));
                }
            }
        }

        diffs
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

    #[test]
    fn load_game_data_returns_defaults_without_file() {
        // When no CWD game.toml exists, load_game_data returns the same as defaults().
        let loaded = load_game_data();
        let expected = defaults();
        assert_eq!(loaded.player, expected.player);
        assert_eq!(loaded.config, expected.config);
        assert_eq!(loaded.monsters, expected.monsters);
    }

    #[test]
    fn diff_game_data_detects_config_changes() {
        let old = defaults().clone();
        let mut new = old.clone();
        new.config.regen_interval = 5;
        new.config.fov_radius = 12;
        let diffs = diff_game_data(&old, &new);
        assert!(diffs.iter().any(|d| d.contains("regen_interval")));
        assert!(diffs.iter().any(|d| d.contains("fov_radius")));
        assert!(diffs.iter().any(|d| d.contains("5")));
        assert!(diffs.iter().any(|d| d.contains("12")));
    }

    #[test]
    fn diff_game_data_detects_monster_changes() {
        let old = defaults().clone();
        let mut new = old.clone();
        // Change Troll HP.
        if let Some(troll) = new.monsters.iter_mut().find(|m| m.name == "Troll") {
            troll.hp = 25;
            troll.attack = 8;
        }
        let diffs = diff_game_data(&old, &new);
        assert!(diffs.iter().any(|d| d.contains("Troll")));
        assert!(diffs.iter().any(|d| d.contains("HP 20 -> 25")));
        assert!(diffs.iter().any(|d| d.contains("ATK 6 -> 8")));
    }

    #[test]
    fn diff_game_data_identical_returns_empty() {
        let data = defaults().clone();
        let diffs = diff_game_data(&data, &data);
        assert!(diffs.is_empty());
    }
}
