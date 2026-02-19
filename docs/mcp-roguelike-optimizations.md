# MCP Roguelike — Optimization Ideas for AI Play

Learned from playtesting a roguelike via MCP tools. Core principle: **the MCP server should handle mechanical execution, the LLM should handle strategic decisions.** Every API call should ideally involve a real choice.

## Status

Most optimizations from this doc have been implemented. See "Still Open" for remaining ideas.

## Pain Points Observed
1. **Junction-hopping** — Autorun stops at every corridor branch, even when direction is obvious. Most turns are navigation, not tactics.
2. **Exit alignment** — ASCII map requires character-counting to find exit gaps. Frequent wall-bump retries.
3. **No map memory** — Only FOV is returned. LLM must mentally track explored areas for strategic planning.
4. **Trivial combat is verbose** — Goblin fights are foregone conclusions (2 HP cost, guaranteed win) but cost 2-3 API calls each.

## Implemented

### New Actions
- ~~**`auto_explore`**~~ — Done. Finds nearest frontier, pathfinds to it. Stops for monsters, damage, or when frontier is reached.
- ~~**`pathfind_to(x, y)`**~~ — Done. A\* pathfind to any visible/explored coordinate. Stops for monsters or damage.
- ~~**`auto_fight`**~~ — Done. Resolves combat with the weakest adjacent monster to the death in one call. Returns fight metadata (rounds, HP lost, target killed).
- **`flee_to_corridor`** — Not implemented. Could be built on existing pathfinding.

### Smarter Autorun
- ~~Don't stop at junctions when path is obvious~~ — Done. Autorun tuned to stop only at meaningful decision points (corridor forks, room exits), not every open-neighbor junction.

### Protocol-Level (Still Open)
- **Batch actions** — Submit multiple actions, get back final state (or state where something interrupted).
- **Decision-only mode** — Server advances through trivial states automatically, only returns when player input actually matters.

## Observe Data: Layered Architecture (Implemented)

The naive approach (send full ASCII map) has three problems:
1. **Token cost** — Full 80x40 map is ~3200 chars per call, adds up fast.
2. **Three-state ambiguity** — ASCII can't distinguish unexplored / explored-not-in-FOV / in-FOV. Terminals use black/grey/bright; flat ASCII misleads the LLM about data reliability (monsters may have moved in explored-but-not-visible areas).
3. **Redundancy** — Most of the map doesn't change between calls.

### Solution: Hierarchical Layers

All four layers are now implemented.

~~**Layer 1 — FOV (always sent, ASCII)**~~ Done.
The current FOV ASCII around the player — the **tactical** view for combat positioning and immediate threats. Can be omitted via `compact` mode for LLMs that only need stats and entity info.

~~**Layer 2 — Exploration graph (always sent when changed, structured JSON)**~~ Done (`crates/core/src/exploration_graph.rs`, integrated into MCP server).
Explored areas represented as a room/corridor graph with per-room metadata:
```json
"exploration": {
  "current_room": 5,
  "rooms": [
    {"id": 5, "cleared": true, "exits": [{"direction": "north", "explored": true, "target_room": 4}], "distance": 0, ...},
    {"id": 6, "cleared": false, "monsters": [{"name": "Troll", "hp": 20, "max_hp": 20}], "distance": 12, ...}
  ],
  "corridor_frontiers": [{"x": 45, "y": 12, "dead_end": false}]
}
```
- ~50 tokens vs ~3200 for full ASCII map
- Naturally handles three-state problem: unexplored = `explored: false` rooms with no distance, explored-not-visible = rooms in graph with stale threat data, in-FOV = the ASCII map
- Gives strategic context (where to go, what to avoid) without spatial noise
- Includes A\* pathfinding distance from player to each explored room center

~~**Layer 3 — Delta/conditional (only when changed)**~~ Done (fingerprint-based delta compression in `mcp_server.rs`).
- `exploration_graph_fingerprint()` hashes relevant game state; graph is only serialized when fingerprint changes
- First observation after `new_game`/`load_game` always includes full graph (`force = true`)
- Subsequent calls get `"exploration_unchanged": true` if topology hasn't shifted

~~**Layer 4 — Full ASCII map (on-demand via separate tool)**~~ Done (`get_explored_map` MCP tool).
- Returns full explored map with frontier markers (`~` = unexplored adjacent), entity glyphs at current positions if in FOV
- Includes `frontier_exits` coordinates for easy `pathfind_to` navigation
- LLM calls this only at strategic decision points, not every turn

### How This Plays Out
| Situation | Data sent |
|---|---|
| Typical exploration turn | FOV + `exploration_unchanged: true` |
| New room discovered | FOV + updated exploration graph |
| Strategic decision point | LLM calls `get_explored_map` for full ASCII |
| Mid-combat | FOV + `exploration_unchanged: true` |

### Design Pattern
This mirrors **hierarchical world representation** in game AI: tactical layer (grid-based FOV for positioning) + strategic layer (topological room graph for navigation). Different cognitive tasks need different data structures. Also mirrors **delta compression** in game netcode — only send what changed.

## ~~Game Stats Summary~~ Done.
Kills, rooms found, explored %, steps taken, and seed code are included in every observation response. Compact mode shortens field names (`player_hp` → `hp`, `visible_entities` → `entities`, etc.) to reduce per-turn token overhead.

## Impact Estimates
| Optimization | Savings | Status |
|---|---|---|
| `auto_explore` | ~70% of exploration turns | Done |
| `pathfind_to` | ~50% of navigation fumbling | Done |
| `auto_fight` (trivial) | 2-3 calls per weak monster | Done |
| Layered observe data | Eliminates wall-bump retries + saves tokens | Done |
| Smarter autorun | ~30% of corridor traversal | Done |
| Compact mode | Omits ASCII map, shortens field names | Done |

## Still Open
- **`flee_to_corridor`** — Pathfind to nearest 1-wide corridor for tactical positioning.
- **Batch actions** — Submit multiple actions, get back final state (or state where something interrupted).
- **Decision-only mode** — Server advances through trivial states automatically, only returns when player input actually matters.

## General MCP Game Design Principle
Separate the **pathfinding layer** (mechanical) from the **decision layer** (strategic). The MCP tool boundary should sit between them. Mechanical execution belongs server-side; strategic choices are the LLM's job.
