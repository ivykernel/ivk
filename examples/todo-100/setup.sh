#!/usr/bin/env bash
# Materialize the Phase 7 100-task demo fixture under .cache/todo-100/.
#
# 100 tiny independent tasks. Each task lives in src/task_NNN.js with
# a paired failing test in test/task_NNN.test.js. The bug pattern is
# always the same: `return n` instead of `return n + 1`. Agents (or this
# script's simulation mode) fix tasks one by one in their own workspace.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
DEST="$ROOT/.cache/todo-100"
IVK="$ROOT/target/release/ivk"

if [[ ! -x "$IVK" ]]; then
  echo "ERROR: ivk binary missing. Build first: cargo build --release --workspace" >&2
  exit 2
fi

/bin/rm -rf "$DEST"
/bin/mkdir -p "$DEST/src" "$DEST/test"

cat > "$DEST/package.json" <<'JSON'
{
  "name": "ivk-todo-100",
  "private": true,
  "version": "0.0.0",
  "type": "module",
  "scripts": {
    "test": "node --test test/*.test.js",
    "test:one": "node --test"
  }
}
JSON

cat > "$DEST/README.md" <<'MD'
# todo-100

100 tiny independent tasks, one buggy function per task.

The bug pattern is uniform: each `task_NNN.js` exports `increment(n)` which
should return `n + 1` but currently returns `n`. Each `task_NNN.test.js`
asserts that.

Use this as the canonical many-task workload to demonstrate `ivk` at scale:
spin up 100 parallel workspaces, fix some tasks, discard the rest.

Run the simulator (no real agent needed):

```bash
bash examples/todo-100/simulate.sh   # fixes a random 30 tasks, exports each
```
MD

for i in $(/usr/bin/seq -f "%03g" 1 100); do
  cat > "$DEST/src/task_${i}.js" <<EOF
// task_${i}: should return n + 1. Currently returns n. Fix it.
export function increment_${i}(n) {
  return n;
}
EOF

  cat > "$DEST/test/task_${i}.test.js" <<EOF
import { test } from "node:test";
import assert from "node:assert";
import { increment_${i} } from "../src/task_${i}.js";

test("task_${i}: increment(0) == 1", () =>
  assert.strictEqual(increment_${i}(0), 1));
test("task_${i}: increment(41) == 42", () =>
  assert.strictEqual(increment_${i}(41), 42));
EOF
done

cd "$DEST"
/usr/bin/git init -q -b main
/usr/bin/git -c user.email=demo@ivykernel.dev -c user.name=demo add -A
/usr/bin/git -c user.email=demo@ivykernel.dev -c user.name=demo commit -q -m "initial: 100 buggy tasks"

"$IVK" init >/dev/null

echo "[todo-100] set up at $DEST"
echo "[todo-100] 100 tasks, all currently failing their tests."
echo "[todo-100] Next: bash examples/todo-100/simulate.sh   (no real agents needed)"
