#!/usr/bin/env python3
"""LLM-driven roguelike playtesting with dual backends.

Supports two backends:
  - api:         Anthropic API with a local MCP server subprocess (requires SDK)
  - claude-code: Claude Code CLI with --mcp-config (requires `claude` in PATH)

Both backends produce identical EnhancedBatchStats-compatible JSON for use with
tools/visualize.py and the headless runner's --report flag.  Games can be run
in parallel via --parallel N (especially useful with the claude-code backend).

Usage:
    # Anthropic API (default):
    ANTHROPIC_API_KEY=... python3 tools/llm_playtest.py -n 50

    # Claude Code backend:
    python3 tools/llm_playtest.py -n 10 --backend claude-code

    # Parallel execution (4 concurrent games):
    python3 tools/llm_playtest.py -n 10 --backend claude-code --parallel 4
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

# Allow importing playtest_analytics from the same directory.
sys.path.insert(0, str(Path(__file__).parent))
import playtest_analytics as pa

try:
    import anthropic
    HAS_ANTHROPIC = True
except ImportError:
    HAS_ANTHROPIC = False


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
                "compact": {"type": "boolean", "description": "Omit ASCII map from responses"},
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
You are playtesting a roguelike dungeon crawler. Your goal: survive and explore \
as much of the dungeon as possible.

## Combat Math
Before engaging any monster, reason about the fight:
- Your damage per round = your ATK - monster DEF
- Monster damage per round = monster ATK - your DEF
- Rounds to kill monster = ceil(monster HP / your damage)
- Rounds until you die = ceil(your HP / monster damage)
- If rounds-to-kill >= rounds-to-die, you WILL die. Retreat immediately.

## Strategies (choose based on situation)
- **Aggressive**: Engage when you clearly win the damage race and have HP buffer. \
Good for: Goblins, Orcs at high HP.
- **Cautious**: Only fight when rounds-to-kill is well under rounds-to-die. \
Retreat from anything close. Good for: mid-HP, unknown threats.
- **Exploratory**: Prioritize map coverage. Avoid fights unless forced or trivial. \
Good for: low HP, after tough fights, late-game cleanup.

Adapt your strategy fluidly — reassess after every encounter.

## Recovering HP
You regenerate 1 HP every 3 turns of movement. Two ways to regen safely:
- **Corridor running**: When a monster chases you in a straight line, it stays \
1 tile behind and never attacks — you take zero damage while moving. Just keep \
running and you regen. Good for: healing up with a troll on your tail.
- **Corner kiting**: Hit a tough monster, duck around a corner to break line of \
sight (monsters freeze when outside your FOV radius of 8). Regen, come back, \
repeat. This can kill even trolls: hit for 2, retreat, regen 4 HP over 12 turns, \
repeat 10x. Good for: rooms and junctions where corners are available.

## Tools & Flow
1. Call new_game to start (seed provided in first message, always pass compact=true)
2. Use auto_explore for all movement (maximizes tiles per call)
3. Use auto_fight (via act) for fights you commit to
4. To retreat: pathfind_to a tile away from the monster, then auto_explore to regen
5. Game ends when game_over=true or no frontiers remain

Keep responses extremely brief — 1 sentence: your action and reasoning.
When the game ends, write 1-2 sentence strategy_notes summarizing key decisions.\
"""


# ---------------------------------------------------------------------------
# Game loop
# ---------------------------------------------------------------------------

def _strip_maps_from_message(msg):
    """Remove 'map' from tool_result JSON blocks in a single user message.

    Called incrementally: when new tool results are appended to the conversation,
    the previous user message gets its map data stripped.  This keeps only the
    latest tool results' maps intact while avoiding O(n) re-scanning of all
    messages each iteration.
    """
    content = msg.get("content")
    if not isinstance(content, list):
        return
    for block in content:
        if not isinstance(block, dict) or block.get("type") != "tool_result":
            continue
        raw = block.get("content", "")
        if not isinstance(raw, str) or '"map"' not in raw:
            continue
        try:
            data = json.loads(raw)
            if isinstance(data, dict) and "map" in data:
                del data["map"]
                block["content"] = json.dumps(data)
        except (json.JSONDecodeError, TypeError):
            pass


