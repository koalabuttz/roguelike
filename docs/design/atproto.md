# AT Protocol (Bluesky) Integration

Design for adding AT Protocol identity and portable save storage to the roguelike, enabling cross-platform play across terminal, SSH, and web (WASM) frontends with saves that follow the player.

## Goals

1. **Bluesky login** on terminal, SSH, and web frontends via AT Protocol OAuth.
2. **Portable saves** stored in the user's AT Protocol PDS repository, accessible from any server or client.
3. **Account migration** for existing username/password players to link their atproto identity.
4. **Federation-friendly**: a player's saves work on any server running this game, not just the original.

## Non-goals

- Replacing username/password auth entirely (it remains as a fallback for users without atproto accounts).
- Social features (posting game results to Bluesky, leaderboards). Those are separate future work.
- Implementing a custom PDS. We use the player's existing PDS (e.g., Bluesky's hosted PDS).

## Background: AT Protocol OAuth

AT Protocol OAuth is a strict profile of OAuth 2.0 that mandates:

- **Authorization Code + PKCE (S256)** as the only grant type. No device flow (RFC 8628), no implicit grant.
- **DPoP** (Demonstration of Proof-of-Possession) with mandatory server nonces. Every token request includes a signed JWT proving the client holds a private key.
- **PAR** (Pushed Authorization Requests). The client posts auth parameters to a PAR endpoint before redirecting the user, receiving a `request_uri` in return.
- **Client metadata documents** instead of traditional client registration. The `client_id` is a URL pointing to a publicly-hosted JSON document describing the client.
- **ES256** (P-256 ECDSA) as the baseline signing algorithm for DPoP proofs.

Handle resolution follows a multi-step chain:

```
handle (e.g., alice.bsky.social)
  -> DID resolution (DNS TXT or HTTPS well-known)
    -> DID document (contains PDS service endpoint)
      -> PDS resource server metadata (/.well-known/oauth-protected-resource)
        -> Authorization server URL
          -> AS metadata (/.well-known/oauth-authorization-server)
            -> PAR endpoint, token endpoint, JWKS, etc.
```

The DID (e.g., `did:plc:abc123`) is the stable, permanent identity. Handles can change; DIDs cannot.

## Architecture

```
                           +-------------------------+
                           |   Bluesky Auth Server   |
                           |   (bsky.social or       |
                           |    user's AS)           |
                           +--+------+----------+----+
                         PAR+ |      | browser   | browser
                        token |      | redirect  | redirect
                              |      |           | (loopback)
                +-------------+    +-+--------+  |
                |                  |           |  |
                v                  v           |  v
+---------------+----+   +--------+--------+  | +------------------+
|  SSH Server        |   |  HTTP Server    |  | |  Terminal Binary  |
|  (russh :2222)     |   |  (axum :443)    |  | |  (local)         |
|                    |   |                 |  | |                  |
|  lobby: atproto    |   | /oauth/callback |  | |  loopback OAuth  |
|  OAuth via HTTP <--+-->| /oauth/client-  |  | |  127.0.0.1:{port}|
|  callback bridge   |   |   metadata.json |  | |  opens browser   |
|                    |   | /static/ (WASM) |  | |  persists tokens |
+--------+-----------+   +-+---------------+  | +--------+---------+
         |                  |                  |          |
         |                  |  +------+--------+          |
         |                  |  |  Browser      |          |
         |                  |  |  (WASM game)  |          |
         |                  |  |  JS handles   |          |
         |                  |  |  OAuth via    |          |
         |                  |  |  @atproto/    |          |
         |                  |  |  oauth-client |          |
         |                  |  +------+--------+          |
         |                  |         |                   |
         |    +-------------+--+      |                   |
         +--->|  PdsSaveBackend |<----+-------------------+
              |                 |
              |  XRPC calls to  |
              |  user's PDS:    |
              |  putRecord      |
              |  getRecord      |
              |  uploadBlob     |
              |  getBlob        |
              +---------+-------+
                        |
                        v
              +---------+-------+
              |  User's PDS     |
              |  (their repo)   |
              |                 |
              |  collection:    |
              |  *.save.*       |
              +--------+--------+
                       |
          +------------+------------+
          |            |            |
     autosave      slot-1 ..   settings
     slot-5
```

### Key design decisions

**DID as the universal identity key.** All save data is addressed by DID, not by handle or username. Handles can change; DIDs are permanent.

**Saves on the PDS, not the server.** Server-local saves would not be accessible from other servers. Storing saves in the user's PDS repository makes them portable to any server running the game. If a server shuts down, users still have their saves.

**Hybrid record+blob storage.** Record fields contain lightweight metadata (turn count, HP, explored %) for slot-selection UI. The full game state is stored as a blob referenced from the record. This avoids downloading full saves just to display a slot list.

**Local save cache.** PDS writes are network roundtrips. The server maintains a local cache (filesystem for SSH, `localStorage` for WASM) as a write buffer. Autosaves go to the local cache; PDS sync happens asynchronously on explicit save, session end, or periodically.

**Both auth methods coexist.** Username/password login remains for users without atproto accounts. The lobby offers both options. Existing accounts can be linked to a DID.

## Prerequisite: Extract `SaveBackend` to `crates/saves` (Done)

> **Status: Complete.** The `SaveBackend` trait now lives in `crates/saves/src/lib.rs`, depending only on `roguelike-core`. The `tui` crate re-exports it (`pub use roguelike_saves::SaveBackend`). Both `terminal` (`local_saves.rs`) and `ssh` (`saves.rs`) implement the trait from `roguelike-saves`. This unblocks the atproto and WASM dependency chains.

The `SaveBackend` trait was originally in `crates/tui/src/saves.rs`, but `roguelike-tui` depends on `crossterm` which doesn't compile to WASM. The trait itself is a pure interface over `GameState`, `SlotMetadata`, and `Settings` — no platform dependencies.

It lives in `crates/saves` rather than `roguelike-core` because constrained platforms (GBA with 32KB SRAM, C64 with tape/floppy) need completely different save mechanisms — they can't implement an interface that assumes multiple JSON save slots.

```
roguelike-core          ← universal (GameState, SlotMetadata, Settings)
  ↑
roguelike-saves         ← SaveBackend trait (connected platforms only)
  ↑         ↑
tui       atproto/web   ← implementations
```

Connected platforms (`terminal`, `ssh`, `atproto`, `web`) depend on `roguelike-saves`. Constrained platforms (`gba`, `c64`) don't — they have their own save mechanisms suited to their hardware. The C64 can optionally reach the PDS via an external [bridge server](c64-atproto-bridge.md) that translates binary TCP packets to XRPC calls.

## Lexicon Design

Custom lexicons use a reverse-domain namespace. The domain is TBD; placeholders use `example.com` below. Replace `com.example.roguelike` with the actual namespace once a domain is chosen.

### `com.example.roguelike.save.gameState`

