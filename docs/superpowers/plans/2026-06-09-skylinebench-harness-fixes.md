# SkylineBench Harness Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the four issues observed in run `20260609-191326`: the agent could read the benchmark source, `submit_solution` hung until the MCP client timeout, `control_time` had no step default/cap, and runs ended messily.

**Architecture:** The benchmark broker stops finalizing inside the MCP server's lifetime. On run end it snapshots `RunState` to `end-state.json`; `run.sh` then runs a new `benchmark-finalize` subcommand *after* `claude` exits, which performs settle + final measurement and writes `run-record.json`/`score.json`. The agent keeps Bash/Write/Read (it may write and run scratch code to analyze MCP results) but is blocked from reading the benchmark repo at the OS level: `claude` is wrapped in macOS `sandbox-exec` with a profile that denies `file-read*` on the repo subpath. Everything the sandboxed agent needs (broker binary copy, mcp.json, scratch workspace) lives in a per-run temp dir outside the repo. Permission deny rules / `--disallowedTools` alone cannot do this — Bash subprocesses (`cat`, `python open()`) bypass them (per the official permissions docs), hence Seatbelt. `control_time` steps default to 1 in-game calendar day (585 ticks ≈ 4096-frame game week ÷ 7) and are capped at 3 days (1755 ticks), enforced broker-side.

**Tech Stack:** Rust (tokio, rmcp, serde, clap) in `broker/`; bash (`benchmark/run.sh`); prompt markdown.

**Context for the implementer:**
- All broker work happens in `/Users/isaac.povey/Documents/personal/SkylineBench/broker/`. Run tests with `cargo test` from that directory.
- The mock mod (`broker/src/mock.rs`) implements `/clock`: `op:"step"` adds `ticks` to its tick counter and returns it — tests use this to observe what tick count the server actually requested.
- The repo is on branch `fix-lazy-baseline` with uncommitted changes in the same files this plan touches; Task 0 commits them first.
- Decision already made with the user: "day" = the CS1 *calendar* day (~585 ticks; the game week is 4096 frames), NOT the 65536-frame day/night cycle. Finalize-after-claude-exits was chosen over synchronous finalize inside `submit_solution`.

---

### Task 0: Commit the in-flight lazy-baseline work

**Files:**
- Modify: none (commit only)

- [ ] **Step 1: Verify the working tree builds and tests pass**

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: all tests PASS (if not, stop and report — do not proceed on a red tree).

- [ ] **Step 2: Commit**

```bash
git add broker/src/benchmark/measure.rs broker/src/benchmark/server.rs broker/src/benchmark/state.rs broker/src/main.rs
git commit -m "fix(broker): measure baseline lazily on first tool call"
```

---

### Task 1: Day-tick constants in BenchConfig

**Files:**
- Modify: `broker/src/benchmark/config.rs`

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `broker/src/benchmark/config.rs`:

```rust
#[test]
fn day_ticks_default_to_one_calendar_day() {
    let c = BenchConfig::default();
    // One CS1 calendar day ≈ 585 ticks (the game week is 4096 frames).
    assert_eq!(c.day_ticks, 585);
    assert_eq!(c.max_step_days, 3);
    assert_eq!(c.max_step_ticks(), 1755);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path broker/Cargo.toml day_ticks_default -- --nocapture`
Expected: FAIL to compile ("no field `day_ticks`").

- [ ] **Step 3: Implement**

In `BenchConfig` (after `cost_base_length_m`):

```rust
    /// Ticks in one in-game calendar day (the CS1 game week is 4096 sim
    /// frames, so a day is 4096/7 ≈ 585). `control_time` steps default to
    /// one day and are capped at `max_step_days`.
    pub day_ticks: u32,
    pub max_step_days: u32,
```

In `Default::default()` add:

```rust
            day_ticks: 585,
            max_step_days: 3,
```

Add an impl method on `BenchConfig` (outside `Default`):

```rust
impl BenchConfig {
    pub fn max_step_ticks(&self) -> u32 {
        self.day_ticks * self.max_step_days
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: PASS (note: `BenchConfig` derives `Deserialize` without `#[serde(default)]`; nothing currently deserializes old configs except tests that round-trip current structs, so no migration needed).

- [ ] **Step 5: Commit**

```bash
git add broker/src/benchmark/config.rs
git commit -m "feat(broker): add day_ticks/max_step_days to BenchConfig"
```

---

### Task 2: control_time defaults to 1 day, caps at 3 days

**Files:**
- Modify: `broker/src/benchmark/server.rs` (the `control_time` tool + tests)

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `broker/src/benchmark/server.rs`:

