#!/usr/bin/env python3
"""TUI for monitoring and chatting with running LLM playtest games.

Usage:
    python3 tools/playtest_chat.py          # Launch TUI
    python3 tools/playtest_chat.py --send 123-64x48 "message"   # One-shot send
    python3 tools/playtest_chat.py --all "message"              # Broadcast
"""
import curses
import glob
import json
import os
import sys
import time


FIFO_PATTERN = "/tmp/roguelike-inject-*.fifo"
SPECTATE_PATTERN = "/tmp/roguelike-spectate-*.txt"
LOG_DIR = "tools/output/playtest_logs"


# ---------------------------------------------------------------------------
# Data helpers
# ---------------------------------------------------------------------------

def list_games():
    """Return list of game dicts with seed, fifo_path, spectate_path, log_path."""
    # Find all spectate files (games that have started).
    games = {}
    for path in sorted(glob.glob(SPECTATE_PATTERN)):
        seed = os.path.basename(path).removeprefix("roguelike-spectate-").removesuffix(".txt")
        games[seed] = {
            "seed": seed,
            "spectate_path": path,
            "fifo_path": None,
            "log_path": None,
            "status_line": "",
            "has_fifo": False,
        }

    # Match inject FIFOs.
    for path in glob.glob(FIFO_PATTERN):
        seed = os.path.basename(path).removeprefix("roguelike-inject-").removesuffix(".fifo")
        if seed in games:
            games[seed]["fifo_path"] = path
            games[seed]["has_fifo"] = True
        else:
            games[seed] = {
                "seed": seed,
                "spectate_path": None,
                "fifo_path": path,
                "log_path": None,
                "status_line": "(no spectate file)",
                "has_fifo": True,
            }

    # Match log files.
    if os.path.isdir(LOG_DIR):
        for path in glob.glob(os.path.join(LOG_DIR, "game-*.jsonl")):
            seed = os.path.basename(path).removeprefix("game-").removesuffix(".jsonl")
            if seed in games:
                games[seed]["log_path"] = path

    # Read status lines from spectate files.
    for g in games.values():
        if g["spectate_path"] and os.path.exists(g["spectate_path"]):
            try:
                with open(g["spectate_path"]) as f:
                    for line in f:
                        if line.startswith("HP "):
                            g["status_line"] = line.strip()
            except OSError:
                pass

    return list(games.values())


def read_spectate(path):
    """Read spectate file contents, return list of lines."""
    if not path or not os.path.exists(path):
        return ["(no spectate file)"]
    try:
        with open(path) as f:
            return [l.rstrip() for l in f.readlines()]
    except OSError:
        return ["(error reading spectate file)"]


def read_conversation(path, max_entries=50):
    """Parse jsonl log into displayable conversation entries."""
    if not path or not os.path.exists(path):
        return ["(no log file)"]

    entries = []
    try:
        with open(path) as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    obj = json.loads(line)
                except json.JSONDecodeError:
                    continue

                t = obj.get("type", "")

                if t == "assistant":
                    content = obj.get("message", {}).get("content", [])
                    if not isinstance(content, list):
                        continue
                    for block in content:
                        bt = block.get("type", "")
                        if bt == "text":
                            text = block.get("text", "").strip()
                            if text:
                                # Wrap long text.
                                for paragraph in text.split("\n"):
                                    entries.append(("assistant", paragraph))
                        elif bt == "tool_use":
                            name = block.get("name", "").replace("mcp__roguelike__", "")
                            inp = block.get("input", {})
                            brief = json.dumps(inp) if inp else ""
                            if len(brief) > 60:
                                brief = brief[:57] + "..."
                            entries.append(("tool", f"{name}({brief})"))

                elif t == "user":
                    content = obj.get("message", {}).get("content", "")
                    # Injected user messages are plain strings.
                    if isinstance(content, str) and content and not content.startswith("{"):
                        entries.append(("user_inject", content))

    except OSError:
        entries.append(("error", "(error reading log)"))

    # Return last N entries.
    return entries[-max_entries:] if entries else ["(empty conversation)"]


def wrap_text(text, width):
    """Word-wrap text to fit within width columns. Returns list of lines."""
    if width <= 0:
        return [text]
    lines = []
    for line in text.split("\n"):
        while len(line) > width:
            # Find last space within width.
            brk = line.rfind(" ", 0, width)
            if brk <= 0:
                brk = width  # Hard break if no space found.
            lines.append(line[:brk])
            line = line[brk:].lstrip()
        lines.append(line)
    return lines


