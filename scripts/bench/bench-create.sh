#!/usr/bin/env bash
# Measure one cell of the materialization matrix.
#
# Usage: bench-create.sh <approach> <size> <N> [csv_path] [--parallel P]
#   approach: A=git worktree, B=cp -R, C=cp -cR, D=rsync hardlink,
#             E=ivk-pure (clonefile WT only), F=ivk-hybrid (worktree + clonefile WT)
#   size:     S | M | MD | L  (must have been generated with gen-repo.sh)
#   N:        number of workspaces to create
#   --parallel P: spawn P concurrent creates via xargs (default 1 = serial)
#
# Emits one CSV row to stdout (and appends to csv_path if given):
#   approach,size,N,parallel,create_ms,disk_apparent_kb,disk_actual_kb,disk_real_kb,cleanup_ms,first_edit_ms
#
# disk_apparent_kb = du -skA  (sum of file sizes)
# disk_actual_kb   = du -sk   (per-file allocated blocks; misleading on APFS clones
#                              because du does not credit shared extents)
# disk_real_kb     = df delta (volume free-space drop; ground truth)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=lib.sh
source "$SCRIPT_DIR/lib.sh"

APPROACH="${1:?usage: bench-create.sh <A|B|C|D|E|F> <S|M|MD|L> <N> [csv] [--parallel P]}"
SIZE="${2:?size required}"
N="${3:?N required}"
shift 3 || true

CSV_OUT=""
PARALLEL=1
while [[ $# -gt 0 ]]; do
  case "$1" in
    --parallel) PARALLEL="$2"; shift 2 ;;
    *)          CSV_OUT="$1";  shift   ;;
  esac
done

case "$APPROACH" in A|B|C|D|E|F|G) ;; *) echo "unknown approach: $APPROACH" >&2; exit 2 ;; esac
case "$SIZE"     in S|M|MD|L)    ;; *) echo "unknown size: $SIZE"         >&2; exit 2 ;; esac

REPO="$ROOT/.cache/repo-$SIZE"
WS_DIR="$ROOT/.cache/ws-$APPROACH-$SIZE-N$N-P$PARALLEL"

if [[ ! -f "$REPO/.git/HEAD" ]]; then
  echo "ERROR: $REPO not found. Run scripts/bench/gen-repo.sh $SIZE first." >&2
  exit 2
fi

CREATE_ONE="$SCRIPT_DIR/create-one.sh"

# Always start clean; any leftover from a previous run skews timing and disk.
$RM -rf "$WS_DIR"
mkdir -p "$WS_DIR"

# Approaches A and F record metadata under $REPO/.git/worktrees/.
# Prune stale entries from prior runs so add doesn't reject paths.
/usr/bin/git -C "$REPO" worktree prune

fs_sync
DF_BEFORE_KB=$(df_free_kb "$WS_DIR")
t0=$(now_ms)
if [[ "$PARALLEL" -gt 1 ]]; then
  /usr/bin/seq 1 "$N" | /usr/bin/xargs -P "$PARALLEL" -I {} \
    /bin/bash "$CREATE_ONE" "$APPROACH" "$REPO" "$WS_DIR/ws-{}"
else
  for i in $(/usr/bin/seq 1 "$N"); do
    /bin/bash "$CREATE_ONE" "$APPROACH" "$REPO" "$WS_DIR/ws-$i"
  done
fi
fs_sync
t1=$(now_ms)
CREATE_MS=$((t1 - t0))
DF_AFTER_KB=$(df_free_kb "$WS_DIR")
DISK_REAL_KB=$((DF_BEFORE_KB - DF_AFTER_KB))

DISK_APPARENT_KB=$(du_apparent_kb "$WS_DIR")
DISK_ACTUAL_KB=$(du_actual_kb "$WS_DIR")

# First-edit cost: append a small payload to one file in workspace 1.
EDIT_TARGET=$(/usr/bin/find "$WS_DIR/ws-1" -type f -not -path '*/.git/*' 2>/dev/null | /usr/bin/head -1 || true)
if [[ -n "$EDIT_TARGET" ]]; then
  fs_sync
  t0=$(now_ms)
  /bin/echo "ivk-bench-edit" >> "$EDIT_TARGET"
  fs_sync
  t1=$(now_ms)
  FIRST_EDIT_MS=$((t1 - t0))
else
  FIRST_EDIT_MS=-1
fi

# Cleanup phase
fs_sync
t0=$(now_ms)
case "$APPROACH" in
  A|F|G)
    # All three register worktrees in the source repo; same cleanup path.
    for i in $(/usr/bin/seq 1 "$N"); do
      /usr/bin/git -C "$REPO" worktree remove --force "$WS_DIR/ws-$i" >/dev/null 2>&1 || true
    done
    $RM -rf "$WS_DIR"
    /usr/bin/git -C "$REPO" worktree prune
    ;;
  *)
    $RM -rf "$WS_DIR"
    ;;
esac
fs_sync
t1=$(now_ms)
CLEANUP_MS=$((t1 - t0))

ROW="$APPROACH,$SIZE,$N,$PARALLEL,$CREATE_MS,$DISK_APPARENT_KB,$DISK_ACTUAL_KB,$DISK_REAL_KB,$CLEANUP_MS,$FIRST_EDIT_MS"
echo "$ROW"
if [[ -n "$CSV_OUT" ]]; then
  echo "$ROW" >> "$CSV_OUT"
fi
