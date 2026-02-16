#!/usr/bin/env python3
"""LLM-driven roguelike playtesting via the Anthropic API.

Spawns the MCP server as a subprocess and drives games through the Anthropic
API's tool_use loop. Each game gets a fresh conversation to avoid context
overflow. Outputs EnhancedBatchStats-compatible JSON for use with
tools/visualize.py and the headless runner's --report flag.

Usage:
    # Run 50 games with default model:
    ANTHROPIC_API_KEY=... python3 tools/llm_playtest.py -n 50

    # Use specific model and seed:
    python3 tools/llm_playtest.py -n 20 -m claude-sonnet-4-20250514 -s 42

    # Custom output, generate charts after:
    python3 tools/llm_playtest.py -n 100 -o results.json --report
"""

import argparse
import json
import subprocess
import sys
import time
from pathlib import Path

# Allow importing playtest_analytics from the same directory.
sys.path.insert(0, str(Path(__file__).parent))
import playtest_analytics as pa

try:
    import anthropic
except ImportError:
    print(
        "ERROR: anthropic SDK is required. Install it with:\n"
        "  pip install anthropic>=0.40\n"
        "Or use the tools venv:\n"
        "  source tools/.venv/bin/activate\n"
        "  pip install -r tools/requirements.txt",
        file=sys.stderr,
    )
    sys.exit(1)


# ---------------------------------------------------------------------------
# MCP client — stdio JSON-RPC to the mcp_server subprocess
# ---------------------------------------------------------------------------

class McpClient:
    """Communicates with the roguelike MCP server over stdio JSON-RPC."""

    def __init__(self, binary_path):
        self.binary_path = binary_path
        self.process = None
        self._request_id = 0

    def start(self):
        """Spawn the MCP server subprocess."""
        self.process = subprocess.Popen(
            [self.binary_path],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        # Send initialize handshake.
        result = self._call("initialize", {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "llm-playtest", "version": "1.0.0"},
        })
        # Send initialized notification (no id, no response expected).
        self._send_notification("notifications/initialized", {})
        return result

    def stop(self):
        """Terminate the MCP server subprocess."""
        if self.process:
            self.process.stdin.close()
            self.process.wait(timeout=5)
            self.process = None

    def call_tool(self, name, arguments=None):
        """Call an MCP tool and return the parsed result content."""
        result = self._call("tools/call", {
            "name": name,
            "arguments": arguments or {},
        })
        # MCP tool results have a "content" array; extract text content.
        content = result.get("content", [])
        for item in content:
            if item.get("type") == "text":
                try:
                    return json.loads(item["text"])
                except (json.JSONDecodeError, KeyError):
                    return item.get("text", "")
        return result

    def _next_id(self):
        self._request_id += 1
        return self._request_id

    def _call(self, method, params):
        """Send a JSON-RPC request and wait for the response."""
        req_id = self._next_id()
        request = {
            "jsonrpc": "2.0",
            "id": req_id,
            "method": method,
            "params": params,
        }
        self._send(request)
        return self._recv(req_id)

    def _send_notification(self, method, params):
        """Send a JSON-RPC notification (no id, no response)."""
        notification = {
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }
        self._send(notification)

    def _send(self, obj):
        """Write a JSON-RPC message to the subprocess stdin."""
        line = json.dumps(obj) + "\n"
        self.process.stdin.write(line.encode())
        self.process.stdin.flush()

    def _recv(self, expected_id):
        """Read JSON-RPC responses until we get the one matching expected_id."""
        while True:
            line = self.process.stdout.readline()
            if not line:
                raise ConnectionError("MCP server closed stdout")
            line = line.strip()
            if not line:
                continue
            try:
                msg = json.loads(line)
            except json.JSONDecodeError:
                continue
            # Skip notifications (no id).
            if "id" not in msg:
                continue
            if msg.get("id") == expected_id:
                if "error" in msg:
                    err = msg["error"]
                    raise RuntimeError(
                        f"MCP error {err.get('code')}: {err.get('message')}"
                    )
                return msg.get("result", {})


