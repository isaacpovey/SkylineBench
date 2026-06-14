# Multi-harness support (non-Claude models) — design

**Date:** 2026-06-14
**Status:** Approved pending spec review

## Goal

Let the benchmark run agents on harnesses other than Claude Code, so we can
score non-Anthropic models on the same city task. Three new harnesses:

1. **codex** — OpenAI's `codex` CLI, for OpenAI models.
2. **gemini** — Google's `gemini` CLI, for Gemini models.
3. **opencode** — the `opencode` CLI with **OpenRouter** as the backend, for
   open-source / open-weight models.

The benchmark task, scoring, maps, and the MCP broker tools are unchanged. All
four harnesses speak stdio MCP, so the broker's tools are reused verbatim. What
changes is the layer around the agent: how it is launched, configured,
authenticated, and how its output is turned into a transcript.

## Decisions (settled in brainstorming)

- **OSS harness:** `opencode`, backed by OpenRouter.
- **Transcript fidelity:** full parity — every harness renders into the same
  `transcript.md` structure and the same live console lines as Claude,
  populated with whatever each harness exposes (see "Parity limits").
- **Run UX:** an explicit `--harness <name>` flag on `run.sh`, defaulting to
  `claude` (today's behavior is preserved when the flag is omitted).
- **Adapter home:** the Rust broker. Per-harness logic is typed, pure, and unit
  tested; `run.sh` stays a thin orchestrator.

## Current coupling to Claude (what we are abstracting)

Three places hardcode Claude today:

1. **`benchmark/run.sh`** — the `claude` invocation: flags
   (`-p`, `--model`, `--mcp-config`, `--strict-mcp-config`, `--allowedTools`,
   `--disallowedTools`, `--permission-mode bypassPermissions`,
   `--output-format stream-json`), the `mcp.json` shape, `CLAUDE_CONFIG_DIR`
   OAuth handling, and the `format-stream` / `render-transcript` piping.
2. **`broker/src/benchmark/transcript.rs`** — `render_transcript` and
   `format_event_live` parse Claude `stream-json` (objects with
   `type`/`message`/`content` and `thinking`/`text`/`tool_use`/`tool_result`
   blocks).
3. **`broker/src/main.rs`** — the `render-transcript` and `format-stream`
   subcommands.

The run lock and `benchmark-finalize` are already harness-agnostic — they wrap
or run after whatever command is exec'd. The deny-repo-read sandbox is also
harness-agnostic but **macOS-only** (`sandbox-exec`); this work makes it
pluggable across OSes (see "Sandbox"). `caffeinate` (macOS keep-awake) is
**removed** — it is macOS-specific and not needed for portability.

## Harness landscape (researched 2026-06-14)

All three new harnesses support headless single-prompt runs, stdio MCP, a model
flag, API-key auth (no interactive login required), a config-dir isolation env
var, and a JSON output mode. The differences:

| | **claude** (today) | **codex** | **gemini** | **opencode** |
|---|---|---|---|---|
| invoke | `claude -p P` | `codex exec P` | `gemini -p P` | `opencode run P` |
| model flag | `--model` | `-m` | `-m` | `-m openrouter/org/model` |
| MCP config | `mcp.json` (`mcpServers`) | `config.toml` (`[mcp_servers.x]`) | `settings.json` (`mcpServers`) | `opencode.json` (`mcp`, command **array**) |
| bypass perms | `--permission-mode bypassPermissions` | `-a never -s workspace-write` | `--approval-mode yolo` | `--dangerously-skip-permissions` / `permission:allow` |
| auth (env) | OAuth dir / `ANTHROPIC_API_KEY` | `OPENAI_API_KEY` | `GEMINI_API_KEY` | `OPENROUTER_API_KEY` |
| config isolation | `CLAUDE_CONFIG_DIR` | `CODEX_HOME` | `GEMINI_CLI_HOME` | `OPENCODE_CONFIG` |
| transcript JSON | `--output-format stream-json` | `--json` | `--output-format stream-json` | `--format json` |

## Architecture

```
run.sh (thin orchestrator)
  ├─ parse --harness / --model
  ├─ skylinebench harness-prepare  ──>  writes config files + launch.argv + launch.env
  ├─ read argv (mapfile -d '') + export env
  ├─ wrap: sandbox (per-OS backend)  ──>  exec harness CLI  ──> stdout (harness JSON)
  │     tee transcript.jsonl | skylinebench format-stream --harness X
  ├─ skylinebench render-transcript --harness X  ──> transcript.md
  └─ skylinebench benchmark-finalize (unchanged)
```

Two new seams in the broker, described below.

### Seam 1 — Launch: `broker/src/benchmark/harness/`

A `Harness` enum parsed from the `--harness` string:

```rust
enum Harness { Claude, Codex, Gemini, Opencode }
```

A pure builder per harness, written in the project's `(deps) => ({args})`
style — each takes a single `LaunchInputs` struct and returns a `LaunchSpec`:

```rust
struct LaunchInputs {
    model: Option<String>,
    prompt: String,
    mcp_command: String,        // the `broker benchmark …` invocation (sh -c …)
    session_dir: PathBuf,
    out_dir: PathBuf,
}
```

Every run is headless — each harness uses its JSON output mode (the parsed
transcript path). `run.sh`'s old `--watch`/`--interactive` flag is removed as
part of this work (see "`run.sh` changes"); there is no interactive variant per
harness to maintain.

```rust

struct LaunchSpec {
    argv: Vec<String>,                 // harness CLI + args (prompt included);
                                       //   NO sandbox wrapper (added by run.sh)
    env: Vec<(String, String)>,        // isolation env only (e.g. CODEX_HOME=…);
                                       //   never secrets
    config_files: Vec<ConfigFile>,     // { path, contents } written into session_dir
    required_env: Vec<String>,         // secrets that must already be set (preflight)
    output_format: OutputFormat,       // selects the transcript parser
}
```

New subcommand:

```
skylinebench harness-prepare \
  --harness <name> --model <m> \
  --mcp-command <cmd> --session-dir <dir> --out <dir>
```

It writes the config file(s) to disk and emits two sidecar files into the
session dir:

- `launch.argv` — **NUL-delimited** argv. NUL is used (not newline) because the
  prompt arg contains newlines; `run.sh` reads it with `mapfile -d '' ARGV <
  launch.argv`. No `jq` dependency.
- `launch.env` — **NUL-delimited** `KEY=VALUE` isolation env pairs; `run.sh`
  exports each.

#### Per-harness LaunchSpec contents

- **claude** — `argv = claude -p P [--model m] --mcp-config <session>/mcp.json
  --strict-mcp-config --allowedTools <list> --disallowedTools WebFetch,WebSearch
  --permission-mode bypassPermissions --output-format stream-json --verbose`.
  `env = CLAUDE_CONFIG_DIR=…`. `config_files = mcp.json` (today's content).
  `required_env`: none here — `run.sh` keeps the existing OAuth-dir check, and
  `ANTHROPIC_API_KEY` also works. Behavior is byte-for-byte what runs today.
- **codex** — `argv = codex exec P -m <model> -a never -s workspace-write
  --json`. `env = CODEX_HOME=<session>/codex`. `config_files =
  <session>/codex/config.toml` with `[mcp_servers.skylinebench]` (command =
  `sh`, args = the broker invocation). Tool restriction via
  `enabled_tools`/`disabled_tools` on that server where supported.
  `required_env = [OPENAI_API_KEY]`.
- **gemini** — `argv = gemini -p P -m <model> --approval-mode yolo
  --output-format stream-json`. `env = GEMINI_CLI_HOME=<session>/gemini,
  GEMINI_CLI_TRUST_WORKSPACE=true`. `config_files =
  <session>/gemini/.gemini/settings.json` with `mcpServers.skylinebench`
  (stdio: command + args), web tools excluded via `excludeTools`/`tools.exclude`.
  `required_env = [GEMINI_API_KEY]`.
- **opencode** — `argv = opencode run P -m openrouter/<org/model> --format json
  --dangerously-skip-permissions`. `env = OPENCODE_CONFIG=<session>/opencode.json`.
  `config_files = <session>/opencode.json` with: `$schema`, `mcp.skylinebench`
  (`type: "local"`, `command:[sh,-c,…]`), `provider.openrouter` block,
  `permission: "allow"`, and built-in web tools disabled via `tools`.
  `required_env = [OPENROUTER_API_KEY]`.

The model flag is omitted when `--model` is not given (each harness then uses
its own default), matching today's Claude behavior.

### Seam 2 — Transcript: refactor `broker/src/benchmark/transcript.rs`

A normalized event model that all harnesses map into:

```rust
enum TranscriptEvent {
    SessionStart,
    Assistant(Vec<Block>),
    ToolResult(String),
}
enum Block {
    Thinking(String),
    Text(String),
    ToolUse { name: String, input: Value },
    ToolResultBlock(String),
}
```

- One **parser per harness**: `fn parse_events(harness, jsonl) -> Vec<TranscriptEvent>`
  and `fn parse_line_live(harness, line) -> Vec<TranscriptEvent>`. The Claude
  parser is today's logic moved behind this interface unchanged.
- One **renderer**, reused across harnesses: `render(events) -> String` for
  `transcript.md`, and a live formatter for the console — the existing markdown
  and console formats, now fed normalized events.
- The `render-transcript` and `format-stream` subcommands gain
  `--harness <name>` (default `claude`), so omitting it preserves today's
  output exactly.

Per-harness parser mapping:

- **codex** (`--json`): `agent_message`/`assistant_message` → `Text`,
  `reasoning` → `Thinking`, `mcp_tool_call` → `ToolUse` + `ToolResultBlock`,
  `command_execution` → `ToolUse` + `ToolResultBlock`. Tolerate both `type` and
  `item_type` field names and both message-type spellings.
- **gemini** (`stream-json`): `message`(role assistant) → `Text`, `tool_use` →
  `ToolUse`, `tool_result` → `ToolResult`. No `Thinking` (Gemini exposes none).
- **opencode** (`--format json`, JSONL): `text` parts → `Text`, `tool_use`
  state (input/output) → `ToolUse` + `ToolResultBlock`, `step_*` as turn
  boundaries.

### Parity limits (explicit)

"Full parity" means **same structure, populated with what each harness
exposes** — not identical content:

- **Gemini has no reasoning/thinking stream** (only a token count), so its
  transcripts have no `<details>Thinking</details>` blocks. This is a harness
  limitation, not a bug.
- **Codex `--json` schema drifts across versions** and `--experimental-json`
  omits tool args/results; **opencode `--format json` has a known
  end-of-stream bug** (may exit before the final event). Parsers are therefore
  defensive: skip unknown/malformed lines (as today), capture full stdout, and
  never fail the run on a parse miss.

### `run.sh` changes

- Add `--harness <name>` (default `claude`); validate against the known set,
  error on unknown.
- **Remove the `--watch`/`--interactive` flag and its branch.** All runs are
  headless; the single exec path pipes harness stdout through
  `format-stream --harness X` and then `render-transcript --harness X`. This
  deletes the `WATCH` variable, the interactive `CMD` branch, and the
  watch-specific `|| true` handling.
- **Preflight per harness:** the harness binary is on `PATH` and every
  `required_env` secret is set; otherwise exit early with a clear message
  (mirrors today's "not logged in" check). Claude keeps its existing OAuth-dir
  check.
- Replace the inline `claude …` command construction with: call
  `harness-prepare`, `mapfile -d ''` the argv, export the isolation env, select
  the sandbox wrapper for the host OS (see "Sandbox"), then wrap and exec.
- **Remove `caffeinate`** (the `KEEPAWAKE` array and its `command -v` guard).
- Record `harness.txt` next to `model.txt` in the run dir, plus `sandbox.txt`
  naming the active backend (`seatbelt` / `bubblewrap` / `firejail` / `none`)
  so a run's anti-cheat status is auditable. Copy the harness config file(s)
  into the run dir for reproducibility (as `mcp.json` is copied today).
- The **deny-repo-read sandbox wraps every harness** where a backend exists —
  it is the hard anti-cheat guarantee. Per-harness tool allowlists / web-disable
  are best-effort on top of it.
- `DRY_RUN=1` prints the resolved argv, isolation env, config file contents, and
  the selected sandbox wrapper for the chosen harness.
- Update docs that reference `--watch` / `caffeinate` (`README.md`,
  `benchmark/README.md`) to drop them and document `--harness` and the
  per-OS sandbox behavior.

## Sandbox (cross-OS, pluggable)

The sandbox enforces one invariant on every OS: **the agent process cannot read
the repository subtree** (it must not reach the scoring source). The wrapper is
selected at runtime by host OS / available tooling, all preserving that
deny-repo-read semantic:

- **macOS — `sandbox-exec` (Seatbelt):** today's profile, `(allow default)
  (deny file-read* (subpath "$ROOT"))`. Unchanged.
- **Linux — `bubblewrap` (`bwrap`), else `firejail`:** bubblewrap binds the
  real filesystem then masks the repo (`--dev-bind / /` + `--tmpfs "$ROOT"`),
  hiding repo contents while leaving everything else readable; firejail uses
  `--blacklist="$ROOT"` as the fallback. First available wins.
- **Neither available / other OS:** run **unsandboxed** with a loud stderr
  warning that anti-cheat is OFF, and record `sandbox.txt = none`. The run is
  still produced (so the benchmark works anywhere) but its integrity is flagged.

Selection lives in a small, testable unit (a `sandbox` concern: detect backend →
return the wrapper argv prefix + any profile file to write + the backend name).
`run.sh` no longer hard-errors when `sandbox-exec` is absent.

## Anti-cheat model

The sandbox (per-OS backend above) denies reading the repo subtree and wraps
whatever argv is exec'd, so it protects every harness identically — the agent
cannot read the scoring source regardless of harness, on macOS or Linux. On an
unsupported OS the run proceeds unsandboxed with a recorded `sandbox.txt = none`
warning. Tool restriction (MCP-only allowlists, disabling web tools) is layered
on where each harness supports it, but is treated as defense-in-depth, not the
primary guarantee, because the built-in tool sets and restriction syntaxes
differ per harness.

## Testing

Table-driven Rust unit tests, built TDD:

- **LaunchSpec per harness** — assert argv, isolation env, `required_env`, and
  rendered config-file contents for a fixed `LaunchInputs` (snapshot-style).
- **Transcript parser per harness** — feed captured sample-output lines (one
  fixture file per harness, committed under the broker test tree) and assert
  the normalized events and the rendered markdown.
- **Claude regression** — the refactored Claude path produces byte-identical
  `transcript.md` and live lines vs the pre-refactor output on an existing
  captured `transcript.jsonl`.
- **Sandbox backend selection** — given a (detected OS, available tools) input,
  assert the chosen backend name and the wrapper argv prefix (macOS→seatbelt,
  Linux+bwrap→bubblewrap, Linux+firejail-only→firejail, none→unsandboxed +
  warning).
- **`run.sh` DRY_RUN** — for each harness, the printed argv/env/config/sandbox
  match expectations.

The pure-function, typed design is what makes these tests cheap — the main
payoff of putting the adapters in Rust.

## Scope & staging

One spec, one feature. Implementation is staged so each step is independently
verifiable:

1. **Harness module + normalized transcript + pluggable sandbox**, and
   **refactor Claude onto them** — behavior-preserving on macOS (existing runs
   produce identical output); `caffeinate` removed; Linux/unsandboxed backends
   added.
2. **codex** — LaunchSpec + parser + tests + fixture.
3. **gemini** — LaunchSpec + parser + tests + fixture.
4. **opencode** — LaunchSpec + parser + tests + fixture.

Each new harness after step 1 is an additive `LaunchSpec` + parser + tests +
one sample fixture, with no change to the broker tools or scoring.

## Out of scope

- Harness presets / model→harness inference (explicit `--harness` only).
- Changes to scoring, maps, the MCP tool set, or `benchmark-finalize`.
- Website / results-page changes for new models (separate work once runs
  exist).
- Windows sandboxing (the harness runner targets macOS + Linux; other OSes run
  unsandboxed with a recorded warning).
