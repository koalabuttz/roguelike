# Spectator Mode: Watching LLM Play in Terminal

## Problem

When the LLM plays via the MCP server, humans can't see the game — all communication
happens over JSON-RPC pipes. Players should be able to watch the LLM play in their
terminal as if they were playing it themselves.

## Core Challenge

The MCP server uses stdin/stdout for JSON-RPC with Claude Code. Game frames need a
**side channel** to reach a human's terminal.

## Options

### 1. File-based spectator (simplest)

After each `act()` call, the MCP server writes the rendered game frame to a file.
A separate viewer process watches the file and displays it.

```
MCP Server                    Spectator Terminal
    |                              |
    |-- act("move_north")          |
    |-- update game state          |
    |-- write frame -> spectate.txt -> detect change
    |                              |-- clear terminal
    |                              +-- render frame
```

**Effort:** S (hours). ~10 lines in MCP server + a small viewer script/binary.

**Pros:**
- Zero coupling between MCP server and spectator
- Trivial to implement
- Works on all platforms

**Cons:**
- Slight latency from file polling
- Temp file cleanup
- Single viewer only (unless multiple processes read the same file)

### 2. Crossterm to stderr

Render directly to the terminal via stderr using crossterm. MCP protocol uses stdout,
so stderr is free. Add a `--spectate` flag to the MCP server binary.

```
Claude Code <--stdout (JSON-RPC)--> MCP Server
                                      |
                                    stderr
                                      v
                                   Terminal
```

**Effort:** S-M. Reuse existing `render.rs`, route output to stderr.

**Pros:**
- No second process needed
- Real-time rendering
- Reuses existing render module

**Cons:**
- Only works if the MCP server's stderr is connected to a visible terminal
- Claude Code may swallow or redirect stderr
- Not viable if the MCP server runs as a headless subprocess

### 3. TCP spectator server (most flexible)

The MCP server spins up a TCP listener on `localhost:PORT`. Spectator clients connect
and receive rendered frames after each action.

```
Claude Code <--stdio--> MCP Server --TCP:7878--> Spectator Client(s)
```

**Effort:** M. TCP server in MCP binary + a spectator client binary.

**Pros:**
- Multiple simultaneous viewers
- Works remotely (SSH, LAN)
- Natural upgrade path to WebSocket for web viewers
- Aligns with the networking roadmap

**Cons:**
- More code (TCP server, client binary, connection management)
- Port management (pick a port, handle conflicts)
- Firewall considerations on some systems

### 4. Named pipe / Unix socket

Same as TCP but uses OS-level IPC primitives. Spectator attaches to a named pipe
or Unix domain socket.

**Effort:** M. Similar to TCP but platform-specific.

**Pros:**
- No port management
- Lower overhead than TCP
- Natural OS-level IPC

**Cons:**
- Windows named pipes differ from Unix domain sockets — platform-specific code
- Doesn't extend to remote viewing

## Current Status

**Option 1 (file-based) is implemented** in `crates/mcp/src/spectate.rs`.

- Opt-in via `ROGUELIKE_SPECTATE_PATH` environment variable (set to a file path)
- Atomic writes (write to `.tmp`, then rename) to prevent partial reads by viewers
- Integrated into all MCP actions: `new_game`, `act`, `pathfind_to`, `auto_explore`, `auto_fight`, `load_game`
- Renders the full explored map (with entities in FOV, frontiers as `~`), a status line (HP, turn, kills, explored %, seed), and the last 4 log messages
- Errors are silently ignored — spectating is best-effort and never breaks the MCP server

Usage:

```sh
ROGUELIKE_SPECTATE_PATH=/tmp/roguelike-spectate.txt cargo run --bin mcp_server
```

A viewer process can watch the file and display it (e.g., `watch -n 0.1 cat /tmp/roguelike-spectate.txt`).

## Next Step

The original plan was to graduate to **Option 3** (TCP) for remote viewing capabilities. However, the [AT Protocol spectating design](atproto-spectating.md) now covers the remote spectating use case via federated PDS frame publishing and Jetstream delivery — with zero infrastructure to operate.

**TCP remains useful** for low-latency local spectating (e.g., a second terminal on the same machine or LAN) where the AT Protocol round-trip would add unnecessary delay. But for the primary use case of "watch someone play from anywhere," atproto spectating supersedes TCP.

**Recommended path:**

1. Keep the file-based spectator as-is for local MCP spectating (done).
2. Implement atproto spectating for federated remote viewing (see [atproto-spectating.md](atproto-spectating.md)).
3. Only add TCP if there's a concrete need for low-latency local multi-viewer that the file-based approach can't satisfy.
