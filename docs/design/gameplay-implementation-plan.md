# Gameplay Implementation Plan

> **Status:** In progress. Phases 1-3 complete (wandering monsters, items, stairs). Phase 4 (item-based progression) is next.

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
| 2 | [Items & inventory](#phase-2-items--inventory) | M-L | Problem #3: Combat is solved arithmetic | **Complete** |
| 3 | [Stairs / multi-level dungeons](#phase-3-stairs--multi-level-dungeons) | M | Problem #1: No win condition | **Complete** |
| 4 | [Item-based progression](#phase-4-item-based-progression) | M | Problem #4: No progression | Next |

Phase 4 benefits from Phase 3 (deeper floors drop better gear) but is not strictly blocked by it. Items is ordered before stairs because it blocks ~4 downstream features (the most of any item on the roadmap) and combines with wandering monsters to create a resource economy.

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
- **Stairs + Item Progression:** Depth-gated items create a pull toward deeper floors — better equipment only spawns below `min_depth`. The player can farm floor 1 for consumables (safe but gear-capped, wandering monsters punish this) or push deeper for powerful equipment and enchantment scrolls (risky but rewarding).

These interactions mean balance tuning after Phase 4 will differ significantly from tuning after Phase 1 alone. Re-run balance CI after the full set, not just per-phase.

---

## Phase 1: Wandering Monsters ✓

**Complete.** Implemented with `[wandering]` config in `game.toml`, `try_spawn_wandering()` in `game.rs`, `Wander` AI behavior in `entity.rs`, distance-based sound cues, idle acceleration, and grace period. The implementation exceeded the original proposal — it includes spawn chance, idle threshold/acceleration for camping detection, and multi-distance sound cues (`sound_far`, `sound_medium`, `sound_near`).

Playtest gate validated — LLM playtest sessions 5 and 6 confirmed the impact of wandering monsters and the item system on gameplay.

---

## Phase 2: Items & Inventory ✓

**Complete.** Implemented with a 26-slot Brogue-style stackable inventory (`Inventory` struct and `InvSlot` in `rules/items.rs`, shared across all tiers, slots a–z). Features: `item.rs` (ItemKind, Item, Equipment), data-driven `[[items]]` in `game.toml`, floor items (`HashMap<Pos, Vec<Item>>`), equipment slots (weapon/armor), effective ATK/DEF from equipment, consumable stacking, equipped-item indicators, item coloring in both terminal and C64 inventory UIs, item spawning per room, MCP actions (pickup, use_item, equip_item, drop_item by slot letter), look mode item display, spectate frame item rendering, C64 two-phase inventory with action bar and equip bonus display, and comprehensive unit + scenario tests. Golden replays regenerated.

### Design (reference)

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

## Phase 3: Stairs / Multi-Level Dungeons ✓

**Complete.** Implemented with `StairsDown`/`StairsUp` tile variants, `depth`/`max_depth` on GameState, `descend()` method (derives new seed from `original_seed + depth`), depth-based monster stat scaling, `min_depth` on monster definitions, `target_depth` win condition, `game_won` flag, `Descend` GameCommand, MCP `descend` action, and depth/floor info in observations. Player stats, inventory, and equipment preserved across floors. Golden replays regenerated.

### Design (reference)

Add descending stairs (`>`) to each floor. Entering stairs generates a new, harder floor. The game ends when the player dies or reaches a target depth (or plays indefinitely in endless mode).

#### Dungeon model

One-way descent. `GameState` gains a `depth: Stat` field and `descend()` method. Previous floors are discarded. This avoids the complexity of storing multiple floors and aligns with many classic roguelikes (Brogue, early DCSS).

On descend, derive a new seed (`original_seed + depth`) for determinism. Generate a new map, spawn monsters scaled to depth, preserve the player entity (HP, stats, inventory, equipment), and place the player at the up-stairs position.

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
target_depth = 5                # Win condition: reach this floor
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

## Phase 4: Item-Based Progression

**Gives exploration and combat meaning beyond survival — through discovery and decisions, not counters.**

### Design

Progression comes from the item system, not XP. The player grows stronger by finding depth-gated equipment, using enchantment scrolls to upgrade gear, and collecting rare permanent consumables. This creates a power curve driven by exploration and decision-making rather than a number ticking up from kills.

**Why not XP/leveling:** The gameplay plan's Problem #3 was "combat is solved arithmetic." XP/leveling adds more arithmetic — stats go up predictably, monsters scale with depth, and the game becomes a race between two linear curves. The player doesn't make decisions around XP; it just accumulates from kills they were going to make anyway (especially since wandering monsters already punish grinding). Item-based progression creates real decisions: which item to keep, when to use consumables, what to enchant, whether to push deeper for better gear or farm the current floor for safety.

#### Three progression pillars

**1. Depth-gated items** — Better equipment spawns on deeper floors via `min_depth`. This is the baseline power curve: floor 1 has basic gear, floor 3 has stronger weapons, floor 5 has the best equipment. The player gets stronger by descending, not by grinding.

**2. Enchantment scrolls** — The core progression resource (inspired by Brogue). Finding a Scroll of Enchantment lets the player permanently upgrade one piece of equipment: a +2 sword becomes +3, or +2 armor becomes +3. The player chooses where the power goes — offense vs defense. This is stat growth through item decisions.

**3. Permanent consumables** — Rare stat-boosting potions that provide milestone moments. Strength Potion (+1 ATK permanent) already exists in Phase 2's item set. Phase 4 expands this family: Toughness Potion (+1 DEF), Potion of Sight (+1 FOV radius). These are rare enough to feel like events, common enough to provide a progression arc.

#### New items

Expand the item pool from Phase 2's 6 items. New items use the existing `ItemKind` enum and spawn system — no new data model needed.

```toml
# Enchantment scrolls — the core progression mechanic
[[items]]
name = "Scroll of Enchantment"
glyph = "?"
color = "Magenta"
type = "scroll"
effect = "enchant"
spawn_weight = 8
min_depth = 2

# Permanent consumables — rare milestone moments
[[items]]
name = "Toughness Potion"
glyph = "!"
color = "Blue"
type = "potion"
effect = "toughness"
spawn_weight = 5
min_depth = 3

[[items]]
name = "Potion of Sight"
glyph = "!"
color = "Yellow"
type = "potion"
effect = "sight"
spawn_weight = 3
min_depth = 4

# Depth-gated equipment — better gear on deeper floors
[[items]]
name = "Chain Mail"
glyph = "["
color = "White"
type = "equipment"
slot = "armor"
attack_bonus = 0
defense_bonus = 4
spawn_weight = 5
min_depth = 3

[[items]]
name = "Scroll of Teleport"
glyph = "?"
color = "Cyan"
type = "scroll"
effect = "teleport"
spawn_weight = 8
min_depth = 1
```

#### Enchantment system

Equipment gains an `enchant_level: u8` field (default 0). When the player uses a Scroll of Enchantment, they choose which equipped item to upgrade. The enchantment adds +1 to the item's primary stat (ATK for weapons, DEF for armor). Enchantment stacks — a Short Sword (+2 ATK base) enchanted twice becomes a +4 Short Sword.

```rust
// Addition to ItemKind or Equipment
pub enchant_level: u8,  // +1 per enchantment scroll used

// Effective stats
fn effective_attack(base_atk: Stat, weapon: &Equipment) -> Stat {
    base_atk + weapon.attack_bonus + weapon.enchant_level as Stat
}
```

Config in `game.toml`:

```toml
[enchantment]
max_enchant_level = 5       # Cap to prevent runaway scaling
```

#### Enchantment UI

Using a Scroll of Enchantment opens a simple selection prompt:

```
Enchant which item?
  a) Short Sword +2 ATK (+1 enchant)
  b) Leather Armor +2 DEF
```

On C64, this is a 2-line PETSCII overlay — no complex UI needed.

#### Permanent consumable effects

| Potion | Effect | Config key |
|--------|--------|-----------|
| Strength Potion | +1 ATK permanently | `strength_potion_bonus = 1` |
| Toughness Potion | +1 DEF permanently | `toughness_potion_bonus = 1` |
| Potion of Sight | +1 FOV radius permanently | `sight_potion_bonus = 1` |

These modify the player's base stats directly. The effect is immediate and logged ("You feel stronger! +1 ATK.").

#### Progression curve example

```
Floor 1: Bare fists (5 ATK, 2 DEF). Find Short Sword (+2 ATK) and Leather Armor (+2 DEF).
Floor 2: Find Scroll of Enchantment → enchant sword to +3 ATK. Total: 10 ATK, 4 DEF.
Floor 3: Find Chain Mail (+4 DEF), swap out Leather Armor. Find Strength Potion (+1 ATK permanent).
Floor 4: Find Long Sword (+4 ATK), equip it. Enchant it once. Total: 14 ATK, 6 DEF.
Floor 5: Well-equipped for the final push. Consumable stockpile (potions, scrolls) is the buffer.
```

The player's power increases through discovery and choices, not automatic accumulation. Bad luck on item drops is mitigated by enchantment scrolls (upgrade what you have) and permanent consumables (direct stat boosts).

#### Synergy with stairs

Depth-gated items create a natural pull toward descending — the best gear is deeper. This interacts with wandering monsters: spending too long on one floor is punished, but pushing too fast means facing harder monsters without adequate equipment. The tension between "farm this floor for consumables" and "descend for better gear" replaces the XP grind/push dynamic with item-driven decisions.

#### Files touched

| File | Change |
|------|--------|
| `item.rs` | Add `ScrollEffect::Enchant` variant. Add `enchant_level: u8` to equipment. Add `PotionEffect::Toughness`, `PotionEffect::Sight` variants. |
| `data.rs` | Add `EnchantmentConfig` struct to `GameData`. Parse new items from TOML. |
| `game.toml` | Add `[enchantment]` section. Add new `[[items]]` entries for enchantment scrolls, permanent consumables, and depth-gated equipment. |
| `game.rs` | Handle enchantment scroll use (prompt for target, apply bonus). Handle permanent consumable effects (modify base stats). |
| `command.rs` | Add `EnchantTarget(usize)` to `GameCommand` for enchantment selection. |

#### MCP impact

Add `enchant_level` to equipment info in observations. Add `enchant` action to the `act` tool (with target slot). LLMs can now factor equipment quality and enchantment decisions into strategy.

#### Testing

- **Unit test:** Enchantment scroll increases equipment's enchant_level by 1.
- **Unit test:** Effective ATK/DEF includes enchantment bonus.
- **Unit test:** Enchantment caps at `max_enchant_level`.
- **Unit test:** Toughness Potion permanently increases base DEF.
- **Unit test:** Potion of Sight permanently increases FOV radius.
- **Unit test:** Depth-gated items don't spawn above their `min_depth`.
- **Scenario test:** Player with enchanted weapon kills monsters faster than with base weapon.
- **Golden replays:** Regenerate.
- **Balance CI:** Will detect progression impact automatically.

#### Playtest gate

Run 3+ LLM playtest sessions after implementation. This is the final Part 1 gate — validate the complete core loop:

- **Progression is visible.** LLMs should have noticeably better equipment on floor 3+ than on floor 1. If gear quality feels flat across floors, depth-gating needs steeper differentiation.
- **Enchantment decisions happen.** LLMs should use enchantment scrolls and choose between weapon vs armor upgrades. If they always enchant the same slot, the decision isn't meaningful enough.
- **Risk/reward decisions emerge.** With items, stairs, and item-based progression all active, LLMs should exhibit varied strategies — some farming for consumables, some pushing deep for better gear.
- **The four motivating problems are resolved.** Revisit each: (1) win condition exists via stairs, (2) regen is punished by wandering monsters, (3) combat involves item decisions, (4) progression comes from item discovery and enchantment. If any remain unresolved, iterate before moving to Part 2.

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
- **Phase 4:** Use enchantment scrolls (heuristic: enchant the weaker equipment slot). Use permanent consumables immediately.
- **Phase 5:** No changes needed — mood affects monster AI, not player AI.

---

## Implementation Order

```
Part 1: Core Loop Completion
──────────────────────────────────────────────

Phase 1: Wandering Monsters (S) ✓
    Complete.

Phase 2: Items & Inventory (M-L) ✓
    Complete. Items on ground, inventory, equipment,
    consumables, scrolls. Data-driven via game.toml.

Phase 3: Stairs / Multi-Level Dungeons (M) ✓
    Complete. Win condition, depth scaling, min_depth
    on monsters. Player preserves stats/inventory across floors.

Phase 4: Item-Based Progression (M)          ← NEXT
    Expands item system: enchantment scrolls, permanent
    consumables, depth-gated gear. Benefits from stairs
    (deeper floors = better equipment).
    Ship → final playtest gate → validate all 4 problems resolved.

Part 2: Simulation Enrichment (deferred until Part 1 validated)
──────────────────────────────────────────────

Phase 5: Creature Mood & Memory (M)
Phase 6: Property Bitfields & Interaction Table (S-M)
    Independent. Can develop in parallel once Part 1 is stable.
```

After Part 1, the game has: time pressure (wandering monsters), resource management (items), a win condition (stairs), and progression (enchantment scrolls, depth-gated gear, permanent consumables). This transforms the current "solved arithmetic arena" into a game with meaningful decisions on every turn. Part 2 adds emergent depth on top of that foundation.
