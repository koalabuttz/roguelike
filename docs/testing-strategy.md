# Testing Strategy

**Project-wide testing approach for the roguelike dungeon crawler.**
Covers core logic tests, CI verification, cross-platform determinism, and
property tests. For C64-specific tests (mos-test, VICE integration), see the
[C64 platform guide](platforms/c64-platform-guide.md#7-c64-specific-tests). For
per-feature test plans, see the individual phases in the
[gameplay implementation plan](design/gameplay-implementation-plan.md).

---

## 1. Core Tests

`cargo test -p roguelike-core` on the host with standard rustc. No emulator
needed. All game logic tests run on the host.

## 2. CI `no_std` Verification

Cross-compile `tier_micro` for `thumbv6m-none-eabi` to catch accidental `std`
dependencies:

```bash
cargo check -p roguelike-core --no-default-features --target thumbv6m-none-eabi
```

## 3. Balance Drift Detection

A CI test verifies that `game.toml` default values match
`roguelike_core::rules::balance` constants.

## 4. Tier Determinism

For each tier, generate a dungeon with a known seed using that tier's mapgen
and compare the resulting tile layout against a stored golden snapshot. This
ensures that a micro-tier seed produces byte-identical output on every
platform — PC, GBA, and C64 all get the same dungeon. Same test for compact
and standard tiers.

## 5. GameStep Compliance

Verify that `MicroGameState` wrapped in the `GameStep` adapter produces the
same sequence of `GameEvent`s as a direct `MicroGameState` call for a fixed
seed and input sequence.

## 6. Direction Roundtrip

Verify that every `Direction` variant round-trips through
`GameCommand::Move(dir)` encoding/decoding and maps to the expected `(dx, dy)`
offset pair.

## 7. Property Tests

Property tests (in `core/tests/`) verify invariants across the full input
space:

```rust
#[test]
fn damage_never_negative() {
    for atk in 0..=20u8 {
        for def in 0..=20u8 {
            assert!(combat::damage(atk, def) <= atk);
        }
    }
}

#[test]
fn lfsr_has_full_period() {
    let mut rng = LfsrRng::new(0xACE1);
    let start = rng.state();
    for i in 0u32..65536 {
        rng.next_u8(); rng.next_u8();
        if rng.state() == start {
            assert_eq!(i + 1, 65535, "LFSR period too short");
            return;
        }
    }
    panic!("LFSR did not cycle");
}

#[test]
fn room_intersection_is_symmetric() {
    let a = Room { x: 2, y: 2, w: 5, h: 5 };
    let b = Room { x: 4, y: 4, w: 5, h: 5 };
    assert_eq!(a.intersects(&b), b.intersects(&a));
}
```

## 8. Platform-Specific Tests

- **C64**: `mos-test` for MOS simulator tests, VICE `-keybuf` for integration.
  See [C64 platform guide](platforms/c64-platform-guide.md#7-c64-specific-tests).
- **GBA**: `mgba-rom-test` headless verification.
  See [GBA port design](platforms/gba-port.md).
- **Per-feature tests**: See individual feature sections in the
  [gameplay implementation plan](design/gameplay-implementation-plan.md).
