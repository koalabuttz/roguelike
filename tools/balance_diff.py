#!/usr/bin/env python3
"""Compare two balance stats JSON files and output a markdown diff report.

Usage: python3 tools/balance_diff.py <baseline.json> <current.json>

The JSON files should have the shape:
  {"default": <EnhancedBatchStats>, "arena": <...>, "corridor": <...>}

Outputs markdown to stdout suitable for GitHub Step Summary or PR comments.
Exit code is always 0 (informational, not a gate).
"""

import json
import sys

# --- Thresholds ---
# Win rate thresholds (percentage points)
WIN_RATE_MAJOR_PP = 5.0
WIN_RATE_MINOR_PP = 2.0
# Relative change thresholds (%)
TURNS_MAJOR_PCT = 10.0
TURNS_MINOR_PCT = 5.0
KILLS_MAJOR_PCT = 10.0
# Per-monster damage change threshold (%)
MONSTER_CHANGE_PCT = 5.0

PRESET_GAMES = {"default": 500, "arena": 50, "corridor": 50}


def fmt_pct(value):
    """Format a float as a percentage string."""
    return f"{value:.1f}%"


def fmt_delta_pp(delta):
    """Format a percentage-point delta with sign and label."""
    sign = "+" if delta >= 0 else ""
    label = "easier" if delta > 0 else "harder" if delta < 0 else ""
    suffix = f" ({label})" if label else ""
    return f"{sign}{delta:.1f}pp{suffix}"


def fmt_delta_rel(baseline, current):
    """Format a relative % change between two values."""
    if baseline == 0:
        if current == 0:
            return "0.0%"
        return "+inf%" if current > 0 else "-inf%"
    delta = ((current - baseline) / abs(baseline)) * 100
    sign = "+" if delta >= 0 else ""
    return f"{sign}{delta:.1f}%"


def rel_change(baseline, current):
    """Compute relative change as a fraction (not %)."""
    if baseline == 0:
        return 0.0 if current == 0 else float("inf")
    return ((current - baseline) / abs(baseline)) * 100


def compare_preset(name, baseline, current):
    """Compare a single preset's stats. Returns (markdown_rows, verdict_level)."""
    games = PRESET_GAMES.get(name, "?")

    metrics = [
        ("Win Rate", "win_rate", True),       # True = percentage-point comparison
        ("Avg Turns", "avg_turns", False),     # False = relative comparison
        ("Avg Kills", "avg_kills", False),
        ("Avg HP Remaining", "avg_hp_remaining", False),
        ("Avg Explored", "avg_explored_pct", True),
    ]

    rows = []
    verdict = "STABLE"

    for label, field, is_pp in metrics:
        bv = baseline.get(field, 0)
        cv = current.get(field, 0)

        if is_pp:
            b_str = fmt_pct(bv)
            c_str = fmt_pct(cv)
            delta = cv - bv
            d_str = fmt_delta_pp(delta)
        else:
            b_str = f"{bv:.1f}"
            c_str = f"{cv:.1f}"
            d_str = fmt_delta_rel(bv, cv)
            delta = rel_change(bv, cv)

        rows.append(f"| {label} | {b_str} | {c_str} | {d_str} |")

        # Verdict escalation
        if field == "win_rate":
            abs_delta = abs(cv - bv)
            if abs_delta >= WIN_RATE_MAJOR_PP:
                verdict = "BALANCE SHIFT"
            elif abs_delta >= WIN_RATE_MINOR_PP and verdict != "BALANCE SHIFT":
                verdict = "MINOR SHIFT"
        elif field == "avg_turns":
            abs_rel = abs(delta)
            if abs_rel >= TURNS_MAJOR_PCT:
                verdict = "BALANCE SHIFT"
            elif abs_rel >= TURNS_MINOR_PCT and verdict != "BALANCE SHIFT":
                verdict = "MINOR SHIFT"
        elif field == "avg_kills":
            if abs(delta) >= KILLS_MAJOR_PCT:
                verdict = "BALANCE SHIFT"

    # Per-monster breakdown
    monster_rows = []
    all_monsters = set()
    for category in ("damage_dealt_by_type", "damage_taken_by_type"):
        all_monsters.update(baseline.get(category, {}).keys())
        all_monsters.update(current.get(category, {}).keys())

    for monster in sorted(all_monsters):
        for category, cat_label in [
            ("damage_dealt_by_type", "Dealt"),
            ("damage_taken_by_type", "Taken"),
        ]:
            bv = baseline.get(category, {}).get(monster, 0)
            cv = current.get(category, {}).get(monster, 0)
            rc = rel_change(bv, cv)
            if abs(rc) >= MONSTER_CHANGE_PCT or (bv == 0) != (cv == 0):
                d_str = fmt_delta_rel(bv, cv)
                monster_rows.append(
                    f"| {monster} | {cat_label} | {bv:.1f} | {cv:.1f} | {d_str} |"
                )

    header = f"### {name.title()} Preset ({games} games)\n"
    table = "| Metric | Baseline | Current | Delta |\n"
    table += "|--------|----------|---------|-------|\n"
    table += "\n".join(rows)

    section = header + table

    if monster_rows:
        section += "\n\n#### Per-Monster Changes\n"
        section += "| Monster | Category | Baseline | Current | Delta |\n"
        section += "|---------|----------|----------|---------|-------|\n"
        section += "\n".join(monster_rows)

    return section, verdict


def main():
    if len(sys.argv) < 3:
        print("Usage: python3 tools/balance_diff.py <baseline.json> <current.json>",
              file=sys.stderr)
        sys.exit(1)

    baseline_path = sys.argv[1]
    current_path = sys.argv[2]

    # Load baseline (missing = no comparison possible)
    try:
        with open(baseline_path) as f:
            baseline = json.load(f)
    except FileNotFoundError:
        print("<!-- balance-check -->")
        print("## Balance Check Results")
        print("**No baseline available** — this run will establish the baseline.")
        sys.exit(0)

    with open(current_path) as f:
        current = json.load(f)

    # Determine which presets to compare
    all_presets = sorted(set(list(baseline.keys()) + list(current.keys())))

    sections = []
    overall_verdict = "STABLE"

    for preset in all_presets:
        if preset not in baseline:
            sections.append(f"### {preset.title()} Preset\n*New preset (no baseline)*")
            continue
        if preset not in current:
            sections.append(f"### {preset.title()} Preset\n*Preset removed from current run*")
            continue

        section, verdict = compare_preset(preset, baseline[preset], current[preset])
        sections.append(section)

        # Escalate overall verdict
        if verdict == "BALANCE SHIFT":
            overall_verdict = "BALANCE SHIFT"
        elif verdict == "MINOR SHIFT" and overall_verdict != "BALANCE SHIFT":
            overall_verdict = "MINOR SHIFT"

    # Output
    print("<!-- balance-check -->")
    print("## Balance Check Results")
    print(f"**Verdict: {overall_verdict}**")
    print()
    print("\n\n".join(sections))


if __name__ == "__main__":
    main()
