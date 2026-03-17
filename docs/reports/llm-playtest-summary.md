# LLM Playtest Summary

Consolidated findings from six playtest sessions (2026-02-13 through 2026-03-17) where Claude Opus 4.6 played the roguelike via MCP tools. Sessions 1-5 used the standard tier; Session 6 used the micro tier (no_std, C64-compatible). See individual session reports for full detail.

## Cross-Session Efficiency

| Metric | Session 1 | Session 2 | Session 3 | Session 4 | Session 5 | Session 6 |
|--------|-----------|-----------|-----------|-----------|-----------|-----------|
| Tier | Standard | Standard | Standard | Standard | Standard | **Micro** |
| Tool calls | ~80 | ~70 | ~35 | ~30 | ~30 | **~150** |
| Wasted calls | ~40 (50%) | ~40 (57%) | ~2 (6%) | ~0 (0%) | ~0 (0%) | **~0 (0%)** |
| Floors | 1 | 1 | 1 | 1 | 1 | **5** |
| Explored | ~100% | 84% | 100% | 100% | 100% | **100% (D1-4)** |
| Kills (best) | 7 | 5 | 11 | 4 | 18 | **18** |
| Final HP | 2/30 | 30/30 | 30/30 | 30/30 | 30/30 | **26/30** |
| Items | No | No | No | No | Yes | **Yes** |
| Session end | Near-death | Stuck at 84% | Cleared | Cleared | Cleared | Budget cap |
| Primary tool | autorun | autorun | pathfind_to | auto_explore | auto_explore | **auto_explore** |

Tool call waste dropped from 50-57% (Sessions 1-2) to 0% (Sessions 4-6). Session 6 has higher total calls because the game is 5 floors deep.

## Key Gameplay Findings

### The attrition → regen → too-easy arc

- **Session 1:** No healing. 17 monsters × ~2+ HP each = guaranteed death. Game was a countdown.
- **Session 2:** HP regen added (1 HP / 3 turns). Game became playable and strategic.
- **Sessions 3-4:** Regen now too generous — player fully heals between every encounter. No resource tension remains.

### Combat depth

Combat is solved arithmetic across all sessions. Every encounter has exactly one correct answer:
- Goblin: ~1 HP cost → always fight
- Orc: ~5 HP cost → always fight
- Troll: ~37 HP cost → always flee (but trolls never spawned in Sessions 3-4)

No positioning, resource, or tactical decisions exist. `auto_fight` perfectly matches the combat depth.

### Missing win condition

Flagged in all four sessions. After clearing the dungeon, the game simply continues with nothing left to do. A descending staircase would immediately create meaningful decisions.

## Recommendations

### Resolved

| Recommendation | Session | Resolution |
|---|---|---|
| HP regeneration | 1 | 1 HP / 3 turns (possibly too generous) |
| Autorun room awareness | 1, 2 | Removed RoomTransition, refined CorridorBranches |
| `pathfind_to` tool | 1, 2 | A* pathfinding through explored tiles |
| Frontier markers on explored map | 1, 2 | `~` glyphs + `frontier_exits` coordinates |
| Omniscient info removed | 1 | `total_monsters`/`total_rooms` removed |
| `auto_explore` tool | 3 | Frontier detection + pathfinding in one call |
| `new_tiles_revealed` feedback | 3 | Added to pathfind/autorun/auto_explore responses |
| Win condition (stairs/exit) | 1-4 | 5-floor dungeon with descend mechanic |
| Guarantee troll spawns | 3, 4 | Trolls spawn reliably on depth 2+ |
| Combat depth — items | 2-4 | Health Potions, Short Sword, Leather Armor |
| Micro tier pathfinding | 6 | no_std BFS for auto_explore/pathfind_to/auto_fight |
| Stairs discovery | 6 | StairsFound autorun stop + stairs coords in observe |
| Item action discoverability | 6 | Error messages list all valid actions |

### Unresolved

| Recommendation | Sessions | Priority |
|---|---|---|
| Mid-combat potion use | 6 | Medium |
| Increase monster density (later floors) | 2-4, 6 | Medium |
| Rebalance regen (cap at 70% or stationary-only) | 3, 4 | Medium |
| Damage variance | 2-4 | Low |
| Multi-entrance rooms / anti-chokepoint | 1-3 | Low |
| Room exit metadata in observations | 1-4 | Low |

## MCP Design Principles Validated

1. **Server handles mechanics, LLM handles strategy** — confirmed across all 4 sessions
2. **One tool call = one meaningful decision** — `auto_explore` achieves this for exploration
3. **Proactive observation data** — returning observations from every action eliminates redundant `observe` calls
4. **Structured metadata over ASCII parsing** — `frontier_exits`, `new_tiles_revealed`, `explore_target_x/y` are more reliable than visual ASCII interpretation
5. **Hierarchical world representation** — tactical FOV layer + strategic exploration graph serves different cognitive tasks with appropriate data structures

## Individual Sessions

- [Session 1](llm-playtest-session-1.md) — Baseline. No healing, no pathfinding. Identified core MCP pain points.
- [Session 2](llm-playtest-session-2.md) — HP regen added. Still suffered from autorun/navigation issues.
- [Session 3](llm-playtest-session-3.md) — pathfind_to + frontier markers. Efficiency nearly doubled.
- [Session 4](llm-playtest-session-4.md) — auto_explore. MCP interface reached maturity. Bottleneck shifted from interface to content.
- [Session 5](llm-playtest-session-5.md) — Item system validation. Equipment creates meaningful troll gate.
- [Session 6](llm-playtest-session-6.md) — Micro tier. BFS pathfinding, StairsFound, stairs coords, auto_fight. Reached depth 5.
