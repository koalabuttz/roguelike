# LLM Playtest Report — 2026-03-17 (Session 6: Micro Tier)

**Player:** Claude Opus 4.6 via MCP server (claude-code backend)
**Tier:** Micro (64x48 maps, no_std engine, seeds 123-125)
**Games:** 3 parallel, $5/game budget
**Best result:** Depth 5/5, 18 kills, 26/30 HP, 100% explored (depths 1-4)
**Worst result:** Died depth 2, killed by Troll

## What Changed Since Session 5

Session 5 (2026-02-23) validated the item system on standard tier. Since then, the micro
capability tier received a complete no_std game engine, multi-depth dungeons (5 floors),
items, and equipment. But the micro tier had *none* of the pathfinding infrastructure —
`auto_explore`, `pathfind_to`, and `auto_fight` all returned "This operation requires a
standard-tier game." LLM playtesting on micro seeds was nearly impossible — 50+ manual
move commands to explore 50% of one floor.

This session developed and validated:
- **BFS pathfinding** — no_std two-pass BFS with fixed-size buffers (1.1 KB), replacing
  the standard tier's A* (which requires `HashMap`/`BinaryHeap` from std)
- **`auto_fight` for micro tier** — port of the weakest-adjacent fight-to-death loop
- **`StairsFound` autorun stop** — all tiers stop when stepping onto stairs
- **Stairs coordinates in `observe()`** — spatial memory analogue for LLMs
- **Error message fix** — item actions (`use_item_X`, `equip_item_X`) now listed in
  unknown-action errors

## Development Progression (3 Rounds)

The features were developed iteratively with playtesting after each round:

| Round | Features Added | Max Depth | Stairs Found | auto_fight |
|-------|---------------|-----------|-------------|------------|
| 1 | BFS pathfinding, micro auto_explore/pathfind_to | 1 (stuck) | Never | "requires standard tier" |
| 2 | + StairsFound stop, + micro auto_fight | 2 | Sometimes | Works |
| 3 | + stairs coords in observe, + error message fix | **5** | Every game | Works |

Each round exposed the next bottleneck, which was fixed before the next run.

## Round 3 Results (Final)

| Seed | Depth | Kills | HP | Explored | Calls | Cost | Result |
|------|-------|-------|----|----------|-------|------|--------|
| 123-64x48 | **5/5** | 18 | 26/30 | 61% (D5) | 164 | $5.00 | Survived |
| 124-64x48 | 2/5 | 10 | 0/30 | 31% | 135 | $4.26 | **Died** (Troll) |
| 125-64x48 | 2/5 | 7 | 10/30 | 89% | 156 | $4.90 | Survived |

**Win rate:** 67% | **Avg kills:** 11.7 | **Avg HP remaining:** 12.0

### Tool Usage

| Tool | Game 1 | Game 2 | Game 3 |
|------|--------|--------|--------|
| auto_explore | 85 | 17 | 40 |
| auto_fight | 18 | 10 | 7 |
| Total calls | 164 | 135 | 156 |

## MCP Server Observations

### 1. BFS pathfinding works correctly (MAJOR POSITIVE)

The two-pass BFS (find nearest frontier, then find first step toward it) performs identically
to A* for the LLM's purposes. All three games explored multiple floors without pathfinding
failures. The 256-entry ring queue never overflowed on 64x48 maps.

The BFS re-pathfinds each step (no stored path), which handles map changes from combat and
monster spawns gracefully. This is actually simpler than the standard tier's A* approach
which precomputes the full path.

### 2. `StairsFound` stop is the right design (MAJOR POSITIVE)

Game 1 floor 1: auto_explore path crossed the stairs tile at (58, 8), autorun stopped with
`"stop_reason": "stairs_found"`. The LLM immediately called `descend`. Zero wasted calls.

This matches how standard roguelikes handle it — autorun stops at points of interest. The
check placement after damage/monster checks ensures urgent interruptions take priority.

### 3. Stairs coords in observe() solved the spatial memory gap (MAJOR POSITIVE)

The biggest improvement from Round 2 to Round 3. In Round 2, both games reached 100%
explored but couldn't find stairs — the LLM spent hundreds of tokens on blind `look_at`
scans or read the source code to cheat. In Round 3, the `"stairs": [x, y]` field appeared
in every observation once the tile was explored, and games descended immediately.

Game 3 explicitly referenced the stairs location in its strategy notes: "unable to reach
stairs at (43,9)" — it *knew* where stairs were, the problem was a troll blocking the path.
That's a real gameplay decision, not a tooling gap.

