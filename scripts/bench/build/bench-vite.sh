#!/usr/bin/env bash
# Build-artifact spike: TypeScript / Vite.
#
# Compares disk + time for N parallel workspaces of the Vite fixture under:
#   - Approach A (git worktree): each worktree gets its own checked-out copy
#     of node_modules + dist (fixture commits them, so checkout produces them).
#   - Approach G (Rust clonewt): each workspace is clonefile'd, including
#     node_modules + dist (block-shared with the source).
#
# Then runs `pnpm build` in each workspace to measure how much disk fresh
# build outputs add per workspace.
#
# Usage: bench-vite.sh <N> [csv_out]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BENCH_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
# shellcheck source=../lib.sh
source "$BENCH_DIR/lib.sh"

N="${1:?usage: bench-vite.sh <N> [csv_out]}"
CSV_OUT="${2:-}"

FIX="$ROOT/.cache/fixture-vite"
STORE="$ROOT/.cache/pnpm-store"
CLONEWT="$ROOT/target/release/clonewt"

if [[ ! -f "$FIX/.git/HEAD" ]]; then
  echo "ERROR: fixture missing. Run setup-vite.sh first." >&2
  exit 2
fi
if [[ ! -x "$CLONEWT" ]]; then
  echo "ERROR: clonewt binary missing. (cd clonewt && cargo build --release)" >&2
  exit 2
fi

run_cell() {
  local approach="$1"
  local stage="$2"          # "post-clone" or "post-rebuild"
  local workdir="$3"
  local before_kb after_kb t_create_ms t_build_ms cleanup_ms

  $RM -rf "$workdir"
  mkdir -p "$workdir"
  /usr/bin/git -C "$FIX" worktree prune

  fs_sync
  before_kb=$(df_free_kb "$workdir")
  t0=$(now_ms)
  for i in $(/usr/bin/seq 1 "$N"); do
    /bin/bash "$BENCH_DIR/create-one.sh" "$approach" "$FIX" "$workdir/ws-$i"
  done
  fs_sync
  t1=$(now_ms)
  t_create_ms=$((t1 - t0))
  after_kb=$(df_free_kb "$workdir")
  local disk_after_clone_kb=$((before_kb - after_kb))

  # Stage 1: report post-clone disk
  emit "$approach" "$N" "post-clone" "$t_create_ms" "$disk_after_clone_kb" "0"

  # Stage 2: pnpm build in each workspace (serial to keep measurement clean).
  # Each ws inherits store-dir from the fixture's .npmrc, so pnpm reuses the
  # shared content store.
  fs_sync
  before_kb=$(df_free_kb "$workdir")
  t0=$(now_ms)
  # Capture build stdout/stderr to a per-workspace log; do not pass --silent
  # (vite errors on the flag in this version) and do not swallow real failures.
  local build_log="$workdir/build.log"
  : > "$build_log"
  for i in $(/usr/bin/seq 1 "$N"); do
    if ! (cd "$workdir/ws-$i" && pnpm build) >> "$build_log" 2>&1; then
      echo "  WARN: build failed in ws-$i (see $build_log)" >&2
    fi
  done
  fs_sync
  t1=$(now_ms)
  t_build_ms=$((t1 - t0))
  after_kb=$(df_free_kb "$workdir")
  local disk_after_build_kb=$(( (disk_after_clone_kb) + (before_kb - after_kb) ))

  emit "$approach" "$N" "post-rebuild" "$t_create_ms" "$disk_after_build_kb" "$t_build_ms"

  # Cleanup
  fs_sync
  t0=$(now_ms)
  case "$approach" in
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
  fs_sync
  t1=$(now_ms)
  cleanup_ms=$((t1 - t0))
  echo "  cleanup: ${cleanup_ms} ms" >&2
}

emit() {
  local approach="$1" N="$2" stage="$3" create_ms="$4" disk_kb="$5" build_ms="$6"
  local row="vite,$approach,$N,$stage,$create_ms,$disk_kb,$build_ms"
  echo "$row"
  if [[ -n "$CSV_OUT" ]]; then
    echo "$row" >> "$CSV_OUT"
  fi
}

if [[ -n "$CSV_OUT" && ! -f "$CSV_OUT" ]]; then
  echo "scenario,approach,N,stage,create_ms,disk_real_kb,build_ms" > "$CSV_OUT"
fi

echo "=== Vite bench N=$N ===" >&2
for approach in A G; do
  echo ">>> approach $approach" >&2
  run_cell "$approach" "" "$ROOT/.cache/ws-vite-$approach-N$N"
done
