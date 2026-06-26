#!/usr/bin/env bash
# Run the full bench matrix and emit a timestamped CSV.
#
# Usage: collect.sh [--sizes "S M"] [--approaches "A B C D"] [--ns "1 10 50 100"]
#
# Defaults: sizes=S M, approaches=A B C D, Ns=1 10 50 100.
# Size L is opt-in because it requires ~2 GB of repo cache plus per-approach disk.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

SIZES="S M"
APPROACHES="A B C D"
NS="1 10 50 100"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --sizes)      SIZES="$2";      shift 2 ;;
    --approaches) APPROACHES="$2"; shift 2 ;;
    --ns)         NS="$2";         shift 2 ;;
    *) echo "unknown arg: $1"; exit 2 ;;
  esac
done

TS="$(date +%Y%m%d-%H%M%S)"
OUT="$ROOT/results/raw/$TS"
mkdir -p "$OUT"
CSV="$OUT/bench-create.csv"
LOG="$OUT/run.log"

echo "approach,size,N,create_ms,disk_apparent_kb,disk_actual_kb,disk_real_kb,cleanup_ms,first_edit_ms" > "$CSV"

{
  echo "=== ivk benchmark spike ==="
  echo "timestamp: $TS"
  echo "host: $(/usr/bin/uname -a)"
  echo "git:  $(/usr/bin/git --version)"
  echo "sizes: $SIZES"
  echo "approaches: $APPROACHES"
  echo "ns: $NS"
  echo
} | /usr/bin/tee "$LOG"

for size in $SIZES; do
  /bin/bash "$SCRIPT_DIR/gen-repo.sh" "$size" 2>&1 | /usr/bin/tee -a "$LOG"
done

for size in $SIZES; do
  for approach in $APPROACHES; do
    for N in $NS; do
      msg="--- approach=$approach size=$size N=$N ---"
      echo "$msg" | /usr/bin/tee -a "$LOG"
      if ! /bin/bash "$SCRIPT_DIR/bench-create.sh" "$approach" "$size" "$N" "$CSV" 2>>"$LOG"; then
        echo "FAIL: approach=$approach size=$size N=$N (see $LOG)" | /usr/bin/tee -a "$LOG"
      fi
    done
  done
done

echo
echo "=== done ==="
echo "csv: $CSV"
echo "log: $LOG"
echo
echo "Quick view:"
/usr/bin/column -t -s, "$CSV" | head -60
