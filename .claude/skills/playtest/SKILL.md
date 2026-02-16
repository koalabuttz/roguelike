---
name: playtest
description: Play roguelike games strategically using MCP tools, collect analytics, and compare against the dumb AI baseline.
---

# Playtest Skill

Play roguelike games strategically using the connected MCP tools, collect per-game analytics, and output results compatible with `tools/visualize.py`.

## Usage

- `/playtest` — Play 5 games (default)
- `/playtest 10` — Play 10 games
- `/playtest --seed 42` — Play 5 games starting from seed 42
- `/playtest 10 --seed 100` — Play 10 games starting from seed 100

## Architecture: Subagent per Game

**IMPORTANT**: To prevent context overflow, delegate each game to a subagent using the Task tool with `subagent_type: "general-purpose"`. Each game generates 15-30 MCP tool responses containing map ASCII, entity lists, and messages — running games directly in the main conversation would overflow context after just a few games.

The main conversation orchestrates: parse args, launch subagents, collect results, aggregate, report.

## Setup

1. Parse the arguments from the user's invocation to determine game count and starting seed.
2. Set default values: 5 games if no count specified, use a seed based on current unix timestamp if none given (run a quick `date +%s` via Bash and take the last 5 digits).

## Game Execution

For each game, launch a subagent using the Task tool:

```
Task tool call:
  subagent_type: "general-purpose"
  description: "Play roguelike game seed N"
  prompt: <see subagent prompt template below>
```

You can run up to 3-4 subagents in parallel by including multiple Task tool calls in a single message. For 10 games, launch them in batches of 3-4.

### Subagent Prompt Template

Use this prompt for each subagent, filling in the seed:

```
Play one roguelike game with seed {SEED} and return analytics as JSON.

RULES:
- Player: 30 HP, 5 ATK, 2 DEF. Regenerates 1 HP every 3 turns.
- Goblin (g): 6 HP, 3 ATK, 0 DEF → 1 dmg/turn to player, 2 hits to kill
- Orc (o): 12 HP, 4 ATK, 1 DEF → 2 dmg/turn to player, 3 hits to kill
- Troll (T): 20 HP, 6 ATK, 3 DEF → 4 dmg/turn to player, 10 hits to kill
- Damage formula: ATK - DEF (minimum 0)

STRATEGY:
- Use mcp__roguelike__auto_explore for ALL movement
- Use mcp__roguelike__act with action "auto_fight" for ALL combat
- HP > 20: fight anything including Trolls
- HP 10-20: fight Goblins and Orcs, avoid Trolls (use auto_explore to flee)
- HP <= 10: fight Goblins only, avoid Orcs and Trolls
- HP <= 5: avoid everything, just explore to regenerate

PROCEDURE:
1. Call mcp__roguelike__new_game with seed={SEED}
2. Loop: auto_explore → if monsters visible, decide fight/flee → auto_fight or flee → repeat
3. Stop when game_over=true OR frontier_count=0 with explored_pct > 90

TRACKING — track these as you play:
- kills_by_type: dict of monster_name → kill count
- damage_dealt_by_type: dict of monster_name → total damage dealt (estimate: auto_fight_rounds * max(0, 5 - monster_def))
- damage_taken_by_type: dict of monster_name → total HP lost (from auto_fight_player_hp_lost)
- auto_explore_calls, auto_fight_calls, decision_count (flee/strategic moves), total tool_calls
- Note the first and last kill (by tool call number, not turn)

RETURN FORMAT — after the game ends, respond with ONLY this JSON (no other text):
```json
{
  "kills_by_type": {},
  "damage_dealt_by_type": {},
  "damage_taken_by_type": {},
  "final_hp": 0,
  "explored_pct": 0,
  "first_kill_turn": null,
  "last_kill_turn": null,
  "monsters_spawned": 0,
  "turns": 0,
  "game_over": false,
  "seed": {SEED},
  "llm_metrics": {
    "tool_calls": 0,
    "decision_count": 0,
    "auto_explore_calls": 0,
    "auto_fight_calls": 0,
    "strategy_notes": "2-sentence narrative of the run",
    "model": "claude-code"
  }
}
```

Fill in all fields from your tracking. Set final_hp, explored_pct, game_over from the last observation.

IMPORTANT — strategy_notes should be a 1-2 sentence narrative capturing the KEY DECISIONS you made, not just the outcome. Mention:
- Any tactical retreats and why (e.g. "Fled Troll at 8 HP, circled back after regen")
- Close calls or interesting moments (e.g. "Survived Orc fight with 1 HP")
- Whether you cleared the dungeon or what stopped you
Examples:
- "Cleared 7 rooms systematically. Fled Troll at 8 HP, explored two more rooms to regen, then returned to finish it at 15 HP."
- "Killed 3 Goblins and 2 Orcs easily, then got cornered by a Troll in a corridor at 12 HP — died after 6 rounds."
- "Full clear with 22 HP remaining. No Trolls spawned, straightforward run."
```

