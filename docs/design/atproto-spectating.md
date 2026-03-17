# AT Protocol Spectating

Design for federated, cross-platform game spectating using AT Protocol. Players publish live game frames to their PDS repository; spectators subscribe via Jetstream and render the frames locally.

This document builds on two existing designs:

- [spectator-mode.md](spectator-mode.md) — the local file-based spectator (implemented) and planned TCP upgrade
- [atproto.md](atproto.md) — AT Protocol identity, OAuth, and PDS save storage

## Goals

1. **Federated spectating.** Anyone with a Jetstream connection can watch any game in real time, regardless of which server the player is on.
2. **No central relay.** The player's PDS is the only server involved. No game-specific matchmaking server, no WebSocket relay, no infrastructure to operate beyond what AT Protocol already provides.
3. **Cross-platform frame production.** Any platform that can make HTTP requests can publish spectate frames — including the SSH server, terminal client, MCP server, and future web frontend.
4. **Graceful degradation for constrained platforms.** Platforms that cannot make HTTP requests (GBA, C64) are not penalized. They continue using the file-based spectator or produce no spectate output at all. The design must not add mandatory dependencies to `roguelike-core`.
5. **Discovery.** Spectators can find active games to watch without knowing the player's DID in advance.

## Non-Goals

- **Replay storage.** Atproto spectating is live. Replay files (deterministic command logs) are a separate system — see [Server-Attested Replays](#future-extension-server-attested-replays) for the design sketch. Old spectate records may be garbage-collected.
- **Chat or interaction.** Spectators watch passively. Twitch-style chat or audience interaction is out of scope.
- **Replacing the file-based spectator.** The `ROGUELIKE_SPECTATE_PATH` mechanism remains for local MCP spectating. Atproto spectating is an additional transport, not a replacement.
- **Streaming video or tiles.** Frames are plain-text ASCII, matching the existing `render_frame()` output. Graphical spectating (canvas/tile-based) is a future layer on top.

## Background: Why AT Protocol

AT Protocol's architecture maps naturally onto game spectating:

| AT Protocol concept | Spectating analog |
|---------------------|-------------------|
| PDS repository | Player's "broadcast channel" — frames are records in their repo |
| Jetstream | Real-time frame delivery — subscribe by DID + collection |
| DID | Stable player identity — survives handle changes, server migrations |
| Custom Lexicon | Typed frame schema with versioning and validation |
| Federation | Any PDS can host a player; any Jetstream subscriber can watch |

The critical advantage over a custom TCP/WebSocket relay is **zero infrastructure**. The player's existing PDS (e.g., Bluesky's hosted PDS) handles storage, delivery, and availability. The game server only needs to write records — it doesn't need to maintain connections to spectators.

### Rate Limits

Bluesky's hosted PDS allows ~1,666 `createRecord` calls per hour (~27/minute). A turn-based roguelike generates far fewer turns than this — even aggressive play rarely exceeds 5 turns/minute, and the SSH server's MCP spectating averages 1-2 frames/second during autorun bursts.

Self-hosted PDS instances have no rate limits, making them ideal for high-frequency games or testing.

### Jetstream vs. Firehose

The AT Protocol firehose (`com.atproto.sync.subscribeRepos`) delivers all events from all users on a relay — terabytes/day. **Jetstream** is a filtered, lightweight WebSocket API (~99% smaller) that supports filtering by:

- **DID** — watch one specific player
- **Collection** — only spectate records, not posts/likes/saves

Jetstream is the correct choice for spectating. A Rust client connects to `wss://jetstream2.us-east.bsky.network/subscribe` with query parameters `?wantedDids=did:plc:abc123&wantedCollections=com.example.roguelike.spectate.frame`.

## Architecture

```
Producer (game server/client)              Consumer (spectator)
─────────────────────────────              ────────────────────

  GameState                                  Jetstream
     │                                     (WebSocket)
     ▼                                         │
  render_frame()                               │
     │                                         ▼
     ▼                                   Parse record
  FrameSink trait                              │
     │                                         ▼
     ├─► FileFrameSink (existing)         Render to terminal
     ├─► AtprotoFrameSink (new)           (crossterm / canvas)
     └─► TcpFrameSink (future)
              │
              ▼
         PDS XRPC
       createRecord
```

### Producer Side

The producer is any platform that runs the game loop. After each turn, the game loop calls `frame_sink.write_frame(state)` through a trait. Platforms choose which sink(s) to use:

| Platform | FrameSink implementation | Notes |
|----------|-------------------------|-------|
| SSH server | `AtprotoFrameSink` | Player is authenticated via atproto OAuth; DPoP tokens available |
| Terminal | `AtprotoFrameSink` (opt-in) | Requires atproto login; off by default |
| MCP server | `FileFrameSink` (existing) | Local spectating via `ROGUELIKE_SPECTATE_PATH` |
| Web (WASM) | `AtprotoFrameSink` via JS | JS layer handles XRPC; WASM calls through `wasm_bindgen` |
| GBA / Vita / C64 | None or `FileFrameSink` | No HTTP stack; constrained platforms don't produce atproto frames |

### Consumer Side

The consumer connects to Jetstream, receives frame records in real time, and renders them to whatever display is available:

