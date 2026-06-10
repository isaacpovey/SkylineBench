#!/usr/bin/env bash
set -euo pipefail

MAP=""
MOD_URL="http://127.0.0.1:8787"
MAP_SOURCE="test"
WATCH=0
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUN_ID="$(date +%Y%m%d-%H%M%S)"
OUT_DIR="$ROOT/benchmark/runs/$RUN_ID"

while [ $# -gt 0 ]; do
  case "$1" in
    --map) MAP="$2"; shift 2 ;;
    --map-source) MAP_SOURCE="$2"; shift 2 ;;
    --mod-url) MOD_URL="$2"; shift 2 ;;
    --out) OUT_DIR="$2"; shift 2 ;;
    --watch|--interactive) WATCH=1; shift ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

[ -n "$MAP" ] || { echo "usage: run.sh --map <id> [--watch] [--mod-url URL] [--map-source SRC] [--out DIR]" >&2; exit 2; }
case "$MAP" in
  *[!A-Za-z0-9_-]*) echo "map id must be alphanumeric, dash, or underscore" >&2; exit 2 ;;
esac

mkdir -p "$OUT_DIR"

# Always build a fresh release binary so the MCP server can never be a stale
# build that lacks the `benchmark` subcommand (skipped under DRY_RUN).
BROKER_BIN="$ROOT/broker/target/release/skylinebench"
if [ "${DRY_RUN:-0}" != "1" ]; then
  echo "building broker (release)…" >&2
  cargo build --release --manifest-path "$ROOT/broker/Cargo.toml" >&2 || { echo "broker build failed" >&2; exit 1; }
fi

# The pre-serve baseline (and the post-run settle/final windows) drive the sim
# for tens of seconds; give Claude Code's MCP startup + tool timeouts generous
# headroom so the server isn't killed mid-measurement (defaults are ~30s/60s).
export MCP_TIMEOUT="${MCP_TIMEOUT:-600000}"
export MCP_TOOL_TIMEOUT="${MCP_TOOL_TIMEOUT:-600000}"

# Generate the MCP config: Claude Code spawns `broker benchmark` over stdio.
MCP_CONFIG="$OUT_DIR/mcp.json"
cat > "$MCP_CONFIG" <<JSON
{
  "mcpServers": {
    "skylinebench": {
      "command": "sh",
      "args": ["-c", "$BROKER_BIN benchmark --map $MAP --map-source $MAP_SOURCE --mod-url $MOD_URL --out $OUT_DIR"]
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
  # Capture the raw stream-json to transcript.jsonl unchanged, render a
  # human-readable line per event to the console, and also save that to run.log.
  # `|| true`: when a run ends the broker exits and closes the MCP connection,
  # so `claude` exits non-zero — that's expected, not a failure of the run.
  "${CMD[@]}" | tee "$OUT_DIR/transcript.jsonl" | "$BROKER_BIN" format-stream | tee "$OUT_DIR/run.log" || true
  "$BROKER_BIN" render-transcript --input "$OUT_DIR/transcript.jsonl" --out "$OUT_DIR/transcript.md"
fi

echo "artifacts in $OUT_DIR"