# ---------------------------------------------------------------------------
# Anthropic API tool schemas (mirror MCP tools)
# ---------------------------------------------------------------------------

TOOL_SCHEMAS = [
    {
        "name": "new_game",
        "description": "Start a new roguelike game. Returns the initial game state.",
        "input_schema": {
            "type": "object",
            "properties": {
                "width": {"type": "integer", "description": "Map width (default 80)"},
                "height": {"type": "integer", "description": "Map height (default 40)"},
                "seed": {"type": "integer", "description": "Random seed"},
            },
        },
    },
    {
        "name": "act",
        "description": (
            "Take an action: move_north/south/east/west/ne/nw/se/sw, wait, "
            "autorun_<dir>, or auto_fight. auto_fight resolves combat with the "
            "weakest adjacent monster in one call."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "action": {"type": "string", "description": "The action to take"},
            },
            "required": ["action"],
        },
    },
    {
        "name": "auto_explore",
        "description": (
            "Find nearest frontier and walk to it. Stops for monsters, damage, "
            "or reaching the frontier. Best way to explore."
        ),
        "input_schema": {"type": "object", "properties": {}},
    },
    {
        "name": "pathfind_to",
        "description": "A* pathfind to target tile. Stops for monsters or damage.",
        "input_schema": {
            "type": "object",
            "properties": {
                "x": {"type": "integer", "description": "Target X"},
                "y": {"type": "integer", "description": "Target Y"},
            },
            "required": ["x", "y"],
        },
    },
    {
        "name": "observe",
        "description": "Get current visible state without taking an action.",
        "input_schema": {"type": "object", "properties": {}},
    },
    {
        "name": "get_explored_map",
        "description": "Full explored map with frontier markers (~).",
        "input_schema": {"type": "object", "properties": {}},
    },
    {
        "name": "get_rules",
        "description": "Read game rules and mechanics.",
        "input_schema": {"type": "object", "properties": {}},
    },
]

SYSTEM_PROMPT = """\
You are playtesting a roguelike dungeon crawler. Play strategically to survive \
and explore the dungeon fully.

Strategy guidelines:
- Use auto_explore for all movement (maximizes tiles per tool call)
- Use auto_fight (via act with action "auto_fight") for all combat
- HP > 15 or monster is Goblin/Orc: fight
- HP <= 10 and monster is Troll: retreat (pathfind away, then auto_explore)
- HP 10-15: fight Goblins, avoid Trolls, judge Orcs by remaining HP
- You regenerate 1 HP every 3 turns — retreating to explore recovers HP
- Target: complete the game in 15-30 tool calls

Game flow:
1. Call new_game to start (seed will be provided in the first user message)
2. Loop: auto_explore → when monster spotted, decide fight/flee → auto_fight or retreat → repeat
3. Game ends when game_over=true (you died) or explored_pct is high and no frontiers remain

Keep responses extremely brief — state your action and reasoning in 1 sentence.
When the game ends (game_over=true or fully explored), write a 1-2 sentence \
summary of key decisions: tactical retreats, close calls, what killed you, or \
how you cleared the dungeon. This is your strategy_notes for the run.\
"""


# ---------------------------------------------------------------------------
# Game loop
# ---------------------------------------------------------------------------