Stores a single save slot (autosave or numbered slot). The record key (`rkey`) identifies the slot.

```json
{
  "lexicon": 1,
  "id": "com.example.roguelike.save.gameState",
  "defs": {
    "main": {
      "type": "record",
      "key": "any",
      "description": "A saved game state. The rkey identifies the slot: 'autosave', 'slot-1' through 'slot-5'.",
      "record": {
        "type": "object",
        "required": ["saveData", "turnCount", "playerHp", "playerMaxHp", "exploredPct", "savedAt"],
        "properties": {
          "saveData": {
            "type": "blob",
            "description": "The full serialized GameState JSON.",
            "accept": ["application/json"],
            "maxSize": 1048576
          },
          "turnCount": {
            "type": "integer",
            "description": "Number of turns elapsed in this save."
          },
          "playerHp": {
            "type": "integer",
            "description": "Player's current HP."
          },
          "playerMaxHp": {
            "type": "integer",
            "description": "Player's maximum HP."
          },
          "exploredPct": {
            "type": "integer",
            "description": "Percentage of the map explored (0-100)."
          },
          "playerName": {
            "type": "string",
            "maxLength": 64,
            "description": "Player's chosen character name, if set."
          },
          "seedCode": {
            "type": "string",
            "maxLength": 32,
            "description": "Seed code for this game (e.g., 'r7z3kq-80x39')."
          },
          "savedAt": {
            "type": "string",
            "format": "datetime",
            "description": "ISO 8601 timestamp of when the save was written."
          }
        }
      }
    }
  }
}
```

**Record keys (rkeys):**

| rkey | Purpose |
|------|---------|
| `autosave` | Automatic save written every N turns |
| `slot-1` through `slot-5` | Manual save slots (casual mode) |

**Why blob for saveData:** Game state JSON is currently 20-80KB for an 80x40 map. As the game grows (multi-floor dungeons, inventory, quest state), saves will grow. The blob approach handles up to 1MB without schema changes. The record itself stays small since metadata fields (turnCount, playerHp, etc.) map directly to the existing `SlotMetadata` struct.

**Blob integrity:** AT Protocol blob references use CIDs (SHA-256 content hashes). The PDS verifies blob integrity on storage and retrieval. No additional hash field is needed in the record.

### `com.example.roguelike.save.settings`

Stores user preferences. Single record per user.

```json
{
  "lexicon": 1,
  "id": "com.example.roguelike.save.settings",
  "defs": {
    "main": {
      "type": "record",
      "key": "literal:self",
      "description": "User game settings/preferences. Single record with rkey 'self'.",
      "record": {
        "type": "object",
        "required": [],
        "properties": {
          "casualMode": { "type": "boolean" },
          "viKeys": { "type": "boolean" },
          "numpad": { "type": "boolean" },
          "showCorpses": { "type": "boolean" },
          "showCoordinates": { "type": "boolean" },
          "showExploredPct": { "type": "boolean" },
          "showKeybindHints": { "type": "boolean" },
          "animationSpeedMs": { "type": "integer" },
          "autosaveFrequency": { "type": "integer" },
          "messageLogLines": { "type": "integer" },
          "colorPalette": { "type": "string", "maxLength": 32 },
          "playerName": { "type": "string", "maxLength": 64 },
          "pronouns": { "type": "string", "maxLength": 32 },
          "leftHandLayout": { "type": "string", "maxLength": 32 }
        }
      }
    }
  }
}
```

**Design note:** Settings are small structured data (~200 bytes). They live directly in the record with no blob. All fields are optional to allow forward/backward compatibility as settings are added.

**Enum serialization stability:** Three fields — `colorPalette`, `pronouns`, and `leftHandLayout` — are Rust enums (`ColorPalette`, `Pronouns`, `LeftHandLayout`) serialized as strings. The serde string representation of these enums becomes the stable interchange format stored on the PDS. Renaming an enum variant is a breaking change that would silently fail to deserialize settings from the PDS. These enum variant names should be treated as part of the public API once the lexicon is published.

## OAuth Integration

### SSH Frontend

The SSH server already accepts all connections at the SSH protocol level (`auth_none` returns `Auth::Accept` in `server.rs`). Authentication is handled in the application-layer lobby. Adding atproto OAuth requires an HTTP server running alongside the SSH server to handle OAuth callbacks.

**Flow:**

```
1. User connects via SSH, sees lobby menu.
2. User selects "Login with Bluesky".
3. Lobby prompts for their handle (e.g., "alice.bsky.social").
4. Server resolves handle -> DID -> PDS -> Authorization Server.
5. Server generates PKCE verifier, DPoP keypair, state token.
6. Server submits PAR request to the AS, receives request_uri.
7. Server creates a oneshot channel, stores sender in OAuthPendingStore keyed by state.
8. Lobby displays the authorization URL and blocks on the oneshot receiver.
9. User opens the URL in their browser, authenticates with Bluesky.
10. Bluesky redirects to https://<domain>/oauth/callback?code=...&state=...
11. HTTP server's callback handler:
    a. Looks up state in OAuthPendingStore.
    b. Exchanges auth code for tokens (with DPoP proof + PKCE verifier).
    c. Extracts DID from token response (sub claim).
    d. Sends DID through the oneshot channel.
    e. Returns a "You can close this tab" page to the browser.
12. SSH lobby receives the DID, loads saves from PDS, enters game session.
```

**Bridging async HTTP callback to sync lobby thread:** The lobby runs on a `spawn_blocking` thread (see the `channel_open_session` handler in `server.rs`). Communication uses a `tokio::sync::oneshot` channel. The lobby blocks via `rt_handle.block_on(receiver)`, matching the existing `block_on` pattern used for session cleanup in the same function.

**Timeout:** If the user doesn't complete browser auth within 5 minutes, the oneshot receiver times out and the lobby returns to the main menu.

**Key data structures:**

```rust
/// Stored in a DashMap<String, PendingOAuth> keyed by the `state` parameter.
/// Shared between the SSH lobby thread and the axum callback handler.
pub struct OAuthPendingStore {
    pending: DashMap<String, PendingOAuth>,
}

struct PendingOAuth {
    /// Sends the completed auth result back to the blocking lobby thread.
    sender: tokio::sync::oneshot::Sender<OAuthResult>,
    /// PKCE code verifier (needed for token exchange in the callback).
    pkce_verifier: String,
    /// DPoP keypair (needed for token exchange and subsequent PDS calls).
    dpop_key: p256::SecretKey,
    /// The user's resolved DID (verified independently during handle resolution).
    resolved_did: String,
    /// The user's PDS URL (needed to construct PdsSaveBackend).
    pds_url: String,
    /// The authorization server's token endpoint.
    token_endpoint: String,
    /// When this entry was created (for timeout/cleanup).
    created_at: std::time::Instant,
}

/// Sent through the oneshot channel from the HTTP callback to the SSH lobby.
pub struct OAuthResult {
    pub did: String,
    pub pds_url: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub dpop_key: p256::SecretKey,
}
```

