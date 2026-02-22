# Procedural Generation: Design Exploration

> **Status:** Exploration. This document surveys procedural generation techniques for dungeon layout, evaluates each against this project's architecture and constraints, and identifies candidate enhancements to the current generator.

## Current System

The dungeon generator lives in `crates/core/src/map.rs:222-264` (`Map::generate()`). It uses **random room placement with L-shaped corridor connections**:

1. Fill the grid with `Tile::Wall`.
2. Place up to `max_rooms` (default 30) random rectangular rooms (size 4–10), rejecting overlaps.
3. Connect each room to the previous one via a 1-tile-wide L-shaped corridor (50% chance horizontal-first or vertical-first).
4. Player spawns at the center of the first room.

This is fast, simple, and always produces connected maps. It also has clear limitations:

- **Linear topology.** Rooms form a chain (1→2→3→...→N) with no loops or alternate paths. Every journey between two rooms has exactly one route.
- **Rectangular monotony.** All rooms are axis-aligned rectangles. No organic shapes, no irregular caverns.
- **Predictable corridors.** Every corridor is an L-shape between adjacent room centers. No winding passages, no T-junctions, no dead ends beyond the chain endpoints.
- **No structural variety.** Every floor looks fundamentally the same — a chain of boxes.

### Extension Points

The architecture provides two clean seams for enhancement:

1. **`Map::generate()` / `Map::from_preset()`** — The generation method is called from `GameState::with_data()` via the map RNG stream. New algorithms slot in here. The existing `MapPreset` enum already demonstrates multiple generators coexisting.

2. **`Tile` enum** — Currently `Wall` and `Floor`. The `move_cost()` method has comments anticipating `Water`, `Swamp`, `Lava`. New tile types enable terrain-aware generation.

3. **Split RNG streams** — `map_rng` and `spawn_rng` are derived independently from the master seed (`game.rs:405`). Changes to map generation don't affect monster spawning determinism, and vice versa.

---

## Evaluation Criteria

Every technique below is evaluated against five criteria derived from the project's architecture and philosophy:

### 1. Constrained-platform viability

The GBA port (`crates/gba/`) targets 32 KB IWRAM, 256 KB EWRAM, a 16 MHz ARM7 CPU, and `no_std`. The C64 port targets ~46 KB usable RAM and a 1 MHz 6502. Any generation algorithm must work within `SimBudget` caps — it can't require unbounded heap allocation, floating-point math, or deep recursion. Algorithms that need only integer arithmetic, fixed-size arrays, and bounded iteration are strongly preferred.

### 2. "Don't close doors"

Choosing a technique should benefit (or at minimum not block) future work: stairs/multi-level dungeons (gameplay plan Phase 2), items (Phase 3), creature mood (Phase 5), acoustic propagation (sound.rs), cellular automata simulation (simulation.md), and the interaction table (properties.rs). The ideal generator produces maps that are richer input for downstream systems, not just visually different.

### 3. Deterministic reproducibility

Seeded RNG is a core feature — seed codes are shareable, replays are deterministic, daily challenges depend on identical dungeon generation. Any new algorithm must be fully deterministic given a seed. Algorithms that require backtracking-with-restart (like WFC) need careful handling to maintain reproducibility.

### 4. Data-driven extensibility

The project's content pipeline flows through `game.toml` → `GameData` → generation. New generators should be parameterizable via TOML, not hardcoded. Adding a room shape or corridor style should be a data change, not a code change where possible.

### 5. Modular composition

The workspace architecture (`core` depends on nothing, frontends depend on `core`) means generation must stay in `core` with zero platform dependencies. But more importantly, generation techniques should compose — a pipeline where algorithm A handles macro layout and algorithm B handles room interiors is better than a monolithic generator that does everything.

---

## Technique Survey

### 1. BSP (Binary Space Partitioning) Trees

**How it works:** Recursively subdivide the map rectangle into halves (alternating horizontal/vertical splits). Each leaf node gets a room. Connect sibling leaves with corridors, walking up the tree.

**Constrained-platform viability:** Excellent. The tree depth is `log2(max_rooms)` — about 5 levels for 30 rooms. Each node is ~20 bytes (split axis, position, children). Total scratch memory: ~1.2 KB for 60 nodes. Integer-only. Iterative implementation possible (stack-based traversal avoids deep recursion).

**"Don't close doors" value:** Medium. BSP guarantees non-overlapping rooms with even spatial distribution — good input for stair placement (place stairs in the leaf farthest from the player via tree distance). However, the tree structure is inherently acyclic — no loops without post-processing.