### Collecting Results

Each subagent returns a JSON string. Parse it and collect into an `all_games` array. If a subagent fails or returns invalid JSON, log the error and skip that game.

## Analytics Output

After all games complete, write results using a Bash command:

```bash
python3 -c "
import json, sys
sys.path.insert(0, 'tools')
import playtest_analytics as pa

all_games = json.loads(sys.stdin.read())
meta = {'source': 'playtest_skill', 'model': 'claude-code'}
batch_stats = pa.write_results('tools/output/llm_playtest_results.json', all_games, meta)
print(json.dumps(batch_stats, indent=2))
" << 'GAMES_JSON'
<insert JSON array of all per-game analytics dicts here>
GAMES_JSON
```

## Reporting

After writing results, print a summary table with strategy notes under each game:

```
=== LLM Playtest Results (N games) ===
Win rate:     XX.X%
Avg kills:    X.X
Avg HP:       X.X
Avg explored: XX.X%

Per-game results:
  Game 1 (seed=42): SURVIVED | HP=21 kills=4 explored=87%
    → Cleared 7 rooms systematically. Fled Troll at 8 HP, circled back after regen to finish it.
  Game 2 (seed=43): DIED     | HP=0  kills=2 explored=54%
    → Killed 2 Goblins easily, then got cornered by a Troll in a dead-end corridor at 12 HP.
  ...
```

The `→` line is the `strategy_notes` field from each game's `llm_metrics`. Always display it — this is the main qualitative insight from each run.

Then offer to generate charts:

```
Results saved to tools/output/llm_playtest_results.json
Run visualization: cat tools/output/llm_playtest_results.json | python3 -c "import json,sys; print(json.dumps(json.load(sys.stdin)['batch_stats']))" | python3 tools/visualize.py batch
```

## Strategy Guidelines Reference

These are embedded in the subagent prompt above, but for reference:

### Monster Stats

| Monster | HP | ATK | DEF | Damage to Player (ATK-2) | Hits to Kill (5-DEF per hit) |
|---------|---:|----:|----:|-------------------------:|-----------------------------:|
| Goblin  |  6 |   3 |   0 | 1/turn                   | 2 hits                       |
| Orc     | 12 |   4 |   1 | 2/turn                   | 3 hits                       |
| Troll   | 20 |   6 |   3 | 4/turn                   | 10 hits                      |

### Decision Rules

- **HP > 20**: Fight anything including Trolls
- **HP 10-20**: Fight Goblins and Orcs. Avoid Trolls.
- **HP <= 10**: Fight Goblins only. Avoid Orcs and Trolls.
- **HP <= 5**: Avoid everything — just explore to regenerate.

### Efficiency Tips

- Use `auto_explore` for ALL movement — never use individual move commands
- Use `auto_fight` for ALL combat — never use individual attack moves
- These two tools resolve multi-step actions in a single call, keeping tool usage at 15-30 per game
- Don't call `observe` or `get_explored_map` unless you have a specific tactical need
