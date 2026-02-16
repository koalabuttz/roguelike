#!/usr/bin/env python3
"""Roguelike analytics visualizer.

Reads JSON output from the headless runner and generates PNG charts + text insights.

Usage:
    # Batch analytics (EnhancedBatchStats from stdout):
    cargo run --bin headless -- --games 100 --analytics | python3 tools/visualize.py batch

    # Sweep results (SweepPoint[] from stdout):
    cargo run --bin headless -- --sweep sweep.json | python3 tools/visualize.py sweep

    # Analysis (analysis JSON from file — redirect stderr):
    cargo run --bin headless -- --games 100 --analytics --analysis 2>analysis.json
    python3 tools/visualize.py analysis analysis.json
"""

import argparse
import json
import os
import sys

try:
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    import matplotlib.ticker as mticker
except ImportError:
    print(
        "ERROR: matplotlib is required. Install it with:\n"
        "  python3 -m venv tools/.venv\n"
        "  source tools/.venv/bin/activate\n"
        "  pip install -r tools/requirements.txt",
        file=sys.stderr,
    )
    sys.exit(1)

# ---------------------------------------------------------------------------
# Dark theme matching roguelike aesthetic
# ---------------------------------------------------------------------------
DARK_BG = "#1a1a2e"
PANEL_BG = "#16213e"
TEXT_COLOR = "#e0e0e0"
ACCENT_COLORS = ["#e94560", "#0f3460", "#53d8fb", "#f5a623", "#a29bfe", "#6c5ce7"]

plt.rcParams.update({
    "figure.facecolor": DARK_BG,
    "axes.facecolor": PANEL_BG,
    "axes.edgecolor": TEXT_COLOR,
    "axes.labelcolor": TEXT_COLOR,
    "xtick.color": TEXT_COLOR,
    "ytick.color": TEXT_COLOR,
    "text.color": TEXT_COLOR,
    "font.family": "monospace",
    "font.size": 10,
    "figure.dpi": 150,
})


def ensure_output_dir(output_dir: str) -> None:
    os.makedirs(output_dir, exist_ok=True)


# ---------------------------------------------------------------------------
# Batch mode charts
# ---------------------------------------------------------------------------

def chart_kills_by_type(data: dict, output_dir: str) -> None:
    """Horizontal bar chart of average kills by monster type."""
    kills = data.get("kills_by_type", {})
    if not kills:
        return
    names = sorted(kills.keys(), key=lambda k: kills[k])
    values = [kills[n] for n in names]

    fig, ax = plt.subplots(figsize=(8, max(3, len(names) * 0.6)))
    colors = [ACCENT_COLORS[i % len(ACCENT_COLORS)] for i in range(len(names))]
    ax.barh(names, values, color=colors)
    ax.set_xlabel("Avg Kills per Game")
    ax.set_title("Kills by Monster Type")
    fig.tight_layout()
    fig.savefig(os.path.join(output_dir, "kills_by_type.png"))
    plt.close(fig)


def chart_damage_comparison(data: dict, output_dir: str) -> None:
    """Grouped bar chart: damage dealt vs taken per monster type."""
    dealt = data.get("damage_dealt_by_type", {})
    taken = data.get("damage_taken_by_type", {})
    all_types = sorted(set(dealt.keys()) | set(taken.keys()))
    if not all_types:
        return

    import numpy as np
    x = np.arange(len(all_types))
    width = 0.35
    dealt_vals = [dealt.get(t, 0) for t in all_types]
    taken_vals = [taken.get(t, 0) for t in all_types]

    fig, ax = plt.subplots(figsize=(8, 5))
    ax.bar(x - width / 2, dealt_vals, width, label="Dealt to", color=ACCENT_COLORS[0])
    ax.bar(x + width / 2, taken_vals, width, label="Taken from", color=ACCENT_COLORS[2])
    ax.set_xticks(x)
    ax.set_xticklabels(all_types)
    ax.set_ylabel("Avg Damage per Game")
    ax.set_title("Damage Dealt vs Taken by Monster Type")
    ax.legend()
    fig.tight_layout()
    fig.savefig(os.path.join(output_dir, "damage_comparison.png"))
    plt.close(fig)


