#!/usr/bin/env python3
"""MCP client that plays the roguelike with combat math and exploration strategy."""
import json
import math
import subprocess
import sys
import time
import threading

# Monster stats from game.toml (not provided in entity observations)
MONSTER_STATS = {
    "Goblin": {"atk": 3, "def": 0},
    "Orc":    {"atk": 4, "def": 1},
    "Troll":  {"atk": 6, "def": 3},
}

class McpClient:
    def __init__(self, cmd):
        self.proc = subprocess.Popen(
            cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE
        )
        self.req_id = 0
        self.stderr_lines = []
        self._stderr_thread = threading.Thread(target=self._read_stderr, daemon=True)
        self._stderr_thread.start()
        time.sleep(0.3)

    def _read_stderr(self):
        for line in self.proc.stderr:
            self.stderr_lines.append(line.decode().rstrip())

    def _send(self, msg):
        data = json.dumps(msg) + "\n"
        self.proc.stdin.write(data.encode())
        self.proc.stdin.flush()

    def _recv(self):
        line = self.proc.stdout.readline().decode().strip()
        if not line:
            return None
        return json.loads(line)

    def initialize(self):
        self.req_id += 1
        self._send({
            "jsonrpc": "2.0", "id": self.req_id, "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05", "capabilities": {},
                "clientInfo": {"name": "playtest-client", "version": "0.1.0"}
            }
        })
        resp = self._recv()
        self._send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        return resp

    def call_tool(self, name, arguments=None):
        self.req_id += 1
        self._send({
            "jsonrpc": "2.0", "id": self.req_id, "method": "tools/call",
            "params": {"name": name, "arguments": arguments or {}}
        })
        resp = self._recv()
        if resp and "result" in resp:
            content = resp["result"].get("content", [])
            if content and content[0].get("type") == "text":
                return json.loads(content[0]["text"])
        if resp and "error" in resp:
            return {"error": resp["error"]}
        return resp

    def close(self):
        try:
            self.proc.stdin.close()
            self.proc.wait(timeout=5)
        except:
            self.proc.kill()


def combat_analysis(player_atk, player_def, player_hp, monster):
    """Return (should_fight, analysis_str) using known monster stats."""
    m_name = monster.get("name", "?")
    m_hp = monster.get("hp", 0)
    stats = MONSTER_STATS.get(m_name, {"atk": 4, "def": 1})  # default to Orc-like
    m_atk = stats["atk"]
    m_def = stats["def"]

    my_dmg = max(1, player_atk - m_def)
    m_dmg = max(0, m_atk - player_def)

    rounds_to_kill = math.ceil(m_hp / my_dmg) if my_dmg > 0 else 999
    rounds_to_die = math.ceil(player_hp / m_dmg) if m_dmg > 0 else 999

    hp_cost = rounds_to_kill * m_dmg
    analysis = (f"{m_name} HP={m_hp} ATK={m_atk} DEF={m_def} | "
                f"I deal {my_dmg}/rnd, it deals {m_dmg}/rnd | "
                f"Kill in {rounds_to_kill} rnds, I die in {rounds_to_die} rnds | "
                f"HP cost~{hp_cost}")

    if m_dmg == 0:
        return True, analysis + " → SAFE"
    if rounds_to_kill < rounds_to_die and (player_hp - hp_cost) > 3:
        return True, analysis + f" → FIGHT (HP after~{player_hp - hp_cost})"
    if rounds_to_kill < rounds_to_die:
        return True, analysis + f" → RISKY FIGHT (HP after~{player_hp - hp_cost})"
    return False, analysis + " → RETREAT (would die)"


def get_retreat_direction(px, py, mx, my):
    """Pick a cardinal direction away from the monster."""
    dx = px - mx
    dy = py - my
    if dx == 0 and dy == 0:
        return "move_north"
    if abs(dx) >= abs(dy):
        return "move_east" if dx > 0 else "move_west"
    return "move_south" if dy > 0 else "move_north"


def update_player(obs, state):
    """Update player state from an observation."""
    state["hp"] = obs.get("hp", state["hp"])
    state["max_hp"] = obs.get("max_hp", state["max_hp"])
    state["atk"] = obs.get("atk", state["atk"])
    state["def"] = obs.get("def", state["def"])
    state["explored"] = obs.get("explored", state["explored"])
    state["steps"] = obs.get("steps", state["steps"])
    state["game_over"] = obs.get("game_over", False)
    state["x"] = obs.get("x", state.get("x", 0))
    state["y"] = obs.get("y", state.get("y", 0))


