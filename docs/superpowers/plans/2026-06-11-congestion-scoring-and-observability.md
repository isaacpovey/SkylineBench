# Congestion Scoring & Consequence Observability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace global-flow scoring with congested-road-meters scoring, give the agent neutral visibility into city-health consequences (population, abandonment, building frontage), and make road builds fail with real reasons instead of silently bypassing the game's placement rules.

**Architecture:** The C# mod (`mod/src`) gains new read fields (segment length, abandoned-building count), a frontage counter, and pre-creation placement validation with precise error codes plus a `/action/validate-road` dry-run endpoint. The Rust broker (`broker/src`) gains a pure congestion-math module, a rewritten score/config/record schema (v2), rolling congestion telemetry in `benchmark_progress`, and game-check enrichment of `apply_plan validate_only`. The agent prompt is rewritten for the new metric.

**Tech Stack:** Rust (tokio/rmcp/serde, `cargo test` with an axum mock bridge), C# .NET 3.5 mod built with Mono msbuild/xbuild against Cities: Skylines DLLs (`mod/test` has a game-free test exe).

**Spec:** `docs/superpowers/specs/2026-06-10-congestion-scoring-and-observability-design.md`

**One deliberate deviation from the spec:** §4 named the game's `NetTool` test-mode as the primary validation API with `NetAI.CheckBuildPosition` as fallback. This plan implements explicit checks instead (out-of-area via `GameAreaManager`, slope via `NetInfo.m_maxSlope`, building collision via capsule-vs-footprint geometry). Rationale: the explicit checks are fully specifiable and reviewable here, run no vanilla rules we don't want (money, road-spacing), and don't depend on an 18-parameter undocumented signature. The error-code surface is exactly what the spec requires, minus `HEIGHT_LIMIT` (only meaningful for elevated/tunnel placement, deferred). Flag this to the operator at review.

**Dependency note:** Broker changes land first and tolerate an old mod (new wire fields use serde defaults). The new scoring only produces nonzero `congested_meters` once the mod is rebuilt and deployed (Task 11) — until then live runs would abort at baseline with "zero congested meters", which is the intended guard. Mock-based tests are unaffected.

---

## File Structure

| File | Change |
| --- | --- |
| `broker/src/contract.rs` | wire types: `SegmentLoad.length`, `ServiceMetrics.abandoned_buildings`, new `ActionError` variants, `ActionResult.{zoned_buildings_fronting, colliding_buildings}` |
| `broker/src/mock.rs` | emit new fields; `/action/validate-road` route |
| `broker/src/benchmark/congestion.rs` | **new** — pure congested-meters math (instant + window-mean) |
| `broker/src/benchmark/config.rs` | `w_congestion`, `congestion_threshold`, `congestion_end_ratio`, `pop_guard_ratio`, `max_step_days: 7`; drop `w_flow`/`target_gain`/`flow_target`/`guard_ratio` |
| `broker/src/benchmark/record.rs` | schema v2: `WindowStats.congested_meters`, `EndReason::CongestionTarget`, `ScoreNorms.congestion`, `Score.flow_gain` |
| `broker/src/benchmark/score.rs` | new formula + population guard |
| `broker/src/benchmark/measure.rs` | per-segment window accumulation; finalize bails on zero baseline congestion |
| `broker/src/benchmark/flow_window.rs` → `rolling_window.rs` | rename `FlowWindow` → `RollingWindow`; drop `target_reached` |
| `broker/src/benchmark/state.rs` | `observe_metrics`, congestion window, new `progress()` fields, congestion end condition |
| `broker/src/benchmark/server.rs` | wire `observe_metrics`, typed `get_metrics`, 7-day cap text, `validate_only` game checks |
| `broker/src/service.rs` | extract `metrics_value(m, groups)` from `get_metrics` |
| `broker/src/bridge_client.rs` | `validate_road()` |
| `mod/src/dto/Dtos.cs` | `SegmentLoadDto.Length`, `MetricsDto.AbandonedBuildings`, `ActionResultDto.{ZonedBuildingsFronting, CollidingBuildings}` |
| `mod/src/bridge/GameReads.cs` | read segment length + abandoned count |
| `mod/src/json/Serialize.cs` | serialize the new fields |
| `mod/src/bridge/Frontage.cs` | **new** — zoned-buildings-near-segment counter |
| `mod/src/bridge/BuildValidator.cs` | **new** — placement checks (area/slope/collision) |
| `mod/src/bridge/ErrorCode.cs` | `OBJECT_COLLISION`, `SLOPE_TOO_STEEP`, `OUT_OF_AREA`, `TOO_MANY_CONNECTIONS`, `NET_BUFFER_FULL` |
| `mod/src/bridge/GameActions.cs` | validation + frontage wiring; honest failure codes; `ValidateRoad` |
| `mod/src/http/Router.cs`, `Handlers.cs` | `/action/validate-road` |
| `mod/test/SerializeTests.cs` | tests for new serialization |
| `benchmark/prompt.md` | rewritten scoring/goal/telemetry text |
| `benchmark/experiments/null_control.py` | length-weighted congestion summary at threshold 0.7 |

Commands used throughout:
- Broker tests: `cd /Users/isaac.povey/Documents/personal/SkylineBench/broker && cargo test`
- Mod tests: `cd /Users/isaac.povey/Documents/personal/SkylineBench/mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe`
- Mod build+install: `cd /Users/isaac.povey/Documents/personal/SkylineBench/mod && ./build.sh`

Every commit message ends with:
```
Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
```

---

### Task 1: Broker wire types (contract + mock)

**Files:**
- Modify: `broker/src/contract.rs`
- Modify: `broker/src/mock.rs`

- [ ] **Step 1: Write failing tests in `contract.rs`**

Append inside `mod tests` in `broker/src/contract.rs`:

```rust
#[test]
fn segment_load_defaults_length_for_old_mod_payloads() {
    let l: SegmentLoad = serde_json::from_str(r#"{"segment_id": 3, "density": 0.9}"#).unwrap();
    assert_eq!(l.length, 0.0);
    let l: SegmentLoad =
        serde_json::from_str(r#"{"segment_id": 3, "density": 0.9, "length": 52.5}"#).unwrap();
    assert_eq!(l.length, 52.5);
}

#[test]
fn service_metrics_default_abandoned_buildings() {
    let s: ServiceMetrics = serde_json::from_str(r#"{"happiness": 80}"#).unwrap();
    assert_eq!(s.abandoned_buildings, 0);
}

#[test]
fn action_result_defaults_new_consequence_fields() {
    let r: ActionResult = serde_json::from_str(r#"{"ok": true}"#).unwrap();
    assert_eq!(r.zoned_buildings_fronting, None);
    assert!(r.colliding_buildings.is_empty());
}

#[test]
fn new_action_errors_serialize_screaming_snake() {
    assert_eq!(serde_json::to_string(&ActionError::ObjectCollision).unwrap(), "\"OBJECT_COLLISION\"");
    assert_eq!(serde_json::to_string(&ActionError::SlopeTooSteep).unwrap(), "\"SLOPE_TOO_STEEP\"");
    assert_eq!(serde_json::to_string(&ActionError::OutOfArea).unwrap(), "\"OUT_OF_AREA\"");
    assert_eq!(serde_json::to_string(&ActionError::TooManyConnections).unwrap(), "\"TOO_MANY_CONNECTIONS\"");
    assert_eq!(serde_json::to_string(&ActionError::NetBufferFull).unwrap(), "\"NET_BUFFER_FULL\"");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd broker && cargo test contract`
Expected: compile errors (`length`, `abandoned_buildings`, variants missing).

- [ ] **Step 3: Implement the wire-type changes**

