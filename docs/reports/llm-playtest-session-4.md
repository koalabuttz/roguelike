# LLM Playtest Report — 2026-02-13 (Session 4)

**Player:** Claude Opus 4.6 via MCP server
**Session length:** ~30 tool calls
**Result:** 100% explored, alive at 30/30 HP. Dungeon fully cleared.
**Kills:** 4 (3 goblins, 1 orc, 0 trolls)
**Rooms found:** 10

## What Changed Since Session 3

The developer added:
- **`auto_explore` tool** — the single biggest improvement across all sessions. Combines
  frontier detection + pathfinding into one call. This was the #1 recommendation in the
  optimizations doc ("collapses ~70% of exploration turns into one call") and it delivers.
- **`new_tiles_revealed` field** in pathfind/autorun/auto_explore responses — Session 3
  specifically requested this. Provides immediate feedback on whether a frontier was productive.
- **`explore_target_x/y`** in auto_explore responses — shows where the agent is heading,
  useful for understanding movement decisions.

## MCP Server Observations

### 1. `auto_explore` is transformative (MAJOR POSITIVE)

This is the most impactful tool addition across all four sessions. The exploration loop is
now a single repeated call:

```
auto_explore → (stops for monster/frontier) → auto_fight or auto_explore again
```

Compare the exploration strategies across sessions:

| Session | Exploration method | Calls to explore 100% |
|---------|-------------------|----------------------|
| 1 | Manual move + autorun | ~80 (never reached 100%) |
| 2 | Manual move + autorun + get_explored_map | ~70 (stuck at 84%) |
| 3 | get_explored_map → pick frontier → pathfind_to | ~35 |
| **4** | **auto_explore (repeat)** | **~30** |

The key insight from the optimizations doc holds: "the MCP server should handle mechanical
execution, the LLM should handle strategic decisions." With `auto_explore`, the LLM doesn't
even need to *pick* which frontier to visit — the server chooses the nearest one. This is
the right default for exploration, where "go to the closest unknown area" is almost always
correct.

### 2. `new_tiles_revealed` is useful but underutilized (MINOR POSITIVE)

The field appears in every auto_explore response, which is great. However, there's no
corresponding feedback when a frontier turns out to be a dead end (0 new tiles, wall beyond).
The LLM sees `new_tiles_revealed: 1` and has to call `auto_explore` again to discover the
frontier list shrank. Consider: when `new_tiles_revealed` is low (0-2), include a hint
like `"dead_end": true` so the LLM knows not to revisit that area.

### 3. `frontier_exits` list can be excessively long (LOW)

Several auto_explore responses returned 15-20+ frontier exits. This is mostly token waste —
the LLM is calling `auto_explore` which picks the best frontier automatically. The long list
only matters if the LLM wants to override the auto selection (e.g., to go to a specific area
of interest). Consider:
- Trimming to the nearest 5 frontiers by default
- Or omitting `frontier_exits` from `auto_explore` entirely (it's more relevant to
  `get_explored_map` where the LLM is manually planning)
- Or adding a `frontier_count` summary field instead

### 4. Auto_explore stop reasons are well-calibrated (POSITIVE)

Across ~20 auto_explore calls, I observed:
- `path_complete` — reached the frontier target, normal behavior
- `monster_spotted` — correctly interrupted exploration for combat (3 times)
- No false stops, no spurious interruptions

This is a huge contrast with Sessions 1-2 where autorun stopped constantly at phantom
"junctions." The stop reason system is now tight and predictable.

### 5. The `observe` tool is now nearly obsolete (OBSERVATION)

I never called `observe` once this session. Every action tool (`auto_explore`, `pathfind_to`,
`auto_fight`, `act`) returns a full observation. The only reason to call `observe` would be
to check state without taking an action, which never came up. This is good design — the
server proactively returns everything the LLM needs.

### 6. Room exit metadata still missing (MEDIUM — Sessions 1, 2, 3, 4)

