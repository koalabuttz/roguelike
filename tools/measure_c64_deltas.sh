#!/bin/bash
# Measure C64 code size (and optionally performance) deltas for commits
# that lack concrete metrics in c64-optimization-log.md.
#
# Usage:
#   ./tools/measure_c64_deltas.sh                    # size only, all commits
#   ./tools/measure_c64_deltas.sh --profile           # size + VICE profiling for Group B
#   ./tools/measure_c64_deltas.sh --commit 7150872    # single commit
#
# All builds use the current Docker image regardless of what the commit's
# Makefile specified, to reduce variables.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DOCKER_IMAGE="ghcr.io/koalabuttz/rust-mos:ac2fb2277-4537158-4aaa40e16"
DOCKER_PATH="/usr/local/rust-mos/bin:/usr/local/bin:/usr/bin:/bin"
PROFILE_SEED="1a2b"
PROFILE_TURNS=20
DO_PROFILE=false
SINGLE_COMMIT=""
RESULTS_DIR="$REPO_ROOT/tools/output/c64-deltas"

# Group A: size only (have link.ld, memmap.py at both before/after)
GROUP_A_COMMITS=(7150872 e4ea693 e8a9359 7d75e93)
# Group B: size + profile (early commits, no link.ld, no tooling)
GROUP_B_COMMITS=(8b7314c e95b911 9e46660 32f6ce5)

while [[ $# -gt 0 ]]; do
    case "$1" in
        --profile) DO_PROFILE=true; shift ;;
        --commit)  SINGLE_COMMIT="$2"; shift 2 ;;
        *) echo "Unknown arg: $1"; exit 1 ;;
    esac
done

mkdir -p "$RESULTS_DIR"

# ─────────────────────────────────────────────────────────────────────
# Build one worktree and capture size data
# ─────────────────────────────────────────────────────────────────────
build_and_measure() {
    local ref="$1"       # git ref (commit hash or hash^)
    local label="$2"     # "before" or "after"
    local commit="$3"    # the commit being measured (for output naming)
    local workdir="/tmp/c64-measure-${label}-$$"
    local outfile="$RESULTS_DIR/${commit}_${label}.txt"

    echo "[$commit $label] Creating worktree at $ref ..."
    git -C "$REPO_ROOT" worktree add "$workdir" "$ref" --detach --quiet 2>/dev/null

    # --- Patch the worktree ---

    # 1. Edition 2024 → 2021 for rust-mos
    sed -i 's/edition = "2024"/edition = "2021"/' "$workdir/Cargo.toml"

    # 2. Copy tooling from HEAD
    git -C "$REPO_ROOT" show HEAD:crates/c64/memmap.py > "$workdir/crates/c64/memmap.py" 2>/dev/null || true
    git -C "$REPO_ROOT" show HEAD:crates/c64/profile.py > "$workdir/crates/c64/profile.py" 2>/dev/null || true

    # 3. Construct RUSTFLAGS (env var overrides .cargo/config.toml)
    local rustflags="-C link-arg=-Wl,-Map,map.txt"
    if [ -f "$workdir/crates/c64/link.ld" ]; then
        rustflags="-C link-arg=-Tlink.ld -C link-arg=-Wl,--no-check-sections -C link-arg=-Wl,--allow-multiple-definition $rustflags"
    fi

    # 4. Build
    echo "[$commit $label] Building (RUSTFLAGS: ${rustflags:0:60}...) ..."
    if ! sudo docker run --rm \
        -e "PATH=$DOCKER_PATH" \
        -e "RUSTFLAGS=$rustflags" \
        -v "$workdir:/project" \
        -w /project/crates/c64 \
        "$DOCKER_IMAGE" \
        cargo build --release 2>"$RESULTS_DIR/${commit}_${label}_build.log"; then
        echo "[$commit $label] BUILD FAILED — see ${commit}_${label}_build.log"
        git -C "$REPO_ROOT" worktree remove "$workdir" --force 2>/dev/null || true
        return 1
    fi

    # 5. Capture size data
    {
        echo "=== $commit $label ($ref) ==="
        echo ""

        # PRG file size
        local prg="$workdir/crates/c64/target/mos-c64-none/release/roguelike-c64"
        if [ -f "$prg" ]; then
            echo "PRG size: $(stat -c%s "$prg") bytes"
        else
            echo "PRG: not found"
        fi
        echo ""

        # Linker map analysis
        local mapfile="$workdir/crates/c64/map.txt"
        if [ -f "$mapfile" ]; then
            python3 "$workdir/crates/c64/memmap.py" "$mapfile" 2>&1
        else
            echo "map.txt: not found"
        fi
    } > "$outfile"

    echo "[$commit $label] Size data → $outfile"
    cat "$outfile"
    echo ""

    # Store paths for profiling (caller may need them)
    eval "${label^^}_WORKDIR=$workdir"
    eval "${label^^}_PRG=$workdir/crates/c64/target/mos-c64-none/release/roguelike-c64"
    eval "${label^^}_MAP=$workdir/crates/c64/map.txt"
    eval "${label^^}_PROFILE_PY=$workdir/crates/c64/profile.py"
}

