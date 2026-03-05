---
name: c64build
description: Build the C64 port using the Docker rust-mos toolchain, optionally deploy to VICE.
---

# C64 Build Skill

Build the C64 roguelike port using the Docker rust-mos toolchain and optionally deploy to VICE.

## Usage

- `/c64build` — Build only
- `/c64build deploy` — Build and deploy to VICE
- `/c64build size` — Build and show binary size breakdown

## Instructions

### Build

Always build from the project root using `make` in `crates/c64/`:

```bash
make -C crates/c64 build
```

This runs the Docker rust-mos image, patches the workspace edition 2024->2021 for compatibility, and restores it after build. The PRG output is at `target/mos-c64-none/release/roguelike-c64`.

**NEVER use plain `cargo build` for the C64 target.** The C64 requires the rust-mos Docker toolchain.

### Size Check

If the user asked for `size`:

```bash
make -C crates/c64 size
```

Reports the PRG size and checks against the 16 KB budget.

### Deploy to VICE

If the user asked for `deploy`, after a successful build:

```bash
cp target/mos-c64-none/release/roguelike-c64 /mnt/chromeos/MyFiles/Downloads/roguelike-builds/roguelike-c64.prg
printf 'l "\\\\tsclient\\Local Storage\\Download\\roguelike-builds\\roguelike-c64.prg" 0\n' | ncat -w 5 10.0.27.44 6510
```

This copies the PRG to the shared ChromeOS downloads folder, then loads it into VICE running on the Windows machine (10.0.27.44:6510) via the remote monitor protocol.

## Constraints

- The C64 uses `no_std` — no heap, no `HashMap`, no `String`.
- Screen codes are PETSCII, not ASCII. Check `crates/c64/src/render.rs` for the character mapping.
- Memory is extremely tight (~141 bytes free in RAM). Check the linker map if builds fail with overflow.
- Each new function adds a static stack frame to `.noinit`. Minimize function count.
- Never use `opt-level = "z"` — it causes codegen bugs with seed input on rust-mos.
- Never use `core::mem::transmute` for function pointer trampolines — it corrupts imaginary register state.