**Error handling in the OAuth flow:**

| Failure | Where | User sees |
|---------|-------|-----------|
| Invalid handle (resolution fails) | Step 4 | "Could not resolve handle. Check spelling." |
| PDS/AS unreachable (network error) | Step 4-6 | "Could not reach authentication server. Try again." |
| PAR rejected (AS rejects client) | Step 6 | "Authentication failed. The server may not support this client." |
| User doesn't complete browser auth | Step 8 (timeout) | "Authorization timed out. Press any key to return." |
| Callback state mismatch (stale/invalid) | Step 11a | Browser sees "Authorization expired. Please try again." |
| Token exchange fails | Step 11b | Browser sees error; lobby times out. |

All errors return the user to the lobby main menu. The lobby's `text_input` / `show_message` / `wait_for_key` pattern (already used for login/register errors) handles the display.

**Lobby interface changes:**

`LobbyResult` currently returns `LoggedIn(String)` with just a username. With atproto, the lobby needs to communicate which save backend to use:

```rust
pub enum LobbyResult {
    /// Legacy username/password login. Use filesystem SaveManager.
    LoggedIn(String),
    /// Atproto login. Use PdsSaveBackend with these credentials.
    LoggedInAtproto {
        display_name: String,   // handle or linked username, for logging/display
        did: String,
        pds_url: String,
        access_token: String,
        refresh_token: Option<String>,
        dpop_key: p256::SecretKey,
    },
    Quit,
}
```

The caller in `server.rs` (`channel_open_session`) runs a lobby↔session loop. After the lobby returns, the loop matches on the `LobbyResult` variant to construct either a `SaveManager` (filesystem) or `PdsSaveBackend`, then passes it to `session::run_session`. The session returns `SessionResult::LogOut` (back to lobby) or `SessionResult::Quit` (disconnect). Inside the session, a server menu (Play / Watch / Log Out) gates access to the game loop. The game loop returns `GameLoopResult::Lobby` (back to server menu) or `GameLoopResult::Quit`.

On SSH, the title and pause menus show "Lobby" instead of "Quit", wired to `MenuAction::Lobby` which propagates up as `GameLoopResult::Lobby`.

**`run_lobby` signature changes:**

```rust
// Before:
pub fn run_lobby<W: Write>(
    w: &mut W, rx: &Receiver<Vec<u8>>, parser: &mut AnsiParser,
    accounts: &AccountStore,
    width: i32, height: i32, active_sessions: usize,
) -> std::io::Result<LobbyResult>

// After (adds access to shared OAuth state):
pub fn run_lobby<W: Write>(
    w: &mut W, rx: &Receiver<Vec<u8>>, parser: &mut AnsiParser,
    accounts: &AccountStore,
    oauth: &OAuthPendingStore,
    rt_handle: &tokio::runtime::Handle,   // for blocking on async OAuth operations
    http_base_url: &str,                  // e.g., "https://example.com"
    width: i32, height: i32, active_sessions: usize,
) -> std::io::Result<LobbyResult>
```

### Terminal Frontend

The terminal binary is a local, single-user application. It uses a loopback redirect (RFC 8252 for native apps) — simpler than the SSH case because there's no shared state between concurrent sessions.

**Flow:**

```
1. User selects "Login with Bluesky" from the title screen.
2. Terminal prompts for their handle (e.g., "alice.bsky.social").
3. Terminal resolves handle -> DID -> PDS -> Authorization Server.
4. Terminal generates PKCE verifier, DPoP keypair, state token.
5. Terminal binds a temporary HTTP server to 127.0.0.1:0 (OS-assigned port).
6. Terminal submits PAR request with redirect_uri=http://127.0.0.1:{port}/callback.
7. Terminal opens the authorization URL in the user's default browser (via `open`/`xdg-open`).
8. User authenticates with Bluesky in their browser.
9. Bluesky redirects to http://127.0.0.1:{port}/callback?code=...&state=...
10. Localhost server receives the callback, exchanges code for tokens, shuts down.
11. Terminal shows "Logged in as alice.bsky.social" and returns to the title screen.
12. Saves now use PdsSaveBackend (with local filesystem cache for performance).
```

**Why loopback:** AT Protocol OAuth follows RFC 8252 which supports `http://127.0.0.1` redirects for native applications. No domain, no HTTPS, no TLS certificate needed. The authorization server accepts any port on the loopback address, so the terminal picks a random available port.

**Browser opening:** The terminal uses `open` (macOS), `xdg-open` (Linux), or `start` (Windows) to launch the user's default browser. If the browser can't be opened (e.g., headless server, no display), the terminal prints the URL and asks the user to open it manually — matching the SSH flow.

**Token persistence:** Unlike the SSH server (which holds tokens in memory per-session), the terminal persists tokens to the local filesystem (`~/.local/share/roguelike/atproto_tokens.json`). On next launch, the terminal checks for valid tokens before showing the title screen. If a refresh token is still valid, the user is logged in automatically. If expired, they re-authenticate.

```rust
/// Stored at ~/.local/share/roguelike/atproto_tokens.json
pub struct PersistedTokens {
    pub did: String,
    pub handle: String,
    pub pds_url: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub dpop_key_pem: String,          // PEM-encoded ES256 private key
    pub access_token_expires_at: String, // ISO 8601
}
```

**Sync/async bridge:** The terminal binary is currently synchronous. Adding atproto requires a tokio runtime for HTTP calls (handle resolution, PAR, token exchange, PDS XRPC). The terminal creates a `tokio::runtime::Runtime` at startup (gated behind the `atproto` feature flag) and uses `runtime.block_on()` for individual operations. The game loop itself remains synchronous.

**Feature flag:** Atproto support is opt-in via a Cargo feature to avoid adding tokio/reqwest to the default terminal build:

```toml
# crates/terminal/Cargo.toml
[features]
default = ["dev-tools", "gamepad"]
atproto = ["roguelike-atproto", "tokio/rt"]
```

Without the `atproto` feature, the terminal behaves exactly as it does today — local saves, no login, no network calls. The "Login with Bluesky" title screen option is hidden when compiled without `atproto`.

**Title screen integration:** The title menu gains a "Login with Bluesky" / "Log Out ({handle})" entry. When logged in, the title screen shows "Logged in as alice.bsky.social" and the save/load system uses `PdsSaveBackend` with a local filesystem cache. When not logged in (or compiled without `atproto`), saves use the existing `LocalSaveBackend`.

**Local saves remain available:** Logging in with Bluesky does not delete or migrate local saves automatically. The user can choose to migrate via a "Migrate Local Saves to Bluesky" option in settings. Local saves and PDS saves coexist — the active backend depends on login state.

### WASM Frontend

In the browser, OAuth is the native flow. The JavaScript layer handles the entire OAuth exchange using `@atproto/oauth-client-browser`. The Rust/WASM code never touches OAuth directly.

