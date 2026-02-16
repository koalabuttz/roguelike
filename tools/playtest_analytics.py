"""Shared analytics module for LLM playtesting.

Provides functions to track per-game analytics from MCP tool responses and
aggregate them into EnhancedBatchStats-compatible JSON for use with
tools/visualize.py and the headless runner's --report flag.
"""

import json
import time
from pathlib import Path

# ---------------------------------------------------------------------------
# Monster stats (from src/data.rs)
# ---------------------------------------------------------------------------

MONSTER_STATS = {
    "Goblin": {"hp": 6, "attack": 3, "defense": 0},
    "Orc": {"hp": 12, "attack": 4, "defense": 1},
    "Troll": {"hp": 20, "attack": 6, "defense": 3},
}

PLAYER_ATTACK = 5
PLAYER_DEFENSE = 2
PLAYER_MAX_HP = 30


# ---------------------------------------------------------------------------
# Per-game analytics
# ---------------------------------------------------------------------------

def new_game_analytics(seed):
    """Create an empty per-game analytics dict matching GameAnalytics fields."""
    return {
        "kills_by_type": {},
        "damage_dealt_by_type": {},
        "damage_taken_by_type": {},
        "final_hp": 0,
        "explored_pct": 0,
        "first_kill_turn": None,
        "last_kill_turn": None,
        "monsters_spawned": 0,
        "turns": 0,
        "game_over": False,
        "seed": seed,
        "llm_metrics": {
            "tool_calls": 0,
            "decision_count": 0,
            "auto_explore_calls": 0,
            "auto_fight_calls": 0,
            "strategy_notes": "",
            "model": "",
        },
    }


def update_fight_analytics(analytics, response):
    """Record damage dealt/taken from an auto_fight MCP response.

    The response dict should contain: fight_target, fight_rounds,
    fight_hp_lost, fight_target_killed, kills.
    """
    target = response.get("fight_target", "Unknown")
    rounds = response.get("fight_rounds", 0)
    hp_lost = response.get("fight_hp_lost", 0)
    killed = response.get("fight_target_killed", False)
    total_kills = response.get("kills", 0)

    # Damage taken from this monster.
    analytics["damage_taken_by_type"][target] = (
        analytics["damage_taken_by_type"].get(target, 0) + hp_lost
    )

    # Estimate damage dealt: rounds * max(0, PLAYER_ATK - monster_DEF).
    monster_def = MONSTER_STATS.get(target, {}).get("defense", 0)
    damage_per_round = max(0, PLAYER_ATTACK - monster_def)
    damage_dealt = rounds * damage_per_round
    analytics["damage_dealt_by_type"][target] = (
        analytics["damage_dealt_by_type"].get(target, 0) + damage_dealt
    )

    if killed:
        analytics["kills_by_type"][target] = (
            analytics["kills_by_type"].get(target, 0) + 1
        )
        turn = analytics["turns"]
        if analytics["first_kill_turn"] is None:
            analytics["first_kill_turn"] = turn
        analytics["last_kill_turn"] = turn

    analytics["llm_metrics"]["auto_fight_calls"] += 1


def finalize_game(analytics, last_observation, llm_metrics=None):
    """Fill final fields from the last observation and optional LLM metrics."""
    analytics["final_hp"] = last_observation.get("hp", 0)
    analytics["explored_pct"] = last_observation.get("explored", 0)
    analytics["game_over"] = last_observation.get("game_over", False)
    # Turns: use kills as a proxy if not tracked directly — the observation
    # doesn't expose turn_count, but we track tool calls as a rough measure.
    # The seed is already set at creation time.

    if llm_metrics:
        analytics["llm_metrics"].update(llm_metrics)


# ---------------------------------------------------------------------------
# Aggregation (mirrors analytics::aggregate() in Rust)
# ---------------------------------------------------------------------------

def aggregate(all_games):
    """Compute EnhancedBatchStats-compatible dict from a list of game analytics."""
    n = len(all_games)
    if n == 0:
        return {
            "games": 0,
            "win_rate": 0.0,
            "avg_turns": 0.0,
            "avg_kills": 0.0,
            "avg_hp_remaining": 0.0,
            "avg_explored_pct": 0.0,
            "kills_by_type": {},
            "damage_dealt_by_type": {},
            "damage_taken_by_type": {},
            "avg_first_kill_turn": None,
        }

    wins = sum(1 for g in all_games if not g["game_over"])
    total_kills = sum(sum(g["kills_by_type"].values()) for g in all_games)
    total_hp = sum(g["final_hp"] for g in all_games)
    total_explored = sum(g["explored_pct"] for g in all_games)
    total_turns = sum(g["turns"] for g in all_games)

    # Per-type aggregates.
    kills_by_type = {}
    damage_dealt_by_type = {}
    damage_taken_by_type = {}

    for g in all_games:
        for k, v in g["kills_by_type"].items():
            kills_by_type[k] = kills_by_type.get(k, 0) + v
        for k, v in g["damage_dealt_by_type"].items():
            damage_dealt_by_type[k] = damage_dealt_by_type.get(k, 0) + v
        for k, v in g["damage_taken_by_type"].items():
            damage_taken_by_type[k] = damage_taken_by_type.get(k, 0) + v

    for d in (kills_by_type, damage_dealt_by_type, damage_taken_by_type):
        for k in d:
            d[k] /= n

    first_kills = [g["first_kill_turn"] for g in all_games if g["first_kill_turn"] is not None]
    avg_first_kill_turn = (
        sum(first_kills) / len(first_kills) if first_kills else None
    )

    return {
        "games": n,
        "win_rate": wins / n,
        "avg_turns": total_turns / n,
        "avg_kills": total_kills / n,
        "avg_hp_remaining": total_hp / n,
        "avg_explored_pct": total_explored / n,
        "kills_by_type": kills_by_type,
        "damage_dealt_by_type": damage_dealt_by_type,
        "damage_taken_by_type": damage_taken_by_type,
        "avg_first_kill_turn": avg_first_kill_turn,
    }


# ---------------------------------------------------------------------------
# Output
# ---------------------------------------------------------------------------

def write_results(path, all_analytics, meta=None):
    """Write results JSON with batch_stats, per_game, and meta sections."""
    batch_stats = aggregate(all_analytics)

    # Strip llm_metrics from per-game for the batch_stats feed, but keep in per_game.
    result = {
        "batch_stats": batch_stats,
        "per_game": all_analytics,
        "meta": meta or {},
    }

    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w") as f:
        json.dump(result, f, indent=2)

    return batch_stats
