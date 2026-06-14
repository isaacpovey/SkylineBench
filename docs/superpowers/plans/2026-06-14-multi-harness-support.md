# Multi-harness support (codex, gemini, opencode) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let `benchmark/run.sh` drive the benchmark on non-Claude agent harnesses (codex, gemini, opencode/OpenRouter) via an `--harness` flag, with all launch/transcript logic typed and unit-tested in the Rust broker, and a cross-OS anti-cheat sandbox.

**Architecture:** Two new typed seams in the broker — a `harness` module (a `Harness` enum + pure `LaunchSpec` builders per harness) and a `sandbox` module (pure backend selection). `transcript.rs` is refactored to a normalized event model with one parser per harness feeding a single renderer. `run.sh` becomes a thin orchestrator that asks the broker (`harness-prepare`, `sandbox-prepare`) for the argv/env/config/wrapper, then execs. The MCP broker tools and scoring are untouched.

**Tech Stack:** Rust (clap, serde_json, tokio), Bash (`run.sh`), `cargo test` for unit tests (inline `#[cfg(test)]` modules, matching the existing convention).

---

## Design reference (read before starting)

The spec is `docs/superpowers/specs/2026-06-14-multi-harness-support-design.md`. Key facts the tasks below depend on:

- All harnesses emit **line-delimited JSON** (JSONL), so a per-line parser works for every harness.
- Each harness differs in: invoke command, model flag, MCP config file/format, bypass-permission flag, auth env var, config-isolation env var. The per-harness `LaunchSpec` builder encodes these.
- "Full parity" = same `transcript.md` structure populated with what each harness exposes. Gemini has no reasoning events (no Thinking blocks). Codex/opencode JSON schemas are version-sensitive — parsers are defensive (skip unknown/malformed; never panic).

## File structure

**Create:**
- `broker/src/benchmark/harness/mod.rs` — `Harness` enum, `LaunchInputs`, `LaunchSpec`, `ConfigFile`, `build()` dispatcher.
- `broker/src/benchmark/harness/claude.rs` — Claude `LaunchSpec` builder + `ALLOWED` tool list.
- `broker/src/benchmark/harness/codex.rs` — Codex `LaunchSpec` builder.
- `broker/src/benchmark/harness/gemini.rs` — Gemini `LaunchSpec` builder.
- `broker/src/benchmark/harness/opencode.rs` — opencode `LaunchSpec` builder.
- `broker/src/benchmark/sandbox.rs` — `SandboxInputs`, `Backend`, `SandboxPlan`, pure `select()`.

**Modify:**
- `broker/src/benchmark/transcript.rs` — normalized `Event`/`Block` model, per-harness parsers, single renderer; `render_transcript`/`format_event_live` gain a `Harness` arg.
- `broker/src/benchmark/mod.rs` — module decls + re-exports.
- `broker/src/main.rs` — `--harness` on `render-transcript`/`format-stream`; new `harness-prepare` + `sandbox-prepare` subcommands.
- `benchmark/run.sh` — `--harness` flag, preflight, harness-prepare/sandbox-prepare wiring, remove `--watch` + `caffeinate`.
- `README.md`, `benchmark/README.md` — document `--harness`, drop `--watch`/`caffeinate`.

---

# Phase 1 — Foundation: harness module, sandbox, transcript refactor, Claude on the new path

This phase adds the abstraction and **moves Claude onto it with identical behavior**. After Phase 1, `run.sh` (claude) produces byte-identical output to today, and the broker compiles with the new modules.

## Task 1: Harness enum + core types

**Files:**
- Create: `broker/src/benchmark/harness/mod.rs`
- Modify: `broker/src/benchmark/mod.rs:13` (add `pub mod harness;`)

- [ ] **Step 1: Write the failing test**

Create `broker/src/benchmark/harness/mod.rs` with only the types + test (no builders yet):

```rust
use std::path::PathBuf;

/// Which agent CLI drives the benchmark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Harness {
    Claude,
    Codex,
    Gemini,
    Opencode,
}

impl Harness {
    pub fn parse(s: &str) -> Option<Harness> {
        match s {
            "claude" => Some(Harness::Claude),
            "codex" => Some(Harness::Codex),
            "gemini" => Some(Harness::Gemini),
            "opencode" => Some(Harness::Opencode),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Harness::Claude => "claude",
            Harness::Codex => "codex",
            Harness::Gemini => "gemini",
            Harness::Opencode => "opencode",
        }
    }
}

/// Everything a harness builder needs to construct its launch command.
pub struct LaunchInputs {
    pub model: Option<String>,
    pub prompt: String,
    /// The shell string that starts the MCP broker (passed to `sh -c`).
    pub mcp_shell: String,
    /// Per-run scratch dir, outside the repo, where config files are written.
    pub session_dir: PathBuf,
}

/// A config file the harness needs on disk before launch.
pub struct ConfigFile {
    pub path: PathBuf,
    pub contents: String,
}

/// The fully-resolved plan for launching one harness.
pub struct LaunchSpec {
    /// Harness CLI + args (prompt included). NO sandbox wrapper.
    pub argv: Vec<String>,
    /// Isolation env to export (e.g. CODEX_HOME). Never secrets.
    pub env: Vec<(String, String)>,
    /// Config files to write into session_dir.
    pub config_files: Vec<ConfigFile>,
    /// Secret env vars that must already be set (preflight).
    pub required_env: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_harnesses_and_rejects_unknown() {
        assert_eq!(Harness::parse("claude"), Some(Harness::Claude));
        assert_eq!(Harness::parse("codex"), Some(Harness::Codex));
        assert_eq!(Harness::parse("gemini"), Some(Harness::Gemini));
        assert_eq!(Harness::parse("opencode"), Some(Harness::Opencode));
        assert_eq!(Harness::parse("gpt"), None);
        assert_eq!(Harness::Codex.as_str(), "codex");
    }
}
```

Add to `broker/src/benchmark/mod.rs` after line 13 (`pub mod transcript;`) — only the `harness` module for now (`sandbox` is added in Task 6):

```rust
pub mod harness;
```