def print_messages(obs):
    for msg in obs.get("messages", []):
        if isinstance(msg, str) and msg:
            print(f"  > {msg}")


def play_game(seed):
    client = McpClient(["./target/debug/mcp_server"])
    if client.proc.poll() is not None:
        print("Server failed to start")
        return

    client.initialize()

    # Start game
    obs = client.call_tool("new_game", {"seed": seed, "compact": True})
    state = {"hp": 30, "max_hp": 30, "atk": 5, "def": 2, "explored": 0, "steps": 0,
             "game_over": False, "x": 0, "y": 0}
    update_player(obs, state)

    print(f"=== GAME START seed={seed} ===")
    print(f"Player: HP={state['hp']}/{state['max_hp']} ATK={state['atk']} DEF={state['def']}")

    turn = 0
    max_turns = 300
    kills = 0
    retreats = 0
    kite_tracker = {}  # name -> retreat count, to detect chase monsters

    while not state["game_over"] and turn < max_turns:
        turn += 1

        # Auto-explore
        obs = client.call_tool("auto_explore")
        if obs is None or "error" in obs:
            err = obs.get("error", {}).get("message", str(obs)) if obs else "None"
            print(f"[{turn}] auto_explore error: {err}")
            # If no frontiers, we're done
            if "No reachable frontier" in str(err):
                print(f"\n=== FULLY EXPLORED ===")
            break

        update_player(obs, state)
        stop_reason = obs.get("stop_reason", "")
        entities = obs.get("entities", [])
        frontiers = obs.get("frontiers", 0)
        new_tiles = obs.get("new_tiles", 0)

        print_messages(obs)
        print(f"[{turn}] HP={state['hp']}/{state['max_hp']} explored={state['explored']} steps={state['steps']} new={new_tiles} frontiers={frontiers} stop={stop_reason}")

        if state["game_over"]:
            print(f"\n=== GAME OVER at step {state['steps']} ===")
            break

        if frontiers == 0 and stop_reason != "monster_spotted":
            print(f"\n=== FULLY EXPLORED ===")
            break

        # Monster spotted - combat decision
        if stop_reason == "monster_spotted" and entities:
            alive_monsters = [e for e in entities if e.get("alive", True)]
            if not alive_monsters:
                continue

            # Find closest alive monster
            px, py = state["x"], state["y"]
            closest = min(alive_monsters,
                         key=lambda e: abs(e.get("x",0)-px) + abs(e.get("y",0)-py))
            dist = abs(closest.get("x",0)-px) + abs(closest.get("y",0)-py)

            should_fight, analysis = combat_analysis(state["atk"], state["def"], state["hp"], closest)
            print(f"  COMBAT (dist={dist}): {analysis}")

            # Kiting: if we've retreated from this type 2+ times, it's a chaser — kite it
            m_name = closest.get('name', '?')
            if not should_fight:
                kite_tracker[m_name] = kite_tracker.get(m_name, 0) + 1

            if not should_fight and kite_tracker.get(m_name, 0) >= 2:
                # KITE: hit once, run 12 steps to regen, repeat
                stats = MONSTER_STATS.get(m_name, {"atk": 4, "def": 1})
                my_dmg = max(1, state["atk"] - stats["def"])
                m_hp_est = closest.get("hp", 20)
                hits_needed = math.ceil(m_hp_est / my_dmg)
                print(f"  KITING {m_name}! Need {hits_needed} hits of {my_dmg} dmg each")
                for hit_num in range(hits_needed + 2):
                    if state["game_over"]:
                        break
                    # Move into monster to attack (one step)
                    mx, my = closest.get("x", 0), closest.get("y", 0)
                    dx = mx - state["x"]
                    dy = my - state["y"]
                    if abs(dx) >= abs(dy):
                        atk_dir = "move_east" if dx > 0 else "move_west"
                        run_dir = "move_west" if dx > 0 else "move_east"
                    else:
                        atk_dir = "move_south" if dy > 0 else "move_north"
                        run_dir = "move_north" if dy > 0 else "move_south"
                    # Attack
                    a_obs = client.call_tool("act", {"action": atk_dir})
                    if a_obs and "error" not in a_obs:
                        update_player(a_obs, state)
                        print_messages(a_obs)
                        if state["game_over"]:
                            break
                        # Check if monster died
                        killed = any("dead" in str(m) for m in a_obs.get("messages", []))
                        if killed:
                            kills += 1
                            kite_tracker.pop(m_name, None)
                            print(f"  KITE KILLED {m_name}! HP={state['hp']}/{state['max_hp']}")
                            break
                    # Run away 12 steps to regen 4 HP
                    for _ in range(12):
                        r = client.call_tool("act", {"action": run_dir})
                        if r and "error" not in r:
                            update_player(r, state)
                        else:
                            break
                    print(f"  Kite hit {hit_num+1}, HP={state['hp']}/{state['max_hp']}")
                continue

            if should_fight:
                # Walk toward monster step by step until adjacent, then auto_fight
                mx, my = closest.get("x", 0), closest.get("y", 0)
                approach_limit = 20
                for _ in range(approach_limit):
                    px, py = state["x"], state["y"]
                    cdist = abs(mx - px) + abs(my - py)
                    if cdist <= 1:
                        break
                    # Move one step toward monster
                    dx = mx - px
                    dy = my - py
                    if abs(dx) >= abs(dy):
                        action = "move_east" if dx > 0 else "move_west"
                    else:
                        action = "move_south" if dy > 0 else "move_north"
                    step_obs = client.call_tool("act", {"action": action})
                    if step_obs and "error" not in step_obs:
                        update_player(step_obs, state)
                        print_messages(step_obs)
                        if state["game_over"]:
                            break
                        # Update monster position from entities
                        for e in step_obs.get("entities", []):
                            if e.get("name") == closest.get("name") and e.get("alive", True):
                                mx, my = e.get("x", mx), e.get("y", my)
                    else:
                        break

                if not state["game_over"]:
                    # Now try auto_fight
                    fight_obs = client.call_tool("act", {"action": "auto_fight"})
                    if fight_obs and "error" not in fight_obs:
                        update_player(fight_obs, state)
                        print_messages(fight_obs)
                        if fight_obs.get("fight_target_killed", False):
                            kills += 1
                            print(f"  KILLED {closest.get('name','?')}! HP={state['hp']}/{state['max_hp']} lost={fight_obs.get('fight_hp_lost',0)}")
                        if state["game_over"]:
                            print(f"\n=== DIED fighting {closest.get('name','?')} ===")
                            break
                    else:
                        err = fight_obs.get("error", {}).get("message", "") if fight_obs else ""
                        print(f"  auto_fight failed: {err}")
                else:
                    print(f"\n=== DIED approaching {closest.get('name','?')} ===")
                    break
            else:
                # Retreat! Run away for HP regen (1 HP per 3 steps)
                retreats += 1
                m_name = closest.get('name', '?')
                print(f"  RETREATING from {m_name}!")
                mx, my = closest.get("x", 0), closest.get("y", 0)

                # Run 20+ steps away for regen
                retreat_steps = 0
                for _ in range(30):
                    if state["game_over"]:
                        break
                    direction = get_retreat_direction(state["x"], state["y"], mx, my)
                    r_obs = client.call_tool("act", {"action": direction})
                    if r_obs and "error" not in r_obs:
                        update_player(r_obs, state)
                        retreat_steps += 1
                    else:
                        # Try perpendicular
                        for alt in ["move_north", "move_south", "move_east", "move_west"]:
                            if alt != direction:
                                r_obs = client.call_tool("act", {"action": alt})
                                if r_obs and "error" not in r_obs:
                                    update_player(r_obs, state)
                                    retreat_steps += 1
                                    break
                        else:
                            break

                print(f"  Retreated {retreat_steps} steps, HP={state['hp']}/{state['max_hp']}")

    print(f"\nFinal: HP={state['hp']}/{state['max_hp']} ATK={state['atk']} DEF={state['def']}")
    print(f"Steps={state['steps']} Explored={state['explored']} Kills={kills} Retreats={retreats}")
    client.close()


if __name__ == "__main__":
    seed = int(sys.argv[1]) if len(sys.argv) > 1 else 64462
    play_game(seed)
