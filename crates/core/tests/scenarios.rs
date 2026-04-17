//! Balance integration tests using the scenario framework.
//!
//! These tests verify game balance properties — e.g., that certain monster
//! configurations are survivable or lethal as expected.

use roguelike_core::map::MapPreset;
use roguelike_core::scenario::Scenario;

// --- Survivability tests ---

#[test]
fn default_player_kills_goblin() {
    // Player (ATK=5, DEF=2, HP=30) vs Goblin (ATK=3, DEF=0, HP=6).
    // Player deals 5 dmg/hit, kills in 2 hits. Takes 1 dmg/hit.
    Scenario::new(20, 20, 42)
        .preset(MapPreset::SingleRoom)
        .kill_all()
        .spawn("goblin", 4, 5)
        .run_turns(50)
        .assert_alive()
        .assert_kills(1);
}

#[test]
fn default_player_kills_orc() {
    // Player (ATK=5, DEF=2) vs Orc (ATK=4, DEF=1, HP=12).
    // Player deals 4 dmg/hit, kills in 3 hits. Takes 2 dmg/hit.
    Scenario::new(20, 20, 42)
        .preset(MapPreset::SingleRoom)
        .kill_all()
        .spawn("orc", 4, 5)
        .run_turns(50)
        .assert_alive()
        .assert_kills(1);
}

#[test]
fn weak_player_dies_to_troll() {
    // Player with 5 HP and 0 defense vs Troll (ATK=6, DEF=3, HP=20).
    Scenario::new(20, 20, 42)
        .preset(MapPreset::SingleRoom)
        .kill_all()
        .set_player_hp(5)
        .set_player_defense(0)
        .spawn("troll", 4, 5)
        .run_turns(50)
        .assert_dead();
}

#[test]
fn god_mode_is_invulnerable() {
    // Even with 1 HP, god mode prevents death.
    Scenario::new(20, 20, 42)
        .preset(MapPreset::Arena)
        .god_mode()
        .set_player_hp(1)
        .run_turns(100)
        .assert_alive();
}

#[test]
fn no_monsters_always_survives() {
    Scenario::new(20, 20, 42)
        .preset(MapPreset::SingleRoom)
        .kill_all()
        .disable_wandering()
        .run_turns(50)
        .assert_alive()
        .assert_kills(0)
        .assert_turns(50);
}

// --- Auto-fight resolution tests ---

#[test]
fn auto_fight_goblin_quick_kill() {
    // Strong player should kill goblin in minimal turns.
    Scenario::new(20, 20, 42)
        .preset(MapPreset::SingleRoom)
        .kill_all()
        .set_player_attack(100)
        .spawn("goblin", 4, 5)
        .run_auto_fight(50)
        .assert_alive()
        .assert_kills(1)
        .assert_turns_less_than(5);
}

#[test]
fn multiple_goblins_survivable() {
    // Player should survive 2 goblins with default stats.
    Scenario::new(20, 20, 42)
        .preset(MapPreset::SingleRoom)
        .kill_all()
        .spawn("goblin", 4, 5)
        .spawn("goblin", 6, 5)
        .run_turns(100)
        .assert_alive();
}

// --- Wandering monster tests ---

#[test]
fn wandering_monsters_spawn_after_grace_period() {
    // Run long enough past grace period (50) for spawns to occur.
    let result = Scenario::new(80, 40, 42).kill_all().run_turns(200);

    // At least 1 wanderer should have spawned.
    assert!(
        result.gs.wandering_spawned > 0,
        "Expected at least 1 wandering spawn after 200 turns, got {}",
        result.gs.wandering_spawned,
    );
}

#[test]
fn no_wandering_during_grace_period() {
    // Run only 30 turns — well within the 50-turn grace period.
    let result = Scenario::new(80, 40, 42).kill_all().run_turns(30);

    assert_eq!(
        result.gs.wandering_spawned, 0,
        "No wanderers should spawn during grace period",
    );
}

#[test]
fn wandering_cap_respected() {
    // Run many turns; cap is 5 alive Wander-AI entities.
    let result = Scenario::new(80, 40, 42).kill_all().run_turns(500);

    let wander_alive = result
        .gs
        .entities
        .iter()
        .skip(1)
        .filter(|e| e.alive && e.ai == roguelike_core::entity::AiPersonality::Patrol)
        .count();

    assert!(
        wander_alive <= 5,
        "Expected at most 5 alive wanderers, got {}",
        wander_alive,
    );
}