def play_game_api(mcp_binary, model, seed, max_tool_calls=60):
    """Play a single game via the Anthropic API tool_use loop.

    Self-contained: creates its own McpClient and anthropic.Anthropic client.
    Returns the per-game analytics dict.
    """
    analytics = pa.new_game_analytics(seed)
    analytics["llm_metrics"]["model"] = model

    client = McpClient(str(mcp_binary))
    try:
        client.start()
        api_client = anthropic.Anthropic()

        messages = [
            {"role": "user", "content": f"Play a new game with seed {seed}. Start by calling new_game with that seed."},
        ]

        tool_calls = 0
        last_observation = {}
        stall_count = 0
        last_kills = 0
        last_explored = 0
        total_input_tokens = 0
        total_output_tokens = 0

        last_tool_results_msg = None

        while tool_calls < max_tool_calls:
            response = api_client.messages.create(
                model=model,
                max_tokens=1024,
                system=SYSTEM_PROMPT,
                tools=TOOL_SCHEMAS,
                messages=messages,
            )

            # Accumulate token usage.
            if hasattr(response, "usage") and response.usage:
                total_input_tokens += getattr(response.usage, "input_tokens", 0)
                total_output_tokens += getattr(response.usage, "output_tokens", 0)

            # Process the response content blocks.
            assistant_content = response.content
            messages.append({"role": "assistant", "content": assistant_content})

            # Check for stop_reason — if end_turn with no tool_use, game is done.
            if response.stop_reason == "end_turn":
                break

            # Capture text blocks as strategy notes.
            for block in assistant_content:
                if block.type == "text" and block.text.strip():
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

                    current_kills = result.get("kills", last_kills)
                    # MCP responses use short key "explored"; analytics dicts use "explored_pct".
                    current_explored = result.get("explored", last_explored)

                    if current_kills == last_kills and current_explored == last_explored:
                        stall_count += 1
                    else:
                        stall_count = 0
                    last_kills = current_kills
                    last_explored = current_explored

                    if stall_count >= 10:
                        break

                    if result.get("game_over", False):
                        tool_results.append({
                            "type": "tool_result",
                            "tool_use_id": block.id,
                            "content": json.dumps(result),
                        })
                        break

                tool_results.append({
                    "type": "tool_result",
                    "tool_use_id": block.id,
                    "content": json.dumps(result) if isinstance(result, dict) else str(result),
                })

            if not tool_results:
                break

            # Strip map data from previous tool results before adding new ones.
            # Only the latest tool results retain map data to save context tokens.
            if last_tool_results_msg is not None:
                _strip_maps_from_message(last_tool_results_msg)
            user_msg = {"role": "user", "content": tool_results}
            messages.append(user_msg)
            last_tool_results_msg = user_msg

            if last_observation.get("game_over", False):
                break

            if stall_count >= 10:
                break

        # Finalize analytics.
        analytics["llm_metrics"]["token_usage"] = {
            "input_tokens": total_input_tokens,
            "output_tokens": total_output_tokens,
        }
        analytics["turns"] = last_observation.get("kills", 0)  # rough proxy
        pa.finalize_game(analytics, last_observation)

    except Exception as e:
        analytics["error"] = str(e)
        analytics["game_over"] = True
        analytics["llm_metrics"]["strategy_notes"] = f"Error: {e}"
    finally:
        try:
            client.stop()
        except Exception:
            pass

    return analytics


# ---------------------------------------------------------------------------
# Claude Code backend
# ---------------------------------------------------------------------------

_ALLOWED_MCP_TOOLS = [
    "mcp__roguelike__new_game",
    "mcp__roguelike__act",
    "mcp__roguelike__auto_explore",
    "mcp__roguelike__pathfind_to",
    "mcp__roguelike__observe",
    "mcp__roguelike__get_explored_map",
    "mcp__roguelike__get_rules",
]


def play_game_claude_code(mcp_config_path, seed, max_budget=2.00):
    """Play a single game via the Claude Code CLI.

    Spawns `claude -p` as a subprocess with MCP config, parses the JSON
    output array for analytics.  Returns the per-game analytics dict.
    """
    analytics = pa.new_game_analytics(seed)
    analytics["llm_metrics"]["model"] = "claude-code"

    user_prompt = (
        f"Play a new game with seed {seed}. "
        f"Start by calling new_game with that seed."
    )

    cmd = [
        "claude", "-p",
        "--output-format", "json",
        "--system-prompt", SYSTEM_PROMPT,
        "--mcp-config", str(mcp_config_path),
        "--strict-mcp-config",
        "--dangerously-skip-permissions",
        "--no-session-persistence",
        "--max-turns", "200",
        "--max-budget-usd", str(max_budget),
        "--allowedTools", ",".join(_ALLOWED_MCP_TOOLS),
    ]

    # Strip CLAUDECODE env var — nested claude refuses to start otherwise.
    env = {k: v for k, v in os.environ.items() if k != "CLAUDECODE"}

    try:
        proc = subprocess.run(
            cmd,
            input=user_prompt,
            capture_output=True,
            text=True,
            timeout=600,
            env=env,
        )
        if proc.returncode != 0 and not proc.stdout.strip():
            stderr_snippet = (proc.stderr or "")[:500]
            analytics["error"] = f"claude exited {proc.returncode}: {stderr_snippet}"
            analytics["game_over"] = True
            analytics["llm_metrics"]["strategy_notes"] = analytics["error"]
        else:
            _parse_claude_code_output(proc.stdout, analytics)
    except subprocess.TimeoutExpired:
        analytics["error"] = "timeout"
        analytics["game_over"] = True
        analytics["llm_metrics"]["strategy_notes"] = "Game timed out (600s)"
    except Exception as e:
        analytics["error"] = str(e)
        analytics["game_over"] = True
        analytics["llm_metrics"]["strategy_notes"] = f"Error: {e}"

    return analytics


