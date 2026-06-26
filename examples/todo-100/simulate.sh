#!/usr/bin/env bash
# Simulate 100 parallel "agents" attacking the todo-100 fixture.
#
# What it does:
#   1. Verify the fixture exists (run setup.sh first).
#   2. For each task in a random sample of 30 (configurable via --pass N):
#        a. ivk new task-NNN     -> materialize a workspace
#        b. patch src/task_NNN.js -> apply the fix in the workspace only
#        c. ivk ch new task-NNN  -> commit + record changeset
#        d. ivk export ch_xxx     -> create agent/task-NNN branch
#   3. For the rest of the 100, intentionally leave them untouched.
#   4. Run ivk gc to demonstrate cleanup of the abandoned workspaces.
#   5. Print a one-line summary suitable for the LP.
#
# This is not a real-agent simulation; it's a deterministic harness that
# exercises every code path the LP claim depends on, so the user can run
# it locally and reproduce the numbers without spending agent budget.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
DEST="$ROOT/.cache/todo-100"
IVK="$ROOT/target/release/ivk"

# Defaults.
PASS_COUNT=30      # how many tasks the "agents" successfully fix
FAIL_COUNT=20      # how many they make broken edits in (then discard)
                   # remaining (100 - PASS - FAIL) are abandoned untouched

while [[ $# -gt 0 ]]; do
  case "$1" in
    --pass) PASS_COUNT="$2"; shift 2 ;;
    --fail) FAIL_COUNT="$2"; shift 2 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

if [[ ! -f "$DEST/.git/HEAD" ]]; then
  echo "ERROR: fixture missing. Run examples/todo-100/setup.sh first." >&2
  exit 2
fi
if [[ ! -x "$IVK" ]]; then
  echo "ERROR: ivk binary missing. cargo build --release --workspace" >&2
  exit 2
fi

cd "$DEST"

# Clear any previous simulation state.
"$IVK" gc --dry-run --json >/dev/null 2>&1 || true
for ws in $(/bin/ls .ivk/workspaces/ 2>/dev/null || true); do
  /usr/bin/git worktree remove --force ".ivk/workspaces/$ws" 2>/dev/null || true
done
/bin/rm -rf .ivk/workspaces .ivk/changesets
/bin/mkdir -p .ivk/workspaces .ivk/changesets
/usr/bin/git worktree prune
# Clear any agent/task-NNN branches from prior runs.
for b in $(/usr/bin/git branch --format='%(refname:short)' 2>/dev/null | /usr/bin/grep -E '^agent/task-' || true); do
  /usr/bin/git branch -D "$b" >/dev/null 2>&1 || true
done

TS=$(/bin/date +%s)
RESULTS_DIR="$ROOT/results/todo-100-${TS}"
/bin/mkdir -p "$RESULTS_DIR"
SUMMARY="$RESULTS_DIR/summary.txt"
CSV="$RESULTS_DIR/per-task.csv"
echo "task,outcome,wall_ms" > "$CSV"

# Reproducible per-run sample using a deterministic SHA-based shuffle.
ALL_TASKS=$(/usr/bin/seq -f "%03g" 1 100)
SHUFFLED=$(echo "$ALL_TASKS" | /usr/bin/awk 'BEGIN{srand(42)} {print rand()"\t"$0}' | /usr/bin/sort -n | /usr/bin/cut -f2)

PASS_LIST=$(echo "$SHUFFLED" | /usr/bin/head -n "$PASS_COUNT")
FAIL_LIST=$(echo "$SHUFFLED" | /usr/bin/sed -n "$((PASS_COUNT+1)),$((PASS_COUNT+FAIL_COUNT))p")
ABANDON_COUNT=$(( 100 - PASS_COUNT - FAIL_COUNT ))

now_ms() { /usr/bin/perl -MTime::HiRes=time -E 'say int(time()*1000)'; }

T0_ALL=$(now_ms)