This has been flagged in every session. With `auto_explore` handling most navigation, it's
less critical than ever. But when the LLM *does* want to make a manual navigation decision
(e.g., "I want to explore the east wing before the west wing"), it still has to parse ASCII
to understand room topology. A `room_exits` field would help, though the priority has
rightfully dropped given auto_explore.

## Gameplay Observations

### 1. No win condition — the game's most pressing problem (Sessions 1, 2, 3, 4)

This has been the #1 unaddressed recommendation across all four sessions. After 100%
exploration and killing every monster, the game simply... continues. There is no:
- Stairs to descend
- Boss to defeat
- Treasure to collect
- Score screen
- "You win" message

The MCP tooling is now excellent. The core exploration/combat loop is satisfying. But without
a goal, there's no tension driving the experience. Every session ends the same way: "dungeon
cleared, nothing left to do." This should be the top development priority.

**Minimum viable implementation:** Add descending stairs (`>`) placed in a random room.
Walking onto them ends the level. Even this simple addition would create meaningful decisions:
"Do I keep exploring for more kills, or descend while healthy?"

### 2. No trolls — again (Sessions 3, 4)

For the second consecutive session, zero trolls spawned. The troll is the *only* monster that
threatens the player (4 dmg/turn × 10 rounds = 40 total HP, exceeding max HP of 30). Without
it, every combat encounter is a foregone conclusion:

| Monster | Rounds to kill | Net HP cost | Decision |
|---------|---------------|-------------|----------|
| Goblin | 2 | ~1 HP | Always fight |
| Orc | 3 | ~5 HP | Always fight |
| **Troll** | **10** | **~37 HP** | **Fight or flee?** |

The troll is the only enemy that would force the LLM to make a real tactical decision.
Session 3 recommended guaranteeing at least one troll per dungeon — this would dramatically
improve the experience.

### 3. Monster density is too low (Sessions 2, 3, 4)

Only **4 monsters** across **10 rooms** and the entire map. That's 0.4 monsters per room.
Many rooms were completely empty. The dungeon felt more like a maze puzzle than a hostile
environment.

| Session | Rooms | Monsters | Ratio |
|---------|-------|----------|-------|
| 1 | 8 | 7+ | ~0.9/room |
| 2 | 11 | 5 | ~0.5/room |
| 3 | 9 | 11 | ~1.2/room |
| **4** | **10** | **4** | **0.4/room** |

Session 3 had the best density at ~1.2 monsters/room. Session 4 was the lowest. With
auto_explore making navigation trivial, the game needs more combat encounters to maintain
engagement. Consider:
- Minimum 1 monster per room
- Monster groups (2-3 goblins, orc + goblin pairs)
- Corridor patrols (monsters in corridors, not just rooms)

### 4. HP regeneration remains too generous (Sessions 3, 4)

Identical finding to Session 3. My lowest HP was **27/30** (after the single orc fight). I
entered every subsequent fight at 30/30. The regen rate (1 HP / 3 turns) combined with the
long travel distances between encounters means the player always fully heals.

The problem is compounded by low monster density — fewer fights means more walking means
more regen. If monster density increases, regen may naturally feel more balanced. But even
so, Session 3's suggestion of capping regen at 50-70% max HP or making it stationary-only
would create meaningful resource tension.

### 5. Combat is still pure arithmetic (Sessions 2, 3, 4)

Third session in a row flagging this. The combat decision tree is:
1. Is it a troll? → Never spawns, so moot
2. Is it anything else? → `auto_fight`

There are no positioning decisions (corridors are always 1-wide), no resource decisions
(always at full HP, no items), no risk assessment (outcome is deterministic). The `auto_fight`
command perfectly matches the combat depth — which is to say, combat is a single button press.

Suggestions from previous sessions remain valid:
- Damage variance (ATK +/- 1-2) for uncertainty
- Items (potions, weapons) for resource decisions
- Special abilities (goblin ambush, orc charge) for tactical variety
- Terrain effects for positional play

### 6. Exploration flow is the game's strongest feature (POSITIVE — Sessions 3, 4)