**Flow:**

```
1. User visits the game website, clicks "Login with Bluesky".
2. JS calls BrowserOAuthClient.signIn(handle), which redirects to Bluesky.
3. User authenticates, Bluesky redirects back with auth code in URL.
4. JS calls BrowserOAuthClient.callback(params), receives session with DID.
5. JS passes the DID and access token to the WASM module.
6. WASM game loop starts, using PdsSaveBackend with the provided credentials.
```

**Token refresh:** The JS OAuth client handles token refresh automatically. The WASM module calls back to JS when it needs to make PDS API calls, ensuring tokens are always fresh.

### Client Metadata Document

All three frontends share a single client metadata document served at `https://<domain>/oauth/client-metadata.json`:

```json
{
  "client_id": "https://<domain>/oauth/client-metadata.json",
  "client_name": "Roguelike",
  "application_type": "web",
  "grant_types": ["authorization_code", "refresh_token"],
  "response_types": ["code"],
  "scope": "atproto",
  "redirect_uris": [
    "https://<domain>/oauth/callback",
    "http://127.0.0.1/callback"
  ],
  "dpop_bound_access_tokens": true,
  "token_endpoint_auth_method": "none"
}
```

The first redirect URI is for the SSH and WASM frontends (server-side callback). The second is for the terminal binary (loopback redirect per RFC 8252). AT Protocol authorization servers accept any port on `http://127.0.0.1` when this loopback URI pattern is registered, so the terminal can use a random OS-assigned port.

This is a public client (no client secret). Token lifetimes are shorter for public clients (access token < 30 min, refresh token session limited to 2 weeks).

### HTTP Server Endpoints

The axum HTTP server (running alongside the SSH server) serves OAuth callbacks and the WASM frontend. The terminal binary does not use this server — it runs its own temporary loopback server for OAuth callbacks.

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/` | GET | Serve the WASM game client (static files) |
| `/oauth/client-metadata.json` | GET | AT Protocol client metadata document |
| `/oauth/callback` | GET | OAuth authorization code callback |

**HTTPS requirement:** AT Protocol OAuth requires HTTPS for all endpoints, including the client metadata URL and redirect URI. The server needs a TLS certificate (via Let's Encrypt / reverse proxy).

## PDS Save Backend

A new `PdsSaveBackend` implements the `SaveBackend` trait (from `roguelike-saves`) using XRPC calls to the user's PDS.

### Trait mapping

| `SaveBackend` method | Implementation |
|----------------------|----------------|
| `has_autosave()` | `getRecord(gameState, "autosave")` -- check for success vs 404 |
| `load_autosave()` | `getRecord` -> extract blob ref -> `getBlob` -> `GameState::load_from_json` |
| `write_autosave(json, meta)` | `uploadBlob(json_bytes)` -> `putRecord(gameState, "autosave", {blob_ref, meta})` |
| `delete_autosave()` | `deleteRecord(gameState, "autosave")` |
| `load_autosave_metadata()` | `getRecord(gameState, "autosave")` -> extract metadata fields from record |
| `save_to_slot(state, slot, name)` | `uploadBlob` -> `putRecord(gameState, "slot-{n}", ...)` |
| `load_from_slot(slot)` | `getRecord(gameState, "slot-{n}")` -> `getBlob` -> deserialize |
| `load_all_slot_metadata()` | `listRecords(gameState)` -> extract metadata from each record |
| `has_any_save()` | `listRecords(gameState, limit=1)` -- check if non-empty |
| `has_save_for_title(casual)` | Combine `has_autosave` + slot check depending on mode |
| `load_settings()` | `getRecord(settings, "self")` -> deserialize fields -> `Settings` |
| `save_settings(settings)` | `putRecord(settings, "self", settings_record)` |

### DID sanitization for filesystem paths

DIDs contain colons (e.g., `did:plc:abc123`), which are illegal in Windows filenames and awkward on Unix. For filesystem paths (local cache directory, Phase 1 local saves), sanitize by replacing colons with underscores:

```
did:plc:abc123  ->  did_plc_abc123
```

Centralize this in a helper:

```rust
fn did_to_dir_name(did: &str) -> String {
    did.replace(':', "_")
}
```

Cache paths become `data_dir/cache/did_plc_abc123/`. Phase 1 local saves use `data_dir/saves/did_plc_abc123/`.

### Sync/async bridge for `SaveBackend`

`SaveBackend` methods are synchronous (`fn load_autosave(&self) -> Result<GameState, String>`), but PDS calls are async (HTTP via reqwest). The bridge depends on the frontend:

**SSH:** The game loop already runs in `spawn_blocking` with access to a tokio runtime handle (see the `channel_open_session` handler in `server.rs`). `PdsSaveBackend` stores a `tokio::runtime::Handle` and calls `handle.block_on(async_pds_call())` inside each trait method. This is the same pattern the codebase already uses.

**WASM:** `PdsSaveBackend` calls back to JS via `wasm_bindgen`. The JS side makes the async HTTP request and returns the result synchronously to the Web Worker (which can block). Alternatively, the WASM save backend operates only on the `localStorage` cache, and a JS background task syncs to the PDS independently.

### Caching strategy

Network roundtrips to the PDS add latency. The game's autosave fires every N turns, which would cause noticeable stutters if each autosave hit the network.

**Solution: local write cache with async PDS sync.**

```
Game loop                     Background sync task
---------                     --------------------
autosave -> local cache       periodically: flush cache -> PDS
(instant, in-memory/disk)     (async, non-blocking)

explicit "Save Game" ->       immediate: cache + PDS write
session end ->                final: flush all dirty state to PDS
session start ->              initial: PDS read -> populate cache
```

For SSH, the local cache is the filesystem under `data_dir/cache/{did}/`. For WASM, it's `localStorage`. The cache is a performance optimization; the PDS is the source of truth.

**Conflict resolution:** If the same user plays simultaneously on two frontends (unlikely but possible), the last write wins. The `savedAt` timestamp in the record enables a "this save is older than expected, overwrite?" prompt if needed.

### Steam Cloud coexistence

The local filesystem cache is the natural integration point for Steam Cloud saves. Steam Cloud operates at the filesystem level — it syncs files between machines transparently. Since the `PdsSaveBackend` already writes to a local cache directory before syncing to the PDS, Steam Cloud can sync that same directory with zero game code changes.

**Save hierarchy:**

```
AT Protocol PDS        ← Cross-ecosystem portability (SSH, web, C64 bridge, any server)
  ↕ async sync
Local filesystem       ← Fast, always available, write cache for PDS
  ↕ Steam Auto-Cloud
Steam Cloud            ← Cross-machine portability within Steam ecosystem
```

The PDS is the source of truth for cross-platform identity and federation. Steam Cloud is a transparent backup of the local cache, providing multi-PC sync for Steam/Steam Deck users who may not have an atproto identity.

**Steam Auto-Cloud (zero-code, recommended):** Configure Steam Auto-Cloud in the Steamworks partner dashboard to sync the save and cache directories:

```
# Non-atproto users (local saves only):
~/.local/share/roguelike/saves/          → *.json