```rust
    async fn bench_with_mock() -> BenchmarkServer {
        use crate::benchmark::config::BenchConfig;
        use crate::benchmark::state::RunState;
        use crate::bridge_client::BridgeClient;
        use crate::mock;
        use std::collections::HashMap;
        use std::sync::Arc;
        use tokio::sync::Mutex;

        let (addr, server) = mock::bind("127.0.0.1:0".parse().unwrap()).await;
        tokio::spawn(server);
        let client = Arc::new(BridgeClient::new(format!("http://{addr}")));
        let mut st = RunState::new(BenchConfig::default(), HashMap::new());
        // Pre-set a baseline so ensure_baseline doesn't drive the mock clock
        // and skew the tick assertions below.
        st.baseline = Some(crate::benchmark::record::WindowStats {
            flow_mean: 50.0,
            active_vehicles_mean: 10.0,
            population: 100,
        });
        BenchmarkServer::new(client, Arc::new(Mutex::new(st)))
    }

    fn result_text(res: &CallToolResult) -> String {
        res.content
            .iter()
            .filter_map(|c| c.as_text().map(|t| t.text.clone()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[tokio::test]
    async fn step_without_ticks_defaults_to_one_day() {
        let bench = bench_with_mock().await;
        let res = bench
            .control_time(Parameters(crate::service::ControlTimeArgs {
                op: "step".into(),
                ticks: None,
                speed: None,
            }))
            .await
            .unwrap();
        let text = result_text(&res);
        // Mock starts at tick 0 and adds the requested ticks: default = 585.
        assert!(text.contains("\"tick\":585"), "got: {text}");
    }

    #[tokio::test]
    async fn step_above_three_days_is_rejected() {
        let bench = bench_with_mock().await;
        let res = bench
            .control_time(Parameters(crate::service::ControlTimeArgs {
                op: "step".into(),
                ticks: Some(4096),
                speed: None,
            }))
            .await
            .unwrap();
        assert_eq!(res.is_error, Some(true));
        let text = result_text(&res);
        assert!(text.contains("1755"), "error should state the cap, got: {text}");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path broker/Cargo.toml step_ -- --nocapture`
Expected: `step_without_ticks_defaults_to_one_day` FAILS (mock tick stays 0); `step_above_three_days_is_rejected` FAILS (no error returned).

- [ ] **Step 3: Implement**

In `broker/src/benchmark/server.rs`, replace the body of `control_time` with:

```rust
    #[tool(description = "Control simulation time: pause, resume, step, or set speed. \
        `step` defaults to 1 in-game day (585 ticks) when `ticks` is omitted; \
        the maximum step is 3 days (1755 ticks).")]
    async fn control_time(&self, Parameters(args): Parameters<ControlTimeArgs>) -> Result<CallToolResult, ErrorData> {
        self.ensure_baseline().await;
        if self.run_ended().await {
            return self.finish(serde_json::json!({ "ok": false, "run_ended": true })).await;
        }
        let (day_ticks, max_ticks) = {
            let s = self.state.lock().await;
            (s.config.day_ticks, s.config.max_step_ticks())
        };
        let args = match args.op.as_str() {
            "step" => ControlTimeArgs { ticks: Some(args.ticks.unwrap_or(day_ticks)), ..args },
            _ => args,
        };
        if args.op == "step" {
            let requested = args.ticks.unwrap_or(0);
            if requested > max_ticks {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "step of {requested} ticks exceeds the cap of {max_ticks} ticks \
                     (3 in-game days; 1 day ≈ {day_ticks} ticks). Request {max_ticks} or fewer."
                ))]));
            }
        }
        match service::control_time(&self.client, args).await {
            Ok(v) => {
                // A transient metrics fetch failure just skips this flow sample; the
                // next get_metrics/control_time call will resample. Non-fatal.
                if let Ok(m) = self.client.metrics().await {
                    self.state.lock().await.push_flow(m.traffic.flow_percent as f64);
                }
                self.finish(v).await
            }
            Err(e) => Ok(tool_err(e)),
        }
    }
```

`ControlTimeArgs { .. }` struct-update requires no `Clone`; it moves `args.op` via `..args` — note `args.op.as_str()` is matched *before* the move, so bind the op first if the borrow checker complains:

