---
name: c64build
description: Build the C64 port using the Docker rust-mos toolchain, optionally deploy to VICE.
---

# C64 Build Skill

Build the C64 roguelike port using the Docker rust-mos toolchain and optionally deploy to VICE.

## Usage

- `/c64build` — Build only
- `/c64build deploy` — Build and deploy to VICE
- `/c64build size` — Build with linker map and show RAM/hiram usage
- `/c64build profile` — Build, then profile 50 turns via VICE (requires xdotool + $DISPLAY)

## Instructions

### Build

Always build from the project root using `make` in `crates/c64/`:

```bash
make -C crates/c64 build
```

This runs the Docker rust-mos image, patches the workspace edition 2024->2021 for compatibility, and restores it after build. The PRG output is at `crates/c64/target/mos-c64-none/release/roguelike-c64` (note: the C64 crate has its own `target/` directory, NOT the workspace root `target/`).

**NEVER use plain `cargo build` for the C64 target.** The C64 requires the rust-mos Docker toolchain.

### Size Check

If the user asked for `size`:

```bash
make -C crates/c64 size
```

Generates a linker map and reports per-section RAM/hiram usage. This is the primary way to check if the build fits in memory.

### Deploy to VICE

If the user asked for `deploy`, after a successful build:

```bash
cp crates/c64/target/mos-c64-none/release/roguelike-c64 /mnt/chromeos/MyFiles/Downloads/roguelike-builds/roguelike-c64.prg
printf 'l "\\\\tsclient\\Local Storage\\Download\\roguelike-builds\\roguelike-c64.prg" 0\n' | ncat -w 5 10.0.27.44 6510
```

This copies the PRG to the shared ChromeOS downloads folder, then loads it into VICE running on the Windows machine (10.0.27.44:6510) via the remote monitor protocol.

### Profile

If the user asked for `profile`:

```bash
make -C crates/c64 profile
```

Launches VICE with the text remote monitor and xdotool keyboard injection, plays 50 turns automatically, and reports per-function cycle counts using VICE's built-in profiler. Requires `x64sc`, `xdotool`, and `$DISPLAY`.

Options via make variables: `PROFILE_TURNS=50`, `PROFILE_MODE=builtin` (or `mapgen` for map generation cost).

Note: LTO inlines most game functions into `game_loop`. Only overlay functions (`compute_fov`, `generate`) survive as separate callable symbols. The built-in profiler tracks these via JSR/RTS regardless.

## Constraints

- The C64 uses `no_std` — no heap, no `HashMap`, no `String`.
- Screen codes are PETSCII, not ASCII. Check `crates/c64/src/render.rs` for the character mapping.
- Memory is tight (~4.5 KB free in RAM). Run `make -C crates/c64 size` to check per-section usage.
- Each new function adds a static stack frame to `.noinit`. Minimize function count.
- Never use `opt-level = "z"` — it causes codegen bugs with seed input on rust-mos.
- Never use `core::mem::transmute` for function pointer trampolines — it corrupts imaginary register state.
