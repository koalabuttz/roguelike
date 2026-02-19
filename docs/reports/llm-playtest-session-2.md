# LLM Playtest Report — 2026-02-13 (Session 2)

**Player:** Claude Opus 4.6 via MCP server
**Session length:** ~70 tool calls
**Result:** Still alive at 30/30 HP, game ongoing (stopped to write report)
**Kills:** 5 (4 goblins, 1 orc)
**Explored:** 84% (11 rooms found)

## What Changed Since Session 1

The developer addressed several Session 1 recommendations:
- **HP regeneration added** (1 HP per 3 turns) — massive improvement, game no longer
  feels like a slow death march
- **`get_explored_map` tool added** — enables strategic exploration planning
- **`explored_pct` bug fixed** — no longer exceeds 100%
- **Omniscient info (`total_monsters`, `total_rooms`) removed** — good change

These are meaningful improvements. The game is noticeably more playable.

## Changes Implemented After Session 2

The top three MCP issues from both sessions (responsible for ~57% of wasted tool calls)
were addressed in a single implementation pass:

### Fix 1: Smarter Autorun (addresses MCP Issue #1)
- **Removed `RoomTransition` as a stop condition entirely.** Autorun now crosses
  room/corridor boundaries freely instead of stopping at every transition.
- **Refined `CorridorBranches` to only fire at true decision points.** Old behavior:
  stop whenever corridor topology changes (any neighbor count difference). New behavior:
  only stop when the forward path is blocked AND there are 2+ alternative directions.
  If you can keep going straight through a junction, autorun continues.
- **Net effect:** Traversing a room is now 1 autorun call (wall-to-wall) instead of
  5-8. Corridors with side branches no longer cause spurious stops.
- **Files changed:** `src/game.rs` — `AutorunStopReason` enum (removed `RoomTransition`,
  added `PathComplete`), `autorun()` method rewritten.

### Fix 2: `pathfind_to(x, y)` Tool (addresses MCP Issue #2)
- **New A* pathfinding module** (`src/pathfinding.rs`) with Chebyshev distance heuristic
  for 8-directional movement. Only pathfinds through explored, walkable tiles (no fog
  of war cheating). Ignores entities — the step loop handles monster detection.
- **New `pathfind_to()` method** on `GameState` — walks the A* path step by step,
  stopping early for: monster spotted, damage taken, game over, max steps. Returns
  `PathComplete` when the destination is reached.
- **New MCP tool** `pathfind_to` with `{x, y}` parameters — returns observation merged
  with autorun metadata (steps taken, stop reason).
- **Net effect:** Navigating to a visible exit is 1 tool call instead of 3-8.
- **Files changed:** `src/pathfinding.rs` (new), `src/lib.rs`, `src/game.rs`, `src/mcp.rs`.

### Fix 3: Frontier Markers on Explored Map (addresses MCP Issue #3)
- **New `frontier_tiles()` method** — identifies explored floor tiles adjacent to at
  least one unexplored tile. These mark the boundary of explored territory.
- **`explored_map()` renders frontier tiles as `~`** instead of `.`, making exploration
  boundaries visible at a glance.
- **`get_explored_map` response now includes `frontier_exits`** — a JSON array of
  `{x, y}` coordinates for all frontier tiles. Combined with `pathfind_to`, the LLM
  can navigate directly to the nearest frontier.
- **`get_rules` updated** with `~` symbol documentation.
- **Net effect:** "Where haven't I been?" is answered by one `get_explored_map` call
  instead of ~20 calls of aimless wandering.
- **Files changed:** `src/game.rs`, `src/mcp.rs`.

### Test Coverage
- 11 new tests added (143 total, up from 132):
  - 6 A* pathfinding unit tests (path to self, adjacent, diagonal, around walls, blocked)
  - 5 `pathfind_to()` integration tests (reaches target, errors, stops for monsters/damage)
  - 4 frontier tile tests (edge detection, fully explored, floor-only, `~` rendering)
  - 2 new autorun tests (runs through junction, stops at T-junction)
  - 3 existing autorun tests updated for new behavior

## Bugs Found

### Silent wall collisions consume no turn but waste a tool call
Moving into a wall returns the same game state with no error message or indication
that the move failed. The LLM has to diff the coordinates to realize nothing happened.
This is especially bad when trying to navigate room exits — 5+ tool calls wasted per
room on failed directional guesses.

**Suggestion:** Return a `move_failed: true` flag or a message like "You bump into a
wall." to make failures explicit.

## MCP Server Issues

### 1. Autorun junction detection is STILL too sensitive (HIGH — fixed post-session)
**Problem:** This remains the single biggest token waster. Autorun stops after 0-1
steps inside rooms and at corridor-to-room transitions where there's only one logical
path forward. Example from this session: traversing a diamond-shaped room required
6 separate autorun calls, each stopping after 1 step.

**Impact:** ~35% of tool calls were single-step or zero-step autoruns. The `room_transition`
and `corridor_branches` stop reasons trigger far too aggressively.

**Recommendation from Session 1 still applies:** Only stop at actual decision points.
Inside rooms, continue until hitting a wall, room exit, or monster.

### 2. No pathfinding tool (HIGH — fixed post-session)
**Problem:** Navigating to visible room exits is the second-biggest time sink. The LLM
can see the exit on the ASCII map but must guess the correct cardinal/diagonal move
sequence. Example: finding the east exit of room 9 took 8 tool calls — moving to the
wrong row, overshooting south, backtracking north, then finally hitting the right
diagonal.

