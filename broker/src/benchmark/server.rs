//! Instrumented benchmark MCP server (spec §7).
//!
//! Parallels [`crate::tools::Skyline`] but delegates to the same `service::*`
//! functions while attaching a `benchmark_progress` telemetry block to every
//! response, counting mutating actions via [`RunState`], adding
//! `submit_solution`, and omitting `reset_scenario`.

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

use crate::benchmark::measure::measure_window;
use crate::benchmark::persist::EndStatePersister;
use crate::benchmark::record::EndReason;
use crate::benchmark::state::RunState;
use crate::bridge_client::BridgeClient;
use crate::geometry::horizontal_distance;
use crate::service::{
    self, BuildRoadArgs, BulldozeArgs, ControlTimeArgs, GetMetricsArgs, ObserveAreaArgs,
    QuerySegmentsArgs, RenderMapArgs, ServiceError, SetZoningArgs, UpgradeRoadArgs,
};

#[derive(Clone)]
pub struct BenchmarkServer {
    client: Arc<BridgeClient>,
    state: Arc<Mutex<RunState>>,
    persist: Option<Arc<EndStatePersister>>,
    renders_dir: Option<std::path::PathBuf>,
    tool_router: ToolRouter<Self>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct SubmitArgs {
    /// Optional free-text rationale from the agent. Accepted so the agent can
    /// explain its solution; not used in scoring.
    #[serde(default)]
    pub note: Option<String>,
}

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

/// Merge the agent-facing telemetry block into a JSON object result (spec §7).
pub fn with_progress(mut value: Value, state: &RunState) -> Value {
    if let Value::Object(ref mut map) = value {
        map.insert("benchmark_progress".into(), state.progress());
    }
    value
}

/// Split a step of `total` ticks into chunks of at most `chunk` ticks, so each
/// bridge call stays short and the whole tool call can bail out before the MCP
/// client timeout instead of being killed by it.
pub fn step_chunks(total: u32, chunk: u32) -> Vec<u32> {
    let chunk = chunk.max(1);
    let full = (0..total / chunk).map(move |_| chunk);
    let rem = total % chunk;
    full.chain((rem > 0).then_some(rem)).collect()
}

fn tool_err(err: ServiceError) -> CallToolResult {
    CallToolResult::error(vec![Content::text(err.to_string())])
}

impl BenchmarkServer {
    pub fn new(client: Arc<BridgeClient>, state: Arc<Mutex<RunState>>) -> Self {
        Self { client, state, persist: None, renders_dir: None, tool_router: Self::tool_router() }
    }

    pub fn with_persist(self, persist: Arc<EndStatePersister>) -> Self {
        Self { persist: Some(persist), ..self }
    }

    pub fn with_renders_dir(self, dir: std::path::PathBuf) -> Self {
        Self { renders_dir: Some(dir), ..self }
    }

    /// Best-effort frame write: a failed render persist must never fail the
    /// tool call (same policy as end-state persistence).
    async fn persist_render(&self, png: &[u8], tick: u64, trigger: &str) {
        let Some(dir) = &self.renders_dir else { return };
        let (seq, changes, flow) = {
            let mut s = self.state.lock().await;
            (s.next_render_seq(), s.num_changes, s.flow.mean())
        };
        let _ = std::fs::create_dir_all(dir);
        let name = format!("{seq:05}-tick{tick}.png");
        if let Err(e) = std::fs::write(dir.join(&name), png) {
            eprintln!("benchmark: render persist error: {e}");
            return;
        }
        let line = serde_json::json!({
            "seq": seq, "file": name, "tick": tick, "trigger": trigger,
            "changes": changes, "flow": flow,
        });
        let appended = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("index.jsonl"))
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "{line}")
            });
        if let Err(e) = appended {
            eprintln!("benchmark: render index error: {e}");
        }
    }

    async fn finish(&self, value: Value) -> Result<CallToolResult, ErrorData> {
        let mut s = self.state.lock().await;
        s.check_timeout();
        if let Some(p) = &self.persist {
            if let Err(e) = p.write(&s) {
                eprintln!("benchmark: end-state persist error: {e}");
            }
        }
        let merged = with_progress(value, &s);
        Ok(CallToolResult::success(vec![Content::text(merged.to_string())]))
    }

    async fn run_ended(&self) -> bool {
        self.state.lock().await.end_reason.is_some()
    }

    /// Measure the baseline window on the first tool call (the city is still
    /// untouched then). Deferred out of startup so the MCP `initialize`
    /// handshake — which has its own ~60s request timeout — isn't blocked by
    /// the slow window. No-op once measured.
    async fn ensure_baseline(&self) {
        let cfg = {
            let s = self.state.lock().await;
            if s.baseline.is_some() {
                return;
            }
            s.config.clone()
        };
        if let Ok(m) = measure_window(&self.client, &cfg).await {
            let mut s = self.state.lock().await;
            if s.baseline.is_none() {
                s.baseline = Some(m.stats);
                s.baseline_flow_samples = m.samples;
            }
        }
    }
}

