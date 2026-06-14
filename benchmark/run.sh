#!/usr/bin/env bash
set -euo pipefail

MAP=""
MOD_URL="http://127.0.0.1:8787"
MAP_SOURCE="test"
MODEL=""
HARNESS="claude"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUN_ID="$(date +%Y%m%d-%H%M%S)"
OUT_DIR="$ROOT/benchmark/runs/$RUN_ID"

if [ -f "$ROOT/.env" ]; then
  set -a
  # shellcheck disable=SC1091
  . "$ROOT/.env"
  set +a
fi

while [ $# -gt 0 ]; do
  case "$1" in
    --map) MAP="$2"; shift 2 ;;
    --map-source) MAP_SOURCE="$2"; shift 2 ;;
    --mod-url) MOD_URL="$2"; shift 2 ;;
    --out) OUT_DIR="$2"; shift 2 ;;
    --model) MODEL="$2"; shift 2 ;;
    --harness) HARNESS="$2"; shift 2 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

[ -n "$MAP" ] || { echo "usage: run.sh --map <id> [--harness claude|codex|gemini|opencode] [--model NAME] [--mod-url URL] [--map-source SRC] [--out DIR]" >&2; exit 2; }
case "$MAP" in
  *[!A-Za-z0-9_-]*) echo "map id must be alphanumeric, dash, or underscore" >&2; exit 2 ;;
esac

# Resolve a map id to its in-game save name via benchmark/maps/maps.tsv.
# Tab-separated: id<TAB>save_name<TAB>source<TAB>game_version; '#'/blank skipped.
resolve_save_name() {
  local want="$1" maps="$ROOT/benchmark/maps/maps.tsv" id save_name rest
  [ -f "$maps" ] || { echo "missing map binding file: $maps" >&2; return 1; }
  while IFS="$(printf '\t')" read -r id save_name rest; do
    case "$id" in ''|'#'*) continue ;; esac
    if [ "$id" = "$want" ]; then printf '%s\n' "$save_name"; return 0; fi
  done < "$maps"
  echo "unknown map id '$want'. Known ids:" >&2
  while IFS="$(printf '\t')" read -r id _; do
    case "$id" in ''|'#'*) continue ;; esac
    echo "  $id" >&2
  done < "$maps"
  return 1
}

# Minimal JSON string encoder (escape backslash and double-quote).
json_str() {
  local s="$1"
  s="${s//\\/\\\\}"
  s="${s//\"/\\\"}"
  printf '"%s"' "$s"
}

# Issue the load and wait for the level-reload bridge cycle to finish.
# Returns non-zero on timeout (surfaces the "invalid file"/unstable-load case).
load_and_wait() {
  local save_name="$1" deadline resp h
  resp="$(curl -fsS -X POST "$MOD_URL/load-save" \
    -H 'content-type: application/json' \
    -d "$(printf '{"save_name":%s}' "$(json_str "$save_name")")" 2>/dev/null || true)"
  case "$(printf '%s' "$resp" | tr -d '[:space:]')" in
    *'"ok":false'*)
      echo "load rejected for save '$save_name'. Mod reported available saves:" >&2
      printf '%s\n' "$resp" >&2
      return 1 ;;
    "") echo "mod not reachable at $MOD_URL/load-save" >&2; return 1 ;;
  esac
  # Phase 1: bridge goes down (reload started). Tolerate up to 30s.
  deadline=$(( $(date +%s) + 30 ))
  until [ "$(date +%s)" -ge "$deadline" ]; do
    curl -fsS "$MOD_URL/health" >/dev/null 2>&1 || break
    sleep 1
  done
  # If the bridge never dropped, the reload may not have started — leave a
  # breadcrumb, since phase 2 could then pass against the stale city.
  if curl -fsS "$MOD_URL/health" >/dev/null 2>&1; then
    echo "note: bridge did not go down during the reload window; proceeding to phase-2 check" >&2
  fi
  # Phase 2: bridge back up with a city loaded (reload finished). Up to 180s.
  deadline=$(( $(date +%s) + 180 ))
  until [ "$(date +%s)" -ge "$deadline" ]; do
    h="$(curl -fsS "$MOD_URL/health" 2>/dev/null || true)"
    case "$(printf '%s' "$h" | tr -d '[:space:]')" in
      *'"city_loaded":true'*) return 0 ;;
    esac
    sleep 2
  done
  echo "timed out waiting for save '$save_name' to finish loading" >&2
  return 1
}

SAVE_NAME="$(resolve_save_name "$MAP")" || exit 1

# Preflight + load (skipped under DRY_RUN, which only inspects the resolved
# command). Reachability is implied by load_and_wait's first curl.
if [ "${DRY_RUN:-0}" != "1" ]; then
  echo "loading map '$MAP' (save '$SAVE_NAME')…" >&2
  load_and_wait "$SAVE_NAME" || exit 1
fi

