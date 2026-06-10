# Batch Actions (apply_plan) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the agent stage a whole rebuild as one `apply_plan` tool call: a list of ops (including multi-point polylines that auto-split under the 200 m segment cap), a `validate_only` dry-run that prices and checks the plan without touching the city, and stop-on-error execution.

**Architecture:** A new `broker/src/benchmark/plan.rs` holds the pure logic — op expansion (polyline → ≤180 m chunks), pre-validation against a city snapshot, no game I/O — so it is unit-testable. The `BenchmarkServer` gains an `apply_plan` tool that snapshots road/zone types + network once, expands, validates ALL ops up front (any invalid op rejects the whole plan before anything executes), then either returns the priced validation (dry-run) or executes sequentially through the existing `service::*` functions, recording one change per successful op — identical accounting to the single-op tools, so scores stay comparable.

**Tech Stack:** Rust (serde tagged enums, rmcp, mock-mod tests).

**Evidence this matters:** run `20260609-210135` issued 169 modifications as 169 separate calls, learned the 200 m cap by trial ("Max segment ~100m, so I'll chunk the rest"), and left a half-applied interchange rebuild in place when errors hit mid-sequence.

**Change accounting decision:** one *expanded op* = one change (a 10-segment rebuild costs 10 changes, exactly as it would via single calls). Batching buys atomicity and round-trips, not a score discount — otherwise the changes score term loses meaning across runs.

---

### Task 1: Pure plan module — ops, expansion, validation

**Files:**
- Create: `broker/src/benchmark/plan.rs`
- Modify: `broker/src/benchmark/mod.rs` (add `pub mod plan;`)

- [ ] **Step 1: Write the failing tests**