#[tool_router]
impl BenchmarkServer {
    #[tool(description = "Summarise the city: tick, population, funds, traffic flow, network size.")]
    async fn get_city_overview(&self) -> Result<CallToolResult, ErrorData> {
        self.ensure_baseline().await;
        match service::get_city_overview(&self.client).await {
            Ok(v) => self.finish(v).await,
            Err(e) => Ok(tool_err(e)),
        }
    }

    #[tool(description = "Observe the playable area: road network, buildings, zones, intersections, dead ends. \
        Optional `bounds` restricts to a rectangle.")]
    async fn observe_area(&self, Parameters(args): Parameters<ObserveAreaArgs>) -> Result<CallToolResult, ErrorData> {
        self.ensure_baseline().await;
        match service::observe_area(&self.client, args).await {
            Ok(v) => self.finish(v).await,
            Err(e) => Ok(tool_err(e)),
        }
    }

    #[tool(description = "Query road segments sorted by congestion (default) — the 'worst N segments' \
        search. Optional filters: min_density, bounds, prefab_contains; sort_by length or \
        speed_limit instead. Returns density, direction, lanes, and midpoint per segment.")]
    async fn query_segments(&self, Parameters(args): Parameters<QuerySegmentsArgs>) -> Result<CallToolResult, ErrorData> {
        self.ensure_baseline().await;
        match service::query_segments(&self.client, args).await {
            Ok(v) => self.finish(v).await,
            Err(e) => Ok(tool_err(e)),
        }
    }

    #[tool(description = "Get city metrics, optionally filtered to groups: traffic, economy, population, services.")]
    async fn get_metrics(&self, Parameters(args): Parameters<GetMetricsArgs>) -> Result<CallToolResult, ErrorData> {
        self.ensure_baseline().await;
        match service::get_metrics(&self.client, args).await {
            Ok(v) => {
                if let Some(flow) = v.get("traffic").and_then(|t| t.get("flow_percent")).and_then(|f| f.as_f64()) {
                    self.state.lock().await.push_flow(flow);
                }
                self.finish(v).await
            }
            Err(e) => Ok(tool_err(e)),
        }
    }

    #[tool(description = "List the available road types (with construction cost).")]
    async fn list_road_types(&self) -> Result<CallToolResult, ErrorData> {
        self.ensure_baseline().await;
        match service::list_road_types(&self.client).await {
            Ok(v) => self.finish(v).await,
            Err(e) => Ok(tool_err(e)),
        }
    }

    #[tool(description = "List the available zone types.")]
    async fn list_zone_types(&self) -> Result<CallToolResult, ErrorData> {
        self.ensure_baseline().await;
        match service::list_zone_types(&self.client).await {
            Ok(v) => self.finish(v).await,
            Err(e) => Ok(tool_err(e)),
        }
    }

