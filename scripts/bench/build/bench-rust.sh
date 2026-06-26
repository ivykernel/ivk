#!/usr/bin/env bash
# Build-artifact spike: Rust / Cargo.
#
# Scenarios compared at N workspaces:
#   A          git worktree, each ws has its own target/ committed in the fixture
#   G          ivk clonewt, each ws's target/ is clonefile'd from the source
#   G-shared   ivk clonewt + CARGO_TARGET_DIR pointing to one shared dir
#
# Rebuild step: `cargo build --release` in each workspace, serial.
#
# Usage: bench-rust.sh <N> [csv_out]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BENCH_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
# shellcheck source=../lib.sh
source "$BENCH_DIR/lib.sh"

N="${1:?usage: bench-rust.sh <N> [csv_out]}"
CSV_OUT="${2:-}"

FIX="$ROOT/.cache/fixture-rust"
SHARED_TARGET="$ROOT/.cache/cargo-target-shared"
CLONEWT="$ROOT/target/release/clonewt"

if [[ ! -f "$FIX/.git/HEAD" ]]; then
  echo "ERROR: fixture missing. Run setup-rust.sh first." >&2
  exit 2
fi
if [[ ! -x "$CLONEWT" ]]; then
  echo "ERROR: clonewt binary missing." >&2
  exit 2
fi

emit() {
  local approach="$1" N="$2" stage="$3" create_ms="$4" disk_kb="$5" build_ms="$6"
  local row="rust,$approach,$N,$stage,$create_ms,$disk_kb,$build_ms"
  echo "$row"
  if [[ -n "$CSV_OUT" ]]; then echo "$row" >> "$CSV_OUT"; fi
}

run_cell() {
  local approach="$1"      # A, G, or G-shared
  local workdir="$2"
  local cargo_env="$3"     # extra env for cargo, e.g. "CARGO_TARGET_DIR=..."
  local create_approach="$approach"
  if [[ "$create_approach" == "G-shared" ]]; then
    create_approach="G"
  fi

  $RM -rf "$workdir" "$SHARED_TARGET"
  mkdir -p "$workdir"
  if [[ "$approach" == "G-shared" ]]; then mkdir -p "$SHARED_TARGET"; fi
  /usr/bin/git -C "$FIX" worktree prune

  fs_sync
  local before_kb t0 t1 t_create_ms after_kb disk_after_clone_kb
  before_kb=$(df_free_kb "$workdir")
  t0=$(now_ms)
  for i in $(/usr/bin/seq 1 "$N"); do
    /bin/bash "$BENCH_DIR/create-one.sh" "$create_approach" "$FIX" "$workdir/ws-$i"
  done
  fs_sync
  t1=$(now_ms)
  t_create_ms=$((t1 - t0))
  after_kb=$(df_free_kb "$workdir")
  disk_after_clone_kb=$((before_kb - after_kb))
  emit "$approach" "$N" "post-clone" "$t_create_ms" "$disk_after_clone_kb" "0"

  # Rebuild step
  fs_sync
  before_kb=$(df_free_kb "$workdir")
  t0=$(now_ms)
  local build_log="$workdir/build.log"
  : > "$build_log"
  for i in $(/usr/bin/seq 1 "$N"); do
    if ! (cd "$workdir/ws-$i" && env $cargo_env cargo build --release) >> "$build_log" 2>&1; then
      echo "  WARN: build failed in ws-$i (see $build_log)" >&2
    fi
  done
  fs_sync
  t1=$(now_ms)
  local t_build_ms=$((t1 - t0))
  after_kb=$(df_free_kb "$workdir")
  local disk_total_kb=$(( disk_after_clone_kb + (before_kb - after_kb) ))
  # For G-shared, also account for the shared target dir.
  if [[ "$approach" == "G-shared" ]]; then
    local shared_kb
    shared_kb=$(/usr/bin/du -sk "$SHARED_TARGET" | awk '{print $1}')
    disk_total_kb=$(( disk_total_kb + shared_kb ))
  fi
  emit "$approach" "$N" "post-rebuild" "$t_create_ms" "$disk_total_kb" "$t_build_ms"

  # Cleanup
  fs_sync
  t0=$(now_ms)
  case "$create_approach" in
    A|F|G)
      for i in $(/usr/bin/seq 1 "$N"); do
        /usr/bin/git -C "$FIX" worktree remove --force "$workdir/ws-$i" >/dev/null 2>&1 || true
      done
      $RM -rf "$workdir"
      /usr/bin/git -C "$FIX" worktree prune
      ;;
    *)
      $RM -rf "$workdir"
      ;;
  esac
  $RM -rf "$SHARED_TARGET"
  fs_sync
  t1=$(now_ms)
  echo "  cleanup: $((t1 - t0)) ms" >&2
}

if [[ -n "$CSV_OUT" && ! -f "$CSV_OUT" ]]; then
  echo "scenario,approach,N,stage,create_ms,disk_real_kb,build_ms" > "$CSV_OUT"
fi

echo "=== Rust bench N=$N ===" >&2
echo ">>> approach A" >&2
run_cell "A"        "$ROOT/.cache/ws-rust-A-N$N"        ""
echo ">>> approach G" >&2
run_cell "G"        "$ROOT/.cache/ws-rust-G-N$N"        ""
echo ">>> approach G-shared (CARGO_TARGET_DIR shared)" >&2
run_cell "G-shared" "$ROOT/.cache/ws-rust-Gs-N$N"       "CARGO_TARGET_DIR=$SHARED_TARGET"