def send_message(fifo_path, message):
    """Write a message to an inject FIFO (non-blocking open)."""
    fd = os.open(fifo_path, os.O_WRONLY | os.O_NONBLOCK)
    try:
        os.write(fd, (message.strip() + "\n").encode())
    finally:
        os.close(fd)


# ---------------------------------------------------------------------------
# Curses TUI
# ---------------------------------------------------------------------------

def run_tui(stdscr):
    curses.curs_set(0)
    stdscr.nodelay(True)
    stdscr.timeout(500)  # Refresh every 500ms.

    # Set up colors.
    curses.start_color()
    curses.use_default_colors()
    curses.init_pair(1, curses.COLOR_GREEN, -1)    # assistant text
    curses.init_pair(2, curses.COLOR_CYAN, -1)     # tool calls
    curses.init_pair(3, curses.COLOR_YELLOW, -1)   # user inject
    curses.init_pair(4, curses.COLOR_WHITE, curses.COLOR_BLUE)  # selected game
    curses.init_pair(5, curses.COLOR_RED, -1)      # headers
    curses.init_pair(6, curses.COLOR_WHITE, -1)    # normal
    curses.init_pair(7, curses.COLOR_MAGENTA, -1)  # status

    selected = 0
    input_buf = ""
    input_mode = False
    scroll_offset = 0
    mode = "map"  # "map" or "chat"
    status_msg = ""
    status_expire = 0

    while True:
        stdscr.erase()
        h, w = stdscr.getmaxyx()

        games = list_games()
        if selected >= len(games):
            selected = max(0, len(games) - 1)

        # Layout: sidebar (left 30 cols) | main panel (right).
        sidebar_w = min(34, w // 3)
        main_x = sidebar_w + 1
        main_w = w - main_x - 1

        # --- Sidebar: game list ---
        stdscr.attron(curses.color_pair(5) | curses.A_BOLD)
        stdscr.addnstr(0, 0, " PLAYTEST GAMES ", sidebar_w)
        stdscr.attroff(curses.color_pair(5) | curses.A_BOLD)

        if not games:
            stdscr.addnstr(2, 1, "No games found.", sidebar_w - 1)
            stdscr.addnstr(3, 1, "Start a playtest to", sidebar_w - 1)
            stdscr.addnstr(4, 1, "see games here.", sidebar_w - 1)
        else:
            for i, g in enumerate(games):
                if i + 1 >= h - 2:
                    break
                y = i + 1
                seed = g["seed"]
                fifo_icon = ">" if g["has_fifo"] else " "

                # Parse status for compact display.
                sl = g["status_line"]
                compact = ""
                if sl:
                    # Extract key fields from "HP 30/30 | Depth 1/5 | Turn 8 | ..."
                    parts = sl.split("|")
                    bits = []
                    for p in parts:
                        p = p.strip()
                        if p.startswith("HP"):
                            bits.append(p)
                        elif p.startswith("Depth"):
                            bits.append(p.replace("Depth ", "D"))
                        elif p.startswith("Turn"):
                            bits.append(p.replace("Turn ", "T"))
                        elif p.startswith("Kills"):
                            bits.append(p.replace("Kills ", "K"))
                    compact = " ".join(bits)

                if i == selected:
                    attr = curses.color_pair(4) | curses.A_BOLD
                else:
                    attr = curses.color_pair(6)

                label = f"{fifo_icon}{seed}"
                stdscr.addnstr(y, 0, label.ljust(sidebar_w), sidebar_w, attr)
                # Status on next line if room.
                if compact and i * 2 + 2 < h - 2:
                    stdscr.addnstr(y, min(len(label) + 1, sidebar_w - len(compact) - 1),
                                   compact[:sidebar_w - len(label) - 2], sidebar_w - len(label) - 1,
                                   curses.color_pair(7))

        # --- Divider ---
        for y in range(h):
            try:
                stdscr.addch(y, sidebar_w, '|' if y > 0 else '+')
            except curses.error:
                pass

        # --- Main panel ---
        if games and 0 <= selected < len(games):
            game = games[selected]

            # Header with mode tabs.
            header = f" [{game['seed']}] "
            map_tab = "[M]ap" if mode == "map" else " M ap"
            chat_tab = "[C]hat" if mode == "chat" else " C hat"
            tab_str = f"  {map_tab}  {chat_tab}"
            stdscr.addnstr(0, main_x, (header + tab_str).ljust(main_w), main_w,
                           curses.color_pair(5) | curses.A_BOLD)

            content_y = 1
            content_h = h - 4 if input_mode else h - 3

            if mode == "map":
                # Show spectate file.
                lines = read_spectate(game["spectate_path"])
                for i, line in enumerate(lines):
                    if content_y + i >= content_h + 1:
                        break
                    try:
                        stdscr.addnstr(content_y + i, main_x, line[:main_w], main_w)
                    except curses.error:
                        pass

            elif mode == "chat":
                # Show conversation log.
                entries = read_conversation(game["log_path"], max_entries=200)
                if isinstance(entries, list) and entries and isinstance(entries[0], str):
                    # Simple string list (error/empty).
                    for i, line in enumerate(entries):
                        if content_y + i >= content_h + 1:
                            break
                        try:
                            stdscr.addnstr(content_y + i, main_x, line[:main_w], main_w)
                        except curses.error:
                            pass
                else:
                    # Word-wrap entries into screen lines, then show the
                    # last content_h lines (auto-scroll to bottom).
                    wrapped = []  # list of (attr, line_str)
                    for entry in entries:
                        etype, text = entry
                        if etype == "assistant":
                            attr = curses.color_pair(1)
                            prefix = ""
                            continuation = ""
                        elif etype == "tool":
                            attr = curses.color_pair(2)
                            prefix = "  > "
                            continuation = "    "
                        elif etype == "user_inject":
                            attr = curses.color_pair(3) | curses.A_BOLD
                            prefix = "YOU: "
                            continuation = "     "
                        else:
                            attr = curses.color_pair(6)
                            prefix = ""
                            continuation = ""

                        full = prefix + text
                        lines = wrap_text(full, main_w)
                        for j, wl in enumerate(lines):
                            if j > 0 and continuation:
                                wl = continuation + wl
                            wrapped.append((attr, wl))

                    # Apply scroll offset (0 = bottom, positive = lines scrolled up).
                    total_lines = len(wrapped)
                    max_scroll = max(0, total_lines - content_h)
                    scroll_offset = min(scroll_offset, max_scroll)

                    if scroll_offset == 0:
                        # Pinned to bottom.
                        visible = wrapped[-(content_h):]
                    else:
                        end = total_lines - scroll_offset
                        start = max(0, end - content_h)
                        visible = wrapped[start:end]

                    for i, (attr, line) in enumerate(visible):
                        if content_y + i >= content_h + 1:
                            break
                        try:
                            stdscr.addnstr(content_y + i, main_x, line[:main_w], main_w, attr)
                        except curses.error:
                            pass

                    # Scroll indicator.
                    if scroll_offset > 0:
                        indicator = f" [{scroll_offset} lines above] "
                        try:
                            stdscr.addnstr(content_y, main_x + main_w - len(indicator) - 1,
                                           indicator, len(indicator),
                                           curses.color_pair(7) | curses.A_BOLD)
                        except curses.error:
                            pass

        # --- Input / status bar ---
        bar_y = h - 2
        try:
            stdscr.addnstr(bar_y, 0, "-" * w, w)
        except curses.error:
            pass

        if input_mode:
            prompt = f"Send to {games[selected]['seed'] if games else '?'}: "
            try:
                stdscr.addnstr(h - 1, 0, prompt + input_buf, w - 1,
                               curses.color_pair(3) | curses.A_BOLD)
            except curses.error:
                pass
            curses.curs_set(1)
        else:
            help_text = "j/k:select  arrows:scroll  m:map  c:chat  i:inject  q:quit"
            if status_msg and time.time() < status_expire:
                help_text = status_msg
            try:
                stdscr.addnstr(h - 1, 0, help_text[:w - 1], w - 1, curses.color_pair(7))
            except curses.error:
                pass
            curses.curs_set(0)

        stdscr.refresh()

        # --- Input handling ---
        try:
            ch = stdscr.getch()
        except curses.error:
            ch = -1

        if ch == -1:
            continue

        if input_mode:
            if ch == 27:  # Escape
                input_mode = False
                input_buf = ""
            elif ch in (10, 13):  # Enter
                if input_buf.strip() and games and 0 <= selected < len(games):
                    game = games[selected]
                    if game["fifo_path"]:
                        try:
                            send_message(game["fifo_path"], input_buf.strip())
                            status_msg = f"Sent to {game['seed']}"
                            status_expire = time.time() + 3
                        except OSError as e:
                            status_msg = f"Error: {e}"
                            status_expire = time.time() + 5
                    else:
                        status_msg = f"No FIFO for {game['seed']} (game ended?)"
                        status_expire = time.time() + 3
                input_mode = False
                input_buf = ""
                mode = "chat"  # Switch to chat to see response.
                scroll_offset = 0  # Pin to bottom to see response.
            elif ch in (8, 127, curses.KEY_BACKSPACE):  # Backspace
                input_buf = input_buf[:-1]
            elif 32 <= ch <= 126:
                input_buf += chr(ch)
        else:
            if ch == ord("q"):
                return
            elif ch == ord("j"):
                if games:
                    selected = min(selected + 1, len(games) - 1)
                    scroll_offset = 0
            elif ch == ord("k"):
                selected = max(selected - 1, 0)
                scroll_offset = 0
            elif ch == curses.KEY_UP:
                scroll_offset += 3
            elif ch == curses.KEY_DOWN:
                scroll_offset = max(0, scroll_offset - 3)
            elif ch == ord("m"):
                mode = "map"
            elif ch == ord("c"):
                mode = "chat"
            elif ch == ord("i"):
                if games and games[selected].get("has_fifo"):
                    input_mode = True
                    input_buf = ""
                else:
                    status_msg = "No active FIFO for this game"
                    status_expire = time.time() + 3
            elif ch == ord("a"):
                # Broadcast mode — type a message to send to all.
                input_mode = True
                input_buf = ""
                # Hack: we'll handle broadcast in the send logic.
                # For now, just use regular inject and let user switch.
                status_msg = "Type message (sends to selected game)"
                status_expire = time.time() + 2
            elif ch == ord("1") and len(games) >= 1:
                selected = 0
            elif ch == ord("2") and len(games) >= 2:
                selected = 1
            elif ch == ord("3") and len(games) >= 3:
                selected = 2
            elif ch == ord("4") and len(games) >= 4:
                selected = 3
            elif ch == ord("5") and len(games) >= 5:
                selected = 4
            elif ch == 21 or ch == curses.KEY_PPAGE:  # Ctrl+U / PgUp
                scroll_offset += max(1, (h - 4) // 2)
            elif ch == 4 or ch == curses.KEY_NPAGE:  # Ctrl+D / PgDn
                scroll_offset = max(0, scroll_offset - max(1, (h - 4) // 2))
            elif ch == ord("G"):  # Jump to bottom.
                scroll_offset = 0
            elif ch == ord("g"):  # Jump to top.
                scroll_offset = 999999
            elif ch == ord("\t"):  # Tab toggles map/chat.
                mode = "chat" if mode == "map" else "map"


# ---------------------------------------------------------------------------
# CLI one-shot mode (backward compat)
# ---------------------------------------------------------------------------

def cli_send(args):
    """Handle --send and --all CLI flags."""
    if args[0] == "--all":
        if len(args) < 2:
            print("Usage: playtest_chat.py --all \"message\"", file=sys.stderr)
            sys.exit(1)
        message = " ".join(args[1:])
        games = list_games()
        sent = 0
        for g in games:
            if g["fifo_path"]:
                try:
                    send_message(g["fifo_path"], message)
                    print(f"Sent to {g['seed']}")
                    sent += 1
                except OSError as e:
                    print(f"Failed for {g['seed']}: {e}", file=sys.stderr)
        if not sent:
            print("No running games with FIFOs found.", file=sys.stderr)
            sys.exit(1)
        return

    if args[0] == "--send":
        args = args[1:]

    if len(args) >= 2:
        seed = args[0]
        message = " ".join(args[1:])
        fifo_path = f"/tmp/roguelike-inject-{seed}.fifo"
        if not os.path.exists(fifo_path):
            print(f"No inject FIFO for game {seed}", file=sys.stderr)
            sys.exit(1)
        try:
            send_message(fifo_path, message)
            print(f"Sent to {seed}")
        except OSError as e:
            print(f"Error: {e}", file=sys.stderr)
            sys.exit(1)
        return

    print("Usage: playtest_chat.py [--send] <seed> \"message\"", file=sys.stderr)
    sys.exit(1)


def main():
    args = sys.argv[1:]

    if args and args[0] in ("--send", "--all"):
        cli_send(args)
        return

    if args and not args[0].startswith("-"):
        # Positional args = CLI one-shot mode.
        cli_send(args)
        return

    # Default: launch TUI.
    curses.wrapper(run_tui)


if __name__ == "__main__":
    main()
