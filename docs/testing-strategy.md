# Testing Strategy

**Project-wide testing approach for the roguelike dungeon crawler.**
Covers core logic tests, golden replays, scenario tests, invariant property
tests, MCP integration tests, CI verification, benchmarks, and balance CI.
For C64-specific tests (mos-test, VICE integration), see the
[C64 platform guide](platforms/c64-platform-guide.md#7-c64-specific-tests). For
per-feature test plans, see the individual phases in the
[gameplay implementation plan](design/gameplay-implementation-plan.md).

---

## Quick Reference

| Category | Command | What it checks |
|----------|---------|----------------|
| Unit tests | `cargo test -p roguelike-core --lib` | Module-level logic across core modules |
| Golden replays | `cargo test -p roguelike-core --test golden_replays` | Deterministic replay regression — detects unintended gameplay changes |
| Scenario tests | `cargo test -p roguelike-core --test scenarios` | Balance properties — e.g., "player survives 2 goblins", "troll kills weak player" |
| Invariant tests | `cargo test -p roguelike-core --test invariants` | Property-based: random command sequences verify HP bounds, explored monotonicity, dead-stay-dead, save/load roundtrip |
| MCP integration | `cargo test -p roguelike-mcp --test mcp_integration` | All MCP tools: response schemas, error paths, session lifecycle |
| MCP property tests | `cargo test -p roguelike-mcp --test mcp_proptest` | Random MCP tool sequences verify game invariants hold through the JSON interface |
| Benchmarks | `cargo bench -p roguelike-core` | Criterion benchmarks for FOV, pathfinding, game step, and exploration graph |
| All tests | `cargo test --workspace` | ~960 unit + integration tests across all crates |

---

## 1. Core Unit Tests

`cargo test -p roguelike-core --lib` on the host with standard rustc. No emulator
needed. Standard `#[test]` functions inside each module's `#[cfg(test)] mod tests`
block. Tests individual functions and methods in isolation.

## 2. Golden Replay Tests

Stored recordings of full game playthroughs with expected final state. Detect
when a code change unintentionally alters gameplay. Located in
`crates/core/tests/golden_replays/`.

```bash
cargo test -p roguelike-core --test golden_replays
```

When golden tests fail after an intentional gameplay change, regenerate:

```bash
cargo run --bin headless -- --regenerate-goldens crates/core/tests/golden_replays/
```

## 3. Scenario Tests

Use the scenario framework (`crates/core/src/scenario.rs`) when testing how
gameplay changes affect balance. Scenarios use a fluent builder API to set up
specific game states and assert outcomes.

```bash
cargo test -p roguelike-core --test scenarios
```

Add scenario tests when changing monster stats, player stats, combat formulas,
or AI behavior.

## 4. Invariant Property Tests

Property-based tests using `proptest` that generate random `GameCommand`
sequences and verify fundamental invariants hold after every `step()`.
Located in `crates/core/tests/invariants.rs`.

Invariants checked:
- `player.hp <= player.max_hp`
- `game_over == true` iff `player.hp <= 0`
- Explored set never shrinks
- Dead entities stay dead
- Player is always on a walkable tile (while alive)
- `observe()` and `save_to_json()`/`load_from_json()` never panic

```bash
cargo test -p roguelike-core --test invariants
```

## 5. MCP Integration & Property Tests

Two test files in `crates/mcp/tests/`:

- **`mcp_integration.rs`** — Deterministic tests calling MCP tool methods directly. Covers all tools, response schemas, error paths, session lifecycle.
- **`mcp_proptest.rs`** — Property-based tests generating random sequences of MCP tool calls. Verifies the session layer preserves game invariants through the JSON interface.

```bash
cargo test -p roguelike-mcp
```

## 6. CI `no_std` Verification

Cross-compile `tier_micro` and `rules` for `thumbv6m-none-eabi` to catch
accidental `std` dependencies:

```bash
cargo check -p roguelike-core --no-default-features --target thumbv6m-none-eabi
```

## 7. Balance Drift Detection

A CI test verifies that `game.toml` default values match
`roguelike_core::rules::balance` constants.

## 8. Balance CI

The `.github/workflows/balance.yml` workflow runs on every push that touches
gameplay files. It runs 500+ deterministic games with the headless runner,
compares against a cached baseline, and posts a balance diff verdict
(**STABLE**, **MINOR SHIFT**, or **BALANCE SHIFT**) to the workflow summary
and PR comments.

## 9. Tier Determinism

For each tier, generate a dungeon with a known seed using that tier's mapgen
and compare the resulting tile layout against a stored golden snapshot. This
ensures that a micro-tier seed produces byte-identical output on every
platform — PC, GBA, and C64 all get the same dungeon.

## 10. GameStep Compliance

Verify that `MicroGameState` wrapped in the `GameStep` adapter produces the
same sequence of `GameEvent`s as a direct `MicroGameState` call for a fixed
seed and input sequence.

## 11. Direction Roundtrip

Verify that every `Direction` variant round-trips through
`GameCommand::Move(dir)` encoding/decoding and maps to the expected `(dx, dy)`
offset pair.

## 12. Benchmarks

Criterion benchmarks for performance-sensitive operations:

```bash
cargo bench -p roguelike-core
```

Benchmark reports are uploaded as CI artifacts (30-day retention).

## 13. Platform-Specific Tests

- **C64**: `mos-test` for MOS simulator tests, VICE `-keybuf` for integration.
  See [C64 platform guide](platforms/c64-platform-guide.md#7-c64-specific-tests).
- **GBA**: `mgba-rom-test` headless verification.
  See [GBA port design](platforms/gba-port.md).

## CI Pipeline

The `.github/workflows/ci.yml` workflow runs on every push and PR. Four jobs:

1. **Lint** — `cargo fmt --check` + `cargo clippy` (including `raw-usb` feature)
2. **Test** — `cargo test --workspace` on 4 platforms: Linux x86_64, Linux ARM64, macOS ARM64, Windows x86_64
3. **Benchmark** — `cargo bench -p roguelike-core` with Criterion reports
4. **Audit** — `cargo audit` for known vulnerabilities (advisory, non-blocking)
