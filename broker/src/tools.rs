//! rmcp adapter exposing the `service` layer as MCP tools.
//!
//! Each tool is a thin wrapper that delegates to the matching `service::*`
//! function and converts the result into MCP content. JSON results are returned
//! as text content; `render_map` returns the rendered PNG as an image content
//! block. Any `ServiceError` is surfaced as an MCP tool error rather than a
//! protocol error or panic.

use std::sync::Arc;

use base64::Engine;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, ErrorData, ServerHandler,
};
use serde_json::Value;

use crate::bridge_client::BridgeClient;
use crate::service::{
    self, BuildRoadArgs, BulldozeArgs, ControlTimeArgs, GetMetricsArgs, ObserveAreaArgs,
    QueryProblemsArgs, QuerySegmentsArgs, RenderMapArgs, ResetScenarioArgs, ServiceError,
    SetZoningArgs, TraceRouteArgs, UpgradeRoadArgs, ViewArgs,
};

#[derive(Clone)]
pub struct Skyline {
    client: Arc<BridgeClient>,
    tool_router: ToolRouter<Self>,
}

impl Skyline {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            client: Arc::new(BridgeClient::new(base_url)),
            tool_router: Self::tool_router(),
        }
    }
}

fn tool_error(err: ServiceError) -> CallToolResult {
    CallToolResult::error(vec![Content::text(err.to_string())])
}

fn json_result(value: Value) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::success(vec![Content::text(
        value.to_string(),
    )]))
}

#[tool_router]
impl Skyline {
    #[tool(
        description = "Summarise the city: tick, population, funds, traffic flow, network size, \
            and a list of any abandoned buildings (id + position) you may want to clear."
    )]
    async fn get_city_overview(&self) -> Result<CallToolResult, ErrorData> {
        match service::get_city_overview(&self.client).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(
        description = "Observe the playable area: road network, buildings, zones, intersections, dead ends. \
            Optional `bounds` restricts to a rectangle."
    )]
    async fn observe_area(
        &self,
        Parameters(args): Parameters<ObserveAreaArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match service::observe_area(&self.client, args).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(
        description = "Locate the specific buildings behind a problem-count spike (the leading \
            death-spiral signal in get_metrics `services`): which buildings lost road access or a \
            utility, and where (id + position + problem list). Optional `filter` (a single problem \
            name like \"road_not_connected\") and `bounds`."
    )]
    async fn query_problems(
        &self,
        Parameters(args): Parameters<QueryProblemsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match service::query_problems(&self.client, args).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(
        description = "Query road segments sorted by congestion (default) — the 'worst N segments' \
            search. Optional filters: min_density, bounds, prefab_contains; sort_by length or \
            speed_limit instead. Returns density, direction, lanes, and midpoint per segment."
    )]
    async fn query_segments(
        &self,
        Parameters(args): Parameters<QuerySegmentsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match service::query_segments(&self.client, args).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(
        description = "Render the road network to a PNG image: congestion colours, lane widths, \
        one-way arrows, coordinate grid. Returns the image plus a JSON legend."
    )]
    async fn render_map(
        &self,
        Parameters(args): Parameters<RenderMapArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match service::render_map(&self.client, args).await {
            Ok((png, legend)) => {
                let data = base64::engine::general_purpose::STANDARD.encode(png);
                Ok(CallToolResult::success(vec![
                    Content::image(data, "image/png".to_string()),
                    Content::text(legend.to_string()),
                ]))
            }
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(
        description = "Get city metrics, optionally filtered to groups: traffic, economy, population, services."
    )]
    async fn get_metrics(
        &self,
        Parameters(args): Parameters<GetMetricsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match service::get_metrics(&self.client, args).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(description = "Build a road between two positions of a given road type.")]
    async fn build_road(
        &self,
        Parameters(args): Parameters<BuildRoadArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match service::build_road(&self.client, args).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(description = "Dry-run a road build: test placement (collisions, slope, water, height, bounds) \
        WITHOUT committing or creating any segment. Same args as build_road. Use it to check a placement \
        before build_road commits it. Note: connectivity warnings (isolated island) are surfaced only by \
        build_road, not here.")]
    async fn validate_road(
        &self,
        Parameters(args): Parameters<BuildRoadArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match service::validate_road(&self.client, args).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(description = "List the available road types (with construction cost).")]
    async fn list_road_types(&self) -> Result<CallToolResult, ErrorData> {
        match service::list_road_types(&self.client).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(description = "List the available zone types.")]
    async fn list_zone_types(&self) -> Result<CallToolResult, ErrorData> {
        match service::list_zone_types(&self.client).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(description = "Control simulation time: pause, resume, step, or set speed.")]
    async fn control_time(
        &self,
        Parameters(args): Parameters<ControlTimeArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match service::control_time(&self.client, args).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(
        description = "Remove a network segment, node, or building. target_type = segment | node | building. \
            Bulldozing an abandoned building (find their ids via `get_city_overview`'s `abandoned_buildings` \
            list, or the `abandoned` flag in `observe_area`) clears the blight it radiates onto its neighbours \
            and frees the lot to be rebuilt by demand."
    )]
    async fn bulldoze(
        &self,
        Parameters(args): Parameters<BulldozeArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match service::bulldoze(&self.client, args).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(
        description = "Change an existing road segment's type. The segment is re-created \
        under a NEW id — `replaced` in the response maps old_segment_id to new_segment_id; \
        refresh any cached ids. The original travel direction is preserved: an `end_to_start` \
        segment stays `end_to_start` after upgrade. Always call `observe_area` or \
        `query_segments` after upgrading one-way segments to confirm direction is correct."
    )]
    async fn upgrade_road(
        &self,
        Parameters(args): Parameters<UpgradeRoadArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match service::upgrade_road(&self.client, args).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(description = "Set zoning over a rectangular area. zone_type from list_zone_types.")]
    async fn set_zoning(
        &self,
        Parameters(args): Parameters<SetZoningArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match service::set_zoning(&self.client, args).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(
        description = "Estimate the route traffic would take between two positions \
        (snapped to nearest road nodes), honoring one-way directions and speed limits. \
        Free read — use it to check whether a new link will actually attract traffic."
    )]
    async fn trace_route(
        &self,
        Parameters(args): Parameters<TraceRouteArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match service::trace_route(&self.client, args).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(description = "Reload a named savegame — the benchmark reset primitive.")]
    async fn reset_scenario(
        &self,
        Parameters(args): Parameters<ResetScenarioArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match service::reset_scenario(&self.client, args).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(
        description = "Angled 3-D screenshot of a location: a 45° game render showing road height, \
        bridges, pillars and overpass clearance — use it to SEE elevation that render_map (top-down) cannot. \
        Args: x, z (world metres), optional size (default 350; larger zooms out), top_down (default false)."
    )]
    async fn view_3d(
        &self,
        Parameters(args): Parameters<ViewArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match service::view_3d(&self.client, args).await {
            Ok(png) => {
                let data = base64::engine::general_purpose::STANDARD.encode(png);
                Ok(CallToolResult::success(vec![Content::image(
                    data,
                    "image/png".to_string(),
                )]))
            }
            Err(e) => Ok(tool_error(e)),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Skyline {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "SkylineBench broker: observe and modify a city simulation via the bridge.",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_all_tools() {
        let tools = Skyline::tool_router().list_all();
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
                "query_problems",
                "query_segments",
                "render_map",
                "reset_scenario",
                "set_zoning",
                "trace_route",
                "upgrade_road",
                "validate_road",
                "view_3d",
            ]
        );
    }
}