In `broker/src/contract.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentLoad {
    pub segment_id: u32,
    pub density: f32,
    /// Metres. Defaults to 0 for payloads from a mod predating the field.
    #[serde(default)]
    pub length: f32,
}
```

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceMetrics {
    pub happiness: u8,
    /// Buildings flagged Abandoned — a lagging signal that parts of the city
    /// have lost road access or services.
    #[serde(default)]
    pub abandoned_buildings: u32,
}
```

Extend `ActionError` (keep existing variants; `Collision` stays for legacy paths):

```rust
pub enum ActionError {
    Collision,
    ObjectCollision,
    SlopeTooSteep,
    OutOfArea,
    TooManyConnections,
    NetBufferFull,
    InsufficientFunds,
    OutOfBounds,
    InvalidPrefab,
    SegmentTooLong,
    DegenerateSegment,
    InvalidArgs,
    Unknown,
}
```

Extend `ActionResult`:

```rust
pub struct ActionResult {
    pub ok: bool,
    #[serde(default)]
    pub created_nodes: Vec<u32>,
    #[serde(default)]
    pub created_segments: Vec<u32>,
    #[serde(default)]
    pub snapped_nodes: Vec<u32>,
    #[serde(default)]
    pub destroyed: Vec<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reason: Option<ActionError>,
    /// Zoned (RCIO) buildings fronting the affected segment — a neutral fact
    /// for the agent, not a warning. None when the mod didn't compute it.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub zoned_buildings_fronting: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub colliding_buildings: Vec<u32>,
}
```

- [ ] **Step 4: Fix `mock.rs` to compile and emit the new fields**

In `broker/src/mock.rs`:
- `metrics()`: `SegmentLoad { segment_id: sg.id, density: (sg.id % 10) as f32 / 10.0, length: sg.length }` and `ServiceMetrics { happiness: 75, abandoned_buildings: 0 }`.
- Every `ActionResult { ... }` literal (in `build_road`, `bulldoze` ×2, `upgrade_road` ×2, `set_zone` ×2): add `zoned_buildings_fronting: None, colliding_buildings: vec![]`. In `build_road`'s success result use `zoned_buildings_fronting: Some(0)` (the mock has no buildings).

Other `ActionResult` construction sites the compiler flags (none expected outside mock) get the same two defaults.

- [ ] **Step 5: Run the full suite**

Run: `cd broker && cargo test`
Expected: PASS (contract tests green; everything else unchanged).

- [ ] **Step 6: Commit**

```bash
git add broker/src/contract.rs broker/src/mock.rs
git commit -m "feat(broker): wire types for congestion scoring and consequence facts"
```

---

### Task 2: Congestion math module

**Files:**
- Create: `broker/src/benchmark/congestion.rs`
- Modify: `broker/src/benchmark/mod.rs` (add `pub mod congestion;`)

- [ ] **Step 1: Write the module with failing tests**

`broker/src/benchmark/congestion.rs`:

```rust
use std::collections::HashMap;

use crate::contract::SegmentLoad;

/// Congested road-meters in one metrics sample: total length of segments at or
/// above the density threshold (spec 2026-06-10 §2.1). Used for the rolling
/// progress value; measurement windows use [`WindowAccum`] instead.
pub fn instant_congested_meters(loads: &[SegmentLoad], threshold: f64) -> f64 {
    loads
        .iter()
        .filter(|l| f64::from(l.density) >= threshold)
        .map(|l| f64::from(l.length))
        .sum()
}

#[derive(Debug, Default)]
struct SegmentAccum {
    density_sum: f64,
    samples: u32,
    length: f64,
}

/// Accumulates per-segment densities across a measurement window so congested
/// meters are computed from each segment's MEAN density over the window, not
/// per-sample flickers. A segment absent from a sample (e.g. bulldozed) only
/// contributes the samples where it exists.
#[derive(Debug, Default)]
pub struct WindowAccum {
    per_segment: HashMap<u32, SegmentAccum>,
}

impl WindowAccum {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, loads: &[SegmentLoad]) {
        for l in loads {
            let e = self.per_segment.entry(l.segment_id).or_default();
            e.density_sum += f64::from(l.density);
            e.samples += 1;
            e.length = f64::from(l.length);
        }
    }

    pub fn congested_meters(&self, threshold: f64) -> f64 {
        self.per_segment
            .values()
            .filter(|s| s.samples > 0 && s.density_sum / f64::from(s.samples) >= threshold)
            .map(|s| s.length)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(id: u32, density: f32, length: f32) -> SegmentLoad {
        SegmentLoad { segment_id: id, density, length }
    }

    #[test]
    fn instant_sums_lengths_at_or_above_threshold() {
        let loads = vec![load(1, 0.9, 100.0), load(2, 0.7, 50.0), load(3, 0.69, 999.0)];
        assert_eq!(instant_congested_meters(&loads, 0.7), 150.0);
    }

    #[test]
    fn instant_on_empty_is_zero() {
        assert_eq!(instant_congested_meters(&[], 0.7), 0.0);
    }

    #[test]
    fn window_uses_mean_density_per_segment() {
        let mut w = WindowAccum::new();
        // Segment 1 flickers above the threshold once but its mean is 0.6.
        w.push(&[load(1, 0.9, 100.0), load(2, 0.8, 40.0)]);
        w.push(&[load(1, 0.3, 100.0), load(2, 0.8, 40.0)]);
        assert_eq!(w.congested_meters(0.7), 40.0);
    }

    #[test]
    fn window_handles_segment_absent_from_some_samples() {
        let mut w = WindowAccum::new();
        w.push(&[load(7, 0.8, 60.0)]);
        w.push(&[]); // segment bulldozed mid-window
        assert_eq!(w.congested_meters(0.7), 60.0, "mean over existing samples only");
    }
}
```

In `broker/src/benchmark/mod.rs` add `pub mod congestion;` alongside the other modules.

- [ ] **Step 2: Run tests**

Run: `cd broker && cargo test congestion`
Expected: PASS (4 tests).

- [ ] **Step 3: Commit**

```bash
git add broker/src/benchmark/congestion.rs broker/src/benchmark/mod.rs
git commit -m "feat(broker): congested-meters math (instant + window-mean)"
```

---

### Task 3: Scoring core sweep (config, record, score, measure, state, server)

This is one atomic change — the config/record renames ripple through every benchmark module, so the task compiles only at the end. Work through the steps in order and let the compiler enumerate remaining sites. One commit.

**Files:**
- Modify: `broker/src/benchmark/config.rs`
- Modify: `broker/src/benchmark/record.rs`
- Modify: `broker/src/benchmark/score.rs`
- Modify: `broker/src/benchmark/measure.rs`
- Rename: `broker/src/benchmark/flow_window.rs` → `broker/src/benchmark/rolling_window.rs`
- Modify: `broker/src/benchmark/state.rs`
- Modify: `broker/src/benchmark/server.rs`
- Modify: `broker/src/benchmark/mod.rs`
- Modify: `broker/src/service.rs` (extract `metrics_value`)

- [ ] **Step 1: Rewrite `config.rs`**

```rust
use serde::{Deserialize, Serialize};

/// Benchmark scoring + protocol constants (spec 2026-06-10 §6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchConfig {
    pub w_congestion: f64,
    pub w_money: f64,
    pub w_changes: f64,
    /// Segment density (0..1) at or above which a segment counts as congested.
    pub congestion_threshold: f64,
    pub budget: f64,
    pub change_cap: f64,
    /// Early-success: the run ends when windowed congested meters fall to this
    /// fraction of the baseline.
    pub congestion_end_ratio: f64,
    pub window_ticks: u32,
    pub settle_ticks: u32,
    pub window_samples: u32,
    pub wall_clock_cap_secs: u64,
    /// Invalid run when final population < pop_guard_ratio × baseline. The
    /// null-control run (benchmark/experiments) showed a natural population
    /// floor of ~87% of peak, so 0.8 tolerates lifecycle waves.
    pub pop_guard_ratio: f64,
    /// Length (m) over which a road's `construction_cost` is charged once
    /// (cost = construction_cost · length / cost_base_length_m). Calibrated.
    pub cost_base_length_m: f64,
    /// Ticks in one in-game calendar day (the CS1 game week is 4096 sim
    /// frames, so a day is 4096/7 ≈ 585). `control_time` steps default to
    /// one day and are capped at `max_step_days`.
    pub day_ticks: u32,
    pub max_step_days: u32,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            w_congestion: 0.60,
            w_money: 0.20,
            w_changes: 0.20,
            congestion_threshold: 0.7,
            budget: 10_000_000.0,
            change_cap: 300.0,
            congestion_end_ratio: 0.05,
            window_ticks: 2048,
            settle_ticks: 8192,
            window_samples: 8,
            wall_clock_cap_secs: 10_800,
            pop_guard_ratio: 0.8,
            cost_base_length_m: 64.0,
            day_ticks: 585,
            max_step_days: 7,
        }
    }
}

impl BenchConfig {
    pub fn max_step_ticks(&self) -> u32 {
        self.day_ticks * self.max_step_days
    }
}
```

Replace the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_weights_sum_to_one() {
        let c = BenchConfig::default();
        let sum = c.w_congestion + c.w_money + c.w_changes;
        assert!((sum - 1.0).abs() < 1e-9, "weights sum to {sum}");
    }

    #[test]
    fn congestion_dominates_and_guards_are_calibrated() {
        let c = BenchConfig::default();
        assert!(c.w_congestion > c.w_money && c.w_congestion > c.w_changes);
        assert_eq!(c.congestion_threshold, 0.7);
        assert_eq!(c.congestion_end_ratio, 0.05);
        assert_eq!(c.pop_guard_ratio, 0.8);
        assert_eq!(c.wall_clock_cap_secs, 10_800);
    }

    #[test]
    fn step_cap_is_seven_days() {
        let c = BenchConfig::default();
        assert_eq!(c.day_ticks, 585);
        assert_eq!(c.max_step_days, 7);
        assert_eq!(c.max_step_ticks(), 4095);
    }

    #[test]
    fn default_resource_envelope() {
        let c = BenchConfig::default();
        assert_eq!(c.budget, 10_000_000.0);
        assert_eq!(c.change_cap, 300.0);
    }
}
```