Create `broker/src/benchmark/plan.rs` with the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{ActionError, Position, RoadType};
    use std::collections::{HashMap, HashSet};

    fn pos(x: f32, z: f32) -> Position {
        Position { x, y: 0.0, z }
    }

    fn ctx() -> ExecCtx {
        ExecCtx {
            road_types: vec![RoadType { name: "road".into(), construction_cost: 1000 }],
            zone_types: vec!["residential".into()],
            segment_ids: HashSet::from([10]),
            node_ids: HashSet::from([1, 2]),
            building_ids: HashSet::new(),
            segment_lengths: HashMap::from([(10, 64.0)]),
        }
    }

    #[test]
    fn split_span_respects_chunk_length() {
        let chunks = split_span(pos(0.0, 0.0), pos(500.0, 0.0));
        assert_eq!(chunks.len(), 3);
        assert!((chunks[0].1.x - 166.66667).abs() < 0.01);
        assert_eq!(chunks[0].0.x, 0.0);
        assert_eq!(chunks[2].1.x, 500.0);
        let max = chunks
            .iter()
            .map(|(a, b)| crate::geometry::horizontal_distance(*a, *b))
            .fold(0.0_f32, f32::max);
        assert!(max <= POLYLINE_CHUNK_M + 0.01, "max chunk {max}");
    }

    #[test]
    fn short_span_is_one_chunk() {
        assert_eq!(split_span(pos(0.0, 0.0), pos(50.0, 0.0)).len(), 1);
    }

    #[test]
    fn expand_polyline_chains_points_and_splits() {
        let ops = vec![PlanOp::BuildPolyline {
            points: vec![pos(0.0, 0.0), pos(250.0, 0.0), pos(250.0, 100.0)],
            road_type: "road".into(),
            snap: true,
        }];
        let exec = expand(&ops);
        // 250m span → 2 chunks, 100m span → 1 chunk.
        assert_eq!(exec.len(), 3);
        assert!(exec.iter().all(|(source, _)| *source == 0));
        match &exec[2].1 {
            ExecOp::Build { from, to, .. } => {
                assert_eq!(from.x, 250.0);
                assert_eq!(to.z, 100.0);
            }
            other => panic!("expected Build, got {other:?}"),
        }
    }

    #[test]
    fn expand_splits_long_build_road_too() {
        let ops = vec![PlanOp::BuildRoad {
            from: pos(0.0, 0.0),
            to: pos(400.0, 0.0),
            road_type: "road".into(),
            snap: true,
        }];
        assert_eq!(expand(&ops).len(), 3);
    }

    #[test]
    fn validate_catches_each_failure_mode() {
        let c = ctx();
        let bad_prefab = ExecOp::Build { from: pos(0.0, 0.0), to: pos(50.0, 0.0), road_type: "monorail".into(), snap: true };
        assert_eq!(validate(&bad_prefab, &c), Err(ActionError::InvalidPrefab));

        let missing_segment = ExecOp::Upgrade { segment: 99, road_type: "road".into() };
        assert_eq!(validate(&missing_segment, &c), Err(ActionError::InvalidArgs));

        let missing_bulldoze = ExecOp::Bulldoze { target_type: "segment".into(), id: 99 };
        assert_eq!(validate(&missing_bulldoze, &c), Err(ActionError::InvalidArgs));

        let bad_zone = ExecOp::Zone {
            area: crate::contract::Bounds { min_x: 0.0, min_z: 0.0, max_x: 8.0, max_z: 8.0 },
            zone_type: "spaceport".into(),
        };
        assert_eq!(validate(&bad_zone, &c), Err(ActionError::InvalidArgs));

        let good = ExecOp::Upgrade { segment: 10, road_type: "road".into() };
        assert_eq!(validate(&good, &c), Ok(()));
    }

    #[test]
    fn degenerate_polyline_is_invalid() {
        let ops = vec![PlanOp::BuildPolyline { points: vec![pos(0.0, 0.0)], road_type: "road".into(), snap: true }];
        let exec = expand(&ops);
        assert_eq!(exec.len(), 1);
        assert_eq!(validate(&exec[0].1, &ctx()), Err(ActionError::InvalidArgs));
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --manifest-path broker/Cargo.toml benchmark::plan`
Expected: FAIL — module contents missing. (Add `pub mod plan;` to `broker/src/benchmark/mod.rs` first so the file compiles into the tree.)

- [ ] **Step 3: Implement the module**

Above the test module in `broker/src/benchmark/plan.rs`:

```rust
//! Pure batch-plan logic for the `apply_plan` tool: op expansion (polylines →
//! straight chunks under the segment-length cap) and pre-validation against a
//! city snapshot. No game I/O here — the server wires it to the bridge.

use std::collections::{HashMap, HashSet};

use serde::Deserialize;

use crate::contract::{ActionError, Bounds, Position, RoadType};
use crate::geometry::horizontal_distance;
use crate::validate::validate_build_road;

/// Spans are split into chunks at most this long — comfortably under the game's
/// 200 m segment cap so endpoint snapping can't push a chunk over it.
pub const POLYLINE_CHUNK_M: f32 = 180.0;
/// Limits keeping one tool call's work (and wall-clock) bounded.
pub const MAX_OPS: usize = 50;
pub const MAX_EXPANDED_OPS: usize = 120;

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PlanOp {
    /// Straight link; spans longer than the segment cap are auto-split.
    BuildRoad { from: Position, to: Position, road_type: String, #[serde(default = "default_true")] snap: bool },
    /// Poly-link through `points` in order; each leg is auto-split.
    BuildPolyline { points: Vec<Position>, road_type: String, #[serde(default = "default_true")] snap: bool },
    UpgradeRoad { segment: u32, road_type: String },
    Bulldoze { target_type: String, id: u32 },
    SetZoning { area: Bounds, zone_type: String },
}

fn default_true() -> bool {
    true
}

/// A primitive, directly executable op (post-expansion).
#[derive(Debug, Clone, PartialEq)]
pub enum ExecOp {
    Build { from: Position, to: Position, road_type: String, snap: bool },
    Upgrade { segment: u32, road_type: String },
    Bulldoze { target_type: String, id: u32 },
    Zone { area: Bounds, zone_type: String },
    /// Placeholder for a source op that cannot expand (e.g. a 1-point
    /// polyline); always fails validation with INVALID_ARGS.
    Invalid,
}

/// City snapshot the plan is validated against.
pub struct ExecCtx {
    pub road_types: Vec<RoadType>,
    pub zone_types: Vec<String>,
    pub segment_ids: HashSet<u32>,
    pub node_ids: HashSet<u32>,
    pub building_ids: HashSet<u32>,
    pub segment_lengths: HashMap<u32, f32>,
}

fn lerp_pos(a: Position, b: Position, t: f32) -> Position {
    Position {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
        z: a.z + (b.z - a.z) * t,
    }
}

/// Split one straight span into equal chunks no longer than POLYLINE_CHUNK_M.
pub fn split_span(from: Position, to: Position) -> Vec<(Position, Position)> {
    let len = horizontal_distance(from, to);
    let n = (len / POLYLINE_CHUNK_M).ceil().max(1.0) as usize;
    (0..n)
        .map(|i| {
            (
                lerp_pos(from, to, i as f32 / n as f32),
                lerp_pos(from, to, (i + 1) as f32 / n as f32),
            )
        })
        .collect()
}

/// Expand source ops into primitive ops, each tagged with its source index.
pub fn expand(ops: &[PlanOp]) -> Vec<(usize, ExecOp)> {
    ops.iter()
        .enumerate()
        .flat_map(|(i, op)| -> Vec<(usize, ExecOp)> {
            match op {
                PlanOp::BuildRoad { from, to, road_type, snap } => split_span(*from, *to)
                    .into_iter()
                    .map(|(a, b)| (i, ExecOp::Build { from: a, to: b, road_type: road_type.clone(), snap: *snap }))
                    .collect(),
                PlanOp::BuildPolyline { points, road_type, snap } => {
                    if points.len() < 2 {
                        return vec![(i, ExecOp::Invalid)];
                    }
                    points
                        .windows(2)
                        .flat_map(|w| split_span(w[0], w[1]))
                        .map(|(a, b)| (i, ExecOp::Build { from: a, to: b, road_type: road_type.clone(), snap: *snap }))
                        .collect()
                }
                PlanOp::UpgradeRoad { segment, road_type } => {
                    vec![(i, ExecOp::Upgrade { segment: *segment, road_type: road_type.clone() })]
                }
                PlanOp::Bulldoze { target_type, id } => {
                    vec![(i, ExecOp::Bulldoze { target_type: target_type.clone(), id: *id })]
                }
                PlanOp::SetZoning { area, zone_type } => {
                    vec![(i, ExecOp::Zone { area: *area, zone_type: zone_type.clone() })]
                }
            }
        })
        .collect()
}

/// Structural pre-validation against the snapshot. The game can still reject
/// an op at execution time (COLLISION, INSUFFICIENT_FUNDS) — only it knows.
pub fn validate(op: &ExecOp, ctx: &ExecCtx) -> Result<(), ActionError> {
    match op {
        ExecOp::Build { from, to, road_type, .. } => {
            validate_build_road(*from, *to, road_type, &ctx.road_types)
        }
        ExecOp::Upgrade { segment, road_type } => {
            if !ctx.road_types.iter().any(|t| t.name == *road_type) {
                return Err(ActionError::InvalidPrefab);
            }
            ctx.segment_ids.contains(segment).then_some(()).ok_or(ActionError::InvalidArgs)
        }
        ExecOp::Bulldoze { target_type, id } => {
            let known = match target_type.as_str() {
                "segment" => ctx.segment_ids.contains(id),
                "node" => ctx.node_ids.contains(id),
                "building" => ctx.building_ids.contains(id),
                _ => false,
            };
            known.then_some(()).ok_or(ActionError::InvalidArgs)
        }
        ExecOp::Zone { zone_type, .. } => ctx
            .zone_types
            .contains(zone_type)
            .then_some(())
            .ok_or(ActionError::InvalidArgs),
        ExecOp::Invalid => Err(ActionError::InvalidArgs),
    }
}

/// Tool name an exec op is recorded under (matches single-op accounting).
pub fn tool_name(op: &ExecOp) -> &'static str {
    match op {
        ExecOp::Build { .. } => "build_road",
        ExecOp::Upgrade { .. } => "upgrade_road",
        ExecOp::Bulldoze { .. } => "bulldoze",
        ExecOp::Zone { .. } => "set_zoning",
        ExecOp::Invalid => "invalid",
    }
}
```

Add to `broker/src/benchmark/mod.rs`:
```rust
pub mod plan;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path broker/Cargo.toml benchmark::plan`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
git add broker/src/benchmark/plan.rs broker/src/benchmark/mod.rs
git commit -m "feat(broker): pure batch-plan expansion and validation"
```

---

### Task 2: apply_plan tool on the benchmark server

**Files:**
- Modify: `broker/src/benchmark/server.rs` (args struct, tool, tests; tool-count list)
- Modify: `benchmark/run.sh` (`ALLOWED` list)
- Modify: `benchmark/prompt.md` (mention the tool)

- [ ] **Step 1: Write the failing server tests**

Add to `broker/src/benchmark/server.rs` tests:

```rust
    fn plan_build(x0: f32, x1: f32) -> crate::benchmark::plan::PlanOp {
        crate::benchmark::plan::PlanOp::BuildRoad {
            from: crate::contract::Position { x: x0, y: 0.0, z: 0.0 },
            to: crate::contract::Position { x: x1, y: 0.0, z: 0.0 },
            road_type: "road".into(),
            snap: true,
        }
    }

    /// Like bench_with_mock, but with the mock's road-cost table seeded so
    /// estimated costs are non-zero (bench_with_mock passes an empty map).
    async fn bench_with_mock_costs() -> BenchmarkServer {
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
        let mut st = RunState::new(
            BenchConfig::default(),
            HashMap::from([("road".to_string(), 1000i64)]),
        );
        st.baseline = Some(crate::benchmark::record::WindowStats {
            flow_mean: 50.0,
            active_vehicles_mean: 10.0,
            population: 100,
        });
        BenchmarkServer::new(client, Arc::new(Mutex::new(st)))
    }

    #[tokio::test]
    async fn apply_plan_validate_only_prices_without_mutating() {
        let bench = bench_with_mock_costs().await;
        let res = bench
            .apply_plan(Parameters(ApplyPlanArgs {
                ops: vec![plan_build(0.0, 50.0), plan_build(1000.0, 1400.0)],
                validate_only: true,
                stop_on_error: true,
            }))
            .await
            .unwrap();
        let text = result_text(&res);
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["validate_only"], true);
        // 50m span = 1 op; 400m span = 3 chunks → 4 expanded ops priced.
        assert_eq!(v["results"].as_array().unwrap().len(), 4);
        assert!(v["total_estimated_cost"].as_i64().unwrap() > 0);
        assert_eq!(v["benchmark_progress"]["num_changes"], 0, "dry-run must not record changes");
    }

    #[tokio::test]
    async fn apply_plan_executes_and_records_each_op() {
        let bench = bench_with_mock_costs().await;
        let res = bench
            .apply_plan(Parameters(ApplyPlanArgs {
                ops: vec![plan_build(0.0, 50.0), plan_build(1000.0, 1400.0)],
                validate_only: false,
                stop_on_error: true,
            }))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result_text(&res)).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["benchmark_progress"]["num_changes"], 4);
        assert!(v["results"].as_array().unwrap().iter().all(|r| r["executed"] == true && r["ok"] == true));
    }

    #[tokio::test]
    async fn apply_plan_rejects_whole_plan_on_invalid_op() {
        let bench = bench_with_mock().await;
        let res = bench
            .apply_plan(Parameters(ApplyPlanArgs {
                ops: vec![
                    plan_build(0.0, 50.0),
                    crate::benchmark::plan::PlanOp::UpgradeRoad { segment: 9999, road_type: "road".into() },
                ],
                validate_only: false,
                stop_on_error: true,
            }))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result_text(&res)).unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["benchmark_progress"]["num_changes"], 0, "nothing may execute when validation fails");
        let results = v["results"].as_array().unwrap();
        assert_eq!(results[1]["valid"], false);
        assert_eq!(results[1]["reason"], "INVALID_ARGS");
    }

    #[tokio::test]
    async fn apply_plan_stops_at_runtime_failure() {
        let bench = bench_with_mock().await;
        // Build one road, find its segment id, then: bulldoze it twice. The
        // second bulldoze passes pre-validation (snapshot taken once) but
        // fails at runtime — execution must stop there.
        let built = bench
            .build_road(Parameters(crate::service::BuildRoadArgs {
                from: crate::contract::Position { x: 0.0, y: 0.0, z: 0.0 },
                to: crate::contract::Position { x: 50.0, y: 0.0, z: 0.0 },
                road_type: "road".into(),
                snap: true,
            }))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result_text(&built)).unwrap();
        let seg = v["created_segments"][0].as_u64().unwrap() as u32;

        let res = bench
            .apply_plan(Parameters(ApplyPlanArgs {
                ops: vec![
                    crate::benchmark::plan::PlanOp::Bulldoze { target_type: "segment".into(), id: seg },
                    crate::benchmark::plan::PlanOp::Bulldoze { target_type: "segment".into(), id: seg },
                    plan_build(2000.0, 2050.0),
                ],
                validate_only: false,
                stop_on_error: true,
            }))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result_text(&res)).unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["stopped_at"], 1);
        let results = v["results"].as_array().unwrap();
        assert_eq!(results[0]["ok"], true);
        assert_eq!(results[1]["ok"], false);
        assert_eq!(results[2]["executed"], false, "ops after the failure are skipped");
        // 1 change from the setup build + 1 from the successful bulldoze.
        assert_eq!(v["benchmark_progress"]["num_changes"], 2);
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --manifest-path broker/Cargo.toml apply_plan`
Expected: FAIL — `ApplyPlanArgs` / `apply_plan` missing.

- [ ] **Step 3: Implement the tool**

In `broker/src/benchmark/server.rs` add the args struct next to `SubmitArgs`:

```rust
#[derive(Deserialize, schemars::JsonSchema)]
pub struct ApplyPlanArgs {
    /// Ops applied in order. Polylines and long build_road spans are
    /// auto-split into segments under the 200 m cap.
    pub ops: Vec<crate::benchmark::plan::PlanOp>,
    /// true: validate and price every op, mutate nothing, record no changes.
    #[serde(default)]
    pub validate_only: bool,
    /// true (default): a runtime failure stops the remaining ops.
    #[serde(default = "default_stop_on_error")]
    pub stop_on_error: bool,
}

fn default_stop_on_error() -> bool {
    true
}
```

Add the tool inside the `#[tool_router] impl BenchmarkServer` block:

```rust
    #[tool(description = "Apply a batch of modifications in one call: build_road / build_polyline \
        (multi-point, auto-split under the 200 m segment cap) / upgrade_road / bulldoze / set_zoning. \
        Every op is validated and priced up front — any structurally invalid op rejects the WHOLE plan \
        before anything executes. Set validate_only=true for a free dry-run (no changes recorded). \
        Each executed op counts as one change, identical to the single-op tools. The game can still \
        reject an op at execution time (e.g. COLLISION); stop_on_error (default true) then skips the rest.")]
    async fn apply_plan(&self, Parameters(args): Parameters<ApplyPlanArgs>) -> Result<CallToolResult, ErrorData> {
        use crate::benchmark::plan::{expand, tool_name, validate, ExecCtx, ExecOp, MAX_EXPANDED_OPS, MAX_OPS};

        self.ensure_baseline().await;
        if self.run_ended().await {
            return self.finish(serde_json::json!({ "ok": false, "run_ended": true })).await;
        }
        if args.ops.is_empty() || args.ops.len() > MAX_OPS {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "plan must contain 1..={MAX_OPS} ops (got {})", args.ops.len()
            ))]));
        }

        let (road_types, zone_types, net, buildings) = match tokio::try_join!(
            self.client.road_types(),
            self.client.zone_types(),
            self.client.network(),
            self.client.buildings(),
        ) {
            Ok((r, z, n, b)) => (r.road_types, z.zone_types, n, b.buildings),
            Err(e) => return Ok(tool_err(ServiceError::Bridge(e))),
        };
        let ctx = ExecCtx {
            road_types,
            zone_types,
            segment_ids: net.segments.iter().map(|s| s.id).collect(),
            node_ids: net.nodes.iter().map(|n| n.id).collect(),
            building_ids: buildings.iter().map(|b| b.id).collect(),
            segment_lengths: net.segments.iter().map(|s| (s.id, s.length)).collect(),
        };

        let exec = expand(&args.ops);
        if exec.len() > MAX_EXPANDED_OPS {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "plan expands to {} segments; the cap is {MAX_EXPANDED_OPS} — split it into smaller plans",
                exec.len()
            ))]));
        }

        let estimate = |op: &ExecOp, state: &crate::benchmark::state::RunState| -> i64 {
            match op {
                ExecOp::Build { from, to, road_type, .. } => {
                    state.build_cost(road_type, horizontal_distance(*from, *to))
                }
                ExecOp::Upgrade { segment, road_type } => {
                    state.build_cost(road_type, ctx.segment_lengths.get(segment).copied().unwrap_or(0.0))
                }
                _ => 0,
            }
        };

        let validations: Vec<(usize, &ExecOp, Result<(), crate::contract::ActionError>, i64)> = {
            let state = self.state.lock().await;
            exec.iter()
                .map(|(source, op)| (*source, op, validate(op, &ctx), estimate(op, &state)))
                .collect()
        };
        let all_valid = validations.iter().all(|(_, _, v, _)| v.is_ok());
        let total_estimated_cost: i64 = validations.iter().map(|(_, _, _, c)| c).sum();

        if args.validate_only || !all_valid {
            let results: Vec<Value> = validations
                .iter()
                .enumerate()
                .map(|(i, (source, op, v, cost))| {
                    serde_json::json!({
                        "op_index": i,
                        "source_op": source,
                        "tool": tool_name(op),
                        "valid": v.is_ok(),
                        "reason": v.err(),
                        "estimated_cost": cost,
                        "executed": false,
                    })
                })
                .collect();
            return self
                .finish(serde_json::json!({
                    "ok": all_valid,
                    "validate_only": args.validate_only,
                    "results": results,
                    "total_estimated_cost": total_estimated_cost,
                    "stopped_at": Value::Null,
                }))
                .await;
        }

        let mut results: Vec<Value> = Vec::with_capacity(validations.len());
        let mut stopped_at: Option<usize> = None;
        for (i, (source, op, _, cost)) in validations.iter().enumerate() {
            if stopped_at.is_some() && args.stop_on_error {
                results.push(serde_json::json!({
                    "op_index": i, "source_op": source, "tool": tool_name(op),
                    "valid": true, "estimated_cost": cost, "executed": false,
                }));
                continue;
            }
            let outcome = match (*op).clone() {
                ExecOp::Build { from, to, road_type, snap } => {
                    service::build_road(&self.client, BuildRoadArgs { from, to, road_type, snap }).await
                }
                ExecOp::Upgrade { segment, road_type } => {
                    service::upgrade_road(&self.client, UpgradeRoadArgs { segment, road_type }).await
                }
                ExecOp::Bulldoze { target_type, id } => {
                    service::bulldoze(&self.client, BulldozeArgs { target_type, id }).await
                }
                ExecOp::Zone { area, zone_type } => {
                    service::set_zoning(&self.client, SetZoningArgs { area, zone_type }).await
                }
                ExecOp::Invalid => unreachable!("invalid ops never pass the all_valid gate"),
            };
            match outcome {
                Ok(v) => {
                    let ok = v.get("ok").and_then(|b| b.as_bool()) == Some(true);
                    if ok {
                        self.state.lock().await.record_mutation(tool_name(op), *cost);
                    } else if stopped_at.is_none() {
                        stopped_at = Some(i);
                    }
                    results.push(serde_json::json!({
                        "op_index": i, "source_op": source, "tool": tool_name(op),
                        "valid": true, "estimated_cost": cost, "executed": true,
                        "ok": ok, "action": v,
                    }));
                }
                Err(e) => return Ok(tool_err(e)),
            }
        }

        self.finish(serde_json::json!({
            "ok": stopped_at.is_none(),
            "validate_only": false,
            "results": results,
            "total_estimated_cost": total_estimated_cost,
            "stopped_at": stopped_at,
        }))
        .await
    }
```

Add the imports the block needs at the top of the file: `use crate::geometry::horizontal_distance;` is already imported; add `use crate::service::SetZoningArgs;` to the existing `crate::service::{...}` list if missing (it is already there), and `Value` is already in scope via `serde_json::Value`.

Update the tool-count test: insert `"apply_plan"` at the front of the sorted expected list (it sorts before `build_road`).

- [ ] **Step 4: Run the suite**

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: ALL PASS.

- [ ] **Step 5: Allow the tool in run.sh and mention it in the prompt**

`benchmark/run.sh` — add `,mcp__skylinebench__apply_plan` to `ALLOWED`.

`benchmark/prompt.md` — extend the Modify bullet:

```markdown
- Modify in batch: `apply_plan` stages several ops (including multi-point polylines that
  auto-split under the 200 m segment cap) in one call, validates and prices ALL of them
  before anything executes, and supports `validate_only: true` as a free dry-run. Prefer
  one validated plan per rebuild over loose single calls; each executed op still counts
  as one change.
```

- [ ] **Step 6: Commit**

```bash
git add broker/src/benchmark/server.rs benchmark/run.sh benchmark/prompt.md
git commit -m "feat(benchmark): apply_plan batch tool with dry-run validation"
```