def _parse_claude_code_output(raw_output, analytics):
    """Parse JSON output from ``claude -p --output-format json``.

    The output is a JSON array of message objects, each with a ``type``
    field (system, assistant, user, result).  We extract tool_use blocks
    (for counting), tool_result content (for game observations and fight
    analytics), and text blocks (for strategy notes).
    """
    if not raw_output or not raw_output.strip():
        analytics["error"] = "empty output from claude"
        analytics["game_over"] = True
        return

    try:
        messages = json.loads(raw_output)
    except json.JSONDecodeError as e:
        analytics["error"] = f"JSON parse error: {e}"
        analytics["game_over"] = True
        return

    if not isinstance(messages, list):
        # Might be a single result object — wrap it.
        messages = [messages]

    last_observation = {}
    tool_calls = 0

    for msg in messages:
        if not isinstance(msg, dict):
            continue
        msg_type = msg.get("type", "")

        # Assistant messages contain tool_use and text blocks.
        if msg_type == "assistant":
            content = msg.get("message", {}).get("content", [])
            if not isinstance(content, list):
                continue
            for block in content:
                block_type = block.get("type", "")

                if block_type == "tool_use":
                    tool_calls += 1
                    tool_name = block.get("name", "")
                    # Strip mcp__roguelike__ prefix for matching.
                    short_name = tool_name.replace("mcp__roguelike__", "")
                    tool_input = block.get("input", {})

                    if short_name == "auto_explore":
                        analytics["llm_metrics"]["auto_explore_calls"] += 1
                    elif short_name == "act" and tool_input.get("action") == "auto_fight":
                        pass  # Counted via update_fight_analytics below.
                    elif short_name not in ("new_game", "get_rules", "observe",
                                            "get_explored_map"):
                        analytics["llm_metrics"]["decision_count"] += 1

                elif block_type == "text":
                    text = block.get("text", "").strip()
                    if text:
                        analytics["llm_metrics"]["strategy_notes"] = text

        # User messages contain tool_result blocks.
        elif msg_type == "user":
            content = msg.get("message", {}).get("content", [])
            if not isinstance(content, list):
                continue
            for block in content:
                if not isinstance(block, dict):
                    continue
                if block.get("type") != "tool_result":
                    continue
                raw_content = block.get("content", "")
                # tool_result content may be a string or a list of content blocks.
                text_content = ""
                if isinstance(raw_content, str):
                    text_content = raw_content
                elif isinstance(raw_content, list):
                    for item in raw_content:
                        if isinstance(item, dict) and item.get("type") == "text":
                            text_content = item.get("text", "")
                            break

                if not text_content:
                    continue
                try:
                    obs = json.loads(text_content)
                except (json.JSONDecodeError, TypeError):
                    continue

                if not isinstance(obs, dict):
                    continue

                last_observation = obs

                # Detect auto_fight results by the fight_target field.
                if "fight_target" in obs:
                    pa.update_fight_analytics(analytics, obs)

        # Result message — extract cost and token info.
        elif msg_type == "result":
            cost = msg.get("total_cost_usd")
            if cost is not None:
                analytics["llm_metrics"]["cost_usd"] = cost
            usage = msg.get("usage", {})
            if usage:
                analytics["llm_metrics"]["token_usage"] = {
                    "input_tokens": usage.get("input_tokens", 0),
                    "output_tokens": usage.get("output_tokens", 0),
                    "cache_creation_input_tokens": usage.get("cache_creation_input_tokens", 0),
                    "cache_read_input_tokens": usage.get("cache_read_input_tokens", 0),
                }

    analytics["llm_metrics"]["tool_calls"] = tool_calls
    analytics["turns"] = last_observation.get("kills", 0)
    pa.finalize_game(analytics, last_observation)


