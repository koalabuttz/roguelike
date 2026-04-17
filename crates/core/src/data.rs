use serde::{Deserialize, Serialize};

use crate::entity::AiPersonality;
use crate::rules::balance;
use crate::rules::monster_table::{self, MonsterKind};
use crate::types::{Coord, GameColor, Stat};

fn default_sight_radius() -> Coord {
    balance::FOV_RADIUS as Coord
}

/// Top-level game data — player stats, config knobs, and monster definitions.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GameData {
    pub player: PlayerDef,
    pub config: GameConfig,
    #[serde(default)]
    pub wandering: WanderingConfig,
    #[serde(default)]
    pub depth_scaling: DepthScaling,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonsterDef {
    pub name: String,
    pub glyph: String,
    pub color: String,
    pub hp: Stat,
    pub attack: Stat,
    pub defense: Stat,
    pub ai: String,
    pub spawn_weight: u32,
    #[serde(default = "default_sight_radius")]
    pub sight_radius: Coord,
}

fn default_target_depth() -> Stat {
    balance::TARGET_DEPTH as Stat
}

fn default_hp_per_floor() -> Stat {
    balance::MONSTER_HP_PER_FLOOR as Stat
}

fn default_atk_per_floor() -> Stat {
    balance::MONSTER_ATK_PER_FLOOR as Stat
}

fn default_depth_scale_interval() -> Stat {
    balance::DEPTH_SCALE_INTERVAL as Stat
}

/// Game-wide tuning knobs — change these to rebalance without touching logic.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GameConfig {
    pub fov_radius: Coord,
    pub max_rooms: Stat,
    pub room_size_min: Coord,
    pub room_size_max: Coord,
    pub max_monsters_per_room: Stat,
    pub ui_bottom_rows: Stat,
    pub max_autorun_steps: Stat,
    pub regen_interval: Stat,
    #[serde(default = "default_target_depth")]
    pub target_depth: Stat,
}

/// Per-floor monster stat scaling for multi-level dungeons.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct DepthScaling {
    #[serde(default = "default_hp_per_floor")]
    pub monster_hp_per_floor: Stat,
    #[serde(default = "default_atk_per_floor")]
    pub monster_atk_per_floor: Stat,
    #[serde(default = "default_depth_scale_interval")]
    pub depth_scale_interval: Stat,
}

impl Default for DepthScaling {
    fn default() -> Self {
        Self {
            monster_hp_per_floor: default_hp_per_floor(),
            monster_atk_per_floor: default_atk_per_floor(),
            depth_scale_interval: default_depth_scale_interval(),
        }
    }
}

fn default_spawn_interval() -> Stat {
    balance::WANDERING_SPAWN_INTERVAL as Stat
}
fn default_spawn_chance() -> Stat {
    balance::WANDERING_SPAWN_CHANCE as Stat
}
fn default_grace_period() -> Stat {
    balance::WANDERING_GRACE_PERIOD as Stat
}
fn default_max_wandering() -> Stat {
    balance::WANDERING_MAX_ACTIVE as Stat
}
fn default_sound_far() -> Coord {
    balance::WANDERING_SOUND_FAR as Coord
}
fn default_sound_medium() -> Coord {
    balance::WANDERING_SOUND_MEDIUM as Coord
}
fn default_sound_near() -> Coord {
    balance::WANDERING_SOUND_NEAR as Coord
}
fn default_idle_threshold() -> Stat {
    balance::WANDERING_IDLE_THRESHOLD as Stat
}
fn default_idle_acceleration() -> Stat {
    balance::WANDERING_IDLE_ACCELERATION as Stat
}