- [ ] **Step 2: Run test to verify it fails (compiles + passes is the goal, but confirm it's wired)**

Run: `cargo test --manifest-path broker/Cargo.toml harness::tests::parses_known`
Expected: PASS (the type test is self-contained). If it does not compile, fix the module wiring.

- [ ] **Step 3: Commit**

```bash
git add broker/src/benchmark/harness/mod.rs broker/src/benchmark/mod.rs
git commit -m "feat(broker): add Harness enum + launch types"
```

## Task 2: Claude LaunchSpec builder

**Files:**
- Create: `broker/src/benchmark/harness/claude.rs`
- Modify: `broker/src/benchmark/harness/mod.rs` (add `mod claude;` + `build()` dispatcher)

- [ ] **Step 1: Write the failing test**

Create `broker/src/benchmark/harness/claude.rs`:

```rust
use super::{ConfigFile, LaunchInputs, LaunchSpec};

/// The MCP tool allowlist Claude is given (the benchmark tools only).
pub const ALLOWED: &str = "mcp__skylinebench__build_road,mcp__skylinebench__bulldoze,mcp__skylinebench__upgrade_road,mcp__skylinebench__set_zoning,mcp__skylinebench__control_time,mcp__skylinebench__get_city_overview,mcp__skylinebench__observe_area,mcp__skylinebench__get_metrics,mcp__skylinebench__list_road_types,mcp__skylinebench__list_zone_types,mcp__skylinebench__render_map,mcp__skylinebench__submit_solution,mcp__skylinebench__query_segments,mcp__skylinebench__apply_plan,mcp__skylinebench__trace_route";

pub fn spec(inputs: &LaunchInputs) -> LaunchSpec {
    let mcp_config = inputs.session_dir.join("mcp.json");
    let contents = serde_json::json!({
        "mcpServers": {
            "skylinebench": { "command": "sh", "args": ["-c", inputs.mcp_shell] }
        }
    });

    let mut argv = vec!["claude".to_string(), "-p".to_string(), inputs.prompt.clone()];
    if let Some(model) = &inputs.model {
        argv.push("--model".to_string());
        argv.push(model.clone());
    }
    argv.extend(
        [
            "--mcp-config",
            &mcp_config.display().to_string(),
            "--strict-mcp-config",
            "--allowedTools",
            ALLOWED,
            "--disallowedTools",
            "WebFetch,WebSearch",
            "--permission-mode",
            "bypassPermissions",
            "--output-format",
            "stream-json",
            "--verbose",
        ]
        .map(String::from),
    );

    LaunchSpec {
        argv,
        // CLAUDE_CONFIG_DIR is the operator's persistent OAuth dir, set by run.sh.
        env: vec![],
        config_files: vec![ConfigFile {
            path: mcp_config,
            contents: serde_json::to_string_pretty(&contents).unwrap(),
        }],
        required_env: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn inputs() -> LaunchInputs {
        LaunchInputs {
            model: Some("claude-opus-4-8".to_string()),
            prompt: "improve traffic".to_string(),
            mcp_shell: "/s/skylinebench benchmark --map m".to_string(),
            session_dir: PathBuf::from("/tmp/sess"),
        }
    }

    #[test]
    fn builds_claude_argv_with_model_and_stream_json() {
        let spec = spec(&inputs());
        assert_eq!(spec.argv[0], "claude");
        assert!(spec.argv.contains(&"-p".to_string()));
        assert!(spec.argv.contains(&"improve traffic".to_string()));
        assert!(spec.argv.windows(2).any(|w| w == ["--model", "claude-opus-4-8"]));
        assert!(spec.argv.contains(&"stream-json".to_string()));
        assert!(spec.argv.contains(&"bypassPermissions".to_string()));
        assert!(spec.required_env.is_empty());
    }

    #[test]
    fn omits_model_when_absent() {
        let mut i = inputs();
        i.model = None;
        let spec = spec(&i);
        assert!(!spec.argv.contains(&"--model".to_string()));
    }

    #[test]
    fn writes_mcp_json_with_server() {
        let spec = spec(&inputs());
        let cf = &spec.config_files[0];
        assert!(cf.path.ends_with("mcp.json"));
        assert!(cf.contents.contains("mcpServers"));
        assert!(cf.contents.contains("skylinebench"));
        assert!(cf.contents.contains("benchmark --map m"));
    }
}
```

Add to `broker/src/benchmark/harness/mod.rs` (after the type definitions, before `#[cfg(test)]`):

```rust
mod claude;

/// Build the launch plan for a harness.
pub fn build(harness: Harness, inputs: &LaunchInputs) -> LaunchSpec {
    match harness {
        Harness::Claude => claude::spec(inputs),
        // Added in later phases:
        Harness::Codex => todo!("codex builder — Task 12"),
        Harness::Gemini => todo!("gemini builder — Task 14"),
        Harness::Opencode => todo!("opencode builder — Task 16"),
    }
}
```

- [ ] **Step 2: Run test to verify it fails, then passes**

Run: `cargo test --manifest-path broker/Cargo.toml harness::claude`
Expected: PASS (all three claude tests). The `todo!()` arms are never hit by these tests.

- [ ] **Step 3: Commit**

```bash
git add broker/src/benchmark/harness/
git commit -m "feat(broker): claude LaunchSpec builder + build() dispatcher"
```

## Task 3: Normalized transcript model + Claude parser (refactor)

This replaces the Claude-specific parsing in `transcript.rs` with a normalized model, **preserving exact output**. The signatures of `render_transcript`/`format_event_live` change to take a `Harness`.

**Files:**
- Modify: `broker/src/benchmark/transcript.rs` (full rewrite of non-test code; keep + extend tests)

- [ ] **Step 1: Write the new model, parser, and renderer**

Replace the entire non-test portion of `broker/src/benchmark/transcript.rs` (lines 1–166) with:

```rust
use serde_json::Value;

use crate::benchmark::harness::Harness;

/// One renderable block inside a turn.
pub enum Block {
    Thinking(String),
    Text(String),
    ToolUse { name: String, input: Value },
    /// A tool result's inner text parts (one harness "tool_result" block).
    ToolResult { parts: Vec<String> },
}

/// A normalized transcript event, harness-independent.
pub enum Event {
    SessionStart,
    /// Assistant turn: Thinking / Text / ToolUse blocks.
    Assistant(Vec<Block>),
    /// Tool-result turn: ToolResult blocks.
    Results(Vec<Block>),
    /// Final result text (live "done" line only).
    Done(String),
}

/// Parse one JSONL line (already deserialized) into 0+ normalized events.
pub fn parse_line(harness: Harness, v: &Value) -> Vec<Event> {
    match harness {
        Harness::Claude => parse_claude(v),
        Harness::Codex => parse_codex(v),
        Harness::Gemini => parse_gemini(v),
        Harness::Opencode => parse_opencode(v),
    }
}

fn parse_claude(v: &Value) -> Vec<Event> {
    let kind = match v.get("type").and_then(|t| t.as_str()) {
        Some(k) => k,
        None => return vec![],
    };
    match kind {
        "system" if v.get("subtype").and_then(|s| s.as_str()) == Some("init") => {
            vec![Event::SessionStart]
        }
        "assistant" => {
            let blocks = claude_blocks(v, false);
            if blocks.is_empty() { vec![] } else { vec![Event::Assistant(blocks)] }
        }
        "user" => {
            let blocks = claude_blocks(v, true);
            if blocks.is_empty() { vec![] } else { vec![Event::Results(blocks)] }
        }
        "result" => v
            .get("result")
            .and_then(|r| r.as_str())
            .map(|r| vec![Event::Done(r.to_string())])
            .unwrap_or_default(),
        _ => vec![],
    }
}

/// Collect blocks from a claude message. `results` selects tool_result blocks
/// (user turn) vs thinking/text/tool_use blocks (assistant turn).
fn claude_blocks(v: &Value, results: bool) -> Vec<Block> {
    let content = match v.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_array()) {
        Some(c) => c,
        None => return vec![],
    };
    content
        .iter()
        .filter_map(|b| {
            let t = b.get("type")?.as_str()?;
            match (results, t) {
                (false, "thinking") => Some(Block::Thinking(b.get("thinking")?.as_str()?.to_string())),
                (false, "text") => Some(Block::Text(b.get("text")?.as_str()?.to_string())),
                (false, "tool_use") => Some(Block::ToolUse {
                    name: b.get("name")?.as_str()?.to_string(),
                    input: b.get("input").cloned().unwrap_or(Value::Null),
                }),
                (true, "tool_result") => {
                    let parts = b
                        .get("content")?
                        .as_array()?
                        .iter()
                        .filter_map(|c| c.get("text").and_then(|t| t.as_str()).map(String::from))
                        .collect();
                    Some(Block::ToolResult { parts })
                }
                _ => None,
            }
        })
        .collect()
}

// Stubs for later phases — return no events so other harnesses are inert until
// implemented (Tasks 13, 15, 17).
fn parse_codex(_v: &Value) -> Vec<Event> { vec![] }
fn parse_gemini(_v: &Value) -> Vec<Event> { vec![] }
fn parse_opencode(_v: &Value) -> Vec<Event> { vec![] }

/// Render a captured JSONL transcript into readable markdown.
pub fn render_transcript(harness: Harness, jsonl: &str) -> String {
    jsonl
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .flat_map(|v| parse_line(harness, &v))
        .filter_map(|e| render_md_event(&e))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_md_event(event: &Event) -> Option<String> {
    match event {
        Event::Assistant(blocks) => {
            let rendered: Vec<String> = blocks.iter().filter_map(render_md_block).collect();
            (!rendered.is_empty()).then(|| format!("### Assistant\n\n{}", rendered.join("\n\n")))
        }
        Event::Results(blocks) => {
            let rendered: Vec<String> = blocks.iter().filter_map(render_md_block).collect();
            (!rendered.is_empty()).then(|| format!("### Tool result\n\n{}", rendered.join("\n\n")))
        }
        Event::SessionStart | Event::Done(_) => None,
    }
}

fn render_md_block(block: &Block) -> Option<String> {
    match block {
        Block::Thinking(t) => {
            Some(format!("<details><summary>Thinking</summary>\n\n{t}\n\n</details>"))
        }
        Block::Text(t) => Some(t.clone()),
        Block::ToolUse { name, input } => {
            let pretty = serde_json::to_string_pretty(input).unwrap_or_else(|_| input.to_string());
            Some(format!("**→ {name}**\n```json\n{pretty}\n```"))
        }
        Block::ToolResult { parts } => Some(format!("```\n{}\n```", parts.join("\n"))),
    }
}

/// Format one JSONL line (already deserialized) into a compact live console
/// string. Returns None when there is nothing useful to show.
pub fn format_event_live(harness: Harness, v: &Value) -> Option<String> {
    let lines: Vec<String> = parse_line(harness, v).iter().filter_map(render_live_event).collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn render_live_event(event: &Event) -> Option<String> {
    match event {
        Event::SessionStart => Some("● session started".to_string()),
        Event::Done(r) => Some(format!("● done: {r}")),
        Event::Assistant(blocks) => {
            let lines: Vec<String> = blocks.iter().filter_map(render_live_block).collect();
            (!lines.is_empty()).then(|| lines.join("\n"))
        }
        Event::Results(blocks) => {
            let lines: Vec<String> = blocks
                .iter()
                .filter_map(|b| match b {
                    Block::ToolResult { parts } => render_live_result(parts),
                    _ => None,
                })
                .collect();
            (!lines.is_empty()).then(|| lines.join("\n"))
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    let out: String = s.chars().take(max).collect();
    if s.chars().count() > max { format!("{out}…") } else { out }
}

fn render_live_block(block: &Block) -> Option<String> {
    match block {
        Block::Thinking(t) => {
            let t = t.trim();
            (!t.is_empty()).then(|| {
                let indented = t.lines().map(|l| format!("  {l}")).collect::<Vec<_>>().join("\n");
                format!("  [thinking]\n{indented}")
            })
        }
        Block::Text(t) => {
            let t = t.trim();
            (!t.is_empty()).then(|| format!("  {t}"))
        }
        Block::ToolUse { name, input } => {
            let name = name.trim_start_matches("mcp__skylinebench__");
            Some(format!("  → {name} {}", truncate(&input.to_string(), 120)))
        }
        Block::ToolResult { .. } => None,
    }
}

fn render_live_result(parts: &[String]) -> Option<String> {
    let text = parts.join(" ");
    if let Ok(v) = serde_json::from_str::<Value>(&text) {
        if let Some(p) = v.get("city_status").or_else(|| v.get("benchmark_progress")) {
            let optf = |new: &str, old: &str, prec: usize| {
                p.get(new)
                    .or_else(|| p.get(old))
                    .and_then(|x| x.as_f64())
                    .map_or("?".to_string(), |n| format!("{n:.prec$}"))
            };
            let getu = |new: &str, old: &str| {
                p.get(new).or_else(|| p.get(old)).and_then(|x| x.as_u64()).unwrap_or(0)
            };
            let rejected = v.get("ok").and_then(|x| x.as_bool()) == Some(false);
            let junctions = p
                .get("congested_junctions")
                .and_then(|x| x.as_u64())
                .map_or("?".to_string(), |n| n.to_string());
            return Some(format!(
                "    ↳ congested {}m / {} junctions  flow {}  changes {}  spent {}  {}s left{}",
                optf("congested_road_meters", "congested_meters_current", 0),
                junctions,
                optf("traffic_flow", "flow_current", 1),
                getu("changes_made", "num_changes"),
                p.get("money_spent").and_then(|x| x.as_i64()).unwrap_or(0),
                getu("time_remaining", "seconds_remaining"),
                if rejected { "  (rejected)" } else { "" },
            ));
        }
    }
    Some(format!("    ↳ {}", truncate(text.trim(), 80)))
}
```

- [ ] **Step 2: Update the existing tests to pass a `Harness`**

In the `#[cfg(test)] mod tests` block, update the two call sites that take a string and the four that take a `Value`. Add `use crate::benchmark::harness::Harness;` at the top of the test module, then:
- `render_transcript(jsonl)` → `render_transcript(Harness::Claude, jsonl)`
- `render_transcript("not json\n{}\n")` → `render_transcript(Harness::Claude, "not json\n{}\n")`
- each `format_event_live(&event)` → `format_event_live(Harness::Claude, &event)`

- [ ] **Step 3: Run the transcript tests**

Run: `cargo test --manifest-path broker/Cargo.toml transcript`
Expected: PASS — all 7 existing tests, proving the refactor preserves Claude output exactly.

- [ ] **Step 4: Commit**

```bash
git add broker/src/benchmark/transcript.rs
git commit -m "refactor(broker): normalized transcript events + claude parser"
```

## Task 4: Wire `--harness` into `render-transcript` and `format-stream`

**Files:**
- Modify: `broker/src/main.rs:28-37` (subcommand defs), `:104-121` (handlers)
- Modify: `broker/src/benchmark/mod.rs:21` (re-export `Harness`, `parse_line`)

- [ ] **Step 1: Re-export from the benchmark module**

In `broker/src/benchmark/mod.rs`, change the transcript re-export line (currently line 21) to:

```rust
pub use transcript::{format_event_live, render_transcript};
pub use harness::Harness;
```

- [ ] **Step 2: Add `--harness` to the two subcommands**

In `broker/src/main.rs`, change the `RenderTranscript` and `FormatStream` variants:

```rust
    /// Render a captured JSONL transcript to readable markdown.
    RenderTranscript {
        #[arg(long)]
        input: std::path::PathBuf,
        #[arg(long)]
        out: std::path::PathBuf,
        #[arg(long, default_value = "claude")]
        harness: String,
    },
    /// Read JSONL on stdin and print a human-readable line per event
    /// (for live console display during a run).
    FormatStream {
        #[arg(long, default_value = "claude")]
        harness: String,
    },
```

- [ ] **Step 3: Update the handlers**

In `broker/src/main.rs`, replace the `RenderTranscript` and `FormatStream` match arms:

```rust
        Command::RenderTranscript { input, out, harness } => {
            let harness = skylinebench::benchmark::Harness::parse(&harness)
                .ok_or_else(|| anyhow::anyhow!("unknown harness: {harness}"))?;
            let jsonl = std::fs::read_to_string(&input)?;
            std::fs::write(&out, skylinebench::benchmark::render_transcript(harness, &jsonl))?;
        }
        Command::FormatStream { harness } => {
            use std::io::{BufRead, Write};
            let harness = skylinebench::benchmark::Harness::parse(&harness)
                .ok_or_else(|| anyhow::anyhow!("unknown harness: {harness}"))?;
            let stdin = std::io::stdin();
            let mut out = std::io::stdout();
            for line in stdin.lock().lines() {
                let line = line?;
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(text) = skylinebench::benchmark::format_event_live(harness, &v) {
                        writeln!(out, "{text}")?;
                        out.flush()?;
                    }
                }
            }
        }
```

- [ ] **Step 4: Build to verify**

Run: `cargo build --manifest-path broker/Cargo.toml`
Expected: clean build.

- [ ] **Step 5: Commit**

```bash
git add broker/src/main.rs broker/src/benchmark/mod.rs
git commit -m "feat(broker): --harness flag on render-transcript and format-stream"
```

## Task 5: `harness-prepare` subcommand

Writes config files + `launch.argv` (NUL-delimited) + `launch.env` + `launch.required-env` into the session dir.

**Files:**
- Modify: `broker/src/main.rs` (new subcommand variant + handler)
- Modify: `broker/src/benchmark/mod.rs` (re-export `harness::{build, LaunchInputs}`)

- [ ] **Step 1: Re-export builder types**

In `broker/src/benchmark/mod.rs`, **replace** the `pub use harness::Harness;` line added in Task 4 with the fuller re-export (avoids a duplicate `Harness` import):

```rust
pub use harness::{build as build_launch, ConfigFile, Harness, LaunchInputs, LaunchSpec};
```

- [ ] **Step 2: Add the subcommand variant**

In `broker/src/main.rs`, add to the `Command` enum:

```rust
    /// Resolve a harness launch plan: write its config files and emit
    /// launch.argv (NUL-delimited), launch.env (NUL-delimited KEY=VALUE), and
    /// launch.required-env (newline-delimited) into --session-dir.
    HarnessPrepare {
        #[arg(long)]
        harness: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        prompt_file: std::path::PathBuf,
        #[arg(long)]
        mcp_shell: String,
        #[arg(long)]
        session_dir: std::path::PathBuf,
    },
```

- [ ] **Step 3: Add the handler**

In `broker/src/main.rs`, add a match arm:

```rust
        Command::HarnessPrepare { harness, model, prompt_file, mcp_shell, session_dir } => {
            let harness = skylinebench::benchmark::Harness::parse(&harness)
                .ok_or_else(|| anyhow::anyhow!("unknown harness: {harness}"))?;
            let prompt = std::fs::read_to_string(&prompt_file)?;
            let inputs = skylinebench::benchmark::LaunchInputs {
                model,
                prompt,
                mcp_shell,
                session_dir: session_dir.clone(),
            };
            let spec = skylinebench::benchmark::build_launch(harness, &inputs);

            for cf in &spec.config_files {
                if let Some(parent) = cf.path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&cf.path, &cf.contents)?;
            }

            let argv_blob: Vec<u8> =
                spec.argv.iter().flat_map(|a| a.as_bytes().iter().copied().chain(std::iter::once(0u8))).collect();
            std::fs::write(session_dir.join("launch.argv"), argv_blob)?;

            let env_blob: Vec<u8> = spec
                .env
                .iter()
                .flat_map(|(k, v)| format!("{k}={v}").into_bytes().into_iter().chain(std::iter::once(0u8)))
                .collect();
            std::fs::write(session_dir.join("launch.env"), env_blob)?;

            std::fs::write(session_dir.join("launch.required-env"), spec.required_env.join("\n"))?;
        }
```

- [ ] **Step 4: Smoke-test it end to end**

Run:

```bash
cargo build --manifest-path broker/Cargo.toml && \
mkdir -p /tmp/hp && printf 'do the thing' > /tmp/hp/prompt.md && \
./broker/target/debug/skylinebench harness-prepare --harness claude \
  --model claude-opus-4-8 --prompt-file /tmp/hp/prompt.md \
  --mcp-shell '/s/skylinebench benchmark --map m' --session-dir /tmp/hp && \
echo "--- argv ---" && tr '\0' '\n' < /tmp/hp/launch.argv && \
echo "--- mcp.json ---" && cat /tmp/hp/mcp.json
```

Expected: `launch.argv` lists `claude`, `-p`, `do the thing`, `--model`, `claude-opus-4-8`, … and `mcp.json` contains the `skylinebench` server. `launch.required-env` is empty.

- [ ] **Step 5: Commit**

```bash
git add broker/src/main.rs broker/src/benchmark/mod.rs
git commit -m "feat(broker): harness-prepare subcommand"
```

## Task 6: Sandbox backend selection (pure)

**Files:**
- Create: `broker/src/benchmark/sandbox.rs`
- Modify: `broker/src/benchmark/mod.rs` (add `pub mod sandbox;`)

- [ ] **Step 1: Write the failing test + module**

Create `broker/src/benchmark/sandbox.rs`:

```rust
use std::path::PathBuf;

use crate::benchmark::harness::ConfigFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Mac,
    Linux,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Seatbelt,
    Bubblewrap,
    Firejail,
    None,
}

impl Backend {
    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Seatbelt => "seatbelt",
            Backend::Bubblewrap => "bubblewrap",
            Backend::Firejail => "firejail",
            Backend::None => "none",
        }
    }
}

pub struct SandboxInputs {
    pub os: Os,
    pub sandbox_exec_available: bool,
    pub bwrap_available: bool,
    pub firejail_available: bool,
    /// Repo root to deny reads of (the anti-cheat invariant).
    pub repo_root: PathBuf,
    pub session_dir: PathBuf,
}

pub struct SandboxPlan {
    pub backend: Backend,
    /// Wrapper argv prefix to prepend to the harness argv.
    pub wrapper_argv: Vec<String>,
    /// Profile file to write before launch, if the backend needs one.
    pub profile_file: Option<ConfigFile>,
    /// Stderr warning when anti-cheat is OFF (backend == None).
    pub warning: Option<String>,
}

/// Select a sandbox backend that preserves the deny-repo-read invariant on the
/// given host, or None (unsandboxed) with a warning when none is available.
pub fn select(inputs: &SandboxInputs) -> SandboxPlan {
    let root = inputs.repo_root.display().to_string();
    match inputs.os {
        Os::Mac if inputs.sandbox_exec_available => {
            let profile_path = inputs.session_dir.join("deny-repo.sb");
            let contents = format!("(version 1)\n(allow default)\n(deny file-read* (subpath \"{root}\"))\n");
            SandboxPlan {
                backend: Backend::Seatbelt,
                wrapper_argv: vec![
                    "sandbox-exec".to_string(),
                    "-f".to_string(),
                    profile_path.display().to_string(),
                ],
                profile_file: Some(ConfigFile { path: profile_path, contents }),
                warning: None,
            }
        }
        Os::Linux if inputs.bwrap_available => SandboxPlan {
            backend: Backend::Bubblewrap,
            wrapper_argv: vec![
                "bwrap".to_string(),
                "--dev-bind".to_string(),
                "/".to_string(),
                "/".to_string(),
                "--tmpfs".to_string(),
                root,
                "--die-with-parent".to_string(),
            ],
            profile_file: None,
            warning: None,
        },
        Os::Linux if inputs.firejail_available => SandboxPlan {
            backend: Backend::Firejail,
            wrapper_argv: vec![
                "firejail".to_string(),
                "--quiet".to_string(),
                "--noprofile".to_string(),
                format!("--blacklist={root}"),
            ],
            profile_file: None,
            warning: None,
        },
        _ => SandboxPlan {
            backend: Backend::None,
            wrapper_argv: vec![],
            profile_file: None,
            warning: Some(format!(
                "anti-cheat sandbox unavailable on this host (no sandbox-exec/bwrap/firejail) — the agent CAN read {root}; this run is NOT integrity-protected"
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(os: Os) -> SandboxInputs {
        SandboxInputs {
            os,
            sandbox_exec_available: false,
            bwrap_available: false,
            firejail_available: false,
            repo_root: PathBuf::from("/repo"),
            session_dir: PathBuf::from("/sess"),
        }
    }

    #[test]
    fn mac_uses_seatbelt_with_profile() {
        let mut i = base(Os::Mac);
        i.sandbox_exec_available = true;
        let plan = select(&i);
        assert_eq!(plan.backend, Backend::Seatbelt);
        assert_eq!(plan.wrapper_argv[0], "sandbox-exec");
        let profile = plan.profile_file.unwrap();
        assert!(profile.contents.contains("(deny file-read* (subpath \"/repo\"))"));
    }

    #[test]
    fn linux_prefers_bubblewrap_then_firejail() {
        let mut i = base(Os::Linux);
        i.bwrap_available = true;
        i.firejail_available = true;
        assert_eq!(select(&i).backend, Backend::Bubblewrap);

        i.bwrap_available = false;
        let plan = select(&i);
        assert_eq!(plan.backend, Backend::Firejail);
        assert!(plan.wrapper_argv.contains(&"--blacklist=/repo".to_string()));
    }

    #[test]
    fn no_backend_warns_and_is_unsandboxed() {
        let plan = select(&base(Os::Other));
        assert_eq!(plan.backend, Backend::None);
        assert!(plan.wrapper_argv.is_empty());
        assert!(plan.warning.unwrap().contains("NOT integrity-protected"));
    }
}
```

Add `pub mod sandbox;` to `broker/src/benchmark/mod.rs` (right after `pub mod harness;`).

- [ ] **Step 2: Run the sandbox tests**

Run: `cargo test --manifest-path broker/Cargo.toml sandbox`
Expected: PASS (3 tests).

- [ ] **Step 3: Commit**

```bash
git add broker/src/benchmark/sandbox.rs broker/src/benchmark/mod.rs
git commit -m "feat(broker): pure cross-OS sandbox backend selection"
```

## Task 7: `sandbox-prepare` subcommand

Detects the host backend, writes any profile file + `sandbox.argv`, prints the backend name to stdout and any warning to stderr.

**Files:**
- Modify: `broker/src/main.rs` (subcommand + handler)
- Modify: `broker/src/benchmark/mod.rs` (re-export sandbox items)

- [ ] **Step 1: Re-export sandbox items**

In `broker/src/benchmark/mod.rs` add:

```rust
pub use sandbox::{select as select_sandbox, Backend, Os, SandboxInputs};
```

- [ ] **Step 2: Add the subcommand variant**

```rust
    /// Detect this host's sandbox backend, write its profile + sandbox.argv
    /// (NUL-delimited wrapper prefix) into --session-dir, print the backend
    /// name to stdout, and warn on stderr if unsandboxed.
    SandboxPrepare {
        #[arg(long)]
        root: std::path::PathBuf,
        #[arg(long)]
        session_dir: std::path::PathBuf,
    },
```

- [ ] **Step 3: Add the handler (with host detection)**

```rust
        Command::SandboxPrepare { root, session_dir } => {
            use skylinebench::benchmark::{select_sandbox, Os, SandboxInputs};

            fn on_path(bin: &str) -> bool {
                std::env::var_os("PATH")
                    .map(|paths| {
                        std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file())
                    })
                    .unwrap_or(false)
            }

            let os = if cfg!(target_os = "macos") {
                Os::Mac
            } else if cfg!(target_os = "linux") {
                Os::Linux
            } else {
                Os::Other
            };

            let inputs = SandboxInputs {
                os,
                sandbox_exec_available: on_path("sandbox-exec"),
                bwrap_available: on_path("bwrap"),
                firejail_available: on_path("firejail"),
                repo_root: root,
                session_dir: session_dir.clone(),
            };
            let plan = select_sandbox(&inputs);

            if let Some(cf) = &plan.profile_file {
                std::fs::write(&cf.path, &cf.contents)?;
            }
            let blob: Vec<u8> = plan
                .wrapper_argv
                .iter()
                .flat_map(|a| a.as_bytes().iter().copied().chain(std::iter::once(0u8)))
                .collect();
            std::fs::write(session_dir.join("sandbox.argv"), blob)?;

            if let Some(w) = &plan.warning {
                eprintln!("WARNING: {w}");
            }
            println!("{}", plan.backend.as_str());
        }
```

- [ ] **Step 4: Smoke-test**

Run:

```bash
cargo build --manifest-path broker/Cargo.toml && \
mkdir -p /tmp/sb && ./broker/target/debug/skylinebench sandbox-prepare --root "$PWD" --session-dir /tmp/sb && \
echo "--- sandbox.argv ---" && tr '\0' '\n' < /tmp/sb/sandbox.argv
```

Expected (on macOS): prints `seatbelt`, writes `/tmp/sb/deny-repo.sb`, and `sandbox.argv` lists `sandbox-exec`, `-f`, `/tmp/sb/deny-repo.sb`.

- [ ] **Step 5: Commit**

```bash
git add broker/src/main.rs broker/src/benchmark/mod.rs
git commit -m "feat(broker): sandbox-prepare subcommand with host detection"
```

## Task 8: Rewrite `run.sh` onto the new path (claude only)

Replace Claude-specific launch construction with `harness-prepare` + `sandbox-prepare`, add `--harness` (default claude), remove `--watch` and `caffeinate`. After this task, a claude run still works and is byte-identical.

**Files:**
- Modify: `benchmark/run.sh`

- [ ] **Step 1: Replace argument parsing (remove `--watch`, add `--harness`)**

In `benchmark/run.sh`, change the variable defaults and the arg loop. Replace:

```bash
MODEL=""
WATCH=0
```

with:

```bash
MODEL=""
HARNESS="claude"
```

Replace the two arg-loop cases:

```bash
    --model) MODEL="$2"; shift 2 ;;
    --watch|--interactive) WATCH=1; shift ;;
```

with:

```bash
    --model) MODEL="$2"; shift 2 ;;
    --harness) HARNESS="$2"; shift 2 ;;
```

And update the usage line:

```bash
[ -n "$MAP" ] || { echo "usage: run.sh --map <id> [--harness claude|codex|gemini|opencode] [--model NAME] [--mod-url URL] [--map-source SRC] [--out DIR]" >&2; exit 2; }
```

- [ ] **Step 2: Gate the Claude OAuth/config block on `--harness claude`**

Wrap the existing `CLAUDE_CONFIG_DIR` block (from the `CLAUDE_CONFIG_DIR="${BENCH_CLAUDE_CONFIG:-...}"` line through `export CLAUDE_CONFIG_DIR`) so it only runs for claude:

```bash
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
```

- [ ] **Step 3: Replace MCP config + command construction**

Find the block that builds `MCP_CONFIG` (the `cat > "$MCP_CONFIG" <<JSON … JSON` heredoc and the `cp "$MCP_CONFIG" "$OUT_DIR/mcp.json"` line) and the later `PROMPT=…`, `ALLOWED=…`, `DISALLOWED=…`, `SANDBOX=(...)`, `KEEPAWAKE=(...)`, `MODEL_ARGS=(...)`, and `if [ "$WATCH" -eq 1 ]` CMD construction. Replace **all of that** (from the `MCP_CONFIG="$SESSION_DIR/mcp.json"` line down to the end of the `WATCH`/headless `CMD=(...)` if/else) with:

```bash
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

# Preflight: harness binary on PATH + required secrets present.
command -v "${ARGV[0]}" >/dev/null || { echo "harness '$HARNESS' binary '${ARGV[0]}' not found on PATH" >&2; exit 1; }
if [ -s "$SESSION_DIR/launch.required-env" ]; then
  while IFS= read -r var; do
    [ -z "$var" ] && continue
    if [ -z "${!var:-}" ]; then echo "harness '$HARNESS' requires \$$var to be set" >&2; exit 1; fi
  done < "$SESSION_DIR/launch.required-env"
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
```

Also **delete** the now-unused earlier `SANDBOX_PROFILE` heredoc block and the `command -v sandbox-exec …` hard-error (sandbox-prepare now owns sandbox selection). Keep `command -v sandbox-exec` logic out of run.sh entirely.

- [ ] **Step 4: Replace the DRY_RUN print and the exec/headless block**

Replace the `if [ "${DRY_RUN:-0}" = "1" ]` block with:

```bash
if [ "${DRY_RUN:-0}" = "1" ]; then
  printf '%q ' "${CMD[@]}"; echo
  echo "--- harness: $HARNESS / sandbox: $SANDBOX_BACKEND ---" >&2
  echo "--- launch.env ---" >&2; tr '\0' '\n' < "$SESSION_DIR/launch.env" >&2
  for f in "$SESSION_DIR"/mcp.json "$SESSION_DIR"/codex/config.toml "$SESSION_DIR"/gemini/.gemini/settings.json "$SESSION_DIR"/opencode.json; do
    [ -f "$f" ] && { echo "--- $f ---" >&2; cat "$f" >&2; }
  done
  exit 0
fi
```

Replace the `if [ "$WATCH" -eq 1 ]; then … else … fi` exec block (the one that runs `CMD` and tees to `transcript.jsonl`) with the single headless path:

```bash
# `|| true`: when the broker hits the wall-clock cap it closes the MCP
# connection, so the harness exits non-zero — expected, not a failure.
(cd "$WORKSPACE" && "${CMD[@]}") | tee "$OUT_DIR/transcript.jsonl" | "$REPO_BIN" format-stream --harness "$HARNESS" | tee "$OUT_DIR/run.log" || true
```

- [ ] **Step 5: Update the render-transcript call + harness recording**

Replace the existing `render-transcript` invocation (inside the `if [ "$WATCH" -ne 1 ]` guard — remove the guard) with:

```bash
"$REPO_BIN" render-transcript --input "$OUT_DIR/transcript.jsonl" --out "$OUT_DIR/transcript.md" --harness "$HARNESS" || true
```

Where `model.txt` is written, also write `harness.txt`. Find the `MODEL_ARGS` recording (the `printf '%s\n' "$MODEL" > "$OUT_DIR/model.txt"`) — that block was deleted in Step 3, so add near the top after `mkdir -p "$OUT_DIR"`:

```bash
printf '%s\n' "$HARNESS" > "$OUT_DIR/harness.txt"
[ -n "$MODEL" ] && printf '%s\n' "$MODEL" > "$OUT_DIR/model.txt"
```

- [ ] **Step 6: DRY_RUN smoke-test for claude**

Run:

```bash
DRY_RUN=1 ./benchmark/run.sh --map gridlock-v1
```

Expected: prints a `sandbox-exec -f … claude -p … --output-format stream-json` command, `harness: claude / sandbox: seatbelt`, and the `mcp.json` contents. No errors.

- [ ] **Step 7: Commit**

```bash
git add benchmark/run.sh
git commit -m "feat(benchmark): run.sh uses harness-prepare/sandbox-prepare; drop watch + caffeinate"
```

## Task 9: Docs — `--harness` and dropped `--watch`/`caffeinate`

**Files:**
- Modify: `README.md`, `benchmark/README.md`

- [ ] **Step 1: Update `benchmark/README.md`**

In `benchmark/README.md`, change the run step. Replace the `--watch` bullet and `--model` bullet with:

```markdown
3. Run: `./benchmark/run.sh --map gridlock-v1`
   - Use `--harness <claude|codex|gemini|opencode>` to pick the agent harness
     (default `claude`). codex needs `OPENAI_API_KEY`, gemini `GEMINI_API_KEY`,
     opencode `OPENROUTER_API_KEY`; each must be on `PATH`.
   - Use `--model <name>` to pick the model (e.g. `claude-opus-4-8`,
     `gpt-5.5`, `gemini-2.5-pro`, `openrouter/qwen/qwen-2.5-coder-32b-instruct`).
     The harness + model are recorded in the run dir as `harness.txt` / `model.txt`.
   - The deny-repo-read sandbox (macOS Seatbelt, Linux bubblewrap/firejail)
     wraps the agent; the active backend is recorded in `sandbox.txt`. On a host
     with no sandbox available the run proceeds with a loud warning and
     `sandbox.txt = none`.
```

Remove any remaining `--watch` reference in this file.

- [ ] **Step 2: Update root `README.md`**

In `README.md`, in the "Running a benchmark" section step 3, replace the `--watch` parenthetical:

```markdown
3. **Run:** `./benchmark/run.sh --map gridlock-v1` (add `--harness codex`
   etc. to run a non-Claude agent).
```

- [ ] **Step 3: Commit**

```bash
git add README.md benchmark/README.md
git commit -m "docs: document --harness, remove --watch references"
```

---

# Phase 2 — Codex

## Task 10: Codex LaunchSpec builder

**Files:**
- Create: `broker/src/benchmark/harness/codex.rs`
- Modify: `broker/src/benchmark/harness/mod.rs` (`mod codex;`, replace `todo!()` arm)

- [ ] **Step 1: Write the failing test + builder**

Create `broker/src/benchmark/harness/codex.rs`:

```rust
use super::{ConfigFile, LaunchInputs, LaunchSpec};

pub fn spec(inputs: &LaunchInputs) -> LaunchSpec {
    let config_path = inputs.session_dir.join("codex").join("config.toml");
    // sh -c "<mcp_shell>" as the stdio MCP server.
    let contents = format!(
        "[mcp_servers.skylinebench]\ncommand = \"sh\"\nargs = [\"-c\", {}]\n",
        toml_string(&inputs.mcp_shell),
    );

    let model_args = inputs.model.iter().flat_map(|m| ["-m".to_string(), m.clone()]);
    let head = ["codex", "exec", inputs.prompt.as_str()].map(String::from);
    let tail = ["-a", "never", "-s", "workspace-write", "--json"].map(String::from);
    let argv: Vec<String> = head.into_iter().chain(model_args).chain(tail).collect();

    LaunchSpec {
        argv,
        env: vec![(
            "CODEX_HOME".to_string(),
            inputs.session_dir.join("codex").display().to_string(),
        )],
        config_files: vec![ConfigFile { path: config_path, contents }],
        required_env: vec!["OPENAI_API_KEY".to_string()],
    }
}

/// Minimal TOML basic-string encoder (escape backslash and quote).
fn toml_string(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn inputs() -> LaunchInputs {
        LaunchInputs {
            model: Some("gpt-5.5".to_string()),
            prompt: "improve traffic".to_string(),
            mcp_shell: "/s/skylinebench benchmark --map m".to_string(),
            session_dir: PathBuf::from("/tmp/sess"),
        }
    }

    #[test]
    fn builds_codex_exec_argv() {
        let spec = spec(&inputs());
        assert_eq!(&spec.argv[0..2], &["codex", "exec"]);
        assert!(spec.argv.contains(&"improve traffic".to_string()));
        assert!(spec.argv.windows(2).any(|w| w == ["-m", "gpt-5.5"]));
        assert!(spec.argv.contains(&"--json".to_string()));
        assert!(spec.argv.windows(2).any(|w| w == ["-a", "never"]));
        assert_eq!(spec.required_env, vec!["OPENAI_API_KEY".to_string()]);
    }

    #[test]
    fn isolates_codex_home_and_writes_config_toml() {
        let spec = spec(&inputs());
        assert!(spec.env.iter().any(|(k, v)| k == "CODEX_HOME" && v.ends_with("codex")));
        let cf = &spec.config_files[0];
        assert!(cf.path.ends_with("config.toml"));
        assert!(cf.contents.contains("[mcp_servers.skylinebench]"));
        assert!(cf.contents.contains("benchmark --map m"));
    }
}
```

In `broker/src/benchmark/harness/mod.rs`: add `mod codex;` next to `mod claude;`, and replace the `Harness::Codex => todo!(...)` arm with `Harness::Codex => codex::spec(inputs),`.

- [ ] **Step 2: Run the tests**

Run: `cargo test --manifest-path broker/Cargo.toml harness::codex`
Expected: PASS (2 tests).

- [ ] **Step 3: Commit**

```bash
git add broker/src/benchmark/harness/
git commit -m "feat(broker): codex LaunchSpec builder"
```

## Task 11: Codex transcript parser

**Files:**
- Modify: `broker/src/benchmark/transcript.rs` (replace `parse_codex` stub + add tests)

- [ ] **Step 1: Replace the `parse_codex` stub**

In `broker/src/benchmark/transcript.rs`, replace `fn parse_codex(_v: &Value) -> Vec<Event> { vec![] }` with:

```rust
/// Codex `--json` item stream. We render on `item.completed` only (item.started
/// /updated would duplicate). Defensive about `type`/`item_type` and message
/// type spelling, which drift across codex versions.
fn parse_codex(v: &Value) -> Vec<Event> {
    let event_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
    if event_type != "item.completed" {
        return vec![];
    }
    let item = match v.get("item") {
        Some(i) => i,
        None => return vec![],
    };
    let item_type = item
        .get("type")
        .or_else(|| item.get("item_type"))
        .and_then(|t| t.as_str())
        .unwrap_or("");
    let text = |key: &str| item.get(key).and_then(|t| t.as_str()).map(String::from);

    match item_type {
        "agent_message" | "assistant_message" => {
            text("text").map(|t| vec![Event::Assistant(vec![Block::Text(t)])]).unwrap_or_default()
        }
        "reasoning" => {
            text("text").map(|t| vec![Event::Assistant(vec![Block::Thinking(t)])]).unwrap_or_default()
        }
        "mcp_tool_call" => {
            let name = item.get("tool").and_then(|t| t.as_str()).unwrap_or("tool").to_string();
            let input = item.get("arguments").cloned().unwrap_or(Value::Null);
            let mut events = vec![Event::Assistant(vec![Block::ToolUse { name, input }])];
            if let Some(result) = item.get("result") {
                events.push(Event::Results(vec![Block::ToolResult { parts: result_parts(result) }]));
            }
            events
        }
        "command_execution" => {
            let command = item.get("command").and_then(|c| c.as_str()).unwrap_or("").to_string();
            let input = serde_json::json!({ "command": command });
            let mut events = vec![Event::Assistant(vec![Block::ToolUse { name: "bash".to_string(), input }])];
            if let Some(out) = item.get("aggregated_output").and_then(|o| o.as_str()) {
                events.push(Event::Results(vec![Block::ToolResult { parts: vec![out.to_string()] }]));
            }
            events
        }
        _ => vec![],
    }
}

/// Extract readable text parts from an MCP tool result value: prefer a
/// `content` array of `{text}` (the MCP shape) so live progress parsing works;
/// otherwise fall back to pretty JSON.
fn result_parts(result: &Value) -> Vec<String> {
    if let Some(arr) = result.get("content").and_then(|c| c.as_array()) {
        let texts: Vec<String> = arr
            .iter()
            .filter_map(|c| c.get("text").and_then(|t| t.as_str()).map(String::from))
            .collect();
        if !texts.is_empty() {
            return texts;
        }
    }
    vec![serde_json::to_string_pretty(result).unwrap_or_else(|_| result.to_string())]
}
```

- [ ] **Step 2: Add codex parser tests**

In the `#[cfg(test)] mod tests` block of `transcript.rs`, add:

```rust
    #[test]
    fn codex_renders_message_reasoning_and_tool_call() {
        let jsonl = concat!(
            r#"{"type":"item.completed","item":{"type":"reasoning","text":"Plan the bypass."}}"#,
            "\n",
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"Building it."}}"#,
            "\n",
            r#"{"type":"item.completed","item":{"type":"mcp_tool_call","tool":"build_road","arguments":{"road_type":"Highway"},"result":{"content":[{"text":"{\"ok\":true}"}]}}}"#,
            "\n",
        );
        let md = render_transcript(Harness::Codex, jsonl);
        assert!(md.contains("Plan the bypass."), "reasoning: {md}");
        assert!(md.contains("Building it."), "message: {md}");
        assert!(md.contains("build_road"), "tool: {md}");
        assert!(md.contains("Highway"), "args: {md}");
        assert!(md.contains("ok"), "result: {md}");
    }

    #[test]
    fn codex_ignores_started_items() {
        let line: Value = serde_json::from_str(
            r#"{"type":"item.started","item":{"type":"mcp_tool_call","tool":"build_road"}}"#,
        )
        .unwrap();
        assert!(format_event_live(Harness::Codex, &line).is_none());
    }
```

- [ ] **Step 3: Run the tests**

Run: `cargo test --manifest-path broker/Cargo.toml transcript`
Expected: PASS (existing + 2 new codex tests).

- [ ] **Step 4: Commit**

```bash
git add broker/src/benchmark/transcript.rs
git commit -m "feat(broker): codex transcript parser"
```

---

# Phase 3 — Gemini

## Task 12: Gemini LaunchSpec builder

**Files:**
- Create: `broker/src/benchmark/harness/gemini.rs`
- Modify: `broker/src/benchmark/harness/mod.rs` (`mod gemini;`, replace `todo!()` arm)

- [ ] **Step 1: Write the failing test + builder**

Create `broker/src/benchmark/harness/gemini.rs`:

```rust
use super::{ConfigFile, LaunchInputs, LaunchSpec};

pub fn spec(inputs: &LaunchInputs) -> LaunchSpec {
    let settings_path = inputs.session_dir.join("gemini").join(".gemini").join("settings.json");
    let contents = serde_json::json!({
        "mcpServers": {
            "skylinebench": {
                "command": "sh",
                "args": ["-c", inputs.mcp_shell],
                "excludeTools": ["web_fetch", "google_web_search"]
            }
        }
    });

    let model_args = inputs.model.iter().flat_map(|m| ["-m".to_string(), m.clone()]);
    let head = ["gemini", "-p", inputs.prompt.as_str()].map(String::from);
    let tail = ["--approval-mode", "yolo", "--output-format", "stream-json"].map(String::from);
    let argv: Vec<String> = head.into_iter().chain(model_args).chain(tail).collect();

    LaunchSpec {
        argv,
        env: vec![
            ("GEMINI_CLI_HOME".to_string(), inputs.session_dir.join("gemini").display().to_string()),
            ("GEMINI_CLI_TRUST_WORKSPACE".to_string(), "true".to_string()),
        ],
        config_files: vec![ConfigFile {
            path: settings_path,
            contents: serde_json::to_string_pretty(&contents)
                .expect("serde_json::Value is always serializable"),
        }],
        required_env: vec!["GEMINI_API_KEY".to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn inputs() -> LaunchInputs {
        LaunchInputs {
            model: Some("gemini-2.5-pro".to_string()),
            prompt: "improve traffic".to_string(),
            mcp_shell: "/s/skylinebench benchmark --map m".to_string(),
            session_dir: PathBuf::from("/tmp/sess"),
        }
    }

    #[test]
    fn builds_gemini_argv() {
        let spec = spec(&inputs());
        assert_eq!(spec.argv[0], "gemini");
        assert!(spec.argv.windows(2).any(|w| w == ["-p", "improve traffic"]));
        assert!(spec.argv.windows(2).any(|w| w == ["-m", "gemini-2.5-pro"]));
        assert!(spec.argv.windows(2).any(|w| w == ["--approval-mode", "yolo"]));
        assert!(spec.argv.contains(&"stream-json".to_string()));
        assert_eq!(spec.required_env, vec!["GEMINI_API_KEY".to_string()]);
    }

    #[test]
    fn isolates_home_and_writes_settings() {
        let spec = spec(&inputs());
        assert!(spec.env.iter().any(|(k, _)| k == "GEMINI_CLI_HOME"));
        assert!(spec.env.iter().any(|(k, v)| k == "GEMINI_CLI_TRUST_WORKSPACE" && v == "true"));
        let cf = &spec.config_files[0];
        assert!(cf.path.ends_with("settings.json"));
        assert!(cf.contents.contains("mcpServers"));
        assert!(cf.contents.contains("excludeTools"));
    }
}
```

In `broker/src/benchmark/harness/mod.rs`: add `mod gemini;`, replace the `Harness::Gemini => todo!(...)` arm with `Harness::Gemini => gemini::spec(inputs),`.

- [ ] **Step 2: Run the tests**

Run: `cargo test --manifest-path broker/Cargo.toml harness::gemini`
Expected: PASS (2 tests).

- [ ] **Step 3: Commit**

```bash
git add broker/src/benchmark/harness/
git commit -m "feat(broker): gemini LaunchSpec builder"
```

## Task 13: Gemini transcript parser

**Files:**
- Modify: `broker/src/benchmark/transcript.rs` (replace `parse_gemini` stub + tests)

- [ ] **Step 1: Replace the `parse_gemini` stub**

Replace `fn parse_gemini(_v: &Value) -> Vec<Event> { vec![] }` with:

```rust
/// Gemini `--output-format stream-json` events. No reasoning stream exists, so
/// there are no Thinking blocks (a harness limitation, by design).
fn parse_gemini(v: &Value) -> Vec<Event> {
    match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
        "init" => vec![Event::SessionStart],
        "message" if v.get("role").and_then(|r| r.as_str()) == Some("assistant") => v
            .get("content")
            .and_then(|c| c.as_str())
            .filter(|t| !t.is_empty())
            .map(|t| vec![Event::Assistant(vec![Block::Text(t.to_string())])])
            .unwrap_or_default(),
        "tool_use" => {
            let name = v.get("tool_name").and_then(|n| n.as_str()).unwrap_or("tool").to_string();
            let input = v.get("parameters").cloned().unwrap_or(Value::Null);
            vec![Event::Assistant(vec![Block::ToolUse { name, input }])]
        }
        "tool_result" => {
            let part = v
                .get("output")
                .and_then(|o| o.as_str())
                .map(String::from)
                .or_else(|| v.get("error").map(|e| e.to_string()))
                .unwrap_or_default();
            vec![Event::Results(vec![Block::ToolResult { parts: vec![part] }])]
        }
        _ => vec![],
    }
}
```

- [ ] **Step 2: Add gemini parser tests**

Add to the test module:

```rust
    #[test]
    fn gemini_renders_message_and_tool_call_no_thinking() {
        let jsonl = concat!(
            r#"{"type":"init","session_id":"s1"}"#,
            "\n",
            r#"{"type":"message","role":"assistant","content":"Adding a bypass."}"#,
            "\n",
            r#"{"type":"tool_use","tool_name":"build_road","tool_id":"t1","parameters":{"road_type":"Highway"}}"#,
            "\n",
            r#"{"type":"tool_result","tool_id":"t1","status":"success","output":"{\"ok\":true}"}"#,
            "\n",
        );
        let md = render_transcript(Harness::Gemini, jsonl);
        assert!(md.contains("Adding a bypass."), "message: {md}");
        assert!(md.contains("build_road"), "tool: {md}");
        assert!(md.contains("Highway"), "args: {md}");
        assert!(md.contains("ok"), "result: {md}");
        assert!(!md.contains("Thinking"), "gemini has no reasoning: {md}");
    }

    #[test]
    fn gemini_init_is_session_start_live() {
        let line: Value = serde_json::from_str(r#"{"type":"init","session_id":"s1"}"#).unwrap();
        assert_eq!(format_event_live(Harness::Gemini, &line).as_deref(), Some("● session started"));
    }
```

- [ ] **Step 3: Run the tests**

Run: `cargo test --manifest-path broker/Cargo.toml transcript`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add broker/src/benchmark/transcript.rs
git commit -m "feat(broker): gemini transcript parser"
```

---

# Phase 4 — opencode (OpenRouter)

## Task 14: opencode LaunchSpec builder

**Files:**
- Create: `broker/src/benchmark/harness/opencode.rs`
- Modify: `broker/src/benchmark/harness/mod.rs` (`mod opencode;`, replace `todo!()` arm)

- [ ] **Step 1: Write the failing test + builder**

Create `broker/src/benchmark/harness/opencode.rs`:

```rust
use super::{ConfigFile, LaunchInputs, LaunchSpec};

pub fn spec(inputs: &LaunchInputs) -> LaunchSpec {
    let config_path = inputs.session_dir.join("opencode.json");
    let contents = serde_json::json!({
        "$schema": "https://opencode.ai/config.json",
        "permission": "allow",
        "tools": { "webfetch": false, "websearch": false },
        "mcp": {
            "skylinebench": {
                "type": "local",
                "command": ["sh", "-c", inputs.mcp_shell],
                "enabled": true
            }
        }
    });

    let model_args = inputs.model.iter().flat_map(|m| ["-m".to_string(), m.clone()]);
    let head = ["opencode", "run", inputs.prompt.as_str()].map(String::from);
    let tail = ["--format", "json", "--dangerously-skip-permissions"].map(String::from);
    let argv: Vec<String> = head.into_iter().chain(model_args).chain(tail).collect();

    LaunchSpec {
        argv,
        env: vec![("OPENCODE_CONFIG".to_string(), config_path.display().to_string())],
        config_files: vec![ConfigFile {
            path: config_path,
            contents: serde_json::to_string_pretty(&contents)
                .expect("serde_json::Value is always serializable"),
        }],
        required_env: vec!["OPENROUTER_API_KEY".to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn inputs() -> LaunchInputs {
        LaunchInputs {
            model: Some("openrouter/qwen/qwen-2.5-coder-32b-instruct".to_string()),
            prompt: "improve traffic".to_string(),
            mcp_shell: "/s/skylinebench benchmark --map m".to_string(),
            session_dir: PathBuf::from("/tmp/sess"),
        }
    }

    #[test]
    fn builds_opencode_run_argv() {
        let spec = spec(&inputs());
        assert_eq!(&spec.argv[0..2], &["opencode", "run"]);
        assert!(spec.argv.windows(2).any(|w| w == ["-m", "openrouter/qwen/qwen-2.5-coder-32b-instruct"]));
        assert!(spec.argv.windows(2).any(|w| w == ["--format", "json"]));
        assert!(spec.argv.contains(&"--dangerously-skip-permissions".to_string()));
        assert_eq!(spec.required_env, vec!["OPENROUTER_API_KEY".to_string()]);
    }

    #[test]
    fn writes_config_with_mcp_and_permission_allow() {
        let spec = spec(&inputs());
        assert!(spec.env.iter().any(|(k, _)| k == "OPENCODE_CONFIG"));
        let cf = &spec.config_files[0];
        assert!(cf.path.ends_with("opencode.json"));
        assert!(cf.contents.contains("\"type\": \"local\""));
        assert!(cf.contents.contains("\"permission\": \"allow\""));
        assert!(cf.contents.contains("benchmark --map m"));
    }
}
```

In `broker/src/benchmark/harness/mod.rs`: add `mod opencode;`, replace the `Harness::Opencode => todo!(...)` arm with `Harness::Opencode => opencode::spec(inputs),`.

- [ ] **Step 2: Run the tests**

Run: `cargo test --manifest-path broker/Cargo.toml harness::opencode`
Expected: PASS (2 tests).

- [ ] **Step 3: Commit**

```bash
git add broker/src/benchmark/harness/
git commit -m "feat(broker): opencode LaunchSpec builder"
```

## Task 15: opencode transcript parser

**Files:**
- Modify: `broker/src/benchmark/transcript.rs` (replace `parse_opencode` stub + tests)

- [ ] **Step 1: Replace the `parse_opencode` stub**

Replace `fn parse_opencode(_v: &Value) -> Vec<Event> { vec![] }` with:

```rust
/// opencode `run --format json` JSONL parts. Schema is version-sensitive
/// (end-of-stream bug #26855), so be defensive and skip unknown parts.
fn parse_opencode(v: &Value) -> Vec<Event> {
    match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
        "text" => v
            .get("part")
            .and_then(|p| p.get("text"))
            .or_else(|| v.get("text"))
            .and_then(|t| t.as_str())
            .filter(|t| !t.is_empty())
            .map(|t| vec![Event::Assistant(vec![Block::Text(t.to_string())])])
            .unwrap_or_default(),
        "tool_use" => {
            let part = v.get("part").unwrap_or(v);
            let name = part
                .get("tool")
                .or_else(|| part.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("tool")
                .to_string();
            let state = part.get("state");
            let input = state
                .and_then(|s| s.get("input"))
                .cloned()
                .unwrap_or(Value::Null);
            let mut events = vec![Event::Assistant(vec![Block::ToolUse { name, input }])];
            if let Some(output) = state.and_then(|s| s.get("output")) {
                events.push(Event::Results(vec![Block::ToolResult { parts: vec![value_to_text(output)] }]));
            }
            events
        }
        "step_finish" if v.get("reason").and_then(|r| r.as_str()) == Some("stop") => {
            vec![Event::Done("complete".to_string())]
        }
        _ => vec![],
    }
}
```

- [ ] **Step 2: Add opencode parser tests**

Add to the test module:

```rust
    #[test]
    fn opencode_renders_text_and_tool_use() {
        let jsonl = concat!(
            r#"{"type":"text","part":{"text":"Adding a bypass."}}"#,
            "\n",
            r#"{"type":"tool_use","part":{"tool":"build_road","state":{"input":{"road_type":"Highway"},"output":"{\"ok\":true}","status":"completed"}}}"#,
            "\n",
            r#"{"type":"step_finish","reason":"stop"}"#,
            "\n",
        );
        let md = render_transcript(Harness::Opencode, jsonl);
        assert!(md.contains("Adding a bypass."), "text: {md}");
        assert!(md.contains("build_road"), "tool: {md}");
        assert!(md.contains("Highway"), "args: {md}");
        assert!(md.contains("ok"), "result: {md}");
    }

    #[test]
    fn opencode_step_finish_stop_is_done_live() {
        let line: Value = serde_json::from_str(r#"{"type":"step_finish","reason":"stop"}"#).unwrap();
        assert_eq!(format_event_live(Harness::Opencode, &line).as_deref(), Some("● done: complete"));
    }
```

- [ ] **Step 3: Run the full test suite**

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: PASS (all unit + integration tests, including `tests/broker_e2e.rs`).

- [ ] **Step 4: Commit**

```bash
git add broker/src/benchmark/transcript.rs
git commit -m "feat(broker): opencode transcript parser"
```

## Task 16: Cross-harness DRY_RUN verification

**Files:** none (verification only)

- [ ] **Step 1: Build release**

Run: `cargo build --release --manifest-path broker/Cargo.toml`
Expected: clean build.

- [ ] **Step 2: DRY_RUN each non-claude harness**

For each harness, confirm the argv, config, and isolation env look right (secrets need not be set under DRY_RUN — the preflight only runs in real mode):

```bash
DRY_RUN=1 ./benchmark/run.sh --map gridlock-v1 --harness codex --model gpt-5.5
DRY_RUN=1 ./benchmark/run.sh --map gridlock-v1 --harness gemini --model gemini-2.5-pro
DRY_RUN=1 ./benchmark/run.sh --map gridlock-v1 --harness opencode --model openrouter/qwen/qwen-2.5-coder-32b-instruct
```

Expected for each: the printed `CMD` starts with the sandbox wrapper then the right harness binary + flags; the dumped config file matches the harness (`config.toml` for codex, `settings.json` for gemini, `opencode.json` for opencode); `launch.env` shows the right isolation var (`CODEX_HOME` / `GEMINI_CLI_HOME` / `OPENCODE_CONFIG`).

- [ ] **Step 3: Confirm an unknown harness errors**

Run: `DRY_RUN=1 ./benchmark/run.sh --map gridlock-v1 --harness bogus`
Expected: `unknown harness: bogus` (non-zero exit) from `harness-prepare`.

- [ ] **Step 4: Commit (if any doc tweaks were needed)**

No code changes expected. If DRY_RUN surfaced a bug, fix it in the relevant task's file and commit with a `fix(broker):` message.

---

## Notes for the implementer

- **Verify harness CLI versions before a real run.** The codex `--json` schema, gemini `stream-json` event names, and opencode `--format json` parts are version-sensitive (see the spec's "Parity limits"). The parsers skip unknown lines, so a schema drift degrades the transcript but never fails the run. If a real run produces an empty/garbled transcript, capture a few raw lines from `transcript.jsonl` and adjust the relevant `parse_*` function + its test.
- **The MCP tool surface.** The broker exposes only the benchmark tools; each harness also has built-ins. Web tools are disabled per harness where a config exists (claude `--disallowedTools`, gemini `excludeTools`, opencode `tools`). The sandbox is the hard anti-cheat guarantee regardless.
- **No new `cargo` deps.** Everything uses `serde_json` and std, already in `broker/Cargo.toml`.
```
