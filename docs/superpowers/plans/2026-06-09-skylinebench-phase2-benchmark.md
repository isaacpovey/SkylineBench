# SkylineBench Phase 2 — Traffic-Improvement Benchmark Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `benchmark` mode to the broker that scores a Claude Code agent on improving traffic in a bad-traffic CS1 city, plus a launcher script that runs the agent and captures its transcript.

**Architecture:** A new `benchmark` module in the existing Rust broker reuses the live MCP tool surface but instruments it: it counts mutating actions, computes per-action cost from a road-cost table, samples city-wide flow, enforces three run-end conditions (submit / flow-target / 3h cap), runs baseline + settle + final measurement windows by driving the clock, and writes a `run-record.json` + `score.json` per run. Scoring is a pure, replayable function. A `benchmark/run.sh` launches Claude Code headless against `broker benchmark` over stdio, supplies a versioned prompt, and bundles the captured transcript with the score artifacts.

**Tech Stack:** Rust (broker: tokio, rmcp 1.7, axum mock, serde), C# / Mono net35 (mod, zero-dep), Bash (runner).

**Spec:** `docs/superpowers/specs/2026-06-09-skylinebench-phase2-benchmark-design.md` — section references below are to that spec.

---

## Context the engineer needs

- `broker/src/contract.rs` is the **frozen wire format**. Any change here must be mirrored in the mock (`broker/src/mock.rs`) and the C# mod, or deserialization breaks. Both sides serialize to exactly these shapes.
- The broker has an **in-memory mock mod** (`broker/src/mock.rs`, axum) used by all broker tests — no game required. Its `/metrics` flow is deterministic: `flow_percent = max(0, 100 − segment_count·5)`. Tests drive the benchmark engine against this mock.
- Existing MCP tools live in `broker/src/tools.rs` (the `Skyline` rmcp server, 12 tools) and delegate to pure-ish functions in `broker/src/service.rs`. **Benchmark mode adds a parallel `BenchmarkServer`** that delegates to the same `service::*` functions but wraps them with instrumentation — `Skyline` is left untouched.
- Run all broker tests with `cd broker && cargo test`. Currently **37 tests + 1 doctest** pass. Each task below adds tests; the count grows.
- The C# mod has a zero-dependency test runner. Build/test with: `cd mod && xbuild test/Tests.csproj /p:Configuration=Debug && mono test/bin/Debug/Tests.exe` (Homebrew Mono has **`xbuild`, not `msbuild`**). The mod compiles against game assemblies via `./build.sh`.
- Functional style where it maps to Rust/C# (immutability, iterators over mutate-in-loop); minimal comments; keep the mod thin (no business logic in C#).
- **Commit after every task.** Commit messages end with the `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>` trailer. Work stays on branch `skylinebench-phase2-benchmark`.

## File structure

**Broker — new files (`broker/src/benchmark/`):**
- `mod.rs` — module declarations + re-exports.
- `config.rs` — `BenchConfig`: weights, normalization bounds, window sizes, caps (spec §6). Pure, with `Default`.
- `cost.rs` — pure `road_cost(construction_cost, length_m, &BenchConfig) -> i64` (spec §8 cost model).
- `record.rs` — serde types written to disk: `RunRecord`, `WindowStats`, `ActionEntry`, `EndReason`, `Score`, `MapInfo`, `Tally`.
- `score.rs` — pure `score_run(&RunRecord, &BenchConfig) -> Score` (normalization + weighted sum + throughput guard, spec §4–§5).
- `flow_window.rs` — pure rolling-window helpers: push a sample, windowed mean, flow-target test.
- `state.rs` — `RunState`: live counters, cost tally, action log, flow buffer, baseline, road-cost table, start instant, end reason; mutation methods.
- `measure.rs` — async window measurement + finalize (settle + final + write artifacts) by driving the `BridgeClient`.
- `server.rs` — `BenchmarkServer` rmcp tool surface: delegates to `service::*`, instruments, attaches `benchmark_progress`, adds `submit_solution`.
- `transcript.rs` — pure `render_transcript(jsonl: &str) -> String` (stream-json → readable markdown).

**Broker — modified files:**
- `contract.rs` — add `RoadType { name, construction_cost }`; change `RoadTypes.road_types` to `Vec<RoadType>`.
- `bridge_client.rs` — `road_types()` returns the new shape (no code change beyond the type; verify).
- `service.rs` — adapt the three call sites that read `road_types` (validate/list/build/upgrade) to the new shape.
- `validate.rs` — `validate_build_road` compares against road-type **names**.
- `mock.rs` — `/road-types` emits `{name, construction_cost}` objects.
- `lib.rs` — add `pub mod benchmark;`.
- `main.rs` — add `Benchmark { … }` and `RenderTranscript { … }` subcommands.

**Mod — modified files:**
- `src/bridge/ErrorCode.cs` — `Prefabs.RoadNames()` → `Prefabs.Roads()` returning name + construction cost.
- `src/http/Handlers.cs` — `RoadTypes()` emits the `{name, construction_cost}` array.
- `mod/test/SerializeTests.cs` (or a new test file) — assert the new road-types JSON shape.

**New top-level `benchmark/`:**
- `benchmark/prompt.md` — the agent task prompt (spec §11).
- `benchmark/run.sh` — launcher (headless + `--watch`), transcript capture, artifact bundling.
- `benchmark/maps/README.md` — how the pinned save is configured (Unlimited Money + Unlock All) and documented (spec §9).
- `benchmark/README.md` — operator run instructions (spec §2, §11).

---

## Phase A — Contract: construction cost on road types

### Task A1: Add `RoadType` to the contract

