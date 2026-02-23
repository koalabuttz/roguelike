//! Combat analytics and balance testing infrastructure.
//!
//! Tracks what happens during a game by snapshotting entity state before each
//! `step()` and diffing afterward. This "observe from outside" pattern avoids
//! modifying core game logic while capturing every HP change and kill.
//!
//! All functions are free functions — no methods added to `GameState`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::game::GameState;
use crate::map::Tile;
use crate::types::Stat;

/// A single combat event inferred from entity state changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatEvent {
    pub turn: Stat,
    /// Entity that dealt damage (inferred: the one that didn't lose HP).
    pub attacker_name: String,
    /// Entity that received damage.
    pub defender_name: String,
    pub damage: Stat,
    pub defender_killed: bool,
}

/// Per-game analytics collected via snapshot/diff during play.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameAnalytics {
    pub kills_by_type: HashMap<String, Stat>,
    /// Damage the player dealt TO each monster type.
    pub damage_dealt_by_type: HashMap<String, Stat>,
    /// Damage the player took FROM each monster type.
    pub damage_taken_by_type: HashMap<String, Stat>,
    pub final_hp: Stat,
    pub explored_pct: Stat,
    pub first_kill_turn: Option<Stat>,
    pub last_kill_turn: Option<Stat>,
    pub monsters_spawned: Stat,
    pub combat_log: Vec<CombatEvent>,
    pub turns: Stat,
    pub game_over: bool,
    pub seed: u64,
}

/// Create a fresh analytics tracker for a game with the given seed.
pub fn new_analytics(seed: u64) -> GameAnalytics {
    GameAnalytics {
        kills_by_type: HashMap::new(),
        damage_dealt_by_type: HashMap::new(),
        damage_taken_by_type: HashMap::new(),
        final_hp: 0,
        explored_pct: 0,
        first_kill_turn: None,
        last_kill_turn: None,
        monsters_spawned: 0,
        combat_log: Vec::new(),
        turns: 0,
        game_over: false,
        seed,
    }
}

/// Build a minimal `GameAnalytics` from a finished `GameState`.
///
/// Populates the fields needed for `aggregate()` (turns, kills, HP,
/// explored_pct, win/loss) without per-turn combat tracking. The
/// damage-by-type and combat_log fields remain empty.
///
/// Use this when you need aggregate sweep/batch stats but don't need
/// per-fight detail — it avoids the per-turn snapshot/diff overhead
/// of `run_single_game_tracked`.
pub fn from_game_state(gs: &GameState, seed: u64) -> GameAnalytics {
    let mut kills_by_type: HashMap<String, Stat> = HashMap::new();
    for entity in gs.entities.iter().skip(1) {
        if !entity.alive {
            *kills_by_type.entry(entity.name.clone()).or_insert(0) += 1;
        }
    }

    let floor_count = gs.map.tiles.iter().filter(|t| **t == Tile::Floor).count() as Stat;
    let explored_floors = gs
        .explored
        .iter()
        .filter(|&&(x, y)| gs.map.in_bounds(x, y) && gs.map.tiles[gs.map.idx(x, y)] == Tile::Floor)
        .count() as Stat;
    let explored_pct = if floor_count > 0 {
        (explored_floors * 100) / floor_count
    } else {
        0
    };

    GameAnalytics {
        kills_by_type,
        damage_dealt_by_type: HashMap::new(),
        damage_taken_by_type: HashMap::new(),
        final_hp: gs.entities[0].hp,
        explored_pct,
        first_kill_turn: None,
        last_kill_turn: None,
        monsters_spawned: (gs.entities.len() - 1) as Stat,
        combat_log: Vec::new(),
        turns: gs.turn_count,
        game_over: gs.game_over,
        seed,
    }
}

/// Snapshot of a single entity's state for diffing.
///
/// Captures (name, hp, alive) for each entity at a point in time.
pub fn snapshot_entities(gs: &GameState) -> Vec<(String, Stat, bool)> {
    gs.entities
        .iter()
        .map(|e| (e.name.clone(), e.hp, e.alive))
        .collect()
}