    #[tool(description = "Render the road network to a PNG image: congestion colours, lane widths, \
        one-way arrows, coordinate grid. Returns the image plus a JSON legend.")]
    async fn render_map(&self, Parameters(args): Parameters<RenderMapArgs>) -> Result<CallToolResult, ErrorData> {
        self.ensure_baseline().await;
        match service::render_map(&self.client, args).await {
            Ok((png, legend)) => {
                if self.renders_dir.is_some() {
                    let tick = self.client.health().await.map(|h| h.tick).unwrap_or(0);
                    self.persist_render(&png, tick, "render_map").await;
                }
                let data = base64::engine::general_purpose::STANDARD.encode(&png);
                let progress = {
                    let mut s = self.state.lock().await;
                    s.check_timeout();
                    if let Some(p) = &self.persist {
                        if let Err(e) = p.write(&s) {
                            eprintln!("benchmark: end-state persist error: {e}");
                        }
                    }
                    s.progress()
                };
                let mut text = legend;
                if let Value::Object(ref mut map) = text {
                    map.insert("benchmark_progress".into(), progress);
                }
                Ok(CallToolResult::success(vec![
                    Content::image(data, "image/png".to_string()),
                    Content::text(text.to_string()),
                ]))
            }
            Err(e) => Ok(tool_err(e)),
        }
    }

    #[tool(description = "Control simulation time: pause, resume, step, or set speed. \
        `step` defaults to 1 in-game day (585 ticks) when `ticks` is omitted; \
        the maximum step is 3 days (1755 ticks). Long steps are driven in chunks; \
        if the response has `partial: true`, call step again for the remainder of the ticks.")]
    async fn control_time(&self, Parameters(args): Parameters<ControlTimeArgs>) -> Result<CallToolResult, ErrorData> {
        self.ensure_baseline().await;
        if self.run_ended().await {
            return self.finish(serde_json::json!({ "ok": false, "run_ended": true })).await;
        }
        let (day_ticks, max_ticks, max_step_days) = {
            let s = self.state.lock().await;
            (s.config.day_ticks, s.config.max_step_ticks(), s.config.max_step_days)
        };
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
        // 600 s MCP client timeout minus ~150 s headroom for one in-flight chunk.
        // Budget is only checked *after* a chunk completes, so worst case is
        // 450 s + the duration of one additional chunk.
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
                Err(e) => {
                    return Ok(CallToolResult::error(vec![Content::text(format!(
                        "{e} (step had already advanced {advanced} of {requested} ticks; \
                         check the clock before retrying)"
                    ))]))
                }
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
        if self.renders_dir.is_some() {
            let frame = service::render_map(
                &self.client,
                crate::service::RenderMapArgs {
                    bounds: None,
                    width_px: 1024,
                    height_px: 1024,
                    grid_spacing_m: None,
                },
            )
            .await;
            if let Ok((png, _)) = frame {
                let tick = out.get("tick").and_then(|t| t.as_u64()).unwrap_or(0);
                self.persist_render(&png, tick, "step").await;
            }
        }
        self.finish(out).await
    }

    #[tool(description = "Build a road between two positions of a given road type.")]
    async fn build_road(&self, Parameters(args): Parameters<BuildRoadArgs>) -> Result<CallToolResult, ErrorData> {
        self.ensure_baseline().await;
        if self.run_ended().await {
            return self.finish(serde_json::json!({ "ok": false, "run_ended": true })).await;
        }
        let length = horizontal_distance(args.from, args.to);
        let road_type = args.road_type.clone();
        match service::build_road(&self.client, args).await {
            Ok(v) => {
                if v.get("ok").and_then(|b| b.as_bool()) == Some(true) {
                    let mut s = self.state.lock().await;
                    let cost = s.build_cost(&road_type, length);
                    s.record_mutation("build_road", cost);
                }
                self.finish(v).await
            }
            Err(e) => Ok(tool_err(e)),
        }
    }

