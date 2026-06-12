# Junction-aware scoring + simulation reframe Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a congested-junction signal to the score (blended with congested length), replace the hard population cliff with a graded health factor, and reframe the agent prompt as "optimise a city simulation" with the scoring formula hidden.

**Architecture:** Two phases. Phase 1 (Tasks 1–5) builds the scoring engine broker-side: a junction-counting primitive, config constants, the `congested_junctions` measurement carried green through every construction site, then the blended + health-multiplied score. Phase 2 (Tasks 6–10) reshapes the agent-facing surface: live junction readout + `benchmark_progress`→`city_status` rename, the transcript renderer, the prompt rewrite, and the README. No mod/C# change.

**Task ordering is chosen so the crate compiles and every test passes after each task.** Adding the required `WindowStats.congested_junctions` field breaks every construction site at once, so Task 3 introduces the field AND wires all sites in one green commit (scoring logic unchanged); later tasks evolve the logic.

**Tech Stack:** Rust (broker, `cargo test`), Markdown prompt.

**Design spec:** `docs/superpowers/specs/2026-06-11-junction-scoring-and-simulation-reframe-design.md`

---

## File Structure

- `broker/src/benchmark/congestion.rs` — junction primitive (`Topology`, `congested_junctions`) + `WindowAccum::mean_density`. (Task 1)
- `broker/src/benchmark/config.rs` — new constants (Task 2); remove `pop_guard_ratio` (Task 5), `congestion_end_ratio` (Task 7).
- `broker/src/benchmark/record.rs` — `WindowStats.congested_junctions`, schema v3, `Score` diagnostics. (Task 3, 5)
- `broker/src/benchmark/measure.rs` — fetch `network()`, compute junctions. (Task 3)
- `broker/src/benchmark/score.rs` — blend + health. (Task 5)
- `broker/src/benchmark/state.rs` — live topology/density cache, `city_status` fields, drop arming. (Task 6)
- `broker/src/benchmark/server.rs` — block rename, topology refresh. (Task 7)
- `broker/src/benchmark/transcript.rs` — read `city_status`, fall back to `benchmark_progress`. (Task 8)
- `benchmark/prompt.md` — full rewrite. (Task 9)
- `benchmark/README.md` — scoring + framing note. (Task 10)

Contract types already present (no change): `Network { nodes, segments: Vec<NetSegment{id,start_node,end_node,...}> }` in `broker/src/contract.rs`; `BridgeClient::network()` exists.

---

# Phase 1 — scoring engine

## Task 1: Junction-counting primitive in `congestion.rs`

**Files:** Modify `broker/src/benchmark/congestion.rs`

- [ ] **Step 1: Write failing tests**

Add inside the existing `mod tests` block (after the last test, before the closing `}`):

```rust
    use crate::contract::{NetNode, NetSegment, Network};

    fn node(id: u32) -> NetNode { NetNode { id, x: 0.0, y: 0.0, z: 0.0 } }
    fn seg(id: u32, a: u32, b: u32) -> NetSegment {
        NetSegment {
            id, start_node: a, end_node: b, prefab: "road".into(), lanes: 2,
            length: 100.0, one_way: false, travel_direction: "both".into(), speed_limit: 1.0,
        }
    }

    #[test]
    fn junction_needs_degree_at_least_min_and_two_congested_approaches() {
        let net = Network {
            nodes: vec![node(1), node(2), node(3), node(4), node(5)],
            segments: vec![seg(10, 1, 3), seg(11, 1, 4), seg(12, 1, 5), seg(20, 2, 3)],
        };
        let topo = Topology::from_network(&net);
        let dense = |id: u32| match id { 10 | 11 => Some(0.9), _ => Some(0.2) };
        assert_eq!(congested_junctions(&topo, dense, 0.7, 3, 2), 1);
        let one = |id: u32| match id { 10 => Some(0.9), _ => Some(0.2) };
        assert_eq!(congested_junctions(&topo, one, 0.7, 3, 2), 0);
    }

    #[test]
    fn degree_two_node_is_never_a_junction_even_if_both_congested() {
        let net = Network { nodes: vec![node(2), node(3), node(4)], segments: vec![seg(20, 2, 3), seg(21, 2, 4)] };
        let topo = Topology::from_network(&net);
        assert_eq!(congested_junctions(&topo, |_| Some(0.9), 0.7, 3, 2), 0);
    }

    #[test]
    fn missing_density_counts_as_not_congested() {
        let net = Network { nodes: vec![node(1), node(3), node(4), node(5)], segments: vec![seg(10, 1, 3), seg(11, 1, 4), seg(12, 1, 5)] };
        let topo = Topology::from_network(&net);
        let dense = |id: u32| match id { 10 | 11 => Some(0.9), _ => None };
        assert_eq!(congested_junctions(&topo, dense, 0.7, 3, 2), 1);
    }

    #[test]
    fn window_mean_density_reads_back_per_segment() {
        let mut w = WindowAccum::new();
        w.push(&[load(1, 0.8, 100.0)]);
        w.push(&[load(1, 0.6, 100.0)]);
        assert!((w.mean_density(1).unwrap() - 0.7).abs() < 1e-9);
        assert_eq!(w.mean_density(999), None);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path broker/Cargo.toml congestion`
Expected: FAIL to compile — `Topology`, `congested_junctions`, `mean_density` don't exist.

- [ ] **Step 3: Implement**

