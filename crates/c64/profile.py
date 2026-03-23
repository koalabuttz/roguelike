#!/usr/bin/env python3
"""Profile C64 roguelike via VICE remote monitor.

Launches VICE with the text remote monitor, uses VICE's built-in profiler
(profile on/flat/func) for cycle counting, and xdotool for keyboard input
injection into the VICE SDL2 window.

Requires: x64sc (VICE 3.8+), xdotool, a display ($DISPLAY).

Usage:
    python3 profile.py map.txt <prg> [--turns 50] [--mode coarse|builtin|mapgen]
    python3 profile.py map.txt <prg> --seed 42 --runs 5   # median of 5 runs
"""

import argparse
import os
import re
import socket
import statistics
import subprocess
import sys
import tempfile
import time

# PAL: 312 lines × 63 cycles = 19,656 cycles/frame (50 Hz)
PAL_FRAME_CYCLES = 19_656
PAL_CLOCK_HZ = 985_248

# Symbols we care about (matched against the end of the linker symbol name)
GAME_SYMBOLS = {
    "step": "::step",
    "step_inner": "::step_inner",
    "compute_fov": "::compute_fov",
    "generate": "::generate",
    "render_after_step": "::render_after_step",
    "render_map": "::render_map",
    "render_items": "::render_items",
    "render_entities": "::render_entities",
    "render_status_bar": "::render_status_bar",
    "render_messages": "::render_messages",
    "game_loop": "::game_loop",
}

# xdotool key names for game input
# VICE SDL2 uses positional keyboard mapping by default
KEY_RETURN = "Return"
KEY_DOWN = "Down"
KEY_W = "w"
KEY_A = "a"
KEY_S = "s"
KEY_D = "d"
KEY_SPACE = "space"

# Cycle through directions for gameplay turns
MOVE_KEYS = [KEY_W, KEY_S, KEY_A, KEY_D, KEY_W, KEY_D, KEY_S, KEY_A]


def parse_map(path):
    """Parse linker map for function symbols.

    Returns dict of {short_name: (vma_addr, size, full_name)}.
    """
    pattern = re.compile(
        r"^\s+([0-9a-f]+)\s+[0-9a-f]+\s+([0-9a-f]+)\s+\d+\s{2,}(\S.+)$"
    )
    symbols = {}
    with open(path) as f:
        for line in f:
            m = pattern.match(line)
            if not m:
                continue
            vma = int(m.group(1), 16)
            size = int(m.group(2), 16)
            name = m.group(3).strip()
            for short, suffix in GAME_SYMBOLS.items():
                if name.endswith(suffix) and short not in symbols:
                    symbols[short] = (vma, size, name)
    return symbols


def generate_labels_file(symbols, path):
    """Generate a VICE-compatible label file for load_labels.

    Format: al C:xxxx .label_name
    """
    with open(path, "w") as f:
        for short, (vma, size, full) in sorted(symbols.items(), key=lambda x: x[1][0]):
            f.write(f"al C:{vma:04x} .{short}\n")
    return path


# ---------------------------------------------------------------------------
# Text monitor client
# ---------------------------------------------------------------------------

class VICETextMonitor:
    """TCP client for VICE's text remote monitor.

    Per VICE docs: sending any command pauses the emulator.
    Use 'x' (exit) to resume execution.
    Use 'q' (quit) to terminate VICE.
    """

    def __init__(self, host="127.0.0.1", port=6510):
        self.host = host
        self.port = port
        self.sock = None

    def connect(self, retries=30):
        for attempt in range(retries):
            try:
                self.sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
                self.sock.settimeout(15)
                self.sock.connect((self.host, self.port))
                self._read_response(timeout=3)
                return
            except (ConnectionRefusedError, OSError):
                if self.sock:
                    self.sock.close()
                if attempt < retries - 1:
                    time.sleep(0.5)
        raise ConnectionError(f"Cannot connect to text monitor at {self.host}:{self.port}")

    def _read_response(self, timeout=5):
        """Read until VICE prompt (C:$xxxx) appears."""
        self.sock.settimeout(timeout)
        buf = b""
        while True:
            try:
                data = self.sock.recv(4096)
                if not data:
                    break
                buf += data
                if re.search(rb"\(C:\$[0-9a-f]+\)", buf):
                    break
            except socket.timeout:
                break
        return buf.decode("latin-1", errors="replace")

    def send(self, cmd, timeout=10):
        """Send command, wait for response. VICE pauses on any command."""
        self.sock.sendall((cmd + "\n").encode())
        time.sleep(0.1)
        return self._read_response(timeout=timeout)

    def resume(self):
        """Resume execution. 'x' = exit monitor, return to emulation."""
        self.sock.sendall(b"x\n")
        time.sleep(0.1)

    def quit(self):
        """Terminate VICE."""
        try:
            self.sock.sendall(b"q\n")
        except OSError:
            return

    def close(self):
        if self.sock:
            try:
                self.sock.close()
            except OSError:
                return


