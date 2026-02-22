# Gameplay Implementation Plan

> **Status:** In progress. Phase 1 (wandering monsters) is complete. Phase 2 (items) is next.

## Motivation

Four LLM playtest sessions ([summary](../reports/llm-playtest-summary.md)) converged on the same diagnosis:

1. **No win condition.** After clearing the dungeon, the game continues with nothing to do. *Solved when: the game can end in victory.*
2. **Regen is too generous.** The player fully heals between every encounter — no resource tension remains. *Solved when: LLM survival rate drops below 80% and waiting is no longer the dominant strategy.*
3. **Combat is solved arithmetic.** Every encounter has exactly one correct answer. `auto_fight` perfectly matches the combat depth. *Solved when: `auto_fight` is not the optimal strategy in >50% of encounters (item use, flee, or positioning matter).*
4. **No progression.** Killing monsters has no benefit beyond survival. *Solved when: the player's power increases over the course of a run, enabling harder content.*

The MCP interface reached maturity in Session 4 (0% wasted tool calls). The bottleneck shifted from interface to content.

## Overview

### Part 1: Core Loop Completion

Four phases, each addressing one playtest finding. Independently shippable. Sequentially validated via LLM playtesting. **All four must be complete before moving to Part 2.**

| Phase | Feature | Effort | Addresses | Status |
|-------|---------|--------|-----------|--------|
| 1 | [Wandering monsters](#phase-1-wandering-monsters) | S | Problem #2: Regen too generous | **Complete** |
| 2 | [Items & inventory](#phase-2-items--inventory) | M-L | Problem #3: Combat is solved arithmetic | Next |
| 3 | [Stairs / multi-level dungeons](#phase-3-stairs--multi-level-dungeons) | M | Problem #1: No win condition | — |
| 4 | [Experience & leveling](#phase-4-experience--leveling) | M | Problem #4: No progression | — |

Phase 4 benefits from Phase 3 (leveling needs something to scale against) but is not strictly blocked by it. Items is ordered before stairs because it blocks ~4 downstream features (the most of any item on the roadmap) and combines with wandering monsters to create a resource economy.

### Part 2: Simulation Enrichment (deferred)

These phases add emergent depth but do not address the core loop gap. **Defer until Part 1 is complete and playtested** — the core loop will inform their design. See the [simulation architecture doc](../architecture/simulation.md) for full design context.

| Phase | Feature | Effort | Prerequisite |
|-------|---------|--------|--------------|
| 5 | [Creature mood & memory](#phase-5-creature-mood--memory) | M | Part 1 complete |
| 6 | [Property bitfields & interaction table](#phase-6-property-bitfields--interaction-table) | S-M | Part 1 complete |

## Phase Interactions

While each phase is independently valuable, some combinations produce emergent gameplay that neither creates alone:

- **Wandering monsters + Items:** Healing potions become a finite resource under time pressure. This is where "solved arithmetic" actually breaks — the decision isn't "fight or not" but "use the potion now or save it for deeper floors."
- **Items + Stairs:** Carrying loot between floors creates persistent investment. Dying on floor 5 with a Long Sword hurts more than dying on floor 5 with nothing.
- **Stairs + XP:** Depth scaling and player scaling race against each other. The player can grind floor 1 for XP (safe but slow, wandering monsters punish this) or push deeper for better XP (risky but faster).

These interactions mean balance tuning after Phase 4 will differ significantly from tuning after Phase 1 alone. Re-run balance CI after the full set, not just per-phase.

---

## Phase 1: Wandering Monsters ✓

**Complete.** Implemented with `[wandering]` config in `game.toml`, `try_spawn_wandering()` in `game.rs`, `Wander` AI behavior in `entity.rs`, distance-based sound cues, idle acceleration, and grace period. The implementation exceeded the original proposal — it includes spawn chance, idle threshold/acceleration for camping detection, and multi-distance sound cues (`sound_far`, `sound_medium`, `sound_near`).

Playtest gate validation is pending — run LLM playtest sessions to measure survival rate impact before proceeding to Phase 2.

---

## Phase 2: Items & Inventory

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

#### State additions

```rust
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

In `combat.rs`, `melee_attack()` reads effective ATK/DEF from the entity's base stats plus equipment bonuses. Only the player has equipment — monsters use base stats. This keeps `melee_attack()` simple: pass effective stats rather than changing the function signature.

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

#### Playtest gate

Run 3+ LLM playtest sessions after implementation. Validate:

- **Item decisions happen.** LLMs should use `pickup` and `use_item` actions. If they ignore items, the items aren't impactful enough.
- **`auto_fight` is no longer optimal.** Using a healing potion mid-fight or equipping a weapon before a fight should produce better outcomes than raw `auto_fight`.
- **Inventory creates trade-offs.** Full inventory forces drop decisions.

If LLMs play identically with and without items, the item effects need to be stronger or more varied before proceeding.

---

## Phase 3: Stairs / Multi-Level Dungeons

**Addresses the #1 playtest complaint: no win condition.**

### Design

Add descending stairs (`>`) to each floor. Entering stairs generates a new, harder floor. The game ends when the player dies or reaches a target depth (or plays indefinitely in endless mode).

#### Dungeon model

One-way descent. `GameState` gains a `depth: Stat` field and `descend()` method. Previous floors are discarded. This avoids the complexity of storing multiple floors and aligns with many classic roguelikes (Brogue, early DCSS).

On descend, derive a new seed (`original_seed + depth`) for determinism. Generate a new map, spawn monsters scaled to depth, preserve the player entity (HP, stats, inventory, XP), and place the player at the up-stairs position.

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

#### Win condition

When the player descends past `target_depth`, trigger a victory screen. Add `game_won: bool` to `GameState` alongside `game_over`.

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

#### Testing

- **Unit test:** Stairs are placed on every generated map. Descend produces a valid new floor.
- **Unit test:** Player stats (HP, etc.) are preserved across descend.
- **Unit test:** Floor seed is deterministic (`seed + depth`).
- **Scenario test:** Player can reach floor 5 with god mode, verifying the descent chain works.
- **Golden replays:** Regenerate (stairs change map layout).
- **Invariant tests:** Add `depth >= 1` and `max_depth >= depth` to property checks.

#### Playtest gate

Run 3+ LLM playtest sessions after implementation. Validate:

- **LLMs find and use stairs.** The MCP observation includes `>` on the map and `depth`/`target_depth` in stats — LLMs should navigate to stairs and descend.
- **Difficulty ramps.** Survival rate should decrease on deeper floors. If floor 5 feels the same as floor 1, scaling needs tuning.
- **The game has a win condition.** At least one LLM session should reach (or attempt to reach) the target depth.

If LLMs don't engage with stairs, consider adding a `descend` action to the MCP `act` tool (rather than requiring pathfind-to + move onto `>`).

---

## Phase 4: Experience & Leveling

**Gives kills meaning beyond survival.**

### Design

Monsters award XP on death. Accumulating enough XP triggers a level-up with stat increases. This creates a reason to seek out fights (risk/reward) and enables tackling harder floors.

#### Player progression fields

```rust
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

On monster death, award XP equal to the monster's `xp` value. When accumulated XP crosses the next threshold in `xp_per_level`, increment level and apply stat bonuses (+HP, +ATK every level, +DEF every 2 levels). Multiple level-ups can trigger from a single kill.

#### Synergy with stairs

With depth scaling (Phase 3), leveling creates a natural power curve: the player gets stronger as floors get harder. Without stairs, leveling still works — it just doesn't have as much to scale against.

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

#### Playtest gate

Run 3+ LLM playtest sessions after implementation. This is the final Part 1 gate — validate the complete core loop:

- **Progression is visible.** LLMs should reach level 2+ in a typical session. If they don't, XP thresholds are too high or monster XP values are too low.
- **Risk/reward decisions emerge.** With items, stairs, and XP all active, LLMs should exhibit varied strategies — some grinding for XP, some pushing deep quickly.
- **The four motivating problems are resolved.** Revisit each: (1) win condition exists via stairs, (2) regen is punished by wandering monsters, (3) combat involves item decisions, (4) kills award progression. If any remain unresolved, iterate before moving to Part 2.

---

## Part 2: Simulation Enrichment

> **Prerequisite:** Part 1 (Phases 1-4) is complete and validated via LLM playtesting. The core loop shapes the design of these phases — mood and properties are more useful when there are items to interact with, floors to flee across, and progression to modulate.

---

## Phase 5: Creature Mood & Memory

**Cheap emergent narrative — 2 bytes per entity.**

Directly from the [simulation architecture doc](../architecture/simulation.md), adapted for incremental implementation.

### Design

Add `mood: i8` and `memory: u8` to `Entity`. Mood influences AI behavior: frightened monsters flee, enraged monsters are more aggressive. Memory records events that shift mood.

#### Entity changes

```rust
pub struct Entity {
    // ... existing fields ...
    pub mood: i8,       // -128 (terrified) to 127 (enraged). Default 0 (neutral).
    pub memory: u8,     // Bitflags: SAW_ALLY_DIE, WAS_HIT, LANDED_HIT, etc.
}
```

Memory flags: `SAW_ALLY_DIE` (bit 0), `WAS_HIT` (bit 1), `LANDED_HIT` (bit 2), `LOW_HP` (bit 3).

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

Move away from the player (opposite of chase vector). Try diagonal away, then cardinal away, then any walkable tile. Species awareness uses `glyph` equality as a proxy (all goblins are `g`, all orcs are `o`) — no new data needed.

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
- **Phase 2:** Pick up and use items (simple heuristic: always pick up, use healing potions when low).
- **Phase 3:** Descend stairs when the floor is cleared.
- **Phase 4:** No changes needed — XP is automatic.
- **Phase 5:** No changes needed — mood affects monster AI, not player AI.

---

## Implementation Order

```
Part 1: Core Loop Completion
──────────────────────────────────────────────

Phase 1: Wandering Monsters (S) ✓
    Complete. Playtest gate validation pending.

Phase 2: Items & Inventory (M-L)          ← NEXT
    Most blocking feature. Creates resource decisions.
    Combined with Phase 1, transforms the resource economy.
    Ship → playtest gate → validate.

Phase 3: Stairs / Multi-Level Dungeons (M)
    Win condition. More interesting with items (carry loot
    between floors, depth-gated items).
    Ship → playtest gate → validate.

Phase 4: Experience & Leveling (M)
    Benefits from stairs (difficulty scaling) and items
    (XP decisions interact with resource decisions).
    Ship → final playtest gate → validate all 4 problems resolved.

Part 2: Simulation Enrichment (deferred until Part 1 validated)
──────────────────────────────────────────────

Phase 5: Creature Mood & Memory (M)
Phase 6: Property Bitfields & Interaction Table (S-M)
    Independent. Can develop in parallel once Part 1 is stable.
```

After Part 1, the game has: time pressure (wandering monsters), resource management (items), a win condition (stairs), and progression (XP). This transforms the current "solved arithmetic arena" into a game with meaningful decisions on every turn. Part 2 adds emergent depth on top of that foundation.