**Design principle:** This is a spatial memory analogue. A human player sees `>` on the map
once and remembers where it is. LLMs can't retain spatial information across tool calls —
earlier observations scroll out of context. The `stairs` field compensates by providing
persistent state that a human player would hold in working memory.

### 4. Missing item actions in error messages caused cascading failure (BUG — FIXED)

Game 3 tried `"equip_b"` (wrong) instead of `"equip_item_b"` (correct). The error response
listed only movement and combat actions, omitting item actions entirely. The LLM concluded
"items can't be used via MCP" and fought a troll with base stats despite having a Short Sword
(+3 ATK) and Leather Armor (+2 DEF) in inventory.

With those items equipped: 5 dmg/round to troll (winnable in 4 rounds, taking 12 dmg with
both equipped). Without: 2 dmg/round (11 rounds, certain death). This single missing help
string likely cost the game.

Fixed by adding `pickup, use_item_X, equip_item_X, drop_item_X (X = inventory slot a-z)`
to the error message.

### 5. Compact mode is counterproductive for micro tier (OBSERVATION)

Games using `compact=true` (no ASCII map) had worse situational awareness. The ASCII map is
the primary way to see monster positions relative to corridors and room layout. With the
`stairs` field now providing structured data, compact mode's worst problem (stairs
invisibility) is solved, but map visibility remains valuable for tactical positioning.

## Gameplay Observations

### 1. Trolls are the skill gate (Sessions 3, 4, 5, 6)

Session 4 noted trolls never spawned. Now they spawn reliably on depth 2+, and they create
exactly the tactical decisions Sessions 3-4 wanted:

| Game | Troll Encounter | Outcome | Decision Quality |
|------|----------------|---------|-----------------|
| 1 | D4 troll | Avoided (ran past) | **Good** — recognized unwinnable math |
| 2 | D2 troll | Engaged without weapon | **Fatal** — didn't check stats first |
| 3 | D2 troll | Fought under-equipped | **Bad** — had equipment but couldn't use it |

The troll forces exactly the decision Sessions 3-4 wanted: "fight or flee?" Game 1's LLM
solved this correctly by checking combat math before engaging. Game 2's LLM learned the
hard way.

### 2. Equipment changes the combat calculus meaningfully

Items didn't exist in Sessions 1-4. Session 5 validated they work. Session 6 shows they
create genuine tactical gates:

| Stat | Base | + Short Sword | + Leather Armor | + Both |
|------|------|---------------|-----------------|--------|
| ATK | 5 | **8** | 5 | **8** |
| DEF | 2 | 2 | **4** | **4** |
| Dmg to Troll (DEF 3) | 2 | **5** | 2 | **5** |
| Dmg from Troll (ATK 7) | 5 | 5 | **3** | **3** |
| Rounds to kill Troll | 11 | **4** | 11 | **4** |
| HP cost per Troll | 55 (dead) | 20 | 33 (dead) | **12** |

Equipment makes trolls beatable. The Short Sword alone swings a troll fight from certain
death to comfortable win. Game 3 had both items in inventory but couldn't equip them due
to the action-name bug — demonstrating how critical the equipment gate is.

### 3. Multi-depth creates real strategic tension

The 5-floor dungeon creates the decision Session 4 called the #1 missing feature: "Do I
keep exploring or descend?" Game 1's LLM explored 100% of each floor before descending,
collecting all items and killing all safe monsters. This greedy strategy worked — it reached
depth 5 with 8 health potions and full equipment.

Depth scaling (monsters get +HP/+ATK per floor) creates escalating pressure. Goblins on
depth 1 are free kills; orcs on depth 5 are genuinely dangerous (ATK 8 vs base DEF 2 = 6
dmg/round).

### 4. Corridor running is the emergent meta

Multiple games independently discovered corridor running for regen: run away from a troll
down a long corridor, regenerate 1 HP every 3 turns, reverse at the wall, take one hit,
run the other way. This is emergent tactical behavior the LLM figured out from game
mechanics — exactly the kind of strategy Sessions 3-4 wanted to see.

It's not reliable (direction changes at walls cost a free hit), but it shows the LLM
reasoning about spatial tactics beyond simple fight/flee binary.

### 5. Budget is the binding constraint

All three games hit or approached the $5 budget cap. Game 1 reached depth 5 but stopped
at 61% explored (25 frontiers remaining). Games 2-3 stopped on depth 2. The ~150 tool
calls per game at ~$5 each suggests deeper runs need either higher budget ($10-15 for a
full 5-floor clear) or more efficient play.