mkdir -p "$OUT_DIR"
printf '%s\n' "$HARNESS" > "$OUT_DIR/harness.txt"
if [ -n "$MODEL" ]; then printf '%s\n' "$MODEL" > "$OUT_DIR/model.txt"; fi

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

# Per-run session dir OUTSIDE the repo: the agent runs under a deny-repo-read
# sandbox (Seatbelt/bubblewrap/firejail; see sandbox-prepare) added for
# anti-cheating — run 20260609-191326 read the scoring source via Bash.
# Everything the harness must read or exec therefore lives here: the broker
# binary copy, the harness config, and the scratch workspace. The agent may
# freely write/run code in its workspace; only repo reads die.
# Lives under ~/Library/Caches (not TMPDIR): macOS periodically reaps
# /var/folders temp dirs, which deleted a live workspace mid-run on 2026-06-09.
SESSION_BASE="$HOME/Library/Caches/skylinebench"
mkdir -p "$SESSION_BASE"
SESSION_DIR="$(mktemp -d "$SESSION_BASE/$RUN_ID.XXXXXX")"
WORKSPACE="$SESSION_DIR/workspace"
mkdir -p "$WORKSPACE"

# Always build a fresh release binary so the MCP server can never be a stale
# build that lacks the `benchmark` subcommand (skipped under DRY_RUN). The
# binary is copied into SESSION_DIR because the repo copy is unreadable
# inside the agent sandbox. REPO_BIN (unsandboxed) runs the harness-prepare /
# sandbox-prepare / format-stream / render-transcript helpers; BROKER_BIN (the
# copy) is what the harness spawns as the MCP server.
REPO_BIN="$ROOT/broker/target/release/skylinebench"
BROKER_BIN="$SESSION_DIR/skylinebench"
if [ "${DRY_RUN:-0}" != "1" ]; then
  echo "building broker (release)…" >&2
  cargo build --release --manifest-path "$ROOT/broker/Cargo.toml" >&2 || { echo "broker build failed" >&2; exit 1; }
  cp "$REPO_BIN" "$BROKER_BIN"
fi

# Claude Code MCP startup/tool timeouts: the post-run settle/final windows drive
# the sim for tens of seconds, so give generous headroom (defaults ~30s/60s).
# Claude Code-specific env (harmless to other harnesses, which ignore it); the
# baseline is measured lazily on the first tool call to avoid blocking the MCP
# `initialize` handshake regardless of harness.
export MCP_TIMEOUT="${MCP_TIMEOUT:-600000}"
export MCP_TOOL_TIMEOUT="${MCP_TOOL_TIMEOUT:-600000}"

# Isolated Claude config: a dedicated CLAUDE_CONFIG_DIR so the agent inherits
# none of the operator's plugins, hooks, skills, or global CLAUDE.md (run
# 2026-06-11 loaded the operator's superpowers plugin via a SessionStart hook,
# polluting the agent's context). The dir is persistent — credentials don't
# transfer from the operator's config (macOS keeps them keyed to it), so the
# operator logs into this dir ONCE and every run reuses it.
if [ "$HARNESS" = "claude" ]; then
  CLAUDE_CONFIG_DIR="${BENCH_CLAUDE_CONFIG:-$HOME/Library/Application Support/skylinebench/claude-config}"
  mkdir -p "$CLAUDE_CONFIG_DIR"
  [ -f "$CLAUDE_CONFIG_DIR/.claude.json" ] || printf '{"hasCompletedOnboarding": true}\n' > "$CLAUDE_CONFIG_DIR/.claude.json"
  if [ "${DRY_RUN:-0}" != "1" ] && [ -z "${ANTHROPIC_API_KEY:-}" ]; then
    if ! grep -q oauthAccount "$CLAUDE_CONFIG_DIR/.claude.json" 2>/dev/null; then
      echo "benchmark Claude config is not logged in. One-time setup:" >&2
      echo "  CLAUDE_CONFIG_DIR=\"$CLAUDE_CONFIG_DIR\" claude  # then /login, then /exit" >&2
      exit 1
    fi
  fi
  export CLAUDE_CONFIG_DIR
fi

# The MCP broker command the harness will spawn over stdio.
MCP_SHELL="$BROKER_BIN benchmark --map $MAP --map-source $MAP_SOURCE --mod-url $MOD_URL --out $OUT_DIR --renders-dir $SESSION_DIR/renders --screenshots-dir $SESSION_DIR/screenshots"

# Resolve the harness launch plan (config files + argv + env + required env).
"$REPO_BIN" harness-prepare \
  --harness "$HARNESS" \
  ${MODEL:+--model "$MODEL"} \
  --prompt-file "$ROOT/benchmark/prompt.md" \
  --mcp-shell "$MCP_SHELL" \
  --session-dir "$SESSION_DIR"

# NUL-delimited read loop (portable to bash 3.2 on stock macOS; `mapfile -d`
# needs bash 4.4+ which macOS does not ship).
ARGV=()
while IFS= read -r -d '' a; do ARGV+=("$a"); done < "$SESSION_DIR/launch.argv"
while IFS= read -r -d '' kv; do export "$kv"; done < "$SESSION_DIR/launch.env"