# ---------------------------------------------------------------------------
# Batch runner
# ---------------------------------------------------------------------------

def _run_batch(play_fn, game_args_list, output_path, meta, parallel=1):
    """Run a batch of games, writing incremental results.

    play_fn: callable that takes **kwargs and returns analytics dict
    game_args_list: list of kwarg dicts for play_fn
    output_path: path to write JSON results
    meta: metadata dict for results file
    parallel: number of concurrent games (1 = sequential)
    """
    all_analytics = []
    total = len(game_args_list)

    def _run_one(idx, kwargs):
        game_start = time.time()
        analytics = play_fn(**kwargs)
        elapsed = time.time() - game_start
        return idx, analytics, elapsed

    def _report(idx, analytics, elapsed):
        status = "DIED" if analytics["game_over"] else "SURVIVED"
        kills = sum(analytics["kills_by_type"].values())
        hp = analytics["final_hp"]
        explored = analytics["explored_pct"]
        calls = analytics["llm_metrics"]["tool_calls"]
        notes = analytics["llm_metrics"]["strategy_notes"]
        error = analytics.get("error", "")
        if error:
            print(f"  Game {idx + 1}/{total} (seed={analytics['seed']}): "
                  f"ERROR ({elapsed:.1f}s) — {error}")
        else:
            token_info = ""
            tu = analytics["llm_metrics"].get("token_usage", {})
            if tu:
                tok_in = tu.get("input_tokens", 0) + tu.get("cache_creation_input_tokens", 0) + tu.get("cache_read_input_tokens", 0)
                tok_out = tu.get("output_tokens", 0)
                token_info = f" tokens={tok_in + tok_out:,}"
            print(f"  Game {idx + 1}/{total} (seed={analytics['seed']}): "
                  f"{status} | HP={hp} kills={kills} explored={explored}% "
                  f"calls={calls}{token_info} ({elapsed:.1f}s)")
            if notes:
                print(f"    \u2192 {notes}")

    try:
        if parallel <= 1:
            # Sequential — backward-compatible output.
            for idx, kwargs in enumerate(game_args_list):
                _, analytics, elapsed = _run_one(idx, kwargs)
                all_analytics.append(analytics)
                _report(idx, analytics, elapsed)
                meta["games_completed"] = len(all_analytics)
                pa.write_results(output_path, all_analytics, meta)
        else:
            # Parallel execution.
            with ThreadPoolExecutor(max_workers=parallel) as executor:
                futures = {
                    executor.submit(_run_one, idx, kwargs): idx
                    for idx, kwargs in enumerate(game_args_list)
                }
                try:
                    for future in as_completed(futures):
                        idx, analytics, elapsed = future.result()
                        all_analytics.append(analytics)
                        _report(idx, analytics, elapsed)
                        meta["games_completed"] = len(all_analytics)
                        pa.write_results(output_path, all_analytics, meta)
                except KeyboardInterrupt:
                    print("\nInterrupted — cancelling remaining games...")
                    executor.shutdown(wait=False, cancel_futures=True)
    except KeyboardInterrupt:
        print("\nInterrupted.")

    return all_analytics


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def _resolve_mcp_binary(path_str):
    """Resolve the MCP server binary path, trying relative to project root."""
    path = Path(path_str)
    if path.exists():
        return path
    project_root = Path(__file__).parent.parent
    path = project_root / path_str
    if path.exists():
        return path
    return None


def _resolve_mcp_config(config_path=None):
    """Resolve the MCP config path for the claude-code backend.

    If config_path is given, use it directly.  Otherwise, search for
    .mcp.json in the project root and its parent.
    """
    if config_path:
        p = Path(config_path)
        if p.exists():
            return p
        print(f"ERROR: MCP config not found at {config_path}", file=sys.stderr)
        sys.exit(1)

    project_root = Path(__file__).parent.parent
    for d in [project_root, project_root.parent]:
        candidate = d / ".mcp.json"
        if candidate.exists():
            return candidate

    # No .mcp.json found — generate a temporary one pointing at the binary.
    mcp_binary = _resolve_mcp_binary("target/release/mcp_server")
    if mcp_binary is None:
        print(
            "ERROR: No .mcp.json found and MCP binary not available.\n"
            "Either create .mcp.json or build: cargo build --release --bin mcp_server",
            file=sys.stderr,
        )
        sys.exit(1)

    import atexit
    import tempfile
    config = {
        "mcpServers": {
            "roguelike": {
                "command": str(mcp_binary.resolve()),
                "args": [],
            }
        }
    }
    tmp = tempfile.NamedTemporaryFile(
        mode="w", suffix=".json", prefix="mcp_config_", delete=False,
    )
    json.dump(config, tmp)
    tmp.close()
    atexit.register(lambda p=tmp.name: os.unlink(p) if os.path.exists(p) else None)
    return Path(tmp.name)