# ---------------------------------------------------------------------------
# Input injection via xdotool
# ---------------------------------------------------------------------------

class XdotoolInput:
    """Keyboard input injection via xdotool into the VICE SDL2 window."""

    def __init__(self, vice_pid):
        self.vice_pid = vice_pid
        self.window_id = None

    def find_window(self, retries=10):
        """Find the VICE SDL2 window by PID."""
        for attempt in range(retries):
            result = subprocess.run(
                ["xdotool", "search", "--pid", str(self.vice_pid)],
                capture_output=True, text=True,
            )
            wids = result.stdout.strip().split("\n")
            wids = [w for w in wids if w.strip()]
            if wids:
                self.window_id = wids[0]
                return self.window_id
            time.sleep(0.5)
        raise RuntimeError(f"Cannot find VICE window for PID {self.vice_pid}")

    def send_key(self, key_name):
        """Send a single keypress to the VICE window."""
        if not self.window_id:
            raise RuntimeError("No window ID — call find_window() first")
        subprocess.run(
            ["xdotool", "key", "--window", self.window_id, key_name],
            capture_output=True,
        )

    def send_key_down(self, key_name):
        """Press and hold a key."""
        if not self.window_id:
            raise RuntimeError("No window ID")
        subprocess.run(
            ["xdotool", "keydown", "--window", self.window_id, key_name],
            capture_output=True,
        )

    def send_key_up(self, key_name):
        """Release a key."""
        if not self.window_id:
            raise RuntimeError("No window ID")
        subprocess.run(
            ["xdotool", "keyup", "--window", self.window_id, key_name],
            capture_output=True,
        )

    def type_text(self, text, delay_ms=80):
        """Type a string character by character into the VICE window.

        Uses xdotool type which simulates real keyboard input, including
        proper keysym translation for digits and letters.
        """
        if not self.window_id:
            raise RuntimeError("No window ID — call find_window() first")
        subprocess.run(
            ["xdotool", "type", "--window", self.window_id,
             "--delay", str(delay_ms), text],
            capture_output=True,
        )


# ---------------------------------------------------------------------------
# VICE launcher
# ---------------------------------------------------------------------------

def launch_vice(prg_path, text_port=6510):
    """Launch VICE with SDL2 window + text remote monitor.

    NOT using -console: we need a real SDL2 window for xdotool input.
    """
    cmd = [
        "x64sc",
        "-warp",
        "+sound",
        "-remotemonitor",
        "-remotemonitoraddress", f"ip4://127.0.0.1:{text_port}",
        "-autostartprgmode", "1",
        "-autostart", prg_path,
    ]
    return subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def check_prerequisites():
    """Verify xdotool and DISPLAY are available."""
    if not os.environ.get("DISPLAY"):
        print("ERROR: $DISPLAY not set. VICE needs a display for keyboard input.")
        print("  On headless systems, install xvfb and use: xvfb-run python3 profile.py ...")
        sys.exit(1)
    result = subprocess.run(["which", "xdotool"], capture_output=True)
    if result.returncode != 0:
        print("ERROR: xdotool not found. Install with: sudo apt install xdotool")
        sys.exit(1)


# ---------------------------------------------------------------------------
# Navigation helpers
# ---------------------------------------------------------------------------