def chart_win_rate(data: dict, output_dir: str) -> None:
    """Simple win rate metric display."""
    wr = data.get("win_rate", 0)
    fig, ax = plt.subplots(figsize=(4, 3))
    ax.set_xlim(0, 1)
    ax.set_ylim(0, 1)
    ax.barh([0.5], [wr], height=0.3, color=ACCENT_COLORS[0], alpha=0.8)
    ax.barh([0.5], [1], height=0.3, color=PANEL_BG, alpha=0.3)
    ax.text(0.5, 0.5, f"{wr * 100:.1f}%", ha="center", va="center",
            fontsize=28, fontweight="bold", color=TEXT_COLOR)
    ax.text(0.5, 0.15, f"({data.get('games', '?')} games)", ha="center",
            va="center", fontsize=10, color=TEXT_COLOR)
    ax.set_title("Win Rate")
    ax.set_yticks([])
    ax.set_xticks([])
    ax.spines["top"].set_visible(False)
    ax.spines["right"].set_visible(False)
    ax.spines["bottom"].set_visible(False)
    ax.spines["left"].set_visible(False)
    fig.tight_layout()
    fig.savefig(os.path.join(output_dir, "win_rate.png"))
    plt.close(fig)


# ---------------------------------------------------------------------------
# Sweep mode charts
# ---------------------------------------------------------------------------

def _extract_sweep_axes(points: list) -> dict:
    """Group sweep points by varying parameter, return {param: [(value, stats)]}."""
    axes: dict = {}
    for pt in points:
        overrides = pt.get("overrides", {})
        stats = pt.get("stats", {})
        for param, value in overrides.items():
            if value is not None:
                axes.setdefault(param, []).append((value, stats))
    return axes


def chart_sweep_win_rate(points: list, output_dir: str) -> None:
    """Line chart: win rate vs swept parameter."""
    axes = _extract_sweep_axes(points)
    if not axes:
        return
    fig, ax = plt.subplots(figsize=(8, 5))
    for i, (param, entries) in enumerate(sorted(axes.items())):
        entries.sort(key=lambda e: e[0])
        xs = [e[0] for e in entries]
        ys = [e[1].get("win_rate", 0) * 100 for e in entries]
        ax.plot(xs, ys, marker="o", label=param,
                color=ACCENT_COLORS[i % len(ACCENT_COLORS)])
    ax.set_ylabel("Win Rate (%)")
    ax.set_xlabel("Parameter Value")
    ax.set_title("Win Rate vs Parameter")
    ax.legend()
    ax.yaxis.set_major_formatter(mticker.FormatStrFormatter("%.0f%%"))
    fig.tight_layout()
    fig.savefig(os.path.join(output_dir, "sweep_win_rate.png"))
    plt.close(fig)


def chart_sweep_turns(points: list, output_dir: str) -> None:
    """Line chart: avg turns vs swept parameter."""
    axes = _extract_sweep_axes(points)
    if not axes:
        return
    fig, ax = plt.subplots(figsize=(8, 5))
    for i, (param, entries) in enumerate(sorted(axes.items())):
        entries.sort(key=lambda e: e[0])
        xs = [e[0] for e in entries]
        ys = [e[1].get("avg_turns", 0) for e in entries]
        ax.plot(xs, ys, marker="s", label=param,
                color=ACCENT_COLORS[i % len(ACCENT_COLORS)])
    ax.set_ylabel("Avg Turns")
    ax.set_xlabel("Parameter Value")
    ax.set_title("Average Turns vs Parameter")
    ax.legend()
    fig.tight_layout()
    fig.savefig(os.path.join(output_dir, "sweep_avg_turns.png"))
    plt.close(fig)