| Consumer | Rendering | Notes |
|----------|-----------|-------|
| SSH server (Watch menu) | Crossterm to SSH channel | Reuses existing `CrosstermRenderer` |
| Terminal client | Crossterm to local terminal | Same renderer as gameplay |
| Web viewer | HTML `<pre>` or canvas | Simplest: dump ASCII into a monospace element |
| CLI tool | `cat` equivalent | Like `tools/spectate.sh` but reading from Jetstream instead of a file |

Constrained platforms (GBA, Vita, C64) are **not** expected to be spectate consumers. They lack the networking stack for Jetstream WebSocket connections. This is acceptable — spectating is a social/community feature that belongs on connected platforms.

## Lexicon Design

### `com.example.roguelike.spectate.frame`

A single rendered game frame. Created after each player turn. The record is self-contained — a spectator can render any individual frame without prior context.

```json
{
  "lexicon": 1,
  "id": "com.example.roguelike.spectate.frame",
  "defs": {
    "main": {
      "type": "record",
      "key": "tid",
      "description": "A single spectate frame. Created after each player turn. TID-keyed for chronological ordering.",
      "record": {
        "type": "object",
        "required": ["map", "status", "turn", "createdAt"],
        "properties": {
          "map": {
            "type": "array",
            "description": "ASCII map lines. Each element is one row of the rendered map.",
            "items": { "type": "string", "maxLength": 256 },
            "maxLength": 128
          },
          "status": {
            "type": "string",
            "maxLength": 256,
            "description": "Status line: HP, turn, kills, explored %, seed code."
          },
          "messages": {
            "type": "array",
            "description": "Recent combat/event messages (last N from the log).",
            "items": { "type": "string", "maxLength": 256 },
            "maxLength": 8
          },
          "turn": {
            "type": "integer",
            "description": "Turn number. Enables gap detection and ordering."
          },
          "seedCode": {
            "type": "string",
            "maxLength": 32,
            "description": "Seed code for this game. Allows spectators to identify which run they're watching."
          },
          "gameOver": {
            "type": "boolean",
            "description": "True if the player died this turn. Signals end of stream."
          },
          "createdAt": {
            "type": "string",
            "format": "datetime",
            "description": "ISO 8601 timestamp. Required by AT Protocol for record ordering."
          }
        }
      }
    }
  }
}
```

**Why TID keys:** AT Protocol TIDs (timestamp identifiers) are lexicographically sortable by creation time. This gives frames a natural chronological order, enables `listRecords` with `reverse=true` to get the latest frame, and allows range queries for catch-up.

**Why not blobs:** Each frame is 2-5KB of ASCII text — well within AT Protocol's record size limits (up to 64KB for the full record). Inline string arrays avoid the upload-blob-then-reference dance, which would double the XRPC calls per frame.

**Why self-contained frames:** A spectator joining mid-game sees the full explored map, current HP, and recent messages in a single record. No need to replay from turn 1. This matches how the existing `render_frame()` already works — each frame includes the full explored map, not just a diff.

### `com.example.roguelike.spectate.session`

Metadata about an active spectating session. One record per game, created at game start, updated at game end.

```json
{
  "lexicon": 1,
  "id": "com.example.roguelike.spectate.session",
  "defs": {
    "main": {
      "type": "record",
      "key": "tid",
      "description": "An active or completed spectate session. Created when a game starts broadcasting.",
      "record": {
        "type": "object",
        "required": ["seedCode", "startedAt", "active", "createdAt"],
        "properties": {
          "seedCode": {
            "type": "string",
            "maxLength": 32,
            "description": "Seed code for this game."
          },
          "playerName": {
            "type": "string",
            "maxLength": 64,
            "description": "Player's chosen character name, if set."
          },
          "mapWidth": {
            "type": "integer",
            "description": "Map width in tiles."
          },
          "mapHeight": {
            "type": "integer",
            "description": "Map height in tiles."
          },
          "active": {
            "type": "boolean",
            "description": "True while the game is in progress. Set to false when the game ends."
          },
          "endReason": {
            "type": "string",
            "maxGraphemes": 64,
            "description": "How the game ended: 'death', 'quit', 'disconnect'. Null while active."
          },
          "finalTurn": {
            "type": "integer",
            "description": "Turn count when the game ended. Null while active."
          },
          "startedAt": {
            "type": "string",
            "format": "datetime"
          },
          "endedAt": {
            "type": "string",
            "format": "datetime"
          },
          "createdAt": {
            "type": "string",
            "format": "datetime"
          }
        }
      }
    }
  }
}
```

**Purpose:** Discovery. A spectator can `listRecords` on any player's `spectate.session` collection, filter for `active: true`, and know there's a live game to watch. The session record also provides display metadata (seed, player name, map dimensions) for a "games to watch" list without fetching any frames.

**Lifecycle:**

1. Game starts → `createRecord` with `active: true`
2. Game in progress → frames written to `spectate.frame` collection
3. Game ends → `putRecord` to update session with `active: false`, `endReason`, `finalTurn`, `endedAt`

## Frame Production: The `FrameSink` Trait

### Trait Definition

Lives in `crates/core/src/spectate.rs` alongside `render_frame()` (already extracted from `crates/mcp/src/spectate.rs`):