**Deterministic:** Yes. Fully deterministic given a seed.

**Data-driven potential:** Split ratios, minimum leaf sizes, room-to-leaf size ratios are all natural TOML parameters.

**Composition role:** Best as a **macro layout** algorithm. Can be combined with other techniques for room interiors (CA for cave-like rooms, prefabs for special rooms).

**Tradeoffs:**

| Strength | Weakness |
|----------|----------|
| Guaranteed non-overlapping rooms | Grid-like, mechanical feel |
| Even spatial spread | No natural loops (tree topology) |
| Clean, predictable corridors | Hard to produce organic shapes |
| Low memory, fast, simple | Rooms always axis-aligned |

**Suitability: B+.** A solid upgrade over the current generator. Better spatial distribution, but still fundamentally rectangular-and-corridors. Worth implementing as an alternative `MapPreset`.

---

### 2. Cellular Automata (Cave Generation)

**How it works:** Initialize a grid with random wall/floor (typically 45% wall). Apply a smoothing rule iteratively (4–7 passes): a cell becomes wall if it has 5+ wall neighbors, floor otherwise. Flood-fill to find the largest connected region; fill smaller regions or connect them with tunnels.

**Constrained-platform viability:** Excellent. The entire algorithm operates on the existing `tiles: Vec<Tile>` array in-place. No extra allocation needed — just iterate the grid and apply neighbor-counting rules. On GBA (30×20 map = 600 tiles), each pass is ~600 tile checks × 8 neighbors = ~4,800 comparisons. Even on C64, 5 passes of a 40×22 grid completes in milliseconds.

**"Don't close doors" value:** High. Cave-like rooms are rich input for the acoustic propagation system (sound bounces differently in irregular spaces vs. rectangles). They create natural choke points that make creature mood and flee AI more interesting. The tile state layer (`simulation.md` Phase B) benefits from organic terrain shapes.

**Deterministic:** Yes, fully deterministic. The only RNG use is the initial random fill.

**Data-driven potential:** Initial wall density, neighbor threshold, number of passes, minimum region size, and connection strategy are all natural TOML parameters.

**Composition role:** Best as a **room interior** shaper or as an **alternative floor type**. Use BSP or random placement for the macro layout, then run CA within individual room bounds to give them organic shapes. Or use CA for entire "cave" floors in a multi-level dungeon, alternating with constructed dungeon floors.

**Tradeoffs:**

| Strength | Weakness |
|----------|----------|
| Beautiful organic shapes | No distinct rooms or corridors |
| Extremely fast, tiny memory | Frequently creates disconnected regions |
| Trivial to implement | Hard to place specific features (stairs, items) |
| Natural choke points | Unpredictable output shape |

**Suitability: A for variety, B for standalone use.** Best combined with a room-based macro layout. As a standalone generator, it needs significant post-processing to create gameplay-ready levels.

---

### 3. Delaunay Triangulation + MST Corridors

**How it works:** After rooms are placed (by any method), compute the Delaunay triangulation of room centers to get a well-behaved connectivity graph. Extract the minimum spanning tree (MST) for guaranteed full connectivity. Add back 10–15% of the non-MST Delaunay edges to create loops and alternate paths.

**Constrained-platform viability:** Moderate. The Bowyer-Watson algorithm for Delaunay triangulation is `O(n log n)` with bounded memory (triangle soup sized proportional to room count). For 30 rooms: ~60 triangles × ~24 bytes = ~1.5 KB scratch. MST extraction (Kruskal's or Prim's) needs a sorted edge list: 30 rooms × ~45 edges × 8 bytes = ~360 bytes. All integer-friendly (use squared distances to avoid sqrt). On C64 with 12 rooms, the budget is trivially met. **No floating point required** — Delaunay can use integer-only orient2d predicates.

**"Don't close doors" value:** Very high. This is the single most impactful improvement for gameplay. Loops create tactical choices (flee one way, circle around), make auto-explore more interesting, enable the acoustic propagation system to create tension (hearing monsters from two directions), and give creature mood/flee AI meaningful escape routes. Loops are also essential for multi-level dungeons — stairs feel better when you can approach them from different directions.

**Deterministic:** Yes. Delaunay triangulation is deterministic given point positions. MST is deterministic with consistent tie-breaking. Loop-edge selection uses the RNG (deterministic given seed).

