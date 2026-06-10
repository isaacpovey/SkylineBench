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
    RenderMapArgs, ServiceError, SetZoningArgs, UpgradeRoadArgs,
};

#[derive(Clone)]
pub struct BenchmarkServer {
    client: Arc<BridgeClient>,
    state: Arc<Mutex<RunState>>,
    persist: Option<Arc<EndStatePersister>>,
    tool_router: ToolRouter<Self>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct SubmitArgs {
    /// Optional free-text rationale from the agent. Accepted so the agent can
    /// explain its solution; not used in scoring.
    #[serde(default)]
    pub note: Option<String>,
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
        Self { client, state, persist: None, tool_router: Self::tool_router() }
    }

    pub fn with_persist(self, persist: Arc<EndStatePersister>) -> Self {
        Self { persist: Some(persist), ..self }
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

    #[tool(description = "Render the road network to a PNG image.")]
    async fn render_map(&self, Parameters(args): Parameters<RenderMapArgs>) -> Result<CallToolResult, ErrorData> {
        self.ensure_baseline().await;
        match service::render_map(&self.client, args).await {
            Ok(png) => {
                let data = base64::engine::general_purpose::STANDARD.encode(png);
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
                Ok(CallToolResult::success(vec![
                    Content::image(data, "image/png".to_string()),
                    Content::text(serde_json::json!({ "benchmark_progress": progress }).to_string()),
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

    #[tool(description = "Change an existing road segment's type. Validates the new road_type first.")]
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
    fn registers_twelve_tools_including_submit_excluding_reset() {
        let tools = BenchmarkServer::tool_router().list_all();
        let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            vec![
                "build_road",
                "bulldoze",
                "control_time",
                "get_city_overview",
                "get_metrics",
                "list_road_types",
                "list_zone_types",
                "observe_area",
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