def play_game(client, api_client, model, seed, max_tool_calls=60):
    """Play a single game via the Anthropic API tool_use loop.

    Returns the per-game analytics dict.
    """
    analytics = pa.new_game_analytics(seed)
    analytics["llm_metrics"]["model"] = model

    messages = [
        {"role": "user", "content": f"Play a new game with seed {seed}. Start by calling new_game with that seed."},
    ]

    tool_calls = 0
    last_observation = {}
    stall_count = 0
    last_kills = 0
    last_explored = 0

    while tool_calls < max_tool_calls:
        response = api_client.messages.create(
            model=model,
            max_tokens=1024,
            system=SYSTEM_PROMPT,
            tools=TOOL_SCHEMAS,
            messages=messages,
        )

        # Process the response content blocks.
        assistant_content = response.content
        messages.append({"role": "assistant", "content": assistant_content})

        # Check for stop_reason — if end_turn with no tool_use, game is done.
        if response.stop_reason == "end_turn":
            break

        # Capture text blocks as strategy notes.
        for block in assistant_content:
            if block.type == "text" and block.text.strip():
                # Keep the last substantive text as strategy notes.
                analytics["llm_metrics"]["strategy_notes"] = block.text.strip()

        # Process each tool_use block.
        tool_results = []
        for block in assistant_content:
            if block.type != "tool_use":
                continue

            tool_calls += 1
            analytics["llm_metrics"]["tool_calls"] = tool_calls
            tool_name = block.name
            tool_input = block.input

            # Execute against MCP server.
            try:
                result = client.call_tool(tool_name, tool_input)
            except RuntimeError as e:
                # MCP error — return error to the LLM.
                tool_results.append({
                    "type": "tool_result",
                    "tool_use_id": block.id,
                    "content": f"Error: {e}",
                    "is_error": True,
                })
                continue

            # Track analytics from the response.
            if isinstance(result, dict):
                last_observation = result

                if tool_name == "auto_explore":
                    analytics["llm_metrics"]["auto_explore_calls"] += 1
                elif tool_name == "act" and tool_input.get("action") == "auto_fight":
                    pa.update_fight_analytics(analytics, result)
                elif tool_name not in ("new_game", "get_rules", "observe",
                                        "get_explored_map"):
                    analytics["llm_metrics"]["decision_count"] += 1

                # Update running turn count from kills (proxy).
                current_kills = result.get("kills", last_kills)
                current_explored = result.get("explored_pct", last_explored)

                # Stall detection: no progress for 10 calls.
                if current_kills == last_kills and current_explored == last_explored:
                    stall_count += 1
                else:
                    stall_count = 0
                last_kills = current_kills
                last_explored = current_explored

                if stall_count >= 10:
                    break

                # Game over check.
                if result.get("game_over", False):
                    tool_results.append({
                        "type": "tool_result",
                        "tool_use_id": block.id,
                        "content": json.dumps(result),
                    })
                    break
            else:
                # String result (e.g., get_rules).
                pass

            tool_results.append({
                "type": "tool_result",
                "tool_use_id": block.id,
                "content": json.dumps(result) if isinstance(result, dict) else str(result),
            })

        if not tool_results:
            break

        messages.append({"role": "user", "content": tool_results})

        # Break if game is over.
        if last_observation.get("game_over", False):
            break

        if stall_count >= 10:
            break

    # Finalize analytics.
    analytics["turns"] = last_observation.get("kills", 0)  # rough proxy
    pa.finalize_game(analytics, last_observation)

    return analytics