/// Configuration for wandering monster spawns and sound cues.
///
/// All fields have sensible defaults and can be overridden in `game.toml`.
/// Forward-compatible with themed floors (per-floor overrides) and SimBudget.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct WanderingConfig {
    /// Turns between spawn checks.
    #[serde(default = "default_spawn_interval")]
    pub spawn_interval: Stat,
    /// Percent chance per check (0-100).
    #[serde(default = "default_spawn_chance")]
    pub spawn_chance: Stat,
    /// No spawns before this turn.
    #[serde(default = "default_grace_period")]
    pub grace_period: Stat,
    /// Max alive wandering monsters.
    #[serde(default = "default_max_wandering")]
    pub max_wandering: Stat,
    /// Distance threshold for "faint sound" message.
    #[serde(default = "default_sound_far")]
    pub sound_far: Coord,
    /// Distance threshold for "footsteps nearby" message.
    #[serde(default = "default_sound_medium")]
    pub sound_medium: Coord,
    /// Distance threshold for "very close" message.
    #[serde(default = "default_sound_near")]
    pub sound_near: Coord,
    /// Consecutive waits before idle acceleration kicks in.
    #[serde(default = "default_idle_threshold")]
    pub idle_threshold: Stat,
    /// Divisor for spawn_interval when player is idle.
    #[serde(default = "default_idle_acceleration")]
    pub idle_acceleration: Stat,
}

impl Default for WanderingConfig {
    fn default() -> Self {
        Self {
            spawn_interval: default_spawn_interval(),
            spawn_chance: default_spawn_chance(),
            grace_period: default_grace_period(),
            max_wandering: default_max_wandering(),
            sound_far: default_sound_far(),
            sound_medium: default_sound_medium(),
            sound_near: default_sound_near(),
            idle_threshold: default_idle_threshold(),
            idle_acceleration: default_idle_acceleration(),
        }
    }
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

    /// Parse the AI string into an AiPersonality.
    pub fn ai_personality(&self) -> AiPersonality {
        match self.ai.to_lowercase().as_str() {
            "chase" | "aggressive" => AiPersonality::Aggressive,
            "wander" | "patrol" => AiPersonality::Patrol,
            _ => AiPersonality::Player,
        }
    }

