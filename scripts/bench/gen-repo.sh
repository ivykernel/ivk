#!/usr/bin/env bash
# Generate (or reuse cached) a synthetic git repo of the requested size.
#
# Usage: gen-repo.sh <S|M|L>
#
# Writes to: <ivk>/.cache/repo-<size>/
# Idempotent: if the cache already exists with a committed HEAD, does nothing.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=lib.sh
source "$SCRIPT_DIR/lib.sh"

SIZE="${1:?usage: gen-repo.sh <S|M|MD|L>}"
case "$SIZE" in S|M|MD|L) ;; *) echo "size must be S|M|MD|L"; exit 2 ;; esac

CACHE="$ROOT/.cache/repo-$SIZE"

if [[ -f "$CACHE/.git/HEAD" ]]; then
  echo "[gen-repo] cache hit: $CACHE"
  exit 0
fi

echo "[gen-repo] generating $SIZE into $CACHE ..."
$RM -rf "$CACHE"
mkdir -p "$CACHE"

/usr/bin/python3 "$SCRIPT_DIR/gen_repo.py" "$SIZE" "$CACHE"

cd "$CACHE"
/usr/bin/git init -q -b main
/usr/bin/git -c user.email=bench@ivykernel.dev -c user.name=bench add -A
/usr/bin/git -c user.email=bench@ivykernel.dev -c user.name=bench commit -q -m "initial $SIZE"

echo "[gen-repo] done. HEAD=$(/usr/bin/git rev-parse --short HEAD)"
