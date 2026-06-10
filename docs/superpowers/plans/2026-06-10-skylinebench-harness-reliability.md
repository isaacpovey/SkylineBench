# Harness Reliability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop runs dying from machine sleep, concurrent-run collisions, vanishing temp workspaces, and 600-second `control_time` MCP timeouts.

**Architecture:** All shell fixes live in `benchmark/run.sh` (caffeinate wrapper, a lock directory, session dirs moved out of `TMPDIR`). The step-timeout fix lives in the broker's benchmark `control_time` handler: long steps are driven in one-day chunks against the mod with a wall-clock budget, returning a `partial` result instead of letting the MCP client kill the call.

**Tech Stack:** bash, Rust (tokio, rmcp), existing mock-mod test harness (`broker/src/mock.rs`).

**Evidence this matters:** run `20260609-210135` lost ~2.8 h to machine sleep ("Request timed out"), had its scratch workspace deleted mid-run, suffered two 600 s `control_time` timeouts, and a second `run.sh` invocation was started 3 minutes into it against the same game instance.

---

### Task 1: Step chunking in the benchmark server

**Files:**
- Modify: `broker/src/benchmark/server.rs` (the `control_time` tool, lines ~183–219, and its test module)

- [ ] **Step 1: Write failing tests for the chunking helper**

Add to the `tests` module at the bottom of `broker/src/benchmark/server.rs`:

```rust
    #[test]
    fn step_chunks_splits_into_days() {
        assert_eq!(step_chunks(1755, 585), vec![585, 585, 585]);
        assert_eq!(step_chunks(600, 585), vec![585, 15]);
        assert_eq!(step_chunks(585, 585), vec![585]);
        assert_eq!(step_chunks(10, 585), vec![10]);
        assert_eq!(step_chunks(0, 585), Vec::<u32>::new());
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --manifest-path broker/Cargo.toml step_chunks`
Expected: FAIL — `cannot find function step_chunks`.

- [ ] **Step 3: Implement `step_chunks`**

Add near the top of `broker/src/benchmark/server.rs` (after the `with_progress` function):

```rust
/// Split a step of `total` ticks into chunks of at most `chunk` ticks, so each
/// bridge call stays short and the whole tool call can bail out before the MCP
/// client timeout instead of being killed by it.
pub fn step_chunks(total: u32, chunk: u32) -> Vec<u32> {
    let chunk = chunk.max(1);
    let full = (0..total / chunk).map(move |_| chunk);
    let rem = total % chunk;
    full.chain((rem > 0).then_some(rem)).collect()
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --manifest-path broker/Cargo.toml step_chunks`
Expected: PASS.

- [ ] **Step 5: Write a failing behaviour test for chunked stepping**

Add to the same `tests` module (the mock advances its tick by exactly the requested amount per `/clock` call, so a chunked drive still lands on the same total):

```rust
    #[tokio::test]
    async fn chunked_step_advances_the_full_requested_ticks() {
        let bench = bench_with_mock().await;
        let res = bench
            .control_time(Parameters(crate::service::ControlTimeArgs {
                op: "step".into(),
                ticks: Some(1755),
                speed: None,
            }))
            .await
            .unwrap();
        let text = result_text(&res);
        assert!(text.contains("\"tick\":1755"), "got: {text}");
        assert!(text.contains("\"ticks_advanced\":1755"), "got: {text}");
        assert!(text.contains("\"partial\":false"), "got: {text}");
    }
```

- [ ] **Step 6: Run it to verify it fails**

Run: `cargo test --manifest-path broker/Cargo.toml chunked_step`
Expected: FAIL — response has no `ticks_advanced` / `partial` fields.

- [ ] **Step 7: Rework the `control_time` step path to drive chunks**

In `control_time` in `broker/src/benchmark/server.rs`, keep the `ensure_baseline` call, the `run_ended` early-return, and the `(day_ticks, max_ticks, max_step_days)` extraction, then replace EVERYTHING from `let is_step = args.op == "step";` to the end of the method (the old `let args = if is_step { ... }` rebinding and the final `match service::control_time(...)` both go away) with:

```rust
        let is_step = args.op == "step";
        if !is_step {
            return match service::control_time(&self.client, args).await {
                Ok(v) => {
                    if let Ok(m) = self.client.metrics().await {
                        self.state.lock().await.push_flow(m.traffic.flow_percent as f64);
                    }
                    self.finish(v).await
                }
                Err(e) => Ok(tool_err(e)),
            };
        }

        let requested = args.ticks.unwrap_or(day_ticks);
        if requested > max_ticks {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "step of {requested} ticks exceeds the cap of {max_ticks} ticks \
                 ({max_step_days} in-game days; 1 day ≈ {day_ticks} ticks). Request {max_ticks} or fewer."
            ))]));
        }
        let chunks = step_chunks(requested, day_ticks);
        let started = std::time::Instant::now();
        let wall_budget = std::time::Duration::from_secs(450);
        let mut advanced: u32 = 0;
        let mut last: Option<Value> = None;
        for chunk in chunks {
            match service::control_time(
                &self.client,
                ControlTimeArgs { op: "step".into(), ticks: Some(chunk), speed: None },
            )
            .await
            {
                Ok(v) => {
                    advanced += chunk;
                    last = Some(v);
                }
                Err(e) => return Ok(tool_err(e)),
            }
            if started.elapsed() > wall_budget {
                break;
            }
        }
        let mut out = match last {
            Some(v) => v,
            // requested == 0: fall through to a single zero-tick call so the
            // response still reports the clock state.
            None => match service::control_time(&self.client, ControlTimeArgs { op: "step".into(), ticks: Some(0), speed: None }).await {
                Ok(v) => v,
                Err(e) => return Ok(tool_err(e)),
            },
        };
        let partial = advanced < requested;
        if let Value::Object(ref mut map) = out {
            map.insert("ticks_advanced".into(), serde_json::json!(advanced));
            map.insert("partial".into(), serde_json::json!(partial));
            if partial {
                map.insert(
                    "message".into(),
                    serde_json::json!(format!(
                        "step ran out of wall-clock budget after {advanced} of {requested} ticks; \
                         call control_time step again for the remainder"
                    )),
                );
            }
        }
        if let Ok(m) = self.client.metrics().await {
            self.state.lock().await.push_flow(m.traffic.flow_percent as f64);
        }
        self.finish(out).await
```

