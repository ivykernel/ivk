#!/usr/bin/env bash
# Live cargo-build race: run `cargo build --release` in parallel on the same
# Rust fixture, materialized two ways. The git worktree's cargo invalidates
# its fingerprints (different absolute paths from the source repo) and does
# a full rebuild; the ivk workspace inherits the source's clonefile'd
# `target/` so cargo finds nothing to do and exits in <1 s.
#
# Usage: scripts/bench/live-build-race.sh [FIX]
#   FIX = path to fixture-rust (default .cache/fixture-rust, made by
#         scripts/bench/build/setup-rust.sh)
#
# Designed to be recorded with vhs (see demos/build-race.tape).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

FIX="${1:-$ROOT/.cache/fixture-rust}"
CREATE_ONE="$SCRIPT_DIR/create-one.sh"
GIT_WS="$ROOT/.cache/build-race-git"
IVK_WS="$ROOT/.cache/build-race-ivk"

if [[ ! -f "$FIX/.git/HEAD" ]]; then
  echo "ERROR: $FIX not found. Run scripts/bench/build/setup-rust.sh first." >&2
  exit 2
fi

now_ms() { /usr/bin/perl -MTime::HiRes=time -E 'say int(time()*1000)'; }
fmt_s() {  /usr/bin/perl -E 'printf("%5.1fs\n", $ARGV[0]/1000)' -- "$1"; }

# Best-effort cleanup of prior runs.
for d in "$GIT_WS" "$IVK_WS"; do
  if [[ -d "$d" ]]; then
    /usr/bin/git -C "$FIX" worktree remove --force "$d" 2>/dev/null || true
    /bin/rm -rf "$d"
  fi
done
/usr/bin/git -C "$FIX" worktree prune

# Setup both workspaces (silent; only the build is the race).
/usr/bin/git -C "$FIX" worktree add -q --detach "$GIT_WS" HEAD >/dev/null
/bin/bash "$CREATE_ONE" G "$FIX" "$IVK_WS" >/dev/null

clear
/usr/bin/tput civis
trap '/usr/bin/tput cnorm; /usr/bin/tput cup 8 0' EXIT
printf "parallel \033[1mcargo build --release\033[0m on the same Rust fixture\n\n"

# Launch both cargo builds in parallel.
mkdir -p "$GIT_WS/.race-log" "$IVK_WS/.race-log"
( cd "$GIT_WS" && /Users/yamatsutaeitarou/.cargo/bin/cargo build --release >"$GIT_WS/.race-log/out" 2>&1 ) &
GIT_PID=$!
GIT_START=$(now_ms)
( cd "$IVK_WS" && /Users/yamatsutaeitarou/.cargo/bin/cargo build --release >"$IVK_WS/.race-log/out" 2>&1 ) &
IVK_PID=$!
IVK_START=$(now_ms)

git_done="" ; ivk_done=""
git_final=0 ; ivk_final=0

while [[ -z "$git_done" || -z "$ivk_done" ]]; do
  now=$(now_ms)

  if [[ -z "$git_done" ]] && ! /bin/kill -0 "$GIT_PID" 2>/dev/null; then
    git_done="done"
    git_final=$(( now - GIT_START ))
  fi
  if [[ -z "$ivk_done" ]] && ! /bin/kill -0 "$IVK_PID" 2>/dev/null; then
    ivk_done="done"
    ivk_final=$(( now - IVK_START ))
  fi

  git_elapsed=$(( now - GIT_START ))
  ivk_elapsed=$(( now - IVK_START ))

  if [[ -z "$git_done" ]]; then
    git_status="\033[33mbuilding...\033[0m"
    git_show="$(fmt_s $git_elapsed)"
  else
    git_status="\033[1;32mDONE in $(fmt_s $git_final | sed 's/^ *//')\033[0m"
    git_show="$(fmt_s $git_final)"
  fi
  if [[ -z "$ivk_done" ]]; then
    ivk_status="\033[33mbuilding...\033[0m"
    ivk_show="$(fmt_s $ivk_elapsed)"
  else
    ivk_status="\033[1;32mDONE in $(fmt_s $ivk_final | sed 's/^ *//')\033[0m"
    ivk_show="$(fmt_s $ivk_final)"
  fi

  /usr/bin/tput cup 3 0
  printf "  git worktree   elapsed %s   %b\033[K\n" "$git_show" "$git_status"
  printf "  ivk            elapsed %s   %b\033[K\n" "$ivk_show" "$ivk_status"

  sleep 0.1
done

# Final summary.
/usr/bin/tput cup 6 0
ratio=$(/usr/bin/perl -E 'printf("%.1f", $ARGV[0] / $ARGV[1])' -- "$git_final" "$(($ivk_final > 0 ? $ivk_final : 1))")
printf "\n  ivk finished \033[1;36m%s×\033[0m faster than git worktree on the same build.\n" "$ratio"

# Cleanup.
/usr/bin/git -C "$FIX" worktree remove --force "$GIT_WS" >/dev/null 2>&1 || true
/usr/bin/git -C "$FIX" worktree remove --force "$IVK_WS" >/dev/null 2>&1 || true
/bin/rm -rf "$GIT_WS" "$IVK_WS"
/usr/bin/git -C "$FIX" worktree prune
