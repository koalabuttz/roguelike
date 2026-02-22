# C64 AT Protocol Bridge

Design for a self-hostable bridge server that connects a Commodore 64 (via Ultimate64/1541 Ultimate II+ Ethernet) to the AT Protocol ecosystem, enabling PDS save storage and federated spectating for the C64 port of the roguelike.

> **Status:** Unimplemented future design. Depends on both the C64 port (`crates/c64`, Tier 5 on the [roadmap](../roadmap.md)) and AT Protocol integration ([atproto.md](atproto.md), Tier 3). This document captures the architecture for when both prerequisites exist.

## Motivation

The C64 is classified as a [constrained platform](../architecture/cross-platform.md#frontend-crates) — no `SaveBackend` dependency, no HTTP stack, no TLS, no JSON parsing, 64KB RAM, 1MHz 6510 CPU. It cannot participate in the atproto ecosystem directly. Every other connected platform (SSH, terminal, web) talks to the user's PDS natively via XRPC. The C64 cannot.

But the game's atproto features — portable saves, federated spectating, Bluesky identity — are valuable regardless of which platform you play on. A C64 player should be able to:

1. **Save games to their PDS** — playable later on SSH or web with the same atproto identity
2. **Broadcast spectate frames** — watchable by anyone via Jetstream, just like SSH players
3. **Load saves from their PDS** — continue a game started on another platform

The bridge server makes this possible by sitting between the C64's simple binary TCP protocol and the full atproto stack.

## Goals

1. **Transparent atproto proxy.** The C64 sends tiny binary packets; the bridge translates them to/from atproto XRPC calls. The C64 never knows atproto exists.
2. **Self-hostable.** Single Docker container, runs on the same LAN as the C64 + Ultimate64. No cloud dependency beyond the user's PDS.
3. **Reuse existing lexicons.** The bridge writes the same `save.gameState`, `save.settings`, `spectate.frame`, and `spectate.session` records defined in [atproto.md](atproto.md#lexicon-design) and [atproto-spectating.md](atproto-spectating.md#lexicon-design). Saves and spectate frames are indistinguishable from those produced by SSH or web clients.
4. **Minimal C64-side complexity.** The C64 binary speaks a fixed protocol over raw TCP. No TLS, no JSON, no OAuth, no strings longer than 255 bytes. All heavy lifting happens on the bridge.
5. **Offline resilience.** The bridge caches saves locally. If the PDS is unreachable, saves persist in the bridge's local storage and sync when connectivity returns.

## Non-Goals

- **Running atproto code on the C64.** The C64 has no TLS stack and cannot make HTTPS requests. The bridge is the atproto client, not the C64.
- **Replacing the C64's native save mechanism.** The C64 port will have its own hardware-appropriate save system (tape/floppy/SD via Ultimate64). The bridge provides an *additional* save path to the PDS, not the only one.
- **General-purpose C64 networking.** The bridge speaks one protocol for one game. It's not a generic internet gateway.
- **Multi-game support.** One bridge instance serves one game (this roguelike). Other C64 games would need their own bridge or a more general solution.

## Background: Ultimate64 Networking

The [Ultimate64](https://ultimate64.com/) (and 1541 Ultimate II+) is an FPGA cartridge/board for the Commodore 64 that provides, among other features, an Ethernet port with a lightweight TCP/IP stack accessible to 6502 code.

**Key constraints of the Ultimate64 network API:**

| Constraint | Impact on bridge design |
|------------|------------------------|
| Raw TCP only (no TLS) | Bridge must be on the same trusted LAN; all traffic is plaintext |
| Limited socket API | Simple connect/send/recv; no multiplexing, no async |
| ~1-2 KB/s effective throughput | Packets must be tiny; no bulk JSON transfers |
| No DNS resolution | C64 connects by IP address (configured at build time or via menu) |
| Single connection at a time | Bridge must handle one C64 client per port |

The C64 side uses the Ultimate64's socket API (typically via a thin 6502 library) to open a TCP connection to `bridge-ip:6510` and exchange fixed-format binary messages.

## Architecture

```
Commodore 64                    Bridge Server                    AT Protocol
+ Ultimate64                    (Docker container)               (User's PDS)
┌──────────────┐    TCP:6510    ┌──────────────────┐    HTTPS    ┌──────────────┐
│  6502 game   │◄──────────────►│  Protocol handler │◄──────────►│  User's PDS  │
│  binary proto│   LAN, plain   │  (Rust or Python) │   XRPC     │  (bsky.social│
│  ~64-256 byte│   packets      │                   │   +DPoP    │   or self-   │
│  messages    │                │  Local save cache  │            │   hosted)    │
└──────────────┘                │  Config (TOML/env) │            └──────────────┘
                                │  Web UI (:8080)    │
                                └──────────────────┘
                                         │
                                    Docker volume
                                    (persistent
                                     save cache)
```

### Component Responsibilities

**C64 game binary:**
- Sends save data as binary packets (see [Wire Protocol](#wire-protocol))
- Receives save data and acknowledgements
- Sends spectate frames (compressed ASCII) after each turn
- Connects to a single bridge IP:port on startup

**Bridge server:**
- Listens on TCP port 6510 for C64 connections
- Authenticates with the user's PDS using stored atproto credentials (configured once via web UI or environment variables)
- Translates binary save packets → atproto `putRecord`/`uploadBlob` XRPC calls using the [save lexicons](atproto.md#lexicon-design)
- Translates binary spectate frames → atproto `createRecord` calls using the [spectate lexicons](atproto-spectating.md#lexicon-design)
- Caches saves locally in a Docker volume for offline resilience
- Provides an optional web UI on port 8080 for configuration and live spectate viewing

**User's PDS:**
- Stores save records and spectate frame records exactly as defined in the existing lexicon designs
- Records are indistinguishable from those created by SSH or web frontends

### Key Design Decisions

**Bridge is external to the Rust workspace.** The bridge is a standalone project (separate repo or `tools/c64-bridge/` directory), not a crate in the roguelike workspace. It depends on the lexicon definitions conceptually but not as a Rust dependency. This keeps `crates/c64` free of networking code and the workspace free of bridge-specific dependencies (reqwest, tokio, etc. are already in `crates/atproto`, but the bridge doesn't share that code — it's a separate binary with a different deployment model).

**Alternative: shared lexicon crate.** If both `crates/atproto` and the bridge need identical record struct definitions, extract a `roguelike-lexicon` crate with just the serde structs. But this is premature — the bridge can define its own matching structs or use raw JSON, and lexicon definitions are small enough to duplicate.

**TCP, not UDP.** The C64's Ultimate64 TCP stack is more reliable and better documented than UDP. Save data must not be lost. The overhead of TCP is negligible at these data rates (~1-2 KB/s).

**Port 6510.** The MOS 6510 is the C64's CPU. A fun default, easily configurable.

**Credentials stored on the bridge, not the C64.** The C64 cannot perform OAuth. The bridge stores atproto credentials (access token, refresh token, DPoP key) obtained during initial setup via the web UI. Token refresh happens on the bridge. If the refresh token expires, the web UI prompts for re-authentication.

## Wire Protocol

The C64-to-bridge protocol is a simple binary format designed for the constraints of 6502 assembly:

- **Little-endian** (6502 native byte order)
- **Fixed headers** — 1-byte message type, 2-byte payload length
- **No strings > 255 bytes** in a single field
- **No JSON** — structured data uses fixed-offset binary fields
- **Request-response** — C64 sends a message, waits for a response before sending the next

### Message Format

```
┌──────┬────────┬─────────────────────┐
│ Type │ Length │ Payload             │
│ 1B   │ 2B LE │ 0–65535 bytes       │
└──────┴────────┴─────────────────────┘
```

### Message Types

#### C64 → Bridge

| Type | Name | Payload | Response |
|------|------|---------|----------|
| `0x01` | `SAVE_GAME` | `slot(1B) + save_data(NB)` | `ACK` or `ERR` |
| `0x02` | `LOAD_GAME` | `slot(1B)` | `SAVE_DATA` or `ERR` |
| `0x03` | `DELETE_SAVE` | `slot(1B)` | `ACK` or `ERR` |
| `0x04` | `LIST_SAVES` | (empty) | `SAVE_LIST` |
| `0x05` | `SPECTATE_FRAME` | `frame_data(NB)` | `ACK` |
| `0x06` | `SESSION_START` | `seed(4B) + width(1B) + height(1B)` | `ACK` |
| `0x07` | `SESSION_END` | `reason(1B) + final_turn(2B)` | `ACK` |
| `0x08` | `LOAD_SETTINGS` | (empty) | `SETTINGS_DATA` or `ERR` |
| `0x09` | `SAVE_SETTINGS` | `settings_data(NB)` | `ACK` or `ERR` |
| `0x0A` | `PING` | (empty) | `PONG` |

#### Bridge → C64

| Type | Name | Payload |
|------|------|---------|
| `0x80` | `ACK` | (empty) |
| `0x81` | `ERR` | `error_code(1B)` |
| `0x82` | `SAVE_DATA` | `save_data(NB)` |
| `0x83` | `SAVE_LIST` | `count(1B) + entries(N × SLOT_ENTRY)` |
| `0x84` | `SETTINGS_DATA` | `settings_data(NB)` |
| `0x85` | `PONG` | (empty) |

#### Slot Entry (in `SAVE_LIST`)

```
┌──────┬───────┬────────┬──────────┬──────────────┐
│ Slot │ Turns │ HP     │ MaxHP    │ Explored%    │
│ 1B   │ 2B LE │ 1B     │ 1B       │ 1B           │
└──────┴───────┴────────┴──────────┴──────────────┘
= 6 bytes per slot
```

This maps directly to the `SlotMetadata` fields in the save lexicon. The bridge constructs the full atproto record (with `savedAt` timestamp, `seedCode` string, etc.) from the binary metadata plus its own context.

#### Save Slots

| Slot byte | Meaning | Atproto rkey |
|-----------|---------|--------------|
| `0x00` | Autosave | `autosave` |
| `0x01`–`0x05` | Manual slots 1–5 | `slot-1` through `slot-5` |

### Save Data Format

The C64 port uses a compact binary save format (not JSON — JSON is too expensive to parse on a 1MHz CPU). The bridge must understand this format to extract metadata for the atproto record fields (`turnCount`, `playerHp`, etc.).

**Option A: Metadata header + opaque blob.** The C64 prepends a fixed-size metadata header to the save data:

```
┌───────────┬────────┬──────────┬──────────┬─────────────┬──────────────┐
│ Turn count│ HP     │ MaxHP    │ Explored%│ Seed        │ Save blob    │
│ 2B LE     │ 1B     │ 1B       │ 1B       │ 4B LE       │ remaining    │
└───────────┴────────┴──────────┴──────────┴─────────────┴──────────────┘
```

The bridge reads the header to populate atproto record fields, then uploads the entire payload (header + blob) as the atproto blob. When loading, the bridge downloads the blob and sends it back as-is — the C64 reads its own header format.

**Option B: Bridge extracts metadata from the binary format.** Requires the bridge to understand the C64's save serialization. More fragile — any save format change requires updating both the C64 code and the bridge. Option A is preferred.

### Spectate Frame Format

The C64's screen is 40×25 characters. A raw frame is 1,000 bytes — within the C64's transmission budget but larger than necessary since most tiles don't change between turns.

**Full frame (simple, recommended for v1):**

```
┌──────┬───────────────────────────────────┐
│ Turn │ Screen data                       │
│ 2B   │ 1000B (40×25 PETSCII characters)  │
└──────┴───────────────────────────────────┘
= 1,002 bytes per frame
```

The bridge converts PETSCII screen data to ASCII for the atproto `spectate.frame` record's `map` field. PETSCII→ASCII mapping is a static lookup table on the bridge side.

**Delta frame (optimization, v2):**

```
┌──────┬───────┬────────────────────────────┐
│ Turn │ Count │ (x, y, char) triples       │
│ 2B   │ 1B    │ N × 3B                     │
└──────┴───────┴────────────────────────────┘
= 3 + (N × 3) bytes, where N = changed tiles
```

A typical turn changes 5-20 tiles, so delta frames are 18-63 bytes vs. 1,002 for full frames. The bridge maintains the full screen buffer and applies deltas before constructing the atproto record.

### Error Codes

| Code | Meaning |
|------|---------|
| `0x01` | Slot not found (no save in requested slot) |
| `0x02` | PDS unreachable (bridge will retry; save is cached locally) |
| `0x03` | Authentication expired (re-auth needed via web UI) |
| `0x04` | Save too large (exceeds blob size limit) |
| `0xFF` | Unknown error |

### Flow Diagram

```
C64                              Bridge                         PDS
 │                                 │                              │
 │──SAVE_GAME(slot=1, data)──────►│                              │
 │                                 │──uploadBlob(data)──────────►│
 │                                 │◄─────────blob_ref───────────│
 │                                 │──putRecord(slot-1, ...)────►│
 │                                 │◄─────────ok─────────────────│
 │◄──────────ACK──────────────────│                              │
 │                                 │                              │
 │──LOAD_GAME(slot=1)────────────►│                              │
 │                                 │──getRecord(slot-1)─────────►│
 │                                 │◄─────────record─────────────│
 │                                 │──getBlob(cid)──────────────►│
 │                                 │◄─────────blob_data──────────│
 │◄──────SAVE_DATA(data)─────────│                              │
 │                                 │                              │
 │──SPECTATE_FRAME(screen)───────►│                              │
 │                                 │──createRecord(frame)───────►│
 │◄──────────ACK──────────────────│                              │
```

## Bridge Server Implementation

### Technology Choice

| Option | Pros | Cons |
|--------|------|------|
| **Rust** | Shares types/knowledge with main project; single binary; low memory | Heavier dev effort for a companion tool |
| **Python** | Fast to prototype; rich atproto libraries (`atproto` package); easy Docker image | Runtime dependency; larger image; GIL |

**Recommendation:** Python for the initial implementation. The `atproto` Python package handles OAuth, DPoP, XRPC, and token refresh out of the box. The bridge is I/O-bound (waiting for C64 packets and PDS responses), so Python's performance is fine. A Rust rewrite is straightforward if needed later.

If `crates/atproto` (the Rust atproto crate from the main project) matures first, a Rust bridge that imports it directly becomes attractive — shared lexicon structs, shared XRPC client, etc.

### Docker Deployment

```yaml
# docker-compose.yml
services:
  c64-roguelike-bridge:
    image: ghcr.io/koalabuttz/c64-roguelike-bridge:latest
    build: .
    ports:
      - "6510:6510"    # C64 binary protocol (TCP)
      - "8080:8080"    # Web UI (configuration + spectate viewer)
    environment:
      - ATPROTO_HANDLE=player.bsky.social
      - ATPROTO_PDS=https://bsky.social
      # App password or stored OAuth tokens (see Authentication section)
      - ATPROTO_APP_PASSWORD=xxxx-xxxx-xxxx-xxxx
    volumes:
      - bridge-data:/app/data    # Local save cache + token storage
    restart: unless-stopped

volumes:
  bridge-data:
```

**Image size target:** < 50MB. Python slim base + `atproto` package + bridge code.

**Resource requirements:** Minimal. The bridge is mostly idle, waking for C64 packets (a few per minute at most) and PDS sync. A Raspberry Pi on the same LAN as the C64 is the ideal host.

### Authentication

The bridge needs atproto credentials to write to the user's PDS. Unlike the SSH/web frontends (which use full OAuth with browser redirect), the bridge is a headless service.

**Option A: App password (simplest, recommended for v1).**

Bluesky supports [app passwords](https://bsky.app/settings/app-passwords) — limited-scope credentials that don't require the full OAuth dance. The user creates an app password in their Bluesky settings and provides it to the bridge via environment variable or web UI. The bridge uses `com.atproto.server.createSession` to authenticate.

Pros: Dead simple. No browser redirect, no DPoP, no PKCE. Works immediately.
Cons: App passwords are a Bluesky-specific convenience, not part of the AT Protocol spec. May not work with all PDS implementations.

**Option B: OAuth Device Flow (future, if atproto adds support).**

AT Protocol currently mandates Authorization Code + PKCE — no device flow (RFC 8628). If device flow is added in the future, the bridge could display a code on the web UI, the user enters it on their phone/computer, and the bridge receives tokens. This is the standard pattern for headless devices.

**Option C: OAuth via bridge web UI (correct but more complex).**

The bridge's web UI (port 8080) acts as the OAuth client. The user clicks "Login with Bluesky" in the browser, completes the OAuth flow, and the bridge stores the resulting tokens. This is identical to the SSH server's OAuth flow described in [atproto.md](atproto.md#ssh-frontend), except the callback URL points to the bridge's HTTP server.

This is the most correct approach and works with any PDS. Recommended upgrade path from Option A once the atproto OAuth infrastructure is implemented.

**Token storage:**

Tokens are stored in the Docker volume (`/app/data/auth.json`), encrypted at rest with a key derived from a user-provided passphrase (or unencrypted if the user accepts the risk — it's their LAN, their tokens).

Token refresh happens automatically. If the refresh token expires (2-week limit for public OAuth clients; app passwords don't expire), the web UI shows a re-authentication prompt.

### Web UI

A lightweight web interface on port 8080 provides:

| Page | Purpose |
|------|---------|
| `/` | Dashboard: connection status (C64 connected?), last save time, PDS sync status |
| `/auth` | Atproto authentication setup (app password entry or OAuth flow) |
| `/saves` | List of cached saves with metadata (slot, turns, HP, timestamp) |
| `/spectate` | Live spectate viewer — renders the latest frame as ASCII in a `<pre>` element |
| `/config` | Bridge configuration (port, PDS URL, spectate enable/disable, throttle interval) |

The spectate page is publicly accessible (no auth required) — anyone on the LAN can watch. This mirrors the [atproto spectating design](atproto-spectating.md#no-authentication-required-for-consumers) where spectate consumption is unauthenticated.

### Local Save Cache

The bridge maintains a local save cache in the Docker volume:

```
/app/data/
  auth.json              # Stored atproto credentials
  config.toml            # Bridge configuration
  cache/
    autosave.bin         # Cached C64 binary save data
    slot-1.bin
    slot-2.bin
    ...
    slot-5.bin
  sync_state.json        # Tracks which saves are dirty (not yet synced to PDS)
```

**Write flow:**
1. C64 sends `SAVE_GAME` → bridge writes to local cache immediately → sends `ACK` to C64
2. Bridge asynchronously uploads to PDS (blob + record)
3. On success, marks the slot as clean in `sync_state.json`

**Read flow:**
1. C64 sends `LOAD_GAME` → bridge checks local cache first
2. If cache hit and clean, returns cached data immediately
3. If cache miss, fetches from PDS, caches locally, returns to C64

This is the same caching strategy described in [atproto.md](atproto.md#caching-strategy), adapted for the bridge's filesystem storage.

### Spectate Frame Publishing

The bridge converts C64 screen data to atproto spectate records:

1. C64 sends `SPECTATE_FRAME` with raw PETSCII screen data
2. Bridge converts PETSCII → ASCII using a static lookup table
3. Bridge splits the 40×25 screen into 25 lines of 40 characters each
4. Bridge constructs a `spectate.frame` record matching the [lexicon](atproto-spectating.md#comexampleroguelikespectateframe):
   ```json
   {
     "map": ["<line1>", "<line2>", ...],
     "status": "<extracted from last map line or separate field>",
     "messages": [],
     "turn": 142,
     "seedCode": "abc123",
     "createdAt": "2026-02-19T12:00:00Z"
   }
   ```
5. Bridge publishes via `createRecord` XRPC call

**Throttling:** Same as [AtprotoFrameSink](atproto-spectating.md#atprotoframesink) — the bridge drops frames arriving faster than a configurable minimum interval (default 500ms). The C64's 1MHz clock and human play speed make this unlikely to trigger, but it protects against rapid-fire saves or test automation.

**Session lifecycle:** The bridge creates a `spectate.session` record on `SESSION_START` and updates it on `SESSION_END`, matching the [session lifecycle](atproto-spectating.md#comexampleroguelikespectatesession) exactly.

## C64-Side Implementation

### Memory Budget

The C64 has 64KB of RAM, of which ~38KB is typically available to a program. The networking code must fit within this alongside the game itself.

| Component | Estimated size |
|-----------|---------------|
| TCP send/receive buffer | 1,024 bytes |
| Protocol message assembly | 256 bytes |
| Screen buffer for spectate | 1,000 bytes (40×25) |
| Connection state | 16 bytes |
| **Total networking overhead** | **~2.3 KB** |

This is acceptable — the game's own state ([simulation budget](../architecture/simulation.md#6-platform-scaling-via-simbudget): 32 entities × 8 bytes = 256 bytes for entities, plus map data) leaves plenty of room.

### 6502 Implementation Notes

The C64 networking code is pure 6502 assembly (or compiled from Rust via [rust-mos](https://github.com/mrk-its/rust-mos) if the C64 port uses that toolchain). Key considerations:

- **Blocking I/O is fine.** The game is turn-based. After the player acts, the game can block on a bridge response for save/load operations. No async required.
- **Spectate frames are fire-and-forget.** Send the frame, don't wait for ACK before the next game tick. If the bridge is slow, frames are dropped — the spectate contract is best-effort ([atproto-spectating.md](atproto-spectating.md#atprotoframesink)).
- **Save operations block.** When the player saves, the game waits for ACK. This matches the existing save UX (brief pause is expected).
- **Bridge IP is hardcoded or menu-configurable.** The simplest approach is a compile-time constant. A nicer approach is a settings menu entry where the user enters the IP (4 bytes, displayed as dotted decimal).

### Integration with C64 Game Loop

The C64 port's game loop (which will live in `crates/c64/` and depend only on `roguelike-core`) gains optional bridge calls:

```
Player input
  → step() (core game logic)
  → render to screen (C64 VIC-II)
  → if bridge_connected:
      send SPECTATE_FRAME (non-blocking, best-effort)
  → if save requested:
      send SAVE_GAME, wait for ACK
  → if load requested:
      send LOAD_GAME, wait for SAVE_DATA
```

If the bridge is not connected (no Ultimate64, or bridge not running), all networking calls are no-ops. The game plays identically — saves go to the C64's local storage (floppy/SD) only.

## Relationship to Existing Designs

| Document | Relationship |
|----------|-------------|
| [atproto.md](atproto.md) | The bridge reuses the save lexicons (`save.gameState`, `save.settings`) and the `PdsSaveBackend` concept. It performs the same XRPC operations (uploadBlob, putRecord, getRecord, getBlob) but from a standalone service rather than an in-process Rust crate. The bridge uses app passwords (initially) instead of the full OAuth flow, diverging from atproto.md's OAuth-only stance — this is a pragmatic concession for a headless device. |
| [atproto-spectating.md](atproto-spectating.md) | The bridge implements the producer side of atproto spectating, publishing `spectate.frame` and `spectate.session` records. Frames are produced from C64 PETSCII screen data instead of `render_frame()`, but the resulting atproto records are schema-identical. Consumers (Jetstream subscribers) cannot distinguish a C64-produced frame from an SSH-produced one. |
| [cross-platform.md](../architecture/cross-platform.md) | The C64 crate depends only on `roguelike-core` — no `roguelike-saves`, no `roguelike-atproto`, no networking crates. The bridge is an external companion, not a workspace crate. This preserves the [constrained platform boundary](../architecture/cross-platform.md#frontend-crates): `NullFrameSink` in the C64 crate, atproto frame publishing in the bridge. |
| [simulation.md](../architecture/simulation.md) | The C64's `SimBudget` (32 entities, 10-tile active radius, event-only simulation) determines the maximum save size. Smaller game state = smaller save packets = faster bridge transfers. The bridge doesn't need to understand simulation details — it treats save data as an opaque blob with a metadata header. |
| [spectator-mode.md](spectator-mode.md) | The bridge provides the C64's path to remote spectating. Without it, the C64 has no spectating capability at all (no file system for `FileFrameSink`, no network for `AtprotoFrameSink`). The bridge fills this gap with an external service. |

## Configuration

### Bridge Configuration (`config.toml`)

```toml
[bridge]
listen_host = "0.0.0.0"
listen_port = 6510
web_ui_port = 8080

[atproto]
handle = "player.bsky.social"
pds_url = "https://bsky.social"
# lexicon_namespace = "com.example.roguelike"  # TBD, same open question as atproto.md

[spectate]
enabled = true
min_publish_interval_ms = 500    # Throttle for rapid frames

[cache]
data_dir = "/app/data"
sync_interval_secs = 30          # How often to flush dirty saves to PDS
```

All values are overridable via environment variables with the `C64_BRIDGE_` prefix (e.g., `C64_BRIDGE_LISTEN_PORT=6510`).

### Docker Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `ATPROTO_HANDLE` | (required) | User's atproto handle |
| `ATPROTO_PDS` | `https://bsky.social` | PDS URL |
| `ATPROTO_APP_PASSWORD` | (required for app password auth) | Bluesky app password |
| `C64_BRIDGE_LISTEN_PORT` | `6510` | C64 TCP listen port |
| `C64_BRIDGE_WEB_PORT` | `8080` | Web UI port |
| `C64_BRIDGE_SPECTATE` | `true` | Enable spectate frame publishing |
| `TZ` | `UTC` | Timezone for timestamps |

## Multi-C64 Support

For retro LAN parties or shared setups, multiple C64s can share one bridge:

**Option A: Multiple bridge instances.** One Docker container per C64/atproto account. Different host ports mapped to each container's 6510. Simplest, most isolated.

```yaml
services:
  bridge-alice:
    image: ghcr.io/koalabuttz/c64-roguelike-bridge:latest
    ports: ["6510:6510", "8080:8080"]
    environment:
      ATPROTO_HANDLE: alice.bsky.social
      ATPROTO_APP_PASSWORD: xxxx

  bridge-bob:
    image: ghcr.io/koalabuttz/c64-roguelike-bridge:latest
    ports: ["6511:6510", "8081:8080"]
    environment:
      ATPROTO_HANDLE: bob.bsky.social
      ATPROTO_APP_PASSWORD: yyyy
```

**Option B: Single bridge, multiple accounts.** The bridge accepts multiple C64 connections on different ports (or the same port, with a login handshake). More complex, lower resource usage. Not recommended for v1.

## Implementation Phases

### Phase 0: Protocol Design and Mockup

**Effort:** S (hours)

Finalize the wire protocol. Write a mock bridge in Python that accepts TCP connections, prints received messages, and sends canned responses. Test with `netcat` or a simple C64 emulator script. No atproto integration yet.

### Phase 1: Bridge with Local Cache Only

**Effort:** M (days)

Implement the bridge server (Python) with:
- TCP listener for the binary protocol
- Local filesystem save cache (read/write `.bin` files)
- Web UI dashboard showing connection status and save list
- Docker packaging

At this point, the bridge is a network save server — the C64 saves/loads over the LAN, but nothing goes to the PDS yet. Useful for testing the C64 networking code independently of atproto.

### Phase 2: PDS Save Integration

**Effort:** M (days). Depends on [atproto.md Phase 2](atproto.md#phase-2-pds-save-backend) (lexicon definitions must be finalized).

Add atproto integration:
- App password authentication (`createSession`)
- `uploadBlob` + `putRecord` for save writes
- `getRecord` + `getBlob` for save reads
- `listRecords` for save slot listing
- Async PDS sync with dirty tracking
- Token refresh and error handling
- Web UI authentication page

### Phase 3: Spectate Frame Publishing

**Effort:** M (days). Depends on [atproto-spectating.md Phase 1](atproto-spectating.md#phase-1-lexicon-design-and-frame-publishing) (spectate lexicons must be defined).

Add spectate support:
- PETSCII → ASCII conversion
- `spectate.frame` record creation
- `spectate.session` lifecycle management
- Throttling
- Web UI live spectate viewer (local rendering of latest frame)

### Phase 4: OAuth Upgrade

**Effort:** M (days). Depends on [atproto.md Phase 1](atproto.md#phase-1-http-server--oauth-ssh) (OAuth infrastructure).

Replace app password auth with proper OAuth:
- Web UI OAuth flow (browser redirect, callback handler)
- DPoP key management
- Token storage with encryption
- Works with any PDS, not just Bluesky

### Phase 5: C64-Side Networking Code

**Effort:** L (week+). Depends on the C64 port existing (`crates/c64`).

Write the 6502 networking library:
- Ultimate64 socket API wrapper
- Binary protocol message assembly/parsing
- Integration with the C64 game loop
- Bridge IP configuration menu
- Graceful handling of bridge disconnection

This is the most uncertain phase — it depends on the C64 port's toolchain (rust-mos vs. hand-written 6502 assembly) and the Ultimate64's API documentation.

## Open Questions

1. **Lexicon namespace.** Same open question as [atproto.md](atproto.md#open-questions) and [atproto-spectating.md](atproto-spectating.md#open-questions). Must be decided before Phase 2.

2. **C64 save format.** The C64 port's save serialization format isn't designed yet. The bridge needs to understand at least the metadata header. This should be co-designed with the C64 port to keep the header simple and fixed-size.

3. **PETSCII → ASCII fidelity.** The C64 uses PETSCII, which has different code points from ASCII. The game's glyphs (`@`, `g`, `o`, `T`, `#`, `.`, `%`) are all in the shared ASCII/PETSCII range, but PETSCII box-drawing characters and other C64-specific glyphs won't round-trip. The bridge should map unknown PETSCII codes to a placeholder (e.g., `?`).

4. **Ultimate64 API stability.** The Ultimate64's networking API is not extensively documented. The bridge protocol should be tested against actual hardware, not just emulators. A fallback using a Raspberry Pi with a serial-to-TCP bridge (connecting to the C64's user port) could provide an alternative if the Ultimate64's Ethernet proves unreliable.

5. **Bridge discovery.** How does the C64 find the bridge on the LAN? Options: (a) hardcoded IP at compile time (simplest); (b) menu entry for IP configuration; (c) UDP broadcast discovery (complex, unlikely worth it). Recommendation: (b), stored in the C64's settings.

6. **App password deprecation.** Bluesky has discussed deprecating app passwords in favor of OAuth-only. If this happens before the bridge ships, Phase 2 and Phase 4 merge — the bridge must implement OAuth from the start. Monitor the [atproto auth discussions](https://github.com/bluesky-social/atproto/discussions/4118).

7. **Save data size.** The C64's compact save format will be much smaller than the JSON saves used by connected platforms (likely 1-5 KB vs. 20-80 KB). This is fine for the bridge — smaller blobs mean faster PDS uploads. But it means C64 saves are not directly loadable by SSH/web clients, which expect JSON `GameState`. Cross-platform save portability would require a conversion step (bridge converts C64 binary ↔ JSON when syncing to PDS). This is a significant design decision: do C64 saves use the same PDS records in a different format, or separate records entirely?

## References

- [Ultimate64 documentation](https://ultimate64.com/Documentation) — Hardware and software documentation
- [1541 Ultimate II+ networking](https://github.com/GideonZ/1541ultimate) — Source and API for the Ultimate series
- [rust-mos](https://github.com/mrk-its/rust-mos) — Rust compiler targeting MOS 6502 (C64, NES, etc.)
- [AT Protocol specification](https://atproto.com/specs) — Full protocol specs
- [Bluesky app passwords](https://bsky.app/settings/app-passwords) — Creating app passwords for headless services
- [atproto Python SDK](https://github.com/MarshalX/atproto) — Python library for AT Protocol (recommended for bridge implementation)
- [PETSCII character set](https://www.c64-wiki.com/wiki/PETSCII) — Character encoding used by the C64
- [MOS 6510 datasheet](https://www.c64-wiki.com/wiki/MOS_6510) — C64 CPU reference