- [ ] **Step 2: Update `record.rs` to schema v2**

```rust
pub const SCHEMA_VERSION: u32 = 2;
```

- `EndReason::FlowTarget` → `EndReason::CongestionTarget` (serde stays snake_case → `"congestion_target"`); update its doc comment and the serialization test.
- `WindowStats` gains `pub congested_meters: f64,` (after `population`).
- `ScoreNorms`: rename `flow` → `congestion`.
- `Score` gains `pub flow_gain: f64,` documented as an unweighted diagnostic (final flow mean − baseline flow mean).
- Update the test fixtures in this file: every `WindowStats { ... }` literal gains `congested_meters` (use `500.0` baseline / `100.0` final), `schema_version: 1` → `SCHEMA_VERSION`, the `end_reason_serializes_snake` test asserts `"congestion_target"` instead of `"flow_target"`.

- [ ] **Step 3: Rewrite `score.rs`**

```rust
use crate::benchmark::config::BenchConfig;
use crate::benchmark::record::{RunRecord, Score, ScoreNorms};

fn clamp01(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

pub fn score_run(record: &RunRecord, cfg: &BenchConfig) -> Score {
    debug_assert!(
        cfg.budget > 0.0 && cfg.change_cap > 0.0,
        "BenchConfig normalization denominators must be positive"
    );
    let baseline_cm = record.baseline.congested_meters;
    let final_cm = record.final_stats.congested_meters;
    let norm = ScoreNorms {
        congestion: if baseline_cm > 0.0 {
            clamp01((baseline_cm - final_cm) / baseline_cm)
        } else {
            0.0
        },
        money: clamp01(record.tally.money_spent as f64 / cfg.budget),
        changes: clamp01(record.tally.num_changes as f64 / cfg.change_cap),
    };
    let weighted = ScoreNorms {
        congestion: cfg.w_congestion * norm.congestion,
        money: cfg.w_money * (1.0 - norm.money),
        changes: cfg.w_changes * (1.0 - norm.changes),
    };
    // Two ways to be invalid: depopulating the city (the anti-cheat guard) or
    // a map with no congestion to fix (scores would be meaningless).
    let pop_guard_failed = (record.final_stats.population as f64)
        < cfg.pop_guard_ratio * record.baseline.population as f64;
    let invalid = pop_guard_failed || baseline_cm <= 0.0;
    let score = if invalid {
        0.0
    } else {
        weighted.congestion + weighted.money + weighted.changes
    };
    Score {
        norm,
        weighted,
        invalid,
        flow_gain: record.final_stats.flow_mean - record.baseline.flow_mean,
        score,
    }
}
```

