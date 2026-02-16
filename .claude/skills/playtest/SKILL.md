---
name: playtest
description: Run LLM playtesting sessions via the llm_playtest.py script with the claude-code backend.
---

# Playtest Skill

Run LLM-driven roguelike playtesting using `tools/llm_playtest.py` with the `claude-code` backend.

## Usage

- `/playtest` — Play 5 games (default)
- `/playtest 10` — Play 10 games
- `/playtest --seed 42` — Play 5 games starting from seed 42
- `/playtest 10 --seed 100 --parallel 4` — Play 10 games, 4 at a time

## Instructions

1. Parse the user's arguments to determine count, seed, and parallelism:
   - First positional number → game count (default: 5)
   - `--seed N` → starting seed
   - `--parallel N` → concurrent games (default: 2)

2. Run the playtest script via Bash:

```bash
python3 tools/llm_playtest.py --backend claude-code -n COUNT --parallel PARALLEL [--seed SEED] [--max-budget BUDGET]
```

Default budget is $2.00/game. The script uses compact mode (no ASCII map in responses) and short field names to minimize token cost.

3. The script handles everything: game execution, analytics collection, incremental result saving, and prints a summary table with per-game strategy notes.

4. After completion, display the results summary from the script output and offer to generate charts:

```bash
cat tools/output/llm_playtest_results.json | python3 -c "import json,sys; print(json.dumps(json.load(sys.stdin)['batch_stats']))" | python3 tools/visualize.py batch
```