The existing tests `step_without_ticks_defaults_to_one_day`, `step_above_three_days_is_rejected`, and `step_of_exactly_the_cap_is_allowed` must still pass unchanged (the default and the cap check are preserved verbatim above; a 585-tick default is a single chunk).

- [ ] **Step 8: Run the full broker test suite**

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: ALL PASS, including the three pre-existing step tests and the two new ones.

- [ ] **Step 9: Update the tool description**

In the same file, change the `control_time` `#[tool(description = ...)]` to:

```rust
    #[tool(description = "Control simulation time: pause, resume, step, or set speed. \
        `step` defaults to 1 in-game day (585 ticks) when `ticks` is omitted; \
        the maximum step is 3 days (1755 ticks). Long steps are driven in chunks; \
        if the response has `partial: true`, call step again for the remaining ticks.")]
```

- [ ] **Step 10: Commit**

```bash
git add broker/src/benchmark/server.rs
git commit -m "fix(broker): drive long steps in day chunks with a wall-clock budget"
```

---

### Task 2: run.sh — keep-awake, run lock, durable session dir

**Files:**
- Modify: `benchmark/run.sh`

- [ ] **Step 1: Add the run lock and move the session dir out of TMPDIR**

In `benchmark/run.sh`, replace lines 30–38 (the `SESSION_DIR` block, starting at the comment `# Per-run session dir OUTSIDE the repo:` and ending at `mkdir -p "$WORKSPACE"`) with:

```bash
# Only one run may drive the single game instance at a time. A second run.sh
# started mid-run (this happened on 2026-06-09: 21:01 + 21:04 against one game)
# corrupts both runs' measurements.
LOCK_DIR="${TMPDIR:-/tmp}/skylinebench.lock"
if ! mkdir "$LOCK_DIR" 2>/dev/null; then
  echo "another benchmark run appears active (lock: $LOCK_DIR). Remove the dir if it is stale." >&2
  exit 1
fi

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
trap 'rm -rf "$SESSION_DIR"; rmdir "$LOCK_DIR" 2>/dev/null' EXIT
WORKSPACE="$SESSION_DIR/workspace"
mkdir -p "$WORKSPACE"
```

(The old `trap 'rm -rf "$SESSION_DIR"' EXIT` line is replaced by the combined trap above — make sure it doesn't appear twice.)

- [ ] **Step 2: Wrap the agent command in caffeinate**

Replace the `SANDBOX=(...)`/`CMD=(...)` block (lines 84–89) with:

```bash
SANDBOX=(sandbox-exec -f "$SANDBOX_PROFILE")
# caffeinate -dims: block display/idle/disk/system sleep for the lifetime of
# the agent session. Machine sleep killed run 20260609-210135 ~2.8h in.
KEEPAWAKE=()
command -v caffeinate >/dev/null && KEEPAWAKE=(caffeinate -dims)
if [ "$WATCH" -eq 1 ]; then
  CMD=("${KEEPAWAKE[@]}" "${SANDBOX[@]}" claude --mcp-config "$MCP_CONFIG" --strict-mcp-config --allowedTools "$ALLOWED" --disallowedTools "$DISALLOWED" --permission-mode bypassPermissions "$PROMPT")
else
  CMD=("${KEEPAWAKE[@]}" "${SANDBOX[@]}" claude -p "$PROMPT" --mcp-config "$MCP_CONFIG" --strict-mcp-config --allowedTools "$ALLOWED" --disallowedTools "$DISALLOWED" --permission-mode bypassPermissions --output-format stream-json --verbose)
fi
```

- [ ] **Step 3: Verify the script**

Run: `bash -n benchmark/run.sh`
Expected: no output (syntax OK).

Run: `DRY_RUN=1 ./benchmark/run.sh --map gridlock-v1`
Expected: the printed command starts with `caffeinate -dims sandbox-exec -f`, and the printed paths reference `~/Library/Caches/skylinebench/`. The lock dir is created and removed (check `ls "${TMPDIR:-/tmp}" | grep skylinebench` shows no leftover `skylinebench.lock`).

Run it twice concurrently to verify the lock:
```bash
( DRY_RUN=1 ./benchmark/run.sh --map gridlock-v1 & mkdir "${TMPDIR:-/tmp}/skylinebench.lock" 2>/dev/null; DRY_RUN=1 ./benchmark/run.sh --map gridlock-v1; rmdir "${TMPDIR:-/tmp}/skylinebench.lock" )
```
Expected: the second invocation prints `another benchmark run appears active` and exits 1.

- [ ] **Step 4: Update the benchmark README**

In `benchmark/README.md`, add to the per-run steps list:

```markdown
   - Runs are serialized by a lock at `$TMPDIR/skylinebench.lock`; never start
     two runs against one game instance. `run.sh` keeps the machine awake
     (`caffeinate`) for the whole session.
```

- [ ] **Step 5: Commit**

```bash
git add benchmark/run.sh benchmark/README.md
git commit -m "fix(benchmark): caffeinate the session, lock concurrent runs, move session dir out of TMPDIR"
```
