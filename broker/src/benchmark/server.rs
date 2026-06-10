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
use crate::benchmark::record::EndReason;
use crate::benchmark::state::RunState;
use crate::bridge_client::BridgeClient;
use crate::geometry::horizontal_distance;
use crate::service::{
    self, BuildRoadArgs, BulldozeArgs, ControlTimeArgs, GetMetricsArgs, RenderMapArgs,
    ServiceError, SetZoningArgs, UpgradeRoadArgs,
};

#[derive(Clone)]
pub struct BenchmarkServer {
    client: Arc<BridgeClient>,
    state: Arc<Mutex<RunState>>,
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

fn tool_err(err: ServiceError) -> CallToolResult {
    CallToolResult::error(vec![Content::text(err.to_string())])
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

    #[tool(description = "Observe the playable area: network, buildings, zones, intersections, dead ends.")]
    async fn observe_area(&self) -> Result<CallToolResult, ErrorData> {
        self.ensure_baseline().await;
        match service::observe_area(&self.client).await {
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

    #[tool(description = "Control simulation time: pause, resume, step, or set speed.")]
    async fn control_time(&self, Parameters(args): Parameters<ControlTimeArgs>) -> Result<CallToolResult, ErrorData> {
        self.ensure_baseline().await;
        if self.run_ended().await {
            return self.finish(serde_json::json!({ "ok": false, "run_ended": true })).await;
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

    #[tool(description = "Declare the run finished. The harness then scores the city. Call when satisfied.")]
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
        // Hold the connection open. The background watcher runs the settle +
        // final measurement and then exits the process, which ends the agent
        // session. Returning here would let `claude -p` finish its turn and tear
        // down this server before finalize completes, losing the artifacts.
        std::future::pending::<()>().await;
        unreachable!("the watcher exits the process during finalize")
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