# Preflight (skipped under DRY_RUN, which only inspects the resolved command):
# harness binary on PATH + required secrets present.
if [ "${DRY_RUN:-0}" != "1" ]; then
  command -v "${ARGV[0]}" >/dev/null || { echo "harness '$HARNESS' binary '${ARGV[0]}' not found on PATH" >&2; exit 1; }
  if [ -s "$SESSION_DIR/launch.required-env" ]; then
    while IFS= read -r var; do
      [ -z "$var" ] && continue
      if [ -z "${!var:-}" ]; then echo "harness '$HARNESS' requires \$$var to be set" >&2; exit 1; fi
    done < "$SESSION_DIR/launch.required-env"
  fi
fi

# Copy harness config(s) into the run dir for reproducibility.
[ -f "$SESSION_DIR/mcp.json" ] && cp "$SESSION_DIR/mcp.json" "$OUT_DIR/"
[ -f "$SESSION_DIR/opencode.json" ] && cp "$SESSION_DIR/opencode.json" "$OUT_DIR/"
[ -f "$SESSION_DIR/codex/config.toml" ] && cp "$SESSION_DIR/codex/config.toml" "$OUT_DIR/codex-config.toml"
[ -f "$SESSION_DIR/gemini/.gemini/settings.json" ] && cp "$SESSION_DIR/gemini/.gemini/settings.json" "$OUT_DIR/gemini-settings.json"

# Select the cross-OS sandbox wrapper (deny-repo-read anti-cheat).
SANDBOX_BACKEND="$("$REPO_BIN" sandbox-prepare --root "$ROOT" --session-dir "$SESSION_DIR")"
SANDBOX_ARGV=()
while IFS= read -r -d '' a; do SANDBOX_ARGV+=("$a"); done < "$SESSION_DIR/sandbox.argv"
printf '%s\n' "$SANDBOX_BACKEND" > "$OUT_DIR/sandbox.txt"

CMD=(${SANDBOX_ARGV[@]:+"${SANDBOX_ARGV[@]}"} "${ARGV[@]}")

if [ "${DRY_RUN:-0}" = "1" ]; then
  printf '%q ' "${CMD[@]}"; echo
  echo "--- harness: $HARNESS / sandbox: $SANDBOX_BACKEND ---" >&2
  echo "--- launch.env ---" >&2; tr '\0' '\n' < "$SESSION_DIR/launch.env" >&2
  for f in "$SESSION_DIR"/mcp.json "$SESSION_DIR"/codex/config.toml "$SESSION_DIR"/gemini/.gemini/settings.json "$SESSION_DIR"/opencode.json; do
    [ -f "$f" ] && { echo "--- $f ---" >&2; cat "$f" >&2; }
  done
  exit 0
fi

# `|| true`: when the broker hits the wall-clock cap it closes the MCP
# connection, so the harness exits non-zero — expected, not a failure.
# Harness stderr (API error detail, model diagnostics) is teed to a file *and*
# the terminal: stream-json on stdout carries only a terse `result` event for
# API failures (e.g. gemini's "Operation cancelled"), so the real reason lives
# on stderr and was previously lost.
(cd "$WORKSPACE" && "${CMD[@]}" 2> >(tee "$OUT_DIR/harness.stderr" >&2)) | tee "$OUT_DIR/transcript.jsonl" | "$REPO_BIN" format-stream --harness "$HARNESS" | tee "$OUT_DIR/run.log" || true

if [ -d "$SESSION_DIR/renders" ]; then
  mv "$SESSION_DIR/renders" "$OUT_DIR/renders"
fi

if [ -d "$SESSION_DIR/screenshots" ]; then
  mv "$SESSION_DIR/screenshots" "$OUT_DIR/screenshots"
fi

"$REPO_BIN" render-transcript --input "$OUT_DIR/transcript.jsonl" --out "$OUT_DIR/transcript.md" --harness "$HARNESS" || true

if [ ! -f "$OUT_DIR/end-state.json" ]; then
  echo "benchmark session ended before writing end-state.json; skipping final measurement" >&2
  if [ -s "$OUT_DIR/run.log" ]; then
    echo "--- last run.log lines ---" >&2
    tail -n 40 "$OUT_DIR/run.log" >&2
  else
    echo "--- last transcript.jsonl lines ---" >&2
    tail -n 20 "$OUT_DIR/transcript.jsonl" >&2
  fi
  echo "artifacts in $OUT_DIR" >&2
  exit 1
fi

# The slow settle + final measurement runs here, outside the agent session, so
# no MCP client timeout can kill it (the old in-server finalize made
# submit_solution hang for 600s and die). Uses the repo binary — run.sh is
# not sandboxed.
echo "finalizing run (settle + final measurement, several minutes)…" >&2
"$REPO_BIN" benchmark-finalize --out "$OUT_DIR" --mod-url "$MOD_URL"

echo "artifacts in $OUT_DIR"