Replace the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark::config::BenchConfig;
    use crate::benchmark::record::*;

    fn record(
        baseline_cm: f64,
        final_cm: f64,
        money: i64,
        changes: u32,
        pop_base: u32,
        pop_final: u32,
    ) -> RunRecord {
        RunRecord {
            schema_version: SCHEMA_VERSION,
            config: BenchConfig::default(),
            map: MapInfo { id: "m".into(), source: "s".into(), game_version: "v".into() },
            started_at: "a".into(),
            ended_at: "b".into(),
            end_reason: EndReason::Submit,
            baseline: WindowStats {
                flow_mean: 55.0,
                active_vehicles_mean: 2000.0,
                population: pop_base,
                congested_meters: baseline_cm,
            },
            final_stats: WindowStats {
                flow_mean: 60.0,
                active_vehicles_mean: 2000.0,
                population: pop_final,
                congested_meters: final_cm,
            },
            flow_samples: FlowSamples { baseline: vec![], final_samples: vec![] },
            tally: Tally { num_changes: changes, money_spent: money },
            actions: vec![],
        }
    }

    #[test]
    fn clearing_all_congestion_cheaply_scores_one() {
        let s = score_run(&record(1000.0, 0.0, 0, 0, 30_000, 30_000), &BenchConfig::default());
        assert!(!s.invalid);
        assert!((s.score - 1.0).abs() < 1e-9, "score = {}", s.score);
    }

    #[test]
    fn unchanged_congestion_scores_only_resource_components() {
        let s = score_run(&record(1000.0, 1000.0, 0, 0, 30_000, 30_000), &BenchConfig::default());
        assert!((s.score - 0.40).abs() < 1e-9, "score = {}", s.score);
    }

    #[test]
    fn worsened_congestion_clamps_to_zero_not_negative() {
        let s = score_run(&record(1000.0, 2000.0, 0, 0, 30_000, 30_000), &BenchConfig::default());
        assert_eq!(s.norm.congestion, 0.0);
    }

    #[test]
    fn population_guard_zeroes_collapsed_runs() {
        let s = score_run(&record(1000.0, 0.0, 0, 0, 30_000, 110), &BenchConfig::default());
        assert!(s.invalid);
        assert_eq!(s.score, 0.0);
    }

    #[test]
    fn population_guard_passes_at_exactly_the_ratio() {
        let s = score_run(&record(1000.0, 0.0, 0, 0, 30_000, 24_000), &BenchConfig::default());
        assert!(!s.invalid, "24_000 == 0.8 * 30_000 must pass");
    }

    #[test]
    fn zero_baseline_congestion_is_invalid() {
        let s = score_run(&record(0.0, 0.0, 0, 0, 30_000, 30_000), &BenchConfig::default());
        assert!(s.invalid);
        assert_eq!(s.score, 0.0);
    }

    #[test]
    fn flow_gain_is_reported_as_diagnostic() {
        let s = score_run(&record(1000.0, 500.0, 0, 0, 30_000, 30_000), &BenchConfig::default());
        assert!((s.flow_gain - 5.0).abs() < 1e-9);
    }

    #[test]
    fn resource_norms_clamp_to_unit() {
        let s = score_run(
            &record(1000.0, 0.0, 50_000_000, 1000, 30_000, 30_000),
            &BenchConfig::default(),
        );
        assert_eq!(s.norm.money, 1.0);
        assert_eq!(s.norm.changes, 1.0);
        assert!((s.score - 0.60).abs() < 1e-9, "score = {}", s.score);
    }
}
```

- [ ] **Step 4: Update `measure.rs`**

In `measure_window`, accumulate congestion:

```rust
use crate::benchmark::congestion::WindowAccum;
// ...
let mut accum = WindowAccum::new();
let mut samples = Vec::with_capacity(n_samples as usize);
for _ in 0..n_samples {
    client.clock("step", Some(chunk), None).await?;
    let m = client.metrics().await?;
    let flow = m.traffic.flow_percent as f64;
    flow_sum += flow;
    veh_sum += m.traffic.active_vehicles as f64;
    last_pop = m.population.total;
    accum.push(&m.traffic.segment_loads);
    samples.push(flow);
}
let n = n_samples as f64;
Ok(WindowMeasurement {
    stats: WindowStats {
        flow_mean: flow_sum / n,
        active_vehicles_mean: veh_sum / n,
        population: last_pop,
        congested_meters: accum.congested_meters(cfg.congestion_threshold),
    },
    samples,
})
```

In `finalize`, after the baseline is resolved (both branches), add:

```rust
anyhow::ensure!(
    baseline.congested_meters > 0.0,
    "baseline congested_meters is 0 — the map has nothing to score against (spec §2.2); \
     check that the mod emits segment lengths and the save actually has congestion"
);
```

Also: `schema_version: 1` in the `RunRecord` literal → `crate::benchmark::record::SCHEMA_VERSION`.

Update tests:
- `measures_window_stats_on_empty_city`: add `assert_eq!(m.stats.congested_meters, 0.0);`.
- `finalize_writes_record_and_score_from_end_state`: the `baseline: Some(WindowStats { ... })` literal gains `congested_meters: 500.0`; add `assert_eq!(rec["baseline"]["congested_meters"], 500.0);` and `assert!(score["flow_gain"].is_number());`.
- `finalize_measures_missing_baseline`: the empty mock city would now bail (zero congestion), so build three roads first — the third gets segment id 9 → mock density 0.9 ≥ 0.7, length 50:

```rust
for (x0, x1) in [(0.0_f32, 50.0_f32), (1000.0, 1050.0), (2000.0, 2050.0)] {
    c.build_road(
        crate::contract::Position { x: x0, y: 0.0, z: 0.0 },
        crate::contract::Position { x: x1, y: 0.0, z: 0.0 },
        "road",
        true,
    )
    .await
    .unwrap();
}
```

then assert `rec["baseline"]["congested_meters"] == 50.0` (and drop the `flow_mean == 100.0` assertion — three segments give flow 85).
- Add a new test `finalize_bails_when_baseline_has_no_congestion`: same as the missing-baseline test but with no roads built; assert `finalize(...).await.is_err()`.

- [ ] **Step 5: Rename the rolling window**

`git mv broker/src/benchmark/flow_window.rs broker/src/benchmark/rolling_window.rs`; in the file rename `FlowWindow` → `RollingWindow`, delete `target_reached` and its test (the congestion end condition compares against a ratio, and flow no longer ends runs), update the doc comment to say it provides windowed means for both flow (diagnostic) and congested meters. Update `broker/src/benchmark/mod.rs` (`pub mod rolling_window;`).

- [ ] **Step 6: Rewrite `state.rs` telemetry**

```rust
use crate::benchmark::congestion::instant_congested_meters;
use crate::benchmark::record::SCHEMA_VERSION;
use crate::benchmark::rolling_window::RollingWindow;
use crate::contract::Metrics;
```

`RunState` fields: `flow: RollingWindow`, plus new:

```rust
pub congestion: RollingWindow,
pub last_population: Option<u32>,
pub last_abandoned_buildings: Option<u32>,
```

(initialize in `new()`: `congestion: RollingWindow::new(window)`, both `last_*: None`).

Replace `push_flow` with:

```rust
/// Fold one metrics sample into the run telemetry. The congestion end
/// condition needs the baseline (for the ratio), so it only arms once the
/// baseline window has been measured.
pub fn observe_metrics(&mut self, m: &Metrics) {
    self.flow.push(m.traffic.flow_percent as f64);
    self.congestion
        .push(instant_congested_meters(&m.traffic.segment_loads, self.config.congestion_threshold));
    self.last_population = Some(m.population.total);
    self.last_abandoned_buildings = Some(m.services.abandoned_buildings);
    let baseline_cm = self.baseline.as_ref().map(|b| b.congested_meters);
    if let Some(base) = baseline_cm {
        if base > 0.0
            && self.congestion.is_full()
            && self.congestion.mean() <= self.config.congestion_end_ratio * base
            && self.end_reason.is_none()
        {
            self.end_reason = Some(EndReason::CongestionTarget);
        }
    }
}
```

`end_state()`: `schema_version: SCHEMA_VERSION`.

`progress()`:

```rust
/// Agent-facing telemetry (spec §7 + 2026-06-10 §3): resources, the scored
/// congestion signal, and neutral city-health facts. Never the score.
pub fn progress(&self) -> Value {
    let baseline_cm = self.baseline.as_ref().map(|b| b.congested_meters);
    json!({
        "money_spent": self.money_spent,
        "num_changes": self.num_changes,
        "congested_meters_current": self.congestion.mean(),
        "congested_meters_baseline": baseline_cm,
        "congested_meters_target": baseline_cm.map(|b| self.config.congestion_end_ratio * b),
        "flow_current": self.flow.mean(),
        "population": self.last_population,
        "abandoned_buildings": self.last_abandoned_buildings,
        "seconds_remaining": self.seconds_remaining(),
    })
}
```

Update the state tests:
- `progress_omits_score_fields`: replace the `flow_target` assertion with `assert!(p["congested_meters_current"].is_number());` and `assert!(p["congested_meters_baseline"].is_null(), "no baseline yet");` keep the score-absence assertions.
- Add:

```rust
#[test]
fn congestion_end_condition_arms_only_with_baseline() {
    use crate::benchmark::record::{EndReason, WindowStats};
    use crate::contract::*;

    let metrics = |density: f32| Metrics {
        tick: 0,
        traffic: TrafficMetrics {
            flow_percent: 50.0,
            active_vehicles: 100,
            segment_loads: vec![SegmentLoad { segment_id: 1, density, length: 100.0 }],
        },
        economy: EconomyMetrics { balance: 0, weekly_income: 0, weekly_expenses: 0, funds: 0 },
        population: PopulationMetrics {
            total: 1000,
            residential_demand: 0,
            commercial_demand: 0,
            workplace_demand: 0,
            employed: 0,
        },
        services: ServiceMetrics { happiness: 80, abandoned_buildings: 2 },
    };

    let mut s = state();
    // Without a baseline, even a fully clear window must not end the run.
    for _ in 0..10 {
        s.observe_metrics(&metrics(0.0));
    }
    assert_eq!(s.end_reason, None);

    s.baseline = Some(WindowStats {
        flow_mean: 50.0,
        active_vehicles_mean: 100.0,
        population: 1000,
        congested_meters: 100.0,
    });
    for _ in 0..10 {
        s.observe_metrics(&metrics(0.0)); // 0 ≤ 0.05 × 100
    }
    assert_eq!(s.end_reason, Some(EndReason::CongestionTarget));
    assert_eq!(s.progress()["population"], 1000);
    assert_eq!(s.progress()["abandoned_buildings"], 2);
}
```

- [ ] **Step 7: Extract `metrics_value` in `service.rs`**

```rust
/// Group-filtered metrics JSON from an already-fetched snapshot, so callers
/// that need the typed `Metrics` (the benchmark server's telemetry) don't
/// fetch twice.
pub fn metrics_value(m: &crate::contract::Metrics, groups: &[String]) -> Value {
    let want = |g: &str| groups.is_empty() || groups.iter().any(|x| x == g);
    let mut out = json!({ "tick": m.tick });
    if want("traffic") {
        out["traffic"] = serde_json::to_value(&m.traffic).unwrap();
    }
    if want("economy") {
        out["economy"] = serde_json::to_value(&m.economy).unwrap();
    }
    if want("population") {
        out["population"] = serde_json::to_value(&m.population).unwrap();
    }
    if want("services") {
        out["services"] = serde_json::to_value(&m.services).unwrap();
    }
    out
}

pub async fn get_metrics(
    client: &BridgeClient,
    args: GetMetricsArgs,
) -> Result<Value, ServiceError> {
    let m = client.metrics().await?;
    Ok(metrics_value(&m, &args.groups))
}
```

- [ ] **Step 8: Wire `server.rs`**

- `get_metrics` tool:

```rust
async fn get_metrics(&self, Parameters(args): Parameters<GetMetricsArgs>) -> Result<CallToolResult, ErrorData> {
    self.ensure_baseline().await;
    match self.client.metrics().await {
        Ok(m) => {
            self.state.lock().await.observe_metrics(&m);
            self.finish(service::metrics_value(&m, &args.groups)).await
        }
        Err(e) => Ok(tool_err(ServiceError::Bridge(e.into()))),
    }
}
```

(`ServiceError::Bridge` is `#[from] BridgeError`, so `ServiceError::Bridge(e)` — match what compiles.)

- In `control_time`, replace both `push_flow` blocks with:

```rust
if let Ok(m) = self.client.metrics().await {
    self.state.lock().await.observe_metrics(&m);
}
```

- `control_time` tool description: "`step` defaults to 1 in-game day (585 ticks) when `ticks` is omitted; the maximum step is 7 days (4095 ticks)." (rest unchanged).
- Server-instructions string in `get_info`: "SkylineBench benchmark: reduce the city's congested road-meters, then call submit_solution. Each response includes benchmark_progress (resources + goal)."

Update server tests:
- `bench_with_mock` / `bench_with_mock_costs` baseline literals gain `congested_meters: 100.0`.
- `attaches_progress_to_json_value`: assert `merged["benchmark_progress"]["congested_meters_current"].is_number()` instead of `flow_target`.
- `step_above_three_days_is_rejected` → rename to `step_above_cap_is_rejected`, request `ticks: Some(5000)`, assert the error text contains `"4095"`.
- `step_of_exactly_the_cap_is_allowed`: `ticks: Some(4095)`, assert `"\"tick\":4095"`.
- `chunked_step_advances_the_full_requested_ticks`: use `4095`; assert `"ticks_advanced":4095`.

- [ ] **Step 9: Full suite green**

Run: `cd broker && cargo test`
Expected: PASS. Also run `cargo test --test broker_e2e` (it only asserts mock flow behavior and should be untouched).

- [ ] **Step 10: Commit**

```bash
git add -A broker/src
git commit -m "feat(broker): congestion-based scoring, population guard, congestion telemetry (schema v2)"
```

---

### Task 4: Mod metrics — segment length + abandoned buildings

**Files:**
- Modify: `mod/src/dto/Dtos.cs`
- Modify: `mod/src/bridge/GameReads.cs`
- Modify: `mod/src/json/Serialize.cs`
- Test: `mod/test/SerializeTests.cs`

- [ ] **Step 1: Write the failing serialization test**