Add to imports (file has `use std::collections::HashMap;` and `use crate::contract::SegmentLoad;`):

```rust
use crate::contract::Network;
```

Add to the `impl WindowAccum` block (after `congested_meters`):

```rust
    /// Mean density of one segment over the window, or None if it never
    /// appeared. Shared with the junction counter.
    pub fn mean_density(&self, segment_id: u32) -> Option<f64> {
        self.per_segment
            .get(&segment_id)
            .filter(|s| s.samples > 0)
            .map(|s| s.density_sum / f64::from(s.samples))
    }
```

Add below the `WindowAccum` impl (above `#[cfg(test)]`):

```rust
/// Road-graph adjacency: node id -> incident segment ids. A node's degree is
/// the number of incident segments; the 200 m auto-split makes many degree-2
/// nodes that are not real intersections, so the counter filters on min degree.
#[derive(Debug, Default)]
pub struct Topology {
    incidence: HashMap<u32, Vec<u32>>,
}

impl Topology {
    pub fn from_network(net: &Network) -> Self {
        let mut incidence: HashMap<u32, Vec<u32>> = HashMap::new();
        for s in &net.segments {
            incidence.entry(s.start_node).or_default().push(s.id);
            incidence.entry(s.end_node).or_default().push(s.id);
        }
        Self { incidence }
    }
}

/// Count congested junctions: nodes of degree ≥ `min_degree` with at least
/// `min_congested` incident segments at/above `threshold`. `density_of` returns
/// a segment's density (window-mean or latest sample), or None when unknown —
/// which counts as not congested.
pub fn congested_junctions(
    topo: &Topology,
    density_of: impl Fn(u32) -> Option<f64>,
    threshold: f64,
    min_degree: usize,
    min_congested: usize,
) -> u32 {
    topo.incidence
        .values()
        .filter(|segs| segs.len() >= min_degree)
        .filter(|segs| {
            segs.iter()
                .filter(|id| density_of(**id).is_some_and(|d| meets(d, threshold)))
                .count()
                >= min_congested
        })
        .count() as u32
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --manifest-path broker/Cargo.toml congestion` → Expected PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/benchmark/congestion.rs
git commit -m "feat(score): junction-counting primitive over network topology"
```

---

## Task 2: Config constants (additive)

**Files:** Modify `broker/src/benchmark/config.rs`

**Additive only.** `congestion_end_ratio` and `pop_guard_ratio` stay (still used elsewhere; removed in Tasks 7 and 5 respectively).

- [ ] **Step 1: Add fields to `struct BenchConfig`** (after `congestion_threshold`):

```rust
    /// Congestion term blends length- and junction-reduction:
    /// blend_meters·meters_reduction + blend_junctions·junction_reduction (sum to 1.0).
    pub blend_meters: f64,
    pub blend_junctions: f64,
    /// A node counts as a junction only at this degree or above.
    pub junction_min_degree: u32,
    /// A junction is congested when ≥ this many incident segments are congested.
    pub junction_min_congested: u32,
    /// Graded population-health factor: 1.0 at population ratio ≥ health_full,
    /// 0.0 at ≤ health_zero, linear between (replaces the old hard cliff).
    pub health_full: f64,
    pub health_zero: f64,
```

- [ ] **Step 2: Add to `Default`** (after `congestion_threshold: 0.7,`; leave `congestion_end_ratio: 0.05,` and `pop_guard_ratio: 0.8,`):

```rust
            blend_meters: 0.5,
            blend_junctions: 0.5,
            junction_min_degree: 3,
            junction_min_congested: 2,
            health_full: 0.95,
            health_zero: 0.75,
```

- [ ] **Step 3: Add a test** (leave existing tests unchanged):

```rust
    #[test]
    fn blend_and_health_constants_are_calibrated() {
        let c = BenchConfig::default();
        assert!((c.blend_meters + c.blend_junctions - 1.0).abs() < 1e-9);
        assert_eq!(c.junction_min_degree, 3);
        assert_eq!(c.junction_min_congested, 2);
        assert!(c.health_full > c.health_zero);
        assert_eq!(c.health_full, 0.95);
        assert_eq!(c.health_zero, 0.75);
    }
```

- [ ] **Step 4: Test**

Run: `cargo test --manifest-path broker/Cargo.toml config::` → Expected PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/benchmark/config.rs
git commit -m "feat(score): add blend/junction/health config constants"
```

---

## Task 3: Introduce `congested_junctions` (field + measure + all sites) — one green commit

**Files:** Modify `record.rs`, `measure.rs`, `state.rs` (test literals), `score.rs` (test literals), `server.rs` (test literals)

Add the required field and wire every construction site at once, leaving scoring logic unchanged so the whole suite stays green. `measure_window` produces the real value; everything else just carries it.

- [ ] **Step 1: `record.rs` — schema bump + field**

Replace:
```rust
/// v2: congestion-based scoring — `WindowStats.congested_meters`,
/// `ScoreNorms.congestion`, `Score.flow_gain`, `EndReason::CongestionTarget`.
pub const SCHEMA_VERSION: u32 = 2;
```
with:
```rust
/// v2: congestion-based scoring — `WindowStats.congested_meters`, etc.
/// v3: junction-aware scoring — `WindowStats.congested_junctions`, blended
/// congestion term, graded population-health factor (no hard pop guard).
pub const SCHEMA_VERSION: u32 = 3;
```