```rust
        let is_step = args.op == "step";
        let args = if is_step {
            ControlTimeArgs { ticks: Some(args.ticks.unwrap_or(day_ticks)), ..args }
        } else {
            args
        };
        if is_step {
            // ... cap check as above
        }
```

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/benchmark/server.rs
git commit -m "feat(broker): default control_time step to 1 game day, cap at 3"
```

---

### Task 3: EndState snapshot type + RunState::end_state()

**Files:**
- Modify: `broker/src/benchmark/record.rs` (add `EndState`, `EndReason::Disconnect`)
- Modify: `broker/src/benchmark/state.rs` (add `end_state()`)
- Modify: `broker/src/benchmark/mod.rs` (re-export `EndState`)

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `broker/src/benchmark/record.rs`:

```rust
    #[test]
    fn end_reason_disconnect_serializes_snake() {
        assert_eq!(serde_json::to_string(&EndReason::Disconnect).unwrap(), "\"disconnect\"");
    }

    #[test]
    fn end_state_round_trips() {
        let e = EndState {
            schema_version: 1,
            config: crate::benchmark::config::BenchConfig::default(),
            map: MapInfo { id: "gridlock-v1".into(), source: "test".into(), game_version: "1.21.1-f9".into() },
            started_at: "t0".into(),
            ended_at: "t1".into(),
            end_reason: EndReason::Submit,
            baseline: Some(WindowStats { flow_mean: 61.0, active_vehicles_mean: 6291.0, population: 102_839 }),
            baseline_flow_samples: vec![61.0, 60.8],
            tally: Tally { num_changes: 10, money_spent: 98_834 },
            actions: vec![ActionEntry { seq: 1, tool: "upgrade_road".into(), cost: 17_081 }],
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: EndState = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }
```

Append inside `mod tests` in `broker/src/benchmark/state.rs`:

```rust
    #[test]
    fn end_state_snapshots_run_and_defaults_to_disconnect() {
        use crate::benchmark::record::{EndReason, MapInfo};

        let mut s = state();
        s.record_mutation("build_road", 12_000);
        let map = MapInfo { id: "m".into(), source: "test".into(), game_version: "v".into() };
        let e = s.end_state(map, "t0".into(), "t1".into());
        assert_eq!(e.end_reason, EndReason::Disconnect);
        assert_eq!(e.tally.num_changes, 1);
        assert_eq!(e.tally.money_spent, 12_000);
        assert_eq!(e.actions.len(), 1);
        assert!(e.baseline.is_none());

        s.end_reason = Some(EndReason::Submit);
        let map = MapInfo { id: "m".into(), source: "test".into(), game_version: "v".into() };
        let e = s.end_state(map, "t0".into(), "t1".into());
        assert_eq!(e.end_reason, EndReason::Submit);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path broker/Cargo.toml end_state -- --nocapture`
Expected: FAIL to compile (`EndState` / `Disconnect` / `end_state` not found).

- [ ] **Step 3: Implement**

In `broker/src/benchmark/record.rs` add to `EndReason`:

```rust
pub enum EndReason {
    Submit,
    FlowTarget,
    Timeout,
    /// The agent session closed without submit_solution (e.g. the agent gave
    /// up or the client crashed). Scored the same as a submit.
    Disconnect,
}
```

Add after `ActionEntry`:

```rust
/// Snapshot of a finished run's state, written to `end-state.json` when the
/// agent session ends. `benchmark-finalize` reads it after the claude process
/// has exited to run the settle + final window and produce the score — the
/// slow measurement must outlive the MCP server's lifetime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EndState {
    pub schema_version: u32,
    pub config: BenchConfig,
    pub map: MapInfo,
    pub started_at: String,
    pub ended_at: String,
    pub end_reason: EndReason,
    pub baseline: Option<WindowStats>,
    pub baseline_flow_samples: Vec<f64>,
    pub tally: Tally,
    pub actions: Vec<ActionEntry>,
}
```

In `broker/src/benchmark/state.rs` add to `impl RunState` (and `use crate::benchmark::record::{ActionEntry, EndReason, EndState, MapInfo, Tally, WindowStats};` — extend the existing import):

```rust
    pub fn end_state(&self, map: MapInfo, started_at: String, ended_at: String) -> EndState {
        EndState {
            schema_version: 1,
            config: self.config.clone(),
            map,
            started_at,
            ended_at,
            end_reason: self.end_reason.unwrap_or(EndReason::Disconnect),
            baseline: self.baseline.clone(),
            baseline_flow_samples: self.baseline_flow_samples.clone(),
            tally: Tally { num_changes: self.num_changes, money_spent: self.money_spent },
            actions: self.actions.clone(),
        }
    }
```

In `broker/src/benchmark/mod.rs` extend the record re-export:

```rust
pub use record::{EndState, MapInfo, WindowStats};
```

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/benchmark/record.rs broker/src/benchmark/state.rs broker/src/benchmark/mod.rs
git commit -m "feat(broker): add EndState snapshot and Disconnect end reason"
```

---

### Task 4: finalize() takes an EndState

**Files:**
- Modify: `broker/src/benchmark/measure.rs`

- [ ] **Step 1: Rewrite the finalize test for the new signature**

Replace the `finalize_writes_record_and_score` test in `broker/src/benchmark/measure.rs` with:

```rust
    #[tokio::test]
    async fn finalize_writes_record_and_score_from_end_state() {
        use crate::benchmark::record::{ActionEntry, EndReason, EndState, MapInfo, Tally, WindowStats};

        let c = client().await;
        let end = EndState {
            schema_version: 1,
            config: BenchConfig::default(),
            map: MapInfo { id: "gridlock-v1".into(), source: "test".into(), game_version: "1.21.1-f9".into() },
            started_at: "t0".into(),
            ended_at: "t1".into(),
            end_reason: EndReason::Submit,
            baseline: Some(WindowStats { flow_mean: 80.0, active_vehicles_mean: 0.0, population: 0 }),
            baseline_flow_samples: vec![80.0],
            tally: Tally { num_changes: 2, money_spent: 5_000 },
            actions: vec![ActionEntry { seq: 1, tool: "build_road".into(), cost: 5_000 }],
        };

        let dir = std::env::temp_dir().join(format!("sb-finalize-{}", std::process::id()));
        finalize(&c, end, &dir).await.unwrap();

        let rec: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("run-record.json")).unwrap()).unwrap();
        assert_eq!(rec["end_reason"], "submit");
        assert_eq!(rec["started_at"], "t0");
        assert_eq!(rec["ended_at"], "t1");
        assert_eq!(rec["tally"]["num_changes"], 2);
        let score: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("score.json")).unwrap()).unwrap();
        assert!(score["score"].is_number());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn finalize_measures_missing_baseline() {
        use crate::benchmark::record::{EndReason, EndState, MapInfo, Tally};

        let c = client().await;
        let end = EndState {
            schema_version: 1,
            config: BenchConfig::default(),
            map: MapInfo { id: "m".into(), source: "test".into(), game_version: "v".into() },
            started_at: "t0".into(),
            ended_at: "t1".into(),
            end_reason: EndReason::Disconnect,
            baseline: None,
            baseline_flow_samples: vec![],
            tally: Tally { num_changes: 0, money_spent: 0 },
            actions: vec![],
        };

        let dir = std::env::temp_dir().join(format!("sb-finalize-nb-{}", std::process::id()));
        finalize(&c, end, &dir).await.unwrap();

        let rec: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("run-record.json")).unwrap()).unwrap();
        // Mock city flow is 100; the measured fallback baseline must be present.
        assert_eq!(rec["baseline"]["flow_mean"], 100.0);
        assert_eq!(rec["end_reason"], "disconnect");
        std::fs::remove_dir_all(&dir).ok();
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path broker/Cargo.toml finalize -- --nocapture`
Expected: FAIL to compile (signature mismatch).

- [ ] **Step 3: Implement**

Replace `finalize` in `broker/src/benchmark/measure.rs` with:

```rust
/// Settle the sim, measure the final window, compute the score, and write
/// `run-record.json` + `score.json` into `out_dir` (spec §3 steps 6–8, §10).
/// Runs from an `EndState` snapshot so it can execute in a separate process
/// after the agent session (and the MCP server) has exited.
pub async fn finalize(client: &BridgeClient, end: EndState, out_dir: &Path) -> anyhow::Result<()> {
    let cfg = end.config.clone();

    // Baseline is normally captured on the agent's first tool call. If the run
    // ended with no tool calls at all, fall back to measuring it now (the city
    // is then still untouched, so it's a valid baseline).
    let (baseline, baseline_flow_samples) = match end.baseline {
        Some(b) => (b, end.baseline_flow_samples),
        None => {
            let m = measure_window(client, &cfg).await?;
            (m.stats, m.samples)
        }
    };

    let settle_cfg = BenchConfig { window_ticks: cfg.settle_ticks, window_samples: 1, ..cfg.clone() };
    let _ = measure_window(client, &settle_cfg).await?;
    let final_m = measure_window(client, &cfg).await?;

    let record = RunRecord {
        schema_version: 1,
        config: cfg.clone(),
        map: end.map,
        started_at: end.started_at,
        ended_at: end.ended_at,
        end_reason: end.end_reason,
        baseline,
        final_stats: final_m.stats,
        flow_samples: FlowSamples { baseline: baseline_flow_samples, final_samples: final_m.samples },
        tally: end.tally,
        actions: end.actions,
    };
    let score = score_run(&record, &cfg);

    // Blocking I/O is acceptable here — finalize runs once at end of run.
    std::fs::create_dir_all(out_dir)?;
    std::fs::write(out_dir.join("run-record.json"), serde_json::to_string_pretty(&record)?)?;
    std::fs::write(out_dir.join("score.json"), serde_json::to_string_pretty(&score)?)?;
    Ok(())
}
```

Update the imports at the top of `measure.rs`:

```rust
use std::path::Path;

use crate::benchmark::config::BenchConfig;
use crate::benchmark::record::{EndState, FlowSamples, RunRecord, WindowStats};
use crate::benchmark::score::score_run;
use crate::bridge_client::{BridgeClient, BridgeError};
```

(`std::sync::Arc`, `tokio::sync::Mutex`, `RunState`, `EndReason`, `MapInfo`, `Tally` are no longer used here — remove them.)

- [ ] **Step 4: Run tests — expect main.rs to break**

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: `measure.rs` tests pass but `main.rs` FAILS to compile (the watcher still calls the old `finalize` signature). That's fixed in Task 5 — if you must keep the tree green per-commit, do Tasks 4 and 5 as one commit (see Task 5 Step 5).

---

### Task 5: main.rs — write end-state.json, never finalize in-server; add `benchmark-finalize`

**Files:**
- Modify: `broker/src/main.rs`

- [ ] **Step 1: Replace the `Benchmark` arm's watcher + add subcommand**

In `enum Command` add:

```rust
    /// Finalize a finished benchmark run: read end-state.json from --out, run
    /// the settle + final measurement against the mod, and write
    /// run-record.json + score.json. Run this AFTER the agent session exits.
    BenchmarkFinalize {
        #[arg(long, default_value = "http://127.0.0.1:8787")]
        mod_url: String,
        #[arg(long)]
        out: std::path::PathBuf,
    },
```

Replace the whole `Command::Benchmark { .. }` match arm body with:

```rust
        Command::Benchmark { mod_url, map, map_source, out } => {
            use std::collections::HashMap;
            use std::sync::Arc;
            use tokio::sync::Mutex;
            use skylinebench::benchmark::{BenchConfig, BenchmarkServer, MapInfo, RunState};
            use skylinebench::bridge_client::BridgeClient;
            use rmcp::ServiceExt;

            fn epoch_secs() -> String {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs().to_string())
                    .unwrap_or_default()
            }

            async fn write_end_state(
                state: &Arc<Mutex<RunState>>,
                out: &std::path::Path,
                map: MapInfo,
                started_at: String,
            ) -> anyhow::Result<()> {
                let end = state.lock().await.end_state(map, started_at, epoch_secs());
                std::fs::create_dir_all(out)?;
                std::fs::write(out.join("end-state.json"), serde_json::to_string_pretty(&end)?)?;
                Ok(())
            }

            let client = Arc::new(BridgeClient::new(mod_url));
            let health = client.health().await?;
            anyhow::ensure!(health.city_loaded, "no city loaded — load the benchmark save first");
            let started_at = epoch_secs();

            let cfg = BenchConfig::default();
            let road_costs: HashMap<String, i64> = client
                .road_types()
                .await?
                .road_types
                .into_iter()
                .map(|r| (r.name, r.construction_cost))
                .collect();

            // The baseline is measured lazily on the agent's first tool call, NOT
            // here — doing it before serving would block the MCP `initialize`
            // handshake (which has its own ~60s request timeout) for the whole
            // slow window on a large city. Serve immediately instead.
            eprintln!("benchmark: serving MCP; baseline measured on first tool call…");
            let state = Arc::new(Mutex::new(RunState::new(cfg, road_costs)));
            let map_info = MapInfo {
                id: map,
                source: map_source,
                game_version: health.game_version,
            };

            // Watchdog: the wall-clock cap is the only end reason that must
            // force the process down mid-session (a submit ends the session
            // naturally — claude exits, stdin closes, and we fall through to
            // the end-state write below). Finalize (settle + measure + score)
            // happens in `benchmark-finalize`, run by run.sh AFTER claude
            // exits, so it can't be killed by client teardown or timeouts.
            let watch_state = state.clone();
            let watch_out = out.clone();
            let watch_map = map_info.clone();
            let watch_started = started_at.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let timed_out = {
                        let mut s = watch_state.lock().await;
                        s.check_timeout();
                        s.end_reason == Some(skylinebench::benchmark::record::EndReason::Timeout)
                    };
                    if timed_out {
                        let code = match write_end_state(&watch_state, &watch_out, watch_map.clone(), watch_started.clone()).await {
                            Ok(()) => {
                                eprintln!("benchmark: wall-clock cap hit; wrote end-state.json");
                                0
                            }
                            Err(e) => {
                                eprintln!("benchmark: end-state write error: {e}");
                                1
                            }
                        };
                        std::process::exit(code);
                    }
                }
            });

            let server = BenchmarkServer::new(client, state.clone())
                .serve((tokio::io::stdin(), tokio::io::stdout()))
                .await?;
            server.waiting().await?;

            // Agent session over (claude closed stdin). Snapshot the run for
            // the out-of-process finalize. end_reason None (the agent quit
            // without submitting) is recorded as `disconnect`.
            write_end_state(&state, &out, map_info, started_at).await?;
            eprintln!("benchmark: session ended; wrote end-state.json to {}", out.display());
        }
        Command::BenchmarkFinalize { mod_url, out } => {
            use skylinebench::benchmark::{finalize, EndState};
            use skylinebench::bridge_client::BridgeClient;

            let path = out.join("end-state.json");
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| anyhow::anyhow!("cannot read {}: {e} — did the benchmark session run?", path.display()))?;
            let end: EndState = serde_json::from_str(&raw)?;

            let client = BridgeClient::new(mod_url);
            let health = client.health().await?;
            anyhow::ensure!(health.city_loaded, "no city loaded — cannot run the settle/final measurement");

            eprintln!("benchmark-finalize: settle + final window (this takes several minutes)…");
            finalize(&client, end, &out).await?;
            eprintln!("benchmark-finalize: wrote run-record.json + score.json to {}", out.display());
        }
```

Note: `skylinebench::benchmark::record::EndReason` must be reachable — `record` is already `pub mod` in `benchmark/mod.rs`. `MapInfo` derives `Clone` already.

- [ ] **Step 2: Build and run the full test suite**

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: PASS (this also resolves Task 4's intentional main.rs breakage).

- [ ] **Step 3: Smoke-test against the mock**

```bash
cargo build --release --manifest-path broker/Cargo.toml
./broker/target/release/skylinebench mock --addr 127.0.0.1:18787 &
MOCK_PID=$!
sleep 1
OUT=$(mktemp -d)
# Drive a benchmark session over stdio: initialize, call submit_solution, close stdin.
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"submit_solution","arguments":{}}}' \
  | ./broker/target/release/skylinebench benchmark --map smoke --mod-url http://127.0.0.1:18787 --out "$OUT"
test -f "$OUT/end-state.json" && echo "END-STATE OK"
./broker/target/release/skylinebench benchmark-finalize --out "$OUT" --mod-url http://127.0.0.1:18787
test -f "$OUT/score.json" && echo "SCORE OK"
kill $MOCK_PID
```

Expected: the `tools/call` response returns (no hang), then `END-STATE OK` and `SCORE OK`. The end-state should show `"end_reason": "submit"` (`grep end_reason "$OUT/end-state.json"`).

- [ ] **Step 4: Commit (combined with Task 4 changes)**

```bash
git add broker/src/benchmark/measure.rs broker/src/main.rs
git commit -m "feat(broker): finalize out-of-process via end-state.json + benchmark-finalize"
```

---

### Task 6: submit_solution returns immediately

**Files:**
- Modify: `broker/src/benchmark/server.rs`

- [ ] **Step 1: Write the failing test**

Append inside `mod tests` in `broker/src/benchmark/server.rs` (reuses `bench_with_mock` from Task 2):

```rust
    #[tokio::test]
    async fn submit_returns_immediately_and_ends_run() {
        let bench = bench_with_mock().await;
        let res = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            bench.submit_solution(Parameters(SubmitArgs { note: None })),
        )
        .await
        .expect("submit_solution must return, not hang")
        .unwrap();
        let text = result_text(&res);
        assert!(text.contains("\"run_ended\":true"), "got: {text}");

        // The run is over: subsequent mutations are rejected.
        let after = bench
            .bulldoze(Parameters(crate::service::BulldozeArgs { target_type: "segment".into(), id: 0 }))
            .await
            .unwrap();
        assert!(result_text(&after).contains("\"run_ended\":true"));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path broker/Cargo.toml submit_returns -- --nocapture`
Expected: FAIL — the timeout fires ("submit_solution must return, not hang").

- [ ] **Step 3: Implement**

Replace `submit_solution` in `broker/src/benchmark/server.rs` with:

```rust
    #[tool(description = "Declare the run finished. Returns immediately; the harness settles and \
        scores the city after your session ends. Call when satisfied, then stop — further \
        modifications will be rejected.")]
    async fn submit_solution(&self, Parameters(_args): Parameters<SubmitArgs>) -> Result<CallToolResult, ErrorData> {
        // Capture the baseline if the agent submits without any prior tool call,
        // so finalize has a "before" snapshot to score against.
        self.ensure_baseline().await;
        {
            let mut s = self.state.lock().await;
            if s.end_reason.is_none() {
                s.end_reason = Some(EndReason::Submit);
            }
        }
        self.finish(serde_json::json!({
            "ok": true,
            "run_ended": true,
            "message": "Solution submitted. The run will be settled and scored after this session ends — finish your turn now.",
        }))
        .await
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/benchmark/server.rs
git commit -m "fix(broker): submit_solution returns immediately instead of hanging"
```

---

### Task 7: run.sh — Seatbelt read-isolation, session dir outside the repo, finalize after claude exits

**Files:**
- Modify: `benchmark/run.sh`

**Design (why each piece exists):**
- The agent MAY write and execute scratch code (Bash/Write/Read stay enabled) to interpret MCP results or plan actions — it just must not read the benchmark source. Tool deny rules can't enforce that against `python open()` etc., so the whole `claude` process tree is wrapped in `sandbox-exec` with `(deny file-read* (subpath <repo>))`. Seatbelt applies to child processes, so Bash subprocesses AND the spawned broker inherit it.
- Because the repo is unreadable inside the sandbox, everything claude must *read or exec* moves to a per-run `SESSION_DIR` outside the repo: a copy of the broker binary, `mcp.json`, and an empty `workspace/` cwd. Writes into the repo are still allowed (only reads are denied), so the broker can write `end-state.json` into `$OUT_DIR` and run.sh (outside the sandbox) tees artifacts there as before.
- `--strict-mcp-config` keeps the user's personal MCP servers out of the run. `WebFetch`/`WebSearch` are disallowed so the agent can't research the benchmark externally. `--permission-mode bypassPermissions` makes headless runs deterministic on machines without permissive defaults — safe because the OS sandbox is the real boundary.
- Known residual risks (accepted): the agent can read its own session transcript under `~/.claude/projects/` (its own conversation — fine, that's "interpreting MCP results"); it could `strings` the copied broker binary (no scoring source there, only compiled constants); Bash `curl` could reach the internet (prompt forbids it; the repo is private).

- [ ] **Step 1: Apply the changes**

After the `mkdir -p "$OUT_DIR"` line (`benchmark/run.sh:28`), add:

```bash
# Per-run session dir OUTSIDE the repo: the agent runs under a Seatbelt profile
# that denies reading the repo (anti-cheating — run 20260609-191326 read the
# scoring source via Bash). Everything claude must read or exec therefore
# lives here: the broker binary copy, mcp.json, and the scratch workspace.
# The agent may freely write/run code in its workspace; only repo reads die.
SESSION_DIR="$(mktemp -d "${TMPDIR:-/tmp}/skylinebench-$RUN_ID.XXXXXX")"
WORKSPACE="$SESSION_DIR/workspace"
mkdir -p "$WORKSPACE"

SANDBOX_PROFILE="$SESSION_DIR/deny-repo.sb"
cat > "$SANDBOX_PROFILE" <<SB
(version 1)
(allow default)
(deny file-read* (subpath "$ROOT"))
SB
```

Replace the `BROKER_BIN=` block (`benchmark/run.sh:30-36`) with:

```bash
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
```

After the `ALLOWED=` line, add:

```bash
DISALLOWED="WebFetch,WebSearch"
```

Replace the two `CMD=(...)` lines with (note the `sandbox-exec` wrapper):

```bash
SANDBOX=(sandbox-exec -f "$SANDBOX_PROFILE")
if [ "$WATCH" -eq 1 ]; then
  CMD=("${SANDBOX[@]}" claude --mcp-config "$MCP_CONFIG" --strict-mcp-config --allowedTools "$ALLOWED" --disallowedTools "$DISALLOWED" --permission-mode bypassPermissions "$PROMPT")
else
  CMD=("${SANDBOX[@]}" claude -p "$PROMPT" --mcp-config "$MCP_CONFIG" --strict-mcp-config --allowedTools "$ALLOWED" --disallowedTools "$DISALLOWED" --permission-mode bypassPermissions --output-format stream-json --verbose)
fi
```

Move the `MCP_CONFIG` into the session dir — change `MCP_CONFIG="$OUT_DIR/mcp.json"` to:

```bash
MCP_CONFIG="$SESSION_DIR/mcp.json"
```

and after the heredoc that writes it, add (the run artifacts should still record the exact MCP config used):

```bash
cp "$MCP_CONFIG" "$OUT_DIR/mcp.json"
```

Replace the final execution block (everything from `if [ "$WATCH" -eq 1 ]; then` near the bottom through `fi`) with:

```bash
if [ "$WATCH" -eq 1 ]; then
  (cd "$WORKSPACE" && "${CMD[@]}")
else
  # Capture the raw stream-json to transcript.jsonl unchanged, render a
  # human-readable line per event to the console, and also save that to run.log.
  # `|| true`: if the broker hits the wall-clock cap it exits and closes the MCP
  # connection, so `claude` exits non-zero — that's expected, not a failure.
  (cd "$WORKSPACE" && "${CMD[@]}") | tee "$OUT_DIR/transcript.jsonl" | "$REPO_BIN" format-stream | tee "$OUT_DIR/run.log" || true
  "$REPO_BIN" render-transcript --input "$OUT_DIR/transcript.jsonl" --out "$OUT_DIR/transcript.md"
fi

# The slow settle + final measurement runs here, outside the agent session, so
# no MCP client timeout can kill it (the old in-server finalize made
# submit_solution hang for 600s and die). Uses the repo binary — run.sh is
# not sandboxed.
echo "finalizing run (settle + final measurement, several minutes)…" >&2
"$REPO_BIN" benchmark-finalize --out "$OUT_DIR" --mod-url "$MOD_URL"

rm -rf "$SESSION_DIR"
```

- [ ] **Step 2: Verify the script**

Run: `bash -n benchmark/run.sh`
Expected: no output (syntax OK).

Run: `DRY_RUN=1 benchmark/run.sh --map smoke`
Expected: printed command starts with `sandbox-exec -f /…/deny-repo.sb claude` and includes `--strict-mcp-config`, `--permission-mode bypassPermissions`, `--disallowedTools WebFetch,WebSearch`, and the `mcp__skylinebench__*` allowlist.

- [ ] **Step 3: Verify the Seatbelt profile actually blocks repo reads but allows scratch work**

```bash
SB=$(mktemp -d)/deny.sb
cat > "$SB" <<EOF
(version 1)
(allow default)
(deny file-read* (subpath "$PWD"))
EOF
sandbox-exec -f "$SB" cat README.md 2>&1 | head -1          # repo read → must FAIL
sandbox-exec -f "$SB" python3 -c "print(open('$PWD/benchmark/prompt.md').read())" 2>&1 | head -1  # subprocess read → must FAIL
cd /tmp && sandbox-exec -f "$SB" sh -c 'echo hi > sb-test && cat sb-test && rm sb-test'  # scratch → must WORK
sandbox-exec -f "$SB" curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:8787/health  # localhost net → 200 if mock/mod running
```

Expected: both repo reads fail with "Operation not permitted", scratch echo/cat works, localhost HTTP works.

- [ ] **Step 4: Commit**

```bash
git add benchmark/run.sh
git commit -m "fix(benchmark): OS-level read isolation via sandbox-exec, finalize after claude exits"
```

---

### Task 8: prompt.md — tick semantics, MCP-only rule, no early-quit bias

**Files:**
- Modify: `benchmark/prompt.md`

- [ ] **Step 1: Rewrite the prompt**

Replace the full contents of `benchmark/prompt.md` with:

```markdown
You are competing in SkylineBench: improve the traffic in this Cities: Skylines city.

You have MCP tools to observe and modify the city:
- Observe (free, unlimited, unscored): `get_city_overview`, `observe_area`, `get_metrics`, `list_road_types`, `list_zone_types`, `render_map`.
- Modify (these are your "changes"): `build_road`, `bulldoze`, `upgrade_road`, `set_zoning`.
- Time: `control_time` (pause / resume / step / speed). Build while paused, then `step` to let traffic respond before measuring. A `step` with no `ticks` advances one in-game day (585 ticks). The maximum step is 3 days (1755 ticks) — traffic patterns repeat daily, so longer waits only burn your wall-clock budget.
- Finish: `submit_solution` when you are satisfied. It returns immediately; the city is settled and scored after your session ends, so finish your turn once it succeeds.

You may write and execute scratch code in your working directory (e.g. scripts that parse tool output or plan a batch of changes). But the city is reachable ONLY through the MCP tools, and you must not attempt to read the benchmark's implementation, its scoring code, or any files outside your working directory — runs that try are disqualified.

Goal: raise city-wide `traffic.flow_percent` (higher = freer-flowing) as much as you can.

How you are scored (you will NOT see your score during the run):
- Most of the score is how much you improve flow.
- You are rewarded for spending less money and for making fewer modifying actions. Reads are free — observe as much as you like.
- INVALID RUN: if you reduce the number of active vehicles too far (you must not "fix" traffic by depopulating the city), the run scores zero. Keep the city alive.

The run ends when any of these happens:
1. You call `submit_solution`.
2. Flow reaches the target shown as `flow_target` in `benchmark_progress`.
3. A 3-hour time limit is reached.

Every tool response includes a `benchmark_progress` block (money spent, changes made, current flow vs target, seconds remaining). Use it to pace yourself.

Work method: observe the network and metrics, find the congestion, make targeted road/zoning changes, step the simulation to let traffic respond, re-measure, and iterate. Expect some interventions to do nothing — diagnose why and try a different class of fix rather than giving up; you have hours of budget and only spend score on modifications, not attempts. Submit when you have evidence further changes won't pay for themselves.
```

- [ ] **Step 2: Commit**

```bash
git add benchmark/prompt.md
git commit -m "docs(benchmark): document tick semantics, MCP-only rule, persistence"
```

---

### Task 9: End-to-end verification

**Files:** none (verification only)

- [ ] **Step 1: Full test suite + build**

Run: `cargo test --manifest-path broker/Cargo.toml && cargo build --release --manifest-path broker/Cargo.toml`
Expected: all PASS.

- [ ] **Step 2: Full pipeline against the mock mod**

```bash
./broker/target/release/skylinebench mock --addr 127.0.0.1:18787 &
MOCK_PID=$!
sleep 1
benchmark/run.sh --map smoke --mod-url http://127.0.0.1:18787
kill $MOCK_PID
```

Expected: the agent run completes; `benchmark/runs/<id>/` contains `end-state.json`, `run-record.json`, `score.json`, `transcript.jsonl`, `run.log`. Inspect `run.log`/`transcript.md`:
- If the agent used Bash/Read at all, any attempt against repo paths failed with "Operation not permitted" — no benchmark source contents appear in the transcript (`grep -c "score_run\|w_flow" transcript.jsonl` → 0).
- `submit_solution` gets a response (`run_ended: true`) rather than timing out, and the session ends shortly after.
- Any `control_time` step over 1755 ticks (if the agent tried one) returned the cap error; steps with no `ticks` advanced 585.

- [ ] **Step 3: Report results to the user**

Summarize the run.log evidence for each of the four fixed issues. Flag anything that didn't behave as planned BEFORE claiming success (verification-before-completion).

---

## Out of scope (flagged for the user)

- **Map choice / flow insensitivity:** run 20260609-191326 showed this save's congestion is diffuse (no 500m cell >3% of load) and `flow_percent` barely responds to local edits. If reruns on this map still plateau at baseline, the map — not the harness — caps the benchmark's discrimination power. Consider a save with engineered chokepoints.
- **`BenchConfig` window sizes** (`window_ticks: 2048`, `settle_ticks: 8192`) were left untouched; they are not expressed in "days". Revisit once day-units feel right.