```rust
/// A destination for spectate frames.
///
/// Implementations handle transport (file, atproto, TCP, etc.).
/// The game loop calls `write_frame` after each turn.
/// All methods are infallible — spectating is best-effort.
pub trait FrameSink {
    /// Write a rendered frame. Called after each player turn.
    fn write_frame(&self, state: &GameState);

    /// Signal that a new game session has started.
    /// Called once at game start, before the first `write_frame`.
    fn session_start(&self, state: &GameState) {
        // Default: no-op. Not all sinks need session tracking.
        let _ = state;
    }

    /// Signal that the game session has ended.
    /// Called once when the game ends (death, quit, disconnect).
    fn session_end(&self, state: &GameState, reason: &str) {
        // Default: no-op.
        let _ = (state, reason);
    }
}
```

**Why in core:** The trait references only `GameState`, which is already in core. No platform dependencies. Any crate can implement it. This follows the same pattern as `Renderer` and `InputSource` — core defines the trait, frontends implement it. (Note: `SaveBackend` follows a different pattern — it lives in `crates/saves`, not core, because constrained platforms like GBA/C64 need completely different save mechanisms. `FrameSink` belongs in core because *all* platforms can pass a `NullFrameSink` at zero cost, unlike `SaveBackend` which imposes interface assumptions about JSON slots.)

**Why infallible:** Spectating must never break gameplay. Network errors, PDS outages, rate limits — all are silently absorbed. This matches the `FileFrameSink` design ("errors are silently ignored").

### `FileFrameSink` (Done)

The `render_frame()` function and the `FrameSink` trait + `NullFrameSink` live in `crates/core/src/spectate.rs`. The MCP crate's `FileFrameSink` (in `crates/mcp/src/spectate.rs`) implements the `FrameSink` trait, using `render_frame` from core and atomic file writes.

```
  crates/core/src/spectate.rs →  FrameSink trait + NullFrameSink + render_frame()
  crates/mcp/src/spectate.rs  →  FileFrameSink (implements FrameSink)
```

### `AtprotoFrameSink`

Lives in `crates/atproto/src/spectate.rs`. Publishes frames as records to the player's PDS.

```rust
pub struct AtprotoFrameSink {
    /// XRPC client for PDS calls (shared with PdsSaveBackend).
    pds_client: Arc<PdsClient>,
    /// Player's DID (repo identifier for createRecord).
    did: String,
    /// Tokio runtime handle for blocking on async XRPC calls.
    rt_handle: tokio::runtime::Handle,
    /// TID of the current session record (for updating on game end).
    session_tid: Mutex<Option<String>>,
    /// Whether spectating is enabled for this session.
    enabled: bool,
}
```