Replace the `WindowStats` struct's fields by adding `pub congested_junctions: u32,` after `pub congested_meters: f64,`.

In `mod tests`, add `congested_junctions` to all three `WindowStats` literals:
- `end_state_round_trips`: the `baseline: Some(WindowStats { ... congested_meters: 500.0 })` → add `, congested_junctions: 12`.
- `run_record_round_trips`: `baseline: WindowStats { ... congested_meters: 500.0 }` → add `, congested_junctions: 8`; `final_stats: WindowStats { ... congested_meters: 100.0 }` → add `, congested_junctions: 2`.

- [ ] **Step 2: `measure.rs` — produce the value**

Replace `use crate::benchmark::congestion::WindowAccum;` with:
```rust
use crate::benchmark::congestion::{congested_junctions, Topology, WindowAccum};
```

In `measure_window`, after `client.clock("pause", None, None).await?;` add:
```rust
    let topology = Topology::from_network(&client.network().await?);
```

Replace the returned `stats: WindowStats { ... congested_meters: accum.congested_meters(cfg.congestion_threshold), }` literal by adding, after the `congested_meters:` line:
```rust
            congested_junctions: congested_junctions(
                &topology,
                |id| accum.mean_density(id),
                cfg.congestion_threshold,
                cfg.junction_min_degree as usize,
                cfg.junction_min_congested as usize,
            ),
```

In `mod tests`, in `measures_window_stats_on_empty_city`, after `assert_eq!(m.stats.congested_meters, 0.0);` add:
```rust
        assert_eq!(m.stats.congested_junctions, 0);
```

Also confirm the mock serves `network()`: `grep -n "network" broker/src/mock.rs | head`. If there is no network route, STOP and report NEEDS_CONTEXT (the empty-city measure test will hang/fail without one); the mock needs a route returning `{"nodes":[],"segments":[]}`.

- [ ] **Step 3: `score.rs`, `state.rs`, `server.rs` — add the field to every other `WindowStats` literal (logic unchanged)**

Find every remaining `WindowStats {` literal and add `congested_junctions: 0,`:
- `score.rs` `mod tests`: the `record(...)` helper builds `baseline:` and `final_stats:` literals — add the field to both.
- `state.rs` `mod tests`: `congestion_end_condition_arms_only_with_baseline` builds a `WindowStats { ... congested_meters: 100.0 }` in a `set_baseline(...)` call — add `, congested_junctions: 0`. Also the `sample_metrics` helper builds a `crate::contract::Metrics` (not WindowStats) — leave it.
- `server.rs` `mod tests`: two `WindowStats { ... congested_meters: 100.0 }` literals (around lines 925–926 and 1274–1275) — add `, congested_junctions: 0` to each.

Verify none missed: `grep -rn "WindowStats {" broker/src | grep -v congested_junctions` → Expected: no matches (every literal now has the field).

- [ ] **Step 4: Full suite green**

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: PASS (behaviour unchanged; the new measured field is just carried through; old scoring untouched).

- [ ] **Step 5: Commit**

```bash
git add broker/src/benchmark/record.rs broker/src/benchmark/measure.rs broker/src/benchmark/score.rs broker/src/benchmark/state.rs broker/src/benchmark/server.rs
git commit -m "feat(score): measure congested_junctions through to WindowStats (schema v3)"
```

---

## Task 4: (reserved — folded into Task 3)

No-op; numbering preserved so later references stay stable.

---

## Task 5: Blended + health-multiplied score; drop hard cliff

**Files:** Modify `record.rs` (`Score` diagnostics), `score.rs`, `config.rs` (remove `pop_guard_ratio`)

- [ ] **Step 1: `record.rs` — `Score` diagnostics**

Add three fields to `struct Score`, after `pub flow_gain: f64,` and before `pub score: f64,`:
```rust
    /// Diagnostics for the blended congestion term and the health factor.
    pub meters_reduction: f64,
    pub junction_reduction: f64,
    pub health: f64,
```

- [ ] **Step 2: `score.rs` — failing tests**

Replace the `fn record(...)` helper with a thin wrapper plus a junction-aware builder:

```rust
    fn record(baseline_cm: f64, final_cm: f64, money: i64, changes: u32, pop_base: u32, pop_final: u32) -> RunRecord {
        record_j(baseline_cm, final_cm, 0, 0, money, changes, pop_base, pop_final)
    }

    #[allow(clippy::too_many_arguments)]
    fn record_j(
        baseline_cm: f64, final_cm: f64, baseline_junctions: u32, final_junctions: u32,
        money: i64, changes: u32, pop_base: u32, pop_final: u32,
    ) -> RunRecord {
        RunRecord {
            schema_version: SCHEMA_VERSION,
            config: BenchConfig::default(),
            map: MapInfo { id: "m".into(), source: "s".into(), game_version: "v".into() },
            started_at: "a".into(),
            ended_at: "b".into(),
            end_reason: EndReason::Submit,
            baseline: WindowStats { flow_mean: 55.0, active_vehicles_mean: 2000.0, population: pop_base, congested_meters: baseline_cm, congested_junctions: baseline_junctions },
            final_stats: WindowStats { flow_mean: 60.0, active_vehicles_mean: 2000.0, population: pop_final, congested_meters: final_cm, congested_junctions: final_junctions },
            flow_samples: FlowSamples { baseline: vec![], final_samples: vec![] },
            tally: Tally { num_changes: changes, money_spent: money },
            actions: vec![],
        }
    }
```