# --- PASS group: agent fixes the bug, ch new, export ---
echo "[simulate] $PASS_COUNT pass attempts ..." >&2
PASS_OK=0
for i in $PASS_LIST; do
  ws="task-$i"
  t0=$(now_ms)
  "$IVK" new "$ws" --json >/dev/null
  ws_dir=".ivk/workspaces/$ws"
  # Apply the canonical fix: `return n` -> `return n + 1`.
  /usr/bin/sed -i '' 's|return n;|return n + 1;|' "$ws_dir/src/task_${i}.js"
  "$IVK" ch new "$ws" --json > "$RESULTS_DIR/ch-$i.json" 2>/dev/null || true
  if /usr/bin/grep -q '"ok": true' "$RESULTS_DIR/ch-$i.json"; then
    ch_id=$(/usr/bin/python3 -c "import json,sys;print(json.load(open('$RESULTS_DIR/ch-$i.json'))['id'])")
    "$IVK" export "$ch_id" --json >/dev/null
    PASS_OK=$((PASS_OK + 1))
    echo "$ws,pass,$(( $(now_ms) - t0 ))" >> "$CSV"
  else
    echo "$ws,pass-error,$(( $(now_ms) - t0 ))" >> "$CSV"
  fi
done

# --- FAIL group: agent breaks the file further, discards via ws rm ---
echo "[simulate] $FAIL_COUNT failed attempts (discarded) ..." >&2
for i in $FAIL_LIST; do
  ws="task-$i"
  t0=$(now_ms)
  "$IVK" new "$ws" --json >/dev/null
  /bin/echo "// broken edit by failed-agent simulation" >> ".ivk/workspaces/$ws/src/task_${i}.js"
  /bin/echo "syntax error here" >> ".ivk/workspaces/$ws/src/task_${i}.js"
  "$IVK" rm "$ws" --json >/dev/null
  echo "$ws,fail-discarded,$(( $(now_ms) - t0 ))" >> "$CSV"
done

# --- ABANDON group: just spawn the workspace and walk away ---
echo "[simulate] $ABANDON_COUNT abandoned (no work done) ..." >&2
for i in $(echo "$SHUFFLED" | /usr/bin/sed -n "$((PASS_COUNT+FAIL_COUNT+1)),100p"); do
  ws="task-$i"
  t0=$(now_ms)
  "$IVK" new "$ws" --json >/dev/null
  echo "$ws,abandoned,$(( $(now_ms) - t0 ))" >> "$CSV"
done

# Sanity-check what gc sees, then run gc for real.
GC_DRY=$("$IVK" gc --dry-run --json)
GC_REAL=$("$IVK" gc --yes --json 2>/dev/null || "$IVK" gc --json)
# gc only touches orphans; abandoned workspaces are still valid worktrees,
# so to demonstrate bulk-rm we additionally rm --exported (work preserved
# on agent/ branches) and --all on what's left.
"$IVK" ws rm --exported --yes --json >/dev/null 2>&1 || true
"$IVK" ws rm --all --yes --force --json >/dev/null 2>&1 || true
GC_FINAL=$("$IVK" gc --json)

T1_ALL=$(now_ms)

# Compose summary.
{
  echo "=== ivk todo-100 simulation ==="
  echo "timestamp: $(/bin/date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "outcomes:  pass=$PASS_OK  fail-discarded=$FAIL_COUNT  abandoned=$ABANDON_COUNT"
  echo "wall:      $(( T1_ALL - T0_ALL )) ms total"
  echo
  echo "branches created (agent/task-NNN):"
  /usr/bin/git branch --format='  %(refname:short)' | /usr/bin/grep -c '^  agent/task-' \
    | /usr/bin/awk '{print "  count = "$0}'
  echo
  echo "final gc:"
  echo "$GC_FINAL" | /usr/bin/python3 -c 'import json,sys; d=json.load(sys.stdin); print("  bytes_reclaimed:", d.get("bytes_reclaimed_human"))'
  echo
  echo "per-task csv: $CSV"
} | /usr/bin/tee "$SUMMARY"

echo
echo "[simulate] done — results in $RESULTS_DIR"