**Impact:** ~25% of tool calls were wasted on navigation within explored areas.

**Same recommendation:** `pathfind_to(x, y)` would collapse these into a single call.

### 3. `get_explored_map` lacks unexplored-exit markers (MEDIUM — fixed post-session)
**Problem:** The `get_explored_map` tool was added (great!), but it only shows what
has been explored — not where to explore next. The LLM spent ~20 tool calls circling
through explored corridors trying to find the remaining 16% of the map with no way to
identify which corridor ends lead to unexplored areas.

**Suggestion:** Mark unexplored exits — tiles at the boundary of explored space that
have walkable neighbors not yet seen. Something like `?` or `!` glyphs at corridor
openings that lead into fog. This would turn "wander and hope" into "go to the nearest
`?` marker."

### 4. Room exit metadata would save significant tool calls (MEDIUM — from Session 1)
**Problem:** The LLM struggles to parse ASCII art to find exits. It frequently
miscounts leading whitespace, confuses wall characters with room boundaries, and
moves in wrong directions.

**Suggestion:** Include a `room_exits` field in observations:
```json
"room_exits": [
  {"direction": "east", "x": 48, "y": 23},
  {"direction": "south", "x": 46, "y": 27}
]
```
This eliminates ASCII parsing errors entirely.

### 5. Autorun into walls returns 0 steps with no useful feedback (LOW)
**Problem:** `autorun_south` frequently returns `{"autorun_steps": 0, "autorun_stop_reason":
"wall_reached"}`. This is a wasted tool call that provides no new information. The LLM
already knew it was facing a wall (it just couldn't tell from the ASCII).

**Suggestion:** Either prevent 0-step autoruns (return an error prompting a different
direction) or include the nearest walkable direction in the response.

## Gameplay Observations

### 1. HP regeneration transforms the game (POSITIVE)
The 1 HP / 3 turns regeneration completely changes the dynamic. After the orc fight
(took 6 damage), I explored corridors and was back to full HP within 18 moves. The
game now rewards patient exploration between fights rather than being pure attrition.
This was the #1 recommendation from Session 1 — great call implementing it.

### 2. Combat is solved after the first fight (NEEDS DEPTH)
By the second goblin encounter, the combat math is fully known:
- Goblin: 2 hits to kill, take 0-1 damage → always auto_fight
- Orc: 3 hits to kill, take 4-6 damage → always auto_fight
- Troll: 10 hits to kill, take 40 damage → always avoid

There are no meaningful combat decisions. Every encounter has exactly one correct
answer determined by pure arithmetic. Consider:
- **Randomized damage** (ATK +/- 1-2) to add uncertainty
- **Special abilities** (goblin ambush from fog, orc charge for double damage)
- **Items** (weapons, armor, potions) that create build choices
- **Terrain effects** (fight in water = slower, fight near torch = bonus)

### 3. The 84% exploration wall (FRUSTRATING)
After finding 11 rooms and clearing all visible corridors, the explored percentage
stalled at 84%. The remaining 16% is presumably behind passages I can't locate.
I spent ~20 tool calls systematically revisiting every junction with no progress.

Without unexplored-exit markers (see MCP Issue #3 above), finding the last rooms is
pure brute force. This is where the session effectively ended — not from death, but
from frustration.

**Suggestion for gameplay:** Add visual hints for nearby unexplored areas — a draft
of air, distant sounds, or slightly different wall textures near hidden passages.

### 4. No win condition (unchanged from Session 1)
Still no stairs, exit, or objective. With HP regen, the game now feels like it could
go on forever — there's no tension between "explore more" and "get to safety." A
descending staircase would immediately add strategic depth: do I keep exploring this
level for loot/XP, or descend while healthy?

### 5. Monster variety is low
Only encountered goblins and one orc across 11 rooms. The dungeon felt empty in the
later areas — long stretches of corridors with no encounters. More frequent spawns or
monster patrols would maintain tension during exploration.

## Session Statistics

| Metric | Value |
|--------|-------|
| Total tool calls | ~70 |
| Wasted on 0-1 step autorun | ~25 (36%) |
| Wasted on navigation fumbles | ~15 (21%) |
| Productive exploration | ~20 (29%) |
| Combat | ~10 (14%) |
| Final HP | 30/30 |
| Explored | 84% |
| Rooms found | 11 |
| Kills | 5 (4 goblins, 1 orc) |
| Cause of session end | Exploration plateau (couldn't find remaining 16%) |

## Priority Recommendations (Updated)

### Fixed post-session:
1. ~~**Autorun room awareness**~~ — **FIXED.** Removed `RoomTransition` stop, refined
   `CorridorBranches` to only fire at true decision points (wall ahead + 2+ alternatives).
2. ~~**`pathfind_to` tool**~~ — **FIXED.** Added A* pathfinding tool that walks the
   shortest path through explored tiles, stopping for monsters/damage.
3. ~~**Unexplored-exit markers on explored map**~~ — **FIXED.** Frontier tiles rendered
   as `~` with `frontier_exits` coordinates in JSON response.

### Still unaddressed:
4. **Win condition (stairs/exit)** — game still has no goal
5. **Room exit metadata** — eliminates ASCII parsing errors
6. **Combat depth** (damage variance, items, abilities) — combat is currently solved
   arithmetic
7. **Explicit wall-bump feedback** — prevents silent wasted tool calls
8. **More monster spawns / variety** — large portions of the dungeon feel empty