/// Compare entity snapshots before/after a step to detect combat events.
///
/// Examines every entity pair: if an entity's HP decreased, we record a
/// `CombatEvent`. For player HP loss, the attacker is inferred as the nearest
/// monster that acted. For monster HP loss, the attacker is the player.
pub fn diff_combat(
    before: &[(String, Stat, bool)],
    gs: &GameState,
    turn: Stat,
    analytics: &mut GameAnalytics,
) {
    for (i, (name, hp_before, alive_before)) in before.iter().enumerate() {
        if i >= gs.entities.len() {
            continue;
        }
        let entity = &gs.entities[i];
        let hp_after = entity.hp;
        let alive_after = entity.alive;

        // Detect HP loss.
        if hp_after < *hp_before {
            let damage = hp_before - hp_after;
            let killed = *alive_before && !alive_after;

            if i == 0 {
                // Player took damage — attacker is some monster.
                // We attribute to "unknown" since multiple monsters could act.
                // In practice, the message log has details, but for analytics
                // we track aggregate damage by type.
                let attacker_name = find_likely_attacker(gs);
                analytics.combat_log.push(CombatEvent {
                    turn,
                    attacker_name: attacker_name.clone(),
                    defender_name: name.clone(),
                    damage,
                    defender_killed: killed,
                });
                *analytics
                    .damage_taken_by_type
                    .entry(attacker_name)
                    .or_insert(0) += damage;
            } else {
                // Monster took damage — player attacked it.
                analytics.combat_log.push(CombatEvent {
                    turn,
                    attacker_name: "Player".to_string(),
                    defender_name: name.clone(),
                    damage,
                    defender_killed: killed,
                });
                *analytics
                    .damage_dealt_by_type
                    .entry(name.clone())
                    .or_insert(0) += damage;

                if killed {
                    *analytics.kills_by_type.entry(name.clone()).or_insert(0) += 1;
                    if analytics.first_kill_turn.is_none() {
                        analytics.first_kill_turn = Some(turn);
                    }
                    analytics.last_kill_turn = Some(turn);
                }
            }
        }
    }
}

/// Best-effort guess at which monster attacked the player.
///
/// Picks the nearest alive monster. This is imperfect when multiple monsters
/// are adjacent, but sufficient for analytics aggregation.
fn find_likely_attacker(gs: &GameState) -> String {
    let px = gs.entities[0].x;
    let py = gs.entities[0].y;
    gs.entities
        .iter()
        .skip(1)
        .filter(|e| e.alive)
        .min_by_key(|e| (e.x - px).abs() + (e.y - py).abs())
        .map(|e| e.name.clone())
        .unwrap_or_else(|| "Unknown".to_string())
}

/// Fill in final summary fields after the game ends.
pub fn finalize_analytics(analytics: &mut GameAnalytics, gs: &GameState) {
    analytics.final_hp = gs.entities[0].hp;
    analytics.turns = gs.turn_count;
    analytics.game_over = gs.game_over;
    analytics.monsters_spawned = (gs.entities.len() - 1) as Stat;

    let floor_count = gs.map.tiles.iter().filter(|t| **t == Tile::Floor).count() as Stat;
    let explored_floors = gs
        .explored
        .iter()
        .filter(|&&(x, y)| gs.map.in_bounds(x, y) && gs.map.tiles[gs.map.idx(x, y)] == Tile::Floor)
        .count() as Stat;
    analytics.explored_pct = if floor_count > 0 {
        (explored_floors * 100) / floor_count
    } else {
        0
    };
}

/// Aggregated statistics across multiple game runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedBatchStats {
    pub games: Stat,
    pub win_rate: f64,
    pub avg_turns: f64,
    pub avg_kills: f64,
    pub avg_hp_remaining: f64,
    pub avg_explored_pct: f64,
    pub kills_by_type: HashMap<String, f64>,
    pub damage_dealt_by_type: HashMap<String, f64>,
    pub damage_taken_by_type: HashMap<String, f64>,
    pub avg_first_kill_turn: Option<f64>,
}

