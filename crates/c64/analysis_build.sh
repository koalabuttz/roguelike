#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"
mkdir -p analysis

sudo docker run --rm \
  -e PATH=/usr/local/rust-mos/bin:/usr/local/bin:/usr/bin:/bin \
  -v "$(realpath ../..)":/project \
  -w /project/crates/c64 \
  ghcr.io/koalabuttz/rust-mos:ac2fb2277-4537158-4aaa40e16 \
  bash -euo pipefail -c '
    export CARGO_TARGET_DIR=target/analysis
    # The debuginfo=1 spike preserved PRG bytes but this pinned Rust-MOS image
    # emitted no qualifying line tables, so the retained build is symbol-only.
    export RUSTFLAGS="-C link-arg=-Tlink.ld -C link-arg=-Wl,--no-check-sections -C link-arg=-Wl,--allow-multiple-definition -C link-arg=-Wl,-Map=analysis/map.txt -Z mir-opt-level=1"
    cargo build --release

    prg=target/analysis/mos-c64-none/release/roguelike-c64
    mapfile -t map_stems < <(
      sed -n "s#.*deps/\\(roguelike_c64-[^. /]*\\)\\..*#\\1#p" analysis/map.txt |
        sort -u
    )
    if [ "${#map_stems[@]}" -ne 1 ]; then
      echo "map names ${#map_stems[@]} candidate Roguelike ELFs; expected one" >&2
      exit 1
    fi
    published="target/analysis/mos-c64-none/release/deps/${map_stems[0]}.elf"
    if [ ! -f "$published" ]; then
      echo "map-named analysis ELF does not exist: $published" >&2
      exit 1
    fi
    candidate="${published}.payload"
    llvm-objcopy -O binary "$published" "$candidate"
    if ! cmp -s "$candidate" <(tail -c +3 "$prg"); then
      rm -f "$candidate"
      echo "map-named analysis ELF does not match the PRG payload" >&2
      exit 1
    fi
    rm -f "$candidate"
    cp "$prg" analysis/roguelike-c64.prg
    cp "$published" analysis/roguelike-c64.elf
  '