    #[tool(description = "Change an existing road segment's type. The segment is re-created \
        under a NEW id — `replaced` in the response maps old_segment_id to new_segment_id; \
        refresh any cached ids.")]
    async fn upgrade_road(&self, Parameters(args): Parameters<UpgradeRoadArgs>) -> Result<CallToolResult, ErrorData> {
        self.ensure_baseline().await;
        if self.run_ended().await {
            return self.finish(serde_json::json!({ "ok": false, "run_ended": true })).await;
        }
        let segment_id = args.segment;
        let road_type = args.road_type.clone();
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
                    let mut s = self.state.lock().await;
                    let cost = s.build_cost(&road_type, length);
                    s.record_mutation("upgrade_road", cost);
                }
                self.finish(v).await
            }
            Err(e) => Ok(tool_err(e)),
        }
    }

    #[tool(description = "Remove a network segment, node, or building. target_type = segment | node | building.")]
    async fn bulldoze(&self, Parameters(args): Parameters<BulldozeArgs>) -> Result<CallToolResult, ErrorData> {
        self.ensure_baseline().await;
        if self.run_ended().await {
            return self.finish(serde_json::json!({ "ok": false, "run_ended": true })).await;
        }
        match service::bulldoze(&self.client, args).await {
            Ok(v) => {
                if v.get("ok").and_then(|b| b.as_bool()) == Some(true) {
                    self.state.lock().await.record_mutation("bulldoze", 0);
                }
                self.finish(v).await
            }
            Err(e) => Ok(tool_err(e)),
        }
    }

    #[tool(description = "Set zoning over a rectangular area. zone_type from list_zone_types.")]
    async fn set_zoning(&self, Parameters(args): Parameters<SetZoningArgs>) -> Result<CallToolResult, ErrorData> {
        self.ensure_baseline().await;
        if self.run_ended().await {
            return self.finish(serde_json::json!({ "ok": false, "run_ended": true })).await;
        }
        match service::set_zoning(&self.client, args).await {
            Ok(v) => {
                if v.get("ok").and_then(|b| b.as_bool()) == Some(true) {
                    self.state.lock().await.record_mutation("set_zoning", 0);
                }
                self.finish(v).await
            }
            Err(e) => Ok(tool_err(e)),
        }
    }

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
                        "reason": v.as_ref().err(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_tools_including_submit_excluding_reset() {
        let tools = BenchmarkServer::tool_router().list_all();
        let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            vec![
                "apply_plan",
                "build_road",
                "bulldoze",
                "control_time",
                "get_city_overview",
                "get_metrics",
                "list_road_types",
                "list_zone_types",
                "observe_area",
                "query_segments",
                "render_map",
                "set_zoning",
                "submit_solution",
                "upgrade_road",
            ]
        );
    }

    #[test]
    fn attaches_progress_to_json_value() {
        use crate::benchmark::config::BenchConfig;
        use crate::benchmark::state::RunState;
        use std::collections::HashMap;

        let state = RunState::new(BenchConfig::default(), HashMap::new());
        let merged = with_progress(serde_json::json!({"ok": true}), &state);
        assert_eq!(merged["ok"], true);
        assert!(merged["benchmark_progress"]["flow_target"].is_number());
    }

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

        // Rejected step must not have advanced the mock clock.
        let pause_res = bench
            .control_time(Parameters(crate::service::ControlTimeArgs {
                op: "pause".into(),
                ticks: None,
                speed: None,
            }))
            .await
            .unwrap();
        let pause_text = result_text(&pause_res);
        assert!(pause_text.contains("\"tick\":0"), "clock should still be at 0, got: {pause_text}");
    }

    #[tokio::test]
    async fn step_of_exactly_the_cap_is_allowed() {
        let bench = bench_with_mock().await;
        let res = bench
            .control_time(Parameters(crate::service::ControlTimeArgs {
                op: "step".into(),
                ticks: Some(1755),
                speed: None,
            }))
            .await
            .unwrap();
        assert_ne!(res.is_error, Some(true), "exact-cap step should succeed");
        let text = result_text(&res);
        assert!(text.contains("\"tick\":1755"), "clock should be at 1755, got: {text}");
    }

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

    #[tokio::test]
    async fn submit_persists_end_state_before_responding() {
        let dir = std::env::temp_dir().join(format!("sb-persist-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let persister = std::sync::Arc::new(crate::benchmark::persist::EndStatePersister {
            out_dir: dir.clone(),
            map: crate::benchmark::record::MapInfo { id: "m".into(), source: "test".into(), game_version: "v".into() },
            started_at: "t0".into(),
        });
        let bench = bench_with_mock().await.with_persist(persister);
        let res = bench.submit_solution(Parameters(SubmitArgs { note: None })).await.unwrap();
        assert!(result_text(&res).contains("\"run_ended\":true"));
        // The snapshot must already be on disk when the response is built.
        let end: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("end-state.json")).unwrap()).unwrap();
        assert_eq!(end["end_reason"], "submit");
        std::fs::remove_dir_all(&dir).ok();
    }

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

    #[tokio::test]
    async fn zero_tick_step_returns_clock_state_without_advancing() {
        let bench = bench_with_mock().await;
        let res = bench
            .control_time(Parameters(crate::service::ControlTimeArgs {
                op: "step".into(),
                ticks: Some(0),
                speed: None,
            }))
            .await
            .unwrap();
        assert_ne!(res.is_error, Some(true), "zero-tick step should not be an error");
        let text = result_text(&res);
        assert!(text.contains("\"tick\":0"), "clock should remain at 0, got: {text}");
        assert!(text.contains("\"ticks_advanced\":0"), "got: {text}");
        assert!(text.contains("\"partial\":false"), "got: {text}");
    }

    #[test]
    fn step_chunks_splits_into_days() {
        assert_eq!(step_chunks(1755, 585), vec![585, 585, 585]);
        assert_eq!(step_chunks(600, 585), vec![585, 15]);
        assert_eq!(step_chunks(585, 585), vec![585]);
        assert_eq!(step_chunks(10, 585), vec![10]);
        assert_eq!(step_chunks(0, 585), Vec::<u32>::new());
    }

    #[tokio::test]
    async fn renders_are_persisted_with_index() {
        let dir = std::env::temp_dir().join(format!("sb-renders-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let bench = bench_with_mock().await.with_renders_dir(dir.clone());

        bench
            .render_map(Parameters(crate::service::RenderMapArgs {
                bounds: None,
                width_px: 32,
                height_px: 32,
                grid_spacing_m: None,
            }))
            .await
            .unwrap();
        bench
            .control_time(Parameters(crate::service::ControlTimeArgs {
                op: "step".into(),
                ticks: Some(10),
                speed: None,
            }))
            .await
            .unwrap();

        let mut frames: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
            .filter(|n| n.ends_with(".png"))
            .collect();
        frames.sort();
        assert_eq!(frames.len(), 2, "one agent render + one auto step frame: {frames:?}");
        assert!(frames[0].starts_with("00001"), "{frames:?}");

        let index = std::fs::read_to_string(dir.join("index.jsonl")).unwrap();
        let lines: Vec<serde_json::Value> =
            index.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["trigger"], "render_map");
        assert_eq!(lines[1]["trigger"], "step");
        assert!(lines[1]["tick"].is_u64());
        std::fs::remove_dir_all(&dir).ok();
    }

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

    #[tokio::test]
    async fn mutations_rejected_after_run_ended() {
        use crate::benchmark::config::BenchConfig;
        use crate::benchmark::record::EndReason;
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
        st.end_reason = Some(EndReason::Submit);
        let state = Arc::new(Mutex::new(st));
        let bench = BenchmarkServer::new(client, state.clone());

        let res = bench
            .bulldoze(Parameters(
                crate::service::BulldozeArgs { target_type: "segment".into(), id: 0 },
            ))
            .await
            .unwrap();
        // run_ended path returns ok:false, run_ended:true and records NO change.
        assert_eq!(state.lock().await.num_changes, 0);
        let _ = res; // result content is a CallToolResult; the key assertion is no mutation recorded
    }
}