def chart_sweep_kills(points: list, output_dir: str) -> None:
    """Line chart: avg kills vs swept parameter."""
    axes = _extract_sweep_axes(points)
    if not axes:
        return
    fig, ax = plt.subplots(figsize=(8, 5))
    for i, (param, entries) in enumerate(sorted(axes.items())):
        entries.sort(key=lambda e: e[0])
        xs = [e[0] for e in entries]
        ys = [e[1].get("avg_kills", 0) for e in entries]
        ax.plot(xs, ys, marker="^", label=param,
                color=ACCENT_COLORS[i % len(ACCENT_COLORS)])
    ax.set_ylabel("Avg Kills")
    ax.set_xlabel("Parameter Value")
    ax.set_title("Average Kills vs Parameter")
    ax.legend()
    fig.tight_layout()
    fig.savefig(os.path.join(output_dir, "sweep_avg_kills.png"))
    plt.close(fig)


# ---------------------------------------------------------------------------
# Analysis mode charts
# ---------------------------------------------------------------------------

def chart_monster_danger(correlations: list, output_dir: str) -> None:
    """Scatter plot: death rate vs avg damage for each monster type."""
    if not correlations:
        return
    fig, ax = plt.subplots(figsize=(8, 6))
    for i, m in enumerate(correlations):
        ax.scatter(
            m.get("avg_damage_dealt", 0),
            m.get("death_rate_when_encountered", 0) * 100,
            s=120,
            color=ACCENT_COLORS[i % len(ACCENT_COLORS)],
            zorder=5,
        )
        ax.annotate(
            m.get("monster_type", "?"),
            (m.get("avg_damage_dealt", 0), m.get("death_rate_when_encountered", 0) * 100),
            textcoords="offset points", xytext=(8, 4),
            fontsize=9, color=TEXT_COLOR,
        )
    ax.set_xlabel("Avg Damage Dealt to Player")
    ax.set_ylabel("Death Rate When Encountered (%)")
    ax.set_title("Monster Danger Ranking")
    ax.yaxis.set_major_formatter(mticker.FormatStrFormatter("%.0f%%"))
    fig.tight_layout()
    fig.savefig(os.path.join(output_dir, "monster_danger.png"))
    plt.close(fig)


def chart_damage_flow_heatmap(flow: dict, output_dir: str) -> None:
    """Heatmap of attacker -> defender damage matrix."""
    entries = flow.get("flows", [])
    if not entries:
        return

    attackers = sorted({e["attacker"] for e in entries})
    defenders = sorted({e["defender"] for e in entries})
    if not attackers or not defenders:
        return

    import numpy as np
    matrix = np.zeros((len(attackers), len(defenders)))
    for e in entries:
        ai = attackers.index(e["attacker"])
        di = defenders.index(e["defender"])
        matrix[ai][di] = e["total_damage"]

    fig, ax = plt.subplots(figsize=(max(5, len(defenders) * 1.2), max(4, len(attackers) * 0.8)))
    im = ax.imshow(matrix, cmap="YlOrRd", aspect="auto")
    ax.set_xticks(range(len(defenders)))
    ax.set_xticklabels(defenders, rotation=45, ha="right")
    ax.set_yticks(range(len(attackers)))
    ax.set_yticklabels(attackers)
    ax.set_xlabel("Defender")
    ax.set_ylabel("Attacker")
    ax.set_title("Damage Flow (Attacker -> Defender)")

    # Annotate cells with values.
    for i in range(len(attackers)):
        for j in range(len(defenders)):
            val = matrix[i][j]
            if val > 0:
                text_c = "white" if val > matrix.max() * 0.6 else TEXT_COLOR
                ax.text(j, i, f"{int(val)}", ha="center", va="center",
                        fontsize=9, color=text_c)

    fig.colorbar(im, ax=ax, label="Total Damage")
    fig.tight_layout()
    fig.savefig(os.path.join(output_dir, "damage_flow.png"))
    plt.close(fig)


# ---------------------------------------------------------------------------
# Insight generation
# ---------------------------------------------------------------------------

