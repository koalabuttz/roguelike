# LLM Playtest Report — 2026-02-13 (Session 3)

**Player:** Claude Opus 4.6 via MCP server
**Session length:** ~35 tool calls
**Result:** 100% explored, alive at 30/30 HP. Dungeon fully cleared.
**Kills:** 11 (7 goblins, 4 orcs)
**Rooms found:** 9

## What Changed Since Session 2

The developer addressed all three top MCP issues from Session 2 in a single pass:
- **Autorun junction detection rewritten** — `RoomTransition` removed, `CorridorBranches`
  only fires at true decision points
- **`pathfind_to(x, y)` added** — A* pathfinding through explored tiles
- **Frontier markers (`~`) and `frontier_exits` added to `get_explored_map`** — shows
  exactly where unexplored territory begins

These three changes are transformative. Session efficiency nearly doubled.

## MCP Server Observations

### 1. `pathfind_to` + `frontier_exits` is a game-changer (POSITIVE)
The exploration loop is now: `get_explored_map` → pick a frontier coordinate →
`pathfind_to(x, y)`. This replaced the ~20 tool calls of aimless wandering that ended
Session 2. I achieved 100% exploration in ~35 total tool calls vs. Session 2's 84% in ~70
calls. This is exactly the right abstraction level — strategic decisions (where to go) stay
with the LLM while mechanical execution (how to walk there) is handled by the server.

### 2. Autorun is much improved (POSITIVE, minor issues remain)
Autorun now correctly traverses rooms and corridors without spurious stops. Long corridor
runs of 10-30+ steps worked flawlessly. The `corridor_branches` stop reason fired
appropriately at T-intersections and genuine decision points.

**One remaining edge case:** `autorun_south` returned 0 steps / `wall_reached` once when
I was at the bottom edge of a room and the exit was southeast. This is correct behavior
(wall is directly south) but unhelpful — the LLM already knew there was a wall south. A
hint like `"nearest_exit_direction": "southeast"` in 0-step responses would prevent the
wasted call.

### 3. `auto_fight` targets one monster at a time (MINOR)
When two monsters are adjacent, `auto_fight` kills one, then you call it again for the
second. This is fine mechanically — it gives you the option to flee between kills — but
the LLM always fights both anyway. An optional `auto_fight_all` that chains adjacent
fights would save a tool call per multi-monster encounter, but this is low priority.

### 4. Room exit metadata still missing (MEDIUM — from Session 1)
With `pathfind_to` available, this is less critical than before. But the LLM still
occasionally misreads the ASCII map when choosing exploration targets. A `room_exits`
field in observations would eliminate this class of errors entirely. Now that `pathfind_to`
exists, the exits would pair perfectly with it: see exit → pathfind to it.

### 5. No feedback distinguishing "explored dead end" from "unexplored potential" (LOW)
When `pathfind_to` reaches its target at a frontier tile and reveals only walls beyond,
there's no indication that this branch is exhausted. The LLM has to call `get_explored_map`
again to see that the frontier shrank. A `new_tiles_revealed: N` field in pathfind/autorun
responses would help the LLM decide whether to keep exploring nearby or move on.

## Gameplay Observations

### 1. HP regeneration makes the game too safe (BALANCE)
Regen was the right call — Session 1's attrition death spiral is gone. But the pendulum
has swung too far. In this session I **never dropped below 24 HP** and entered every single
fight at 30/30. The math:

- Orc fight costs ~3 net HP (6 damage - 1 regen over 3 rounds)
- Goblin fight costs 0-1 net HP
- Travel between rooms is 15-60 steps = 5-20 HP regen

The player fully heals between every encounter. There's no resource tension, no "do I
fight or flee?" decision, no risk management. Possible fixes:
- **Reduce regen to 1 HP / 5 turns** — takes longer to heal, makes back-to-back fights costly
- **Cap regen at 50-70% max HP** — always enter fights slightly wounded
- **Regen only while stationary (waiting)** — creates a risk/reward tradeoff (waiting
  heals but monsters might wander in)
- **More monsters per room** — 2-3 orcs in a room would drain HP faster than regen recovers

### 2. No trolls spawned (SPAWN BALANCE)
Across 9 rooms and the entire dungeon, I fought only goblins and orcs. The Troll — the
one enemy that could threaten the player (4 dmg/turn × 10 rounds = 40 HP, fatal without
retreating) — never appeared. Either the spawn rate is too low or the map was too small.