## Comparison: Standard Tier (Sessions 1-5) vs Micro Tier (Session 6)

| Metric | Session 4 (Standard) | Session 5 (Standard) | Session 6 (Micro) |
|--------|---------------------|---------------------|------------------|
| Map size | 80x40 | 80x40 | 64x48 |
| Depth | 1 (no stairs) | 1 (no stairs) | 1-5 |
| Kills | 4 | 18 (best) | 18 (best) |
| Items | None | Potions, Sword, Armor | Potions, Sword, Armor |
| Tool calls | ~30 | ~30 | ~150 |
| Strategic depth | "Single button press" | Equipment decisions | Equipment + depth + troll avoidance |
| Bottleneck | Game content | Win condition | **Budget / strategy** |

The micro tier, despite running on a no_std engine designed for the C64, now provides the
richest gameplay experience of any session. The game content that Session 4 called "the
development frontier" has been built.

## Session Statistics

| Metric | Game 1 | Game 2 | Game 3 |
|--------|--------|--------|--------|
| Seed | 123-64x48 | 124-64x48 | 125-64x48 |
| Tool calls | 164 | 135 | 156 |
| auto_explore calls | 85 (52%) | 17 (13%) | 40 (26%) |
| auto_fight calls | 18 (11%) | 10 (7%) | 7 (4%) |
| Floors explored | 5 | 2 | 2 |
| Kills | 18 | 10 | 7 |
| Final HP | 26/30 | 0/30 | 10/30 |
| Explored (current floor) | 61% | 31% | 89% |
| Cause of session end | Budget cap | Death (Troll) | Budget cap |

## Efficiency Comparison Across All Sessions

| Metric | S1 | S2 | S3 | S4 | S5 | **S6 (best)** |
|--------|-----|-----|-----|-----|-----|----------|
| Tier | Std | Std | Std | Std | Std | **Micro** |
| Tool calls | ~80 | ~70 | ~35 | ~30 | ~30 | **164** |
| Wasted calls | 50% | 57% | 6% | 0% | ~0% | **~0%** |
| Floors | 1 | 1 | 1 | 1 | 1 | **5** |
| Kills (best) | 7 | 5 | 11 | 4 | 18 | **18** |
| Deepest floor | 1 | 1 | 1 | 1 | 1 | **5** |
| Items | No | No | No | No | Yes | **Yes** |
| Primary tool | autorun | autorun | pathfind_to | auto_explore | auto_explore | **auto_explore** |

Tool call count is higher in Session 6 because the game is 5x deeper (5 floors vs 1).
Per-floor efficiency is comparable to Sessions 4-5. Wasted calls remain near zero.

## Priority Recommendations

### Resolved since Session 5:
1. ~~Win condition (stairs/exit)~~ — 5-floor dungeon with descend mechanic
2. ~~Guarantee troll spawns~~ — trolls appear reliably on depth 2+
3. ~~Micro tier pathfinding~~ — BFS auto_explore/pathfind_to/auto_fight
4. ~~Stairs discovery~~ — StairsFound stop + stairs coords in observe
5. ~~Item action discoverability~~ — error messages now list all valid actions

### Remaining:
1. **Validate item equip/use after error fix** — HIGH — the fix hasn't been tested in a
   playtest yet. This alone could dramatically improve troll survival.
2. **Mid-combat potion use** — MEDIUM — `auto_fight` resolves the full fight in one call
   with no opportunity to use potions. A smarter auto_fight that pops potions at low HP
   would improve survival.
3. **Monster density on later floors** — MEDIUM — depth 1 feels good, depth 4-5 could
   use more encounters.
4. **Budget optimization** — LOW — shorter MCP responses or more aggressive prompt
   caching could stretch the $5 budget further.
5. **Damage variance** — LOW — combat is still deterministic arithmetic, but equipment
   and depth scaling add enough variety that this is less urgent.

### MCP design principles — still validated:
- **Server handles mechanics, LLM handles strategy** — BFS pathfinding in the server,
  troll avoidance decisions by the LLM
- **One tool call = one meaningful decision** — auto_explore + auto_fight achieve this
- **Structured metadata over ASCII parsing** — `stairs` field is the latest example
- **Spatial memory compensation** — LLMs can't remember map features across tool calls;
  the server provides persistent state (stairs coords, frontier count) as a substitute