def batch_insights(data: dict) -> list[str]:
    """Generate text insights from EnhancedBatchStats."""
    insights = []
    wr = data.get("win_rate", 0)
    games = data.get("games", 0)

    # Most dangerous monster (highest avg damage taken).
    taken = data.get("damage_taken_by_type", {})
    if taken:
        worst = max(taken.items(), key=lambda x: x[1])
        insights.append(f"Most dangerous: {worst[0]} (avg {worst[1]:.1f} damage taken/game)")

    # Most killed monster.
    kills = data.get("kills_by_type", {})
    if kills:
        top_kill = max(kills.items(), key=lambda x: x[1])
        insights.append(f"Most hunted: {top_kill[0]} (avg {top_kill[1]:.1f} kills/game)")

    # Win rate assessment.
    if wr >= 0.7:
        assessment = "strong"
    elif wr >= 0.4:
        assessment = "balanced"
    else:
        assessment = "deadly"
    insights.append(f"Win rate: {wr * 100:.0f}% -- {assessment} difficulty ({games} games)")

    # Avg turns.
    avg_turns = data.get("avg_turns", 0)
    if avg_turns:
        insights.append(f"Avg survival: {avg_turns:.0f} turns")

    # Damage efficiency.
    dealt = data.get("damage_dealt_by_type", {})
    total_dealt = sum(dealt.values()) if dealt else 0
    total_taken = sum(taken.values()) if taken else 0
    if total_taken > 0:
        ratio = total_dealt / total_taken
        insights.append(f"Damage efficiency: Player deals {ratio:.1f}x more than received")

    return insights


def sweep_insights(points: list) -> list[str]:
    """Generate text insights from SweepPoint[]."""
    insights = []
    if not points:
        return insights

    # Find 50% win rate threshold per parameter.
    axes = _extract_sweep_axes(points)
    for param, entries in sorted(axes.items()):
        entries.sort(key=lambda e: e[0])
        prev_val = None
        prev_wr = None
        for val, stats in entries:
            wr = stats.get("win_rate", 0)
            if prev_wr is not None and prev_wr < 0.5 <= wr:
                insights.append(
                    f"Survivability threshold: {param} >= {val} for >50% win rate"
                )
            prev_val = val
            prev_wr = wr

        # Optimal range (>70% win rate).
        high_wr = [val for val, stats in entries if stats.get("win_rate", 0) >= 0.7]
        if high_wr:
            insights.append(
                f"Optimal range: {param} in [{min(high_wr)}, {max(high_wr)}] for >70% win rate"
            )

        # Diminishing returns: where win rate increase drops below 5% per step.
        for i in range(1, len(entries)):
            wr_delta = entries[i][1].get("win_rate", 0) - entries[i - 1][1].get("win_rate", 0)
            if wr_delta < 0.05 and entries[i][1].get("win_rate", 0) > 0.5:
                insights.append(
                    f"Diminishing returns: increasing {param} beyond {entries[i][0]} "
                    f"adds <5% win rate"
                )
                break

    return insights


def analysis_insights(difficulty: dict, correlations: list, flow: dict) -> list[str]:
    """Generate text insights from analysis data."""
    insights = []

    # #1 cause of player death.
    if correlations:
        worst = max(correlations, key=lambda c: c.get("death_rate_when_encountered", 0))
        dr = worst.get("death_rate_when_encountered", 0) * 100
        dmg = worst.get("avg_damage_dealt", 0)
        insights.append(
            f"#1 cause of death: {worst['monster_type']} "
            f"(death rate {dr:.0f}% when encountered, avg {dmg:.1f} dmg)"
        )

    # Damage asymmetry from flow.
    entries = flow.get("flows", [])
    player_deals = sum(e["total_damage"] for e in entries if e["attacker"] == "Player")
    player_takes = sum(e["total_damage"] for e in entries if e["defender"] == "Player")
    if player_takes > 0:
        ratio = player_deals / player_takes
        if ratio > 1.5:
            insights.append(f"Damage asymmetry: Player deals {ratio:.1f}x what they take (player favored)")
        elif ratio < 0.8:
            insights.append(f"Damage asymmetry: Player deals only {ratio:.1f}x what they take (monster favored)")
        else:
            insights.append(f"Damage asymmetry: {ratio:.1f}x ratio (roughly balanced)")

    # Monster power ranking.
    if correlations:
        ranked = sorted(
            correlations,
            key=lambda c: c.get("death_rate_when_encountered", 0) * c.get("avg_damage_dealt", 0),
            reverse=True,
        )
        ranking = ", ".join(
            f"{m['monster_type']} ({m.get('death_rate_when_encountered', 0) * 100:.0f}%)"
            for m in ranked
        )
        insights.append(f"Monster power ranking: {ranking}")

    return insights