**Data-driven potential:** Loop percentage (how many non-MST edges to add back) is a natural TOML parameter. Corridor style (L-shaped, A\*-pathed, drunkard's walk) is another.

**Composition role:** **Connectivity layer** — sits between room placement and corridor carving. Works with any room-placement algorithm (random, BSP, prefab). This is the missing piece in the current generator.

**Tradeoffs:**

| Strength | Weakness |
|----------|----------|
| Creates loops and alternate paths | More complex than sequential linking |
| Mathematically well-behaved graph | Needs a Delaunay implementation |
| Controllable loop density | May produce long corridors between distant rooms |
| Works with any room placement | Slightly more memory for triangle soup |

**Suitability: A.** The highest-impact single enhancement. Replacing the current "connect to previous room" chain with Delaunay+MST+loops transforms the topology from a line into a graph. This is the upgrade that would most change how the game plays.

---

### 4. Wave Function Collapse (WFC)

**How it works:** Define a tile set with adjacency constraints (which tiles can neighbor which). Initialize every cell as a superposition of all possible tiles. Iteratively collapse the lowest-entropy cell (pick one tile randomly), propagate constraints to neighbors, repeat until all cells are resolved. Backtrack on contradictions.

**Constrained-platform viability:** Poor. WFC requires per-cell entropy tracking (one value per tile type per cell), a propagation stack, and potentially multiple restarts on contradiction. For an 80×40 grid with 10 tile types: 3,200 cells × 10 possibilities = 32 KB just for the possibility matrix. On GBA with a 30×20 grid it's manageable (~6 KB), but backtracking adds unpredictable memory pressure. On C64, it's impractical. **Does not meet the constrained-platform criterion.**

**"Don't close doors" value:** Medium. WFC produces visually coherent tile patterns, but it doesn't inherently produce gameplay-relevant structures (rooms, corridors, connectivity). It's a spatial pattern solver, not a dungeon designer.

**Deterministic:** Conditionally. WFC is deterministic if contradictions never occur (same seed → same collapse order → same result). But if contradictions require restarts, the retry count may vary across platforms or compiler versions. Careful implementation can make it deterministic, but it's fragile.

**Data-driven potential:** Very high — the tile set and adjacency rules are inherently data-driven. But the data authoring burden is significant: each tile needs adjacency rules on all 4 (or 8) edges.

**Composition role:** Best as a **room interior decorator** — fill the inside of a pre-carved room with WFC-generated patterns (furniture, obstacles, terrain detail). Not suitable as a macro layout algorithm.

**Tradeoffs:**

| Strength | Weakness |
|----------|----------|
| Visually coherent patterns | High memory cost |
| Designer-driven via tile rules | Contradiction/restart complexity |
| Excellent for detail work | No gameplay structure guarantees |
| Flexible tile vocabulary | Tile set authoring is laborious |

**Suitability: C for now.** High complexity, high memory cost, and poor fit for constrained platforms. Revisit when/if a web-only "deluxe" generation mode is desired. Not recommended for `core` where it would need to work on GBA/C64.

---

### 5. Drunkard's Walk / Random Walk

**How it works:** Place a cursor in the map. Each step, move in a random direction and carve floor. Repeat until a target floor percentage is reached.

**Constrained-platform viability:** Excellent. Zero extra memory. Just a cursor position and a loop.

**"Don't close doors" value:** Low-Medium. Produces organic tunnels but with no structural properties — no rooms, no choke points, no clear topology. Downstream systems (stair placement, item spawning, monster density) have nothing to anchor to.

**Deterministic:** Yes.

**Data-driven potential:** Target floor percentage and number of walkers are simple parameters.

**Composition role:** Best as a **corridor carver** between pre-placed rooms. Instead of L-shaped corridors, use a biased random walk from one room center to another. This produces organic, winding corridors while maintaining the room-based structure that gameplay systems depend on.

**Tradeoffs:**

| Strength | Weakness |
|----------|----------|
| Trivially simple | Unpredictable, shapeless output |
| Zero extra memory | Slow convergence (revisits carved tiles) |
| Organic tunnel feel | No rooms, no structural features |
| Guaranteed single-walker connectivity | No control over layout properties |

**Suitability: B as a corridor carver, D as a standalone generator.** Useful in the pipeline, not as a replacement for room-based generation.

---

### 6. Prefab / Template-Based Rooms

**How it works:** Hand-author a library of room templates, each with annotated connection points. The generator selects and places compatible templates, optionally rotating or mirroring them.

**Constrained-platform viability:** Excellent. Templates are `const` data — they live in ROM on GBA/C64, consuming zero RAM. A template is just a small 2D array of tiles with connection-point metadata. 20 templates × ~200 bytes each = ~4 KB in ROM.

**"Don't close doors" value:** Very high. Prefabs are where hand-crafted gameplay design meets procedural generation. A "treasure vault" template with a narrow entrance creates natural choke points for combat. A "flooded chamber" template with `Water` tiles exercises the terrain system. A "crypt" template with `UNDEAD` property flags exercises the interaction table. Prefabs are also the natural home for **stairs** — a "staircase room" template ensures stairs are placed in architecturally interesting locations.

**Deterministic:** Yes. Template selection and placement use the RNG, which is seeded.

**Data-driven potential:** Very high — templates can be defined in external data files. The current `game.toml` pipeline could be extended with a `[[rooms]]` table, or templates could be defined as small ASCII maps in separate files. New templates can be added without code changes.

**Composition role:** **Special rooms** within a procedurally generated layout. The generator places most rooms via BSP or random placement, then inserts 1–3 prefab rooms at key locations (stair room, vault, boss room). This is exactly how Angband (vaults), Cogmind (prefab integration), and ADOM (special levels) work.

**Tradeoffs:**

| Strength | Weakness |
|----------|----------|
| Guaranteed quality per room | Requires designer labor |
| Natural home for gameplay features | Players recognize templates over time |
| Zero RAM cost (ROM data) | Only as varied as the library |
| Supports difficulty/narrative tagging | Connection rules need care |

**Suitability: A.** Essential for multi-level dungeons (gameplay plan Phase 2). When stairs exist, some floors should have hand-crafted rooms that feel special. Start with 5–10 templates and grow the library over time.

---

### 7. Graph-Based / Grammar-Based Generation

**How it works:** Generate an abstract graph representing the level's logical structure (start → key A → lock A → boss → exit), then realize each node as physical geometry. Graph grammars define production rules for expanding simple graphs into complex ones.

**Constrained-platform viability:** Moderate. The abstract graph is small (10–20 nodes × ~16 bytes = ~320 bytes). The complexity is in spatial realization — fitting the graph onto a 2D grid without overlaps. This is essentially a constraint satisfaction problem, which can be expensive. However, for the room counts this project uses (8–30 rooms), brute-force placement with backtracking is tractable even on C64.

**"Don't close doors" value:** Very high — but premature. Graph grammars shine when the game has key-lock puzzles, branching quest structures, and narrative progression. The current game has none of these. The gameplay plan's Phase 3 (items) and a hypothetical future "keys and locked doors" feature would make this technique highly valuable. Adding it now would be engineering for a future that may not arrive.

**Deterministic:** Yes, with careful implementation.

**Data-driven potential:** Grammar rules are inherently data. Production rules could live in TOML or a DSL.

**Composition role:** **Mission structure layer** — defines what the player must *do*, then delegates to other algorithms for how the space *looks*. Sits above BSP/random placement/prefabs in the pipeline.

**Tradeoffs:**

| Strength | Weakness |
|----------|----------|
| Guarantees game logic correctness | Complex to implement |
| Separates design intent from layout | Spatial realization is hard |
| Enables key-lock puzzles | Premature without keys/items |
| Supports non-linear progression | Can feel over-designed |

**Suitability: B+ future, C now.** Excellent technique, but the game needs items and keys first. Revisit after gameplay plan Phase 3.

---

### 8. Voronoi / Delaunay for Room Shapes

**How it works:** Scatter seed points, compute Voronoi cells. Each cell becomes a room with organic, non-rectangular boundaries. The dual Delaunay graph provides connectivity.

**Constrained-platform viability:** Moderate. Fortune's algorithm or Bowyer-Watson can be implemented with bounded memory. For 30 rooms: ~90 triangles, ~1.5 KB scratch. However, converting Voronoi cell boundaries to a tile grid requires polygon rasterization — more code than simple rect carving. On C64 with 12 seed points, the budget is met but the implementation complexity is high for 6502 assembly.

**"Don't close doors" value:** Medium. Organic room shapes are visually interesting and create natural choke points. But the irregularity makes item placement and stair positioning harder — there's no clear "center" or "corner" like a rectangular room has. The `Rect` struct and `contains_interior()` method would need generalization or replacement.

**Deterministic:** Yes.

**Data-driven potential:** Seed point distribution (random, Poisson disk), cell smoothing iterations are natural parameters.

**Composition role:** **Alternative room shapes.** Could replace `Rect` with a more general `Region` type, but this is a significant refactor touching `map.rs`, `spawn.rs`, `game.rs`, and rendering.

**Tradeoffs:**

| Strength | Weakness |
|----------|----------|
| Organic, non-rectangular rooms | `Rect`-based code needs rework |
| Natural connectivity via Delaunay | Polygon rasterization complexity |
| Visually distinctive | Irregular shapes complicate spawning |
| Scales well | High implementation cost |

**Suitability: B-.** Interesting but expensive to integrate with the current `Rect`-based architecture. The organic-room benefit can be achieved more cheaply by running CA inside rectangular rooms.

---

### 9. Agent-Based / Digger Approaches

**How it works:** Autonomous agents move through solid rock, carving tunnels. Agents occasionally widen their path into a room. Multiple agents create interconnected tunnel networks.

**Constrained-platform viability:** Excellent. One agent = one position + direction + a few probability counters. ~16 bytes per agent.

**"Don't close doors" value:** Low-Medium. Similar to drunkard's walk — produces organic tunnels but with little structural predictability. Downstream systems need rooms to anchor to.

**Deterministic:** Yes.

**Composition role:** **Corridor carver**, similar to drunkard's walk but with more character. Could produce mine-themed or ant-colony-themed floors in a multi-level dungeon.

**Suitability: C+.** Interesting flavor but limited structural value. Consider for a themed special floor rather than as a primary generator.

---

### 10. Perlin / Simplex Noise

**How it works:** Generate continuous noise values across the grid. Threshold into terrain types (below 0.3 = water, 0.3–0.7 = floor, above 0.7 = wall).

**Constrained-platform viability:** Moderate. Simplex noise requires per-cell gradient lookups and interpolation. For small grids (30×20) it's fast, but the implementation is non-trivial. Libraries exist for Rust (`noise` crate) but not for `no_std`. A fixed-point simplex implementation is feasible but would need to be written.

**"Don't close doors" value:** Medium. Noise is excellent for overworld terrain and biome distribution. For dungeon interiors, it produces blobby spaces without clear rooms or corridors. However, it could enrich the tile state layer (`simulation.md`) — noise-driven temperature or moisture gradients make floors feel more varied.

**Deterministic:** Yes, by definition (noise functions are deterministic).

**Composition role:** **Terrain detail layer.** Not a dungeon generator itself, but useful for distributing terrain types (water pools, grass patches) within an already-carved dungeon.

**Suitability: C+ for dungeon generation, B+ for terrain detail.** Not a primary generator, but valuable for environmental variety once the `Tile` enum expands.

---

### 11. Constraint Satisfaction

**How it works:** Formulate the dungeon as a logic problem. Define variables (tile types, room properties) and constraints (connectivity, difficulty, key placement). A solver finds valid assignments.

**Constrained-platform viability:** Poor. General CSP solvers are NP-hard and require backtracking search with unpredictable runtime and memory. Not suitable for GBA/C64.

**Deterministic:** Solver-dependent. Most solvers are deterministic with a fixed search order, but performance varies.

**Composition role:** **Validation and repair.** Rather than generating the dungeon via CSP, use it to validate properties of a dungeon generated by other means ("is the critical path solvable?", "are all locked doors reachable?"). This is tractable because the problem size is small (validate a specific property, don't generate from scratch).

**Suitability: D for generation, B for validation.** Too heavy for generation within `core`, but potentially useful as a development-time validation tool (behind `dev-tools` feature flag).

---

## Technique Comparison Matrix

| Technique | Platform Viability | "Don't Close Doors" | Deterministic | Data-Driven | Composition Role | Overall |
|---|---|---|---|---|---|---|
| BSP Trees | A | B | A | B+ | Macro layout | B+ |
| Cellular Automata | A | A | A | A | Room interiors / cave floors | A |
| Delaunay + MST | A- | A+ | A | B | Connectivity | **A** |
| WFC | D | B | B | A | Room detail | C |
| Drunkard's Walk | A | C | A | C | Corridor carver | B- |
| Prefabs | A+ | A+ | A | A | Special rooms | **A** |
| Graph Grammars | B | A+ (future) | A | A | Mission structure | C (now) |
| Voronoi Rooms | B | B | A | B | Room shapes | B- |
| Agent/Digger | A | C+ | A | C | Themed floors | C+ |
| Perlin/Simplex | B | B+ | A | B | Terrain detail | C+ |
| Constraint Sat. | D | B | B | A | Validation | D |

---

## Recommended Enhancements

Based on the evaluation, five enhancements are recommended, ordered by impact-to-effort ratio. Each is independently valuable and composes with the others.

### Enhancement 1: Delaunay/MST Connectivity with Loops

**Priority:** Highest. This is the single change that most transforms gameplay.

**What changes:** Replace the current "connect each room to the previous room" logic in `Map::generate()` with:
1. Compute Delaunay triangulation of room centers.
2. Extract minimum spanning tree (guarantees full connectivity).
3. Add back a configurable percentage of non-MST edges as loops.

**Why it matters:** The current linear chain topology means every pair of rooms has exactly one path between them. With loops, the player faces tactical choices: "Do I take the short route through the troll's room or the long route around?" Flee AI (gameplay plan Phase 5) becomes meaningful — a fleeing goblin can actually escape through a different corridor. Acoustic propagation picks up sounds from two directions. Auto-explore has genuinely different routes to choose from.

**New config:**

```toml
[config]
corridor_loop_chance = 0.15   # Fraction of non-MST Delaunay edges to add back
```

**Constrained-platform cost:** ~1.5 KB scratch memory for Delaunay triangulation of 30 rooms. Well within GBA EWRAM. On C64 (12 rooms), ~400 bytes.

**Files touched:** `map.rs` (new `connect_rooms_delaunay()` method), `data.rs` (new config field).

**Composition:** Works with any room placement method. The current random placement, BSP, or prefab placement all produce room centers that feed into Delaunay.

---

### Enhancement 2: Cellular Automata Cave Floors

**Priority:** High. Introduces visual and tactical variety with minimal code.

**What changes:** Add a new generation mode that uses cellular automata to produce cave-like levels. This runs as an alternative to `Map::generate()`, selectable via `MapPreset::Cave` or (in a multi-level dungeon) via floor-depth rules.

Separately, CA can be used as a post-processing step on individual rooms: after carving a rectangular room, run a few CA passes to erode the edges into organic shapes. This preserves the room-based structure while adding visual variety.

**Why it matters:** Every floor currently looks the same — rectangular rooms connected by L-corridors. Cave floors break the monotony and create different tactical situations. Open caverns reward ranged combat (future) while narrow tunnels favor melee. The acoustic propagation system behaves differently in caves (sound carries further in open spaces, is blocked by irregular walls).

**New config:**

```toml
[config]
cave_initial_wall_pct = 45     # Initial random wall density for CA
cave_smooth_passes = 5         # Number of CA smoothing iterations
cave_wall_threshold = 5        # Neighbor count to become/stay wall
cave_min_open_pct = 40         # Minimum floor percentage (retry if below)
```

**Constrained-platform cost:** Zero extra memory — operates on the existing tile array in-place. The most efficient algorithm in this document.

**Files touched:** `map.rs` (new `generate_cave()` method, new `MapPreset::Cave`), `data.rs` (new config fields).

**"Don't close doors" connection:** Cave levels are ideal for the `Tile::Water` and `Tile::Lava` variants anticipated in the `move_cost()` comment. A cave floor with underground pools is a natural fit for the terrain expansion in `simulation.md`.

---

### Enhancement 3: Prefab Special Rooms

**Priority:** High. Essential for multi-level dungeons, and the most direct path to memorable dungeon moments.

**What changes:** Add a template system where hand-authored rooms can be injected into procedurally generated floors. Each template is a small ASCII grid with tile types and metadata (connection points, difficulty tier, special flags).

Templates integrate with the existing `MapPreset` system but at the room level rather than the floor level. The generator places N-1 rooms procedurally, then selects one prefab template for a special room (stair room, vault, trap room) and places it with connection points aligned to the corridor network.

**Why it matters:** Prefabs are where game design intent is strongest. A narrow-entrance vault with treasure creates a risk/reward decision. A staircase room with multiple exits creates a landmark. A flooded chamber with `Water` tiles exercises terrain mechanics. Prefabs are also the mechanism for **difficulty authoring** — a "goblin warren" template spawns many weak enemies, while a "troll den" template spawns one strong enemy in a tight space.

**Data format option (in `game.toml` or separate files):**

```toml
[[room_templates]]
name = "vault"
min_depth = 3
tags = ["treasure", "guarded"]
width = 7
height = 5
# '.' = floor, '#' = wall, '+' = connection point, '>' = stairs down
layout = """
#######
#.....#
+.....+
#.....#
#######
"""
```

**Constrained-platform cost:** Templates are `const` data. 20 templates × ~200 bytes = ~4 KB, living in ROM on GBA/C64. Zero RAM cost beyond the tiles they carve into the map.

**Files touched:** `map.rs` (template parsing and placement), `data.rs` (template definitions), new `templates.rs` or extension of `map.rs`.

**"Don't close doors" connection:** Prefabs are the natural integration point for items (Phase 3), stairs (Phase 2), and special encounters. A "chest room" prefab can spawn items from the item table. A "stairwell" prefab standardizes stair placement. As the item and monster vocabularies grow via `game.toml`, prefabs become more varied without code changes.

---

### Enhancement 4: BSP as an Alternative Layout

**Priority:** Medium. Adds layout variety and better spatial distribution.

**What changes:** Add `MapPreset::Bsp` (or integrate BSP as a configurable option within the standard generator). BSP recursively subdivides the map, placing one room per leaf and connecting siblings.

**Why it matters:** BSP produces more evenly distributed rooms than random placement. The current generator can cluster rooms in one corner of the map (placement is purely random). BSP guarantees spatial spread. It also produces a natural tree structure that's useful for difficulty scaling — nodes deeper in the tree are "further" from the root, mapping cleanly to "further from the player start."

**Constrained-platform cost:** ~1.2 KB for the BSP tree (60 nodes × 20 bytes). Integer-only. Stack depth ~5 levels for 30 rooms.

**Files touched:** `map.rs` (new `generate_bsp()` method, new `MapPreset::Bsp`).

**Composition:** BSP room placement feeds directly into Enhancement 1 (Delaunay/MST connectivity). Together they produce well-distributed rooms with loop-rich topology.

---

### Enhancement 5: Non-Rectangular Room Shapes

**Priority:** Lower. Visual variety with moderate implementation cost.

**What changes:** Allow rooms to be non-rectangular by combining overlapping rectangles, applying CA erosion to room edges, or using simple geometric shapes (circles, crosses, L-shapes).

The simplest approach: after carving a rectangular room, run 1–2 passes of CA smoothing on just the room's bounding area. This nibbles at corners and edges, producing organic shapes while maintaining the `Rect` bounding box for spawning and placement logic.

**Why it matters:** Breaking the rectangle monotony is the most visible quality-of-life improvement for dungeon variety. Players notice room shapes immediately.

**Constrained-platform cost:** Negligible if using the CA-erosion approach — operates on the existing tile array, bounded to the room's bounding rect.

**Files touched:** `map.rs` (post-processing step after `carve_room()`).

---

## Pipeline Architecture

The recommended enhancements compose into a layered generation pipeline:

```
┌─────────────────────────────────────────────────┐
│  1. Floor Type Selection                        │
│     Choose: dungeon (rooms+corridors) or cave   │
│     Input: depth, seed, config                  │
├────────────────────┬────────────────────────────┤
│  Dungeon Path      │  Cave Path                 │
│                    │                             │
│  2a. Room Layout   │  2b. Cellular Automata     │
│  BSP or random     │  Init → smooth → connect   │
│  placement         │                             │
│                    │  (skip to step 5)           │
│  3. Prefab Insert  │                             │
│  Replace 1-2 rooms │                             │
│  with templates    │                             │
│                    │                             │
│  4. Connectivity   │                             │
│  Delaunay + MST    │                             │
│  + loop edges      │                             │
│                    │                             │
│  5. Room Shaping   │                             │
│  CA erosion on     │                             │
│  room interiors    │                             │
│  (optional)        │                             │
├────────────────────┴────────────────────────────┤
│  6. Structural Walls                            │
│     compute_structural_walls() [existing]       │
│                                                 │
│  7. Feature Placement                           │
│     Stairs, items, monsters [existing + planned]│
└─────────────────────────────────────────────────┘
```

Each step is independently testable. Steps can be enabled/disabled via config. The pipeline produces a `Map` with the same `tiles: Vec<Tile>` and `rooms: Vec<Rect>` that all downstream systems already consume.

### Floor Type Selection for Multi-Level Dungeons

When stairs are implemented (gameplay plan Phase 2), the floor type can vary by depth:

```toml
[depth_generation]
# Floor types cycle or are selected by depth rules
# "dungeon" = rooms + corridors, "cave" = cellular automata
floor_types = ["dungeon", "dungeon", "cave", "dungeon", "cave"]
# Or rule-based:
cave_chance_per_floor = 0.2   # 20% chance each floor is a cave
```

This creates the multi-level dungeon variety that roguelikes like Brogue and DCSS achieve: some floors are constructed dungeons, others are natural caverns.

---

## Implementation Order

```
Enhancement 1: Delaunay/MST Connectivity (Effort: M)
    Highest gameplay impact. Do this first.
    The existing room placement stays — only corridor routing changes.

Enhancement 2: Cellular Automata Caves (Effort: S-M)
    Independent of Enhancement 1. Can develop in parallel.
    Adds a new MapPreset::Cave.

Enhancement 3: Prefab Special Rooms (Effort: M)
    Benefits from Enhancement 1 (prefabs connect to the
    Delaunay graph). Best after connectivity is improved.

Enhancement 4: BSP Layout (Effort: M)
    Independent alternative to random placement.
    Can develop in parallel with 1-3.

Enhancement 5: Room Shaping (Effort: S)
    Quick win. Can apply after any of the above.
    Just a post-processing pass on carved rooms.
```

Enhancements 1 and 2 are the priority. Together they transform the dungeon from "a chain of rectangles" into "a looping network of varied spaces." Enhancement 3 becomes essential once stairs exist. Enhancement 4 adds layout variety. Enhancement 5 is polish.

---

## Techniques Deferred

| Technique | Why deferred | Revisit when |
|-----------|-------------|--------------|
| WFC | Too memory-heavy for constrained platforms; high implementation complexity for uncertain gameplay benefit | Web-only "deluxe" mode, or room-interior decoration for desktop builds |
| Graph Grammars | Requires key-lock mechanics that don't exist yet | After items (Phase 3) + locked doors |
| Voronoi Rooms | High refactoring cost to replace `Rect`-based architecture | Major architecture revision, if ever |
| Constraint Satisfaction | NP-hard, unsuitable for constrained platforms | Dev-tools validation (ensure maps are solvable) |
| Perlin/Simplex Noise | Not a dungeon generator; requires tile type expansion | After `Tile` enum expands (Water, Lava, etc.) for terrain detail distribution |

---

## Relationship to Existing Design Docs

| Doc | Relationship |
|-----|-------------|
| [gameplay-implementation-plan.md](gameplay-implementation-plan.md) | Phase 2 (stairs) needs floor-type variety — this doc provides it. Phase 3 (items) motivates prefab rooms. Phase 5 (creature mood) benefits from loops (flee routes). |
| [simulation.md](../architecture/simulation.md) | Tile state layer and CA simulation benefit from organic terrain. Property bitfields can tag rooms/corridors for generation rules. |
| [acoustic-propagation.md](acoustic-propagation.md) | Sound propagation behaves differently in caves vs. corridors vs. open rooms. Diverse geometry makes the sound system more interesting. |
| [gba-port.md](../platforms/gba-port.md) | All recommended enhancements respect GBA memory/CPU constraints. The SimBudget pattern applies to generation (smaller room caps, fewer CA passes on constrained platforms). |
| [c64-port-proposal.md](../platforms/c64-port-proposal.md) | C64 maps are 40×22 with 12 rooms. All techniques scale down. CA caves are especially cheap. Delaunay with 12 points is trivial. |
| [cross-platform.md](../architecture/cross-platform.md) | Generation stays in `core` with zero platform deps. Platform-specific caps come through `GameConfig` / `SimBudget`, not conditional compilation in the generator. |

---

## Open Questions

1. **Corridor style for Delaunay edges.** L-shaped (current), A\*-pathed (follows existing floor where possible), or drunkard's walk (organic)? Each produces a different feel. Could be configurable per-floor or per-edge.

2. **Cave connectivity repair.** When CA produces disconnected regions, should we fill small regions (simpler, fewer total tiles) or tunnel between them (more complex, preserves more space)? Tunneling preserves more floor area for gameplay but adds implementation complexity.

3. **Prefab data format.** Inline in `game.toml` (simpler, single file) vs. separate `.room` files (cleaner for large template libraries)? The current `data-files` feature flag pattern suggests TOML integration, but a room library might outgrow a single TOML file.

4. **BSP vs. random placement as default.** Should the standard generator switch to BSP, or should BSP be an opt-in alternative? BSP produces more predictable layouts, which could be either a pro (consistent difficulty) or a con (less surprising).

5. **Generation-time budget on constrained platforms.** Should there be a `SimBudget`-style cap on generation time/passes (e.g., max CA iterations, max Delaunay edges), or is generation a one-time cost that can afford to be slower? On C64, generation happens once per floor load — even a 100ms generation time is fine because the player is looking at a "descending..." message.

6. **Room shape representation.** If non-rectangular rooms are added via CA erosion, `Rect` still works as a bounding box for spawning. But should `Room` become a richer type (bounding rect + actual floor tile set) for more precise spawning and feature placement? This is a tradeoff between accuracy and the simplicity of the current `Rect`-everywhere approach.
