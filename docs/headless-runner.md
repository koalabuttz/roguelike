# Headless Runner (Automated Playtesting)

The headless runner plays games automatically using auto-explore + auto-fight AI. It requires the `dev-tools` feature (enabled by default).

```sh
# Run 100 games, output aggregate JSON stats:
cargo run --bin headless -- --games 100

# Run with analytics (per-monster damage/kill tracking):
cargo run --bin headless -- --games 50 --analytics

# Run with analytics + difficulty analysis:
cargo run --bin headless -- --games 50 --analytics --analysis

# Run a parameter sweep (vary player stats, measure win rate):
cargo run --bin headless -- --sweep sweep.json

# Save a golden replay for regression testing:
cargo run --bin headless -- --save-golden crates/core/tests/golden_replays/my_test.json --seed 42

# Regenerate all golden replays after an intentional gameplay change:
cargo run --bin headless -- --regenerate-goldens crates/core/tests/golden_replays/

# Replay a recorded game:
cargo run --bin headless -- --replay replay.json
```

## CLI Flags

| Flag | Description |
|------|-------------|
| `-n`, `--games N` | Number of games to run (default: 10) |
| `-w`, `--width N` | Map width (default: 80) |
| `-H`, `--height N` | Map height (default: 40) |
| `-s`, `--seed N` | Starting seed (increments per game) |
| `-p`, `--preset NAME` | Map preset: `arena`, `corridor`, `labyrinth`, `single_room`, `open_field` |
| `-t`, `--max-turns N` | Max turns per game (default: 500) |
| `-r`, `--replay FILE` | Replay a recorded game from JSON |
| `--save-replays` | Save replay JSON for each game |
| `--analytics` | Collect per-game combat analytics (snapshot/diff each step) |
| `--analysis` | With `--analytics`, compute difficulty metrics and monster correlations |
| `--report FILE` | Generate self-contained HTML report with charts (requires `--analytics` or `--sweep`) |
| `--sweep FILE` | Run parameter sweep from JSON config |
| `--save-golden FILE` | Save run as golden replay JSON for regression testing |
| `--regenerate-goldens DIR` | Re-execute all goldens in a directory, update expected outcomes |

## Parameter Sweep Config

Sweeps test how game balance changes across different player configurations:

```json
{
  "axes": [
    { "param": "player_hp", "values": [10, 20, 30] },
    { "param": "player_attack", "values": [3, 5, 7] }
  ],
  "games_per_point": 10,
  "width": 80,
  "height": 40,
  "max_turns": 500,
  "preset": null
}
```

Supported sweep parameters: `player_hp`, `player_attack`, `player_defense`, `regen_interval`, `max_monsters_per_room`.

## Visualization

Two tools for visualizing analytics output:

### HTML Report (built-in, zero dependencies)

```sh
# Basic report with charts and insights:
cargo run --bin headless -- --games 100 --analytics --report report.html

# Full report with analysis (monster danger, damage flow):
cargo run --bin headless -- --games 100 --analytics --analysis --report report.html

# Sweep report:
cargo run --bin headless -- --sweep sweep.json --report sweep_report.html
```

Opens in any browser. Uses Chart.js (loaded from CDN) with a dark theme.

### Balance Diff (`tools/balance_diff.py`, stdlib only)

```sh
# Compare two combined stats JSON files and output a markdown diff:
python3 tools/balance_diff.py baseline.json current.json
```

Compares win rate, avg turns/kills/HP/explored across presets, flags per-monster damage changes >= 5%, and emits a verdict: STABLE, MINOR SHIFT, or BALANCE SHIFT. Used automatically by the CI balance workflow.

### Python Charts (`tools/visualize.py`, requires matplotlib)

```sh
# Setup (one-time):
python3 -m venv tools/.venv
source tools/.venv/bin/activate
pip install -r tools/requirements.txt

# Batch analytics -> PNGs:
cargo run --bin headless -- --games 100 --analytics | python3 tools/visualize.py batch

# Sweep results -> PNGs:
cargo run --bin headless -- --sweep sweep.json | python3 tools/visualize.py sweep

# Analysis data -> PNGs:
cargo run --bin headless -- --games 100 --analytics --analysis 2>analysis.json
python3 tools/visualize.py analysis analysis.json
```

Output PNGs are saved to `tools/output/` (or `--output-dir DIR`). Both tools also print text insights to stdout.