The procedural dungeon generation creates satisfying layouts. This session's dungeon was a
sprawling network with a central hub, long east-west arteries, and a southwestern complex
of rooms. Each auto_explore call revealed meaningful new territory. The FOV radius of 8
creates genuine anticipation as corridors open into rooms.

With the MCP tooling now mature, the exploration experience is genuinely good. The bottleneck
is no longer the interface — it's the game content waiting at the end of each corridor.

## Session Statistics

| Metric | Value |
|--------|-------|
| Total tool calls | ~30 |
| `auto_explore` calls | ~20 (67%) |
| `pathfind_to` calls | 3 (10%) |
| `auto_fight` calls | 3 (10%) |
| `get_explored_map` calls | 2 (7%) |
| `get_rules` / `new_game` | 2 (7%) |
| Wasted calls | ~0 (0%) |
| Final HP | 30/30 |
| Lowest HP | 27/30 |
| Explored | 100% |
| Rooms found | 10 |
| Kills | 4 (3 goblins, 1 orc, 0 trolls) |
| Cause of session end | Dungeon fully cleared, nothing left to do |

## Efficiency Comparison Across All Sessions

| Metric | Session 1 | Session 2 | Session 3 | Session 4 |
|--------|-----------|-----------|-----------|-----------|
| Tool calls | ~80 | ~70 | ~35 | **~30** |
| Wasted calls | ~40 (50%) | ~40 (57%) | ~2 (6%) | **~0 (0%)** |
| Explored | ~100% | 84% | 100% | **100%** |
| Kills | 7 | 5 | 11 | 4 |
| Final HP | 2/30 | 30/30 | 30/30 | 30/30 |
| Lowest HP | 2/30 | 24/30 | 24/30 | 27/30 |
| Session end | Near-death | Stuck at 84% | Cleared | **Cleared** |
| Primary tool | autorun | autorun | pathfind_to | **auto_explore** |

The MCP interface has reached maturity. Tool call waste dropped from 50-57% (Sessions 1-2)
to 6% (Session 3) to effectively 0% (Session 4). Every tool call this session produced
meaningful game progress. The optimization doc's principle — "the MCP tool boundary should
sit between mechanical execution and strategic decisions" — is now fully realized.

**The bottleneck has shifted from interface to content.** The server expertly handles
navigation, exploration, and combat execution. What's missing is game content that creates
interesting strategic decisions for the LLM to make.

## Priority Recommendations

### MCP tooling — largely complete:
1. ~~HP regeneration~~ — working (possibly too generous)
2. ~~Autorun room awareness~~ — working well
3. ~~`pathfind_to` tool~~ — working excellently
4. ~~Frontier markers on explored map~~ — working excellently
5. ~~`auto_explore` tool~~ — working excellently, biggest single improvement
6. ~~`new_tiles_revealed` feedback~~ — working
7. ~~`auto_fight` command~~ — working well
8. **Trim `frontier_exits` from auto_explore responses** (LOW) — save tokens
9. **Room exit metadata** (LOW) — less critical now with auto_explore

### Gameplay — the development frontier:
1. **Win condition (stairs/exit)** — most important missing feature, flagged in ALL sessions
2. **Guarantee troll spawns** — the only monster creating real decisions never appears
3. **Increase monster density** — 0.4 per room is too sparse, aim for 1-2
4. **Rebalance regen** — cap at 70% HP, or make it stationary-only
5. **Combat depth** — items, damage variance, special abilities, terrain
6. **Multi-entrance rooms** — break the corridor chokepoint meta

### MCP design principles validated:
- **Server handles mechanics, LLM handles strategy** — confirmed across 4 sessions
- **One tool call = one meaningful decision** — `auto_explore` achieves this for exploration
- **Proactive observation data** — returning observations from every action eliminates
  redundant `observe` calls
- **Structured metadata over ASCII parsing** — `frontier_exits`, `new_tiles_revealed`,
  `explore_target_x/y` are more reliable than visual ASCII interpretation
