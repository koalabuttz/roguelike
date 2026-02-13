# MCP Roguelike — Optimization Ideas for AI Play

Learned from playtesting a roguelike via MCP tools. Core principle: **the MCP server should handle mechanical execution, the LLM should handle strategic decisions.** Every API call should ideally involve a real choice.

## Pain Points Observed
1. **Junction-hopping** — Autorun stops at every corridor branch, even when direction is obvious. Most turns are navigation, not tactics.
2. **Exit alignment** — ASCII map requires character-counting to find exit gaps. Frequent wall-bump retries.
3. **No map memory** — Only FOV is returned. LLM must mentally track explored areas for strategic planning.
4. **Trivial combat is verbose** — Goblin fights are foregone conclusions (2 HP cost, guaranteed win) but cost 2-3 API calls each.

## Proposed Improvements

### New Actions (highest impact)
- **`auto_explore`** — Move toward unexplored areas until monster spotted, damage taken, or dead end. Collapses ~70% of exploration turns into one call.
- **`pathfind_to(x, y)`** — Auto-navigate to a known coordinate. Eliminates wall-bump retries and exit-alignment fumbling.
- **`auto_fight`** — For trivial encounters (e.g., goblin at 20+ HP), resolve entire combat in one call. Return outcome summary.
- **`flee_to_corridor`** — Pathfind to nearest 1-wide corridor for tactical positioning.

### Smarter Autorun
- Don't stop at junctions when path is obvious — if running east and hitting a T-junction with no monsters, keep going east.
- "Travel" mode that follows corridors through turns, stopping only at rooms, monsters, or dead ends.

### Protocol-Level
- **Batch actions** — Submit multiple actions, get back final state (or state where something interrupted).
- **Decision-only mode** — Server advances through trivial states automatically, only returns when player input actually matters.

## Observe Data: Layered Architecture

The naive approach (send full ASCII map) has three problems:
1. **Token cost** — Full 80x40 map is ~3200 chars per call, adds up fast.
2. **Three-state ambiguity** — ASCII can't distinguish unexplored / explored-not-in-FOV / in-FOV. Terminals use black/grey/bright; flat ASCII misleads the LLM about data reliability (monsters may have moved in explored-but-not-visible areas).
3. **Redundancy** — Most of the map doesn't change between calls.

### Solution: Hierarchical Layers

**Layer 1 — FOV (always sent, ASCII)**
Keep the current ~15x15 ASCII around the player. This is the **tactical** view for combat positioning and immediate threats. FOV boundary is implicit (edge of rendered area). Small, always relevant.

**Layer 2 — Exploration graph (always sent when changed, structured JSON)**
Represent explored areas as a room/corridor graph instead of ASCII:
```json
"exploration": {
  "current_room": "room_5",
  "rooms": {
    "room_5": {"cleared": true, "exits": {"north": "corridor_4", "east": "room_6"}},
    "room_6": {"cleared": false, "known_threats": ["Troll (20hp)"], "exits": {"west": "room_5"}}
  },
  "unexplored_exits": ["room_5/east", "corridor_4/north"],
  "explored_pct": 45
}
```
- ~50 tokens vs ~3200 for full ASCII map
- Naturally handles three-state problem: unexplored = listed in `unexplored_exits`, explored-not-visible = rooms in graph with stale threat data, in-FOV = the ASCII map
- Gives strategic context (where to go, what to avoid) without spatial noise

**Layer 3 — Delta/conditional (only when changed)**
- If exploration graph hasn't changed (mid-combat, autorunning explored corridor): omit layer 2, send `"exploration_unchanged": true`
- If major changes occurred (new room, monster killed): send updated graph
- Reconciles with "omit unchanged data" optimization — common case is cheap

**Layer 4 — Full ASCII map (on-demand via separate tool)**
- Add `get_map` tool for full explored map with markers (`~` = unexplored adjacent, `.` = explored, FOV area highlighted)
- LLM calls this only at strategic decision points ("where should I go next?"), not every turn
- Expensive view available when needed, not forced on every call

### How This Plays Out
| Situation | Data sent |
|---|---|
| Typical exploration turn | FOV + `exploration_unchanged: true` |
| New room discovered | FOV + updated exploration graph |
| Strategic decision point | LLM calls `get_map` for full ASCII |
| Mid-combat | FOV + `exploration_unchanged: true` |

### Design Pattern
This mirrors **hierarchical world representation** in game AI: tactical layer (grid-based FOV for positioning) + strategic layer (topological room graph for navigation). Different cognitive tasks need different data structures. Also mirrors **delta compression** in game netcode — only send what changed.

## Game Stats Summary
Include alongside observe data: kills, rooms explored, % map revealed, nearest known monsters. Cheap in tokens, high strategic value.

## Impact Estimates
| Optimization | Savings |
|---|---|
| `auto_explore` | ~70% of exploration turns |
| `pathfind_to` | ~50% of navigation fumbling |
| `auto_fight` (trivial) | 2-3 calls per weak monster |
| Layered observe data | Eliminates wall-bump retries + saves tokens |
| Smarter autorun | ~30% of corridor traversal |

## General MCP Game Design Principle
Separate the **pathfinding layer** (mechanical) from the **decision layer** (strategic). The MCP tool boundary should sit between them. Mechanical execution belongs server-side; strategic choices are the LLM's job.