/// Compute aggregate statistics across a batch of game analytics.
pub fn aggregate(games: &[GameAnalytics]) -> EnhancedBatchStats {
    let n = games.len() as f64;
    if n == 0.0 {
        return EnhancedBatchStats {
            games: 0,
            win_rate: 0.0,
            avg_turns: 0.0,
            avg_kills: 0.0,
            avg_hp_remaining: 0.0,
            avg_explored_pct: 0.0,
            kills_by_type: HashMap::new(),
            damage_dealt_by_type: HashMap::new(),
            damage_taken_by_type: HashMap::new(),
            avg_first_kill_turn: None,
        };
    }

    let wins = games.iter().filter(|g| !g.game_over).count() as f64;
    let total_turns: Stat = games.iter().map(|g| g.turns).sum();
    let total_kills: Stat = games
        .iter()
        .map(|g| g.kills_by_type.values().sum::<Stat>())
        .sum();
    let total_hp: Stat = games.iter().map(|g| g.final_hp).sum();
    let total_explored: Stat = games.iter().map(|g| g.explored_pct).sum();

    // Per-type aggregates.
    let mut kills_by_type: HashMap<String, f64> = HashMap::new();
    let mut damage_dealt_by_type: HashMap<String, f64> = HashMap::new();
    let mut damage_taken_by_type: HashMap<String, f64> = HashMap::new();

    for g in games {
        for (k, v) in &g.kills_by_type {
            *kills_by_type.entry(k.clone()).or_insert(0.0) += *v as f64;
        }
        for (k, v) in &g.damage_dealt_by_type {
            *damage_dealt_by_type.entry(k.clone()).or_insert(0.0) += *v as f64;
        }
        for (k, v) in &g.damage_taken_by_type {
            *damage_taken_by_type.entry(k.clone()).or_insert(0.0) += *v as f64;
        }
    }
    for v in kills_by_type.values_mut() {
        *v /= n;
    }
    for v in damage_dealt_by_type.values_mut() {
        *v /= n;
    }
    for v in damage_taken_by_type.values_mut() {
        *v /= n;
    }

    let first_kills: Vec<f64> = games
        .iter()
        .filter_map(|g| g.first_kill_turn.map(|t| t as f64))
        .collect();
    let avg_first_kill_turn = if first_kills.is_empty() {
        None
    } else {
        Some(first_kills.iter().sum::<f64>() / first_kills.len() as f64)
    };

    EnhancedBatchStats {
        games: games.len() as Stat,
        win_rate: wins / n,
        avg_turns: total_turns as f64 / n,
        avg_kills: total_kills as f64 / n,
        avg_hp_remaining: total_hp as f64 / n,
        avg_explored_pct: total_explored as f64 / n,
        kills_by_type,
        damage_dealt_by_type,
        damage_taken_by_type,
        avg_first_kill_turn,
    }
}

// ---------------------------------------------------------------------------
// Phase 5: Combat Analysis Functions
// ---------------------------------------------------------------------------

/// Per-preset difficulty summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetDifficulty {
    pub preset_name: String,
    pub win_rate: f64,
    pub avg_turns: f64,
    pub avg_kills: f64,
    pub most_dangerous_monster: Option<String>,
}

/// Compute difficulty metrics for a named preset from its game runs.
pub fn preset_difficulty(preset_name: &str, games: &[GameAnalytics]) -> PresetDifficulty {
    let stats = aggregate(games);

    // Most dangerous = monster type that dealt the most total damage to player.
    let most_dangerous = stats
        .damage_taken_by_type
        .iter()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(name, _)| name.clone());

    PresetDifficulty {
        preset_name: preset_name.to_string(),
        win_rate: stats.win_rate,
        avg_turns: stats.avg_turns,
        avg_kills: stats.avg_kills,
        most_dangerous_monster: most_dangerous,
    }
}

/// Per-monster-type correlation with player death.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonsterCorrelation {
    pub monster_type: String,
    /// Fraction of games where player died after encountering this type.
    pub death_rate_when_encountered: f64,
    /// Average damage this type dealt to the player per game (when present).
    pub avg_damage_dealt: f64,
}

