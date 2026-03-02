# Procedural Generation: Terrain, Themed Floors & Prefab Integration

> **Status:** Exploration. Builds on [procgen-exploration.md](procgen-exploration.md) with a concrete vision for terrain variety, themed procedural floors, and a prefab integration layer. Covers cross-platform seed determinism including constrained ports (GBA, C64).

## Vision

The dungeon should feel like a place with history and geography, not a chain of rectangles. Three ideas compose to achieve this:

1. **Terrain-typed tiles** — Expand the `Tile` enum beyond Wall/Floor. New terrain types have mechanical consequences (movement cost, sight blocking, sound propagation, damage) that create tactical decisions every turn.

2. **Themed procedural floors** — Each floor selects a *theme* that controls which generation algorithm runs, which terrain types appear, and how rooms/corridors are shaped. A "Flooded Mines" floor uses standard room placement but floods low areas with water. A "Fungal Caverns" floor uses CA generation with sight-blocking fungal growth. The generation is fully procedural — themes are parameterizations, not hand-authored maps.

3. **Prefab injection** — Occasional hand-crafted rooms inserted into procedural floors, and occasional fully hand-crafted special floors. Prefabs are where game design intent is strongest: a vault with a narrow entrance, a staircase room with multiple exits, a boss arena. They're the seasoning, not the meal.

The mix: 80% true procgen (never the same twice), 15% prefab rooms within procedural floors (familiar landmarks in unfamiliar territory), 5% fully prefab special floors (memorable set-piece moments).

---

## Part 1: Terrain Types

### Design Principles

Every new tile type must satisfy three criteria:

1. **Mechanical consequence.** It must change a decision the player makes. A tile that looks different but plays identically is a reskin, not terrain. Water changes whether you flee through it (slow escape) or stand and fight. Fungal growth changes whether you explore cautiously (can't see ahead) or aggressively.

2. **Multi-system interaction.** Each terrain should interact with at least two game systems (movement, FOV, acoustics, creature mood, property bitfields). Single-system terrain feels shallow. Water interacts with movement cost + acoustic propagation (splashing). Rubble interacts with movement cost + acoustics (crunching alerts monsters).

3. **Constrained-platform viability.** The tile must be representable as a single enum variant with no per-tile heap data. Behavior is determined entirely by the variant via `move_cost()`, `blocks_sight()`, `blocks_movement()`, `sound_modifier()`, and `on_enter_damage()`. On GBA/C64, tiles are stored as `u8` indices.

### The `Tile` Enum Expansion

The current enum has two variants (`Wall`, `Floor`). The `move_cost()` method already has comments anticipating `Water`, `Swamp`, `Lava` (`map.rs:14`). The expansion:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Tile {
    // --- Existing ---
    Wall,
    Floor,

    // --- Terrain ---
    ShallowWater,   // move_cost: 2, loud (splashing), extinguishes FLAMMABLE
    DeepWater,      // impassable without item, blocks ground LOS
    Ice,            // move_cost: 1, forced slide until non-ice or wall
    Rubble,         // move_cost: 2, loud (crunching), partial cover
    Chasm,          // impassable, blocks movement, doesn't block LOS
    FungalGrowth,   // move_cost: 1, blocks LOS, doesn't block movement
    Brambles,       // move_cost: 1, 1 damage on entry, monsters avoid
    Moss,           // move_cost: 1, provides persistent light (radius 2)

    // --- Structural ---
    StairsDown,     // walkable, triggers floor transition (Phase 2)
    StairsUp,       // walkable, triggers floor transition (Phase 2)
}
```

That's 12 variants total — fits in a `u8` (or even a 4-bit nibble if C64 packing demands it). Each variant is a zero-size discriminant; all behavior lives in methods.

### Tile Behavior Matrix

| Tile | `move_cost` | `blocks_movement` | `blocks_sight` | Sound Modifier | `on_enter_damage` | Glyph | Color |
|------|-------------|-------------------|----------------|----------------|--------------------|-------|-------|
| Wall | — | Yes | Yes | Reflects | — | `#` | Grey |
| Floor | 1 | No | No | Neutral | 0 | `.` | DarkGrey |
| ShallowWater | 2 | No | No | Loud (splash) | 0 | `~` | Cyan |
| DeepWater | — | Yes | No | Muffled | — | `≈` | DarkBlue |
| Ice | 1 | No | No | Neutral | 0 | `_` | White |
| Rubble | 2 | No | No | Loud (crunch) | 0 | `:` | DarkGrey |
| Chasm | — | Yes | No | Echo | — | ` ` | Black |
| FungalGrowth | 1 | No | Yes | Muffled | 0 | `"` | Green |
| Brambles | 1 | No | No | Neutral | 1 | `%` | DarkGreen |
| Moss | 1 | No | No | Neutral | 0 | `,` | Green |
| StairsDown | 1 | No | No | Neutral | 0 | `>` | Yellow |
| StairsUp | 1 | No | No | Neutral | 0 | `<` | Yellow |

### Tile Methods

```rust
impl Tile {
    pub fn move_cost(&self) -> i32 {
        match self {
            Tile::Floor | Tile::Ice | Tile::FungalGrowth
            | Tile::Brambles | Tile::Moss
            | Tile::StairsDown | Tile::StairsUp => 1,
            Tile::ShallowWater | Tile::Rubble => 2,
            Tile::Wall | Tile::DeepWater | Tile::Chasm => {
                unreachable!("impassable tiles — callers must check blocks_movement()")
            }
        }
    }

    pub fn blocks_movement(&self) -> bool {
        matches!(self, Tile::Wall | Tile::DeepWater | Tile::Chasm)
    }

    pub fn blocks_sight(&self) -> bool {
        matches!(self, Tile::Wall | Tile::FungalGrowth)
    }

    pub fn on_enter_damage(&self) -> i32 {
        match self {
            Tile::Brambles => 1,
            _ => 0,
        }
    }
}
```

`is_walkable()` in `Map` (`map.rs:124`) currently checks `tile == Tile::Floor`. It would change to `!tile.blocks_movement()`.

### Terrain Interactions with Planned Systems

Each terrain type is designed to interact with systems from the [gameplay implementation plan](gameplay-implementation-plan.md) and the [acoustic propagation proposal](acoustic-propagation.md):

| Terrain | Movement (current) | Acoustics (proposed) | Creature Mood (Phase 5) | Properties (Phase 6) | Items (Phase 3) |
|---------|-------------------|---------------------|------------------------|---------------------|-----------------|
| ShallowWater | Slows movement | Splashing alerts monsters | — | Extinguishes FLAMMABLE | — |
| DeepWater | Impassable | Muffles sound across | — | — | Future: swim item |
| Ice | Forced slide | — | Surprise encounters (slide into monsters) | — | Future: ice pick (grip) |
| Rubble | Slows movement | Crunching alerts monsters | — | — | Items buried in rubble? |
| Chasm | Impassable | Sound echoes across | — | — | — |
| FungalGrowth | Normal speed | Muffles sound | Fear triggers (can't see threats) | ORGANIC (burnable?) | — |
| Brambles | Deals 1 damage | — | Monsters avoid (effective flee barrier) | FLAMMABLE | — |
| Moss | Normal speed | — | — | ORGANIC | — |

The "don't close doors" principle: every terrain type enriches at least two downstream systems. None is purely cosmetic.

### Constrained-Platform Representation

All platforms generate the same **80x40 map** (see [Part 6: Unified Map Dimensions](#part-6-unified-map-dimensions--scrolling-viewports)). Constrained platforms render the map through a scrolling viewport. The tile data cost is identical everywhere: 80 x 40 = 3,200 bytes.

**GBA (28x18 viewport over 80x40 map):**
- Tile data: 3,200 bytes in EWRAM (256 KB available — trivial).
- Tile stored as `u8` (12 variants fit trivially).
- Tile graphics: 8x8 pixel tile per variant, stored in VRAM. 12 tiles x 32 bytes = 384 bytes.
- Palette: each terrain has a color index. GBA's 16-color palettes handle this easily.
- `blocks_movement()`, `blocks_sight()`, `move_cost()` are match arms — fast branch tables on ARM7.
- Scrolling: GBA has hardware BG scroll registers — set X/Y offsets and the hardware handles rendering. A scrolling viewport over a tilemap is what the GBA was designed for.

**C64 (40x22 viewport over 80x40 map):**
- Tile data: 3,200 bytes (up from 960 bytes for 40x22). The [port proposal](../platforms/c64-port-proposal.md#42-memory-budget-allocation-updated-for-rust-mos) budgets ~1.2 KB for tile data with ~22 KB headroom — the extra ~2 KB fits comfortably.
- Explored bitfield: 400 bytes (3,200 bits). Visible bitfield: 400 bytes. Total bitfields: 800 bytes (up from 240).
- Custom charset: one PETSCII character per tile variant. 12 custom chars x 8 bytes = 96 bytes in the charset (which has a full 2 KB budget for 256 chars).
- Color RAM: one nybble per tile. C64 has 16 colors — each terrain maps to a fixed color index.
- Behavior lookup: a 12-byte ROM table indexed by tile variant, with bit-packed flags (bit 0: blocks_movement, bit 1: blocks_sight, bit 2: loud, etc.). Single `lda table,x` + bit test.
- Scrolling: dirty-rectangle rendering (already proposed in the port proposal section 3.7). When the player crosses the viewport center, shift the viewport origin and redraw the newly visible edge. Typical scroll: ~40-80 cell updates instead of 1000. At ~20 cycles per cell write: ~800-1,600 cycles — negligible.

Neither platform needs any per-tile heap allocation. The enum variant *is* the data.

### Ice Slide Mechanic

Ice deserves special attention because it's the only terrain with non-trivial movement behavior. When the player (or a monster) moves onto an Ice tile, they continue sliding in the same direction until they hit a non-ice tile or a wall.

```
Implementation: In handle_command() (game.rs), after resolving movement onto
an Ice tile, enter a slide loop:
  1. Record the movement direction (dx, dy).
  2. While the next tile in (dx, dy) is Ice and walkable:
     a. Move the entity one more step.
     b. If an entity occupies the destination, collide (attack for monsters, bump-stop for walls).
  3. Recompute FOV after the full slide resolves.
```

This is a pure game-logic change in `core` — no platform-specific code. On C64, the slide is near-instant (a few hundred cycles of tile checks). On GBA, same. The only rendering consideration is whether to animate each slide step or show the final position; animation is a frontend concern.

---

## Part 2: Themed Procedural Floors

### Design Principles

A "theme" is not a map preset — it's a **generation parameter set** that controls:
1. Which generation algorithm runs (room-based, CA cave, hybrid).
2. Which terrain types are eligible to appear.
3. How corridors are shaped (L-shaped, drunkard's walk, A*-pathed).
4. Room size/count ranges.
5. Post-processing passes (flooding, fungal seeding, erosion, rubble scattering).
6. Monster spawn weighting adjustments (optional, per-theme).
7. Prefab room eligibility (which templates can appear on this theme).

Themes are fully data-driven via `game.toml`. Adding a new theme is a data change, not a code change (assuming the generation algorithms and post-processing passes exist in code).

### Theme Data Model

```toml
[[floor_themes]]
name = "dungeon"
description = "Constructed stone dungeon with rectangular rooms and corridors"
generator = "rooms_and_corridors"    # which algorithm
max_rooms = 30
room_size_min = 4
room_size_max = 10
corridor_style = "l_shaped"          # "l_shaped", "drunkard", "astar"
corridor_loop_chance = 0.15          # fraction of non-MST Delaunay edges (0.0 = chain)
terrain_passes = []                  # no post-processing
eligible_prefab_tags = ["vault", "treasure", "stairwell"]
weight = 50                          # selection weight when rolling floor type

[[floor_themes]]
name = "cave"
description = "Natural cavern system carved by water and time"
generator = "cellular_automata"
ca_initial_wall_pct = 45
ca_smooth_passes = 5
ca_wall_threshold = 5
ca_min_open_pct = 40
terrain_passes = ["water_pools"]     # post-pass: scatter ShallowWater in low areas
eligible_prefab_tags = ["underground_lake", "crystal", "stairwell"]
weight = 20

[[floor_themes]]
name = "flooded_mines"
description = "Abandoned mine works, partially reclaimed by groundwater"
generator = "rooms_and_corridors"
max_rooms = 20
room_size_min = 3
room_size_max = 8
corridor_style = "l_shaped"
corridor_loop_chance = 0.10
terrain_passes = ["flood_low"]       # post-pass: water table flooding
eligible_prefab_tags = ["vault", "stairwell"]
weight = 10

[[floor_themes]]
name = "fungal_caverns"
description = "Organic caves choked with bioluminescent fungal growth"
generator = "cellular_automata"
ca_initial_wall_pct = 42
ca_smooth_passes = 4
ca_wall_threshold = 5
ca_min_open_pct = 45
terrain_passes = ["fungal_seed"]     # post-pass: grow FungalGrowth clusters
eligible_prefab_tags = ["crystal", "stairwell"]
weight = 10

[[floor_themes]]
name = "frozen_vault"
description = "A constructed dungeon locked in ice"
generator = "bsp"
bsp_min_leaf_size = 8
bsp_room_margin = 2
corridor_style = "l_shaped"
corridor_loop_chance = 0.20
terrain_passes = ["freeze_water"]    # post-pass: convert ShallowWater to Ice
eligible_prefab_tags = ["vault", "treasure", "stairwell"]
weight = 5

[[floor_themes]]
name = "catacombs"
description = "Dense grid of burial alcoves and narrow passages"
generator = "rooms_and_corridors"
max_rooms = 50
room_size_min = 2
room_size_max = 4
corridor_style = "l_shaped"
corridor_loop_chance = 0.05          # very few loops — claustrophobic
terrain_passes = []
eligible_prefab_tags = ["stairwell"]
weight = 8

[[floor_themes]]
name = "the_chasm"
description = "A dungeon bisected by an impassable rift"
generator = "rooms_and_corridors"
max_rooms = 20
room_size_min = 4
room_size_max = 8
corridor_style = "l_shaped"
corridor_loop_chance = 0.15
terrain_passes = ["carve_chasm"]     # post-pass: wide Chasm rift with 1-2 bridges
eligible_prefab_tags = ["vault", "stairwell"]
weight = 5

[[floor_themes]]
name = "collapsed_level"
description = "A dungeon ravaged by structural failure"
generator = "rooms_and_corridors"
max_rooms = 25
room_size_min = 4
room_size_max = 10
corridor_style = "l_shaped"
corridor_loop_chance = 0.15
terrain_passes = ["rubble_collapse"] # post-pass: scatter Rubble clusters via CA
eligible_prefab_tags = ["vault", "stairwell"]
weight = 8

[[floor_themes]]
name = "crystal_geode"
description = "A vast open cavern lit by phosphorescent moss"
generator = "cellular_automata"
ca_initial_wall_pct = 35             # more open than standard cave
ca_smooth_passes = 6
ca_wall_threshold = 5
ca_min_open_pct = 55
terrain_passes = ["moss_walls", "pillar_ca"]  # moss on walls, pillar clusters inside
eligible_prefab_tags = ["crystal", "stairwell"]
weight = 5

[[floor_themes]]
name = "the_hive"
description = "Radial tunnels carved by something that lives here"
generator = "agent_digger"
agent_count = 4
agent_room_chance = 0.08             # chance per step of widening into a room
agent_turn_chance = 0.3              # chance per step of changing direction
target_floor_pct = 40                # stop when this % of map is carved
terrain_passes = []
eligible_prefab_tags = ["stairwell"]
weight = 3
```

### Theme Selection

When generating a floor (at game start, or upon descending stairs in Phase 2), the theme is selected by weighted random draw from eligible themes. Depth-based rules can bias selection:

```toml
[depth_rules]
# Force specific themes at specific depths
depth_1 = "dungeon"                  # Floor 1 is always a familiar dungeon
depth_5 = "the_chasm"               # Floor 5 is always the chasm (set piece)
depth_10 = "crystal_geode"          # Final floor is the geode (climactic)

# For non-fixed depths, use theme weights
# Optionally modify weights by depth range:
# [[depth_weight_overrides]]
# min_depth = 4
# max_depth = 7
# theme = "cave"
# weight_multiplier = 2.0            # caves are twice as likely in mid-game
```

The selection uses `map_rng`, so it's deterministic given the seed + depth. A seed code always produces the same sequence of themed floors.

### Themed Floor Catalog

Each theme below describes the generation algorithm, terrain mix, and gameplay feel.

#### 1. Dungeon (Default)

The familiar baseline. Rectangular rooms connected by corridors, with optional Delaunay/MST loops (from [procgen-exploration.md](procgen-exploration.md) Enhancement 1).

- **Generator:** `rooms_and_corridors` (current `Map::generate()` or improved with Delaunay connectivity).
- **Terrain:** Floor, Wall only. Clean constructed stone.
- **Feel:** Predictable, safe, tactical. Rooms and corridors have clear roles. Good training ground.
- **Gameplay:** Standard movement costs, standard sight lines, standard acoustics. The baseline that other themes contrast against.

#### 2. Cave

Organic spaces generated by cellular automata. No distinct rooms or corridors — just interconnected open areas with irregular walls.

- **Generator:** `cellular_automata` (Enhancement 2 from procgen-exploration.md).
- **Terrain:** Floor, Wall, with optional ShallowWater pools.
- **Post-processing:** Flood-fill to find connected regions. Fill small disconnected regions (< 15 tiles). Register the largest 3-5 open areas as "rooms" by bounding rect for spawn anchoring.
- **Feel:** Organic, open, unpredictable. No corridor chokepoints to exploit. Sight lines are irregular — sometimes long, sometimes blocked by jutting walls.
- **Gameplay:** Monsters can approach from any direction. Flee routes are unclear. Sound carries further in open spaces (acoustic system). Good for ranged combat (future).

#### 3. Flooded Mines

A standard dungeon with groundwater. The post-processing "flood" pass converts floor tiles below a y-threshold (or below a noise-field threshold) to ShallowWater.

- **Generator:** `rooms_and_corridors`.
- **Terrain:** Floor, Wall, ShallowWater. Some rooms are dry, some have ankle-deep water, some corridors are partially flooded.
- **Post-processing:** `flood_low` — iterate tiles; for each floor tile, compute a "height" value (e.g., `y + noise(x, y)` using a simple deterministic hash). If height < threshold, convert to ShallowWater.
- **Feel:** Familiar dungeon structure but movement is uneven. You hear splashing from two corridors away.
- **Gameplay:** ShallowWater (move_cost 2) makes flee routes through water slow. Acoustic propagation: splashing in water is louder than walking on stone. Forces the player to choose: fast dry route through dangerous territory, or slow wet route through safe territory.

#### 4. Fungal Caverns

CA caves with vision-blocking fungal growth scattered throughout. The most tactically distinct theme.

- **Generator:** `cellular_automata`.
- **Terrain:** Floor, Wall, FungalGrowth.
- **Post-processing:** `fungal_seed` — scatter N seed points on floor tiles. Run a second CA pass where FungalGrowth spreads from seeds (a cell becomes FungalGrowth if it has 2+ FungalGrowth neighbors and is currently Floor). 3-4 passes produce organic clusters.
- **Feel:** Claustrophobic despite open space. You can walk through the fungus but can't see through it. FOV is fragmented into small pockets.
- **Gameplay:** Creature mood (Phase 5) matters more — surprise encounters trigger fear in both the player and monsters. Acoustic propagation: FungalGrowth muffles sound (like a soft wall). Auto-explore becomes tense because you can't see what's ahead. The only floor type where *open space isn't safe*.

#### 5. Frozen Vault

BSP-generated dungeon (structured, geometric) with ice patches. Rooms tend to be large (vault-like). Water tiles are converted to Ice.

- **Generator:** `bsp` (Enhancement 4 from procgen-exploration.md).
- **Terrain:** Floor, Wall, Ice.
- **Post-processing:** `freeze_water` — first scatter ShallowWater via `flood_low`, then convert all ShallowWater to Ice.
- **Feel:** Architectural. This place was *built*, then frozen. BSP regularity gives it a constructed feel that contrasts with the CA caves.
- **Gameplay:** Ice slide mechanic (move onto ice, slide until hitting non-ice or wall). Combat on ice is chaotic — both player and monsters slide. Large BSP rooms mean long slides. Creates spatial puzzles: "Can I slide across this room to the exit, or will I crash into the troll?"

#### 6. Catacombs

Dense grid of tiny rooms connected by single-tile-wide corridors. Like the Labyrinth preset but tuned for claustrophobia, with very few corridor loops.

- **Generator:** `rooms_and_corridors` with small rooms (2-4 tiles) and high room count (50).
- **Terrain:** Floor, Wall only.
- **Feel:** Every step is a corner. Tight, oppressive, nowhere to run. Each room is a burial alcove barely large enough to stand in.
- **Gameplay:** Creature mood/flee AI is constrained — monsters can't flee because there's nowhere to go. Sound carries far down straight corridors but is blocked by turns. Troll encounters are terrifying because you can't kite. The few corridor loops that exist become critical tactical knowledge.

#### 7. The Chasm

A standard dungeon bisected by a wide impassable rift. Only 1-2 narrow bridges (single-tile-wide Floor spans) cross it.

- **Generator:** `rooms_and_corridors`.
- **Terrain:** Floor, Wall, Chasm.
- **Post-processing:** `carve_chasm` — select a roughly vertical or horizontal line across the map. Widen it to 3-5 tiles. Convert all tiles in the rift to Chasm. Then, for each room that overlaps the rift, either move it or remove it. Place 1-2 single-tile Floor bridges across the rift, ensuring both sides remain connected. If connectivity fails, retry with a different rift position.
- **Feel:** Two halves of a dungeon connected by chokepoints. Monsters on the far side are visible (Chasm doesn't block sight) but unreachable until you find a bridge.
- **Gameplay:** Bridges are kill zones. Sound echoes across the chasm (acoustic system: Chasm tiles propagate sound with bonus range). Strategic depth: do you clear one side fully before crossing, or bridge early for a shortcut?

#### 8. Collapsed Level

A standard dungeon that has been partially destroyed. Rubble clusters fill parts of rooms and corridors, creating cover and slowing movement.

- **Generator:** `rooms_and_corridors`.
- **Terrain:** Floor, Wall, Rubble.
- **Post-processing:** `rubble_collapse` — select 3-5 "collapse epicenters" (random floor tiles, preferring corridor-adjacent positions). Run a small CA pass around each epicenter: tiles within radius 3 have a chance of becoming Rubble (higher chance near the center). Verify corridors remain passable; if a corridor is fully blocked by rubble, clear a 1-tile path through.
- **Feel:** A dungeon that something happened to. Recognizably constructed but degraded. Rubble piles create natural cover inside rooms.
- **Gameplay:** Rubble (move_cost 2) slows movement and is acoustically loud. Rooms with rubble have natural cover (break line of sight around rubble clusters). Forces tactical positioning: use rubble for cover, or go around to avoid the noise?

#### 9. Crystal Geode

A single vast CA cave, more open than standard caves, with phosphorescent Moss covering walls. A few pillar-wall clusters generated inside via a second CA pass.

- **Generator:** `cellular_automata` with lower initial wall density (35%) and more smoothing passes.
- **Terrain:** Floor, Wall, Moss.
- **Post-processing:** `moss_walls` — for each wall tile adjacent to floor, convert adjacent floor tiles (within radius 1-2) to Moss. `pillar_ca` — seed a few small wall clusters inside the open space using a CA pass with inverted rules (cells become wall if they have 6+ wall neighbors, seeded from random points).
- **Feel:** Wide open, well-lit (Moss provides persistent light radius 2), with pillar-walls for cover. The opposite of the Catacombs.
- **Gameplay:** Moss makes most of the floor permanently visible (even outside FOV), so the player can see monsters approaching from far away. Monsters can also see the player from across the room. Creature mood triggers happen early (they see allies die from distance). Combat is about positioning around pillars.

#### 10. The Hive

Radial tunnels carved by agent-diggers from a central hub. Organic tunnel network with occasional widened chambers.

- **Generator:** `agent_digger` — spawn N agents at the map center. Each agent walks in a random direction, carving Floor. Each step: turn randomly with probability `agent_turn_chance`, widen to a room with probability `agent_room_chance`. Stop when `target_floor_pct` is reached.
- **Terrain:** Floor, Wall only. The shape *is* the theme.
- **Feel:** Everything connects back to the center. Radial tunnels create a hub-and-spoke topology.
- **Gameplay:** The central hub is dangerous — multiple approach vectors, monsters converge there. Sound propagates outward from the center through every tunnel simultaneously. Flee routes always lead back to the hub where other things are waiting. Unique exploration dynamic: spokes can be explored independently but you always return to the center.

### Terrain Post-Processing Passes

Each post-processing pass is a standalone function that mutates a `Map` in-place. Passes compose — a theme can chain multiple passes. All passes are deterministic given `map_rng`.

| Pass | Input | Output | Memory Cost | CPU Cost |
|------|-------|--------|-------------|----------|
| `flood_low` | Floor tiles + height function | ShallowWater in low areas | Zero (in-place) | O(W*H) single scan |
| `fungal_seed` | Floor tiles + seed points | FungalGrowth clusters | Zero (in-place CA) | O(W*H) x 3-4 passes |
| `freeze_water` | ShallowWater tiles | Ice tiles | Zero (in-place) | O(W*H) single scan |
| `carve_chasm` | Floor/Wall tiles + rift line | Chasm tiles + bridges | ~100 bytes (rift coords) | O(W*H) + flood-fill |
| `rubble_collapse` | Floor tiles + epicenters | Rubble clusters | Zero (in-place CA) | O(W*H) x 2 passes |
| `moss_walls` | Wall-adjacent Floor tiles | Moss tiles | Zero (in-place) | O(W*H) neighbor scan |
| `pillar_ca` | Open Floor areas + seeds | Wall pillar clusters | Zero (in-place CA) | O(W*H) x 2 passes |

Every pass operates on the existing `tiles: Vec<Tile>` array. No additional allocation. All are bounded-iteration (no recursion, no unbounded loops). Safe for GBA and C64.

### Unified Generation Parameters

All platforms generate the same 80x40 map with the same parameters. This is the foundation of cross-platform seed sharing (see [Part 6](#part-6-unified-map-dimensions--scrolling-viewports)). There are no per-platform parameter overrides for generation — the map is identical everywhere.

| Parameter | All Platforms (80x40) |
|-----------|----------------------|
| `max_rooms` | 30 |
| `room_size_min` | 4 |
| `room_size_max` | 10 |
| CA passes | 5 |
| Agent count (Hive) | 4 |
| Chasm width | 3-5 |
| Fungal seed count | 8-12 |
| Rubble epicenters | 3-5 |

What *does* differ per platform is the **viewport** and **FOV radius** — rendering concerns that don't affect the generated map. The `SimBudget` pattern from [simulation.md](../architecture/simulation.md) applies to per-turn operations (AI tick budget, sound propagation range), not to one-time generation. Generation runs once per floor behind a "Descending..." message; even on C64 it completes in well under 1 second.

---

## Part 3: Prefab Integration

### Two Scopes of Prefab

1. **Prefab rooms** — Hand-authored rooms inserted into procedural floors. The floor is generated normally, then 1-2 rooms are replaced with prefab templates.
2. **Prefab floors** — Entirely hand-authored floors that replace procedural generation. Selected for specific depths or by weighted chance.

Both use the same data format (ASCII grid + metadata). The difference is scope.

### Prefab Room Templates

A room template is a small ASCII grid with tile types and metadata.

```toml
[[room_templates]]
name = "narrow_vault"
tags = ["vault", "treasure"]
min_depth = 3                        # don't appear before floor 3
max_depth = 0                        # 0 = no upper limit
width = 9
height = 7
layout = """
#########
#.......#
##.....##
+...T...+
##.....##
#.......#
#########
"""
# Legend: '#' = Wall, '.' = Floor, '+' = connection point, 'T' = spawn point (treasure/monster)
# Connection points indicate where corridors can attach.
# Spawn points are floor tiles with semantic tags for what to place there.

[[room_templates]]
name = "flooded_chamber"
tags = ["underground_lake"]
min_depth = 2
width = 11
height = 9
layout = """
###########
##.......##
#..~~~~~..#
+..~~~~~..+
#..~~~~~..#
##.......##
###########
"""
# '~' = ShallowWater

[[room_templates]]
name = "stairwell_down"
tags = ["stairwell"]
min_depth = 1
width = 7
height = 7
layout = """
#######
#.....#
#..>..#
+.....+
#.....#
#.....#
#######
"""
# '>' = StairsDown

[[room_templates]]
name = "ice_bridge"
tags = ["vault"]
min_depth = 4
width = 11
height = 5
layout = """
###########
#...._....#
+..____.._+
#....._...#
###########
"""
# '_' = Ice

[[room_templates]]
name = "bramble_garden"
tags = ["treasure"]
min_depth = 3
width = 9
height = 7
layout = """
#########
#.%.%.%.#
#%......#
+...T...+
#......%#
#.%.%.%.#
#########
"""
# '%' = Brambles — monsters won't follow the player into brambles
```

### Prefab Room Placement

When a procedural floor is generated, the pipeline optionally inserts prefab rooms:

1. Generate all rooms procedurally (random placement, BSP, etc.).
2. Select 0-2 prefab templates whose `tags` match the current theme's `eligible_prefab_tags` and whose `min_depth`/`max_depth` contain the current floor depth.
3. For each selected prefab:
   a. Choose a procedural room to replace (prefer rooms that are not the player start room and whose bounding rect can contain the prefab).
   b. Remove the procedural room's tiles (revert to Wall).
   c. Stamp the prefab template onto the map at the room's position.
   d. Update the `rooms` Vec entry with the prefab's bounding rect.
   e. Connect the prefab's connection points (`+`) to the corridor network. If using Delaunay/MST connectivity (Enhancement 1), add the connection points as corridor targets instead of the room center.
4. If no suitable room can be replaced (all too small, wrong shape), skip prefab insertion. Prefabs are optional polish, not required for a valid floor.

The prefab count per floor is configurable:

```toml
[config]
prefab_room_chance = 0.6             # 60% of floors get at least one prefab room
prefab_room_max = 2                  # never more than 2 per floor
```

### Prefab Floor Templates

A prefab floor is a complete map — no procedural generation, just stamped directly.

```toml
[[floor_templates]]
name = "underground_lake"
tags = ["special", "water"]
depth = 5                            # always appears at floor 5 (if depth rules allow)
# depth = 0 means "eligible at any depth, selected by weight"
weight = 0                           # 0 = only placed by depth rule, never randomly
width = 80
height = 40
layout_file = "floors/underground_lake.txt"   # external file for large layouts
# Or inline for small layouts:
# layout = """..."""

# Spawn rules for this floor (override default spawn behavior)
spawn_zones = [
    { x = 5, y = 5, w = 15, h = 10, monsters = ["goblin", "goblin", "orc"] },
    { x = 60, y = 25, w = 10, h = 8, monsters = ["troll"] },
]
player_start = { x = 40, y = 38 }   # override procedural start position

[[floor_templates]]
name = "boss_chamber"
tags = ["special", "boss"]
depth = 10                           # final floor
weight = 0
width = 80
height = 40
layout_file = "floors/boss_chamber.txt"
```

Because all platforms use the same 80x40 map dimensions (see [Part 6](#part-6-unified-map-dimensions--scrolling-viewports)), prefab floors require only **one layout per template** — no platform-specific variants needed. Constrained platforms render the same 80x40 prefab floor through their scrolling viewport. This eliminates the authoring cost of maintaining multiple layout files per prefab floor and guarantees that prefab floors are identical across platforms.

### Prefab Data Storage

**PC/SSH/Web:** Templates parsed from `game.toml` or external layout files via the `data-files` feature. Loaded at startup, stored in `GameData`.

**GBA:** Templates compiled as `const` byte arrays in ROM. The `layout` string is converted at build time to a `&[Tile]` array. 20 room templates x ~200 bytes = ~4 KB ROM. 3 floor templates x 3,200 bytes (80x40) each = ~9.6 KB ROM. Total: ~14 KB ROM — trivial for GBA's 32 MB cartridge space, and zero RAM cost.

**C64:** Templates compiled into data segment (ROM equivalent — the program binary). Same const-array approach. A room template is a compact byte sequence: width (1 byte) + height (1 byte) + tile data (W*H bytes, 4-bit packed if needed). 10 room templates x ~50 bytes = ~500 bytes. Full 80x40 floor templates can be 4-bit packed (2 tiles per byte): 3,200 / 2 = 1,600 bytes each. 2 floor templates x 1,600 bytes = ~3.2 KB. Total: ~3.7 KB — within the C64's memory budget headroom (~18 KB remaining after the unified map data increase).

### Prefab-Procgen Interaction

The key architectural constraint: **prefab rooms must produce the same `rooms: Vec<Rect>` output as procedural rooms**. Downstream systems (spawn, FOV, auto-explore, stairs, items) consume `rooms` without knowing whether a room was hand-authored or procedurally generated.

A prefab room's bounding rect is its `Rect` entry. Its connection points are the positions where corridors attach. The corridor network treats prefab rooms identically to procedural rooms — the Delaunay triangulation uses room centers regardless of room origin.

For prefab floors, the entire `Map` is pre-authored — but `rooms` must still be populated. The floor template declares room regions (or the loader scans the layout for enclosed Floor areas and registers them as rooms). Spawn zones either use the standard `rooms`-based spawning or override with explicit `spawn_zones` from the template.

---

## Part 4: The Generation Pipeline

All of the above composes into a single pipeline:

```
Floor N requested (seed + depth + GameData)
        │
        ├── Derive floor_rng from map_rng (deterministic per seed+depth)
        │
        ▼
┌── Check depth_rules: is this depth a prefab floor? ──── YES ──┐
│                                                                │
NO                                                               ▼
│                                                     Load prefab floor template
│                                                     (same 80x40 layout on all platforms)
│                                                     Populate rooms Vec
│                                                     Skip to step 6
│
▼
1. Select theme (weighted random from eligible themes, using floor_rng)
        │
        ▼
2. Run generator algorithm (selected by theme)
   ├── rooms_and_corridors: random placement or BSP → room list → Delaunay/MST corridors
   ├── cellular_automata: CA init → smooth → flood-fill → register pseudo-rooms
   └── agent_digger: spawn agents → carve → register rooms at widenings
        │
        ▼
3. Insert prefab rooms (0-2, based on theme's eligible_prefab_tags)
   Replace procedural rooms with matching templates
   Connect prefab connection points to corridor network
        │
        ▼
4. Run terrain post-processing passes (ordered list from theme)
   flood_low → fungal_seed → freeze_water → carve_chasm → rubble_collapse → moss_walls → pillar_ca
   (each pass mutates tiles in-place)
        │
        ▼
5. Optional: room shape erosion (CA smoothing on room interiors)
   (Enhancement 5 from procgen-exploration.md)
        │
        ▼
6. compute_structural_walls()
        │
        ▼
7. spawn_monsters() (uses spawn_rng — independent of map generation)
        │
        ▼
8. Place stairs (Phase 2), items (Phase 3)
        │
        ▼
Done: Map with tiles, rooms, entities, stairs, items
```

Each step is independently testable. Steps can be enabled/disabled via config. The pipeline produces a `Map` with `tiles: Vec<Tile>` and `rooms: Vec<Rect>` that all downstream systems already consume.

---

## Part 5: Seed Determinism

### The Core Guarantee

> Given the same seed code, the same floor depth, and the same `GameData`, every platform must produce the same dungeon layout, the same monster placement, and the same gameplay sequence.

This is a load-bearing feature. Seed codes are shareable. Daily challenges depend on identical generation. Replays are deterministic. Breaking this breaks the game's social contract.

### Current Architecture

The seed system (`seed_code.rs`) encodes `(seed: u64, width: Coord, height: Coord, preset: Option<MapPreset>)` into a shareable base-36 string. At `game.rs:405-411`, the seed feeds into deterministic RNG:

```rust
let mut master = StdRng::seed_from_u64(seed);       // ChaCha20, deterministic
let mut map_rng = StdRng::from_rng(&mut master);     // independent stream
let mut spawn_rng = StdRng::from_rng(&mut master);   // independent stream
```

`StdRng` is `rand`'s `ChaCha20Rng` — a cryptographically strong PRNG with identical output across all platforms where Rust's `rand 0.8` runs. The split into `map_rng` and `spawn_rng` means changes to map generation don't affect monster spawning, and vice versa.

### What Threatens Determinism

#### 1. Algorithm changes

Any change to the generation algorithm (new room placement logic, different CA rules, new corridor carving) changes the sequence of RNG draws, which changes every subsequent random decision. This is inherent and unavoidable — algorithm changes break seed compatibility.

**Mitigation:** Version the generation algorithm. The seed code should encode (or imply) which generation version produced it. Old seeds replayed under a new algorithm will produce different dungeons — this must be documented.

```toml
[config]
generation_version = 2               # bump when generation algorithm changes
```

When decoding a seed code, if the generation version doesn't match the current version, warn the player: "This seed was generated with a different dungeon version. Layout may differ."

#### 2. Theme selection

Theme selection is a new RNG draw that doesn't exist in the current generator. Adding it changes the RNG sequence for all subsequent operations.

**Mitigation:** Theme selection draws from `map_rng` at the very start of generation, before any room placement. This is a one-time draw per floor. As long as the theme list and weights are identical (from the same `GameData`), the selection is deterministic.

#### 3. Prefab insertion

Prefab selection and placement draw from `map_rng`. The number and order of draws must be deterministic.

**Mitigation:** Prefab insertion happens at a fixed point in the pipeline (step 3), draws a fixed sequence from `map_rng` (number of prefabs → template selection → room replacement selection), and is controlled by `GameData` config. Same config = same draws.

#### 4. Post-processing passes

Each terrain pass draws from `map_rng`. The order and count of draws must be deterministic.

**Mitigation:** Passes execute in the order listed in the theme's `terrain_passes` array. Each pass is a deterministic function of the current tile state + `map_rng`. Same tile state + same RNG state = same output.

#### 5. Floating-point non-determinism

IEEE 754 floating-point operations can produce different results across architectures due to rounding, FMA (fused multiply-add), and extended precision. If any generation step uses floating point, cross-platform determinism is at risk.

**Mitigation:** No generation step should use floating point. All algorithms (room placement, CA, Delaunay triangulation, A* pathfinding, post-processing passes) use integer arithmetic only. The current FOV system uses `f64` slopes, but FOV is computed *after* generation and doesn't affect map layout.

### The RNG Stream Architecture

With themed floors and multi-level dungeons, the RNG stream architecture becomes more important. The current two-stream split (`map_rng`, `spawn_rng`) extends naturally:

```
master_seed (u64)
    │
    ├── master_rng = ChaCha20(master_seed)
    │
    ├── floor_1_map_rng = ChaCha20::from_rng(&mut master_rng)
    ├── floor_1_spawn_rng = ChaCha20::from_rng(&mut master_rng)
    │
    ├── floor_2_map_rng = ChaCha20::from_rng(&mut master_rng)
    ├── floor_2_spawn_rng = ChaCha20::from_rng(&mut master_rng)
    │
    └── ... (one pair per floor)
```

Each floor gets its own independent `map_rng` and `spawn_rng`. This means:

- Changing floor 3's generation algorithm doesn't affect floor 4's map or monsters.
- Adding a new floor between existing floors doesn't shift RNG streams for later floors.
- The player can share a seed code that produces identical dungeons on all floors.

An alternative (simpler) approach derives per-floor seeds arithmetically:

```rust
fn floor_seed(master_seed: u64, depth: u32) -> u64 {
    // Mix seed and depth to produce a per-floor seed
    // Use a hash-like mixing function, not simple addition
    // (master_seed + depth would produce correlated sequences)
    let mut hasher = master_seed;
    hasher ^= depth as u64;
    hasher = hasher.wrapping_mul(0x517cc1b727220a95);  // mixing constant
    hasher ^= hasher >> 32;
    hasher
}
```

This approach doesn't require storing a master RNG across floor transitions. Given `(master_seed, depth)`, any floor can be regenerated independently. This is important for the C64 port, which can't keep all floor states in memory simultaneously.

### Cross-Platform Seed Sharing via Unified Map Dimensions

#### The Solution: One Map Size Everywhere

All platforms — PC, GBA, C64 — generate the same **80x40 map**. Constrained platforms render it through a scrolling viewport (see [Part 6](#part-6-unified-map-dimensions--scrolling-viewports)). This eliminates the dimension variable entirely: same seed + same PRNG + same dimensions + same algorithm = **identical dungeon on every platform**.

This requires two changes from the current architecture:

1. **Standardize on `GameRng` (xoshiro128\*\*) instead of `StdRng` (ChaCha20).** ChaCha20 has no practical 6502 implementation. xoshiro128** is fast on all platforms, `no_std` compatible, and produces identical output everywhere.

2. **Constrained platforms generate 80x40 maps with scrolling viewports.** See [Part 6](#part-6-unified-map-dimensions--scrolling-viewports) for memory budgets and viewport implementation.

With both changes, a seed code `r7z3kq` means the same dungeon on PC, GBA, and C64. Daily challenges serve one seed. Leaderboards are directly comparable. A friend can share a seed code across any platform.

#### The `GameRng` Implementation

**xoshiro128\*\*** — the 32-bit variant of the xoshiro family. Fast on all platforms (4 shifts + 3 XORs per output), good statistical properties, 128 bits of state (16 bytes), published reference implementations.

```rust
// In core, no_std compatible:
pub struct GameRng {
    state: [u32; 4],  // xoshiro128** state
}

impl GameRng {
    pub fn seed_from_u64(seed: u64) -> Self {
        // SplitMix64 to expand 64-bit seed to 128-bit state
        // (standard seeding procedure for xoshiro)
        let mut sm = seed;
        let s0 = splitmix64(&mut sm) as u32;
        let s1 = (splitmix64(&mut sm) >> 32) as u32;
        let s2 = splitmix64(&mut sm) as u32;
        let s3 = (splitmix64(&mut sm) >> 32) as u32;
        Self { state: [s0, s1, s2, s3] }
    }

    pub fn next_u32(&mut self) -> u32 {
        // xoshiro128** algorithm
        let result = self.state[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.state[1] << 9;
        self.state[2] ^= self.state[0];
        self.state[3] ^= self.state[1];
        self.state[1] ^= self.state[2];
        self.state[0] ^= self.state[3];
        self.state[2] ^= t;
        self.state[3] = self.state[3].rotate_left(11);
        result
    }
}
```

This is `no_std` compatible, integer-only, and produces identical output on any platform.

**Performance per platform:**
- **PC:** xoshiro128** is faster than ChaCha20. No downside.
- **GBA:** Maps directly to ARM7 instructions (shift, XOR, rotate). Fast.
- **C64:** ~200-250 cycles per `next_u32()` in 6502 assembly (four 32-bit state vars in zero page). At ~500-1000 draws per floor generation: ~125,000-250,000 cycles = ~125-250 ms. Acceptable behind a "Descending..." message. The current Galois LFSR is faster (~15 ms), but cross-platform seed sharing is worth the tradeoff.

ChaCha20 remains available for non-generation purposes (e.g., networking, if ever needed). `GameRng` is used exclusively for map generation and monster spawning.

### Seed Code Format

The current seed code format (`<base36_seed>[-<W>x<H>][<preset_char>]`) simplifies under the unified map approach. Since all platforms use 80x40 by default, the dimension suffix becomes a dev/test-only feature:

- **Standard seed:** `r7z3kq` — implies 80x40, no preset. Valid on all platforms.
- **With preset:** `r7z3kq-a` — implies 80x40, Arena preset. Dev/test use.
- **Custom dimensions:** `r7z3kq-120x60` — non-standard size, PC-only. Not shareable to constrained platforms.

For the common case (sharing seeds between players on any platform), the seed code is just the base-36 seed string. No dimensions, no suffix. Clean and universal.

Themed floor selection is determined by the seed + depth + `GameData` config — it doesn't need to be in the seed code. As long as two players have the same `game.toml`, the same seed produces the same theme sequence.

If `game.toml` is modded (via `data.rs:load_game_data()` which tries CWD first), modded theme weights produce different floor sequences. This is acceptable — modded games are already not seed-compatible with unmodded games (different monster stats, different room sizes, etc.). Daily challenge responses can include a hash of relevant `GameData` fields for clients to verify compatibility.

### Seed Versioning: What Happens When Generation Changes

#### The Honest Truth

Seed codes will break between generation versions. This is unavoidable. Any change to the generation pipeline — adding a theme, adding a prefab template, changing corridor connectivity, adjusting CA parameters — shifts the RNG draw sequence, producing a different dungeon from the same seed.

This means: a seed shared today will produce a different dungeon after a generation update. A YouTube video showing "amazing seed r7z3kq" will lead viewers to a different dungeon if they're on a newer version. A friend's "try this" recommendation across a version boundary silently fails.

**This is normal.** Every procedurally generated game with shareable seeds faces this:

- **Minecraft** breaks seeds at major "epoch" boundaries (1.7, 1.13, 1.18). The community built version-specific tools like Chunkbase that require selecting your game version before analyzing a seed.
- **Brogue CE** breaks replay and seed compatibility even between minor point releases. Weekly seed contests implicitly pin the version — all participants play the same build.
- **Spelunky 2** broke all existing community seeds in a December 2020 patch. The developer warned it "may happen again." The community coped by downpatching via Steam.
- **Slay the Spire** — "nearly all seeds from runs more than 2-3 weeks old have been damaged in some way by updates." Speedrunners version-lock.
- **Dwarf Fortress** — the wiki's seed archive is organized by version, with explicit compatibility warnings noting which point releases changed generation.
- **Noita**, **Binding of Isaac**, **Rogue Legacy 2** — all version-specific seeds. No exceptions.

No major game has solved this. The question is not whether seeds break, but how the breakage is communicated.

#### Why Not Keep Old Generation Code?

The tempting fix: when generation changes, keep the old algorithm behind a version flag and dispatch by version. Old seeds always work.

This is what Minecraft does internally for biome generation across its epoch boundaries — and Minecraft has a team of hundreds, billions of dollars in revenue, and saved worlds with chunk boundaries between old and new generation that *require* old code to remain functional.

For this project, maintaining parallel generation codepaths is not worth the cost:

1. **Every change to `Map`, `Tile`, `Rect`, or `GameConfig` must not break old codepaths.** This creates an ever-growing compatibility surface that constrains future design.
2. **Old codepaths accumulate.** Generation v1, v2, v3... each frozen in amber. Dead code that can't be deleted, must be compiled, and might have bugs that can't be fixed without breaking the version they serve.
3. **Roguelike runs are ephemeral.** Unlike Minecraft worlds (which players build in over years), a roguelike run is a single session. There are no half-explored dungeons that span version boundaries. When a run ends, the seed's job is done.
4. **The constrained platforms can't afford it.** Carrying two generation algorithms doubles the code size in ROM. On C64 (~12 KB code budget), this is a real constraint.

The right approach: make seed breakage **visible, predictable, and infrequent** rather than trying to prevent it.

#### The Versioning Strategy

**1. Embed generation version in seed codes.**

Extend the seed code format with an optional version tag:

```
r7z3kq              current version (implicit — always means "whatever I'm running")
r7z3kq.2            explicitly generation version 2
```

The `.N` suffix is appended when displaying a seed (in the UI, in death screens, in leaderboard entries). When a player enters a seed with a version that doesn't match their client, the game shows a clear, non-blocking message:

> "This seed was created with dungeon version 2. You're running version 3. The dungeon layout will differ, but the seed is still playable."

The player can proceed — the seed is still a valid random seed, it just produces a different dungeon. No error, no refusal. Just transparency.

Implementation: `SeedParams` gains a `generation_version: Option<u8>` field. `seed_code.rs` encodes/decodes the `.N` suffix. `Map::generate()` logs the current version. The version is stored in `GameState` for save/replay purposes.

```toml
[config]
generation_version = 2        # bump when generation algorithm changes
```

**2. Batch generation changes into major releases.**

During active development, generation changes are frequent. But from a player's perspective, every patch that changes generation is a seed-breaking event. Batching reduces the number of breaks:

```
Generation v1: Current algorithm
  (rooms + L-corridors, the system as it exists today)

Generation v2: The "themed dungeon" release
  (Delaunay/MST connectivity + CA caves + terrain types + themed floors)
  Ship all of Part 1, Part 2, and the connectivity upgrade together.
  One break instead of ten incremental ones.

Generation v3: The "handcrafted touches" release
  (Prefab rooms + prefab floors + room shape erosion)
  Ship after v2 stabilizes and the community has time with it.

(Future) Generation v4: If graph grammars or key-lock mechanics are added
```

Between generation versions, patches can safely change: UI, balance, monster stats, AI behavior, combat formulas, rendering, network features, save format, sound, and any `GameConfig` value that doesn't affect the generation pipeline. These changes don't break seeds.

**What triggers a version bump:**
- Adding or removing a floor theme (changes theme selection weights and RNG draws).
- Adding or removing a prefab template (changes prefab selection draws).
- Adding or removing a terrain post-processing pass.
- Changing the room placement, corridor, or CA algorithm.
- Changing the order of operations in the generation pipeline.
- Adding or removing a `Tile` variant used in generation.

**What does NOT trigger a version bump:**
- Monster stat changes, new monster types (uses `spawn_rng`, independent of `map_rng`).
- FOV radius changes (computed post-generation).
- AI behavior changes.
- UI, rendering, input changes.
- New `GameConfig` fields that don't feed into generation.

**3. Version-pin daily challenges.**

The daily challenge server includes the expected generation version in its response:

```json
{
  "date": "2026-02-20",
  "seed": "b7f1k2m9",
  "generation_version": 2
}
```

Clients on a mismatched version see: **"Today's daily challenge requires dungeon version 2. Update to participate."** Or, if the client is *ahead* of the server: **"Today's challenge uses dungeon version 2. Your client is on version 3 — the dungeon will differ from other players."**

This ensures the competitive use case (daily leaderboards) is always version-consistent. Players on older clients know they need to update; players on bleeding-edge builds know their scores aren't comparable.

**4. Store initial map state in replays.**

Replays have two possible formats:

- **Seed-based replay:** Store `(seed, generation_version, command_sequence)`. Compact (~100 bytes + commands), but requires the matching generation code to reproduce the map. Breaks across generation versions.
- **State-based replay:** Store `(initial_map_state, command_sequence)`. Larger (~20-80 KB for the map + commands), but the map is stored verbatim — no regeneration needed. Version-proof.

Recommendation: **use state-based replays.** The extra storage is negligible (a single save file is already ~20-80 KB of JSON). Replays are archival by nature — a recording of a great run should be watchable in any future version, not just the version it was recorded on. The golden replay tests in `crates/core/tests/` already store full game state for regression testing; replays would use the same format.

Seed codes remain the lightweight sharing format for "try this dungeon" use cases, where version sensitivity is acceptable.

**5. Freeze the C64 generation version.**

The C64 port ships on physical media (cartridge, disk image) and can't easily be updated. When it ships, it targets a specific generation version — say, v2. The PC/SSH/GBA versions continue to evolve.

To maintain cross-platform seed sharing with the C64:
- The PC client supports a `--generation-version 2` flag (or a settings toggle: "C64 compatible mode") that uses the frozen v2 generation code.
- Daily challenges can optionally serve a "C64-compatible" seed at the pinned version.
- Seed codes from the C64 include the version tag: `r7z3kq.2`. The PC client sees the `.2` suffix and either uses v2 generation (if available) or warns about the version mismatch.

This is much cheaper than maintaining *all* old generation codepaths — only the version that the C64 shipped with needs to be preserved. If the C64 port is later updated (new cartridge revision, disk update via UII+ network), it bumps to the current generation version and the old frozen path can eventually be removed.

#### What Players Should Expect

The following should be communicated in the game's documentation, FAQ, or README:

- **Seeds are version-specific.** A seed code produces the same dungeon for everyone on the same game version. When the dungeon generation algorithm is updated (which happens occasionally to add new floor themes, terrain types, and features), old seed codes will produce different dungeons on the new version.
- **Your saved games are safe.** Save files store the complete dungeon, not just the seed. Loading a save always restores your exact game, regardless of version updates.
- **Daily challenges are version-pinned.** Everyone playing today's daily challenge sees the same dungeon, as long as they're on the current version.
- **Seed codes include a version tag.** When sharing seeds, the version is visible (e.g., `r7z3kq.2`). If you enter a seed from a different version, the game tells you — the seed still works, but the dungeon will differ from what the person who shared it experienced.

This is transparent, honest, and sets the right expectations. Players who understand the system will trust it; players who don't will at least see a clear message when something doesn't match.

---

## Part 6: Unified Map Dimensions & Scrolling Viewports

### The Key Insight

The obstacle to cross-platform seed sharing has always been map dimensions. Room placement uses `rng.gen_range(1..width)` — different widths mean different RNG draws, which cascade into completely different dungeons. No amount of PRNG standardization helps if the bounds differ.

The solution: **all platforms generate 80x40 maps.** Constrained platforms render through a scrolling viewport. The viewport is a pure rendering concern — it doesn't touch `core`, doesn't affect generation, doesn't draw from the RNG. The map is identical everywhere; only the window into it differs.

```
PC: full 80x40 visible (no scroll needed)
┌────────────────────────────────────────────────────────────────────────────────┐
│                                80 columns                                      │
│                                                                                │
│                            full map visible                                     │
│                                40 rows                                          │
│                                                                                │
└────────────────────────────────────────────────────────────────────────────────┘

C64: 40x22 viewport scrolls over 80x40 map
                    ┌──────────────────────────────────────┐
                    │ · · · · · · · · · · · · · · · · · ·  │
┌───────────────────│─· · · · · · · 40x22 viewport · · · ·│──────────────────────┐
│                   │ · · · · · · · · · · · @ · · · · · ·  │                      │
│                   │ · · · · · · · · · · · · · · · · · ·  │                      │
│      80x40        │ · · · · · · · · · · · · · · · · · ·  │                      │
│      full map     └──────────────────────────────────────┘                      │
│      in RAM                                                                     │
│                                                                                 │
└─────────────────────────────────────────────────────────────────────────────────┘

GBA: 28x18 viewport scrolls over 80x40 map (hardware BG scroll registers)
```

### C64: 80x40 with Scrolling Viewport

The C64 port proposal ([c64-port-proposal.md](../platforms/c64-port-proposal.md) section 3.1) already discusses scrolling viewport over a 64x48 map. Expanding to 80x40 is a modest change.

#### Updated Memory Budget

| Data | 40x22 (original) | 80x40 (unified) | Delta |
|------|------------------|-----------------|-------|
| Tile data | 960 B | 3,200 B | +2,240 B |
| Explored bitfield | 120 B | 400 B | +280 B |
| Visible bitfield | 120 B | 400 B | +280 B |
| Room list (30 rooms x 8 bytes) | 128 B | 240 B | +112 B |
| Entity table (48 x 16 bytes) | 256 B | 768 B | +512 B |
| **Total map + entity data** | **~1.6 KB** | **~5.0 KB** | **+3.4 KB** |

The C64 port proposal budgets ~24 KB with ~22 KB headroom. The unified map approach uses ~3.4 KB of that headroom, leaving **~18.6 KB** — still comfortable for items, stairs, sound data, network buffers, and future expansion.

#### Entity Budget

With 30 rooms and up to 2 monsters per room, a full 80x40 dungeon can have ~60 monsters. The original C64 proposal budgets 16 entity slots. The unified approach expands this to **48 slots** (768 bytes).

To keep per-turn CPU cost bounded, AI only runs for entities within a radius of the player. Distant monsters are "frozen" — they exist in the entity table but don't move, think, or run LOS checks. This preserves full seed determinism (all monsters are spawned during generation, deterministically) while keeping AI cost at ~15-20 active entities per turn.

```
Active AI radius: ~20 tiles from player (covers viewport + buffer zone)
Typical active entities: 8-15 (depends on room density in viewport area)
AI cost per active entity: ~200 cycles (LOS + chase)
Worst case: 20 active entities x 200 cycles = 4,000 cycles (~4 ms)
```

#### Viewport Scrolling

The viewport follows the player, centered when possible, clamped at map edges:

```
viewport_x = clamp(player_x - 20, 0, 80 - 40)    // 40-wide viewport
viewport_y = clamp(player_y - 11, 0, 40 - 22)     // 22-tall viewport
```

On player movement, if the viewport shifts, the dirty-rectangle renderer redraws the newly revealed edge (40 tiles for a horizontal scroll, 22 tiles for a vertical scroll). At ~20 cycles per cell write: ~800 cycles. Negligible.

The existing C64 port proposal (section 3.7) already uses dirty-rectangle rendering — scrolling is a natural extension.

#### C64 Seed Entry

The C64 title screen accepts seed codes in the same base-36 format as PC. Since all platforms use 80x40, the dimension suffix is unnecessary for standard games:

```
┌────────────────────────────────────┐
│         ROGUELIKE DUNGEON          │
│                                    │
│    N) NEW GAME                     │
│    S) ENTER SEED: ________         │
│    C) CONTINUE                     │
│    D) DAILY CHALLENGE              │
│                                    │
│    SEED: r7z3kq                    │
└────────────────────────────────────┘
```

A seed `r7z3kq` entered on C64 produces the **exact same dungeon** as on PC. No warnings, no dimension mismatches. The C64 player just sees it through a smaller window.

#### xoshiro128** on 6502

The C64 must implement the same PRNG as the Rust `GameRng`. xoshiro128** on 6502:

- Four 32-bit state variables: 16 bytes in zero page (the fastest addressable memory).
- Operations per `next_u32()`:
  - 32-bit XOR: 4 `eor` instructions (~8 cycles).
  - 32-bit shift left by N: unrolled byte shifts + carry (~20-40 cycles depending on N).
  - 32-bit rotate left by N: shift + OR of shifted-out bits (~30-50 cycles).
  - 32-bit multiply by 5: shift left 2 + add original (~40 cycles).
  - 32-bit multiply by 9: shift left 3 + add original (~50 cycles).
  - Full `next_u32()`: ~200-250 cycles.
- Seeding via SplitMix64: ~500 cycles, once per floor.
- Total generation cost: ~500-1000 RNG draws x ~250 cycles = ~125,000-250,000 cycles = **~125-250 ms**. Acceptable behind a "Descending..." message.

The C64 must also expand from 16-bit seeds to **64-bit seeds** (8 bytes in zero page) to accept the full base-36 seed codes used by PC. The 4-digit hex display from the original C64 proposal is replaced with base-36 encoding matching the PC format.

### GBA: 80x40 with Hardware Scrolling

The GBA port ([gba-port.md](../platforms/gba-port.md)) runs Rust `core` with the `gba` feature flag. It compiles the same `GameRng` (xoshiro128**) for ARM7. Determinism is guaranteed by the compiler — same source code, same algorithm, same output.

#### Memory

- Tile data: 3,200 bytes in EWRAM (256 KB available). Trivial.
- Entity table: 48 x 16 bytes = 768 bytes in IWRAM. Fits within 32 KB.

#### Viewport

GBA display: 240x160 pixels = 30x20 tiles at 8x8. Gameplay viewport: 28x18 tiles (reserving 2 rows for status bar).

The GBA has **hardware BG scroll registers** (REG_BG0HOFS, REG_BG0VOFS) — set the X/Y pixel offset and the hardware composites the visible portion of the tilemap automatically. The 80x40 map is stored as a tilemap in VRAM; scrolling is free.

```
viewport_x = clamp(player_x - 14, 0, 80 - 28)    // 28-wide viewport
viewport_y = clamp(player_y - 9, 0, 40 - 18)      // 18-tall viewport
REG_BG0HOFS = viewport_x * 8                       // pixel offset
REG_BG0VOFS = viewport_y * 8
```

GBA tilemaps support up to 64x64 tiles natively. An 80x40 map slightly exceeds this — use two adjacent 64x32 tilemap screenblocks (a standard GBA technique for oversized maps). Or use 64x64 with the bottom 24 rows unused. Either approach is well-documented in the GBA dev community.

### Overview Map

All platforms support a toggle (button press) to view the entire 80x40 map at a glance. This is a read-only overlay — no gameplay happens while viewing the overview.

#### PC Overview

The PC already shows the full 80x40 map. An overview mode could add: color-coded terrain, monster positions, a fog overlay for unexplored areas, and a minimap in the corner during normal play.

#### C64 Overview (40x25 screen → 80x40 map)

Two compression schemes, composable:

**Horizontal 2:1:** Each screen character represents 2 horizontal map tiles. 80 → 40 columns. The character shows the "more interesting" of the two tiles (monster > stairs > terrain > floor > wall). This fits the C64's 40-column screen exactly.

**Vertical 2:1 via half-block characters:** PETSCII has upper-half (`▀`, $DF) and lower-half (`▄`, $EC) block characters. Each screen character encodes two vertical map rows. 40 → 20 rows for the map, leaving 5 rows for status/controls.

Combined: 80x40 map displayed in 40x20 characters. The full map fits on one screen.

```
┌────────────────────────────────────────┐
│ ▄▄    ▄▄▄▄▄▄▄  ▄▄▄▄  ▄▄▄▄▄▄         │
│ ██    █.....█──█....█──█...██▄▄       │
│ ██▄▄▄▄█.....█  █..@.█  █...██.█      │  <- 2:1 compression
│ ▀▀    ▀▀▀▀▀▀▀  ▀▀▀▀▀▀  ▀▀▀▀▀▀▀      │     80x40 → 40x20
│                                        │
│    ▄▄▄▄▄     ▄▄▄▄                     │
│    █...█─────█..█                      │
│    ▀▀▀▀▀     ▀▀▀▀                     │
│                                        │
│    ···                                 │
├────────────────────────────────────────┤
│ FLOOR 3  HP 24/30  KILLS 7  RM 12/18  │  <- Status
│ Press M to return to game              │  <- Prompt
└────────────────────────────────────────┘
```

Color coding: explored rooms (dim), current FOV (bright), unexplored (black), player `@` (yellow), monsters (red), stairs (white). Terrain types map to their assigned colors.

This requires **no extra RAM** — the overview renders on the fly from the tile array and explored bitfield.

**Trigger:** `M` key (for "Map"). On C64 with joystick: hold Fire + press Up. Returns to gameplay viewport on any key press.

#### GBA Overview (240x160 pixels → 80x40 map)

Each map tile maps to a **3x4 pixel block** (80 x 3 = 240 pixels wide, 40 x 4 = 160 pixels tall). Perfect fit for the GBA screen.

Render using GBA bitmap mode (Mode 3 or Mode 4):
- Wall: dark grey pixel block
- Floor: light grey
- ShallowWater: blue
- Player: yellow
- Monsters: red
- Unexplored: black
- Stairs: white

Simple, readable, and natural for the GBA's pixel-addressed display. Switch to bitmap mode for the overview, back to tile mode for gameplay. Both modes are instant on GBA.

**Trigger:** `Select` button. Returns to gameplay on any button press.

### Cross-Platform Daily Challenges

With unified 80x40 maps, the daily challenge server is simple:

```json
{
  "date": "2026-02-20",
  "seed": "b7f1k2m9",
  "generation_version": 2
}
```

One seed. All platforms. The dungeon is identical everywhere. A C64 player and a PC player explore the same rooms, fight the same monsters, in the same positions. The only difference is the viewport size (and FOV radius, which affects visibility but not the map itself).

Leaderboards are directly comparable — no per-platform categories needed. A C64 player who clears the daily dungeon with fewer turns than a PC player earned it on the same map. The smaller viewport arguably makes the C64 run *harder* (less advance warning of monsters), which adds a natural "hardcore" dimension to cross-platform competition.

### Seed Determinism Invariants

These invariants must hold for the seed system to be trustworthy:

1. **Same (seed, GameData, generation_version) → same Map.** On any platform running the same generation algorithm with `GameRng` (xoshiro128**). Dimensions are always 80x40 for standard games — they are no longer a variable.

2. **map_rng and spawn_rng are independent.** Changing monster definitions in `GameData` doesn't change the map layout. Changing room sizes doesn't change monster placement.

3. **Per-floor seeds are derivable from (master_seed, depth).** No accumulated RNG state needs to persist across floor transitions. Any floor can be regenerated independently.

4. **Post-processing passes don't introduce platform-dependent behavior.** All passes use integer arithmetic and draw from `map_rng`. No floating point, no platform-specific math.

5. **Prefab template data is versioned.** Adding or removing templates changes prefab selection (which draws from `map_rng`). Template changes must bump `generation_version`.

6. **Theme weights are determinism-critical.** Changing theme weights in `game.toml` changes theme selection, which changes everything downstream. Modded config = different dungeons. This is acceptable and expected.

### Testing Seed Determinism

The existing golden replay tests (`crates/core/tests/`) verify determinism by replaying a fixed command sequence against a fixed seed and comparing the resulting game state. These tests extend naturally:

1. **Cross-version regression tests:** For each `generation_version`, store a reference `Map` (tiles + rooms) for a set of test seeds. When the generation code changes, run against the reference. If output differs, the version must be bumped.

2. **Cross-platform equivalence tests:** Generate a Map on the Rust version for a given seed. Export the tile array (3,200 bytes for 80x40). The C64 test suite (running on VICE emulator) generates the same 80x40 map from the same seed using its xoshiro128** implementation and compares tile-by-tile. If they match, cross-platform seed sharing is confirmed.

3. **Theme determinism tests:** For a given seed, verify that the theme selection sequence is identical across runs. Store (seed → [theme_1, theme_2, ..., theme_N]) reference data.

4. **Prefab determinism tests:** For a given seed + depth + theme, verify that prefab insertion (which template, which room replaced, where placed) is identical across runs.

---

## Relationship to Existing Design Docs

| Doc | Relationship |
|-----|-------------|
| [procgen-exploration.md](procgen-exploration.md) | This doc builds on its recommended enhancements (Delaunay/MST, CA caves, prefabs, BSP, room shaping) by placing them in a themed-floor pipeline with terrain variety. |
| [gameplay-implementation-plan.md](gameplay-implementation-plan.md) | Phase 2 (stairs) needs floor-type variety — this doc provides it. Phase 3 (items) motivates treasure vault prefabs. Phase 5 (creature mood) benefits from terrain-aware flee AI. Phase 6 (property bitfields) interacts with terrain (FLAMMABLE + brambles, etc.). |
| [acoustic-propagation.md](acoustic-propagation.md) | Sound propagation is terrain-aware: ShallowWater is loud, FungalGrowth muffles, Chasm echoes. Each themed floor creates a different acoustic environment. |
| [gba-port.md](../platforms/gba-port.md) | GBA uses hardware BG scroll over the unified 80x40 map. `GameRng` (xoshiro128**) is ARM7-native. Tile graphics fit VRAM. Post-processing passes fit SimBudget. Overview map uses bitmap mode. |
| [c64-port-proposal.md](../platforms/c64-port-proposal.md) | C64 generates the full 80x40 map with a scrolling 40x22 viewport (section 3.1 already anticipated scrolling). Replaces Galois LFSR with xoshiro128** for cross-platform seed sharing. Entity table expands from 16 to 48 slots. Memory cost: ~3.4 KB additional from ~22 KB headroom. |
| [cross-platform.md](../architecture/cross-platform.md) | `GameRng` lives in `core` with zero platform deps. Unified 80x40 dimensions mean generation code has no platform conditionals. Viewport size is a frontend concern handled by each platform's renderer. Tile enum expansion respects the `no_std` / feature-flag architecture. |

---

## Open Questions

1. **How many terrain types at launch?** The full 12-variant enum is the aspiration. A phased rollout (v1: ShallowWater + Rubble + Chasm; v2: Ice + FungalGrowth + Brambles + Moss) reduces implementation risk and lets each terrain be playtested before the next is added.

2. **Theme weighting by depth — linear or configurable?** Should deeper floors favor more dangerous themes (Hive, Chasm) automatically, or should weights be fully configurable in `game.toml`? Automatic scaling is simpler; manual weights give designers more control.

3. **Prefab floor count.** How many fully hand-crafted floors should exist? The current proposal suggests 2-3 (underground lake, boss chamber, maybe one more). With unified 80x40 dimensions, each prefab floor requires only one layout — no per-platform variants. This significantly reduces authoring cost and makes a larger prefab library practical.

4. **Ice slide — does it trigger for monsters too?** If yes, ice floors become highly chaotic (monsters sliding into each other, chain reactions). If no, ice is a player-only hazard. Player-and-monster sliding is more interesting but harder to balance.

5. **Terrain damage (brambles) — does it scale?** 1 damage on entry is meaningful early-game (player has 30 HP, goblins have 6 HP) but trivial late-game. Should bramble damage scale with floor depth, or remain fixed as a minor tactical consideration?

6. **FOV radius as difficulty lever.** C64 uses FOV radius 8 (same as PC). FOV radius could be configurable per-platform in `GameConfig`, with the understanding that it doesn't affect seed determinism.

7. **FungalGrowth + FOV interaction.** FungalGrowth blocks sight but not movement. The FOV algorithm (shadowcasting on both PC and C64) must treat FungalGrowth as opaque — identical to Wall for sight purposes but transparent for movement. This is straightforward (`blocks_sight()` check instead of `== Tile::Wall`).

8. **Theme-specific monster spawning.** Should themes adjust monster spawn weights? A "Hive" floor could spawn more goblins (swarm theme). A "Frozen Vault" could spawn only trolls (guardians). This would require per-theme spawn weight overrides in `game.toml`, which complicates the spawn system. Alternatively, themes only affect terrain — monster variety comes from depth scaling.

9. **C64 entity count vs. AI cost.** The unified map requires 48 entity slots (up from 16). AI runs only for entities near the player (~20 tile radius). What happens when a monster wanders *into* the active radius from outside it? If frozen monsters don't move, they'll cluster at their spawn points forever. Options: (a) frozen monsters still exist but are invisible to the player and don't act — simple, slightly unrealistic; (b) frozen monsters do a single random walk step every N turns (very cheap) to simulate ambient wandering; (c) monsters only freeze outside a larger radius (~30 tiles) and are fully active within it.

10. **GBA tilemap size.** GBA hardware tilemaps support up to 64x64 tiles natively. An 80x40 map exceeds the 64-wide limit. The standard solution is two adjacent 64x32 screenblocks with software-managed seam crossing. This is well-documented but adds rendering complexity. Alternative: use a 64x40 map on GBA (slightly narrower than PC). This would break unified dimensions. Is the tilemap seam worth true 80x40 parity, or should GBA be the one platform with a minor dimension concession?

11. **Overview map on C64 — color constraints.** The C64 allows one foreground color per 8x8 character cell. In the 2:1 compressed overview, each cell represents a 2x2 area of map tiles. If two tiles in that area have different colors (e.g., blue water next to green floor), only one color can be shown. Use the "most interesting" tile's color (monster > stairs > terrain > floor > wall). This loses some information but is acceptable for an overview.
