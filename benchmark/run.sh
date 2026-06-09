#!/usr/bin/env bash
set -euo pipefail

MAP=""
MOD_URL="http://127.0.0.1:8787"
WATCH=0
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUN_ID="$(date +%Y%m%d-%H%M%S)"
OUT_DIR="$ROOT/benchmark/runs/$RUN_ID"

while [ $# -gt 0 ]; do
  case "$1" in
    --map) MAP="$2"; shift 2 ;;
    --mod-url) MOD_URL="$2"; shift 2 ;;
    --out) OUT_DIR="$2"; shift 2 ;;
    --watch|--interactive) WATCH=1; shift ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

[ -n "$MAP" ] || { echo "usage: run.sh --map <id> [--watch] [--mod-url URL] [--out DIR]" >&2; exit 2; }

mkdir -p "$OUT_DIR"
BROKER_BIN="$ROOT/broker/target/release/skylinebench"
[ -x "$BROKER_BIN" ] || BROKER_BIN="cargo run --manifest-path $ROOT/broker/Cargo.toml --release --"

# Generate the MCP config: Claude Code spawns `broker benchmark` over stdio.
MCP_CONFIG="$OUT_DIR/mcp.json"
cat > "$MCP_CONFIG" <<JSON
{
  "mcpServers": {
    "skylinebench": {
      "command": "sh",
      "args": ["-c", "$BROKER_BIN benchmark --map $MAP --mod-url $MOD_URL --out $OUT_DIR"]
    }
  }
}
JSON

PROMPT="$(cat "$ROOT/benchmark/prompt.md")"
ALLOWED="mcp__skylinebench__build_road,mcp__skylinebench__bulldoze,mcp__skylinebench__upgrade_road,mcp__skylinebench__set_zoning,mcp__skylinebench__control_time,mcp__skylinebench__get_city_overview,mcp__skylinebench__observe_area,mcp__skylinebench__get_metrics,mcp__skylinebench__list_road_types,mcp__skylinebench__list_zone_types,mcp__skylinebench__render_map,mcp__skylinebench__submit_solution"

if [ "$WATCH" -eq 1 ]; then
  CMD=(claude --mcp-config "$MCP_CONFIG" --allowedTools "$ALLOWED" "$PROMPT")
else
  CMD=(claude -p "$PROMPT" --mcp-config "$MCP_CONFIG" --allowedTools "$ALLOWED" --output-format stream-json --verbose)
fi

if [ "${DRY_RUN:-0}" = "1" ]; then
  printf '%q ' "${CMD[@]}"; echo
  exit 0
fi

if [ "$WATCH" -eq 1 ]; then
  "${CMD[@]}"
else
  "${CMD[@]}" | tee "$OUT_DIR/transcript.jsonl"
  "$BROKER_BIN" render-transcript --input "$OUT_DIR/transcript.jsonl" --out "$OUT_DIR/transcript.md" || \
    cargo run --manifest-path "$ROOT/broker/Cargo.toml" --release -- \
      render-transcript --input "$OUT_DIR/transcript.jsonl" --out "$OUT_DIR/transcript.md"
fi

echo "artifacts in $OUT_DIR"