**Suggestion:** Guarantee at least one troll per dungeon. Place it in a room with a
visible warning (bones, blood) so the player can prepare or avoid. The troll is the only
monster that creates a meaningful decision ("fight or flee?") — without it, every
encounter is a foregone conclusion.

### 3. Combat is still solved arithmetic (unchanged from Session 2)
Every encounter plays identically:
1. See monster in corridor/doorway
2. Calculate: can I kill it before I die? (always yes for goblins/orcs at any HP > 6)
3. `auto_fight`
4. Walk to next room, regen to full

There are zero tactical decisions. No positioning matters (corridor = always 1v1),
no resource choices (no items/abilities), no risk assessment (regen = always full HP).
The `auto_fight` command is the right tool for this — but the fact that it's *always*
the right answer means combat needs more depth, not a better interface.

### 4. Corridor chokepoint is still the only tactic (unchanged)
Every room entrance is a 1-tile-wide corridor. Every multi-monster room becomes a series
of 1v1s in the doorway. I never once needed to fight two monsters simultaneously.

**Suggestion:** Some rooms should have 2-3 tile wide entrances, or monsters that spawn
behind you, or ranged attackers that punish corridor camping. The dungeon generator could
occasionally place a monster *in* a corridor so you can't retreat to a chokepoint.

### 5. No win condition (unchanged from Sessions 1 & 2)
After achieving 100% exploration and 11 kills, the game just... continues. I'm standing
in a fully explored dungeon with nothing left to do. There's no stairs, no boss, no
treasure, no score screen. The game needs a goal — even a simple "find the stairs on
each level" would create tension between thorough exploration and survival.

### 6. Exploration flow is excellent (POSITIVE)
The dungeon layout — rooms connected by corridors in a hub-and-spoke pattern — creates
a satisfying exploration loop. Each corridor promises a new room. The FOV radius of 8
means you discover rooms gradually rather than all at once. The `frontier_exits` system
gives just enough guidance without spoiling the map. This is the strongest aspect of the
game right now.

## Session Statistics

| Metric | Value |
|--------|-------|
| Total tool calls | ~35 |
| `pathfind_to` calls | ~10 (29%) |
| `autorun` calls | ~8 (23%) |
| `auto_fight` calls | ~9 (26%) |
| `get_explored_map` calls | 4 (11%) |
| Wasted calls (0-step, navigation errors) | ~2 (6%) |
| Other (move, rules, new_game) | ~2 (6%) |
| Final HP | 30/30 |
| Lowest HP | 24/30 |
| Explored | 100% |
| Rooms found | 9 |
| Kills | 11 (7 goblins, 4 orcs, 0 trolls) |
| Cause of session end | Dungeon fully cleared, nothing left to do |

## Efficiency Comparison Across Sessions

| Metric | Session 1 | Session 2 | Session 3 |
|--------|-----------|-----------|-----------|
| Tool calls | ~80 | ~70 | **~35** |
| Wasted calls | ~40 (50%) | ~40 (57%) | **~2 (6%)** |
| Explored | ~100% | 84% | **100%** |
| Kills | 7 | 5 | **11** |
| Final HP | 2/30 | 30/30 | 30/30 |
| Session end reason | Near-death | Exploration plateau | **Dungeon cleared** |

The MCP tooling improvements (pathfind_to, frontier_exits, autorun fixes) cut tool call
waste from 50-57% down to 6%. The LLM can now focus almost entirely on strategic decisions
rather than fighting the interface.

## Priority Recommendations

### Addressed since Session 1 (all working well):
1. ~~HP regeneration~~ — working, possibly too generous
2. ~~Autorun room awareness~~ — working, minor 0-step edge case remains
3. ~~`pathfind_to` tool~~ — working excellently
4. ~~Frontier markers on explored map~~ — working excellently
5. ~~Omniscient info removed~~ — good

### Still unaddressed:
1. **Win condition (stairs/exit)** — the most important missing feature. Without a goal,
   the game has no tension and no ending. (Sessions 1, 2, 3)
2. **Combat depth** — damage variance, items, special abilities, terrain effects. Combat
   is pure arithmetic with one correct answer. (Sessions 2, 3)
3. **Troll spawn guarantee** — the only threatening monster never appeared. Ensure at least
   one per dungeon. (Session 3, new)
4. **Regen rebalance** — current rate fully heals between every fight, removing all resource
   tension. (Session 3, new)
5. **Room exit metadata** — less critical now with pathfind_to, but still saves parsing
   errors. (Sessions 1, 2)
6. **Multi-entrance rooms / anti-chokepoint design** — corridor camping is the only tactic.
   (Sessions 1, 2, 3)