Replace the two population-guard tests:
```rust
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
```
with:
```rust
    #[test]
    fn total_population_collapse_drives_health_and_score_to_zero() {
        let s = score_run(&record(1000.0, 0.0, 0, 0, 30_000, 110), &BenchConfig::default());
        assert!((s.health - 0.0).abs() < 1e-9);
        assert!((s.score - 0.0).abs() < 1e-9);
    }

    #[test]
    fn stable_population_has_full_health() {
        let s = score_run(&record(1000.0, 0.0, 0, 0, 30_000, 30_000), &BenchConfig::default());
        assert!((s.health - 1.0).abs() < 1e-9);
    }

    #[test]
    fn depopulation_for_a_small_congestion_gain_nets_below_do_nothing() {
        let depop = score_run(&record_j(1000.0, 800.0, 10, 9, 0, 20, 30_000, 25_200), &BenchConfig::default());
        let do_nothing = score_run(&record(1000.0, 1000.0, 0, 0, 30_000, 30_000), &BenchConfig::default());
        assert!((do_nothing.score - 0.40).abs() < 1e-9, "do-nothing = {}", do_nothing.score);
        assert!(depop.score < do_nothing.score, "depop {} must be < do-nothing {}", depop.score, do_nothing.score);
    }

    #[test]
    fn congestion_term_blends_meters_and_junctions_5050() {
        let s = score_run(&record_j(1000.0, 600.0, 10, 2, 0, 0, 30_000, 30_000), &BenchConfig::default());
        assert!((s.meters_reduction - 0.4).abs() < 1e-9);
        assert!((s.junction_reduction - 0.8).abs() < 1e-9);
        assert!((s.norm.congestion - 0.6).abs() < 1e-9, "blended = {}", s.norm.congestion);
        assert!((s.score - 0.76).abs() < 1e-9, "score = {}", s.score);
    }

    #[test]
    fn zero_baseline_junctions_falls_back_to_meters_only() {
        let s = score_run(&record_j(1000.0, 500.0, 0, 0, 0, 0, 30_000, 30_000), &BenchConfig::default());
        assert!((s.norm.congestion - 0.5).abs() < 1e-9, "meters-only = {}", s.norm.congestion);
    }
```

Run: `cargo test --manifest-path broker/Cargo.toml --lib score` → Expected FAIL (logic + `Score` fields not updated yet).

- [ ] **Step 3: `score.rs` — rewrite `score_run`**

Replace the whole `pub fn score_run(...)` body:

```rust
pub fn score_run(record: &RunRecord, cfg: &BenchConfig) -> Score {
    debug_assert!(
        cfg.budget > 0.0 && cfg.change_cap > 0.0,
        "BenchConfig normalization denominators must be positive"
    );
    let baseline_cm = record.baseline.congested_meters;
    let final_cm = record.final_stats.congested_meters;
    let baseline_j = record.baseline.congested_junctions;
    let final_j = record.final_stats.congested_junctions;

    let meters_reduction =
        if baseline_cm > 0.0 { clamp01((baseline_cm - final_cm) / baseline_cm) } else { 0.0 };
    let junction_reduction = if baseline_j > 0 {
        clamp01((f64::from(baseline_j) - f64::from(final_j)) / f64::from(baseline_j))
    } else {
        0.0
    };
    // No baseline junctions => the junction signal is meaningless; weight the
    // whole congestion term onto meters.
    let (a, b) = if baseline_j > 0 { (cfg.blend_meters, cfg.blend_junctions) } else { (1.0, 0.0) };
    let congestion_reward = a * meters_reduction + b * junction_reduction;

    let pop_ratio = if record.baseline.population > 0 {
        f64::from(record.final_stats.population) / f64::from(record.baseline.population)
    } else {
        1.0
    };
    let health = clamp01((pop_ratio - cfg.health_zero) / (cfg.health_full - cfg.health_zero));

    let norm = ScoreNorms {
        congestion: congestion_reward,
        money: clamp01(record.tally.money_spent as f64 / cfg.budget),
        changes: clamp01(record.tally.num_changes as f64 / cfg.change_cap),
    };
    let weighted = ScoreNorms {
        congestion: cfg.w_congestion * norm.congestion,
        money: cfg.w_money * (1.0 - norm.money),
        changes: cfg.w_changes * (1.0 - norm.changes),
    };
    // A map with no congestion to fix is unscorable. Total population collapse
    // now zeroes the score smoothly through `health`, not a hard cliff.
    let invalid = baseline_cm <= 0.0;
    let score = if invalid {
        0.0
    } else {
        (weighted.congestion + weighted.money + weighted.changes) * health
    };
    Score {
        norm,
        weighted,
        invalid,
        flow_gain: record.final_stats.flow_mean - record.baseline.flow_mean,
        meters_reduction,
        junction_reduction,
        health,
        score,
    }
}
```

Run: `cargo test --manifest-path broker/Cargo.toml --lib score` → Expected PASS.

- [ ] **Step 4: `config.rs` — remove now-unused `pop_guard_ratio`**

`score_run` no longer reads it; nothing else does. Delete the field + doc comment, the `Default` line `pop_guard_ratio: 0.8,`, and in `mod tests` the line `assert_eq!(c.pop_guard_ratio, 0.8);` inside `congestion_dominates_and_guards_are_calibrated`.