/// Compute death correlations for each monster type.
pub fn monster_correlations(games: &[GameAnalytics]) -> Vec<MonsterCorrelation> {
    // Collect all monster types seen across all games.
    let mut all_types: HashMap<String, (Stat, Stat, Stat)> = HashMap::new(); // (encounters, deaths, total_damage)

    for g in games {
        let encountered: std::collections::HashSet<&String> = g
            .damage_dealt_by_type
            .keys()
            .chain(g.damage_taken_by_type.keys())
            .collect();

        for monster in &encountered {
            let entry = all_types.entry((*monster).clone()).or_insert((0, 0, 0));
            entry.0 += 1; // encountered
            if g.game_over {
                entry.1 += 1; // died
            }
            entry.2 += g.damage_taken_by_type.get(*monster).copied().unwrap_or(0);
        }
    }

    all_types
        .into_iter()
        .map(
            |(monster_type, (encounters, deaths, total_damage))| MonsterCorrelation {
                monster_type,
                death_rate_when_encountered: if encounters > 0 {
                    deaths as f64 / encounters as f64
                } else {
                    0.0
                },
                avg_damage_dealt: if encounters > 0 {
                    total_damage as f64 / encounters as f64
                } else {
                    0.0
                },
            },
        )
        .collect()
}

/// A single entry in the damage flow: total damage from one entity type to another.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DamageFlowEntry {
    pub attacker: String,
    pub defender: String,
    pub total_damage: Stat,
}

/// Aggregate damage flow between entity types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DamageFlow {
    pub flows: Vec<DamageFlowEntry>,
}

/// Compute the total damage flow from combat logs across all games.
pub fn damage_flow(games: &[GameAnalytics]) -> DamageFlow {
    let mut map: HashMap<(String, String), Stat> = HashMap::new();

    for g in games {
        for event in &g.combat_log {
            *map.entry((event.attacker_name.clone(), event.defender_name.clone()))
                .or_insert(0) += event.damage;
        }
    }

    let mut flows: Vec<DamageFlowEntry> = map
        .into_iter()
        .map(|((attacker, defender), total_damage)| DamageFlowEntry {
            attacker,
            defender,
            total_damage,
        })
        .collect();
    flows.sort_by(|a, b| b.total_damage.cmp(&a.total_damage));

    DamageFlow { flows }
}

/// Sweep configuration for parameter sweep runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweepConfig {
    pub axes: Vec<SweepAxis>,
    pub games_per_point: Stat,
    pub width: crate::types::Coord,
    pub height: crate::types::Coord,
    pub max_turns: Stat,
    pub preset: Option<crate::map::MapPreset>,
}

/// A single axis of a parameter sweep.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweepAxis {
    pub param: String,
    pub values: Vec<Stat>,
}

/// Overrides for game configuration during sweep runs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigOverrides {
    pub player_hp: Option<Stat>,
    pub player_attack: Option<Stat>,
    pub player_defense: Option<Stat>,
    pub regen_interval: Option<Stat>,
    pub max_monsters_per_room: Option<Stat>,
}

/// Apply config overrides to a game state after creation.
pub fn apply_overrides(gs: &mut GameState, overrides: &ConfigOverrides) {
    if let Some(hp) = overrides.player_hp {
        gs.entities[0].hp = hp;
        gs.entities[0].max_hp = hp;
    }
    if let Some(atk) = overrides.player_attack {
        gs.entities[0].attack = atk;
    }
    if let Some(def) = overrides.player_defense {
        gs.entities[0].defense = def;
    }
    if let Some(regen) = overrides.regen_interval {
        gs.regen_interval = regen;
    }
}

/// Result of a single sweep data point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweepPoint {
    pub overrides: ConfigOverrides,
    pub stats: EnhancedBatchStats,
}

