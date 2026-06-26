#!/usr/bin/env bash
# Materialize the Phase 6 demo task under .cache/demo-task/.
#
# Idempotent: removes any prior demo-task dir and rebuilds from scratch.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
DEST="$ROOT/.cache/demo-task"
IVK="$ROOT/target/release/ivk"

if [[ ! -x "$IVK" ]]; then
  echo "ERROR: ivk binary missing. Build first: cargo build --release --workspace" >&2
  exit 2
fi

/bin/rm -rf "$DEST"
/bin/mkdir -p "$DEST/src" "$DEST/test"

cat > "$DEST/package.json" <<'JSON'
{
  "name": "ivk-demo-task",
  "private": true,
  "version": "0.0.0",
  "type": "module",
  "scripts": {
    "test": "node --test test/*.test.js"
  }
}
JSON

cat > "$DEST/src/sum.ts" <<'TS'
// Buggy implementation. The agent's job: make `npm test` pass.
export function sumTo(n) {
  let total = 0;
  for (let i = 0; i < n; i++) {
    total += i;
  }
  return total;
}
TS

# Keep the test in plain JS so the demo doesn't need ts-loader.
cat > "$DEST/src/sum.js" <<'JS'
export function sumTo(n) {
  let total = 0;
  for (let i = 0; i < n; i++) {
    total += i;
  }
  return total;
}
JS

cat > "$DEST/test/sum.test.js" <<'JS'
import { test } from "node:test";
import assert from "node:assert";
import { sumTo } from "../src/sum.js";

test("sumTo(0) == 0", () => assert.strictEqual(sumTo(0), 0));
test("sumTo(1) == 1", () => assert.strictEqual(sumTo(1), 1));
test("sumTo(3) == 6", () => assert.strictEqual(sumTo(3), 6));
test("sumTo(10) == 55", () => assert.strictEqual(sumTo(10), 55));
JS

cd "$DEST"
/usr/bin/git init -q -b main
/usr/bin/git -c user.email=demo@ivykernel.dev -c user.name=demo add -A
/usr/bin/git -c user.email=demo@ivykernel.dev -c user.name=demo commit -q -m "initial buggy state"

"$IVK" init --agent-instructions >/dev/null

# Commit the agent skill files so they don't show up as untracked changes in
# every workspace the agent creates (which would pollute `ivk ch new`'s diff).
/usr/bin/git -c user.email=demo@ivykernel.dev -c user.name=demo add AGENTS.md skills/
/usr/bin/git -c user.email=demo@ivykernel.dev -c user.name=demo commit -q -m "ivk: agent skill files"

echo "[demo-task] set up at $DEST"
echo "[demo-task] cd $DEST   and start the demo. See examples/demo-task/README.md."
echo "[demo-task] note: the included tests use \`node --test\` which requires Node 18+."