    /// Map this definition's name to a canonical `MonsterKind`.
    /// Returns `None` for custom/modded monsters without a matching kind.
    pub fn monster_kind(&self) -> Option<MonsterKind> {
        monster_table::from_name(&self.name)
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

    /// Known color names — must match the arms of `parse_color()` above.
    const KNOWN_COLORS: &[&str] = &[
        "Black",
        "White",
        "Grey",
        "DarkGrey",
        "Red",
        "DarkRed",
        "Green",
        "DarkGreen",
        "Yellow",
        "DarkBlue",
        "Cyan",
    ];

    /// Known AI personality names — must match the arms of `MonsterDef::ai_personality()`.
    const KNOWN_AI: &[&str] = &["chase", "wander", "aggressive", "patrol"];

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
                    let warnings = validate_game_data(&data);
                    for w in &warnings {
                        eprintln!("[data] Warning: {}", w);
                    }
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

    /// Validate game data and return warnings for suspicious values.
    ///
    /// Returns an empty `Vec` when everything looks correct. Warnings are
    /// informational — a game.toml with odd values still loads and plays.
    pub fn validate_game_data(data: &GameData) -> Vec<String> {
        let mut warnings = Vec::new();

        // --- Player validation ---
        if data.player.hp <= 0 {
            warnings.push(format!("Player HP must be > 0 (got {})", data.player.hp));
        }
        if data.player.attack < 0 {
            warnings.push(format!(
                "Player attack must be >= 0 (got {})",
                data.player.attack
            ));
        }
        if data.player.defense < 0 {
            warnings.push(format!(
                "Player defense must be >= 0 (got {})",
                data.player.defense
            ));
        }
        validate_glyph("Player", &data.player.glyph, &mut warnings);
        validate_color("Player", &data.player.color, &mut warnings);

        // --- Config validation ---
        if data.config.fov_radius <= 0 {
            warnings.push(format!(
                "fov_radius must be > 0 (got {})",
                data.config.fov_radius
            ));
        }
        if data.config.room_size_min > data.config.room_size_max {
            warnings.push(format!(
                "room_size_min ({}) > room_size_max ({})",
                data.config.room_size_min, data.config.room_size_max
            ));
        }
        if data.config.max_rooms <= 0 {
            warnings.push(format!(
                "max_rooms must be > 0 (got {})",
                data.config.max_rooms
            ));
        }
        if data.config.max_monsters_per_room < 0 {
            warnings.push(format!(
                "max_monsters_per_room must be >= 0 (got {})",
                data.config.max_monsters_per_room
            ));
        }

        // --- Depth / scaling validation ---
        if data.config.target_depth <= 0 {
            warnings.push(format!(
                "target_depth must be > 0 (got {})",
                data.config.target_depth
            ));
        }
        if data.depth_scaling.monster_hp_per_floor < 0 {
            warnings.push(format!(
                "depth_scaling.monster_hp_per_floor must be >= 0 (got {})",
                data.depth_scaling.monster_hp_per_floor
            ));
        }
        if data.depth_scaling.monster_atk_per_floor < 0 {
            warnings.push(format!(
                "depth_scaling.monster_atk_per_floor must be >= 0 (got {})",
                data.depth_scaling.monster_atk_per_floor
            ));
        }
        if data.depth_scaling.depth_scale_interval <= 0 {
            warnings.push(format!(
                "depth_scaling.depth_scale_interval must be > 0 (got {})",
                data.depth_scaling.depth_scale_interval
            ));
        }

        // --- Wandering validation ---
        let w = &data.wandering;
        if w.spawn_interval <= 0 {
            warnings.push(format!(
                "wandering.spawn_interval must be > 0 (got {})",
                w.spawn_interval
            ));
        }
        if w.spawn_chance < 0 || w.spawn_chance > 100 {
            warnings.push(format!(
                "wandering.spawn_chance must be 0-100 (got {})",
                w.spawn_chance
            ));
        }
        if w.grace_period < 0 {
            warnings.push(format!(
                "wandering.grace_period must be >= 0 (got {})",
                w.grace_period
            ));
        }
        if w.max_wandering <= 0 {
            warnings.push(format!(
                "wandering.max_wandering must be > 0 (got {})",
                w.max_wandering
            ));
        }
        if w.sound_far < w.sound_medium {
            warnings.push(format!(
                "wandering.sound_far ({}) < sound_medium ({})",
                w.sound_far, w.sound_medium
            ));
        }
        if w.sound_medium < w.sound_near {
            warnings.push(format!(
                "wandering.sound_medium ({}) < sound_near ({})",
                w.sound_medium, w.sound_near
            ));
        }
        if w.sound_near <= 0 {
            warnings.push(format!(
                "wandering.sound_near must be > 0 (got {})",
                w.sound_near
            ));
        }
        if w.idle_threshold <= 0 {
            warnings.push(format!(
                "wandering.idle_threshold must be > 0 (got {})",
                w.idle_threshold
            ));
        }
        if w.idle_acceleration <= 0 {
            warnings.push(format!(
                "wandering.idle_acceleration must be > 0 (got {})",
                w.idle_acceleration
            ));
        }

        // --- Monster validation ---
        let mut seen_names: HashSet<String> = HashSet::new();
        for m in &data.monsters {
            let lower = m.name.to_lowercase();
            if !seen_names.insert(lower) {
                warnings.push(format!("Duplicate monster name: {}", m.name));
            }
            if m.hp <= 0 {
                warnings.push(format!(
                    "Monster '{}' HP must be > 0 (got {})",
                    m.name, m.hp
                ));
            }
            if m.spawn_weight == 0 {
                warnings.push(format!(
                    "Monster '{}' spawn_weight must be > 0 (got 0)",
                    m.name
                ));
            }
            if m.sight_radius <= 0 {
                warnings.push(format!(
                    "Monster '{}' sight_radius must be > 0 (got {})",
                    m.name, m.sight_radius
                ));
            }
            validate_glyph(&m.name, &m.glyph, &mut warnings);
            validate_color(&m.name, &m.color, &mut warnings);
            validate_ai(&m.name, &m.ai, &mut warnings);
        }

        warnings
    }

    fn validate_glyph(entity: &str, glyph: &str, warnings: &mut Vec<String>) {
        let count = glyph.chars().count();
        if count == 0 {
            warnings.push(format!("{entity} glyph is empty"));
        } else if count > 1 {
            warnings.push(format!(
                "{entity} glyph should be 1 char (got {count}: \"{glyph}\")"
            ));
        }
    }

    fn validate_color(entity: &str, color: &str, warnings: &mut Vec<String>) {
        if !KNOWN_COLORS.contains(&color) {
            warnings.push(format!(
                "{entity} has unknown color \"{color}\" (will default to White)"
            ));
        }
    }

    fn validate_ai(monster: &str, ai: &str, warnings: &mut Vec<String>) {
        if !KNOWN_AI.contains(&ai.to_lowercase().as_str()) {
            warnings.push(format!(
                "Monster '{monster}' has unknown AI \"{ai}\" (will default to None)"
            ));
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
        diff_config!(target_depth);

        // Wandering config changes.
        macro_rules! diff_wandering {
            ($field:ident) => {
                if old.wandering.$field != new.wandering.$field {
                    diffs.push(format!(
                        "wandering.{} {} -> {}",
                        stringify!($field),
                        old.wandering.$field,
                        new.wandering.$field
                    ));
                }
            };
        }
        diff_wandering!(spawn_interval);
        diff_wandering!(spawn_chance);
        diff_wandering!(grace_period);
        diff_wandering!(max_wandering);
        diff_wandering!(sound_far);
        diff_wandering!(sound_medium);
        diff_wandering!(sound_near);
        diff_wandering!(idle_threshold);
        diff_wandering!(idle_acceleration);

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
                if old_m.sight_radius != new_m.sight_radius {
                    changes.push(format!(
                        "sight {} -> {}",
                        old_m.sight_radius, new_m.sight_radius
                    ));
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
        assert_eq!(goblin().ai_personality(), AiPersonality::Aggressive);
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

    // --- Validation tests ---

    #[test]
    fn validate_defaults_has_no_warnings() {
        let data = defaults();
        let warnings = validate_game_data(data);
        assert!(
            warnings.is_empty(),
            "Expected no warnings, got: {:?}",
            warnings
        );
    }

    #[test]
    fn validate_catches_negative_hp() {
        let mut data = defaults().clone();
        data.player.hp = -1;
        let warnings = validate_game_data(&data);
        assert!(warnings.iter().any(|w| w.contains("Player HP")));
    }

    #[test]
    fn validate_catches_zero_monster_hp() {
        let mut data = defaults().clone();
        data.monsters[0].hp = 0;
        let warnings = validate_game_data(&data);
        assert!(warnings.iter().any(|w| w.contains("HP must be > 0")));
    }

    #[test]
    fn validate_catches_unknown_color() {
        let mut data = defaults().clone();
        data.player.color = "Magenta".to_string();
        let warnings = validate_game_data(&data);
        assert!(warnings.iter().any(|w| w.contains("unknown color")));
    }

    #[test]
    fn validate_catches_unknown_ai() {
        let mut data = defaults().clone();
        data.monsters[0].ai = "Sleepwalk".to_string();
        let warnings = validate_game_data(&data);
        assert!(warnings.iter().any(|w| w.contains("unknown AI")));
    }

    #[test]
    fn validate_catches_multi_char_glyph() {
        let mut data = defaults().clone();
        data.player.glyph = "@@".to_string();
        let warnings = validate_game_data(&data);
        assert!(warnings.iter().any(|w| w.contains("1 char")));
    }

    #[test]
    fn validate_catches_empty_glyph() {
        let mut data = defaults().clone();
        data.monsters[0].glyph = "".to_string();
        let warnings = validate_game_data(&data);
        assert!(warnings.iter().any(|w| w.contains("glyph is empty")));
    }

    #[test]
    fn validate_catches_room_size_inversion() {
        let mut data = defaults().clone();
        data.config.room_size_min = 12;
        data.config.room_size_max = 4;
        let warnings = validate_game_data(&data);
        assert!(warnings.iter().any(|w| w.contains("room_size_min")));
    }

    #[test]
    fn validate_catches_zero_fov_radius() {
        let mut data = defaults().clone();
        data.config.fov_radius = 0;
        let warnings = validate_game_data(&data);
        assert!(warnings.iter().any(|w| w.contains("fov_radius")));
    }

    #[test]
    fn validate_catches_zero_spawn_weight() {
        let mut data = defaults().clone();
        data.monsters[0].spawn_weight = 0;
        let warnings = validate_game_data(&data);
        assert!(warnings.iter().any(|w| w.contains("spawn_weight")));
    }

    #[test]
    fn validate_catches_duplicate_monster_names() {
        let mut data = defaults().clone();
        let dup = data.monsters[0].clone();
        data.monsters.push(dup);
        let warnings = validate_game_data(&data);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("Duplicate monster name"))
        );
    }

    #[test]
    fn validate_case_insensitive_ai() {
        let mut data = defaults().clone();
        data.monsters[0].ai = "chase".to_string();
        let warnings = validate_game_data(&data);
        // "chase" (lowercase) matches ai_personality() and should not warn.
        assert!(
            !warnings.iter().any(|w| w.contains("unknown AI")),
            "chase should be valid AI, got warnings: {:?}",
            warnings
        );
    }

    // --- sight_radius tests ---

    #[test]
    fn parse_embedded_toml_sight_radius() {
        let data = defaults();
        for m in &data.monsters {
            assert!(
                m.sight_radius > 0,
                "{} should have sight_radius > 0",
                m.name
            );
        }
        assert_eq!(goblin().sight_radius, 6);
        assert_eq!(orc().sight_radius, 7);
        assert_eq!(troll().sight_radius, 5);
    }

    #[test]
    fn sight_radius_defaults_when_missing() {
        let toml = r#"
[player]
hp = 30
attack = 5
defense = 2
glyph = "@"
color = "Yellow"

[config]
fov_radius = 8
max_rooms = 30
room_size_min = 4
room_size_max = 10
max_monsters_per_room = 2
ui_bottom_rows = 5
max_autorun_steps = 100
regen_interval = 3

[[monsters]]
name = "Bat"
glyph = "b"
color = "Grey"
hp = 2
attack = 1
defense = 0
ai = "Chase"
spawn_weight = 100
"#;
        let data = parse_game_data(toml).unwrap();
        assert_eq!(data.monsters[0].sight_radius, 8); // default
    }

    #[test]
    fn validate_catches_zero_sight_radius() {
        let mut data = defaults().clone();
        data.monsters[0].sight_radius = 0;
        let warnings = validate_game_data(&data);
        assert!(warnings.iter().any(|w| w.contains("sight_radius")));
    }

    #[test]
    fn diff_detects_sight_radius_change() {
        let old = defaults().clone();
        let mut new = old.clone();
        if let Some(goblin) = new.monsters.iter_mut().find(|m| m.name == "Goblin") {
            goblin.sight_radius = 10;
        }
        let diffs = diff_game_data(&old, &new);
        assert!(diffs.iter().any(|d| d.contains("sight 6 -> 10")));
    }

    // --- Balance constants verification ---
    // Ensures game.toml defaults stay in sync with rules::balance constants.

    #[test]
    fn game_toml_matches_balance_constants() {
        let data = defaults();

        // Player defaults
        assert_eq!(data.player.hp, balance::PLAYER_HP as Stat);
        assert_eq!(data.player.attack, balance::PLAYER_ATK as Stat);
        assert_eq!(data.player.defense, balance::PLAYER_DEF as Stat);
        assert_eq!(data.player.glyph_char(), balance::PLAYER_GLYPH);

        // Config
        assert_eq!(data.config.fov_radius, balance::FOV_RADIUS as Coord);
        assert_eq!(data.config.max_rooms, balance::MAX_ROOMS as Stat);
        assert_eq!(data.config.room_size_min, balance::ROOM_SIZE_MIN as Coord);
        assert_eq!(data.config.room_size_max, balance::ROOM_SIZE_MAX as Coord);
        assert_eq!(
            data.config.max_monsters_per_room,
            balance::MAX_MONSTERS_PER_ROOM as Stat
        );
        assert_eq!(data.config.ui_bottom_rows, balance::UI_BOTTOM_ROWS as Stat);
        assert_eq!(
            data.config.max_autorun_steps,
            balance::MAX_AUTORUN_STEPS as Stat
        );
        assert_eq!(data.config.regen_interval, balance::REGEN_INTERVAL as Stat);
        assert_eq!(data.config.target_depth, balance::TARGET_DEPTH as Stat);

        // Depth scaling
        assert_eq!(
            data.depth_scaling.monster_hp_per_floor,
            balance::MONSTER_HP_PER_FLOOR as Stat
        );
        assert_eq!(
            data.depth_scaling.monster_atk_per_floor,
            balance::MONSTER_ATK_PER_FLOOR as Stat
        );

        // Wandering config
        assert_eq!(
            data.wandering.spawn_interval,
            balance::WANDERING_SPAWN_INTERVAL as Stat
        );
        assert_eq!(
            data.wandering.spawn_chance,
            balance::WANDERING_SPAWN_CHANCE as Stat
        );
        assert_eq!(
            data.wandering.grace_period,
            balance::WANDERING_GRACE_PERIOD as Stat
        );
        assert_eq!(
            data.wandering.max_wandering,
            balance::WANDERING_MAX_ACTIVE as Stat
        );
        assert_eq!(
            data.wandering.sound_far,
            balance::WANDERING_SOUND_FAR as Coord
        );
        assert_eq!(
            data.wandering.sound_medium,
            balance::WANDERING_SOUND_MEDIUM as Coord
        );
        assert_eq!(
            data.wandering.sound_near,
            balance::WANDERING_SOUND_NEAR as Coord
        );
        assert_eq!(
            data.wandering.idle_threshold,
            balance::WANDERING_IDLE_THRESHOLD as Stat
        );
        assert_eq!(
            data.wandering.idle_acceleration,
            balance::WANDERING_IDLE_ACCELERATION as Stat
        );

        // Monster stats
        let g = goblin();
        assert_eq!(g.hp, balance::GOBLIN_HP as Stat);
        assert_eq!(g.attack, balance::GOBLIN_ATK as Stat);
        assert_eq!(g.defense, balance::GOBLIN_DEF as Stat);
        assert_eq!(g.sight_radius, balance::GOBLIN_SIGHT as Coord);
        assert_eq!(g.spawn_weight, balance::GOBLIN_SPAWN_WEIGHT as u32);
        assert_eq!(g.glyph_char(), balance::GOBLIN_GLYPH);
        assert_eq!(g.monster_kind(), Some(MonsterKind::Goblin));

        let o = orc();
        assert_eq!(o.hp, balance::ORC_HP as Stat);
        assert_eq!(o.attack, balance::ORC_ATK as Stat);
        assert_eq!(o.defense, balance::ORC_DEF as Stat);
        assert_eq!(o.sight_radius, balance::ORC_SIGHT as Coord);
        assert_eq!(o.spawn_weight, balance::ORC_SPAWN_WEIGHT as u32);
        assert_eq!(o.glyph_char(), balance::ORC_GLYPH);
        assert_eq!(o.monster_kind(), Some(MonsterKind::Orc));

        let t = troll();
        assert_eq!(t.hp, balance::TROLL_HP as Stat);
        assert_eq!(t.attack, balance::TROLL_ATK as Stat);
        assert_eq!(t.defense, balance::TROLL_DEF as Stat);
        assert_eq!(t.sight_radius, balance::TROLL_SIGHT as Coord);
        assert_eq!(t.spawn_weight, balance::TROLL_SPAWN_WEIGHT as u32);
        assert_eq!(t.glyph_char(), balance::TROLL_GLYPH);
        assert_eq!(t.monster_kind(), Some(MonsterKind::Troll));
    }
}
