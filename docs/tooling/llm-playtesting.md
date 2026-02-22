# LLM Playtesting

Strategic LLM-driven playtesting where an LLM plays the game making tactical decisions (fight, flee, explore) rather than the headless runner's simple auto-explore + auto-fight AI.

The system prompt teaches the LLM combat math (ATK - DEF = damage per round, compute rounds-to-kill vs rounds-to-die) and recovery strategies (corridor running for safe regen, corner kiting to kill tough monsters). The LLM chooses between aggressive, cautious, and exploratory strategies based on each encounter.

## `/playtest` Skill (Claude Code, interactive)

```sh
# Play 5 games (default) using MCP tools in the current session:
/playtest

# Play 10 games with a specific starting seed:
/playtest 10 --seed 42
```

The skill uses the connected MCP server directly. Results are saved to `tools/output/llm_playtest_results.json`.

## `tools/llm_playtest.py` (standalone, dual-backend, unattended)

```sh
# Setup:
pip install -r tools/requirements.txt
cargo build --release --bin mcp_server

# Claude Code backend (uses `claude` CLI, parallel execution):
python3 tools/llm_playtest.py --backend claude-code -n 10 --parallel 5

# API backend (uses Anthropic API directly):
ANTHROPIC_API_KEY=... python3 tools/llm_playtest.py --backend api -n 50

# Reproducible runs with specific seeds:
python3 tools/llm_playtest.py --backend claude-code -n 5 --seed 63519 --parallel 5

# Custom budget and output path:
python3 tools/llm_playtest.py --backend claude-code -n 10 --max-budget 2.00 -o results.json
```

### Backends

- **`claude-code`**: Spawns `claude -p` subprocesses with MCP config. Supports parallel execution. Default budget: $2.00/game.
- **`api`**: Direct Anthropic API tool_use loop with a local MCP server subprocess. Strips map data from old tool results to reduce context growth. Requires `ANTHROPIC_API_KEY`.

### Analytics

Per-game analytics include token usage (input, output, cache creation, cache read), cost, tool call counts, and strategy notes. Both backends output `EnhancedBatchStats`-compatible JSON:

```sh
cat tools/output/llm_playtest_results.json | \
  python3 -c "import json,sys; print(json.dumps(json.load(sys.stdin)['batch_stats']))" | \
  python3 tools/visualize.py batch
```

## Token Optimization

The MCP server supports a `compact` mode (`new_game` with `compact=true`) that omits the ASCII map from all observation responses, significantly reducing token usage for LLM agents that only need stats and entity info. Observation field names are also shortened (e.g., `player_hp` → `hp`, `visible_entities` → `entities`) to reduce per-turn overhead. The API backend additionally strips map data from old conversation turns to limit context window growth.