# Atproto users (PDS cache):
~/.local/share/roguelike/cache/          → **/*.json

# Atproto tokens (so login persists across machines):
~/.local/share/roguelike/                → atproto_tokens.json
```

On Windows, replace `~/.local/share` with `%APPDATA%`. The paths follow the XDG / platform conventions already used by the terminal binary.

**Three-way conflict resolution:** When a user has both a Bluesky identity and Steam Cloud, saves can exist in three locations (PDS, local cache, Steam Cloud). The `savedAt` timestamp in every save record resolves conflicts:

1. On session start, load from PDS (source of truth).
2. If PDS is unreachable, fall back to local cache (which Steam Cloud keeps current across machines).
3. If the local cache has a newer `savedAt` than the PDS (e.g., played offline, Steam Cloud synced, now PDS is reachable), prompt: "Local save is newer than Bluesky save. Upload to Bluesky?"

This means Steam Cloud acts as a resilience layer — if the PDS is down or the user hasn't linked Bluesky, saves still roam between Steam machines.

**Feature flag:** Steam integration is opt-in via a Cargo feature, following the same pattern as atproto:

```toml
# crates/terminal/Cargo.toml
[features]
default = ["dev-tools", "gamepad"]
atproto = ["roguelike-atproto", "tokio/rt"]
steam = ["steamworks"]
```

Steam Auto-Cloud requires no feature flag (it's external to the binary). The `steam` feature is only needed if using the Steamworks `ISteamRemoteStorage` API for richer conflict handling or Steam Deck-specific integration (e.g., suspend/resume save triggers).

**Users without atproto:** For players who never log in with Bluesky, the save path is simply: local filesystem ↔ Steam Cloud. This is the standard Steam game experience. Atproto adds a layer on top, not a replacement.

### Authentication for PDS calls

XRPC calls to the PDS require a DPoP-bound access token. For SSH, the server holds the token obtained during OAuth. For WASM, the JS layer provides fresh tokens to the WASM module.

**Token lifecycle:**
- Access tokens expire in < 30 minutes (typically 5-15 min).
- The server refreshes tokens using the refresh token before expiry.
- If the refresh token expires (2-week limit for public clients), the user must re-authenticate.

## Account Migration

### Linking existing accounts to atproto

Existing username/password accounts can be linked to an atproto DID. The lobby offers a "Link Bluesky Account" option when logged in with username/password.

**Flow:**

```
1. User logs in with username/password (existing flow).
2. User selects "Link Bluesky Account" from pause menu or settings.
3. Same OAuth flow as "Login with Bluesky" (handle -> browser auth -> DID).
4. Server writes the DID to the existing Account JSON file:
   {
     "password_hash": "...",
     "atproto_did": "did:plc:abc123",
     "atproto_handle": "alice.bsky.social",
     "created": "...",
     "last_login": "..."
   }
5. Server migrates existing saves from local storage to the user's PDS:
   a. Read each save file from data_dir/saves/{username}/.
   b. Upload as blob + putRecord to the PDS.
   c. Mark local saves as migrated (rename or add .migrated suffix).
6. Future logins via atproto DID map to this account.
```

**Reverse lookup:** When a user logs in via atproto, the server checks if any existing Account file has a matching `atproto_did` field. If found, the user gets their existing account context (username for display, etc.).

### Account file changes

The `Account` struct in `accounts.rs` gains two optional fields:

```rust
pub struct Account {
    pub password_hash: String,
    pub atproto_did: Option<String>,      // e.g., "did:plc:abc123"
    pub atproto_handle: Option<String>,   // e.g., "alice.bsky.social" (display only)
    pub created: String,
    pub last_login: String,
}
```

Both fields are `Option` with `#[serde(default)]` for backward compatibility with existing account files.

### Login flow decision tree

**SSH lobby:**

```
Lobby
 |
 +-- "Login" (username/password)
 |     -> AccountStore::login()
 |     -> if account has atproto_did: use PdsSaveBackend
 |     -> else: use filesystem SaveManager (legacy)
 |
 +-- "Register" (username/password)
 |     -> AccountStore::register()
 |     -> use filesystem SaveManager (legacy)
 |
 +-- "Login with Bluesky" (atproto OAuth)
       -> OAuth flow -> DID
       -> if DID matches existing account: use that account
       -> if DID is new: create account with auto-generated username from handle
       -> use PdsSaveBackend
```

**Terminal title screen:**

```
Title Screen
 |
 +-- (not logged in, atproto feature enabled)
 |     "Login with Bluesky" visible
 |     -> loopback OAuth flow -> DID + tokens
 |     -> persist tokens to filesystem
 |     -> switch SaveBackend to PdsSaveBackend (with local cache)
 |     -> return to title screen, now showing "Logged in as {handle}"
 |
 +-- (logged in)
 |     "Log Out ({handle})" visible
 |     -> clear persisted tokens
 |     -> switch SaveBackend back to LocalSaveBackend
 |     -> return to title screen
 |
 +-- (atproto feature not compiled)
       no login options shown, LocalSaveBackend always used
```

## WASM Frontend

The WASM frontend is a new `crates/web` crate that implements the same trait interfaces as SSH and terminal.

### Crate structure

```
crates/web/
  Cargo.toml
  src/
    lib.rs          WASM entry point, JS interop
    renderer.rs     CanvasRenderer implementing Renderer trait
    input.rs        WebInput implementing InputProvider
    saves.rs        Wrapper that calls PdsSaveBackend via JS interop
  static/
    index.html      Page shell, OAuth JS, WASM loader
    oauth.js        @atproto/oauth-client-browser integration
    style.css       Monospace canvas sizing
```

### Renderer

`CanvasRenderer` implements `roguelike_core::platform::Renderer` by drawing to an HTML `<canvas>` element using a monospace glyph grid. Each `draw_char` call maps to a positioned glyph render at `(x * cell_width, y * cell_height)`. `GameColor` maps to CSS/canvas color values.

### Input

Browser keyboard events are captured by a JS event listener and pushed into a channel consumed by the WASM side. The `InputProvider` trait requires blocking `wait_for_key()` calls, which browsers don't support on the main thread.

**Recommended approach: Web Worker.** Run the game loop inside a Web Worker where `Atomics.wait()` enables blocking reads from a `SharedArrayBuffer`. The main thread writes keyboard events into the buffer. The worker renders to an `OffscreenCanvas` transferred at initialization. This preserves the synchronous `InputProvider` contract without modifying the game loop.

Requires `Cross-Origin-Isolation` headers (`Cross-Origin-Opener-Policy: same-origin`, `Cross-Origin-Embedder-Policy: require-corp`).

### Saves

The WASM `SaveBackend` calls back to JS for PDS XRPC operations. The JS layer holds the OAuth session and makes authenticated requests. This avoids implementing DPoP token management in Rust/WASM.

```
WASM SaveBackend -> wasm_bindgen extern -> JS PDS client -> XRPC -> PDS
```

`localStorage` serves as the local write cache (equivalent to the SSH server's filesystem cache).

### Dependencies

```toml
[dependencies]
roguelike-core = { path = "../core", default-features = false }
roguelike-saves = { path = "../saves" }
wasm-bindgen = "0.2"
web-sys = { version = "0.3", features = ["CanvasRenderingContext2d", ...] }
js-sys = "0.3"
getrandom = { version = "0.2", features = ["js"] }  # rand entropy via crypto.getRandomValues
```

The `getrandom` JS feature is required for `rand` to work in WASM.

## New Crate: `crates/atproto`

Shared AT Protocol logic used by the terminal, SSH, and web frontends.

```
crates/atproto/
  Cargo.toml
  src/
    lib.rs
    resolve.rs       Handle -> DID -> PDS -> AS resolution chain
    oauth.rs         PAR, DPoP, token exchange (SSH server use)
    pds.rs           XRPC client for PDS record/blob operations
    save_backend.rs  PdsSaveBackend implementing SaveBackend
    lexicon.rs       Record type definitions matching the lexicons
```

### Dependencies

```toml
[dependencies]
roguelike-core = { path = "../core" }
roguelike-saves = { path = "../saves" }
reqwest = { version = "0.12", features = ["json"] }
serde = { workspace = true }
serde_json = { workspace = true }
p256 = "0.13"            # ES256 key generation for DPoP
jsonwebtoken = "9"        # DPoP proof JWTs
tokio = { version = "1", features = ["rt", "sync"] }
```

Alternatively, `atrium-oauth` (v0.1.6) from the atrium-rs ecosystem can replace the manual `resolve.rs` + `oauth.rs` + DPoP implementation. It handles the full resolution chain, PAR, DPoP key management, and token exchange. This would significantly reduce implementation effort at the cost of a larger dependency tree.

| Approach | Effort | Control | Dependencies |
|----------|--------|---------|--------------|
| Manual (p256 + jsonwebtoken + reqwest) | Higher | Full | Minimal |
| `atrium-oauth` | Lower | Less | Pulls in atrium ecosystem |

Recommended: start with `atrium-oauth` to reduce initial effort. Evaluate whether to vendor/replace later if the dependency proves problematic.

## Implementation Phases

### Phase 1: OAuth (SSH + Terminal)

**Goal:** Users can "Login with Bluesky" on the SSH server and the terminal binary.

**Core atproto crate:**
- Add handle resolution, PAR, DPoP, token exchange in `crates/atproto`.
- Shared logic: `resolve_handle()`, `exchange_code()`, `refresh_tokens()` work for all frontends.

**SSH server:**
- Add axum HTTP server alongside SSH in `main.rs`, sharing `Arc<ServerState>`.
- Serve `/oauth/client-metadata.json` (static JSON).
- Implement `/oauth/callback` handler with `OAuthPendingStore` (state -> oneshot sender).
- Add "Login with Bluesky" to the SSH lobby menu.
- Bridge: lobby blocks on oneshot receiver; callback handler sends DID through it.
- Add `atproto_did` / `atproto_handle` fields to `Account`.
- Requires: domain with HTTPS.

**Terminal binary:**
- Add `atproto` feature flag to `crates/terminal/Cargo.toml` (off by default).
- Bind temporary loopback HTTP server for OAuth callback.
- Open browser via `open`/`xdg-open`/`start`.
- Persist tokens to `~/.local/share/roguelike/atproto_tokens.json`.
- Add "Login with Bluesky" / "Log Out" to the title screen menu.
- Auto-login on startup if persisted refresh token is still valid.

**All frontends:** Saves remain on local filesystem keyed by DID (not yet on PDS).

### Phase 2: PDS Save Backend

**Goal:** Saves are stored on the user's PDS, accessible from any frontend.

- Define and publish lexicons (`com.<domain>.roguelike.save.gameState`, `.settings`).
- Implement `PdsSaveBackend` in `crates/atproto/src/save_backend.rs`.
- Implement local cache layer (filesystem for both SSH and terminal, with dirty tracking and async PDS sync).
- Migrate saves from local-only to PDS-backed for atproto users on all frontends.
- SSH username/password-only users continue using local `SaveManager`. Terminal users without atproto continue using `LocalSaveBackend`.
- Add "Link Bluesky Account" flow for existing SSH accounts.
- Add "Migrate Local Saves to Bluesky" option in terminal settings.
- Save migration tool: reads local saves, uploads to PDS.

### Phase 3: WASM Frontend

**Goal:** Browser-playable game with the same saves as SSH and terminal.

- Create `crates/web` with `CanvasRenderer`, `WebInput`, WASM save bridge.
- Web Worker architecture for blocking game loop.
- JS entry point with `@atproto/oauth-client-browser` for auth.
- Serve static WASM files from the axum HTTP server.
- `localStorage` as the local save cache, PDS as source of truth.
- **Result:** Play on terminal, SSH, or browser — same atproto identity, same saves.

### Phase 4: Polish

- Handle edge cases: PDS unavailable (fall back to cache), token expiry during long sessions, simultaneous play on multiple clients.
- Terminal: graceful fallback when browser can't be opened (print URL, manual paste).
- Terminal: auto-refresh tokens on startup; clear invalid persisted tokens gracefully.
- Settings sync (merge strategy for concurrent changes across terminal/SSH/web).
- Display atproto handle in-game (status bar, death screen, leaderboards).
- Rate limiting for PDS writes (respect PDS operator limits).

## Configuration

### SSH Server

The SSH server's `main.rs` uses a hand-rolled arg parser. The following are added for Phase 1:

| Flag | Env var | Default | Purpose |
|------|---------|---------|---------|
| `--http-port <PORT>` | `ROGUELIKE_HTTP_PORT` | `8443` | HTTP server listen port |
| `--domain <DOMAIN>` | `ROGUELIKE_DOMAIN` | (required for atproto) | Public domain name for OAuth URLs |
| `--tls-cert <PATH>` | `ROGUELIKE_TLS_CERT` | none | TLS certificate path (if terminating TLS directly) |
| `--tls-key <PATH>` | `ROGUELIKE_TLS_KEY` | none | TLS private key path |
| `--no-http` | — | — | Disable the HTTP server entirely (username/password only) |

If `--domain` is not set, the "Login with Bluesky" option is hidden from the lobby menu. This lets operators run the SSH server without HTTPS/atproto support (backward compatible).

If `--tls-cert` / `--tls-key` are not set, the HTTP server listens on plain HTTP. Operators are expected to terminate TLS via a reverse proxy (nginx, Caddy). The `--domain` value is still used to construct OAuth URLs.

### Terminal Binary

The terminal binary requires no server-side configuration. When built with the `atproto` feature, it uses the hosted client metadata URL for the `client_id`:

| Env var | Default | Purpose |
|---------|---------|---------|
| `ROGUELIKE_CLIENT_ID` | `https://<domain>/oauth/client-metadata.json` | Override the client metadata URL (useful for development with `http://localhost`) |

Token persistence path follows the XDG base directory spec: `$XDG_DATA_HOME/roguelike/atproto_tokens.json` (defaults to `~/.local/share/roguelike/atproto_tokens.json` on Linux, `~/Library/Application Support/roguelike/atproto_tokens.json` on macOS).

The "Login with Bluesky" option appears in the title screen only when compiled with the `atproto` feature. No CLI flags needed — the feature flag controls availability, and the user decides whether to log in via the menu.

**`ServerState` changes:**

```rust
pub struct ServerState {
    pub data_dir: PathBuf,
    pub accounts: AccountStore,
    pub active_sessions: AtomicUsize,
    pub max_connections: usize,
    pub idle_timeout_secs: u64,
    // New fields:
    pub oauth: Option<Arc<OAuthPendingStore>>,  // None if --no-http
    pub http_base_url: Option<String>,          // None if --no-http
}
```

## XRPC Request Example

For implementers unfamiliar with AT Protocol, here is a concrete example of writing a save to the PDS.

**Step 1: Upload the blob**

```http
POST https://morel.us-east.host.bsky.network/xrpc/com.atproto.repo.uploadBlob
Authorization: DPoP <access_token>
DPoP: <dpop_proof_jwt>
Content-Type: application/json

<raw save JSON bytes>
```

Response:

```json
{
  "blob": {
    "$type": "blob",
    "ref": { "$link": "bafkreig5ot3j..." },
    "mimeType": "application/json",
    "size": 47832
  }
}
```

**Step 2: Write the record**

```http
POST https://morel.us-east.host.bsky.network/xrpc/com.atproto.repo.putRecord
Authorization: DPoP <access_token>
DPoP: <dpop_proof_jwt>
Content-Type: application/json

{
  "repo": "did:plc:abc123",
  "collection": "com.example.roguelike.save.gameState",
  "rkey": "autosave",
  "record": {
    "saveData": {
      "$type": "blob",
      "ref": { "$link": "bafkreig5ot3j..." },
      "mimeType": "application/json",
      "size": 47832
    },
    "turnCount": 142,
    "playerHp": 18,
    "playerMaxHp": 30,
    "exploredPct": 35,
    "playerName": "David",
    "seedCode": "r7z3kq-80x39",
    "savedAt": "2026-02-19T12:00:00Z"
  }
}
```

**Step 3: Read it back (from any server)**

```http
GET https://morel.us-east.host.bsky.network/xrpc/com.atproto.repo.getRecord?repo=did:plc:abc123&collection=com.example.roguelike.save.gameState&rkey=autosave
Authorization: DPoP <access_token>
DPoP: <dpop_proof_jwt>
```

Response includes the record with metadata fields (for slot display) and the blob ref (for full load). Fetch the blob separately via `com.atproto.sync.getBlob?did=...&cid=bafkreig5ot3j...`.

**DPoP proof JWT** (the `DPoP` header value) is a signed JWT containing `{"jti":"<unique>","htm":"POST","htu":"<endpoint_url>","iat":<now>,"nonce":"<server_nonce>"}`, signed with the session's ES256 DPoP key. If the server returns a `use_dpop_nonce` error, retry with the new nonce from the error response.

## Security Considerations

- **App passwords are not used.** OAuth with PKCE + DPoP is the only atproto auth method. No user credentials are stored on the server.
- **Access tokens are short-lived** (< 30 min) and DPoP-bound. A leaked token cannot be used without the corresponding private key.
- **The server never stores the user's atproto password.** OAuth means the user authenticates directly with their PDS/authorization server.
- **DID verification.** After OAuth completes, the server independently re-resolves the handle to verify the DID matches the one returned by the AS. This prevents a compromised AS from impersonating a different user.
- **Save data privacy.** Saves in the user's PDS repo are readable by anyone who knows the DID and collection name (PDS repos are public by default in AT Protocol). Game saves don't contain sensitive data, but this should be documented for users.
- **HTTPS everywhere.** OAuth endpoints, client metadata, and PDS communication all require TLS.
- **Rate limiting.** PDS operators may rate-limit XRPC calls. The caching layer reduces write frequency, but the server should handle 429 responses gracefully (retry with backoff).

## Testing Strategy

### Unit tests

- **Handle resolution:** Mock HTTP responses for each step in the resolution chain. Test error cases (DNS failure, invalid DID document, missing PDS endpoint).
- **DPoP proof generation:** Verify JWT structure, signature, required claims. Test nonce retry logic.
- **`PdsSaveBackend`:** Mock the XRPC client. Verify correct `putRecord` / `getRecord` / `uploadBlob` calls for each `SaveBackend` method. Test cache dirty tracking and flush behavior.
- **Lexicon record serialization:** Round-trip test Rust structs against the lexicon JSON schema.
- **DID sanitization:** Edge cases — long DIDs, `did:web:` with paths, empty strings.
- **Account migration:** Test linking flow — existing account gains `atproto_did`, reverse lookup by DID finds the account.

### Integration tests

- **OAuth flow (manual/CI):** Use AT Protocol's `http://localhost` client_id exception for development. Run the HTTP server locally, initiate OAuth against a test PDS (or Bluesky staging), verify the full flow from PAR to token exchange.
- **PDS round-trip:** Upload a blob, write a record, read it back, verify the game state deserializes correctly. Use a test account on a real PDS.
- **Cache + sync:** Write autosave to local cache, trigger PDS sync, verify record appears on PDS. Kill the server mid-sync, restart, verify dirty cache entries are flushed.

### Development workflow

For local development without a domain/HTTPS:

1. Use `--no-http` to disable atproto and develop with username/password only.
2. Use `http://localhost` as the client_id for testing OAuth against Bluesky (supported by their AS).
3. Use a tool like `ngrok` or Caddy's automatic HTTPS to get a temporary public HTTPS endpoint for full OAuth testing.

## Open Questions

1. **Lexicon namespace.** Needs a real domain. The namespace is permanent once lexicons are published and users have records. This should be decided before Phase 2 (PDS saves) begins, not needed for Phase 1 (local saves keyed by DID).
2. **PDS repo visibility.** AT Protocol repos are public. Are players comfortable with their save data being world-readable? The data is not sensitive (game state), but the existence of a save reveals that the user plays the game. Consider documenting this in the lobby before first atproto login.
3. **`atrium-oauth` maturity.** The crate is at v0.1.6 with low documentation coverage (1.92% on docs.rs). Evaluate whether it's stable enough for production use or if manual implementation is safer. A spike task (attempt the OAuth flow with `atrium-oauth` against Bluesky's AS) would answer this quickly.
4. **Web Worker browser support.** `SharedArrayBuffer` requires `Cross-Origin-Isolation` headers, which can interfere with third-party embeds (analytics, CDN resources). Evaluate whether this is acceptable for the deployment target.
5. **Save size growth.** Current saves are 20-80KB. With multi-floor dungeons and inventory, they could grow. The blob approach handles up to 1MB. If saves exceed that, compression (gzip blob with `application/gzip` MIME type added to the lexicon `accept` list) extends the limit further.
6. **Offline play.** WASM frontend could support offline play using only the `localStorage` cache, syncing to PDS when connectivity returns. This adds complexity (conflict resolution, dirty tracking) but improves UX on unreliable connections.
7. **Token storage for SSH.** The SSH server needs to persist OAuth tokens (access + refresh) to avoid re-authentication on every SSH connection. Options: (a) in-memory only — user re-auths every connection (simplest, acceptable if sessions are long); (b) store encrypted tokens in the Account file — persistent across restarts; (c) separate token store file per DID. Recommendation: start with (a) for Phase 1, upgrade to (b) if the re-auth friction is too high.
8. **Settings sync timing.** When a user changes settings on one frontend and switches to another, when are settings re-read from the PDS? Options: (a) only on session start (simple, small staleness window); (b) periodic poll during play (complex, marginal benefit). Recommendation: (a), since settings rarely change mid-session and the next session always get the latest.
9. **Terminal token security.** Tokens are stored as plaintext JSON on the local filesystem. On a shared machine, another user with filesystem access could read them. Options: (a) plaintext (simplest, acceptable for single-user machines — the typical terminal game scenario); (b) OS keychain via `keyring` crate (more secure, adds platform-specific dependencies); (c) file permissions (0600) to limit access. Recommendation: (a) with (c) — plaintext JSON with restrictive file permissions, matching how `git` stores credentials. The `keyring` crate can be explored later if users request it.
10. **Terminal binary size.** Adding tokio + reqwest to the terminal binary increases size significantly. The `atproto` feature flag keeps this opt-in, but pre-built release binaries need a decision: ship with or without atproto? Options: (a) ship two binaries (with and without); (b) always include atproto; (c) ship without, users build from source to enable. Recommendation: (b) — always include, since the download size increase is acceptable for the convenience of having the feature available.
11. **Steam Cloud save directory layout.** Steam Auto-Cloud syncs files by path pattern. The local cache directory structure (`cache/{did}/`) uses sanitized DIDs as directory names. If a user links a different Bluesky account, they get a different cache directory, and Steam Cloud syncs both. This is correct (different identities, different saves) but may surprise users who expect a single save set per Steam account. Document this behavior in the Steam store page / FAQ.
12. **Steam Deck suspend/resume.** Steam Deck can suspend the game mid-session. On resume, the PDS connection may have timed out and access tokens expired. The existing token refresh logic (refresh on 401, re-auth if refresh token expired) handles this, but the UX should be tested — a 5-second token refresh pause on resume could feel like a hang. Consider pre-emptively refreshing tokens on resume if the `steam` feature detects a Deck environment.

## References

### AT Protocol Specifications

- [AT Protocol OAuth Specification](https://atproto.com/specs/oauth) — The full OAuth profile. Covers PKCE, DPoP, PAR, client metadata documents, handle resolution, token lifetimes, and security requirements. **Start here** for understanding the auth flow.
- [AT Protocol Lexicon Specification](https://atproto.com/specs/lexicon) — How to define custom record schemas. Covers types, validation, and evolution rules (important for forward/backward compatibility of the save lexicons).
- [AT Protocol Repository Specification](https://atproto.com/specs/repository) — Merkle Search Tree structure, record storage, blob handling, and CID-based content addressing. Explains how records and blobs relate.
- [AT Protocol DID Specification](https://atproto.com/specs/did) — DID resolution methods (`did:plc` via plc.directory, `did:web` via domain). Explains the handle-to-DID bidirectional verification requirement.
- [XRPC Specification](https://atproto.com/specs/xrpc) — The HTTP API layer. Defines how `getRecord`, `putRecord`, `uploadBlob`, etc. are called.

### Implementation Guides

- [OAuth Client Implementation Guide](https://docs.bsky.app/docs/advanced-guides/oauth-client) — Bluesky's practical walkthrough for building an OAuth client. Includes code examples and common pitfalls.
- [OAuth Introduction (Getting Started)](https://atproto.com/guides/oauth) — Higher-level overview of the OAuth flow with less spec detail.

### XRPC Endpoints (used by PdsSaveBackend)

- [com.atproto.repo.putRecord](https://docs.bsky.app/docs/api/com-atproto-repo-put-record) — Create or update a record by collection + rkey.
- [com.atproto.repo.getRecord](https://docs.bsky.app/docs/api/com-atproto-repo-get-record) — Fetch a single record.
- [com.atproto.repo.deleteRecord](https://docs.bsky.app/docs/api/com-atproto-repo-delete-record) — Delete a record.
- [com.atproto.repo.listRecords](https://docs.bsky.app/docs/api/com-atproto-repo-list-records) — List records in a collection (for `load_all_slot_metadata`).
- [com.atproto.repo.uploadBlob](https://docs.bsky.app/docs/api/com-atproto-repo-upload-blob) — Upload a blob, returns the CID-based blob ref.
- [com.atproto.sync.getBlob](https://docs.bsky.app/docs/api/com-atproto-sync-get-blob) — Download a blob by DID + CID.

### Live Server Metadata (useful for testing)

- [Bluesky AS Metadata](https://bsky.social/.well-known/oauth-authorization-server) — Authorization server endpoints, supported algorithms, scopes.
- [Bluesky PDS Protected Resource Metadata](https://bsky.social/.well-known/oauth-protected-resource) — Resource server metadata pointing to the authorization server.

### Rust Crates

- [atrium-oauth](https://crates.io/crates/atrium-oauth) (v0.1.6) — Full atproto OAuth client: handle resolution, PAR, DPoP, PKCE, token exchange. Part of the [atrium-rs](https://github.com/sugyan/atrium) ecosystem.
- [atrium-oauth docs.rs](https://docs.rs/atrium-oauth/latest/atrium_oauth/) — API documentation (low coverage but has usage examples).
- [atproto-oauth](https://crates.io/crates/atproto-oauth) (v0.13.0) — Standalone alternative with DPoP middleware, discovery, JWK management.

### Background / Blog Posts

- [OAuth for AT Protocol (announcement)](https://docs.bsky.app/blog/oauth-atproto) — Bluesky's blog post explaining the motivation and design choices behind atproto OAuth.
- [OAuth Improvements (2025)](https://docs.bsky.app/blog/oauth-improvements) — Updates to the OAuth implementation including granular scopes rollout.
- [Auth Scopes Discussion](https://github.com/bluesky-social/atproto/discussions/4118) — GitHub discussion on the ongoing granular permissions work. Relevant for future-proofing the `scope` field in client metadata.
