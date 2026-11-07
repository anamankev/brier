#!/bin/sh
# The G4 fuzz campaign: every cargo-fuzz target for SECONDS (default 3600) each, JOBS at a time
# (default 3). Logs go to target/fuzz-campaign/<target>.log; the summary lists each target's
# executions and whether it found a crash. Exits non-zero if any target failed.
set -eu

SECONDS_EACH="${SECONDS_EACH:-3600}"
JOBS="${JOBS:-3}"
TOOLCHAIN="nightly-2026-10-20"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/target/fuzz-campaign"
mkdir -p "$OUT"
cd "$ROOT/fuzz"

TARGETS="$(cargo "+$TOOLCHAIN" fuzz list)"
for t in $TARGETS; do
  cargo "+$TOOLCHAIN" fuzz build "$t" >/dev/null
done

run() {
  t="$1"
  if cargo "+$TOOLCHAIN" fuzz run "$t" -- -max_total_time="$SECONDS_EACH" >"$OUT/$t.log" 2>&1; then
    echo "pass" >"$OUT/$t.status"
  else
    echo "FAIL" >"$OUT/$t.status"
  fi
}

running=0
for t in $TARGETS; do
  run "$t" &
  running=$((running + 1))
  if [ "$running" -ge "$JOBS" ]; then
    wait
    running=0
  fi
done
wait

failed=0
for t in $TARGETS; do
  status="$(cat "$OUT/$t.status")"
  runs="$(grep -o 'Done [0-9]* runs' "$OUT/$t.log" | tail -1 || true)"
  echo "$t: $status ($runs in ${SECONDS_EACH} s)"
  [ "$status" = "pass" ] || failed=1
done
exit "$failed"