# ---------------------------------------------------------------------------
# CLI and batch runner
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(
        description="LLM-driven roguelike playtesting via Anthropic API",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=(
            "Examples:\n"
            "  python3 tools/llm_playtest.py -n 50\n"
            "  python3 tools/llm_playtest.py -n 20 -m claude-sonnet-4-20250514 -s 42\n"
            "  python3 tools/llm_playtest.py -n 100 -o results.json --report\n"
        ),
    )
    parser.add_argument("-n", "--games", type=int, default=10, help="Number of games (default: 10)")
    parser.add_argument("-m", "--model", default="claude-sonnet-4-20250514", help="Anthropic model ID")
    parser.add_argument("-s", "--seed", type=int, default=None, help="Starting seed (increments per game)")
    parser.add_argument("-o", "--output", default="tools/output/llm_playtest_results.json", help="Output JSON path")
    parser.add_argument("--mcp-binary", default="target/release/mcp_server", help="Path to MCP server binary")
    parser.add_argument("--report", action="store_true", help="Run visualize.py after completion")
    parser.add_argument("--max-tool-calls", type=int, default=60, help="Max tool calls per game (default: 60)")
    args = parser.parse_args()

    # Resolve MCP binary path.
    mcp_binary = Path(args.mcp_binary)
    if not mcp_binary.exists():
        # Try relative to project root.
        project_root = Path(__file__).parent.parent
        mcp_binary = project_root / args.mcp_binary
    if not mcp_binary.exists():
        print(f"ERROR: MCP server binary not found at {args.mcp_binary}", file=sys.stderr)
        print("Build it first: cargo build --release --bin mcp_server", file=sys.stderr)
        sys.exit(1)

    # Initialize.
    api_client = anthropic.Anthropic()
    client = McpClient(str(mcp_binary))
    all_analytics = []
    start_seed = args.seed if args.seed is not None else int(time.time()) % 100000

    print(f"LLM Playtest: {args.games} games, model={args.model}, start_seed={start_seed}")
    print(f"Output: {args.output}")
    print()

    try:
        client.start()
        print("MCP server started successfully.")

        for i in range(args.games):
            seed = start_seed + i
            print(f"  Game {i + 1}/{args.games} (seed={seed})...", end=" ", flush=True)
            game_start = time.time()

            try:
                analytics = play_game(
                    client, api_client, args.model, seed, args.max_tool_calls,
                )
                elapsed = time.time() - game_start
                all_analytics.append(analytics)

                status = "DIED" if analytics["game_over"] else "SURVIVED"
                kills = sum(analytics["kills_by_type"].values())
                hp = analytics["final_hp"]
                explored = analytics["explored_pct"]
                calls = analytics["llm_metrics"]["tool_calls"]
                notes = analytics["llm_metrics"]["strategy_notes"]
                print(
                    f"{status} | HP={hp} kills={kills} explored={explored}% "
                    f"calls={calls} ({elapsed:.1f}s)"
                )
                if notes:
                    print(f"    \u2192 {notes}")

                # Write intermediate results for crash safety.
                meta = {
                    "model": args.model,
                    "start_seed": start_seed,
                    "games_requested": args.games,
                    "games_completed": len(all_analytics),
                    "timestamp": time.strftime("%Y-%m-%dT%H:%M:%S"),
                }
                pa.write_results(args.output, all_analytics, meta)

            except Exception as e:
                print(f"ERROR: {e}")
                # Try to restart the MCP server for next game.
                try:
                    client.stop()
                except Exception:
                    pass
                client = McpClient(str(mcp_binary))
                client.start()
                continue

    except KeyboardInterrupt:
        print("\nInterrupted.")
    finally:
        client.stop()

    if not all_analytics:
        print("No games completed.")
        sys.exit(1)

    # Final summary.
    batch_stats = pa.aggregate(all_analytics)
    print()
    print(f"=== Results ({len(all_analytics)} games) ===")
    print(f"  Win rate:     {batch_stats['win_rate'] * 100:.1f}%")
    print(f"  Avg kills:    {batch_stats['avg_kills']:.1f}")
    print(f"  Avg HP:       {batch_stats['avg_hp_remaining']:.1f}")
    print(f"  Avg explored: {batch_stats['avg_explored_pct']:.1f}%")
    print()
    for i, g in enumerate(all_analytics):
        status = "DIED" if g["game_over"] else "SURVIVED"
        kills = sum(g["kills_by_type"].values())
        notes = g["llm_metrics"].get("strategy_notes", "")
        print(f"  Game {i + 1} (seed={g['seed']}): {status} | HP={g['final_hp']} kills={kills} explored={g['explored_pct']}%")
        if notes:
            print(f"    \u2192 {notes}")
    print()
    print(f"  Results saved to {args.output}")

    # Optional: run visualize.py.
    if args.report:
        print("\nGenerating charts...")
        try:
            with open(args.output) as f:
                data = json.load(f)
            batch_json = json.dumps(data["batch_stats"])
            subprocess.run(
                [sys.executable, "tools/visualize.py", "batch"],
                input=batch_json,
                text=True,
                cwd=str(Path(__file__).parent.parent),
            )
        except Exception as e:
            print(f"Chart generation failed: {e}", file=sys.stderr)


if __name__ == "__main__":
    main()
