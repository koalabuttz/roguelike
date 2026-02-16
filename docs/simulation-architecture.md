# Simulation Architecture

How to add Caves-of-Qud/NetHack-style emergent simulation to the roguelike while respecting GBA and C64 memory/CPU constraints.

## Design Philosophy

Simulation depth comes from **system interactions, not system complexity**. Five simple systems that all check the same property bits produce more emergent behavior than one complex system. The patterns below are chosen because they scale down gracefully: the same data structures work on desktop and retro hardware with only budget caps changing.

## 1. Property Bitfields

**Files:** `entity.rs`, `data.rs`, new `properties.rs`

Add a `properties: u64` bitfield to `Entity` (line 19) and `MonsterTemplate` (line 5). Each bit represents a material or elemental tag that any system can query.

```rust
// src/properties.rs
pub type Properties = u64; // u32 on GBA

pub const FLAMMABLE:    Properties = 1 << 0;
pub const CONDUCTIVE:   Properties = 1 << 1;
pub const ORGANIC:      Properties = 1 << 2;
pub const REGENERATING: Properties = 1 << 3;
pub const UNDEAD:       Properties = 1 << 4;
pub const POISONOUS:    Properties = 1 << 5;
pub const COLD_BLOODED: Properties = 1 << 6;
pub const METALLIC:     Properties = 1 << 7;
```

Monster templates gain properties:

```rust
pub const TROLL: MonsterTemplate = MonsterTemplate {
    // ...existing fields...
    properties: ORGANIC | REGENERATING,
};
```

Tiles also need properties (see section 3).

**Why bitfields:** Single-cycle operations on all targets. 64 boolean properties per entity in 8 bytes. No heap allocation, no branching — just `AND`/`OR`. The combinatorial interactions are free: fire checks `FLAMMABLE`, lightning checks `CONDUCTIVE`, poison checks `ORGANIC & !UNDEAD`.

## 2. Interaction Lookup Table

**Files:** `data.rs`, `combat.rs`

Add a `const` table mapping `(DamageType, Property) -> Reaction` to `data.rs`, alongside the existing templates. This enriches `melee_attack()` (`combat.rs:6`) without replacing it.

```rust
pub enum DamageType { Physical, Fire, Cold, Poison, Lightning }
pub enum Reaction { Normal, Immune, Vulnerable, Heal }

// DamageType x Property -> Reaction
// Indexed by [damage_type][property_bit_index]
pub const REACTIONS: [[Reaction; 8]; 5] = [
    // Physical: normal against everything
    [Normal, Normal, Normal, Normal, Normal, Normal, Normal, Normal],
    // Fire: effective vs ORGANIC, heals COLD_BLOODED
    [Normal, Normal, Vulnerable, Normal, Normal, Normal, Heal, Normal],
    // ...
];
```

`melee_attack()` walks the defender's set property bits and applies the first non-Normal reaction. This table lives in ROM on retro targets — zero RAM cost.

## 3. Richer Tiles + Cellular Automata

**Files:** `map.rs`, new `simulation.rs`, `game.rs`

### Phase A: Expand tile types

The `Tile` enum (`map.rs:6`) and `move_cost()` (`map.rs:16`) already anticipate this:

```rust
pub enum Tile { Wall, Floor, Water, Lava, Ice, Grass }
```

Pathfinding (`pathfinding.rs`) already respects `move_cost()` via Dijkstra, so new terrain types work for movement immediately.

### Phase B: Tile state layer

Add a per-tile state byte to `Map` (`map.rs:63`):

```rust
pub struct Map {
    pub tiles: Vec<Tile>,
    pub tile_state: Vec<u8>,  // temperature, wetness, etc.
    // ...existing fields...
}
```

### Phase C: Cellular automata

New `src/simulation.rs` — runs after AI in `step()` (`game.rs:461`):

```rust
pub fn tick_environment(map: &mut Map, entities: &mut [Entity], turn: i32, budget: usize) {
    // Process 1/N tiles per turn (round-robin) to stay within budget
    let stride = map.tiles.len() / budget;
    let offset = (turn as usize) % stride.max(1);
    for idx in (offset..map.tiles.len()).step_by(stride.max(1)) {
        // Fire spreads to FLAMMABLE neighbors
        // Water flows to adjacent lower tiles
        // Temperature propagates to neighbors
    }
}
```

The `budget` parameter comes from `SimBudget` (section 6). Players won't notice if gas spreads over 4 turns instead of 1.

## 4. Event Queue

**Files:** new `events.rs`, `game.rs`

Replace ad-hoc turn-counting (like `apply_regen()` at `game.rs:439`) with a general-purpose priority queue:

```rust
// src/events.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GameEvent {
    PoisonTick { entity_idx: usize },
    Rot { entity_idx: usize },
    SpawnMonster { room_idx: usize },
    StatusExpire { entity_idx: usize, status: StatusEffect },
    EnvironmentChange { x: Coord, y: Coord, new_tile: Tile },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimedEvent {
    pub turn: i32,
    pub event: GameEvent,
}
```

Add `event_queue: BinaryHeap<Reverse<TimedEvent>>` to `GameState` (`game.rs:258`). Process events in `step()` after incrementing `turn_count` (line 464).

**Why event-driven:** Zero CPU cost for things that aren't happening. A sword doesn't cost cycles until the turn it rusts. This is how NetHack handles prayer timeouts, item erosion, and delayed instadeath. On GBA, cap drains per turn via `SimBudget::max_events_per_turn`.

## 5. Creature Mood and Memory

**Files:** `entity.rs`, `ai.rs`, `combat.rs`

Add 2 bytes to `Entity` (`entity.rs:19`):

```rust
pub mood: i8,       // -128 (terrified) to 127 (enraged)
pub memory: u8,     // Bitflags: SAW_ALLY_DIE, WAS_HIT, WAS_FED, etc.
```

Expand `AiBehavior` (`entity.rs:12`):

```rust
pub enum AiBehavior {
    None,
    Chase,
    Flee,
    Wander,
    Guard { x: Coord, y: Coord },
}
```

AI dispatch (`ai.rs:29`) becomes mood-aware:

- `mood < -50` overrides Chase to Flee
- When an ally dies in FOV: `mood -= 30` for same-type monsters
- When a monster lands a hit: `mood += 10`

Mood changes happen in `combat.rs` during `melee_attack()`. This creates emergent narrative: goblins that see friends die will flee; trolls rage harder when wounded. The data cost is 2 bytes per entity.

## 6. Platform Scaling via SimBudget

**Files:** `data.rs`, `settings.rs`, `game.rs`

The `Platform` enum (`settings.rs`) already has GBA and C64 variants. Add a budget struct to `data.rs`:

```rust
pub struct SimBudget {
    pub max_entities: usize,
    pub active_sim_radius: Coord,
    pub ca_tiles_per_turn: usize,
    pub max_events_per_turn: usize,
    pub enable_tile_state: bool,
}
```

| Parameter | Desktop | GBA | C64 |
|-----------|---------|-----|-----|
| `max_entities` | 1024 | 128 | 32 |
| `active_sim_radius` | 40 | 16 | 10 |
| `ca_tiles_per_turn` | 2000 | 256 | 64 |
| `max_events_per_turn` | 100 | 16 | 4 |
| `enable_tile_state` | true | true | false |

All simulation systems respect these caps. On C64, the tile state layer is omitted entirely — environmental effects use only the event queue and interaction table.

## 7. Feature Flags

**File:** `Cargo.toml`

```toml
[features]
default = ["dev-tools", "full-sim"]
dev-tools = []
full-sim = []       # Desktop: full CA, temperature, fluids
minimal-sim = []    # Retro: event-only, no CA
```

In `game.rs`, the CA tick is gated:

```rust
#[cfg(feature = "full-sim")]
simulation::tick_environment(&mut self.map, &mut self.entities, self.turn_count, budget);
```

On `minimal-sim`, only the event queue and interaction table fire.

## Implementation Order

Each phase builds on the previous. Phases 2-5 are largely independent once phase 1 is done.

| Phase | What | Files | Effort | Depends on |
|-------|------|-------|--------|------------|
| 1 | Property bitfields | `entity.rs`, `data.rs`, new `properties.rs` | S | — |
| 2 | Expand `Tile` + `move_cost()` | `map.rs`, `render.rs` | S | — |
| 3 | Interaction table | `data.rs`, `combat.rs` | M | Phase 1 |
| 4 | Event queue | new `events.rs`, `game.rs` | M | — |
| 5 | Creature mood/memory + richer AI | `entity.rs`, `ai.rs`, `combat.rs` | M | Phase 1 |
| 6 | Tile state + cellular automata | `map.rs`, new `simulation.rs`, `game.rs` | L | Phases 1, 2 |
| 7 | SimBudget + feature flags | `data.rs`, `Cargo.toml`, `game.rs` | S | — |

## Integration Point

All simulation hooks into one place: `GameState::step()` at `game.rs:454`. The enriched turn loop becomes:

1. Player command (`handle_command`)
2. FOV update (`update_fov`)
3. Monster AI (`run_monster_turns`) — now mood-aware
4. Event queue (`process_events`) — delayed effects fire
5. Cellular automata (`tick_environment`) — environmental spread
6. Turn counter increment
7. Regen (subsumed by event queue in later phases)
