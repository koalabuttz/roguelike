# LLM Playtest Report — 2026-02-13

**Player:** Claude Opus 4.6 via MCP server
**Session length:** ~80 tool calls
**Result:** Died at 2 HP after exploring 110% (bug — fixed post-session) of the map
**Kills:** 7/17 monsters (3 goblins, 2 orcs; fled from 1 orc + 1 goblin pair)

## Bugs Found

### explored_pct exceeds 100% (fixed)
`explored_pct` hit 110%. The calculation divided all explored tiles (including walls
revealed by FOV) by floor-only count. **Fixed in commit 43de46b** — now filters
explored set to floor tiles before computing the percentage.

### total_monsters / total_rooms leak omniscient info (fixed)
The LLM used `total_monsters: 17` and `total_rooms: 16` to make unnaturally precise
risk calculations ("12 monsters left with 18 HP — getting tight"). A human player
would never have this info. **Fixed in commit 43de46b** — removed from responses.

## MCP Server Issues

### 1. Autorun junction detection is too sensitive (HIGH PRIORITY)
**Problem:** Autorun stops after 1 step at "corridor_branches" constantly — especially
inside rooms, where nearly every floor tile has >2 open neighbors. Traversing a room
takes 5-8 autorun calls that each stop after 1 step.

**Impact:** Easily the biggest token waster. ~30% of all tool calls in this session
were single-step autoruns stopping at non-meaningful junctions.

**Suggestion:** Only stop at *actual decision points* — corridor forks where the
player must choose a direction. Inside rooms, autorun should continue until hitting
a wall or room exit. Consider a separate heuristic: if the player is inside a room
(surrounded by open floor), only stop at room exits or walls, not at every tile.

### 2. No pathfinding tool (HIGH PRIORITY)
**Problem:** Navigating to a visible corridor exit is painful. The LLM can see the
exit in the ASCII map but has to guess the correct sequence of cardinal/diagonal
moves. "Move south — wall. Move southwest — wall. Move west then south..." burns
3-5 tool calls per navigation.

**Impact:** ~20% of tool calls were wasted on navigation to visible destinations.

**Suggestion:** Add `pathfind_to(x, y)` — auto-walk to a visible/explored tile via
shortest path. Stops early for the same reasons autorun does (monster spotted, damage
taken). This would collapse dozens of navigation calls into one.

### 3. No explored map view (MEDIUM)
**Problem:** When hunting for the last 3% of unexplored tiles, the LLM has no memory
of the full dungeon layout — only current FOV. It wandered through already-explored
corridors hoping to stumble onto something new.

**Impact:** The final 10% of exploration was pure wandering with no strategy.

**Suggestion:** Add a `get_explored_map` tool that returns all previously-seen tiles
(floor, wall, corridor) with unseen areas blank. This gives the LLM a mental map
to plan exploration routes. Could also include markers for unexplored exits (corridor
openings at the edge of explored space).

### 4. No structural map data (LOW)
**Problem:** The LLM had to visually parse ASCII to find corridor exits, count tiles,
and determine room boundaries. Frequently miscounted leading spaces when trying to
calculate positions of map features.

**Suggestion:** Include `nearby_exits` or `room_exits` in observation — list of
`(x, y, direction)` for walkable passages out of the current room/corridor. The LLM
can then pathfind or move toward specific exits by coordinate.

## Gameplay Observations

### 1. No healing makes the game unwinnable (CRITICAL)
With 30 HP and no recovery, the math doesn't work:
- 17 monsters × minimum 2 HP each (all goblins) = 34 HP damage
- Realistic mix with orcs: ~60+ HP damage needed
- A single troll costs ~40 HP

The game is a countdown to death. Every fight is pure attrition with no possibility
of recovery. Strategic depth is reduced to "minimize damage per fight" with no way
to recover from mistakes or bad luck.

Even basic healing (potions, resting, food) would transform the game from "slow death"
to "resource management."

### 2. No win condition
There are no stairs, exits, amulets, or goals. The session ended when the LLM ran
out of HP. Without a defined objective, there's no tension between exploration (risky)
and winning (requires survival). The LLM defaulted to "explore everything" because
there was nothing else to optimize for.

### 3. "Wait + auto_fight" is the dominant strategy
The LLM discovered that monsters don't attack on the turn they move adjacent — they
only attack next turn. This means:
- **Approaching a monster:** you take 1 extra hit (monster retaliates same turn)
- **Waiting for the monster:** it moves adjacent without attacking, you auto_fight
  next turn saving 1 hit

This is consistent with the turn system (move is the monster's action), but it means
the optimal play is *always* wait. There's no reason to approach. This could be
addressed by giving monsters attack-on-arrival, or by making some monsters ranged
(so waiting is punished).

### 4. Corridor fighting is the only viable tactic
Rooms with 2+ monsters are death traps (both attack every turn). Corridors limit
combat to 1v1. The LLM learned this quickly and always retreated to corridors before
fighting. Rooms became "places to peek into and lure enemies out of."

This is actually a classic roguelike pattern (NetHack corridor farming), but right
now it's the *only* tactic. Items, abilities, or terrain features could make room
combat viable.

### 5. Fleeing rarely works
When the LLM tried to flee the orc+goblin pair in the corridor, the monsters chased
at equal speed — distance never changed. Escape only worked when the LLM broke line
of sight through a room junction. Consider:
- Monsters losing interest after N turns without LOS
- Speed differences (goblins faster, trolls slower)
- Doors the player can close

## Session Statistics

| Metric | Value |
|--------|-------|
| Total tool calls | ~80 |
| Wasted on 1-step autorun | ~25 (31%) |
| Wasted on navigation fumbles | ~15 (19%) |
| Productive exploration | ~25 (31%) |
| Combat | ~15 (19%) |
| Final HP | 2/30 |
| Explored | ~100% |
| Rooms found | 8/16 |
| Kills | 7/17 |
| Cause of near-death | HP attrition (no healing) |

## Priority Recommendations

1. **Healing mechanic** — without this, the game loop is fundamentally broken for
   extended play
2. **Autorun room awareness** — halves the tool calls per session
3. **pathfind_to tool** — eliminates navigation waste
4. **Win condition (stairs)** — gives the game a goal
5. **Explored map tool** — enables strategic exploration
