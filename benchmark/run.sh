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

# Only one run may drive the single game instance at a time. A second run.sh
# started mid-run (this happened on 2026-06-09: 21:01 + 21:04 against one game)
# corrupts both runs' measurements.
LOCK_DIR="${TMPDIR:-/tmp}/skylinebench.lock"
if ! mkdir "$LOCK_DIR" 2>/dev/null; then
  echo "another benchmark run appears active (lock: $LOCK_DIR). Remove the dir if it is stale." >&2
  exit 1
fi
SESSION_DIR=""
trap 'rm -rf "${SESSION_DIR:-}"; rmdir "$LOCK_DIR" 2>/dev/null' EXIT

# Per-run session dir OUTSIDE the repo: the agent runs under a Seatbelt profile
# that denies reading the repo (anti-cheating — run 20260609-191326 read the
# scoring source via Bash). Everything claude must read or exec therefore
# lives here: the broker binary copy, mcp.json, and the scratch workspace.
# The agent may freely write/run code in its workspace; only repo reads die.
# Lives under ~/Library/Caches (not TMPDIR): macOS periodically reaps
# /var/folders temp dirs, which deleted a live workspace mid-run on 2026-06-09.
SESSION_BASE="$HOME/Library/Caches/skylinebench"
mkdir -p "$SESSION_BASE"
SESSION_DIR="$(mktemp -d "$SESSION_BASE/$RUN_ID.XXXXXX")"
WORKSPACE="$SESSION_DIR/workspace"
mkdir -p "$WORKSPACE"

SANDBOX_PROFILE="$SESSION_DIR/deny-repo.sb"
cat > "$SANDBOX_PROFILE" <<SB
(version 1)
(allow default)
(deny file-read* (subpath "$ROOT"))
SB
command -v sandbox-exec >/dev/null || { echo "sandbox-exec not found (macOS only)" >&2; exit 1; }

# Always build a fresh release binary so the MCP server can never be a stale
# build that lacks the `benchmark` subcommand (skipped under DRY_RUN). The
# binary is copied into SESSION_DIR because the repo copy is unreadable
# inside the agent sandbox.
REPO_BIN="$ROOT/broker/target/release/skylinebench"
BROKER_BIN="$SESSION_DIR/skylinebench"
if [ "${DRY_RUN:-0}" != "1" ]; then
  echo "building broker (release)…" >&2
  cargo build --release --manifest-path "$ROOT/broker/Cargo.toml" >&2 || { echo "broker build failed" >&2; exit 1; }
  cp "$REPO_BIN" "$BROKER_BIN"
fi

# The pre-serve baseline (and the post-run settle/final windows) drive the sim
# for tens of seconds; give Claude Code's MCP startup + tool timeouts generous
# headroom so the server isn't killed mid-measurement (defaults are ~30s/60s).
export MCP_TIMEOUT="${MCP_TIMEOUT:-600000}"
export MCP_TOOL_TIMEOUT="${MCP_TOOL_TIMEOUT:-600000}"

# Generate the MCP config: Claude Code spawns `broker benchmark` over stdio.
MCP_CONFIG="$SESSION_DIR/mcp.json"
cat > "$MCP_CONFIG" <<JSON
{
  "mcpServers": {
    "skylinebench": {
      "command": "sh",
      "args": ["-c", "$BROKER_BIN benchmark --map $MAP --map-source $MAP_SOURCE --mod-url $MOD_URL --out $OUT_DIR --renders-dir $SESSION_DIR/renders"]
    }
  }
}
JSON
cp "$MCP_CONFIG" "$OUT_DIR/mcp.json"

PROMPT="$(cat "$ROOT/benchmark/prompt.md")"
ALLOWED="mcp__skylinebench__build_road,mcp__skylinebench__bulldoze,mcp__skylinebench__upgrade_road,mcp__skylinebench__set_zoning,mcp__skylinebench__control_time,mcp__skylinebench__get_city_overview,mcp__skylinebench__observe_area,mcp__skylinebench__get_metrics,mcp__skylinebench__list_road_types,mcp__skylinebench__list_zone_types,mcp__skylinebench__render_map,mcp__skylinebench__submit_solution,mcp__skylinebench__query_segments"
DISALLOWED="WebFetch,WebSearch"

SANDBOX=(sandbox-exec -f "$SANDBOX_PROFILE")
# caffeinate -dims: block display/idle/disk/system sleep for the lifetime of
# the agent session. Machine sleep killed run 20260609-210135 ~2.8h in.
KEEPAWAKE=()
if command -v caffeinate >/dev/null; then KEEPAWAKE=(caffeinate -dims); fi
if [ "$WATCH" -eq 1 ]; then
  CMD=(${KEEPAWAKE[@]:+"${KEEPAWAKE[@]}"} "${SANDBOX[@]}" claude --mcp-config "$MCP_CONFIG" --strict-mcp-config --allowedTools "$ALLOWED" --disallowedTools "$DISALLOWED" --permission-mode bypassPermissions "$PROMPT")
else
  CMD=(${KEEPAWAKE[@]:+"${KEEPAWAKE[@]}"} "${SANDBOX[@]}" claude -p "$PROMPT" --mcp-config "$MCP_CONFIG" --strict-mcp-config --allowedTools "$ALLOWED" --disallowedTools "$DISALLOWED" --permission-mode bypassPermissions --output-format stream-json --verbose)
fi

if [ "${DRY_RUN:-0}" = "1" ]; then
  printf '%q ' "${CMD[@]}"; echo
  echo "--- mcp.json ---" >&2
  cat "$MCP_CONFIG" >&2
  exit 0
fi

if [ "$WATCH" -eq 1 ]; then
  # `|| true`: the broker closing the MCP connection at the wall-clock cap causes
  # claude to exit non-zero — that's expected, not a failure (same rationale as
  # the headless branch below).
  (cd "$WORKSPACE" && "${CMD[@]}") || true
else
  # Capture the raw stream-json to transcript.jsonl unchanged, render a
  # human-readable line per event to the console, and also save that to run.log.
  # `|| true`: if the broker hits the wall-clock cap it exits and closes the MCP
  # connection, so `claude` exits non-zero — that's expected, not a failure.
  (cd "$WORKSPACE" && "${CMD[@]}") | tee "$OUT_DIR/transcript.jsonl" | "$REPO_BIN" format-stream | tee "$OUT_DIR/run.log" || true
  "$REPO_BIN" render-transcript --input "$OUT_DIR/transcript.jsonl" --out "$OUT_DIR/transcript.md"
fi

if [ -d "$SESSION_DIR/renders" ]; then
  mv "$SESSION_DIR/renders" "$OUT_DIR/renders"
fi

# The slow settle + final measurement runs here, outside the agent session, so
# no MCP client timeout can kill it (the old in-server finalize made
# submit_solution hang for 600s and die). Uses the repo binary — run.sh is
# not sandboxed.
echo "finalizing run (settle + final measurement, several minutes)…" >&2
"$REPO_BIN" benchmark-finalize --out "$OUT_DIR" --mod-url "$MOD_URL"

echo "artifacts in $OUT_DIR"