#[test]
fn disable_wandering_prevents_spawns() {
    let result = Scenario::new(80, 40, 42)
        .kill_all()
        .disable_wandering()
        .run_turns(200);

    assert_eq!(
        result.gs.wandering_spawned, 0,
        "disable_wandering should prevent all spawns",
    );
}

// --- Coward personality tests ---

#[test]
fn coward_flees_when_wounded() {
    // Setup: player at Arena center, one Aggressive goblin a few tiles east
    // and one Coward goblin further east. The Coward starts at 1 HP (well
    // below the 33% flee threshold for a 6-HP goblin) so ai_mode returns Flee.
    //
    // `kill_all` only marks initial Arena-spawned monsters dead; it doesn't
    // remove them. Our two newly-spawned goblins land at the tail of the
    // entity vector, so we locate them by position rather than by fixed index.
    let result = Scenario::new(40, 20, 42)
        .preset(MapPreset::Arena)
        .kill_all()
        .disable_wandering()
        .spawn("goblin", 22, 9) // aggressive — east of player (center ~19,9)
        .spawn("goblin", 24, 9) // coward — further east
        .mutate(|gs, _| {
            // Find our spawns by exact position.
            let agg = gs
                .entities
                .iter_mut()
                .find(|e| e.alive && e.x == 22 && e.y == 9)
                .unwrap();
            agg.ai = roguelike_core::entity::AiPersonality::Aggressive;
            let cow = gs
                .entities
                .iter_mut()
                .find(|e| e.alive && e.x == 24 && e.y == 9)
                .unwrap();
            cow.ai = roguelike_core::entity::AiPersonality::Coward;
            cow.hp = 1;
        })
        .build();

    let px_before = result.gs.entities[0].x;

    let final_state = result.run_turns(1);

    // After one turn, look for any alive goblins and verify their positions.
    let px_after = final_state.gs.entities[0].x;
    assert_eq!(
        px_before, px_after,
        "player should not move on its own turn"
    );

    // Find the aggressive and coward by initial-x "bucket": closer-to-player
    // is aggressive (started at 22), further is coward (started at 24).
    let mut positions: Vec<i32> = final_state
        .gs
        .entities
        .iter()
        .skip(1)
        .filter(|e| e.alive)
        .map(|e| e.x)
        .collect();
    positions.sort();
    assert_eq!(
        positions.len(),
        2,
        "expected exactly two alive goblins, got {:?}",
        positions
    );
    let agg_x_after = positions[0];
    let cow_x_after = positions[1];

    assert!(
        agg_x_after < 22,
        "Aggressive goblin should chase west toward player; after={}",
        agg_x_after,
    );
    assert!(
        cow_x_after > 24,
        "Wounded Coward should flee east away from player; after={}",
        cow_x_after,
    );
}

#[test]
fn coward_chases_when_healthy() {
    // A Coward above its HP threshold behaves like Aggressive — chases the
    // player rather than fleeing. Full-HP Goblin: hp * 3 >= max_hp, so
    // ai_mode returns Chase, not Flee.
    let result = Scenario::new(40, 20, 42)
        .preset(MapPreset::Arena)
        .kill_all()
        .disable_wandering()
        .spawn("goblin", 22, 9)
        .mutate(|gs, _| {
            let cow = gs
                .entities
                .iter_mut()
                .find(|e| e.alive && e.x == 22 && e.y == 9)
                .unwrap();
            cow.ai = roguelike_core::entity::AiPersonality::Coward;
        })
        .build();

    let final_state = result.run_turns(1);
    let alive_monster_x = final_state
        .gs
        .entities
        .iter()
        .skip(1)
        .find(|e| e.alive)
        .map(|e| e.x)
        .expect("coward goblin should still be alive");

    assert!(
        alive_monster_x < 22,
        "Healthy Coward should chase west toward player; after={}",
        alive_monster_x,
    );
}

// --- Scenario analytics tests ---

#[test]
fn scenario_produces_analytics() {
    let result = Scenario::new(20, 20, 42)
        .preset(MapPreset::SingleRoom)
        .kill_all()
        .spawn("goblin", 4, 5)
        .set_player_attack(100)
        .run_turns(50);

    let analytics = result
        .analytics
        .as_ref()
        .expect("analytics should be present");
    assert_eq!(analytics.seed, 42);
    assert!(!analytics.game_over);
    assert!(analytics.kills_by_type.get("Goblin").copied().unwrap_or(0) >= 1);
}