def navigate_to_gameplay(text_mon, kbd, seed=None, wait_time=5):
    """Navigate from title screen to gameplay.

    If seed is None, sends RETURN to select "New Game" (random CIA seed).
    If seed is a string, navigates to "Enter Seed" and types it (base36).
    """
    # Resume VICE so it can process keyboard events
    text_mon.resume()
    time.sleep(0.5)

    if seed is not None:
        # Navigate to "Enter Seed" (second menu item when no save exists)
        kbd.send_key(KEY_DOWN)
        time.sleep(0.3)
        kbd.send_key(KEY_RETURN)
        time.sleep(1.0)

        # Type seed code using xdotool type (simulates real typing)
        kbd.type_text(seed)
        time.sleep(0.3)

        # Confirm seed entry
        kbd.send_key(KEY_RETURN)
        print(f"  Entered seed: {seed}")
    else:
        # Send RETURN to select "New Game"
        kbd.send_key(KEY_RETURN)

    # Wait for map generation + first render (warp mode: fast)
    time.sleep(wait_time)

    # Pause and verify we're in gameplay
    resp = text_mon.send("screen")
    if "new game" in resp or "enter seed" in resp.lower():
        print("  WARNING: Still on title screen — retrying...")
        text_mon.resume()
        time.sleep(0.5)
        kbd.send_key(KEY_RETURN)
        time.sleep(wait_time)
        resp = text_mon.send("screen")
        if "new game" in resp:
            print("  ERROR: Cannot navigate past title screen")
            return False

    return True


def send_move(text_mon, kbd, key_name):
    """Send a movement key while VICE is running."""
    text_mon.resume()
    time.sleep(0.1)
    kbd.send_key(key_name)
    time.sleep(0.3)


# ---------------------------------------------------------------------------
# Profiling modes
# ---------------------------------------------------------------------------

def parse_flat_profile(resp):
    """Parse labeled symbol cycle counts from VICE flat profile output.

    Returns dict of {symbol_name: (total, self)} for labeled symbols
    (lines containing .symbol_name).
    """
    results = {}
    for line in resp.splitlines():
        # Match lines like: "  12,362,411   6.1%     2,632,927   1.3% .compute_fov"
        m = re.match(
            r"\s+([\d,]+)\s+[\d.]+%\s+([\d,]+)\s+[\d.]+%\s+\.(\w+)",
            line,
        )
        if m:
            total = int(m.group(1).replace(",", ""))
            self_cyc = int(m.group(2).replace(",", ""))
            name = m.group(3)
            results[name] = (total, self_cyc)
    return results


def profile_builtin(text_mon, kbd, symbols, labels_file, n_turns, frame_budget,
                    seed=None, quiet=False):
    """Use VICE's built-in profiler for per-function cycle counts.

    Returns parsed results dict {name: (total, self)} or None on failure.
    """
    resp = text_mon.send(f'load_labels "{labels_file}"')
    if not quiet:
        print(f"  Labels loaded: {len(symbols)} symbols")

    if not quiet:
        print("  Navigating to gameplay...")
    if not navigate_to_gameplay(text_mon, kbd, seed=seed):
        return None

    # Start profiling
    text_mon.send("profile on")
    if not quiet:
        print(f"  Profiling {n_turns} turns...")

    for i in range(n_turns):
        key = MOVE_KEYS[i % len(MOVE_KEYS)]
        send_move(text_mon, kbd, key)

        if not quiet and (i + 1) % 10 == 0:
            print(f"    {i + 1}/{n_turns}...")

    # Pause and stop profiling
    time.sleep(0.5)
    text_mon.send("profile off")

    # Read flat profile
    resp = text_mon.send("profile flat 50", timeout=10)
    results = parse_flat_profile(resp)

    if not quiet:
        print()
        print("=" * 60)
        seed_info = f", seed={seed}" if seed else ", random seed"
        print(f"VICE Built-in Profile: {n_turns} turns (PAL, {frame_budget:,} cyc/frame{seed_info})")
        print("=" * 60)
        # Strip the prompt from the end
        resp_clean = re.sub(r"\(C:\$[0-9a-f]+\)\s*$", "", resp).strip()
        print(resp_clean)

        # Per-function details for key functions
        for sym in ["step_inner", "compute_fov", "render_after_step", "render_map"]:
            if sym in symbols:
                resp = text_mon.send(f"profile func .{sym}", timeout=5)
                resp_clean = re.sub(r"\(C:\$[0-9a-f]+\)\s*$", "", resp).strip()
                if "unknown" not in resp_clean.lower() and len(resp_clean) > 20:
                    print(f"\n--- {sym} ---")
                    print(resp_clean)

    return results


def profile_coarse(text_mon, kbd, symbols, n_turns, frame_budget):
    """Coarse profiling via stopwatch — currently unavailable.

    LTO inlines step_inner and render_after_step into game_loop, making their
    linker map addresses dead code. Only overlay functions (compute_fov,
    generate) survive as real breakpoint targets.
    """
    print("  Coarse mode is not available: LTO inlines step_inner and")
    print("  render_after_step, making their addresses dead code.")
    print("  Use --mode builtin instead (VICE's built-in profiler).")
    print()


