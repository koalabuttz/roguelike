# Cross-tier content foundation

**Status:** Implemented on the `content-foundation` branch. This is the
foundation for Phase 4 item-based progression, not a general-purpose engine
extraction.

## Direction

The original roguelike is the flagship. Near-term work should make it a deeper,
more opinionated game on Standard/PC, Compact/GBA, and Micro/C64. Do not add
another Retro Forge genre pack or restart broad engine extraction unless a
concrete flagship feature exposes a reusable cross-tier seam.

The next gameplay milestone is
[Phase 4 item-based progression](gameplay-implementation-plan.md#phase-4-item-based-progression):
enchantment scrolls, depth-gated equipment, and permanent consumables. Add
those through the portable content workflow below, one vertical slice at a
time, with semantic tests on all three capability tiers.

## Canonical content contract

`crates/core/data/game.toml` is the single authored source for player balance,
game configuration, wandering/depth tuning, the three monsters, and the nine
current items. `roguelike-content` parses and validates it on the host. The
core build script then emits bounded Rust tables into `OUT_DIR`; Micro and
Compact consume generated exhaustive lookups without TOML, allocation, or
`std`. The catalog now contains nine items after the first Phase 4 vertical
slice added the Potion of Toughness. Aggregate tables are emitted only for the
Standard `data-files` path; keeping them out of constrained builds avoids
materializing writable pointer tables on MOS.

`ItemKind` and `MonsterKind` remain stable `u8` identities used by saves and
constrained arrays. Every TOML record therefore has an explicit snake-case
`id`. The portable parser requires every known ID exactly once, rejects unknown
IDs, and normalizes table order before indexing. Display names are not
identities and may be changed safely.

Authored item properties use named intensities from 0 through 15. The compiler
packs them into the existing eight-byte property bag. Equipment attack and
defense are derived from those bags rather than duplicated in TOML.

## Adding portable content

Rebalancing or reskinning an existing kind only requires editing its canonical
TOML record and rebuilding. To add a new kind that works on all tiers:

1. Add a stable discriminant to `ItemKind` or `MonsterKind` and its `ALL_KINDS`
   list without changing existing discriminants.
2. Extend the expected ID list and generated array length in
   `roguelike-content`.
3. Add the canonical TOML record and implement any genuinely new behavior in
   shared rules plus the tier states that consume it.
4. Add compiler validation, cross-tier semantic tests, save compatibility
   coverage, and C64/GBA size/build verification.

TOML-only desktop identities are deliberately unsupported: a kind that cannot
be represented by the fixed GBA/C64 catalogs is not a portable game feature.

## Desktop overrides and iteration

The terminal, headless runner, and MCP server may load a complete validated
`game.toml` from the working directory. It must contain the fixed portable ID
set.

- `F10` validates and stages the file for the next run. It does not mutate the
  active game.
- `Shift+F10` reloads and explicitly reconciles the active run. Known living
  monsters adopt new presentation, AI, sight, and stats; current HP preserves
  its ratio to maximum HP. Dead and unknown entities are untouched.
- Ground items and future spawns use the new catalog immediately. Inventory
  and equipment receive new default property bags only when an instance still
  equals its old default, preserving interacted items.
- Player base stats are never rewritten mid-run. A reconciled run is marked
  development-modified and cannot be treated as a canonical golden replay.

Standard saves capture the active item catalog so a run remains deterministic.
Older saves without a catalog load the compiled defaults. GBA/C64 save layouts
and enum discriminants are unchanged.

## Boundaries to preserve

- The content compiler owns parsing, portable validation, ordering, and Rust
  emission; it does not own gameplay behavior.
- Structural limits, enum identities, packed layouts, and novel mechanics stay
  in Rust.
- Generated content must not introduce heap allocation into Micro or Compact.
- Any content-system change must pass host schema tests, `no_std` checks, the
  full workspace suite, and real GBA/C64 release builds.

The foundation implementation was compared against `master` with `make size`:
301 bytes of normal RAM and 25 bytes of high RAM were free, versus 300 and 24
bytes on the baseline. After the Potion of Toughness slice, a real release
build leaves 181 bytes of normal RAM and 25 bytes of high RAM free. The packed
inventory and save layouts are unchanged. Future catalog changes must repeat
this comparison.