**Async bridge:** Same pattern as `PdsSaveBackend` — the `FrameSink` trait is synchronous, but PDS calls are async. `AtprotoFrameSink` uses `rt_handle.block_on()` internally, matching the existing codebase pattern (see [atproto.md](atproto.md#syncasync-bridge-for-savebackend)).

**Batching / throttling:** To avoid hitting rate limits during autorun bursts (where multiple turns execute in rapid succession), `AtprotoFrameSink` can drop intermediate frames if they arrive faster than a configurable minimum interval (e.g., 500ms). The spectator sees the latest state, not every intermediate step. This is invisible to the game loop — it calls `write_frame` every turn regardless.

```rust
impl FrameSink for AtprotoFrameSink {
    fn write_frame(&self, state: &GameState) {
        if !self.enabled { return; }

        // Throttle: skip if less than MIN_INTERVAL since last publish
        let now = Instant::now();
        let mut last = self.last_publish.lock().unwrap();
        if now.duration_since(*last) < MIN_PUBLISH_INTERVAL {
            return;
        }
        *last = now;

        let frame = render_frame(state);
        // Fire-and-forget: errors are logged but never propagated
        if let Err(e) = self.publish_frame(state, &frame) {
            tracing::debug!("Spectate publish failed: {e}");
        }
    }
}
```

### Composite Sink

For platforms that want multiple outputs (e.g., SSH server publishing to atproto AND writing to a local file for debugging):

```rust
pub struct CompositeFrameSink {
    sinks: Vec<Box<dyn FrameSink>>,
}

impl FrameSink for CompositeFrameSink {
    fn write_frame(&self, state: &GameState) {
        for sink in &self.sinks {
            sink.write_frame(state);
        }
    }
}
```

This is a convenience, not a requirement. Most platforms will use a single sink.

## Frame Consumption: Jetstream Subscriber

### Connection

The spectate viewer connects to a Jetstream endpoint with filters:

```
wss://jetstream2.us-east.bsky.network/subscribe
  ?wantedDids=did:plc:abc123
  &wantedCollections=com.example.roguelike.spectate.frame
```

Each WebSocket message is a JSON event containing the `commit` operation and the full record value. The viewer extracts `map`, `status`, and `messages` and renders them.

### Rust Implementation

A `JetstreamSubscriber` in `crates/atproto/src/jetstream.rs`:

```rust
pub struct JetstreamSubscriber {
    /// The WebSocket connection to Jetstream.
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

pub struct SpectateFrame {
    pub map: Vec<String>,
    pub status: String,
    pub messages: Vec<String>,
    pub turn: i32,
    pub game_over: bool,
}

impl JetstreamSubscriber {
    /// Connect to Jetstream, filtering for a specific player's spectate frames.
    pub async fn connect(did: &str) -> Result<Self, JetstreamError> {
        let url = format!(
            "wss://jetstream2.us-east.bsky.network/subscribe\
             ?wantedDids={did}\
             &wantedCollections=com.example.roguelike.spectate.frame"
        );
        let (ws, _) = tokio_tungstenite::connect_async(&url).await?;
        Ok(Self { ws })
    }

    /// Block until the next frame arrives.
    pub async fn next_frame(&mut self) -> Result<SpectateFrame, JetstreamError> {
        loop {
            let msg = self.ws.next().await
                .ok_or(JetstreamError::Disconnected)??;
            if let Message::Text(text) = msg {
                if let Ok(frame) = parse_spectate_event(&text) {
                    return Ok(frame);
                }
            }
        }
    }
}
```

**Crate dependency:** `tokio-tungstenite` for WebSocket. This is an async-only dependency, confined to `crates/atproto`. Core and constrained platforms never see it.

### Rendering Spectated Frames

The spectate viewer renders frames using the same `Renderer` trait used for gameplay. On terminal/SSH, this is `CrosstermRenderer`. Each received frame clears the screen and draws the map, status, and messages line by line using `draw_str`.

```rust
fn render_spectate_frame<R: Renderer>(renderer: &mut R, frame: &SpectateFrame) {
    renderer.clear();
    for (y, line) in frame.map.iter().enumerate() {
        renderer.draw_str(0, y as Coord, line, GameColor::White, GameColor::Black);
    }
    let status_y = frame.map.len() as Coord;
    renderer.draw_str(0, status_y, &frame.status, GameColor::Cyan, GameColor::Black);
    for (i, msg) in frame.messages.iter().enumerate() {
        renderer.draw_str(0, status_y + 1 + i as Coord, msg, GameColor::Grey, GameColor::Black);
    }
    renderer.flush();
}
```

This is intentionally simple — no color parsing, no entity highlighting. The ASCII frame is rendered as-is. Future enhancements (colored entities, highlighted combat) can be layered on by enriching the frame format.

## Discovery: Finding Active Games

### For Logged-In Users (SSH Server Menu / Web)

The SSH server menu already has a "Watch a Game (coming soon)" entry (disabled) in `session.rs`. When spectating is implemented, this becomes functional. The menu queries the server's known active sessions:

1. The server tracks which logged-in users have spectating enabled (via an in-memory set of DIDs).
2. The Watch menu lists these as "{handle} — Seed {seedCode}, Turn {turn}".
3. Selecting a player starts a `JetstreamSubscriber` for that DID.

For players on other servers (federated discovery):

1. The user enters a handle (e.g., `alice.bsky.social`).
2. Resolve handle → DID.
3. `listRecords(did, collection=spectate.session, limit=5, reverse=true)`.
4. Filter for `active: true`.
5. If found, start `JetstreamSubscriber` for that DID.

### For Pre-Login Users (SSH Lobby)

The lobby's "Watch a Game" option uses the same flow but doesn't require authentication. Jetstream is a public WebSocket endpoint — no OAuth needed to subscribe. `listRecords` on a public PDS is also unauthenticated.

This means spectating requires zero login. Anyone who connects via SSH can watch anyone else play if they know the handle.

### Aggregate Discovery (Future)

For a "browse all active games" experience without knowing specific handles:

- **Option A: Jetstream collection filter.** Subscribe to `wantedCollections=com.example.roguelike.spectate.session` without a DID filter. This receives session create/update events from all users on the relay's connected PDSes. The viewer maintains a local list of active sessions.
- **Option B: Custom AppView.** A dedicated indexer subscribes to the firehose/Jetstream, indexes all `spectate.session` records, and exposes a query API. This is the AT Protocol-native approach for aggregation but requires running a service.
- **Option C: Server-local list.** Each game server tracks its own active sessions and exposes them in the Watch menu. No federation — spectating is limited to players on the same server.

**Recommendation:** Start with Option C (server-local, no infra). Upgrade to Option A (Jetstream collection filter) when cross-server spectating is desired. Option B is only needed for a "spectate directory" web page.

## Integration with the Game Loop

### Where `write_frame` Is Called

The MCP server calls `FileFrameSink::write_frame()` (via the `FrameSink` trait) after each `act()`, `pathfind_to()`, `auto_explore()`, and `auto_fight()` tool call. The shared game loop in `crates/tui/src/game_loop.rs` calls `frame_sink.write_frame(state)` after each step that changes game state:

```rust
// In game_loop.rs, after a successful step:
if step_result.action_taken {
    frame_sink.write_frame(&game_state);
}
```

The `FrameSink` is passed into `run_game_loop` as an additional parameter:

```rust
pub fn run_game_loop<W: Write, D: DevHooks>(
    renderer: &mut render::CrosstermRenderer<W>,
    input: &mut dyn InputProvider,
    saves: &dyn SaveBackend,
    dev: &mut D,
    config: GameLoopConfig,
    frame_sink: &dyn FrameSink,    // NEW
) -> io::Result<GameLoopResult>
```

Platforms that don't spectate pass a no-op `NullFrameSink`:

```rust
pub struct NullFrameSink;
impl FrameSink for NullFrameSink {
    fn write_frame(&self, _state: &GameState) {}
}
```

### MCP Server Integration

The MCP server uses `FileFrameSink` (which implements `FrameSink`). If an atproto session is available (future MCP-with-identity mode), it can additionally use `AtprotoFrameSink` via `CompositeFrameSink`.

## Constrained Platform Strategy

The "don't close doors" principle means the design must not:

1. Add mandatory dependencies to `roguelike-core`
2. Require HTTP/WebSocket capability from all platforms
3. Change the `GameState` structure in ways that increase memory usage for platforms that don't spectate

### What Constrained Platforms Get

| Capability | GBA | Vita | C64 |
|-----------|-----|------|-----|
| **Produce frames** (atproto) | No | Possible (has WiFi) | Via [bridge](../platforms/c64-atproto-bridge.md) |
| **Produce frames** (file) | No (no filesystem) | Yes | No |
| **Consume frames** (Jetstream) | No | Possible | No |
| **`FrameSink` trait** | `NullFrameSink` | `FileFrameSink` or `AtprotoFrameSink` | `NullFrameSink` (bridge handles atproto externally) |

The GBA and C64 have no networking and minimal I/O. They pass `NullFrameSink` to the game loop — zero overhead, zero code changes needed in the game loop.

The Vita has WiFi and could theoretically participate as both producer and consumer, but this is a stretch goal far beyond the initial implementation.

### How the Design Stays Portable

- **`FrameSink` trait in core** uses only `&GameState` — no `async`, no `tokio`, no `reqwest`. Core has no mandatory platform dependencies, keeping it portable for future GBA/C64 ports (which would use `no_std` feature flags).
- **`render_frame()` in core** is a pure function: `&GameState -> String`. No I/O, no allocation beyond the returned string.
- **`AtprotoFrameSink` in the atproto crate** is the only place that touches HTTP, WebSocket, and tokio. Constrained platform crates never depend on `crates/atproto`.
- **`NullFrameSink` in core** is zero-cost — the compiler eliminates it entirely when monomorphized.

```
Dependency graph (spectating + saves):

  roguelike-core
    FrameSink trait         ← no platform deps
    NullFrameSink           ← no platform deps
    render_frame()          ← no platform deps

  roguelike-saves           ← depends on core only
    SaveBackend trait       ← connected platforms only (not GBA/C64)

  roguelike-mcp
    FileFrameSink           ← std::fs only

  roguelike-atproto         ← depends on core + saves
    AtprotoFrameSink        ← reqwest, tokio
    PdsSaveBackend          ← implements SaveBackend
    JetstreamSubscriber     ← tokio-tungstenite

  roguelike-terminal        ← depends on core + saves + tui (+ atproto when feature enabled)
    uses NullFrameSink or AtprotoFrameSink (opt-in via atproto feature)

  roguelike-ssh             ← depends on core + saves + tui
    uses AtprotoFrameSink (when user has atproto identity)

  roguelike-gba / roguelike-c64  ← depends on core only
    uses NullFrameSink      ← zero additional deps
    own save mechanisms     ← no SaveBackend dependency
```

## Garbage Collection of Old Frames

Spectate frame records accumulate in the player's PDS repo. A 200-turn game produces ~200 records (~2-5KB each, ~400KB-1MB total). Over many games, this grows.

### Strategy: Delete on Session End

When the game ends, the producer deletes all frame records for that session:

```rust
fn session_end(&self, state: &GameState, reason: &str) {
    // Update session record: active=false, endReason, finalTurn
    self.update_session_record(reason, state.turn_count);

    // Delete all frame records for this session
    // listRecords + deleteRecord in a loop
    self.delete_session_frames();
}
```

This keeps the PDS repo clean. The session record remains as a historical marker (lightweight, ~200 bytes) but the frames are ephemeral.

**Edge case: disconnect.** If the player disconnects without a clean shutdown (SSH connection drops, browser tab closed), the session's frames are orphaned. A cleanup routine on next login can delete frames from stale sessions:

```rust
fn cleanup_stale_sessions(&self) {
    // List session records where active=true and startedAt > 1 hour ago
    // For each: set active=false, delete associated frames
}
```

### Alternative: Keep Last N Games

Some players may want to let friends watch replays of recent games. An alternative is to keep the last N sessions' frames and delete older ones. This is a user preference, not a system requirement.

## Security and Privacy

### Public Visibility

AT Protocol PDS repos are public by default. Anyone who knows a player's DID can read their spectate frames and session records. This means:

- Spectators can watch without authentication
- The player's game state (map layout, HP, position) is visible to anyone
- The seed code is published, allowing others to play the same dungeon

This is intentional — spectating is a public, social feature. Players who don't want to be watched simply don't enable spectating. It is opt-in per session via a setting or menu toggle.

### No Authentication Required for Consumers

Jetstream subscriptions and `listRecords` on public PDS repos don't require OAuth. The consumer side has zero auth complexity. This is why pre-login spectating works.

### Producer Authentication

Only the producer (the player) needs atproto OAuth tokens to write frame records to their PDS. This is handled by the existing atproto OAuth flow described in [atproto.md](atproto.md#oauth-integration).

## Implementation Phases

> **Phase numbering note:** These phases (0–4) are specific to atproto spectating and are independent of the phases in [atproto.md](atproto.md#implementation-phases) (1–4), which cover OAuth, PDS saves, and WASM. Where there are cross-dependencies, they are noted explicitly (e.g., spectating Phase 1 depends on atproto.md Phase 1 for OAuth).

### Phase 0: Extract `render_frame` and Define `FrameSink` (Done)

**Effort:** S (hours).

- `crates/core/src/spectate.rs` — `FrameSink` trait, `NullFrameSink`, `render_frame()` all live here.
- `crates/core/src/lib.rs` — `pub mod spectate` is present.
- `crates/mcp/src/spectate.rs` — `FileFrameSink` implements `FrameSink`, uses `render_frame` from core.
- `crates/tui/src/game_loop.rs` — `run_game_loop` accepts `frame_sink: &dyn FrameSink`, calls `write_frame` after autorun, auto-explore, and normal commands.
- `crates/terminal/src/main.rs` — Passes `&NullFrameSink`.
- `crates/ssh/src/session.rs` — Passes `&NullFrameSink` (atproto sink comes in Phase 2).

This was a pure refactor — no new functionality, no new dependencies. All existing tests pass. The MCP spectator works exactly as before.

### Phase 1: Lexicon Design and Frame Publishing

**Effort:** M (days). Depends on [atproto.md Phase 1](atproto.md#phase-1-http-server--oauth-ssh) (OAuth).

Define the `spectate.frame` and `spectate.session` lexicons. Implement `AtprotoFrameSink` in `crates/atproto/src/spectate.rs`. Wire it into the SSH session for players logged in via atproto.

**Changes:**
- `crates/atproto/src/spectate.rs` — new file: `AtprotoFrameSink`
- `crates/atproto/src/lexicon.rs` — add spectate record type definitions
- `crates/ssh/src/session.rs` — use `AtprotoFrameSink` when player has atproto identity

**Validation:** Use the Bluesky PDS API explorer or `curl` to verify records appear. Use `listRecords` to read them back.

### Phase 2: Jetstream Consumer and Watch UI

**Effort:** M (days)

Implement `JetstreamSubscriber` in `crates/atproto/src/jetstream.rs`. Add spectate rendering. Wire into the SSH server's existing "Watch a Game" server menu item (currently disabled in `session.rs`) and add a similar option to the pre-login lobby.

**Changes:**
- `crates/atproto/src/jetstream.rs` — new file: `JetstreamSubscriber`
- `crates/ssh/src/spectate_viewer.rs` — new file: renders received frames to the SSH channel
- `crates/ssh/src/lobby.rs` — add "Watch a Game" option (no login required for spectating)
- `crates/ssh/src/session.rs` — enable existing "Watch a Game" server menu item

**Validation:** Two SSH sessions — one playing, one watching. The watcher sees the player's moves in near-real-time.

### Phase 3: Discovery and Polish

**Effort:** M (days)

Add handle-based discovery (enter a handle, resolve DID, check for active sessions). Add the spectate enable/disable toggle in settings. Implement frame garbage collection on session end. Handle edge cases (disconnect cleanup, rate limit backoff, Jetstream reconnection).

### Phase 4: Web Spectating (Future)

**Effort:** L (week+). Depends on the WASM frontend from [atproto.md Phase 3](atproto.md#phase-3-wasm-frontend).

A web page that spectates a game in the browser. No WASM needed for spectating alone — a simple JS Jetstream client + `<pre>` element rendering is sufficient. This could ship before the full WASM game client.

## Configuration

### Producer Settings

| Setting | Where | Default | Purpose |
|---------|-------|---------|---------|
| Spectate enabled | In-game settings menu | Off | Whether to publish frames to PDS during play |
| Min publish interval | `game.toml` / server config | 500ms | Throttle for autorun bursts |

### Consumer Settings

| Setting | Where | Default | Purpose |
|---------|-------|---------|---------|
| Jetstream endpoint | Server config | `wss://jetstream2.us-east.bsky.network/subscribe` | Which Jetstream relay to use |

Spectating doesn't add settings to the `Settings` struct in core — it's a per-session transport concern, not a gameplay preference. The producer enable/disable is a session-level toggle, not a persisted setting.

## Open Questions

1. **Lexicon namespace.** Same open question as [atproto.md](atproto.md#open-questions). The spectate lexicons share the same namespace prefix. Must be decided before publishing records.

2. **Frame diff compression.** Full frames per turn are simple but redundant — most of the map doesn't change between turns. A diff format (only changed tiles) would reduce record size from ~3KB to ~100 bytes per turn. Trade-off: complexity vs. bandwidth. **Recommendation:** Start with full frames. Optimize later if PDS storage or rate limits become a problem.

3. **Colored frames.** The current `render_frame()` produces plain ASCII with no color information. Adding per-cell color data would enable richer spectate rendering but increases frame size significantly (each cell needs fg+bg color). **Recommendation:** Defer. Plain ASCII is sufficient for v1 and matches the existing spectator.

4. **Spectating MCP games.** The MCP server currently uses file-based spectating. Should MCP games also publish to atproto? This requires the MCP server to have atproto credentials, which doesn't fit the current headless-tool model. **Recommendation:** MCP stays file-based. Atproto spectating is for human-played SSH/terminal/web sessions.

5. **Multiple simultaneous games.** If a player starts a new game while a previous session's frames haven't been cleaned up, the TID ordering ensures frames from different games don't interleave. But a spectator subscribing to the DID would see frames from the new game mixed with cleanup deletes from the old. **Recommendation:** The viewer filters by `seedCode` or only renders frames with `turn` values that increase monotonically.

6. **Jetstream availability.** Jetstream is a Bluesky-operated service, not part of the AT Protocol specification. If it goes down or changes, spectating breaks. **Mitigation:** The `JetstreamSubscriber` endpoint is configurable. Self-hosted Jetstream instances exist. The design doesn't depend on Bluesky specifically.

## Relationship to Other Design Docs

| Doc | Relationship |
|-----|-------------|
| [spectator-mode.md](spectator-mode.md) | Atproto spectating is the federated evolution of the file-based spectator. The file spectator remains for local/MCP use. TCP is reserved for low-latency local multi-viewer if needed; atproto handles the remote-viewing use case. |
| [atproto.md](atproto.md) | Spectating reuses the same atproto infrastructure: OAuth tokens, PDS client, DID resolution. `AtprotoFrameSink` shares the `PdsClient` with `PdsSaveBackend`. The spectate lexicons live alongside the save lexicons under the same namespace. |
| [cross-platform.md](../architecture/cross-platform.md) | The `FrameSink` trait follows the same pattern as `Renderer` and `InputSource` — defined in core, implemented per-platform. Constrained platforms use `NullFrameSink`. (`SaveBackend` follows a different pattern — it lives in `crates/saves`, not core, since constrained platforms need different save mechanisms.) |
| [simulation.md](../architecture/simulation.md) | As the simulation grows (tile state, events, richer AI), spectate frames naturally capture the richer game state through `render_frame()`. No spectating changes needed — the frame producer always reflects current `GameState`. |

## Future Extension: Server-Attested Replays

> **Status:** Design sketch. Depends on the command replay system (chainlink #135) and ATProto OAuth (atproto.md Phase 1).

Spectating is live. Replays are recorded. This section describes how the SSH server can produce **cryptographically attested replay records** — command logs that carry a server signature proving they were produced by real-time human play, not uploaded via the API.

### The Problem: Replay Authenticity

A command replay (seed + command bytes) proves **correctness** — anyone can re-execute the commands and verify the claimed outcome (depth reached, kills, win/loss). But it does not prove **authenticity** — that a human actually played those commands in real-time.

Since the game is deterministic, open-source, and single-player, an attacker can:

1. **Solve programmatically.** Simulate the game tree, compute the optimal command sequence, post it as a "replay." The replay verifies correctly, but nobody played it.
2. **Edit replays.** Play a real game, but fork from earlier save points on mistakes. Post only the polished version.
3. **Craft commands directly.** Skip the game entirely — compute the byte array via the game engine as a library, post it to ATProto via the API. The PDS doesn't know or care where the bytes came from.

ATProto's cryptographic properties (DID signatures, Merkle trees) prove *who* posted the record and *when*, but not *how the command bytes were generated*.

### The Solution: SSH Server as Trusted Witness

The SSH server (`crates/ssh/`) is a **trusted execution environment** — it runs the game logic server-side, the player cannot modify it, and the server observes every input in real-time:

- The server controls the game binary (players connect to it, not the other way around)
- Commands arrive over the SSH channel with server-measured timestamps
- The player is authenticated (SSH login + optional ATProto identity)
- The server can detect non-human timing patterns (sub-millisecond inputs, perfectly uniform intervals)

When a game ends, the SSH server signs the replay with its own keypair:

```
attestation = sign(server_privkey, hash(seed ‖ commands ‖ timing ‖ player_did ‖ game_version))
```

This signature is a cryptographic claim: **"These commands were received in real-time from an authenticated SSH session on this server, playing game version X."** Anyone can verify it against the server's public key (published at the server's DID or a well-known endpoint).

### Lexicon: `com.example.roguelike.replay.attestedRun`

```json
{
  "lexicon": 1,
  "id": "com.example.roguelike.replay.attestedRun",
  "defs": {
    "main": {
      "type": "record",
      "key": "tid",
      "description": "A server-attested game replay. The server signature proves the commands were produced by real-time play on a trusted server, not generated or uploaded programmatically.",
      "record": {
        "type": "object",
        "required": ["seed", "commands", "result", "gameVersion", "serverDid", "attestation", "createdAt"],
        "properties": {
          "seed": {
            "type": "integer",
            "description": "16-bit game seed."
          },
          "mapWidth": { "type": "integer" },
          "mapHeight": { "type": "integer" },
          "commands": {
            "type": "bytes",
            "maxLength": 16384,
            "description": "Command log. One byte per command (Direction 0-7, Wait=8, Descend=9, etc.)."
          },
          "timing": {
            "type": "bytes",
            "description": "Inter-command intervals in centiseconds (1 byte each, capped at 255 = 2.55s). Same length as commands. Enables timing analysis for anomaly detection."
          },
          "result": {
            "type": "object",
            "required": ["turns", "depth", "kills", "won"],
            "properties": {
              "turns": { "type": "integer" },
              "depth": { "type": "integer" },
              "kills": { "type": "integer" },
              "won": { "type": "boolean" }
            }
          },
          "gameVersion": {
            "type": "string",
            "maxLength": 64,
            "description": "Game binary version hash. Replays only verify against the same version."
          },
          "serverDid": {
            "type": "string",
            "description": "DID of the SSH server that attested this replay. Verify the attestation signature against this server's public key."
          },
          "attestation": {
            "type": "bytes",
            "maxLength": 256,
            "description": "Server signature over hash(seed ‖ commands ‖ timing ‖ playerDid ‖ gameVersion). ES256 (P-256 ECDSA)."
          },
          "challengeRef": {
            "type": "string",
            "format": "at-uri",
            "description": "Optional reference to a challenge record that specified the seed. Proves the player didn't choose their own seed."
          },
          "createdAt": {
            "type": "string",
            "format": "datetime"
          }
        }
      }
    }
  }
}
```

### Trust Model

| Property | Unattested replay | Server-attested replay |
|----------|:-:|:-:|
| Proves correct outcome | Yes | Yes |
| Proves human played in real-time | No | Yes (server witnessed inputs) |
| Prevents solver/bot play | No | Yes (server controls execution) |
| Prevents replay editing / save-scumming | No | Yes (server records sequentially) |
| Prevents seed cherry-picking | Only with challenge seeds | Only with challenge seeds |
| Works from C64 / terminal (local play) | Yes | No (no trusted observer) |
| Works from SSH server | Yes | Yes |
| Requires server infrastructure | No | Yes (server must sign + publish key) |

The server doesn't need to be a Bluesky relay or AppView — it just needs a keypair and a way for verifiers to fetch the public key. The server's DID (in `serverDid`) is the lookup mechanism.

### Challenge Seeds

For competitive integrity, the seed should not be chosen by the player. A challenge record specifies the seed, and players reference it:

```json
{
  "id": "com.example.roguelike.replay.challenge",
  "defs": {
    "main": {
      "type": "record",
      "key": "tid",
      "record": {
        "type": "object",
        "required": ["seed", "mapWidth", "mapHeight", "gameVersion", "createdAt"],
        "properties": {
          "seed": { "type": "integer" },
          "mapWidth": { "type": "integer" },
          "mapHeight": { "type": "integer" },
          "gameVersion": {
            "type": "string",
            "description": "Replays must match this version to be valid entries."
          },
          "name": {
            "type": "string",
            "maxGraphemes": 128,
            "description": "Challenge name, e.g. 'Daily Challenge — March 17'."
          },
          "expiresAt": {
            "type": "string",
            "format": "datetime",
            "description": "Optional deadline for submissions."
          },
          "createdAt": { "type": "string", "format": "datetime" }
        }
      }
    }
  }
}
```

Daily challenges could derive seeds from a public source (e.g., `seed = truncate_u16(SHA-256(date_string))`) so no trusted party is needed to post them. Anyone can independently compute the daily seed and verify a challenge record is honest.

### What This Doesn't Solve

- **Colluding servers.** A malicious server operator could sign bot-generated replays. Trust depends on the server's reputation — the same model as speedrun.com trusting specific platforms.
- **Player assistance tools.** A human could use an overlay showing optimal moves while playing on the SSH server. The server sees human-speed, human-pattern inputs but the decisions are computer-assisted. Statistical anomaly detection (superhuman damage avoidance, optimal pathfinding) is the only mitigation, and it's heuristic.
- **Local play attestation.** Terminal and C64 versions have no trusted observer. Replays from local play are inherently unattested. This is fine — local replays are for personal sharing and demo mode (#135), not competitive leaderboards.

### Integration with SSH Server

The SSH server already has the infrastructure:

- **Authentication:** Player identity via SSH login + optional ATProto DID linkage (atproto.md)
- **Command recording:** Add a command buffer to the session (same as `DevSession.command_log` but for production play)
- **Timing:** Record `Instant::now()` delta between each command received on the SSH channel
- **Signing:** Server keypair (the SSH host key could double for this, or a separate signing key)
- **Publishing:** Reuses `PdsClient` from `AtprotoFrameSink` to post the attested replay to the player's PDS on game end

The flow on game completion:

1. Player dies or wins → game loop returns
2. Session builds replay: seed + commands + timing + result
3. Server signs the replay with its key
4. If player has ATProto identity: post `attestedRun` record to player's PDS
5. If no ATProto identity: offer to display the replay as a shareable seed code (fallback)

## References

- [Jetstream documentation](https://docs.bsky.app/blog/jetstream) — Bluesky's lightweight event stream
- [Jetstream GitHub](https://github.com/bluesky-social/jetstream) — Source and self-hosting guide
- [AT Protocol Lexicon spec](https://atproto.com/specs/lexicon) — Custom record schema definition
- [tokio-tungstenite](https://crates.io/crates/tokio-tungstenite) — Async WebSocket client for Rust
- [AT Protocol repo spec](https://atproto.com/specs/repository) — TID format, record key ordering
