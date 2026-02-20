# Gameplay Implementation Plan

> **Status:** Proposed. Implementation plan for high-leverage gameplay features identified through codebase analysis and LLM playtest findings.

Six features prioritized by impact-to-effort ratio and downstream unlock potential. Each phase is independently shippable and testable — no phase requires a later phase to be valuable.

## Motivation

Four LLM playtest sessions ([summary](../reports/llm-playtest-summary.md)) converged on the same diagnosis:

1. **No win condition.** After clearing the dungeon, the game continues with nothing to do.
2. **Regen is too generous.** The player fully heals between every encounter — no resource tension remains.
3. **Combat is solved arithmetic.** Every encounter has exactly one correct answer. `auto_fight` perfectly matches the combat depth.
4. **No progression.** Killing monsters has no benefit beyond survival.

The MCP interface reached maturity in Session 4 (0% wasted tool calls). The bottleneck shifted from interface to content. These six features address the content gap.

## Overview

| Phase | Feature | Effort | Impact | Depends on |
|-------|---------|--------|--------|------------|
| 1 | [Wandering monsters](#phase-1-wandering-monsters) | S | Medium-High | — |
| 2 | [Stairs / multi-level dungeons](#phase-2-stairs--multi-level-dungeons) | M | High | — |
| 3 | [Items & inventory](#phase-3-items--inventory) | M-L | High | — |
| 4 | [Experience & leveling](#phase-4-experience--leveling) | M | High | Phase 2 (benefits from floor scaling) |
| 5 | [Creature mood & memory](#phase-5-creature-mood--memory) | M | Medium-High | — |
| 6 | [Property bitfields & interaction table](#phase-6-property-bitfields--interaction-table) | S-M | Medium | — |

Phases 1, 2, 3, 5, and 6 are independent and can be developed in parallel. Phase 4 benefits from Phase 2 (leveling needs something to scale against) but is not strictly blocked by it.

---

## Phase 1: Wandering Monsters

**The highest leverage-per-effort feature in the entire roadmap.**

### Problem

HP regen (1 HP / 3 turns) is too generous. The player can `wait` indefinitely between encounters to fully heal. All four playtest sessions flagged this. The game has no time pressure.

### Design

Spawn new monsters over time, pressuring the player to keep moving. This makes time a cost — waiting to heal means more enemies to fight.

#### New config in `game.toml`

```toml
[config]
# ... existing fields ...
wandering_spawn_interval = 40    # Spawn a wandering monster every N turns
wandering_spawn_delay = 60       # No wandering spawns before turn N
wandering_max_active = 5         # Cap on simultaneous wandering monsters
```

#### Spawn rules

1. Every `wandering_spawn_interval` turns (after `wandering_spawn_delay`), spawn one monster.
2. Pick a random room that is **not** the player's current room and **not** in the player's FOV.
3. Use the existing weighted spawn table from `spawn.rs` to pick the monster type.
4. Cap active wandering monsters at `wandering_max_active` to prevent unbounded growth.
5. Wandering monsters use the existing `Chase` AI — they behave identically to placed monsters once spawned.

#### Files touched

| File | Change |
|------|--------|
| `data.rs` | Add `wandering_spawn_interval`, `wandering_spawn_delay`, `wandering_max_active` to `GameConfig`. |
| `game.toml` | Add default values for the three new config fields. |
| `game.rs` | Add `wandering_spawned: Stat` field to `GameState`. Add `maybe_spawn_wanderer()` called from `step()` after `apply_regen()`. |
| `spawn.rs` | Extract weighted monster selection into a reusable `pick_monster()` function (currently inline in `spawn_monsters()`). New `spawn_wanderer()` function that picks a room and calls `pick_monster()`. |

#### Integration point

In `GameState::step()`, after the existing regen call:

```rust
pub fn step(&mut self, cmd: GameCommand) -> StepResult {
    // ... existing: handle_command, update_fov, run_monster_turns, turn_count++, apply_regen ...
    self.maybe_spawn_wanderer();
    // ...
}
```

#### MCP impact

The `observe` and `act` responses already include monster lists. Wandering monsters appear naturally. The `auto_explore` tool will stop when a new monster enters FOV (existing `MonsterSpotted` stop reason). No MCP tool changes needed.

#### Testing

- **Unit test:** `maybe_spawn_wanderer()` spawns at correct intervals, respects delay, respects cap.
- **Unit test:** Wanderer spawns outside player's current room and FOV.
- **Scenario test:** Player waiting 200 turns accumulates wandering monsters.
- **Golden replays:** Regenerate (wandering spawns will change outcomes).
- **Balance CI:** Will detect the shift automatically — expect higher monster counts and lower survival rates.

---

## Phase 2: Stairs / Multi-Level Dungeons

**Addresses the #1 playtest complaint: no win condition.**

### Design

Add descending stairs (`>`) to each floor. Entering stairs generates a new, harder floor. The game ends when the player dies or reaches a target depth (or plays indefinitely in endless mode).

#### Dungeon model

Each floor is a complete `GameState`. On descend, generate a new floor from a derived seed. On ascend, restore the previous floor from memory.

```rust
/// In game.rs or a new dungeon.rs
pub struct DungeonState {
    /// The current floor's full game state.
    pub current: GameState,
    /// Completed floors the player can return to (index 0 = floor 1).
    pub floors_above: Vec<GameState>,
    /// Current depth (1-indexed, floor 1 is the starting floor).
    pub depth: Stat,
    /// Maximum depth reached (for scoring).
    pub max_depth: Stat,
}
```

**Alternative (simpler, recommended for v1):** No ascent. One-way descent. `GameState` gains a `depth: Stat` field and `descend()` method. Previous floors are discarded. This avoids the complexity of storing multiple floors and aligns with many classic roguelikes (Brogue, early DCSS).

#### Floor generation

```rust
impl GameState {
    pub fn descend(&mut self, game_data: &GameData) {
        self.depth += 1;
        // Derive a new seed: original_seed + depth ensures determinism
        let floor_seed = self.seed.wrapping_add(self.depth as u64);
        // Generate new map, spawn monsters scaled to depth
        // Preserve player entity (HP, stats, inventory, XP)
        // Place player at the up-stairs position
    }
}
```

#### Difficulty scaling

New `game.toml` section:

```toml
[depth_scaling]
monster_hp_per_floor = 1        # +1 HP per floor
monster_atk_per_floor = 0.5     # +0.5 ATK per floor (rounded)
monster_density_per_floor = 0.1  # +10% monsters per floor
max_monster_scaling = 3.0        # Cap at 3x base stats
```

Alternatively, introduce new monster types at deeper floors by adding a `min_depth` field to `MonsterDef`:

```toml
[[monsters]]
name = "Dragon"
# ...
min_depth = 5
spawn_weight = 5
```

This is more interesting than stat scaling because it introduces qualitatively new threats.

#### Stair placement

Place `>` (down stairs) in a room far from the player start. On floors 2+, also place `<` (up stairs) at the player's spawn point. Add `Tile::StairsDown` and `Tile::StairsUp` variants:

```rust
pub enum Tile {
    Wall,
    Floor,
    StairsDown,  // '>'
    StairsUp,    // '<'
}
```

#### New config in `game.toml`

```toml
[config]
# ... existing fields ...
target_depth = 10               # Win condition: reach this floor
```

#### Files touched

| File | Change |
|------|--------|
| `map.rs` | Add `StairsDown`/`StairsUp` to `Tile` enum. `move_cost()` returns 1 for stairs. Place stairs during `generate()`. |
| `game.rs` | Add `depth: Stat` and `max_depth: Stat` to `GameState`. Add `descend()` method. Handle `>` tile interaction in `handle_command()`. |
| `data.rs` | Add depth scaling config. Optional `min_depth` field on `MonsterDef`. |
| `game.toml` | Add depth scaling defaults. |
| `entity.rs` | No changes — player entity is preserved across floors. |
| `spawn.rs` | Filter `MonsterDef` by `min_depth <= current_depth`. Apply stat scaling. |
| `command.rs` | Add `Descend` / `Ascend` to `GameCommand` (or handle via movement onto stair tiles). |

#### Rendering

TUI: Render `>` as `>` in the floor color (or a distinct color like cyan). Render `<` as `<`.

MCP: Add `depth`, `max_depth`, and `target_depth` to `GameObservation`. Stairs appear as `>`/`<` in the ASCII map.

#### Win condition

When the player descends past `target_depth`, trigger a victory screen. Add `game_won: bool` to `GameState` alongside `game_over`.

#### Testing

- **Unit test:** Stairs are placed on every generated map. Descend produces a valid new floor.
- **Unit test:** Player stats (HP, etc.) are preserved across descend.
- **Unit test:** Floor seed is deterministic (`seed + depth`).
- **Scenario test:** Player can reach floor 5 with god mode, verifying the descent chain works.
- **Golden replays:** Regenerate (stairs change map layout).
- **Invariant tests:** Add `depth >= 1` and `max_depth >= depth` to property checks.

---

## Phase 3: Items & Inventory

**The most blocking feature on the roadmap — unlocks ~4 downstream features.**

### Design

Items on the ground, a fixed-size inventory, and three item categories: consumables (potions), equipment (weapons/armor), and scrolls. This creates the resource management layer that transforms combat from solved arithmetic into meaningful decisions.

#### Item data model

```rust
// New file: crates/core/src/item.rs

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub name: String,
    pub glyph: char,
    pub color: GameColor,
    pub kind: ItemKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ItemKind {
    Potion { effect: PotionEffect },
    Equipment { slot: EquipSlot, attack_bonus: Stat, defense_bonus: Stat },
    Scroll { effect: ScrollEffect },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum PotionEffect {
    Heal(Stat),         // Restore N HP
    Strength,           // +1 ATK permanently
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum EquipSlot {
    Weapon,
    Armor,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ScrollEffect {
    MagicMapping,       // Reveal entire floor
    Teleport,           // Random teleport
}
```

#### Item definitions in `game.toml`

```toml
[[items]]
name = "Healing Potion"
glyph = "!"
color = "Red"
type = "potion"
effect = "heal"
value = 10
spawn_weight = 40
min_depth = 1

[[items]]
name = "Short Sword"
glyph = "/"
color = "Cyan"
type = "equipment"
slot = "weapon"
attack_bonus = 2
defense_bonus = 0
spawn_weight = 15
min_depth = 1
```

#### Floor items

Items on the ground live in a `HashMap` on `Map`, keyed by position:

```rust
// In map.rs or game.rs
pub struct GameState {
    // ... existing fields ...
    pub floor_items: HashMap<Pos, Vec<Item>>,
    pub inventory: Vec<Option<Item>>,  // Fixed-size, e.g., 10 slots
    pub equipped: EquippedItems,
}

pub struct EquippedItems {
    pub weapon: Option<Item>,
    pub armor: Option<Item>,
}
```

#### Item spawning

Extend `spawn.rs` or add `item_spawn.rs`. After room carving, scatter items in rooms using a weighted table (mirrors monster spawning). Config:

```toml
[config]
max_items_per_room = 1
```

#### Commands

| Command | Key | Effect |
|---------|-----|--------|
| `Pickup` | `g` or `,` | Pick up item at player's feet |
| `Inventory` | `i` | Open inventory screen |
| `Drop` | `d` | Drop selected item |
| `Use` | `u` | Use/consume selected item |
| `Equip` | `e` | Equip selected item (auto-swap if slot occupied) |

#### Combat integration

In `combat.rs`, `melee_attack()` reads effective ATK/DEF from the entity's base stats plus equipment bonuses. Add a helper:

```rust
/// Effective attack including equipment bonuses.
pub fn effective_attack(entity: &Entity, equipped: &EquippedItems) -> Stat {
    let bonus = equipped.weapon.as_ref()
        .and_then(|w| match &w.kind {
            ItemKind::Equipment { attack_bonus, .. } => Some(*attack_bonus),
            _ => None,
        })
        .unwrap_or(0);
    entity.attack + bonus
}
```

Only the player has equipment — monsters use base stats. This keeps `melee_attack()` simple: pass effective stats rather than changing the function signature.

#### Files touched

| File | Change |
|------|--------|
| New `item.rs` | `Item`, `ItemKind`, `PotionEffect`, `EquipSlot`, `ScrollEffect` types. |
| `data.rs` | Add `ItemDef` struct, `items: Vec<ItemDef>` to `GameData`. Parse from TOML. |
| `game.toml` | Add `[[items]]` table entries, `max_items_per_room` config. |
| `game.rs` | Add `floor_items`, `inventory`, `equipped` to `GameState`. Handle pickup/use/equip/drop in `handle_command()`. |
| `spawn.rs` or new `item_spawn.rs` | `spawn_items()` mirrors `spawn_monsters()`. |
| `combat.rs` | Read equipment bonuses when computing damage for the player. |
| `command.rs` | Add `Pickup`, `OpenInventory`, `Drop`, `Use`, `Equip` to `GameCommand`. |
| `map.rs` | Render items on ground (`!` for potions, `/` for weapons, `[` for armor, `?` for scrolls). |

#### MCP impact

Add `floor_items` (items at player's feet) and `inventory` to `GameObservation`. Add `pickup`, `use_item`, and `equip` actions to the `act` tool. Item decisions are strategic — this is where LLM play gets interesting.

#### Starter item set (v1)

Keep it small — 4-6 items total. More can be added via `game.toml` without code changes.

| Item | Type | Effect | Weight | Notes |
|------|------|--------|--------|-------|
| Healing Potion | Potion | +10 HP | 40 | Core resource management |
| Strength Potion | Potion | +1 ATK permanent | 10 | Rare power-up |
| Short Sword | Equipment | +2 ATK | 15 | Early weapon upgrade |
| Long Sword | Equipment | +4 ATK | 5 | Rare weapon, min_depth 3 |
| Leather Armor | Equipment | +2 DEF | 15 | Early armor upgrade |
| Scroll of Mapping | Scroll | Reveal floor | 10 | Exploration shortcut |

#### Testing

- **Unit test:** Pickup adds item to inventory, removes from floor. Drop does the reverse.
- **Unit test:** Equipment modifies effective ATK/DEF.
- **Unit test:** Healing potion restores HP, capped at max_hp.
- **Unit test:** Full inventory rejects pickup with a message.
- **Scenario test:** Player with a sword kills a goblin faster than without.
- **Golden replays:** Regenerate.

---

## Phase 4: Experience & Leveling

**Gives kills meaning beyond survival.**

### Design

Monsters award XP on death. Accumulating enough XP triggers a level-up with stat increases. This creates a reason to seek out fights (risk/reward) and enables tackling harder floors.

#### Player progression fields

```rust
// In entity.rs or game.rs
pub struct GameState {
    // ... existing fields ...
    pub player_xp: Stat,
    pub player_level: Stat,
}
```

Keep XP/level on `GameState` rather than `Entity` — only the player levels up, and it avoids bloating the entity struct for all monsters.

#### XP config in `game.toml`

```toml
[leveling]
xp_per_level = [0, 20, 50, 100, 180, 300, 500, 800, 1200, 2000]
hp_per_level = 5           # +5 max HP on level-up (also heals 5)
attack_per_level = 1       # +1 ATK every level
defense_per_2_levels = 1   # +1 DEF every 2 levels
```

#### Monster XP values

Add `xp: Stat` to `MonsterDef`:

```toml
[[monsters]]
name = "Goblin"
# ... existing fields ...
xp = 5

[[monsters]]
name = "Orc"
xp = 15

[[monsters]]
name = "Troll"
xp = 40
```

#### Level-up logic

In `combat.rs` or `game.rs`, after a monster dies:

```rust
fn award_xp(&mut self, monster_idx: usize) {
    let xp = self.monster_xp_value(monster_idx);
    self.player_xp += xp;
    while self.player_xp >= self.xp_for_next_level() {
        self.player_level += 1;
        self.entities[0].max_hp += hp_per_level;
        self.entities[0].hp += hp_per_level;  // Heal on level-up
        self.entities[0].attack += attack_per_level;
        if self.player_level % 2 == 0 {
            self.entities[0].defense += defense_per_2_levels;
        }
        self.log.add(format!("Level up! You are now level {}.", self.player_level));
    }
}
```

#### Synergy with stairs

With depth scaling (Phase 2), leveling creates a natural power curve: the player gets stronger as floors get harder. Without stairs, leveling still works — it just doesn't have as much to scale against.

#### Files touched

| File | Change |
|------|--------|
| `data.rs` | Add `LevelingConfig` struct, `xp: Stat` to `MonsterDef`. |
| `game.toml` | Add `[leveling]` section, `xp` to each `[[monsters]]` entry. |
| `game.rs` | Add `player_xp`, `player_level` to `GameState`. Add `award_xp()`, `xp_for_next_level()`. Call `award_xp()` after monster death in `step()`. |
| `combat.rs` | Return monster index on kill (or handle XP award in `step()` by checking death after `melee_attack()`). |

#### MCP impact

Add `xp`, `level`, `xp_to_next_level` to `GameObservation`. LLMs can now factor XP into risk/reward calculations (e.g., "fight the orc for 15 XP or flee and save HP").

#### Testing

- **Unit test:** Killing a goblin awards 5 XP. Killing an orc awards 15 XP.
- **Unit test:** XP accumulates across kills. Level-up triggers at threshold.
- **Unit test:** Level-up increases stats correctly. DEF increases every 2 levels.
- **Scenario test:** Player reaches level 3 after clearing a floor of goblins.
- **Balance CI:** Will detect stat growth impact automatically.

---

## Phase 5: Creature Mood & Memory

**Cheap emergent narrative — 2 bytes per entity.**

Directly from the [simulation architecture doc](../architecture/simulation.md), adapted for incremental implementation.

### Design

Add `mood: i8` and `memory: u8` to `Entity`. Mood influences AI behavior: frightened monsters flee, enraged monsters are more aggressive. Memory records events that shift mood.

#### Entity changes

```rust
// In entity.rs
pub struct Entity {
    // ... existing fields ...
    pub mood: i8,       // -128 (terrified) to 127 (enraged). Default 0 (neutral).
    pub memory: u8,     // Bitflags: SAW_ALLY_DIE, WAS_HIT, LANDED_HIT, etc.
}
```

Memory flags:

```rust
pub const SAW_ALLY_DIE: u8  = 1 << 0;
pub const WAS_HIT: u8       = 1 << 1;
pub const LANDED_HIT: u8    = 1 << 2;
pub const LOW_HP: u8        = 1 << 3;
```

#### AI behavior expansion

```rust
pub enum AiBehavior {
    None,       // Player
    Chase,      // Greedy chase toward player
    Flee,       // Run away from player (reverse of chase vector)
    Wander,     // Random movement (not toward or away from player)
}
```

#### Mood thresholds

| Mood range | Behavior override | Condition |
|------------|-------------------|-----------|
| < -50 | Flee | Overrides Chase — monster runs away |
| -50 to -20 | Wander | Monster disengages, moves randomly |
| -20 to 80 | Normal | Uses base AI from `MonsterDef` |
| > 80 | Enraged (future) | Could grant +1 ATK, for now same as Chase |

#### Mood triggers

| Event | Mood change | Where |
|-------|-------------|-------|
| Ally dies in FOV | -30 (same species), -15 (different) | `combat.rs` after kill |
| Monster takes damage | -5 per hit | `combat.rs` in `melee_attack()` |
| Monster lands a hit | +10 | `combat.rs` in `melee_attack()` |
| Monster HP < 30% | -20 (once, via LOW_HP flag) | `ai.rs` at start of turn |
| Natural decay | +1 per turn toward 0 | `ai.rs` at start of turn |

The natural decay toward 0 prevents permanent mood states — a fleeing goblin will eventually calm down and resume normal behavior.

#### Flee AI

```rust
fn flee_ai(entities: &mut [Entity], idx: usize, px: Coord, py: Coord, map: &Map) {
    let mx = entities[idx].x;
    let my = entities[idx].y;
    // Move away from player (opposite of chase vector)
    let step_x = (mx - px).signum();
    let step_y = (my - py).signum();
    // Try diagonal away, then cardinal away, then any walkable
    let candidates = [
        (mx + step_x, my + step_y),
        (mx + step_x, my),
        (mx, my + step_y),
    ];
    for (nx, ny) in candidates {
        if map.is_walkable(nx, ny) && !is_occupied_by_monster(entities, nx, ny, idx) {
            entities[idx].x = nx;
            entities[idx].y = ny;
            break;
        }
    }
}
```

#### Species awareness

For "ally dies" to work, monsters need to recognize allies. Use `glyph` equality as a proxy for species (all goblins are `g`, all orcs are `o`). This requires no new data — `entity.glyph` already exists.

#### Message log integration

```
"The Goblin panics and flees!"     // mood crosses -50
"The Orc is enraged!"              // mood crosses 80
"The Goblin calms down."           // mood returns to normal range
```

#### Files touched

| File | Change |
|------|--------|
| `entity.rs` | Add `mood: i8`, `memory: u8` with `#[serde(default)]`. Add memory flag constants. |
| `ai.rs` | Add `flee_ai()`, `wander_ai()`. Modify `run_monster_turns()` to check mood thresholds before dispatching AI. Add mood decay. |
| `combat.rs` | After damage/kill, update mood for nearby same-species monsters (iterate entities in FOV of the dying monster). |
| `data.rs` | Optional: add `cowardice: i8` to `MonsterDef` for species-specific mood sensitivity (goblins more cowardly than trolls). |

#### MCP impact

Add `mood` (as a descriptive string: "neutral", "frightened", "fleeing", "enraged") to entity info in observations. This gives the LLM richer tactical information without exposing raw numbers.

#### Testing

- **Unit test:** Ally death in FOV reduces mood. Ally death out of FOV does not.
- **Unit test:** Mood < -50 causes flee behavior (monster moves away from player).
- **Unit test:** Mood decays toward 0 each turn.
- **Unit test:** Memory flags prevent duplicate mood shifts (LOW_HP only fires once).
- **Scenario test:** Killing 2 of 3 goblins causes the third to flee.
- **Golden replays:** Regenerate.

---

## Phase 6: Property Bitfields & Interaction Table

**Foundation for all future simulation depth.**

Directly from the [simulation architecture doc](../architecture/simulation.md), phases 1 and 3.

### Design

Add a `properties: u64` bitfield to `Entity` (and eventually `Tile`). Add a damage type system and a reaction lookup table. This is the foundation that makes "fire burns organic things" and "lightning conducts through metal" work — but it's useful even before those systems exist, because it enriches monster descriptions and sets up the data model.

#### Property bitfield

```rust
// New file: crates/core/src/properties.rs
pub type Properties = u64;

pub const ORGANIC:      Properties = 1 << 0;
pub const REGENERATING: Properties = 1 << 1;
pub const UNDEAD:       Properties = 1 << 2;
pub const FLAMMABLE:    Properties = 1 << 3;
pub const CONDUCTIVE:   Properties = 1 << 4;
pub const POISONOUS:    Properties = 1 << 5;
pub const COLD_BLOODED: Properties = 1 << 6;
pub const METALLIC:     Properties = 1 << 7;
```

#### Monster properties in `game.toml`

```toml
[[monsters]]
name = "Goblin"
# ... existing fields ...
properties = ["organic"]

[[monsters]]
name = "Troll"
# ... existing fields ...
properties = ["organic", "regenerating"]
```

#### Interaction table

```rust
pub enum DamageType { Physical, Fire, Cold, Poison, Lightning }
pub enum Reaction { Normal, Immune, Vulnerable, Heal }
```

The table is a `const` array indexed by `[DamageType][property_bit_index]`. The immediate use case is enriching the `melee_attack()` log messages ("The Troll regenerates!" when it has `REGENERATING`). The full damage-type system activates when items (fire scrolls, poison potions) or magic are added.

#### Files touched

| File | Change |
|------|--------|
| New `properties.rs` | Constants, `Properties` type alias, helper functions (`has_property`, `property_name`). |
| `entity.rs` | Add `properties: Properties` field with `#[serde(default)]`. |
| `data.rs` | Add `properties: Vec<String>` to `MonsterDef`. Parse into bitfield. |
| `game.toml` | Add `properties` lists to monster definitions. |
| `combat.rs` | (Future) Check properties during damage calculation. |

#### Why now

Phase 6 is cheap (S effort for the bitfield, M for the interaction table) and forward-compatible with everything. Once items exist (Phase 3), a fire potion immediately gets interesting interactions because the property system is already in place. It also enriches MCP observations — the LLM can see that a troll is `organic, regenerating` and reason about it.

#### Testing

- **Unit test:** Property bitfield operations (set, check, combine).
- **Unit test:** `MonsterDef` properties parse correctly from TOML strings.
- **Unit test:** `has_property(entity, REGENERATING)` works.

---

## Cross-Cutting Concerns

### Save/load compatibility

Every new field added to `GameState` or `Entity` must use `#[serde(default)]` to maintain backward compatibility with existing save files. Older saves will load with the new fields set to their defaults (0 for stats, empty for collections).

### Data-driven balance

All tuning constants go in `game.toml`, not in Rust code. This maintains the existing pattern where balance can be adjusted via hot reload (F10) during development and via file override in production.

### Golden replay regeneration

Most phases will change gameplay outcomes. After each phase, regenerate golden replays:

```sh
cargo run --bin headless -- --regenerate-goldens crates/core/tests/golden_replays/
```

### MCP observation growth

Each phase adds fields to `GameObservation`. Monitor the JSON payload size — the MCP design principle is "structured metadata over ASCII parsing." New fields should be concise. Consider the `compact` mode flag for LLM agents that don't need the full ASCII map.

### Headless runner updates

The headless auto-play AI (`bin/headless.rs`) will need updates for:
- **Phase 2:** Descend stairs when the floor is cleared.
- **Phase 3:** Pick up and use items (simple heuristic: always pick up, use healing potions when low).
- **Phase 4:** No changes needed — XP is automatic.
- **Phase 5:** No changes needed — mood affects monster AI, not player AI.

---

## Implementation Order Recommendation

```
Phase 1: Wandering Monsters (S)
    Immediate tension fix. No dependencies. Ship and playtest.

Phase 2: Stairs (M)
    Win condition. Ship and playtest.

    Phase 5: Creature Mood (M)          Phase 6: Properties (S-M)
    Independent. Can develop             Independent. Foundation
    in parallel with Phase 2-3.          for future simulation.

Phase 3: Items & Inventory (M-L)
    Biggest single feature. Ship and playtest.

Phase 4: Experience & Leveling (M)
    Best after stairs exist to scale against.
```

After all six phases, the game has: time pressure (wandering monsters), a win condition (stairs), resource management (items), progression (XP), emergent AI behavior (mood), and a foundation for simulation depth (properties). This transforms the current "solved arithmetic arena" into a game with meaningful decisions on every turn.