/// Generate all override combinations from sweep axes.
pub fn sweep_combinations(config: &SweepConfig) -> Vec<ConfigOverrides> {
    if config.axes.is_empty() {
        return vec![ConfigOverrides::default()];
    }

    let mut combos = vec![ConfigOverrides::default()];

    for axis in &config.axes {
        let mut new_combos = Vec::new();
        for base in &combos {
            for &val in &axis.values {
                let mut combo = base.clone();
                match axis.param.as_str() {
                    "player_hp" => combo.player_hp = Some(val),
                    "player_attack" => combo.player_attack = Some(val),
                    "player_defense" => combo.player_defense = Some(val),
                    "regen_interval" => combo.regen_interval = Some(val),
                    "max_monsters_per_room" => combo.max_monsters_per_room = Some(val),
                    _ => {} // Unknown param — ignore.
                }
                new_combos.push(combo);
            }
        }
        combos = new_combos;
    }

    combos
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::Entity;
    use crate::fov;
    use crate::map::{Map, Tile};
    use crate::message_log::MessageLog;
    use crate::{command::GameCommand, data};

    fn test_game() -> GameState {
        let mut m = Map::new(20, 20);
        for y in 1..=10 {
            for x in 1..=10 {
                let idx = m.idx(x, y);
                m.tiles[idx] = Tile::Floor;
            }
        }
        let player = Entity::player(5, 5);
        let visible = fov::compute_fov(&m, 5, 5, 8);
        let explored = visible.clone();
        GameState {
            map: m,
            entities: vec![player],
            fov_radius: 8,
            visible,
            explored,
            log: MessageLog::new(),
            game_over: false,
            turn_count: 0,
            seed: 42,
            preset: None,
            dirty: false,
            regen_interval: data::config().regen_interval,
            max_autorun_steps: data::config().max_autorun_steps,
            wandering_seed: 0,
            wandering_config: Default::default(),
            idle_count: 0,
            wandering_spawned: 0,
            wandering_spawn_table: Vec::new(),
            ground_items: Vec::new(),
            equipment: Default::default(),
            depth: 1,
            target_depth: 5,
            game_won: false,
            depth_scaling: Default::default(),
            max_rooms: 30,
            room_size_min: 4,
            room_size_max: 10,
            max_monsters_per_room: 2,
        }
    }

    #[test]
    fn new_analytics_has_correct_seed() {
        let a = new_analytics(42);
        assert_eq!(a.seed, 42);
        assert!(a.combat_log.is_empty());
        assert!(a.kills_by_type.is_empty());
    }

    #[test]
    fn snapshot_captures_all_entities() {
        let mut gs = test_game();
        gs.entities
            .push(Entity::from_template(data::goblin(), 3, 3));
        let snap = snapshot_entities(&gs);
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].0, "Player");
        assert_eq!(snap[1].0, "Goblin");
    }

    #[test]
    fn diff_detects_monster_damage() {
        let mut gs = test_game();
        gs.entities
            .push(Entity::from_template(data::goblin(), 6, 5));
        let before = snapshot_entities(&gs);

        // Player attacks goblin (step also runs monster turn, so both take damage).
        gs.step(GameCommand::Move { dx: 1, dy: 0 });

        let mut analytics = new_analytics(42);
        diff_combat(&before, &gs, 1, &mut analytics);

        // Find the event where player attacked the goblin.
        // (Entity 0 is processed first, so goblin's counterattack may appear first.)
        let goblin_hit = analytics
            .combat_log
            .iter()
            .find(|e| e.defender_name == "Goblin")
            .expect("expected an event where goblin took damage");
        assert_eq!(goblin_hit.attacker_name, "Player");
        assert!(goblin_hit.damage > 0);
    }

    #[test]
    fn diff_detects_player_damage() {
        let mut gs = test_game();
        // Place goblin adjacent so it attacks on monster turn.
        gs.entities
            .push(Entity::from_template(data::goblin(), 6, 5));
        gs.update_fov();

        let before = snapshot_entities(&gs);
        gs.step(GameCommand::Wait);

        let mut analytics = new_analytics(42);
        diff_combat(&before, &gs, 1, &mut analytics);

        // Player should have taken damage from goblin.
        let player_damage: Vec<_> = analytics
            .combat_log
            .iter()
            .filter(|e| e.defender_name == "Player")
            .collect();
        assert!(!player_damage.is_empty());
    }

    #[test]
    fn diff_detects_kill() {
        let mut gs = test_game();
        gs.entities[0].attack = 100; // one-shot everything
        gs.entities
            .push(Entity::from_template(data::goblin(), 6, 5));

        let before = snapshot_entities(&gs);
        gs.step(GameCommand::Move { dx: 1, dy: 0 });

        let mut analytics = new_analytics(42);
        diff_combat(&before, &gs, 1, &mut analytics);

        assert_eq!(*analytics.kills_by_type.get("Goblin").unwrap_or(&0), 1);
        assert_eq!(analytics.first_kill_turn, Some(1));
    }

    #[test]
    fn finalize_sets_summary_fields() {
        let gs = test_game();
        let mut analytics = new_analytics(42);
        finalize_analytics(&mut analytics, &gs);

        assert_eq!(analytics.final_hp, 30);
        assert_eq!(analytics.turns, 0);
        assert!(!analytics.game_over);
        assert!(analytics.explored_pct > 0);
    }

    #[test]
    fn from_game_state_captures_kills_and_stats() {
        let mut gs = test_game();
        gs.entities
            .push(Entity::from_template(data::goblin(), 3, 3));
        gs.entities.push(Entity::from_template(data::orc(), 4, 4));
        // Kill the goblin.
        gs.entities[1].alive = false;
        gs.entities[1].hp = 0;
        gs.turn_count = 42;

        let ga = from_game_state(&gs, 99);
        assert_eq!(ga.seed, 99);
        assert_eq!(ga.turns, 42);
        assert!(!ga.game_over);
        assert_eq!(ga.final_hp, 30);
        assert_eq!(ga.monsters_spawned, 2);
        assert_eq!(*ga.kills_by_type.get("Goblin").unwrap_or(&0), 1);
        assert_eq!(*ga.kills_by_type.get("Orc").unwrap_or(&0), 0);
        // No per-turn tracking — combat log and damage maps are empty.
        assert!(ga.combat_log.is_empty());
        assert!(ga.damage_dealt_by_type.is_empty());
        assert!(ga.damage_taken_by_type.is_empty());
    }

    #[test]
    fn from_game_state_produces_valid_aggregate() {
        // Verify aggregate() works correctly with minimal analytics
        // (no combat log, no damage maps — just headline stats).
        let mut gs = test_game();
        gs.entities
            .push(Entity::from_template(data::goblin(), 3, 3));
        gs.entities[1].alive = false;
        gs.entities[1].hp = 0;
        gs.turn_count = 100;

        let ga = from_game_state(&gs, 1);
        let stats = aggregate(&[ga]);
        assert_eq!(stats.games, 1);
        assert_eq!(stats.win_rate, 1.0); // not game_over
        assert_eq!(stats.avg_turns, 100.0);
        assert_eq!(stats.avg_kills, 1.0);
        assert_eq!(stats.avg_hp_remaining, 30.0);
    }

    #[test]
    fn aggregate_empty_games() {
        let stats = aggregate(&[]);
        assert_eq!(stats.games, 0);
        assert_eq!(stats.win_rate, 0.0);
    }

    #[test]
    fn aggregate_single_game() {
        let mut analytics = new_analytics(42);
        analytics.turns = 100;
        analytics.game_over = false;
        analytics.final_hp = 20;
        analytics.explored_pct = 50;
        analytics.kills_by_type.insert("Goblin".to_string(), 3);

        let stats = aggregate(&[analytics]);
        assert_eq!(stats.games, 1);
        assert_eq!(stats.win_rate, 1.0);
        assert_eq!(stats.avg_turns, 100.0);
        assert_eq!(stats.avg_kills, 3.0);
        assert_eq!(stats.avg_hp_remaining, 20.0);
    }

    #[test]
    fn aggregate_multiple_games() {
        let mut a1 = new_analytics(1);
        a1.turns = 100;
        a1.game_over = false;
        a1.kills_by_type.insert("Goblin".to_string(), 2);

        let mut a2 = new_analytics(2);
        a2.turns = 200;
        a2.game_over = true;
        a2.kills_by_type.insert("Goblin".to_string(), 4);

        let stats = aggregate(&[a1, a2]);
        assert_eq!(stats.games, 2);
        assert_eq!(stats.win_rate, 0.5);
        assert_eq!(stats.avg_turns, 150.0);
        assert_eq!(stats.avg_kills, 3.0);
    }

    #[test]
    fn preset_difficulty_computes() {
        let mut a = new_analytics(42);
        a.turns = 100;
        a.game_over = false;
        a.kills_by_type.insert("Troll".to_string(), 1);
        a.damage_taken_by_type.insert("Troll".to_string(), 15);

        let diff = preset_difficulty("arena", &[a]);
        assert_eq!(diff.preset_name, "arena");
        assert_eq!(diff.win_rate, 1.0);
        assert_eq!(diff.most_dangerous_monster, Some("Troll".to_string()));
    }

    #[test]
    fn monster_correlations_computes() {
        let mut a1 = new_analytics(1);
        a1.game_over = true;
        a1.damage_taken_by_type.insert("Troll".to_string(), 20);
        a1.damage_dealt_by_type.insert("Troll".to_string(), 10);

        let mut a2 = new_analytics(2);
        a2.game_over = false;
        a2.damage_taken_by_type.insert("Goblin".to_string(), 5);
        a2.damage_dealt_by_type.insert("Goblin".to_string(), 6);

        let corrs = monster_correlations(&[a1, a2]);
        assert!(!corrs.is_empty());

        let troll = corrs.iter().find(|c| c.monster_type == "Troll").unwrap();
        assert_eq!(troll.death_rate_when_encountered, 1.0);
        assert_eq!(troll.avg_damage_dealt, 20.0);
    }

    #[test]
    fn damage_flow_computes() {
        let mut a = new_analytics(42);
        a.combat_log.push(CombatEvent {
            turn: 1,
            attacker_name: "Player".to_string(),
            defender_name: "Goblin".to_string(),
            damage: 5,
            defender_killed: false,
        });
        a.combat_log.push(CombatEvent {
            turn: 2,
            attacker_name: "Goblin".to_string(),
            defender_name: "Player".to_string(),
            damage: 1,
            defender_killed: false,
        });

        let flow = damage_flow(&[a]);
        let player_to_goblin = flow
            .flows
            .iter()
            .find(|e| e.attacker == "Player" && e.defender == "Goblin")
            .expect("expected Player->Goblin flow");
        assert_eq!(player_to_goblin.total_damage, 5);

        let goblin_to_player = flow
            .flows
            .iter()
            .find(|e| e.attacker == "Goblin" && e.defender == "Player")
            .expect("expected Goblin->Player flow");
        assert_eq!(goblin_to_player.total_damage, 1);
    }

    #[test]
    fn sweep_combinations_empty_axes() {
        let config = SweepConfig {
            axes: vec![],
            games_per_point: 1,
            width: 20,
            height: 20,
            max_turns: 100,
            preset: None,
        };
        let combos = sweep_combinations(&config);
        assert_eq!(combos.len(), 1);
    }

    #[test]
    fn sweep_combinations_single_axis() {
        let config = SweepConfig {
            axes: vec![SweepAxis {
                param: "player_hp".to_string(),
                values: vec![10, 20, 30],
            }],
            games_per_point: 1,
            width: 20,
            height: 20,
            max_turns: 100,
            preset: None,
        };
        let combos = sweep_combinations(&config);
        assert_eq!(combos.len(), 3);
        assert_eq!(combos[0].player_hp, Some(10));
        assert_eq!(combos[1].player_hp, Some(20));
        assert_eq!(combos[2].player_hp, Some(30));
    }

    #[test]
    fn sweep_combinations_two_axes() {
        let config = SweepConfig {
            axes: vec![
                SweepAxis {
                    param: "player_hp".to_string(),
                    values: vec![10, 20],
                },
                SweepAxis {
                    param: "player_attack".to_string(),
                    values: vec![3, 5],
                },
            ],
            games_per_point: 1,
            width: 20,
            height: 20,
            max_turns: 100,
            preset: None,
        };
        let combos = sweep_combinations(&config);
        assert_eq!(combos.len(), 4); // 2 x 2
    }

    #[test]
    fn apply_overrides_sets_stats() {
        let mut gs = test_game();
        let overrides = ConfigOverrides {
            player_hp: Some(50),
            player_attack: Some(10),
            player_defense: Some(5),
            ..Default::default()
        };
        apply_overrides(&mut gs, &overrides);
        assert_eq!(gs.entities[0].hp, 50);
        assert_eq!(gs.entities[0].max_hp, 50);
        assert_eq!(gs.entities[0].attack, 10);
        assert_eq!(gs.entities[0].defense, 5);
    }
}