def profile_mapgen(text_mon, kbd, symbols, frame_budget, seed=None):
    """Profile map generation cost."""
    gen_addr = symbols.get("generate", (None,))[0]
    step_addr = symbols.get("step_inner", (None,))[0]
    if gen_addr is None:
        print("ERROR: generate symbol not found")
        return

    # compute_fov ($D000) is the first overlay call after generate completes,
    # so use it as the end-of-generation marker. (step_inner is inlined by LTO.)
    fov_addr = symbols.get("compute_fov", (None,))[0]
    if fov_addr is None:
        print("ERROR: compute_fov symbol not found (needed as end marker)")
        return

    text_mon.send(f"break exec ${gen_addr:04x}")
    text_mon.send(f"break exec ${fov_addr:04x}")

    # Navigate to gameplay
    text_mon.resume()
    time.sleep(0.5)
    if seed is not None:
        kbd.send_key(KEY_DOWN)
        time.sleep(0.3)
        kbd.send_key(KEY_RETURN)
        time.sleep(1.0)
        kbd.type_text(seed)
        time.sleep(0.3)
        kbd.send_key(KEY_RETURN)
        print(f"  Entered seed: {seed}")
    else:
        kbd.send_key(KEY_RETURN)

    print("  Waiting for map generation...")

    resp = text_mon._read_response(timeout=30)
    if "Stop on" not in resp:
        print("ERROR: generate breakpoint not hit")
        return

    text_mon.send("stopwatch reset")
    text_mon.resume()

    resp = text_mon._read_response(timeout=120)
    sw = text_mon.send("stopwatch")
    m = re.search(r"Stopwatch:\s+(\d+)", sw)
    if not m:
        print("ERROR: cannot read stopwatch after generation")
        return

    gen_cycles = int(m.group(1))
    frames = gen_cycles / frame_budget
    ms = (gen_cycles / PAL_CLOCK_HZ) * 1000

    print()
    print("=" * 55)
    print("C64 Map Generation Profile (PAL)")
    print("=" * 55)
    print(f"\n  generate -> compute_fov: {gen_cycles:>10,} cycles")
    print(f"                           {frames:>10.1f} frames ({ms:.0f} ms)")
    print(f"                           ~{gen_cycles / PAL_FRAME_CYCLES:.0f}x frame budget")
    print()

    text_mon.send("del")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(
        description="Profile C64 roguelike via VICE remote monitor"
    )
    parser.add_argument("map", help="Path to linker map (map.txt)")
    parser.add_argument("prg", help="Path to PRG binary")
    parser.add_argument("--port", type=int, default=6510,
                        help="VICE text monitor port (default: 6510)")
    parser.add_argument("--turns", type=int, default=50,
                        help="Number of turns to profile (default: 50)")
    parser.add_argument("--mode", choices=["coarse", "builtin", "mapgen"],
                        default="builtin",
                        help="Profiling mode (default: builtin)")
    parser.add_argument("--seed", type=str, default=None,
                        help="Base36 seed code for reproducible dungeons (default: random)")
    parser.add_argument("--runs", type=int, default=1,
                        help="Number of runs; reports median when >1 (default: 1)")
    args = parser.parse_args()

    check_prerequisites()

    # Parse symbols
    symbols = parse_map(args.map)
    print(f"Parsed {len(symbols)} symbols from {args.map}:")
    for name, (addr, size, full) in sorted(symbols.items(), key=lambda x: x[1][0]):
        print(f"  {name:20s}  ${addr:04x}  ({size:,} B)")

    required = {"step_inner", "render_after_step"}
    if args.mode == "mapgen":
        required = {"generate"}
    missing = required - symbols.keys()
    if missing:
        print(f"\nERROR: missing required symbols: {missing}")
        sys.exit(1)

    if not os.path.exists(args.prg):
        print(f"\nERROR: PRG not found: {args.prg}")
        sys.exit(1)

    prg_path = os.path.abspath(args.prg)

    # Generate VICE labels file
    labels_file = os.path.join(tempfile.gettempdir(), "roguelike-vice-labels.txt")
    generate_labels_file(symbols, labels_file)

    frame_budget = PAL_FRAME_CYCLES

    if args.runs > 1 and args.mode == "builtin":
        run_multi(args, symbols, labels_file, prg_path, frame_budget)
    else:
        run_single(args, symbols, labels_file, prg_path, frame_budget)

    try:
        os.unlink(labels_file)
    except OSError:
        pass