**Files:**
- Modify: `broker/src/contract.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `broker/src/contract.rs`:

```rust
    #[test]
    fn road_types_round_trips_with_cost() {
        let original = RoadTypes {
            road_types: vec![
                RoadType { name: "Basic Road".into(), construction_cost: 1200 },
                RoadType { name: "Highway".into(), construction_cost: 8000 },
            ],
        };
        let json = serde_json::to_string(&original).unwrap();
        assert!(
            json.contains("\"name\":\"Basic Road\",\"construction_cost\":1200"),
            "got {json}"
        );
        let parsed: RoadTypes = serde_json::from_str(&json).unwrap();
        assert_eq!(original, parsed);
    }
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd broker && cargo test --lib contract::tests::road_types_round_trips_with_cost`
Expected: FAIL to compile — `RoadType` not found, `RoadTypes.road_types` is `Vec<String>`.

- [ ] **Step 3: Change the contract types**

In `broker/src/contract.rs`, replace the `RoadTypes` definition (currently `pub road_types: Vec<String>`) with:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoadType {
    pub name: String,
    pub construction_cost: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoadTypes {
    pub road_types: Vec<RoadType>,
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cd broker && cargo test --lib contract::tests::road_types_round_trips_with_cost`
Expected: PASS. (Other files won't compile yet — that's Task A2+. Run the targeted test with `--lib`; the crate may still fail to build elsewhere. If `cargo test` can't build the lib due to downstream callers, proceed to A2 and run the full suite there.)

- [ ] **Step 5: Commit**

```bash
git add broker/src/contract.rs
git commit -m "feat(contract): add RoadType with construction_cost

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task A2: Propagate the new road-types shape through the broker

**Files:**
- Modify: `broker/src/validate.rs`
- Modify: `broker/src/service.rs`

- [ ] **Step 1: Update `validate_build_road` to compare on names**

In `broker/src/validate.rs`, change the signature and the membership check. Replace the function signature line and the first check:

```rust
use crate::contract::{ActionError, Position, RoadType};
use crate::geometry::{horizontal_distance, in_bounds, playable_bounds, MAX_SEGMENT_LENGTH_M};

pub fn validate_build_road(
    start: Position,
    end: Position,
    road_type: &str,
    known_road_types: &[RoadType],
) -> Result<(), ActionError> {
    if !known_road_types.iter().any(|t| t.name == road_type) {
        return Err(ActionError::InvalidPrefab);
    }
    // ... rest unchanged ...
```

In the same file's `tests` module, update the `road_types()` helper:

```rust
    fn road_types() -> Vec<crate::contract::RoadType> {
        use crate::contract::RoadType;
        vec![
            RoadType { name: "road".into(), construction_cost: 1000 },
            RoadType { name: "highway".into(), construction_cost: 5000 },
        ]
    }
```

- [ ] **Step 2: Update the three `service.rs` call sites**

In `broker/src/service.rs`:

`build_road` — `client.road_types()` now yields `Vec<RoadType>`; pass it straight to `validate_build_road`:

```rust
pub async fn build_road(client: &BridgeClient, args: BuildRoadArgs) -> Result<Value, ServiceError> {
    let road_types = client.road_types().await?.road_types;
    if let Err(reason) = validate_build_road(args.from, args.to, &args.road_type, &road_types) {
        return Ok(action_error_value(reason));
    }
    let res = client
        .build_road(args.from, args.to, &args.road_type, args.snap)
        .await?;
    Ok(serde_json::to_value(res).unwrap())
}
```

`list_road_types` — return the full objects (the agent benefits from seeing costs):

```rust
pub async fn list_road_types(client: &BridgeClient) -> Result<Value, ServiceError> {
    Ok(json!({ "road_types": client.road_types().await?.road_types }))
}
```

`upgrade_road` — compare on names:

```rust
pub async fn upgrade_road(
    client: &BridgeClient,
    args: UpgradeRoadArgs,
) -> Result<Value, ServiceError> {
    let road_types = client.road_types().await?.road_types;
    if !road_types.iter().any(|t| t.name == args.road_type) {
        return Ok(action_error_value(ActionError::InvalidPrefab));
    }
    let res = client.upgrade_road(args.segment, &args.road_type).await?;
    Ok(serde_json::to_value(res).unwrap())
}
```

- [ ] **Step 3: Update the mock so the suite can build (full detail in A3, minimal here)**

The broker tests won't link until `mock.rs` emits the new shape. Do Task A3 now, then return here to run the suite. (If executing strictly task-by-task, treat A2+A3 as one commit.)

- [ ] **Step 4: Run the full broker suite**

Run: `cd broker && cargo test`
Expected: PASS (all existing tests green; `list_road_types` test, if any, now sees objects). The `service.rs` test `build_road_rejects_unknown_type_before_hitting_mod` still expects `INVALID_PREFAB` and still passes.

- [ ] **Step 5: Commit**

```bash
git add broker/src/validate.rs broker/src/service.rs
git commit -m "feat(broker): propagate RoadType through validate + service

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task A3: Mock emits road types with costs

**Files:**
- Modify: `broker/src/mock.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `broker/src/mock.rs` (use the existing mock test harness pattern — start the server, hit `/road-types`):

```rust
    #[tokio::test]
    async fn road_types_endpoint_includes_costs() {
        let (addr, server) = bind("127.0.0.1:0".parse().unwrap()).await;
        tokio::spawn(server);
        let body: crate::contract::RoadTypes = reqwest::Client::new()
            .get(format!("http://{addr}/road-types"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let road = body.road_types.iter().find(|r| r.name == "road").unwrap();
        assert!(road.construction_cost > 0);
        let highway = body.road_types.iter().find(|r| r.name == "highway").unwrap();
        assert!(highway.construction_cost > road.construction_cost);
    }
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd broker && cargo test --lib mock::tests::road_types_endpoint_includes_costs`
Expected: FAIL — `road_types()` returns `Vec<String>`; `RoadTypes` shape mismatch.

- [ ] **Step 3: Update the mock's road-type data**

In `broker/src/mock.rs`, replace the `road_types()` helper and the `road_types_ep` handler:

```rust
fn road_types() -> Vec<crate::contract::RoadType> {
    use crate::contract::RoadType;
    vec![
        RoadType { name: "road".into(), construction_cost: 1000 },
        RoadType { name: "highway".into(), construction_cost: 5000 },
    ]
}

async fn road_types_ep() -> Json<RoadTypes> {
    Json(RoadTypes { road_types: road_types() })
}
```

Then fix the two build/upgrade validity checks in the mock that call `road_types().contains(&body.prefab)` — they now compare against names:

```rust
    if !road_types().iter().any(|r| r.name == body.prefab) {
```

(Apply this in both `build_road` and `upgrade_road` handlers in `mock.rs`.)

- [ ] **Step 4: Run the full broker suite**

Run: `cd broker && cargo test`
Expected: PASS, including the new test.

- [ ] **Step 5: Commit**

```bash
git add broker/src/mock.rs
git commit -m "feat(mock): road-types endpoint returns construction costs

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task A4: Mod emits construction cost from `NetInfo`

**Files:**
- Modify: `mod/src/bridge/ErrorCode.cs`
- Modify: `mod/src/http/Handlers.cs`
- Test: `mod/test/SerializeTests.cs`

- [ ] **Step 1: Confirm the field name offline**

Run (from `mod/`, adjust the path if needed):
```bash
ikdasm "$HOME/Library/Application Support/Steam/steamapps/common/Cities_Skylines/Cities.app/Contents/Resources/Data/Managed/Assembly-CSharp.dll" \
  | grep -iE 'm_constructionCost|constructionCost' | head
```
Expected: a field `int32 m_constructionCost` on `NetInfo` (CS1 exposes this). If the name differs, use the disassembled name in Step 3. Disassembling beats guessing — several Phase-1 bugs were signature mismatches.

- [ ] **Step 2: Add a road-with-cost reader**

In `mod/src/bridge/ErrorCode.cs`, add a small struct and method to `Prefabs` (keep `RoadNames()` for any existing callers, or replace its uses):

```csharp
        public struct RoadInfo { public string Name; public long ConstructionCost; }

        /// <summary>Road-service prefabs with their NetInfo construction cost.</summary>
        public static System.Collections.Generic.List<RoadInfo> Roads()
        {
            var list = new System.Collections.Generic.List<RoadInfo>();
            int count = PrefabCollection<NetInfo>.PrefabCount();
            for (uint i = 0; i < count; i++)
            {
                var p = PrefabCollection<NetInfo>.GetPrefab(i);
                if (p != null && p.name != null && p.m_class != null && p.m_class.m_service == ItemClass.Service.Road)
                    list.Add(new RoadInfo { Name = p.name, ConstructionCost = p.m_constructionCost });
            }
            return list;
        }
```

- [ ] **Step 3: Emit the new JSON shape**

In `mod/src/http/Handlers.cs`, replace `RoadTypes()`:

```csharp
        public static HttpReply RoadTypes()
        {
            var w = new JsonWriter(); w.BeginObject().Name("road_types").BeginArray();
            foreach (var r in Prefabs.Roads())
            {
                w.BeginObject().Name("name").Value(r.Name).Name("construction_cost").Value(r.ConstructionCost).EndObject();
            }
            w.EndArray().EndObject(); return HttpReply.Json(200, w.ToString());
        }
```

(`JsonWriter.Value(long)` already exists — `Health` writes `(long)h.Tick`. Confirm `BeginObject` is legal inside an array element; the writer is used object-in-array in `Serialize.Network`. If not, mirror that exact pattern.)

- [ ] **Step 4: Add a mod test for the shape**

In `mod/test/SerializeTests.cs`, add a test that builds the JSON directly with `JsonWriter` the same way `Handlers.RoadTypes` does (the test runner can't load the game, so test the writer shape, not `Prefabs.Roads()`):

```csharp
        public static void RoadTypesShape()
        {
            var w = new SkylineBench.Json.JsonWriter();
            w.BeginObject().Name("road_types").BeginArray();
            w.BeginObject().Name("name").Value("Basic Road").Name("construction_cost").Value((long)1200).EndObject();
            w.EndArray().EndObject();
            Assert.Equal("{\"road_types\":[{\"name\":\"Basic Road\",\"construction_cost\":1200}]}", w.ToString());
        }
```

Register it in the test runner if registration is manual (check how `SerializeTests` methods are invoked in `mod/test/TestRunner.cs` and add a matching call).

- [ ] **Step 5: Run the mod tests**

Run: `cd mod && xbuild test/Tests.csproj /p:Configuration=Debug && mono test/bin/Debug/Tests.exe`
Expected: all tests pass, including `RoadTypesShape`.

- [ ] **Step 6: Commit**

```bash
git add mod/src/bridge/ErrorCode.cs mod/src/http/Handlers.cs mod/test/SerializeTests.cs
git commit -m "feat(mod): /road-types reports NetInfo construction cost

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Phase B — Benchmark core (pure, fully unit-testable)

### Task B1: Benchmark module skeleton + config

**Files:**
- Create: `broker/src/benchmark/mod.rs`
- Create: `broker/src/benchmark/config.rs`
- Modify: `broker/src/lib.rs`

- [ ] **Step 1: Register the module**

In `broker/src/lib.rs`, add (keep alphabetical-ish with the others):

```rust
pub mod benchmark;
```

Create `broker/src/benchmark/mod.rs`:

```rust
pub mod config;
```

- [ ] **Step 2: Write the failing test**

Create `broker/src/benchmark/config.rs` with only the test first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_weights_sum_to_one() {
        let c = BenchConfig::default();
        let sum = c.w_flow + c.w_money + c.w_changes;
        assert!((sum - 1.0).abs() < 1e-9, "weights sum to {sum}");
    }

    #[test]
    fn default_flow_dominant() {
        let c = BenchConfig::default();
        assert!(c.w_flow > c.w_money && c.w_flow > c.w_changes);
        assert_eq!(c.flow_target, 95.0);
        assert_eq!(c.wall_clock_cap_secs, 10_800);
        assert_eq!(c.guard_ratio, 0.9);
    }
}
```

- [ ] **Step 3: Run it to confirm it fails**

Run: `cd broker && cargo test --lib benchmark::config`
Expected: FAIL — `BenchConfig` undefined.

- [ ] **Step 4: Implement `BenchConfig`**

Prepend to `broker/src/benchmark/config.rs` (spec §6 placeholders; all tunable):

```rust
use serde::{Deserialize, Serialize};

/// Benchmark scoring + protocol constants (spec §6). Values are calibration
/// placeholders tuned against the chosen map during the verify step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchConfig {
    pub w_flow: f64,
    pub w_money: f64,
    pub w_changes: f64,
    pub target_gain: f64,
    pub budget: f64,
    pub change_cap: f64,
    pub flow_target: f64,
    pub window_ticks: u32,
    pub settle_ticks: u32,
    pub window_samples: u32,
    pub wall_clock_cap_secs: u64,
    pub guard_ratio: f64,
    /// Length (m) over which a road's `construction_cost` is charged once
    /// (cost = construction_cost · length / cost_base_length_m). Calibrated.
    pub cost_base_length_m: f64,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            w_flow: 0.60,
            w_money: 0.20,
            w_changes: 0.20,
            target_gain: 40.0,
            budget: 5_000_000.0,
            change_cap: 100.0,
            flow_target: 95.0,
            window_ticks: 2048,
            settle_ticks: 8192,
            window_samples: 8,
            wall_clock_cap_secs: 10_800,
            guard_ratio: 0.9,
            cost_base_length_m: 64.0,
        }
    }
}
```

- [ ] **Step 5: Run the test to confirm it passes**

Run: `cd broker && cargo test --lib benchmark::config`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add broker/src/lib.rs broker/src/benchmark/mod.rs broker/src/benchmark/config.rs
git commit -m "feat(benchmark): add module skeleton + BenchConfig

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task B2: Pure cost model

**Files:**
- Create: `broker/src/benchmark/cost.rs`
- Modify: `broker/src/benchmark/mod.rs`

- [ ] **Step 1: Register + write the failing test**

Add `pub mod cost;` to `broker/src/benchmark/mod.rs`. Create `broker/src/benchmark/cost.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark::config::BenchConfig;

    #[test]
    fn cost_scales_with_length() {
        let cfg = BenchConfig::default(); // cost_base_length_m = 64
        // 64 m of a 1000-cost road == one base length == 1000.
        assert_eq!(road_cost(1000, 64.0, &cfg), 1000);
        // 128 m == double.
        assert_eq!(road_cost(1000, 128.0, &cfg), 2000);
    }

    #[test]
    fn cost_rounds_to_nearest_and_is_non_negative() {
        let cfg = BenchConfig::default();
        assert_eq!(road_cost(1000, 32.0, &cfg), 500);
        assert_eq!(road_cost(0, 100.0, &cfg), 0);
        assert_eq!(road_cost(1000, 0.0, &cfg), 0);
    }
}
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd broker && cargo test --lib benchmark::cost`
Expected: FAIL — `road_cost` undefined.

- [ ] **Step 3: Implement the cost model**

Prepend to `broker/src/benchmark/cost.rs`:

```rust
use crate::benchmark::config::BenchConfig;

/// Cost charged for building `length_m` of a road whose NetInfo
/// `construction_cost` is `construction_cost` (spec §8). Computed from the
/// action, never from in-game funds (money is unlimited in benchmark mode).
pub fn road_cost(construction_cost: i64, length_m: f32, cfg: &BenchConfig) -> i64 {
    let raw = construction_cost as f64 * (length_m as f64) / cfg.cost_base_length_m;
    raw.round().max(0.0) as i64
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cd broker && cargo test --lib benchmark::cost`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/benchmark/mod.rs broker/src/benchmark/cost.rs
git commit -m "feat(benchmark): pure road cost model

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task B3: Run-record + score serde types

**Files:**
- Create: `broker/src/benchmark/record.rs`
- Modify: `broker/src/benchmark/mod.rs`

- [ ] **Step 1: Register + write the failing test**

Add `pub mod record;` to `broker/src/benchmark/mod.rs`. Create `broker/src/benchmark/record.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_reason_serializes_snake() {
        assert_eq!(serde_json::to_string(&EndReason::FlowTarget).unwrap(), "\"flow_target\"");
        assert_eq!(serde_json::to_string(&EndReason::Submit).unwrap(), "\"submit\"");
        assert_eq!(serde_json::to_string(&EndReason::Timeout).unwrap(), "\"timeout\"");
    }

    #[test]
    fn run_record_round_trips() {
        let r = RunRecord {
            schema_version: 1,
            map: MapInfo { id: "gridlock-v1".into(), source: "workshop:123".into(), game_version: "1.21.1-f9".into() },
            started_at: "2026-06-09T00:00:00Z".into(),
            ended_at: "2026-06-09T01:00:00Z".into(),
            end_reason: EndReason::Submit,
            baseline: WindowStats { flow_mean: 6.0, active_vehicles_mean: 240.0, population: 3380 },
            final_stats: WindowStats { flow_mean: 41.0, active_vehicles_mean: 230.0, population: 3375 },
            tally: Tally { num_changes: 12, money_spent: 250_000 },
            actions: vec![ActionEntry { seq: 1, tool: "build_road".into(), cost: 12000 }],
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: RunRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }
}
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd broker && cargo test --lib benchmark::record`
Expected: FAIL — types undefined.

- [ ] **Step 3: Implement the types**

Prepend to `broker/src/benchmark/record.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    Submit,
    FlowTarget,
    Timeout,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapInfo {
    pub id: String,
    pub source: String,
    pub game_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowStats {
    pub flow_mean: f64,
    pub active_vehicles_mean: f64,
    pub population: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tally {
    pub num_changes: u32,
    pub money_spent: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionEntry {
    pub seq: u32,
    pub tool: String,
    pub cost: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRecord {
    pub schema_version: u32,
    pub map: MapInfo,
    pub started_at: String,
    pub ended_at: String,
    pub end_reason: EndReason,
    pub baseline: WindowStats,
    #[serde(rename = "final")]
    pub final_stats: WindowStats,
    pub tally: Tally,
    pub actions: Vec<ActionEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreNorms {
    pub flow: f64,
    pub money: f64,
    pub changes: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Score {
    pub norm: ScoreNorms,
    pub weighted: ScoreNorms,
    pub invalid: bool,
    pub score: f64,
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cd broker && cargo test --lib benchmark::record`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/benchmark/mod.rs broker/src/benchmark/record.rs
git commit -m "feat(benchmark): run-record and score serde types

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task B4: Pure scoring function

**Files:**
- Create: `broker/src/benchmark/score.rs`
- Modify: `broker/src/benchmark/mod.rs`

- [ ] **Step 1: Register + write the failing tests**

Add `pub mod score;` to `broker/src/benchmark/mod.rs`. Create `broker/src/benchmark/score.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark::config::BenchConfig;
    use crate::benchmark::record::*;

    fn record(flow_gain: f64, money: i64, changes: u32, veh_base: f64, veh_final: f64) -> RunRecord {
        RunRecord {
            schema_version: 1,
            map: MapInfo { id: "m".into(), source: "s".into(), game_version: "v".into() },
            started_at: "a".into(),
            ended_at: "b".into(),
            end_reason: EndReason::Submit,
            baseline: WindowStats { flow_mean: 10.0, active_vehicles_mean: veh_base, population: 100 },
            final_stats: WindowStats { flow_mean: 10.0 + flow_gain, active_vehicles_mean: veh_final, population: 100 },
            tally: Tally { num_changes: changes, money_spent: money },
            actions: vec![],
        }
    }

    #[test]
    fn perfect_cheap_run_scores_high() {
        let cfg = BenchConfig::default(); // target_gain 40, budget 5e6, change_cap 100
        // Full flow gain, zero spend, zero changes, throughput preserved.
        let s = score_run(&record(40.0, 0, 0, 240.0, 240.0), &cfg);
        assert!(!s.invalid);
        assert!((s.score - 1.0).abs() < 1e-9, "score = {}", s.score);
    }

    #[test]
    fn flow_dominates() {
        let cfg = BenchConfig::default();
        // No flow gain but free + zero changes: only the efficiency terms (0.40).
        let s = score_run(&record(0.0, 0, 0, 240.0, 240.0), &cfg);
        assert!((s.score - 0.40).abs() < 1e-9, "score = {}", s.score);
    }

    #[test]
    fn norms_clamp_to_unit() {
        let cfg = BenchConfig::default();
        // Over-target flow gain and over-budget spend both clamp.
        let s = score_run(&record(1000.0, 50_000_000, 1000, 240.0, 240.0), &cfg);
        assert_eq!(s.norm.flow, 1.0);
        assert_eq!(s.norm.money, 1.0);
        assert_eq!(s.norm.changes, 1.0);
        // flow maxed (0.60) + money/changes maxed -> their (1-norm) terms vanish.
        assert!((s.score - 0.60).abs() < 1e-9, "score = {}", s.score);
    }

    #[test]
    fn throughput_guard_zeroes_invalid_run() {
        let cfg = BenchConfig::default();
        // active_vehicles drops below 0.9 * baseline -> invalid.
        let s = score_run(&record(40.0, 0, 0, 240.0, 200.0), &cfg);
        assert!(s.invalid);
        assert_eq!(s.score, 0.0);
    }

    #[test]
    fn guard_passes_at_exactly_the_ratio() {
        let cfg = BenchConfig::default();
        let s = score_run(&record(40.0, 0, 0, 240.0, 216.0), &cfg); // 216 == 0.9 * 240
        assert!(!s.invalid);
    }
}
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd broker && cargo test --lib benchmark::score`
Expected: FAIL — `score_run` undefined.

- [ ] **Step 3: Implement scoring**

Prepend to `broker/src/benchmark/score.rs` (spec §4–§5):

```rust
use crate::benchmark::config::BenchConfig;
use crate::benchmark::record::{RunRecord, Score, ScoreNorms};

fn clamp01(x: f64) -> f64 {
    x.max(0.0).min(1.0)
}

pub fn score_run(record: &RunRecord, cfg: &BenchConfig) -> Score {
    let delta_flow = record.final_stats.flow_mean - record.baseline.flow_mean;
    let norm = ScoreNorms {
        flow: clamp01(delta_flow / cfg.target_gain),
        money: clamp01(record.tally.money_spent as f64 / cfg.budget),
        changes: clamp01(record.tally.num_changes as f64 / cfg.change_cap),
    };
    let weighted = ScoreNorms {
        flow: cfg.w_flow * norm.flow,
        money: cfg.w_money * (1.0 - norm.money),
        changes: cfg.w_changes * (1.0 - norm.changes),
    };
    let invalid = record.final_stats.active_vehicles_mean
        < cfg.guard_ratio * record.baseline.active_vehicles_mean;
    let score = if invalid {
        0.0
    } else {
        weighted.flow + weighted.money + weighted.changes
    };
    Score { norm, weighted, invalid, score }
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cd broker && cargo test --lib benchmark::score`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add broker/src/benchmark/mod.rs broker/src/benchmark/score.rs
git commit -m "feat(benchmark): pure scoring with throughput guard

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task B5: Flow rolling-window helpers

**Files:**
- Create: `broker/src/benchmark/flow_window.rs`
- Modify: `broker/src/benchmark/mod.rs`

- [ ] **Step 1: Register + write the failing test**

Add `pub mod flow_window;` to `broker/src/benchmark/mod.rs`. Create `broker/src/benchmark/flow_window.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mean_of_recent_window() {
        let mut w = FlowWindow::new(3);
        w.push(10.0);
        w.push(20.0);
        w.push(30.0);
        w.push(60.0); // evicts 10.0 -> window is [20,30,60]
        assert!((w.mean() - 36.666_666).abs() < 1e-3);
    }

    #[test]
    fn empty_window_mean_is_zero() {
        assert_eq!(FlowWindow::new(4).mean(), 0.0);
    }

    #[test]
    fn target_reached_only_on_windowed_mean() {
        let mut w = FlowWindow::new(2);
        w.push(100.0); // one high sample, mean 100 but window not full
        assert!(!w.target_reached(95.0), "single sample must not trip");
        w.push(96.0); // window full, mean 98 >= 95
        assert!(w.target_reached(95.0));
    }
}
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd broker && cargo test --lib benchmark::flow_window`
Expected: FAIL — `FlowWindow` undefined.

- [ ] **Step 3: Implement the window**

Prepend to `broker/src/benchmark/flow_window.rs`:

```rust
use std::collections::VecDeque;

/// Rolling buffer of recent `flow_percent` samples. The flow-target out (spec
/// §7) fires only once the buffer is full and its mean clears the target, so a
/// single transient post-edit spike can't end the run.
#[derive(Debug)]
pub struct FlowWindow {
    capacity: usize,
    samples: VecDeque<f64>,
}

impl FlowWindow {
    pub fn new(capacity: usize) -> Self {
        Self { capacity: capacity.max(1), samples: VecDeque::new() }
    }

    pub fn push(&mut self, sample: f64) {
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    pub fn mean(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        self.samples.iter().sum::<f64>() / self.samples.len() as f64
    }

    pub fn is_full(&self) -> bool {
        self.samples.len() == self.capacity
    }

    pub fn target_reached(&self, target: f64) -> bool {
        self.is_full() && self.mean() >= target
    }
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cd broker && cargo test --lib benchmark::flow_window`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/benchmark/mod.rs broker/src/benchmark/flow_window.rs
git commit -m "feat(benchmark): rolling flow window with windowed target test

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Phase C — Benchmark runtime (state, measurement, finalize)

### Task C1: Run state + progress envelope

**Files:**
- Create: `broker/src/benchmark/state.rs`
- Modify: `broker/src/benchmark/mod.rs`

- [ ] **Step 1: Register + write the failing test**

Add `pub mod state;` to `broker/src/benchmark/mod.rs`. Create `broker/src/benchmark/state.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark::config::BenchConfig;
    use crate::benchmark::record::WindowStats;
    use std::collections::HashMap;

    fn state() -> RunState {
        let mut costs = HashMap::new();
        costs.insert("road".to_string(), 1000i64);
        RunState::new(
            BenchConfig::default(),
            "gridlock-v1".into(),
            WindowStats { flow_mean: 6.0, active_vehicles_mean: 240.0, population: 3380 },
            costs,
        )
    }

    #[test]
    fn records_changes_and_cost() {
        let mut s = state();
        s.record_mutation("build_road", 12_000);
        s.record_mutation("set_zoning", 0);
        assert_eq!(s.num_changes, 2);
        assert_eq!(s.money_spent, 12_000);
        assert_eq!(s.actions.len(), 2);
        assert_eq!(s.actions[0].seq, 1);
    }

    #[test]
    fn progress_omits_score_fields() {
        let s = state();
        let p = s.progress(900);
        assert_eq!(p["flow_target"], 95.0);
        assert_eq!(p["seconds_remaining"], 900);
        assert!(p.get("score").is_none());
        assert!(p.get("composite_score").is_none());
        assert!(p.get("weights").is_none());
    }

    #[test]
    fn road_cost_lookup_uses_table_and_config() {
        let s = state();
        // 64 m of "road" (cost 1000) at base length 64 -> 1000.
        assert_eq!(s.build_cost("road", 64.0), 1000);
        // Unknown road type -> 0 (validation rejects it elsewhere).
        assert_eq!(s.build_cost("missing", 64.0), 0);
    }
}
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd broker && cargo test --lib benchmark::state`
Expected: FAIL — `RunState` undefined.

- [ ] **Step 3: Implement `RunState`**

Prepend to `broker/src/benchmark/state.rs`:

```rust
use std::collections::HashMap;
use std::time::Instant;

use serde_json::{json, Value};

use crate::benchmark::config::BenchConfig;
use crate::benchmark::cost::road_cost;
use crate::benchmark::flow_window::FlowWindow;
use crate::benchmark::record::{ActionEntry, EndReason, WindowStats};

pub struct RunState {
    pub config: BenchConfig,
    pub map_id: String,
    pub baseline: WindowStats,
    pub road_costs: HashMap<String, i64>,
    pub num_changes: u32,
    pub money_spent: i64,
    pub actions: Vec<ActionEntry>,
    pub flow: FlowWindow,
    pub start: Instant,
    pub end_reason: Option<EndReason>,
}

impl RunState {
    pub fn new(
        config: BenchConfig,
        map_id: String,
        baseline: WindowStats,
        road_costs: HashMap<String, i64>,
    ) -> Self {
        let window = config.window_samples as usize;
        Self {
            config,
            map_id,
            baseline,
            road_costs,
            num_changes: 0,
            money_spent: 0,
            actions: Vec::new(),
            flow: FlowWindow::new(window),
            start: Instant::now(),
            end_reason: None,
        }
    }

    pub fn build_cost(&self, road_type: &str, length_m: f32) -> i64 {
        match self.road_costs.get(road_type) {
            Some(&c) => road_cost(c, length_m, &self.config),
            None => 0,
        }
    }

    pub fn record_mutation(&mut self, tool: &str, cost: i64) {
        self.num_changes += 1;
        self.money_spent += cost;
        self.actions.push(ActionEntry {
            seq: self.num_changes,
            tool: tool.to_string(),
            cost,
        });
    }

    pub fn push_flow(&mut self, sample: f64) {
        self.flow.push(sample);
        if self.flow.target_reached(self.config.flow_target) && self.end_reason.is_none() {
            self.end_reason = Some(EndReason::FlowTarget);
        }
    }

    pub fn seconds_remaining(&self) -> u64 {
        self.config
            .wall_clock_cap_secs
            .saturating_sub(self.start.elapsed().as_secs())
    }

    pub fn check_timeout(&mut self) {
        if self.seconds_remaining() == 0 && self.end_reason.is_none() {
            self.end_reason = Some(EndReason::Timeout);
        }
    }

    /// Agent-facing telemetry (spec §7): resources + goal, never the score.
    pub fn progress(&self, seconds_remaining: u64) -> Value {
        json!({
            "money_spent": self.money_spent,
            "num_changes": self.num_changes,
            "flow_current": self.flow.mean(),
            "flow_target": self.config.flow_target,
            "seconds_remaining": seconds_remaining,
        })
    }
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cd broker && cargo test --lib benchmark::state`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/benchmark/mod.rs broker/src/benchmark/state.rs
git commit -m "feat(benchmark): run state, cost tally, and progress envelope

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task C2: Window measurement against the bridge

**Files:**
- Create: `broker/src/benchmark/measure.rs`
- Modify: `broker/src/benchmark/mod.rs`

- [ ] **Step 1: Register + write the failing test (against the mock)**

Add `pub mod measure;` to `broker/src/benchmark/mod.rs`. Create `broker/src/benchmark/measure.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark::config::BenchConfig;
    use crate::bridge_client::BridgeClient;
    use crate::mock;

    async fn client() -> BridgeClient {
        let (addr, server) = mock::bind("127.0.0.1:0".parse().unwrap()).await;
        tokio::spawn(server);
        BridgeClient::new(format!("http://{addr}"))
    }

    #[tokio::test]
    async fn measures_window_stats_on_empty_city() {
        let c = client().await;
        let cfg = BenchConfig::default();
        // Mock flow on an empty city == 100 (100 - 0*5); active_vehicles == 0.
        let stats = measure_window(&c, &cfg).await.unwrap();
        assert_eq!(stats.flow_mean, 100.0);
        assert_eq!(stats.active_vehicles_mean, 0.0);
    }
}
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd broker && cargo test --lib benchmark::measure`
Expected: FAIL — `measure_window` undefined.

- [ ] **Step 3: Implement window measurement**

Prepend to `broker/src/benchmark/measure.rs`:

```rust
use crate::benchmark::config::BenchConfig;
use crate::benchmark::record::WindowStats;
use crate::bridge_client::{BridgeClient, BridgeError};

/// Step the sim across `window_ticks` in `window_samples` chunks, sampling
/// metrics after each chunk, and return the means (spec §3 baseline/final).
/// Leaves the sim paused (the mod re-pauses after a stepped `clock` op).
pub async fn measure_window(
    client: &BridgeClient,
    cfg: &BenchConfig,
) -> Result<WindowStats, BridgeError> {
    let samples = cfg.window_samples.max(1);
    let chunk = (cfg.window_ticks / samples).max(1);
    client.clock("pause", None, None).await?;

    let mut flow_sum = 0.0_f64;
    let mut veh_sum = 0.0_f64;
    let mut last_pop = 0u32;
    for _ in 0..samples {
        client.clock("step", Some(chunk), None).await?;
        let m = client.metrics().await?;
        flow_sum += m.traffic.flow_percent as f64;
        veh_sum += m.traffic.active_vehicles as f64;
        last_pop = m.population.total;
    }
    let n = samples as f64;
    Ok(WindowStats {
        flow_mean: flow_sum / n,
        active_vehicles_mean: veh_sum / n,
        population: last_pop,
    })
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cd broker && cargo test --lib benchmark::measure`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/benchmark/mod.rs broker/src/benchmark/measure.rs
git commit -m "feat(benchmark): measurement windows via the bridge clock

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task C3: Finalize — settle, final window, write artifacts

**Files:**
- Modify: `broker/src/benchmark/measure.rs`

- [ ] **Step 1: Write the failing test (against the mock)**

Add to the `tests` module in `broker/src/benchmark/measure.rs`:

```rust
    #[tokio::test]
    async fn finalize_writes_record_and_score() {
        use crate::benchmark::record::{EndReason, MapInfo, WindowStats};
        use crate::benchmark::state::RunState;
        use std::collections::HashMap;
        use std::sync::Arc;
        use tokio::sync::Mutex;

        let c = client().await;
        let cfg = BenchConfig::default();
        let baseline = WindowStats { flow_mean: 80.0, active_vehicles_mean: 0.0, population: 0 };
        let state = Arc::new(Mutex::new(RunState::new(
            cfg.clone(),
            "gridlock-v1".into(),
            baseline,
            HashMap::new(),
        )));
        state.lock().await.end_reason = Some(EndReason::Submit);

        let dir = std::env::temp_dir().join(format!("sb-finalize-{}", std::process::id()));
        let map = MapInfo { id: "gridlock-v1".into(), source: "test".into(), game_version: "1.21.1-f9".into() };
        finalize(&c, state, &dir, map, "t0".into(), "t1".into()).await.unwrap();

        let rec: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("run-record.json")).unwrap()).unwrap();
        assert_eq!(rec["end_reason"], "submit");
        assert_eq!(rec["started_at"], "t0");
        assert_eq!(rec["ended_at"], "t1");
        let score: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("score.json")).unwrap()).unwrap();
        assert!(score["score"].is_number());
        std::fs::remove_dir_all(&dir).ok();
    }
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd broker && cargo test --lib benchmark::measure::tests::finalize_writes_record_and_score`
Expected: FAIL — `finalize` undefined.

- [ ] **Step 3: Implement finalize**

Append to `broker/src/benchmark/measure.rs` (add the imports at the top of the file):

```rust
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::benchmark::record::{EndReason, MapInfo, RunRecord, Tally};
use crate::benchmark::score::score_run;
use crate::benchmark::state::RunState;

/// Settle the sim, measure the final window, compute the score, and write
/// `run-record.json` + `score.json` into `out_dir` (spec §3 steps 6–8, §10).
/// `started_at`/`ended_at` are stamped by the caller-supplied clock-free
/// timestamps via the mod tick; here we leave them as ISO strings the caller
/// can override. Returns once both files are written.
pub async fn finalize(
    client: &BridgeClient,
    state: Arc<Mutex<RunState>>,
    out_dir: &Path,
    map: MapInfo,
    started_at: String,
    ended_at: String,
) -> anyhow::Result<()> {
    let (cfg, baseline, tally, actions, end_reason) = {
        let s = state.lock().await;
        (
            s.config.clone(),
            s.baseline.clone(),
            Tally { num_changes: s.num_changes, money_spent: s.money_spent },
            s.actions.clone(),
            s.end_reason.unwrap_or(EndReason::Submit),
        )
    };

    // Settle, then measure the final window.
    let settle_cfg = BenchConfig { window_ticks: cfg.settle_ticks, window_samples: 1, ..cfg.clone() };
    measure_window(client, &settle_cfg).await?;
    let final_stats = measure_window(client, &cfg).await?;

    let record = RunRecord {
        schema_version: 1,
        map,
        started_at,
        ended_at,
        end_reason,
        baseline,
        final_stats,
        tally,
        actions,
    };
    let score = score_run(&record, &cfg);

    std::fs::create_dir_all(out_dir)?;
    std::fs::write(out_dir.join("run-record.json"), serde_json::to_string_pretty(&record)?)?;
    std::fs::write(out_dir.join("score.json"), serde_json::to_string_pretty(&score)?)?;
    Ok(())
}
```

(Timestamps are passed in by the caller — the `benchmark` subcommand stamps them from the wall clock in Task D2. The pure scoring never depends on them.)

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cd broker && cargo test --lib benchmark::measure`
Expected: PASS (both measure tests).

- [ ] **Step 5: Commit**

```bash
git add broker/src/benchmark/measure.rs
git commit -m "feat(benchmark): finalize writes run-record and score artifacts

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Phase D — Benchmark MCP server + wiring

### Task D1: BenchmarkServer tool surface + instrumentation

**Files:**
- Create: `broker/src/benchmark/server.rs`
- Modify: `broker/src/benchmark/mod.rs`

This server mirrors `Skyline` (delegating to the same `service::*` functions) but: (a) attaches `benchmark_progress` to every response, (b) counts mutations and computes their cost, (c) adds `submit_solution`, (d) **omits `reset_scenario`** (mid-session reload crashes; reset is the operator's main-menu step). That nets 12 tools (12 − reset + submit). The finalize-and-exit wiring lives in Task D2; here the server records the end reason and exposes state.

- [ ] **Step 1: Register + write the failing test**

Add `pub mod server;` to `broker/src/benchmark/mod.rs`. Create `broker/src/benchmark/server.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_twelve_tools_including_submit_excluding_reset() {
        let tools = BenchmarkServer::tool_router().list_all();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        assert!(names.contains(&"submit_solution"), "has submit_solution");
        assert!(names.contains(&"build_road"));
        assert!(names.contains(&"get_metrics"));
        // reset_scenario is intentionally NOT exposed in benchmark mode
        // (mid-session reload crashes; reset is the operator's main-menu step).
        assert!(!names.contains(&"reset_scenario"), "no reset_scenario");
        assert_eq!(tools.len(), 12);
    }

    #[test]
    fn attaches_progress_to_json_value() {
        use crate::benchmark::config::BenchConfig;
        use crate::benchmark::record::WindowStats;
        use crate::benchmark::state::RunState;
        use std::collections::HashMap;

        let state = RunState::new(
            BenchConfig::default(),
            "m".into(),
            WindowStats { flow_mean: 0.0, active_vehicles_mean: 0.0, population: 0 },
            HashMap::new(),
        );
        let merged = with_progress(serde_json::json!({"ok": true}), &state);
        assert_eq!(merged["ok"], true);
        assert!(merged["benchmark_progress"]["flow_target"].is_number());
    }
}
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd broker && cargo test --lib benchmark::server`
Expected: FAIL — `BenchmarkServer` / `with_progress` undefined.

- [ ] **Step 3: Implement the server**

Prepend to `broker/src/benchmark/server.rs`. The read tools attach progress; the mutating tools also tally cost. `build_road`/`upgrade_road` compute length (build: straight-line from args; upgrade: look up the segment's length in the live network) before charging cost.

```rust
use std::sync::Arc;

use base64::Engine;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, ErrorData, ServerHandler,
};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::benchmark::record::EndReason;
use crate::benchmark::state::RunState;
use crate::bridge_client::BridgeClient;
use crate::geometry::horizontal_distance;
use crate::service::{
    self, BuildRoadArgs, BulldozeArgs, ControlTimeArgs, GetMetricsArgs, RenderMapArgs,
    SetZoningArgs, UpgradeRoadArgs,
};

#[derive(Clone)]
pub struct BenchmarkServer {
    client: Arc<BridgeClient>,
    state: Arc<Mutex<RunState>>,
    tool_router: ToolRouter<Self>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct SubmitArgs {
    #[serde(default)]
    pub note: Option<String>,
}

/// Merge the agent-facing telemetry block into a JSON object result (spec §7).
pub fn with_progress(mut value: Value, state: &RunState) -> Value {
    if let Value::Object(ref mut map) = value {
        map.insert("benchmark_progress".into(), state.progress(state.seconds_remaining()));
    }
    value
}

impl BenchmarkServer {
    pub fn new(client: Arc<BridgeClient>, state: Arc<Mutex<RunState>>) -> Self {
        Self { client, state, tool_router: Self::tool_router() }
    }

    async fn finish(&self, value: Value) -> Result<CallToolResult, ErrorData> {
        let mut s = self.state.lock().await;
        s.check_timeout();
        let merged = with_progress(value, &s);
        Ok(CallToolResult::success(vec![Content::text(merged.to_string())]))
    }
}

#[tool_router]
impl BenchmarkServer {
    #[tool(description = "Summarise the city: tick, population, funds, traffic flow, network size.")]
    async fn get_city_overview(&self) -> Result<CallToolResult, ErrorData> {
        match service::get_city_overview(&self.client).await {
            Ok(v) => self.finish(v).await,
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
        }
    }

    #[tool(description = "Observe the playable area: network, buildings, zones, intersections, dead ends.")]
    async fn observe_area(&self) -> Result<CallToolResult, ErrorData> {
        match service::observe_area(&self.client).await {
            Ok(v) => self.finish(v).await,
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
        }
    }

    #[tool(description = "Get city metrics, optionally filtered to groups: traffic, economy, population, services.")]
    async fn get_metrics(&self, Parameters(args): Parameters<GetMetricsArgs>) -> Result<CallToolResult, ErrorData> {
        match service::get_metrics(&self.client, args).await {
            Ok(v) => {
                if let Some(flow) = v.get("traffic").and_then(|t| t.get("flow_percent")).and_then(|f| f.as_f64()) {
                    self.state.lock().await.push_flow(flow);
                }
                self.finish(v).await
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
        }
    }

    #[tool(description = "List the available road types (with construction cost).")]
    async fn list_road_types(&self) -> Result<CallToolResult, ErrorData> {
        match service::list_road_types(&self.client).await {
            Ok(v) => self.finish(v).await,
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
        }
    }

    #[tool(description = "List the available zone types.")]
    async fn list_zone_types(&self) -> Result<CallToolResult, ErrorData> {
        match service::list_zone_types(&self.client).await {
            Ok(v) => self.finish(v).await,
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
        }
    }

    #[tool(description = "Render the road network to a PNG image.")]
    async fn render_map(&self, Parameters(args): Parameters<RenderMapArgs>) -> Result<CallToolResult, ErrorData> {
        match service::render_map(&self.client, args).await {
            Ok(png) => {
                let data = base64::engine::general_purpose::STANDARD.encode(png);
                let progress = {
                    let s = self.state.lock().await;
                    s.progress(s.seconds_remaining())
                };
                Ok(CallToolResult::success(vec![
                    Content::image(data, "image/png".to_string()),
                    Content::text(serde_json::json!({ "benchmark_progress": progress }).to_string()),
                ]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
        }
    }

    #[tool(description = "Control simulation time: pause, resume, step, or set speed.")]
    async fn control_time(&self, Parameters(args): Parameters<ControlTimeArgs>) -> Result<CallToolResult, ErrorData> {
        match service::control_time(&self.client, args).await {
            Ok(v) => {
                // Sample flow after the agent advances the sim (spec §7).
                if let Ok(m) = self.client.metrics().await {
                    self.state.lock().await.push_flow(m.traffic.flow_percent as f64);
                }
                self.finish(v).await
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
        }
    }

    #[tool(description = "Build a road between two positions of a given road type.")]
    async fn build_road(&self, Parameters(args): Parameters<BuildRoadArgs>) -> Result<CallToolResult, ErrorData> {
        let length = horizontal_distance(args.from, args.to);
        let road_type = args.road_type.clone();
        match service::build_road(&self.client, args).await {
            Ok(v) => {
                if v.get("ok").and_then(|b| b.as_bool()) == Some(true) {
                    let cost = self.state.lock().await.build_cost(&road_type, length);
                    self.state.lock().await.record_mutation("build_road", cost);
                }
                self.finish(v).await
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
        }
    }

    #[tool(description = "Change an existing road segment's type. Validates the new road_type first.")]
    async fn upgrade_road(&self, Parameters(args): Parameters<UpgradeRoadArgs>) -> Result<CallToolResult, ErrorData> {
        let segment_id = args.segment;
        let road_type = args.road_type.clone();
        // Length of the existing segment, for cost.
        let length = self
            .client
            .network()
            .await
            .ok()
            .and_then(|n| n.segments.into_iter().find(|s| s.id == segment_id).map(|s| s.length))
            .unwrap_or(0.0);
        match service::upgrade_road(&self.client, args).await {
            Ok(v) => {
                if v.get("ok").and_then(|b| b.as_bool()) == Some(true) {
                    let cost = self.state.lock().await.build_cost(&road_type, length);
                    self.state.lock().await.record_mutation("upgrade_road", cost);
                }
                self.finish(v).await
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
        }
    }

    #[tool(description = "Remove a network segment, node, or building. target_type = segment | node | building.")]
    async fn bulldoze(&self, Parameters(args): Parameters<BulldozeArgs>) -> Result<CallToolResult, ErrorData> {
        match service::bulldoze(&self.client, args).await {
            Ok(v) => {
                if v.get("ok").and_then(|b| b.as_bool()) == Some(true) {
                    self.state.lock().await.record_mutation("bulldoze", 0);
                }
                self.finish(v).await
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
        }
    }

    #[tool(description = "Set zoning over a rectangular area. zone_type from list_zone_types.")]
    async fn set_zoning(&self, Parameters(args): Parameters<SetZoningArgs>) -> Result<CallToolResult, ErrorData> {
        match service::set_zoning(&self.client, args).await {
            Ok(v) => {
                if v.get("ok").and_then(|b| b.as_bool()) == Some(true) {
                    self.state.lock().await.record_mutation("set_zoning", 0);
                }
                self.finish(v).await
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
        }
    }

    #[tool(description = "Declare the run finished. The harness then scores the city. Call when satisfied.")]
    async fn submit_solution(&self, Parameters(_args): Parameters<SubmitArgs>) -> Result<CallToolResult, ErrorData> {
        {
            let mut s = self.state.lock().await;
            if s.end_reason.is_none() {
                s.end_reason = Some(EndReason::Submit);
            }
        }
        self.finish(serde_json::json!({ "ok": true, "submitted": true })).await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for BenchmarkServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "SkylineBench benchmark: improve city traffic, then call submit_solution. \
             Each response includes benchmark_progress (resources + goal).",
        )
    }
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cd broker && cargo test --lib benchmark::server`
Expected: PASS (both tests). Then run the full suite: `cargo test` — all green.

- [ ] **Step 5: Commit**

```bash
git add broker/src/benchmark/mod.rs broker/src/benchmark/server.rs
git commit -m "feat(benchmark): instrumented MCP server with submit_solution

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task D2: `benchmark` subcommand wiring

**Files:**
- Modify: `broker/src/main.rs`
- Modify: `broker/src/benchmark/mod.rs` (re-exports)

- [ ] **Step 1: Re-export the pieces main needs**

In `broker/src/benchmark/mod.rs`, add re-exports under the `pub mod` lines:

```rust
pub use config::BenchConfig;
pub use measure::{finalize, measure_window};
pub use record::{MapInfo, WindowStats};
pub use server::BenchmarkServer;
pub use state::RunState;
```

- [ ] **Step 2: Add the subcommand + run loop**

In `broker/src/main.rs`, add to the `Command` enum:

```rust
    /// Run a benchmark session: serve MCP (instrumented) against the mod and
    /// score the run when the agent finishes.
    Benchmark {
        #[arg(long, default_value = "http://127.0.0.1:8787")]
        mod_url: String,
        #[arg(long)]
        map: String,
        #[arg(long, default_value = "test")]
        map_source: String,
        #[arg(long)]
        out: std::path::PathBuf,
    },
```

Add the match arm in `main()`:

```rust
        Command::Benchmark { mod_url, map, map_source, out } => {
            use std::collections::HashMap;
            use std::sync::Arc;
            use tokio::sync::Mutex;
            use skylinebench::benchmark::{
                finalize, measure_window, BenchConfig, BenchmarkServer, MapInfo, RunState,
            };
            use skylinebench::bridge_client::BridgeClient;
            use rmcp::ServiceExt;

            // Epoch-seconds timestamp string — dependency-free (no chrono).
            fn epoch_secs() -> String {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs().to_string())
                    .unwrap_or_default()
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

            eprintln!("benchmark: measuring baseline…");
            let baseline = measure_window(&client, &cfg).await?;
            eprintln!("benchmark: baseline flow {:.1}%", baseline.flow_mean);

            let state = Arc::new(Mutex::new(RunState::new(
                cfg.clone(),
                map.clone(),
                baseline,
                road_costs,
            )));

            // Background watcher: when an out fires, finalize then exit (closing
            // stdio ends the Claude Code session). Spec §11 run-end mechanism.
            let watch_client = client.clone();
            let watch_state = state.clone();
            let game_version = health.game_version.clone();
            let started_at = started_at.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let ended = {
                        let mut s = watch_state.lock().await;
                        s.check_timeout();
                        s.end_reason.is_some()
                    };
                    if ended {
                        let map_info = MapInfo {
                            id: map.clone(),
                            source: map_source.clone(),
                            game_version: game_version.clone(),
                        };
                        let ended_at = epoch_secs();
                        if let Err(e) = finalize(&watch_client, watch_state.clone(), &out, map_info, started_at.clone(), ended_at).await {
                            eprintln!("benchmark: finalize error: {e}");
                        } else {
                            eprintln!("benchmark: wrote artifacts to {}", out.display());
                        }
                        std::process::exit(0);
                    }
                }
            });

            let server = BenchmarkServer::new(client, state)
                .serve((tokio::io::stdin(), tokio::io::stdout()))
                .await?;
            server.waiting().await?;
        }
```

- [ ] **Step 3: Build + full suite**

Run: `cd broker && cargo build && cargo test`
Expected: compiles; all tests pass. (No new unit test here — this is process wiring exercised end-to-end in the live verify step. The watcher/finalize logic it calls is already covered by Task C3.)

- [ ] **Step 4: Smoke-test the baseline against the mock**

Run (two terminals, or background the mock):
```bash
cd broker
cargo run -- mock &              # mock mod on 127.0.0.1:8787
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | \
  cargo run -- benchmark --map smoke --out /tmp/sb-smoke 2>/tmp/sb-bench.log
kill %1
grep -q "measuring baseline" /tmp/sb-bench.log && echo "OK: baseline ran"
```
Expected: the log shows `measuring baseline…` and a baseline flow line; `tools/list` returns 12 tools. (The session won't auto-finalize here because no out fires from a single `tools/list`.)

- [ ] **Step 5: Commit**

```bash
git add broker/src/main.rs broker/src/benchmark/mod.rs
git commit -m "feat(broker): benchmark subcommand — baseline, serve, finalize-on-out

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Phase E — Runner, prompt, transcript, docs

### Task E1: `render-transcript` subcommand (pure renderer)

**Files:**
- Create: `broker/src/benchmark/transcript.rs`
- Modify: `broker/src/benchmark/mod.rs`
- Modify: `broker/src/main.rs`

- [ ] **Step 1: Register + write the failing test**

Add `pub mod transcript;` and `pub use transcript::render_transcript;` to `broker/src/benchmark/mod.rs`. Create `broker/src/benchmark/transcript.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_assistant_text_and_tool_calls() {
        // Two stream-json lines: an assistant message with text + a tool_use,
        // and a user message carrying a tool_result.
        let jsonl = concat!(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Building a bypass."},{"type":"tool_use","name":"build_road","input":{"road_type":"Highway"}}]}}"#,
            "\n",
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":[{"type":"text","text":"{\"ok\":true}"}]}]}}"#,
            "\n",
        );
        let md = render_transcript(jsonl);
        assert!(md.contains("Building a bypass."), "assistant text: {md}");
        assert!(md.contains("build_road"), "tool name: {md}");
        assert!(md.contains("Highway"), "tool input: {md}");
        assert!(md.contains("ok"), "tool result: {md}");
    }

    #[test]
    fn skips_malformed_lines() {
        let md = render_transcript("not json\n{}\n");
        assert!(md.is_empty() || !md.contains("panic"));
    }
}
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd broker && cargo test --lib benchmark::transcript`
Expected: FAIL — `render_transcript` undefined.

- [ ] **Step 3: Implement the renderer**

Prepend to `broker/src/benchmark/transcript.rs`:

```rust
use serde_json::Value;

/// Render Claude Code `stream-json` (one JSON object per line) into a readable
/// markdown transcript (spec §11). Unknown/malformed lines are skipped.
pub fn render_transcript(jsonl: &str) -> String {
    jsonl
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(render_event)
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_event(event: Value) -> Option<String> {
    let kind = event.get("type")?.as_str()?;
    let content = event.get("message")?.get("content")?.as_array()?;
    let role = match kind {
        "assistant" => "Assistant",
        "user" => "Tool result",
        _ => return None,
    };
    let blocks: Vec<String> = content.iter().filter_map(render_block).collect();
    if blocks.is_empty() {
        return None;
    }
    Some(format!("### {role}\n\n{}", blocks.join("\n\n")))
}

fn render_block(block: &Value) -> Option<String> {
    match block.get("type")?.as_str()? {
        "text" => Some(block.get("text")?.as_str()?.to_string()),
        "tool_use" => {
            let name = block.get("name")?.as_str()?;
            let input = block.get("input").cloned().unwrap_or(Value::Null);
            Some(format!("**→ {name}**\n```json\n{}\n```", input))
        }
        "tool_result" => {
            let inner = block.get("content")?.as_array()?;
            let text: String = inner
                .iter()
                .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n");
            Some(format!("```\n{text}\n```"))
        }
        _ => None,
    }
}
```

- [ ] **Step 4: Add the subcommand**

In `broker/src/main.rs`, add to `Command`:

```rust
    /// Render a captured stream-json transcript to readable markdown.
    RenderTranscript {
        #[arg(long)]
        input: std::path::PathBuf,
        #[arg(long)]
        out: std::path::PathBuf,
    },
```

And the match arm:

```rust
        Command::RenderTranscript { input, out } => {
            let jsonl = std::fs::read_to_string(&input)?;
            std::fs::write(&out, skylinebench::benchmark::render_transcript(&jsonl))?;
        }
```

- [ ] **Step 5: Run tests + build**

Run: `cd broker && cargo test --lib benchmark::transcript && cargo build`
Expected: PASS + compiles.

- [ ] **Step 6: Commit**

```bash
git add broker/src/benchmark/mod.rs broker/src/benchmark/transcript.rs broker/src/main.rs
git commit -m "feat(benchmark): stream-json transcript renderer + subcommand

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task E2: The agent task prompt

**Files:**
- Create: `benchmark/prompt.md`

- [ ] **Step 1: Write the prompt**

Create `benchmark/prompt.md` (spec §11 — states the goal, tools, outs, guard, and what scores well; never reveals weights or the composite score):

```markdown
You are competing in SkylineBench: improve the traffic in this Cities: Skylines city.

You have MCP tools to observe and modify the city:
- Observe (free, unlimited, unscored): `get_city_overview`, `observe_area`, `get_metrics`, `list_road_types`, `list_zone_types`, `render_map`.
- Modify (these are your "changes"): `build_road`, `bulldoze`, `upgrade_road`, `set_zoning`.
- Time: `control_time` (pause / resume / step / speed). Build while paused, then `step` to let traffic respond before measuring.
- Finish: `submit_solution` when you are satisfied.

Goal: raise city-wide `traffic.flow_percent` (higher = freer-flowing) as much as you can.

How you are scored (you will NOT see your score during the run):
- Most of the score is how much you improve flow.
- You are rewarded for spending less money and for making fewer modifying actions. Reads are free — observe as much as you like.
- INVALID RUN: if you reduce the number of active vehicles too far (you must not "fix" traffic by depopulating the city), the run scores zero. Keep the city alive.

The run ends when any of these happens:
1. You call `submit_solution`.
2. Flow reaches the target shown as `flow_target` in `benchmark_progress`.
3. A 3-hour time limit is reached.

Every tool response includes a `benchmark_progress` block (money spent, changes made, current flow vs target, seconds remaining). Use it to decide when to stop — a good-enough solution submitted early beats an expensive one.

Work method: observe the network and metrics, find the congestion, make targeted road/zoning changes, step the simulation to let traffic respond, re-measure, and iterate. When the gains plateau, `submit_solution`.
```

- [ ] **Step 2: Commit**

```bash
git add benchmark/prompt.md
git commit -m "docs(benchmark): agent task prompt

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task E3: The launcher script

**Files:**
- Create: `benchmark/run.sh`

- [ ] **Step 1: Write the script**

Create `benchmark/run.sh` (headless by default; `--watch` for interactive; `DRY_RUN=1` prints the command for testing without `claude` installed). The script spawns `broker benchmark` as the stdio MCP server via a generated config:

```bash
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
```

- [ ] **Step 2: Make it executable and lint it**

```bash
chmod +x benchmark/run.sh
shellcheck benchmark/run.sh || true   # shellcheck optional; fix any errors it flags
```
Expected: no fatal shellcheck errors (warnings about word-splitting on the cargo-fallback line are acceptable; the release binary is the normal path).

- [ ] **Step 3: Verify the dry run prints a sane command**

```bash
DRY_RUN=1 ./benchmark/run.sh --map gridlock-v1 | tee /tmp/sb-dry.txt
grep -q -- "--mcp-config" /tmp/sb-dry.txt && grep -q "submit_solution" /tmp/sb-dry.txt && echo "OK: command shape"
DRY_RUN=1 ./benchmark/run.sh --map gridlock-v1 --watch | grep -q -- "--output-format" && echo "ERROR: watch should not stream-json" || echo "OK: watch mode interactive"
```
Expected: headless command contains `--mcp-config`, the allowed tools, and `--output-format stream-json`; watch mode omits `--output-format`.

- [ ] **Step 4: Commit**

```bash
git add benchmark/run.sh
git commit -m "feat(benchmark): run.sh launcher with headless + watch + transcript capture

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

### Task E4: Operator docs + maps directory

**Files:**
- Create: `benchmark/README.md`
- Create: `benchmark/maps/README.md`

- [ ] **Step 1: Write the maps README**

Create `benchmark/maps/README.md`:

```markdown
# Benchmark maps

Pin the bad-traffic save here (the `.crp` file) and document it below.

Requirements (spec §9):
- The save must have **Unlimited Money** and **Unlock All** enabled, so cash never
  blocks building and all tiles/features are available. Money is only a *scoring*
  penalty (the broker computes spend from each action; it never reads in-game funds).
- Record the source and the game version it was made on.

## Pinned saves
| id | file | source | game version | notes |
|----|------|--------|--------------|-------|
| gridlock-v1 | gridlock-v1.crp | (fill in) | 1.21.1-f9 | (what makes traffic bad) |
```

- [ ] **Step 2: Write the operator README**

Create `benchmark/README.md`:

```markdown
# SkylineBench benchmark

Score a Claude Code agent on improving traffic in a bad-traffic city.

## Per-run steps (spec §2, §3)
1. Launch Cities: Skylines and load the benchmark save from the **main menu**
   (never reload mid-session — it crashes). Confirm the city is loaded:
   `curl -s http://127.0.0.1:8787/health` shows `"city_loaded":true`.
2. Build the broker once: `cargo build --release --manifest-path broker/Cargo.toml`.
3. Run: `./benchmark/run.sh --map gridlock-v1`
   - The broker measures a baseline, the agent works, and on any run-end
     condition (submit / flow target / 3h) the harness settles, measures the
     final window, scores, and writes artifacts.
   - Use `--watch` to observe an interactive session instead of headless.
4. Read the results in `benchmark/runs/<timestamp>/`:
   - `score.json` — the composite score and per-term breakdown.
   - `run-record.json` — baseline/final stats, tally, per-action cost log.
   - `transcript.md` / `transcript.jsonl` — what the agent did, for diagnosing a
     poor score (harness issue vs prompt issue).

## Scoring (spec §4)
`score = 0.60·norm(Δflow) + 0.20·(1−norm(money)) + 0.20·(1−norm(changes))`,
zeroed if final active vehicles drop below 90% of baseline. Constants live in
`broker/src/benchmark/config.rs` and are tuned against the map.
```

- [ ] **Step 3: Commit**

```bash
git add benchmark/README.md benchmark/maps/README.md
git commit -m "docs(benchmark): operator + maps README

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Final verification

- [ ] **Full broker suite green:** `cd broker && cargo test` — all prior tests plus the new `benchmark::*` tests pass.
- [ ] **Mod tests green:** `cd mod && xbuild test/Tests.csproj /p:Configuration=Debug && mono test/bin/Debug/Tests.exe`.
- [ ] **Mod builds against the game:** `cd mod && ./build.sh` compiles clean (confirms `NetInfo.m_constructionCost` resolves).
- [ ] **Live verify (operator, with the game running a loaded city):**
  - `curl -s 127.0.0.1:8787/road-types` shows `{name, construction_cost}` objects.
  - `./benchmark/run.sh --map <id> --watch` runs an agent end-to-end; on `submit_solution` the harness writes `run-record.json`, `score.json`, `transcript.md` under `benchmark/runs/<id>/`.
  - Sanity-check and **calibrate** the §6 constants (`target_gain`, `budget`, `change_cap`, `cost_base_length_m`, window/settle ticks) against this first real run, then re-run.
- [ ] **Finishing the branch:** once verified, use superpowers:finishing-a-development-branch to merge `skylinebench-phase2-benchmark` to `main` locally.
