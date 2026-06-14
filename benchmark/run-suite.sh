#!/usr/bin/env bash
set -euo pipefail

MAP=""
SUITE=""
MOD_URL="http://127.0.0.1:8787"
MAP_SOURCE="test"
FAIL_FAST=0
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SUITE_ID="suite-$(date +%Y%m%d-%H%M%S)"
OUT_DIR="$ROOT/benchmark/runs/$SUITE_ID"

while [ $# -gt 0 ]; do
  case "$1" in
    --map) MAP="$2"; shift 2 ;;
    --suite) SUITE="$2"; shift 2 ;;
    --mod-url) MOD_URL="$2"; shift 2 ;;
    --map-source) MAP_SOURCE="$2"; shift 2 ;;
    --out) OUT_DIR="$2"; shift 2 ;;
    --fail-fast) FAIL_FAST=1; shift ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

[ -n "$MAP" ] && [ -n "$SUITE" ] || {
  echo "usage: run-suite.sh --map <id> --suite <file> [--mod-url URL] [--map-source SRC] [--out DIR] [--fail-fast]" >&2
  exit 2
}
[ -f "$SUITE" ] || { echo "suite file not found: $SUITE" >&2; exit 2; }

mkdir -p "$OUT_DIR"
cp "$SUITE" "$OUT_DIR/suite.txt"
SUMMARY="$OUT_DIR/summary.tsv"
printf 'harness\tmodel\trunid\tstatus\texit_code\n' > "$SUMMARY"

# Parse manifest into harness/model pairs (skip '#'/blank).
ENTRIES=()
while IFS= read -r line || [ -n "$line" ]; do
  case "$line" in ''|'#'*) continue ;; esac
  ENTRIES+=("$line")
done < "$SUITE"

[ "${#ENTRIES[@]}" -gt 0 ] || { echo "suite '$SUITE' has no runnable entries" >&2; exit 2; }

# Pre-suite validation: every distinct harness's binary + secrets resolve.
# DRY_RUN=1 run.sh exits 0 only if harness-prepare succeeds; we additionally
# probe the harness binary + required env the same way run.sh does at launch.
echo "validating ${#ENTRIES[@]} suite entries…" >&2
for entry in "${ENTRIES[@]}"; do
  harness="${entry%%:*}"
  model=""
  case "$entry" in *:*) model="${entry#*:}" ;; esac
  if ! DRY_RUN=1 "$ROOT/benchmark/run.sh" --map "$MAP" --map-source "$MAP_SOURCE" \
      --mod-url "$MOD_URL" --harness "$harness" ${model:+--model "$model"} >/dev/null; then
    echo "suite validation failed for entry '$entry'" >&2
    exit 1
  fi
done

run_one() {
  local entry="$1" harness model runid child status=ok code=0
  harness="${entry%%:*}"
  model=""
  case "$entry" in *:*) model="${entry#*:}" ;; esac
  runid="$(date +%Y%m%d-%H%M%S)-$harness${model:+-$model}"
  child="$OUT_DIR/$runid"
  echo "=== running $entry → $child ===" >&2
  if "$ROOT/benchmark/run.sh" --map "$MAP" --map-source "$MAP_SOURCE" \
      --mod-url "$MOD_URL" --harness "$harness" ${model:+--model "$model"} \
      --out "$child"; then
    status=ok
  else
    code=$?
    status=failed
  fi
  printf '%s\t%s\t%s\t%s\t%s\n' "$harness" "$model" "$runid" "$status" "$code" >> "$SUMMARY"
  [ "$status" = ok ]
}

FAILED=0
for entry in "${ENTRIES[@]}"; do
  if ! run_one "$entry"; then
    FAILED=$((FAILED + 1))
    if [ "$FAIL_FAST" = 1 ]; then
      echo "fail-fast: stopping suite after '$entry'" >&2
      break
    fi
  fi
done

echo "suite complete: $OUT_DIR (failed: $FAILED)" >&2
column -t -s "$(printf '\t')" "$SUMMARY" >&2 || cat "$SUMMARY" >&2
[ "$FAILED" -eq 0 ]