Run: `cargo test --manifest-path broker/Cargo.toml --lib` → Expected PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/benchmark/record.rs broker/src/benchmark/score.rs broker/src/benchmark/config.rs
git commit -m "feat(score): blend meters+junctions, graded health factor, drop hard cliff"
```

---

# Phase 2 — agent-facing surface

## Task 6: Live junction readout + `city_status` fields in `state.rs`

**Files:** Modify `broker/src/benchmark/state.rs`, `broker/src/benchmark/config.rs` (remove `congestion_end_ratio`)

- [ ] **Step 1: Imports + fields**

Replace `use crate::benchmark::congestion::instant_congested_meters;` with:
```rust
use crate::benchmark::congestion::{congested_junctions, instant_congested_meters, Topology};
use crate::contract::Network;
```

In `struct RunState`, after `pub last_happiness: Option<u8>,` add:
```rust
    pub topology: Option<Topology>,
    pub last_densities: HashMap<u32, f64>,
```
In `RunState::new`, after `last_happiness: None,` add:
```rust
            topology: None,
            last_densities: HashMap::new(),
```

- [ ] **Step 2: Rewrite `observe_metrics`; add network/junction helpers; drop arming**

Replace the whole `observe_metrics` body:
```rust
    pub fn observe_metrics(&mut self, m: &Metrics) {
        self.flow.push(m.traffic.flow_percent as f64);
        self.congestion
            .push(instant_congested_meters(&m.traffic.segment_loads, self.config.congestion_threshold));
        self.last_population = Some(m.population.total);
        self.last_abandoned_buildings = Some(m.services.abandoned_buildings);
        self.last_happiness = Some(m.services.happiness);
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
with:
```rust
    pub fn observe_metrics(&mut self, m: &Metrics) {
        self.flow.push(m.traffic.flow_percent as f64);
        self.congestion
            .push(instant_congested_meters(&m.traffic.segment_loads, self.config.congestion_threshold));
        self.last_population = Some(m.population.total);
        self.last_abandoned_buildings = Some(m.services.abandoned_buildings);
        self.last_happiness = Some(m.services.happiness);
        self.last_densities = m
            .traffic
            .segment_loads
            .iter()
            .map(|l| (l.segment_id, f64::from(l.density)))
            .collect();
    }

    /// Cache the road graph so the live readout can count congested junctions.
    pub fn observe_network(&mut self, net: &Network) {
        self.topology = Some(Topology::from_network(net));
    }

    fn live_congested_junctions(&self) -> Option<u32> {
        self.topology.as_ref().map(|t| {
            congested_junctions(
                t,
                |id| self.last_densities.get(&id).copied(),
                self.config.congestion_threshold,
                self.config.junction_min_degree as usize,
                self.config.junction_min_congested as usize,
            )
        })
    }
```

(`EndReason` is still used by `check_timeout`, so keep its import.)

- [ ] **Step 3: Rewrite `progress()` to the `city_status` field set**

Replace the whole `progress()` method:
```rust
    pub fn progress(&self) -> Value {
        let baseline_cm = self.baseline.as_ref().map(|b| b.congested_meters);
        json!({
            "money_spent": self.money_spent,
            "num_changes": self.num_changes,
            "congested_meters_current": (!self.congestion.is_empty()).then(|| self.congestion.mean()),
            "congested_meters_baseline": baseline_cm,
            "congested_meters_target": baseline_cm.map(|b| self.config.congestion_end_ratio * b),
            "flow_current": (!self.flow.is_empty()).then(|| self.flow.mean()),
            "population": self.last_population,
            "abandoned_buildings": self.last_abandoned_buildings,
            "happiness": self.last_happiness,
            "seconds_remaining": self.seconds_remaining(),
        })
    }
```
with:
```rust
    /// Neutral simulation readout merged into every tool response — no scoring
    /// formula, weights, caps, or thresholds, just observable city facts.
    pub fn progress(&self) -> Value {
        json!({
            "money_spent": self.money_spent,
            "changes_made": self.num_changes,
            "congested_road_meters": (!self.congestion.is_empty()).then(|| self.congestion.mean()),
            "congested_road_meters_at_start": self.baseline.as_ref().map(|b| b.congested_meters),
            "congested_junctions": self.live_congested_junctions(),
            "congested_junctions_at_start": self.baseline.as_ref().map(|b| b.congested_junctions),
            "traffic_flow": (!self.flow.is_empty()).then(|| self.flow.mean()),
            "population": self.last_population,
            "abandoned_buildings": self.last_abandoned_buildings,
            "happiness": self.last_happiness,
            "time_remaining": self.seconds_remaining(),
        })
    }
```

- [ ] **Step 4: Update `state.rs` tests**

In `progress_omits_score_fields`, replace the old-key assertions:
```rust
        assert!(p["congested_meters_current"].is_null(), "no samples yet");
        assert!(p["flow_current"].is_null(), "no samples yet");
        assert!(p["congested_meters_baseline"].is_null(), "no baseline yet");
        assert!(p["seconds_remaining"].as_u64().unwrap() <= 10_800);
        assert!(p.get("score").is_none());
        assert!(p.get("composite_score").is_none());
        assert!(p.get("weights").is_none());
        assert!(p["happiness"].is_null(), "no happiness before first sample");

        s.observe_metrics(&sample_metrics(0.9));
        let p = s.progress();
        assert!(p["congested_meters_current"].is_number(), "current appears after first sample");
        assert!(p["flow_current"].is_number());
        assert_eq!(p["happiness"], 80, "happiness surfaced from the latest sample");
    }
```
with:
```rust
        assert!(p["congested_road_meters"].is_null(), "no samples yet");
        assert!(p["traffic_flow"].is_null(), "no samples yet");
        assert!(p["congested_road_meters_at_start"].is_null(), "no baseline yet");
        assert!(p["time_remaining"].as_u64().unwrap() <= 10_800);
        assert!(p.get("score").is_none());
        assert!(p.get("composite_score").is_none());
        assert!(p.get("weights").is_none());
        assert!(p.get("congested_meters_target").is_none(), "scoring target must not leak");
        assert!(p["happiness"].is_null(), "no happiness before first sample");

        s.observe_metrics(&sample_metrics(0.9));
        let p = s.progress();
        assert!(p["congested_road_meters"].is_number(), "current appears after first sample");
        assert!(p["traffic_flow"].is_number());
        assert_eq!(p["happiness"], 80, "happiness surfaced from the latest sample");
    }
```

Delete the test `congestion_end_condition_arms_only_with_baseline` entirely (the arming logic it covers is gone). If removing it leaves `WindowStats` / `EndReason::CongestionTarget` imports unused in the test module, remove those unused imports.

- [ ] **Step 5: `config.rs` — remove now-unused `congestion_end_ratio`**

Nothing reads it anymore. Delete the field + doc comment, the `Default` line `congestion_end_ratio: 0.05,`, and in `mod tests` the line `assert_eq!(c.congestion_end_ratio, 0.05);`. Keep the `EndReason::CongestionTarget` enum variant in `record.rs` (valid for old records; never set now).

- [ ] **Step 6: Test**

Run: `cargo test --manifest-path broker/Cargo.toml --lib state config` → Expected PASS.

- [ ] **Step 7: Commit**

```bash
git add broker/src/benchmark/state.rs broker/src/benchmark/config.rs
git commit -m "feat(server): city_status readout with live junctions; drop auto-stop-at-target"
```

---

## Task 7: Rename block to `city_status` + topology refresh in `server.rs`

**Files:** Modify `broker/src/benchmark/server.rs` (and `broker/tests/broker_e2e.rs` if it asserts on the block)

- [ ] **Step 1: Rename at both insert sites + doc + test**

Change `"benchmark_progress"` → `"city_status"` at:
- `with_progress` (~line 69): `map.insert("benchmark_progress".into(), state.progress());`
- render_map (~line 314): `map.insert("benchmark_progress".into(), progress);`
- the test at ~line 902: `merged["benchmark_progress"]["congested_meters_current"]` → `merged["city_status"]["congested_road_meters"]`.

Update the doc comment (~line 4) and the tool-router description (~line 856) — change `benchmark_progress` to `city_status`.

Verify: `grep -n '"benchmark_progress"' broker/src/benchmark/server.rs` → Expected: no matches.

- [ ] **Step 2: Refresh topology at baseline**

In `ensure_baseline`, after the `if let Ok(m) = measure_window(...) { ... }` block closes (after the `}` near line 225–226), add:
```rust
        if let Ok(net) = self.client.network().await {
            self.state.lock().await.observe_network(&net);
        }
```

- [ ] **Step 3: Refresh topology after network-changing actions**

Mutating tools (`build_road`, `bulldoze`, `upgrade_road`, `set_zoning`, `apply_plan`) change the graph. Find where they funnel their result (read `server.rs` around the mutating handlers and the `finish`/record helper). Prefer a single shared post-mutation point; add there:
```rust
        if let Ok(net) = self.client.network().await {
            self.state.lock().await.observe_network(&net);
        }
```
If there is no single shared point, add it in each mutating handler after the action succeeds and before the response is built. Do NOT add it to read-only tools (wasteful). If you cannot identify the mutation path within ~10 minutes of reading, STOP and report NEEDS_CONTEXT with what you found.

- [ ] **Step 4: Full suite green**

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: PASS. If `broker/tests/broker_e2e.rs` asserts on `benchmark_progress`/old field names, update it to `city_status` + new names.

- [ ] **Step 5: Commit**

```bash
git add broker/src/benchmark/server.rs broker/tests/broker_e2e.rs
git commit -m "feat(server): rename telemetry to city_status; refresh topology on baseline + mutations"
```

---

## Task 8: Transcript renderer reads `city_status` (fallback to old)

**Files:** Modify `broker/src/benchmark/transcript.rs`

- [ ] **Step 1: Prefer `city_status` and new keys with fallback**

Replace `if let Some(p) = v.get("benchmark_progress") {` (~line 138) with:
```rust
        if let Some(p) = v.get("city_status").or_else(|| v.get("benchmark_progress")) {
```

Replace the block from `let rejected = ...` through the `return Some(format!(...));`:
```rust
            let rejected = v.get("ok").and_then(|x| x.as_bool()) == Some(false);
            let target = opt("congested_meters_target", 0);
            return Some(format!(
                "    ↳ congested {}m/{target}m  flow {}  changes {}  spent {}  {}s left{}",
                opt("congested_meters_current", 0),
                opt("flow_current", 1),
                p.get("num_changes").and_then(|x| x.as_u64()).unwrap_or(0),
                p.get("money_spent").and_then(|x| x.as_i64()).unwrap_or(0),
                p.get("seconds_remaining").and_then(|x| x.as_u64()).unwrap_or(0),
                if rejected { "  (rejected)" } else { "" },
            ));
```
with:
```rust
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
```
If the `let opt = |...|` closure defined just above is now unused, delete it (the compiler will warn).

- [ ] **Step 2: Add a new-format test (keep the old-format one)**

Keep `live_surfaces_benchmark_progress` (verifies old transcripts still render via fallback). Open the file and copy the exact way it invokes the renderer; add a sibling test using that same call form with a `city_status` payload:
```rust
    // Build/feed the same way live_surfaces_benchmark_progress does, with this payload:
    // {"type":"user","message":{"content":[{"type":"tool_result","content":[{"type":"text",
    //  "text":"{\"ok\":true,\"city_status\":{\"money_spent\":12000,\"changes_made\":3,
    //  \"congested_road_meters\":840.0,\"congested_junctions\":7,\"traffic_flow\":12.3,
    //  \"time_remaining\":580}}"}]}]}}
    // Assert the rendered line contains "840m", "7 junctions", and "580s left".
```
Implement it concretely by mirroring the existing test's structure (same helper/function, same assertion style).

- [ ] **Step 3: Test**

Run: `cargo test --manifest-path broker/Cargo.toml transcript` → Expected PASS (old + new format).

- [ ] **Step 4: Commit**

```bash
git add broker/src/benchmark/transcript.rs
git commit -m "feat(transcript): render city_status (junctions) with benchmark_progress fallback"
```

---

## Task 9: Rewrite the agent prompt

**Files:** Modify `benchmark/prompt.md` (full rewrite)

- [ ] **Step 1: Replace the whole file** with:

```markdown
You are a traffic engineer optimising the road network of this Cities: Skylines city in a live simulation. Your job is to make traffic flow better while keeping the city a good place to live.

You have tools to observe and modify the city:
- Observe (free, unlimited): `get_city_overview`, `observe_area`, `get_metrics`, `list_road_types`, `list_zone_types`, `render_map`, `query_segments` (worst-N congestion search), `trace_route` (estimate the path traffic takes between two points — use it to check a planned link will attract traffic).
- Modify: `build_road`, `bulldoze`, `upgrade_road`, `set_zoning`.
  Note: `build_road` snaps endpoints to existing network nodes within 8 m. Use node positions
  from `observe_area` or the `start_node_pos`/`end_node_pos` fields in `query_segments` results
  — **not** the `midpoint` field, which is the geographic center of a segment, not a node.
  If the response contains `"isolated_island": true`, neither endpoint connected to the network
  and the road is useless — bulldoze it and retry with corrected coordinates.
  Note: `upgrade_road` re-creates the segment under a NEW id (the response maps old → new);
  refresh any segment ids you cached before reusing them.
- Modify in batch: `apply_plan` stages several ops (including multi-point polylines that
  auto-split under the 200 m segment cap) in one call, validates and prices ALL of them
  before anything executes, and supports `validate_only: true` as a free dry-run. Prefer
  one validated plan per rebuild over loose single calls. A `validate_only` dry-run also
  checks build ops against the game's placement rules (collision / slope / map area) and
  reports the reason when one would fail.
- Time: `control_time` (pause / resume / step / speed). Build while paused, then `step` to let traffic respond before measuring. A `step` with no `ticks` advances one in-game day (585 ticks). The maximum step is 7 days (4095 ticks) — traffic patterns repeat daily, so longer waits only burn wall-clock time.
- Finish: `submit_solution` when you are satisfied with the city. It returns immediately; the simulation is settled and assessed after your session ends, so finish your turn once it succeeds.

You may write and execute scratch code in your working directory (e.g. scripts that parse tool output or plan a batch of changes), and you are encouraged to keep a running record — scratch notes or memory — of what you have tried and what worked or backfired, updated as you go so each decision builds on the last. The city is reachable ONLY through the tools above; do not try to read the simulation's own files or anything outside your working directory.

What you are optimising — weigh all of these together:
- **Traffic congestion.** Reduce it on two fronts: the total length of road running at high traffic density, and the number of junctions (intersections) where traffic backs up. Both are reported to you. A long arterial relieved and a jammed interchange untangled are both real wins; chase whichever is actually costing the city.
- **A healthy city.** The people who live here must want to stay. A shrinking population, rising abandonment, or falling happiness means your changes are hurting the city — that is a failure, not a shortcut to less traffic. Watch these readouts and treat a sustained decline as a problem to fix immediately.
- **Cost and disruption.** Spend money sensibly and don't make changes that aren't pulling their weight. But don't be timid: if fixing a bottleneck means tearing out and rebuilding a junction, do it — bold structural change is the right call when it's what the problem needs. Just don't destabilise the city so badly that it spirals into decline you can't recover from.

Every tool response includes a `city_status` block: money spent, changes made, congested road metres (now / at-start), congested junctions (now / at-start), traffic flow, population, abandoned buildings, happiness, and time remaining. Use it to track where you stand.

Work method — repeat this loop:
1. **Explore.** Survey the whole network (`render_map` at several zooms, `observe_area`, `get_metrics`) and find where traffic actually loses time — chokepoints, bad interchanges, missing links — not just where density looks high. Pay attention to which junctions are congested, not only which stretches of road.
2. **Plan.** For the worst problem, write a concrete multi-step plan: which segments to bulldoze, what to build in their place, and how the new geometry will route traffic. Before you commit a change, think through its side effects, not just the local fix:
   - Will it push congestion onto — or create new bottlenecks in — *other* parts of the network, rather than actually removing it?
   - Will it hurt the area around it — happiness, land value, noise — for the buildings beside it?
   - Will it strand or cut off part of the city (lost road frontage, a severed connection)?
   A change that helps vehicles but empties the buildings around it costs you both residents and the traffic you were trying to redistribute.
3. **Execute.** Pause, apply the plan as a batch, then `step` a day or more so traffic re-routes onto the new layout.
4. **Validate — and tell the two regimes apart:**
   - *Traffic re-routing is slow.* Congestion often gets **worse for the first few steps** after a structural change while vehicles find the new layout, then settles. Don't judge a bold change on its first measurement — `step` several days and look at the settled result. Only revert once settling confirms the change left congestion durably worse than before.
   - *Livability damage compounds.* If population or happiness is sliding, or abandonment is climbing, that is **not** a settling transient — it gets worse the longer you wait. Act immediately: find what you changed near those buildings and undo it, rather than stepping through a long settle while the city empties.

Think like a traffic engineer, not a road painter: widening a road in place rarely fixes congestion caused by bad geometry. The high-leverage moves are structural — rebuild a failing junction simpler, add a bypass or a missing crossing, separate through-traffic from local traffic. When widening a corridor stops lowering its density, that is a sign the bottleneck is geometric, not a lane shortage — change the geometry. Expect some interventions to do nothing; diagnose why and change approach rather than giving up. Submit when you have evidence that further changes won't sustainably improve the city.
```

- [ ] **Step 2: Verify no scoring/benchmark/map-specific leakage**

Run: `grep -inE "benchmark|score|scored|weight|congestion_reduction|invalid|80%|budget| cap|gridlock|disqualif|0\.[0-9]" benchmark/prompt.md`
Expected: the only matches are the tool mechanics (8 m, 200 m, 585/4095 ticks — none of which match these patterns) → effectively no matches. Confirm every line returned (if any) is a tool mechanic, not scoring/benchmark/map language.

- [ ] **Step 3: Commit**

```bash
git add benchmark/prompt.md
git commit -m "prompt: reframe as simulation optimisation; hide scoring; junction + fast/slow-revert guidance"
```

---

## Task 10: README scoring + framing note

**Files:** Modify `benchmark/README.md`

- [ ] **Step 1: Replace the Scoring section** (the block from `## Scoring (spec §4)` through `keep prompt.md in sync when retuning them.`) with:

```markdown
## Scoring (operator-facing — the agent is NOT told this)

The agent prompt frames the task as "optimise this city's traffic simulation" and
states its objectives qualitatively; it is deliberately **not** told the formula,
the weights, the caps, or the population thresholds, so it optimises the city
rather than the scoreboard.

`score = (0.60·congestion_reward + 0.20·(1−norm(money)) + 0.20·(1−norm(changes))) · health`

- `congestion_reward = blend_meters·meters_reduction + blend_junctions·junction_reduction`
  (default 0.5/0.5; falls back to meters-only when the baseline has no congested junctions).
- `meters_reduction = max(0, baseline_congested_meters − final_congested_meters) / baseline_congested_meters`.
- `junction_reduction = max(0, baseline_congested_junctions − final_congested_junctions) / baseline_congested_junctions`.
  A **congested junction** is a node of degree ≥ `junction_min_degree` (3) with ≥ `junction_min_congested` (2)
  incident segments at density ≥ `congestion_threshold` (0.7), measured over the final window.
- `health` is a graded population factor (1.0 at population ≥ `health_full`·baseline (0.95),
  0.0 at ≤ `health_zero`·baseline (0.75), linear between) that replaces the old hard 80% cliff —
  depopulating the city drags the score down smoothly instead of being free above 80% or zero below it.
- A run is invalid (score 0) only when the baseline has no congestion to fix.
- Money is normalised against a $10,000,000 budget; changes against a 300-change cap.

Constants live in `broker/src/benchmark/config.rs`. The run ends on `submit_solution`
or the wall-clock cap; the old auto-stop-at-5%-of-baseline condition was removed.
```

- [ ] **Step 2: Check for other stale references**

Run: `grep -n "flow_target\|congestion_end\|pop_guard\|composite\|congested_meters_target" benchmark/README.md`
Expected: no matches. Update any that appear to match the new scoring/telemetry.

- [ ] **Step 3: Commit**

```bash
git add benchmark/README.md
git commit -m "docs: README scoring reflects junctions + health; agent is not told the formula"
```

---

## Final verification (after all tasks)

- [ ] **Whole broker suite green:** `cargo test --manifest-path broker/Cargo.toml` → all PASS.
- [ ] **Release binary builds:** `cargo build --release --manifest-path broker/Cargo.toml` → success.
- [ ] **No leftover block name in shipped code:** `grep -rn '"benchmark_progress"' broker/src` → only the transcript fallback `.or_else(|| v.get("benchmark_progress"))` and the old-format test payload may remain; both server insert sites gone.
- [ ] **Prompt clean:** re-run Task 9 Step 2 grep → no scoring/benchmark/map-specific leakage.
- [ ] **Schema guard intact:** `finalize_bails_on_mismatched_schema_version` passes (version now 3).
- [ ] *(Optional, manual)* a live `--watch` run on `gridlock-v1`: `city_status` shows `congested_junctions`, the agent attempts at least one junction rework, and its reasoning shows no awareness of a scoring boundary.
```