def run_vice_session(prg_path, port, fn):
    """Launch VICE, connect, find window, call fn(text_mon, kbd), shut down.

    fn receives (text_mon, kbd) and should return a value.
    """
    vice = launch_vice(prg_path, port)
    text_mon = VICETextMonitor(port=port)
    kbd = XdotoolInput(vice.pid)
    result = None

    try:
        time.sleep(4)
        text_mon.connect()
        kbd.find_window()
        result = fn(text_mon, kbd)
    finally:
        text_mon.quit()
        text_mon.close()
        try:
            vice.wait(timeout=5)
        except subprocess.TimeoutExpired:
            vice.kill()
            vice.wait()

    return result


def run_single(args, symbols, labels_file, prg_path, frame_budget):
    """Single profiling run (original behavior)."""
    print(f"\nLaunching VICE (warp, port {args.port})...")
    vice = launch_vice(prg_path, args.port)

    text_mon = VICETextMonitor(port=args.port)
    kbd = XdotoolInput(vice.pid)

    try:
        time.sleep(4)
        text_mon.connect()
        print("Connected to text monitor")
        kbd.find_window()
        print(f"Found VICE window: {kbd.window_id}\n")

        if args.mode == "builtin":
            profile_builtin(text_mon, kbd, symbols, labels_file,
                            args.turns, frame_budget, seed=args.seed)
        elif args.mode == "coarse":
            profile_coarse(text_mon, kbd, symbols, args.turns, frame_budget)
        elif args.mode == "mapgen":
            profile_mapgen(text_mon, kbd, symbols, frame_budget, seed=args.seed)

    except KeyboardInterrupt:
        print("\nInterrupted")
    except Exception as e:
        print(f"\nERROR: {e}")
        raise
    finally:
        print("Shutting down VICE...")
        text_mon.quit()
        text_mon.close()
        try:
            vice.wait(timeout=5)
        except subprocess.TimeoutExpired:
            vice.kill()
            vice.wait()
        print("Done.")


def run_multi(args, symbols, labels_file, prg_path, frame_budget):
    """Run profiler multiple times and report median/spread."""
    n_runs = args.runs
    seed_info = f"seed={args.seed}" if args.seed else "random seed"
    print(f"\nMulti-run profile: {n_runs} runs, {args.turns} turns each, {seed_info}")
    print()

    all_results = []

    for run_idx in range(n_runs):
        print(f"--- Run {run_idx + 1}/{n_runs} ---")
        print(f"  Launching VICE (warp, port {args.port})...")

        def do_profile(text_mon, kbd):
            return profile_builtin(
                text_mon, kbd, symbols, labels_file,
                args.turns, frame_budget, seed=args.seed, quiet=True,
            )

        result = run_vice_session(prg_path, args.port, do_profile)

        if result:
            all_results.append(result)
            # Show brief per-run summary
            fov = result.get("compute_fov", (0, 0))[0]
            render = result.get("render_after_step", (0, 0))[0]
            step = result.get("step", (0, 0))[0]
            print(f"  compute_fov={fov:,}  render={render:,}  step={step:,}")
        else:
            print("  FAILED — skipping")
        print()

    if len(all_results) < 2:
        print("ERROR: Need at least 2 successful runs for statistics")
        return

    # Collect all symbol names across runs
    all_names = set()
    for r in all_results:
        all_names.update(r.keys())

    # Compute statistics for each symbol
    KEY_SYMBOLS = ["game_loop", "step", "compute_fov", "render_after_step",
                   "render_map", "render_entities", "render_items",
                   "render_status_bar", "render_messages"]
    display_names = [n for n in KEY_SYMBOLS if n in all_names]

    print("=" * 72)
    print(f"Median of {len(all_results)} runs ({args.turns} turns each, {seed_info})")
    print("=" * 72)
    print(f"  {'Function':25s} {'Median':>12s} {'Min':>12s} {'Max':>12s}  {'Spread':>6s}")
    print("-" * 72)

    for name in display_names:
        totals = [r[name][0] for r in all_results if name in r]
        if not totals:
            continue
        med = int(statistics.median(totals))
        lo = min(totals)
        hi = max(totals)
        spread = ((hi - lo) / med * 100) if med > 0 else 0
        print(f"  {name:25s} {med:12,} {lo:12,} {hi:12,}  ±{spread:.1f}%")

    print()
    print("Use these medians for A/B comparisons between builds.")


if __name__ == "__main__":
    main()