def main():
    parser = argparse.ArgumentParser(
        description="LLM-driven roguelike playtesting",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=(
            "Examples:\n"
            "  # Anthropic API (default):\n"
            "  python3 tools/llm_playtest.py -n 50\n"
            "\n"
            "  # Claude Code backend:\n"
            "  python3 tools/llm_playtest.py -n 10 --backend claude-code\n"
            "\n"
            "  # Parallel execution:\n"
            "  python3 tools/llm_playtest.py -n 10 --backend claude-code --parallel 4\n"
        ),
    )
    parser.add_argument("-n", "--games", type=int, default=10, help="Number of games (default: 10)")
    parser.add_argument("-m", "--model", default="claude-sonnet-4-20250514", help="Anthropic model ID (api backend)")
    parser.add_argument("-s", "--seed", type=int, default=None, help="Starting seed (increments per game)")
    parser.add_argument("-o", "--output", default="tools/output/llm_playtest_results.json", help="Output JSON path")
    parser.add_argument("--backend", choices=["api", "claude-code"], default="api",
                        help="Backend to use (default: api)")
    parser.add_argument("--parallel", type=int, default=1,
                        help="Concurrent games (default: 1)")
    parser.add_argument("--mcp-binary", default="target/release/mcp_server",
                        help="Path to MCP server binary (api backend)")
    parser.add_argument("--mcp-config", default=None,
                        help="MCP config JSON for claude-code (auto-detected if omitted)")
    parser.add_argument("--max-budget", type=float, default=2.00,
                        help="Max USD per game for claude-code (default: 2.00)")
    parser.add_argument("--report", action="store_true", help="Run visualize.py after completion")
    parser.add_argument("--max-tool-calls", type=int, default=60, help="Max tool calls per game (api backend)")
    args = parser.parse_args()

    # Validate backend-specific requirements.
    if args.backend == "api":
        if not HAS_ANTHROPIC:
            print(
                "ERROR: anthropic SDK is required for the api backend.\n"
                "  pip install anthropic>=0.40\n"
                "Or use --backend claude-code instead.",
                file=sys.stderr,
            )
            sys.exit(1)
        mcp_binary = _resolve_mcp_binary(args.mcp_binary)
        if mcp_binary is None:
            print(f"ERROR: MCP server binary not found at {args.mcp_binary}", file=sys.stderr)
            print("Build it first: cargo build --release --bin mcp_server", file=sys.stderr)
            sys.exit(1)
    else:
        if not shutil.which("claude"):
            print(
                "ERROR: 'claude' CLI not found in PATH.\n"
                "Install Claude Code: https://docs.anthropic.com/en/docs/claude-code",
                file=sys.stderr,
            )
            sys.exit(1)
        mcp_config = _resolve_mcp_config(args.mcp_config)

    start_seed = args.seed if args.seed is not None else int(time.time()) % 100000

    backend_label = args.backend
    if args.backend == "api":
        backend_label = f"api ({args.model})"

    print(f"LLM Playtest: {args.games} games, backend={backend_label}, "
          f"start_seed={start_seed}, parallel={args.parallel}")
    print(f"Output: {args.output}")
    print()

    # Build per-game argument lists.
    game_args_list = []
    for i in range(args.games):
        seed = start_seed + i
        if args.backend == "api":
            game_args_list.append({
                "mcp_binary": mcp_binary,
                "model": args.model,
                "seed": seed,
                "max_tool_calls": args.max_tool_calls,
            })
        else:
            game_args_list.append({
                "mcp_config_path": mcp_config,
                "seed": seed,
                "max_budget": args.max_budget,
            })

    play_fn = play_game_api if args.backend == "api" else play_game_claude_code
    meta = {
        "backend": args.backend,
        "model": args.model if args.backend == "api" else "claude-code",
        "start_seed": start_seed,
        "games_requested": args.games,
        "games_completed": 0,
        "parallel": args.parallel,
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%S"),
    }

    all_analytics = _run_batch(
        play_fn, game_args_list, args.output, meta, parallel=args.parallel,
    )

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
        print(f"  Game {i + 1} (seed={g['seed']}): {status} | HP={g['final_hp']} "
              f"kills={kills} explored={g['explored_pct']}%")
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