In `mod/test/SerializeTests.cs`, add (follow the file's existing assertion style — it's a hand-rolled runner; mirror a neighbouring test's shape):

```csharp
public static void MetricsIncludesSegmentLengthAndAbandoned()
{
    var m = new MetricsDto { Tick = 1, FlowPercent = 50f, ActiveVehicles = 10, AbandonedBuildings = 7 };
    m.SegmentLoads.Add(new SegmentLoadDto { SegmentId = 3, Density = 0.9f, Length = 52.5f });
    string json = Serialize.Metrics(m);
    Assert.Contains("\"length\":52.5", json);
    Assert.Contains("\"abandoned_buildings\":7", json);
}
```

Register it in `TestRunner.cs` the same way the other `SerializeTests` methods are registered.

- [ ] **Step 2: Run to verify failure**

Run: `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe`
Expected: compile failure (`Length`, `AbandonedBuildings` missing).

- [ ] **Step 3: Implement**

`mod/src/dto/Dtos.cs`:

```csharp
public struct SegmentLoadDto { public uint SegmentId; public float Density; public float Length; }
```

and add `public uint AbandonedBuildings;` to `MetricsDto` (after `Happiness`).

`mod/src/bridge/GameReads.cs`, in `Metrics()`:
- segment loop: `dto.SegmentLoads.Add(new SegmentLoadDto { SegmentId = i, Density = Mathf.Min(1f, s.m_trafficDensity / 100f), Length = s.m_averageLength });`
- after the district read, count abandonment:

```csharp
var bm = Singleton<BuildingManager>.instance;
uint abandoned = 0;
for (uint i = 0; i < bm.m_buildings.m_buffer.Length; i++)
{
    var b = bm.m_buildings.m_buffer[i];
    if ((b.m_flags & Building.Flags.Created) == Building.Flags.None) continue;
    if ((b.m_flags & Building.Flags.Abandoned) != Building.Flags.None) abandoned++;
}
dto.AbandonedBuildings = abandoned;
```

`mod/src/json/Serialize.cs`, in `Metrics(...)`:
- segment loop adds `.Name("length").Value(sl.Length)`;
- services object: `.Name("abandoned_buildings").Value((long)m.AbandonedBuildings)` after happiness.

- [ ] **Step 4: Run tests**

Run: `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add mod/src mod/test
git commit -m "feat(mod): segment length + abandoned-building count in /metrics"
```

---

### Task 5: Mod ActionResult consequence fields

**Files:**
- Modify: `mod/src/dto/Dtos.cs`
- Modify: `mod/src/json/Serialize.cs`
- Test: `mod/test/SerializeTests.cs`

- [ ] **Step 1: Failing tests**

```csharp
public static void ActionIncludesFrontageWhenComputed()
{
    var r = new ActionResultDto { Ok = true, ZonedBuildingsFronting = 3 };
    Assert.Contains("\"zoned_buildings_fronting\":3", Serialize.Action(r));
    var none = new ActionResultDto { Ok = true }; // -1 default → omitted
    Assert.DoesNotContain("zoned_buildings_fronting", Serialize.Action(none));
}

public static void ActionFailureIncludesCollidingBuildings()
{
    var r = ActionResultDto.Fail(ErrorCode.ObjectCollision); // compiles after Task 7; use the literal "OBJECT_COLLISION" until then
    r.CollidingBuildings.Add(41);
    r.CollidingBuildings.Add(99);
    string json = Serialize.Action(r);
    Assert.Contains("\"reason\":\"OBJECT_COLLISION\"", json);
    Assert.Contains("\"colliding_buildings\":[41,99]", json);
}
```

(Use the string literal `"OBJECT_COLLISION"` in this task; Task 7 introduces the constant. `ErrorCode.cs` isn't compiled into Tests.csproj anyway.)

- [ ] **Step 2: Verify failure, then implement**

`mod/src/dto/Dtos.cs`, in `ActionResultDto`:

```csharp
public int ZonedBuildingsFronting = -1; // -1 = not computed / not applicable
public List<uint> CollidingBuildings = new List<uint>();
```

`mod/src/json/Serialize.cs`:

```csharp
public static string Action(ActionResultDto r)
{
    var w = new JsonWriter();
    w.BeginObject().Name("ok").Value(r.Ok);
    if (r.Ok)
    {
        WriteUintArray(w, "created_nodes", r.CreatedNodes);
        WriteUintArray(w, "created_segments", r.CreatedSegments);
        WriteUintArray(w, "snapped_nodes", r.SnappedNodes);
        WriteUintArray(w, "destroyed", r.Destroyed);
        if (r.ZonedBuildingsFronting >= 0) w.Name("zoned_buildings_fronting").Value((long)r.ZonedBuildingsFronting);
    }
    else
    {
        w.Name("reason").Value(r.Reason);
        if (r.CollidingBuildings.Count > 0) WriteUintArray(w, "colliding_buildings", r.CollidingBuildings);
    }
    w.EndObject();
    return w.ToString();
}
```

- [ ] **Step 3: Run tests, commit**

Run: `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe` → PASS.

```bash
git add mod/src mod/test
git commit -m "feat(mod): zoned_buildings_fronting + colliding_buildings on action results"
```

---

### Task 6: Mod frontage counter

**Files:**
- Create: `mod/src/bridge/Frontage.cs`
- Modify: `mod/src/bridge/GameActions.cs`
- Modify: `mod/SkylineBenchMod.csproj` (add the new compile item next to the other `src\bridge` entries — skip if the csproj globs)

No game-free test is possible (Unity/Colossal types); correctness is exercised in Task 11's live checks. Keep the geometry tiny and obvious.

- [ ] **Step 1: Create `Frontage.cs`**

```csharp
using ColossalFramework;
using UnityEngine;

namespace SkylineBench.Bridge
{
    /// <summary>
    /// Counts zoned (RCIO) buildings that front a road span. CS1 zone blocks
    /// extend 4 cells (32 m) from the road edge, so a building whose centre is
    /// within halfWidth + 36 m of the span's centreline is treated as fronting
    /// it. Geometric approximation: a parallel street closer than the corridor
    /// width can produce small over-counts — acceptable for a neutral
    /// informational field. Must run on the simulation thread.
    /// </summary>
    public static class Frontage
    {
        private const float ZoneDepthM = 32f;
        private const float MarginM = 4f;

        public static uint CountZonedBuildingsNear(Vector3 a, Vector3 b, float roadHalfWidth)
        {
            float corridor = roadHalfWidth + ZoneDepthM + MarginM;
            var bm = Singleton<BuildingManager>.instance;
            uint count = 0;
            for (uint i = 0; i < bm.m_buildings.m_buffer.Length; i++)
            {
                var bld = bm.m_buildings.m_buffer[i];
                if ((bld.m_flags & Building.Flags.Created) == Building.Flags.None) continue;
                if (!IsZoned(bld.Info)) continue;
                if (DistanceToSegmentXZ(bld.m_position, a, b) <= corridor) count++;
            }
            return count;
        }

        private static bool IsZoned(BuildingInfo info)
        {
            if (info == null || info.m_class == null) return false;
            switch (info.m_class.m_service)
            {
                case ItemClass.Service.Residential:
                case ItemClass.Service.Commercial:
                case ItemClass.Service.Industrial:
                case ItemClass.Service.Office:
                    return true;
                default:
                    return false;
            }
        }

        private static float DistanceToSegmentXZ(Vector3 p, Vector3 a, Vector3 b)
        {
            var ap = new Vector2(p.x - a.x, p.z - a.z);
            var ab = new Vector2(b.x - a.x, b.z - a.z);
            float len2 = ab.sqrMagnitude;
            float t = len2 > 0f ? Mathf.Clamp01(Vector2.Dot(ap, ab) / len2) : 0f;
            var closest = new Vector2(a.x + ab.x * t, a.z + ab.y * t);
            return Vector2.Distance(new Vector2(p.x, p.z), closest);
        }
    }
}
```

- [ ] **Step 2: Wire into `GameActions.cs`**

- `BuildRoad`, on success (inside the sim-thread delegate, after `result.CreatedSegments.Add(segId);`):

```csharp
result.ZonedBuildingsFronting = (int)Frontage.CountZonedBuildingsNear(startPos, endPos, prefab.m_halfWidth);
```

- `UpgradeRoad`: capture geometry before the release —

```csharp
Vector3 aPos = nm.m_nodes.m_buffer[startN].m_position;
Vector3 bPos = nm.m_nodes.m_buffer[endN].m_position;
```

(immediately after `startN`/`endN` are read), and on success:

```csharp
r.ZonedBuildingsFronting = (int)Frontage.CountZonedBuildingsNear(aPos, bPos, prefab.m_halfWidth);
```

- `Bulldoze`, in the `"segment"` case, compute BEFORE releasing:

```csharp
case "segment":
{
    var nm = Singleton<NetManager>.instance;
    var seg = nm.m_segments.m_buffer[req.Id];
    int fronting = -1;
    if ((seg.m_flags & NetSegment.Flags.Created) != NetSegment.Flags.None && seg.Info != null)
    {
        Vector3 aPos = nm.m_nodes.m_buffer[seg.m_startNode].m_position;
        Vector3 bPos = nm.m_nodes.m_buffer[seg.m_endNode].m_position;
        fronting = (int)Frontage.CountZonedBuildingsNear(aPos, bPos, seg.Info.m_halfWidth);
    }
    nm.ReleaseSegment((ushort)req.Id, false);
    var res = new ActionResultDto { Ok = true, ZonedBuildingsFronting = fronting };
    res.Destroyed.Add(req.Id);
    return res;
}
```

(restructure the existing `switch` so the segment case returns directly like this; node/building cases keep current behavior).

- [ ] **Step 3: Compile check + commit**

Run: `cd mod && ./build.sh` (needs game DLLs; if unavailable on this machine, `xbuild SkylineBenchMod.csproj /p:Configuration=Release /p:ManagedDLLPath=...` per `mod/README.md`).
Expected: build succeeds.

```bash
git add mod/src mod/SkylineBenchMod.csproj
git commit -m "feat(mod): count zoned buildings fronting affected segments"
```

---

### Task 7: Mod build validation + validate-road endpoint

**Files:**
- Modify: `mod/src/bridge/ErrorCode.cs`
- Create: `mod/src/bridge/BuildValidator.cs`
- Modify: `mod/src/bridge/GameActions.cs`
- Modify: `mod/src/http/Router.cs`, `mod/src/http/Handlers.cs`
- Modify: `mod/SkylineBenchMod.csproj` (new compile item)

- [ ] **Step 1: Extend `ErrorCode.cs`**

```csharp
public const string ObjectCollision = "OBJECT_COLLISION";
public const string SlopeTooSteep = "SLOPE_TOO_STEEP";
public const string OutOfArea = "OUT_OF_AREA";
public const string TooManyConnections = "TOO_MANY_CONNECTIONS";
public const string NetBufferFull = "NET_BUFFER_FULL";
```

- [ ] **Step 2: Create `BuildValidator.cs`**

```csharp
using ColossalFramework;
using UnityEngine;
using SkylineBench.Dto;

namespace SkylineBench.Bridge
{
    /// <summary>
    /// Pre-creation placement checks for road builds. NetManager.CreateSegment
    /// is the low-level constructor and skips all NetTool validation, so
    /// without these checks builds the game UI refuses (through buildings, too
    /// steep, outside owned tiles) silently succeed. Explicit checks rather
    /// than NetTool's test mode keep vanilla rules we don't want (money,
    /// road-spacing) out of the benchmark. Must run on the simulation thread.
    /// </summary>
    public static class BuildValidator
    {
        private const int MaxReportedCollisions = 20;

        /// <summary>null when the placement is valid; otherwise a failure DTO
        /// with the normalized reason (plus colliding building ids).</summary>
        public static ActionResultDto Check(NetInfo prefab, Vector3 a, Vector3 b)
        {
            var am = Singleton<GameAreaManager>.instance;
            if (am.PointOutOfArea(a) || am.PointOutOfArea(b))
                return ActionResultDto.Fail(ErrorCode.OutOfArea);

            float lenXZ = ColossalFramework.Math.VectorUtils.LengthXZ(b - a);
            if (lenXZ > 0.001f && Mathf.Abs(b.y - a.y) / lenXZ > prefab.m_maxSlope)
                return ActionResultDto.Fail(ErrorCode.SlopeTooSteep);

            var colliding = CollidingBuildings(a, b, prefab.m_halfWidth);
            if (colliding.Count > 0)
            {
                var fail = ActionResultDto.Fail(ErrorCode.ObjectCollision);
                fail.CollidingBuildings = colliding;
                return fail;
            }
            return null;
        }

        private static System.Collections.Generic.List<uint> CollidingBuildings(Vector3 a, Vector3 b, float roadHalfWidth)
        {
            var hits = new System.Collections.Generic.List<uint>();
            var bm = Singleton<BuildingManager>.instance;
            for (uint i = 0; i < bm.m_buildings.m_buffer.Length && hits.Count < MaxReportedCollisions; i++)
            {
                var bld = bm.m_buildings.m_buffer[i];
                if ((bld.m_flags & Building.Flags.Created) == Building.Flags.None) continue;
                if (Intersects(a, b, roadHalfWidth, ref bld)) hits.Add(i);
            }
            return hits;
        }

        /// <summary>2-D capsule (road span widened by half-width) vs the
        /// building's rotated footprint rectangle.</summary>
        private static bool Intersects(Vector3 a, Vector3 b, float roadHalfWidth, ref Building bld)
        {
            var info = bld.Info;
            float halfW = (info != null ? info.m_cellWidth : 1) * 4f;
            float halfL = (info != null ? info.m_cellLength : 1) * 4f;
            float cos = Mathf.Cos(-bld.m_angle), sin = Mathf.Sin(-bld.m_angle);
            Vector2 la = ToLocal(a, bld.m_position, cos, sin);
            Vector2 lb = ToLocal(b, bld.m_position, cos, sin);
            return SegmentToRectDistance(la, lb, halfW, halfL) <= roadHalfWidth;
        }

        private static Vector2 ToLocal(Vector3 world, Vector3 centre, float cos, float sin)
        {
            float dx = world.x - centre.x, dz = world.z - centre.z;
            return new Vector2(dx * cos - dz * sin, dx * sin + dz * cos);
        }

        /// <summary>Distance from segment [a,b] to the axis-aligned rectangle
        /// |x| ≤ halfW, |y| ≤ halfL centred at the origin (0 when intersecting).</summary>
        private static float SegmentToRectDistance(Vector2 a, Vector2 b, float halfW, float halfL)
        {
            if (PointInRect(a, halfW, halfL) || PointInRect(b, halfW, halfL)) return 0f;
            Vector2[] corners =
            {
                new Vector2(-halfW, -halfL), new Vector2(halfW, -halfL),
                new Vector2(halfW, halfL), new Vector2(-halfW, halfL)
            };
            float best = float.MaxValue;
            for (int i = 0; i < 4; i++)
            {
                Vector2 c0 = corners[i], c1 = corners[(i + 1) % 4];
                if (SegmentsIntersect(a, b, c0, c1)) return 0f;
                best = Mathf.Min(best, PointToSegmentDistance(c0, a, b));
                best = Mathf.Min(best, PointToSegmentDistance(a, c0, c1));
                best = Mathf.Min(best, PointToSegmentDistance(b, c0, c1));
            }
            return best;
        }

        private static bool PointInRect(Vector2 p, float halfW, float halfL)
        {
            return Mathf.Abs(p.x) <= halfW && Mathf.Abs(p.y) <= halfL;
        }

        private static bool SegmentsIntersect(Vector2 p1, Vector2 p2, Vector2 q1, Vector2 q2)
        {
            float d1 = Cross(q2 - q1, p1 - q1), d2 = Cross(q2 - q1, p2 - q1);
            float d3 = Cross(p2 - p1, q1 - p1), d4 = Cross(p2 - p1, q2 - p1);
            return ((d1 > 0f) != (d2 > 0f)) && ((d3 > 0f) != (d4 > 0f));
        }

        private static float Cross(Vector2 u, Vector2 v) { return u.x * v.y - u.y * v.x; }

        private static float PointToSegmentDistance(Vector2 p, Vector2 a, Vector2 b)
        {
            var ab = b - a;
            float len2 = ab.sqrMagnitude;
            float t = len2 > 0f ? Mathf.Clamp01(Vector2.Dot(p - a, ab) / len2) : 0f;
            return Vector2.Distance(p, a + ab * t);
        }
    }
}
```

Verification note for the executor: `GameAreaManager.PointOutOfArea(Vector3)`, `NetInfo.m_maxSlope`, `NetInfo.m_halfWidth`, `Building.m_angle`, and `NetNode.CountSegments()` are all public CS1 API. If the compiler disagrees on any name, confirm against the game DLLs the way `SaveLoader.cs` documents:
`monodis --method "$HOME/Library/Application Support/Steam/steamapps/common/Cities_Skylines/Cities.app/Contents/Resources/Data/Managed/Assembly-CSharp.dll" | grep -i "PointOutOfArea\|CountSegments"`.

- [ ] **Step 3: Wire validation + honest failure codes into `GameActions.BuildRoad`**

Restructure the sim-thread delegate:

```csharp
return SimThread.Run<ActionResultDto>(delegate
{
    var nm = Singleton<NetManager>.instance;
    var sm = Singleton<SimulationManager>.instance;

    var invalid = BuildValidator.Check(prefab, startPos, endPos);
    if (invalid != null) return invalid;

    var rand = new Randomizer(sm.m_currentBuildIndex);
    var result = new ActionResultDto { Ok = true };
    ushort startId, endId;
    string nodeErr = ResolveNode(nm, startPos, prefab, req.Snap, ref rand, sm, out startId, result);
    if (nodeErr != null) return FailReleasing(nm, result, nodeErr);
    nodeErr = ResolveNode(nm, endPos, prefab, req.Snap, ref rand, sm, out endId, result);
    if (nodeErr != null) return FailReleasing(nm, result, nodeErr);
    Vector3 dir = VectorUtils.NormalizeXZ(endPos - startPos);
    ushort segId;
    bool ok = nm.CreateSegment(out segId, ref rand, prefab, startId, endId, dir, -dir, sm.m_currentBuildIndex, sm.m_currentBuildIndex, false);
    if (!ok) return FailReleasing(nm, result, ErrorCode.NetBufferFull);
    sm.m_currentBuildIndex += 2u;
    result.CreatedSegments.Add(segId);
    result.ZonedBuildingsFronting = (int)Frontage.CountZonedBuildingsNear(startPos, endPos, prefab.m_halfWidth);
    return result;
}, TimeoutMs);
```

with the two helpers (replacing the old `ResolveNode`):

```csharp
/// <summary>null on success; otherwise a normalized ErrorCode. Snapped nodes
/// with a full connection budget (8 segments in CS1) are rejected rather than
/// silently producing a broken junction.</summary>
private static string ResolveNode(NetManager nm, Vector3 p, NetInfo prefab, bool snap, ref Randomizer rand, SimulationManager sm, out ushort id, ActionResultDto result)
{
    id = 0;
    if (snap)
    {
        ushort near = NearestNode(nm, p, SnapToleranceM);
        if (near != 0)
        {
            if (nm.m_nodes.m_buffer[near].CountSegments() >= 8) return ErrorCode.TooManyConnections;
            id = near; result.SnappedNodes.Add(near); return null;
        }
    }
    if (!nm.CreateNode(out id, ref rand, prefab, p, sm.m_currentBuildIndex)) return ErrorCode.NetBufferFull;
    result.CreatedNodes.Add(id);
    return null;
}

/// <summary>Failing after node creation would leak orphan nodes; release them.</summary>
private static ActionResultDto FailReleasing(NetManager nm, ActionResultDto partial, string reason)
{
    foreach (var n in partial.CreatedNodes) nm.ReleaseNode((ushort)n);
    return ActionResultDto.Fail(reason);
}
```

In `UpgradeRoad`, change `if (!ok) return ActionResultDto.Fail(ErrorCode.Collision);` → `ErrorCode.NetBufferFull` (a re-created segment between existing nodes can't "collide"; buffer exhaustion is the real failure there). Upgrades deliberately get no collision/slope validation — vanilla allows widening upgrades to displace buildings, and `zoned_buildings_fronting` is the agent's signal.

- [ ] **Step 4: Add `ValidateRoad` + the endpoint**

`GameActions.cs`:

```csharp
/// <summary>Free dry-run of BuildRoad's placement checks: same validation,
/// nothing created. Ok responses carry the frontage fact for the span.</summary>
public static ActionResultDto ValidateRoad(BuildRoadReq req)
{
    var prefab = Prefabs.FindRoad(req.Prefab);
    if (prefab == null) return ActionResultDto.Fail(ErrorCode.InvalidPrefab);
    var startPos = new Vector3(req.StartX, req.StartY, req.StartZ);
    var endPos = new Vector3(req.EndX, req.EndY, req.EndZ);
    float len = VectorUtils.LengthXZ(endPos - startPos);
    if (len < 0.001f) return ActionResultDto.Fail(ErrorCode.InvalidArgs);
    if (len > MaxSegmentLengthM) return ActionResultDto.Fail(ErrorCode.SegmentTooLong);

    return SimThread.Run<ActionResultDto>(delegate
    {
        var invalid = BuildValidator.Check(prefab, startPos, endPos);
        if (invalid != null) return invalid;
        var ok = new ActionResultDto { Ok = true };
        ok.ZonedBuildingsFronting = (int)Frontage.CountZonedBuildingsNear(startPos, endPos, prefab.m_halfWidth);
        return ok;
    }, TimeoutMs);
}
```

`mod/src/http/Router.cs` (after the build-road case):

```csharp
case "/action/validate-road": return method == "POST" ? Handlers.ValidateRoad(body) : MethodNotAllowed();
```

`mod/src/http/Handlers.cs`:

```csharp
public static HttpReply ValidateRoad(string body) { return HttpReply.Json(200, Serialize.Action(GameActions.ValidateRoad(RequestParse.BuildRoad(JsonReader.Parse(body))))); }
```

- [ ] **Step 5: Build + commit**

Run: `cd mod && ./build.sh` → succeeds.
Run mod unit tests (unchanged but confirm green): `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe`.

```bash
git add mod/src mod/SkylineBenchMod.csproj
git commit -m "feat(mod): placement validation with real failure reasons + validate-road dry-run"
```

---

### Task 8: Broker validate-road plumbing + apply_plan dry-run game checks

**Files:**
- Modify: `broker/src/bridge_client.rs`
- Modify: `broker/src/mock.rs`
- Modify: `broker/src/benchmark/server.rs`

- [ ] **Step 1: Failing test for the enriched dry-run**

In `broker/src/benchmark/server.rs` tests:

```rust
#[tokio::test]
async fn apply_plan_validate_only_runs_game_checks_for_builds() {
    let bench = bench_with_mock_costs().await;
    let res = bench
        .apply_plan(Parameters(ApplyPlanArgs {
            ops: vec![plan_build(0.0, 50.0)],
            validate_only: true,
            stop_on_error: true,
        }))
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&result_text(&res)).unwrap();
    assert_eq!(v["ok"], true);
    let row = &v["results"][0];
    // The mock's validate-road returns ok with zero fronting buildings; the
    // row must carry the game-check fact through.
    assert_eq!(row["zoned_buildings_fronting"], 0, "row: {row}");
    assert_eq!(v["benchmark_progress"]["num_changes"], 0);
}
```

Run: `cd broker && cargo test apply_plan_validate_only_runs_game_checks` → FAIL (field absent).

- [ ] **Step 2: `BridgeClient::validate_road`**

In `broker/src/bridge_client.rs`, next to `build_road` (reusing its `BuildRoadBody`):

```rust
pub async fn validate_road(
    &self,
    start: Position,
    end: Position,
    prefab: &str,
) -> Result<ActionResult, BridgeError> {
    let body = BuildRoadBody { start, end, prefab, snap_to_existing_nodes: true };
    Ok(self
        .http
        .post(format!("{}/action/validate-road", self.base))
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
```

(Match `build_road`'s exact request style in that file.)

- [ ] **Step 3: Mock route**

In `broker/src/mock.rs`:

```rust
async fn validate_road(
    State(_s): State<MockState>,
    Json(body): Json<BuildRoadBody>,
) -> Json<ActionResult> {
    let known = road_types().iter().any(|r| r.name == body.prefab);
    Json(ActionResult {
        ok: known,
        created_nodes: vec![],
        created_segments: vec![],
        snapped_nodes: vec![],
        destroyed: vec![],
        reason: if known { None } else { Some(ActionError::InvalidPrefab) },
        zoned_buildings_fronting: if known { Some(0) } else { None },
        colliding_buildings: vec![],
    })
}
```

and register `.route("/action/validate-road", post(validate_road))`.

- [ ] **Step 4: Enrich `apply_plan`'s validate/dry-run branch in `server.rs`**

Replace the `if args.validate_only || !all_valid { ... }` block body with a sequential loop that augments structurally-valid Build ops with game checks when `validate_only` (game checks are skipped when the plan is already structurally invalid — that path must stay free of bridge calls so whole-plan rejection is instant):

```rust
if args.validate_only || !all_valid {
    let mut results: Vec<Value> = Vec::with_capacity(validations.len());
    for (i, (source, op, v, cost)) in validations.iter().enumerate() {
        let mut row = serde_json::json!({
            "op_index": i,
            "source_op": source,
            "tool": tool_name(op),
            "valid": v.is_ok(),
            "reason": v.as_ref().err(),
            "estimated_cost": cost,
            "executed": false,
        });
        if args.validate_only && all_valid && v.is_ok() {
            if let ExecOp::Build { from, to, road_type, .. } = op {
                match self.client.validate_road(*from, *to, road_type).await {
                    Ok(check) => {
                        row["valid"] = serde_json::json!(check.ok);
                        if let Some(r) = check.reason {
                            row["reason"] = serde_json::to_value(r).unwrap();
                        }
                        if let Some(n) = check.zoned_buildings_fronting {
                            row["zoned_buildings_fronting"] = serde_json::json!(n);
                        }
                        if !check.colliding_buildings.is_empty() {
                            row["colliding_buildings"] =
                                serde_json::to_value(&check.colliding_buildings).unwrap();
                        }
                    }
                    Err(e) => {
                        row["game_check_error"] = serde_json::json!(e.to_string());
                    }
                }
            }
        }
        results.push(row);
    }
    let all_valid = results.iter().all(|r| r["valid"] == true);
    return self
        .finish(serde_json::json!({
            "ok": all_valid,
            "validate_only": args.validate_only,
            "results": results,
            "total_estimated_cost": total_estimated_cost,
            "first_failed_at": Value::Null,
        }))
        .await;
}
```

Update the `apply_plan` tool description's dry-run sentence: "Set validate_only=true for a free dry-run (no changes recorded) — build ops are also checked against the game's placement rules (collision/slope/area) and report `zoned_buildings_fronting`."

- [ ] **Step 5: Run the suite, commit**

Run: `cd broker && cargo test` → PASS (including the existing validate_only test, which now also gets `zoned_buildings_fronting: 0` rows).

```bash
git add broker/src
git commit -m "feat(broker): validate-road dry-run plumbing into apply_plan"
```

---

### Task 9: Agent prompt rewrite

**Files:**
- Modify: `benchmark/prompt.md`

- [ ] **Step 1: Replace the scoring/goal/telemetry text**

Apply these edits (keep everything else, including the work-method loop and scratch-code rules):

1. Tools line for time (old line 13):

> - Time: `control_time` (pause / resume / step / speed). Build while paused, then `step` to let traffic respond before measuring. A `step` with no `ticks` advances one in-game day (585 ticks). The maximum step is 7 days (4095 ticks) — traffic patterns repeat daily, so longer waits only burn your wall-clock budget.

2. Goal line (old line 18):

> Goal: reduce the city's congested road-meters — `congested_meters` is the total length of road segments whose traffic density is at or above 0.7 (densities are 0..1, from `query_segments` / `get_metrics`). Lower is better.

3. Scoring block (old lines 20-26):

> How you are scored (you will NOT see your score during the run):
> - `score = 0.60·congestion_reduction + 0.20·(1 − min(1, money_spent/10,000,000)) + 0.20·(1 − min(1, changes/300))`.
> - `congestion_reduction = max(0, baseline_congested_meters − final_congested_meters) / baseline_congested_meters`, measured city-wide from the FINAL settled state — congestion that merely moves to other segments still counts against you, and peaks along the way do not count. If a change makes congestion worse, leaving it in place costs you score; revert it.
> - Every successful modifying op is one change (a batch `apply_plan` call counts each executed op); reads are free — observe as much as you like.
> - INVALID RUN: if the city's population falls below 80% of baseline, the run scores zero. You are fixing traffic for the people who live here; keep the city alive. Tool responses and `benchmark_progress` carry neutral facts to track this: `population`, `abandoned_buildings`, and `zoned_buildings_fronting` on road modifications (how many zoned buildings front the affected segment — buildings need road frontage to function).

4. Run-end conditions (old lines 28-31):

> The run ends when any of these happens:
> 1. You call `submit_solution`.
> 2. Windowed `congested_meters_current` reaches `congested_meters_target` (5% of baseline) shown in `benchmark_progress`.
> 3. A 3-hour time limit is reached.

5. Telemetry line (old line 35):

> Every tool response includes a `benchmark_progress` block (money spent, changes made, congested meters now / baseline / target, flow, population, abandoned buildings, seconds remaining). Use it to pace yourself.

6. Add one sentence to the apply_plan bullet (old lines 8-12), at its end:

> A `validate_only` dry-run also checks build ops against the game's placement rules (collision / slope / map area) and reports the reason when one would fail.

- [ ] **Step 2: Commit**

```bash
git add benchmark/prompt.md
git commit -m "docs(benchmark): prompt for congestion scoring, population guard, new telemetry"
```

---

### Task 10: Live-game verification (operator-gated)

Requires Cities: Skylines running with the mod and `BasicTrafficScenario` loadable. The bridge listens on `127.0.0.1:8787`; `POST /load-save {"save_name":"BasicTrafficScenario"}` reloads it (takes minutes — poll `/health` for `city_loaded:true`).

- [ ] **Step 1: Deploy** — `cd mod && ./build.sh`, restart the game, reload the save.

- [ ] **Step 2: Metrics fields** —

```bash
curl -s http://127.0.0.1:8787/metrics | python3 -c "
import json,sys; m=json.load(sys.stdin)
sl=m['traffic']['segment_loads']
assert any(s['length']>0 for s in sl), 'lengths missing'
assert 'abandoned_buildings' in m['services'], 'abandoned missing'
print('lengths OK, abandoned =', m['services']['abandoned_buildings'])"
```

- [ ] **Step 3: Validation behaviors** — pick a building from `/buildings` and a clear area from a render, then:
  - validate-road through the building's position → expect `{"ok":false,"reason":"OBJECT_COLLISION","colliding_buildings":[...]}`;
  - build-road at the same span → same failure (and `/network` segment count unchanged);
  - validate-road in the clear area → `ok:true` with a plausible `zoned_buildings_fronting`;
  - a span with y-delta exceeding the prefab slope (e.g. end `y` +50 over a 60 m span) → `SLOPE_TOO_STEEP`;
  - far-corner coordinates (e.g. x=z=8000) → `OUT_OF_AREA`.

- [ ] **Step 4: Bulldoze/upgrade facts** — upgrade one zoned street segment and bulldoze one; both responses must carry `zoned_buildings_fronting` ≥ 0.

- [ ] **Step 5: Commit any fixes** found during verification (each with a test where representable in the mock).

---

### Task 11: Metric validation experiments (operator-gated)

**Files:**
- Modify: `benchmark/experiments/null_control.py`

- [ ] **Step 1: Update the experiment's congestion summary**

Replace `congestion_summary` (it predates the 0–1 density discovery and length field):

```python
def congestion_summary(segment_loads):
    if not segment_loads:
        return {"segments": 0}
    densities = sorted((s["density"] for s in segment_loads), reverse=True)
    congested = [s for s in segment_loads if s["density"] >= 0.7]
    worst_decile = densities[: max(1, len(densities) // 10)]
    return {
        "segments": len(segment_loads),
        "congested_count": len(congested),
        "congested_meters": round(sum(s.get("length", 0.0) for s in congested), 1),
        "mean_density": round(sum(densities) / len(densities), 3),
        "worst_decile_mean": round(sum(worst_decile) / len(worst_decile), 3),
    }
```

and print `congested_meters` in the per-day line.

- [ ] **Step 2: Null-control re-run** — reload the save, then `python3 benchmark/experiments/null_control.py 60 null-control-congestion.jsonl`. Acceptance: `congested_meters` baseline is substantial (>0) and stable (no trend, modest noise) across 60 days; population stays ≥ 80% of day-0 throughout (the new guard would not trip a do-nothing run).

- [ ] **Step 3: Known-good fix probe** — hand-apply one obvious congestion fix near the worst corridor (via `curl` to `/action/build-road` or a short script), step ~10 days, confirm `congested_meters` drops measurably vs the null trace.

- [ ] **Step 4: Full benchmark run** — `./benchmark/run.sh --map gridlock-v1` end-to-end; confirm `score.json` has `norm.congestion`, `flow_gain`, the population guard fields behave, and `benchmark_progress` in the transcript shows the new telemetry.

- [ ] **Step 5: Commit experiment updates + findings**

```bash
git add benchmark/experiments
git commit -m "feat(experiments): length-weighted congestion summary; congestion-metric validation runs"
```

---

## Self-Review Notes

- **Spec coverage:** §2.1 metric (Tasks 2/3/4), §2.2 formula + zero-baseline abort (Tasks 3, measure bail + score invalid), §2.3 population guard (Task 3), §2.4 end condition (Task 3 state), §3 observability (Tasks 3 progress, 4, 5, 6), §4 validation + errors + dry-run (Tasks 7, 8; HEIGHT_LIMIT deferred and NetTool swapped for explicit checks — flagged at top), §5 step cap + prompt (Tasks 3, 9), §7 validation plan (Tasks 10, 11).
- **Known approximations (documented in code):** frontage corridor over-counts near parallel streets; collision capsule uses straight spans (matches `build_road`'s straight-segment API).
- **Old artifacts:** schema v2 does not deserialize v1 run-records; `benchmark/runs/` history stays readable as plain JSON but not via the typed structs. Acceptable in this pre-release phase.