# ---------------------------------------------------------------------------
# Subcommand handlers
# ---------------------------------------------------------------------------

def cmd_batch(args: argparse.Namespace) -> None:
    data = json.load(sys.stdin)
    ensure_output_dir(args.output_dir)

    chart_kills_by_type(data, args.output_dir)
    chart_damage_comparison(data, args.output_dir)
    chart_win_rate(data, args.output_dir)

    print(f"Charts saved to {args.output_dir}/")

    insights = batch_insights(data)
    if insights:
        print("\nINSIGHTS:")
        for i in insights:
            print(f"  * {i}")


def cmd_sweep(args: argparse.Namespace) -> None:
    points = json.load(sys.stdin)
    ensure_output_dir(args.output_dir)

    chart_sweep_win_rate(points, args.output_dir)
    chart_sweep_turns(points, args.output_dir)
    chart_sweep_kills(points, args.output_dir)

    print(f"Charts saved to {args.output_dir}/")

    insights = sweep_insights(points)
    if insights:
        print("\nINSIGHTS:")
        for i in insights:
            print(f"  * {i}")


def cmd_analysis(args: argparse.Namespace) -> None:
    if not args.file:
        print("ERROR: analysis mode requires a file argument", file=sys.stderr)
        print("Usage: python3 tools/visualize.py analysis <analysis.json>", file=sys.stderr)
        sys.exit(1)

    with open(args.file) as f:
        raw = f.read()

    # The analysis file may contain cargo build output, progress lines, and
    # "--- Analysis ---" headers mixed with JSON. We extract only the JSON
    # objects/arrays by looking for lines starting with '{' or '['.
    # Expected order: PresetDifficulty, MonsterCorrelation[], DamageFlow
    lines = raw.split("\n")
    json_chunks: list[str] = []
    current_chunk: list[str] = []
    in_json = False
    depth = 0

    for line in lines:
        stripped = line.strip()
        if not in_json and stripped and stripped[0] in ("{", "["):
            in_json = True
            current_chunk = []

        if in_json:
            current_chunk.append(line)
            depth += stripped.count("{") + stripped.count("[")
            depth -= stripped.count("}") + stripped.count("]")
            if depth <= 0:
                json_chunks.append("\n".join(current_chunk))
                in_json = False
                depth = 0

    objects = []
    for chunk in json_chunks:
        try:
            objects.append(json.loads(chunk))
        except json.JSONDecodeError:
            continue

    difficulty = objects[0] if len(objects) > 0 else {}
    correlations = objects[1] if len(objects) > 1 else []
    flow = objects[2] if len(objects) > 2 else {}

    ensure_output_dir(args.output_dir)

    chart_monster_danger(correlations, args.output_dir)
    chart_damage_flow_heatmap(flow, args.output_dir)

    print(f"Charts saved to {args.output_dir}/")

    insights = analysis_insights(difficulty, correlations, flow)
    if insights:
        print("\nINSIGHTS:")
        for i in insights:
            print(f"  * {i}")


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def main() -> None:
    parser = argparse.ArgumentParser(
        description="Roguelike analytics visualizer",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--output-dir", default="tools/output",
        help="Directory for output PNGs (default: tools/output)",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    subparsers.add_parser("batch", help="Visualize batch analytics (reads JSON from stdin)")
    subparsers.add_parser("sweep", help="Visualize sweep results (reads JSON from stdin)")

    analysis_parser = subparsers.add_parser(
        "analysis", help="Visualize analysis data (reads from file)"
    )
    analysis_parser.add_argument("file", nargs="?", help="Path to analysis JSON file")

    args = parser.parse_args()

    if args.command == "batch":
        cmd_batch(args)
    elif args.command == "sweep":
        cmd_sweep(args)
    elif args.command == "analysis":
        cmd_analysis(args)


if __name__ == "__main__":
    main()