# ─────────────────────────────────────────────────────────────────────
# Profile one build via VICE
# ─────────────────────────────────────────────────────────────────────
profile_build() {
    local label="$1"
    local commit="$2"
    local map_file="$3"
    local prg_file="$4"
    local profile_py="$5"
    local outfile="$RESULTS_DIR/${commit}_${label}_profile.txt"

    if [ ! -f "$map_file" ] || [ ! -f "$prg_file" ]; then
        echo "[$commit $label] Cannot profile: missing map or PRG"
        return 1
    fi

    echo "[$commit $label] Profiling ($PROFILE_TURNS turns, seed=$PROFILE_SEED) ..."
    python3 "$profile_py" "$map_file" "$prg_file" \
        --seed "$PROFILE_SEED" --turns "$PROFILE_TURNS" \
        > "$outfile" 2>&1 || true

    echo "[$commit $label] Profile → $outfile"
    cat "$outfile"
    echo ""
}

# ─────────────────────────────────────────────────────────────────────
# Measure one commit (before/after)
# ─────────────────────────────────────────────────────────────────────
measure_commit() {
    local commit="$1"
    local do_profile="$2"

    echo "============================================================"
    echo "  Measuring commit: $commit"
    echo "  $(git -C "$REPO_ROOT" log --oneline -1 "$commit")"
    echo "============================================================"
    echo ""

    # Build before and after
    BEFORE_WORKDIR="" BEFORE_PRG="" BEFORE_MAP="" BEFORE_PROFILE_PY=""
    AFTER_WORKDIR="" AFTER_PRG="" AFTER_MAP="" AFTER_PROFILE_PY=""

    build_and_measure "${commit}^" "before" "$commit" || true
    build_and_measure "$commit"    "after"  "$commit" || true

    # Profile if requested
    if [ "$do_profile" = true ] && [ -n "$BEFORE_WORKDIR" ] && [ -n "$AFTER_WORKDIR" ]; then
        profile_build "before" "$commit" "$BEFORE_MAP" "$BEFORE_PRG" "$BEFORE_PROFILE_PY"
        profile_build "after"  "$commit" "$AFTER_MAP"  "$AFTER_PRG"  "$AFTER_PROFILE_PY"
    fi

    # Cleanup worktrees
    if [ -n "$BEFORE_WORKDIR" ] && [ -d "$BEFORE_WORKDIR" ]; then
        git -C "$REPO_ROOT" worktree remove "$BEFORE_WORKDIR" --force 2>/dev/null || rm -rf "$BEFORE_WORKDIR"
    fi
    if [ -n "$AFTER_WORKDIR" ] && [ -d "$AFTER_WORKDIR" ]; then
        git -C "$REPO_ROOT" worktree remove "$AFTER_WORKDIR" --force 2>/dev/null || rm -rf "$AFTER_WORKDIR"
    fi

    echo ""
}

# ─────────────────────────────────────────────────────────────────────
# Main
# ─────────────────────────────────────────────────────────────────────
echo "C64 Optimization Delta Measurement"
echo "Docker: $DOCKER_IMAGE"
echo "Profile: $DO_PROFILE (seed=$PROFILE_SEED, turns=$PROFILE_TURNS)"
echo "Results: $RESULTS_DIR"
echo ""

if [ -n "$SINGLE_COMMIT" ]; then
    # Check if it's a Group B commit
    is_group_b=false
    for c in "${GROUP_B_COMMITS[@]}"; do
        [[ "$SINGLE_COMMIT" == "$c"* ]] && is_group_b=true
    done
    measure_commit "$SINGLE_COMMIT" "$( [ "$DO_PROFILE" = true ] && [ "$is_group_b" = true ] && echo true || echo false )"
else
    # Group A: size only
    for commit in "${GROUP_A_COMMITS[@]}"; do
        measure_commit "$commit" false
    done

    # Group B: size, optionally profile
    for commit in "${GROUP_B_COMMITS[@]}"; do
        measure_commit "$commit" "$DO_PROFILE"
    done
fi

echo "============================================================"
echo "  All measurements complete. Results in: $RESULTS_DIR"
echo "============================================================"
