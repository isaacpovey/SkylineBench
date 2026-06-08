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
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, Error, ServerHandler,
};
use serde_json::Value;

use crate::bridge_client::BridgeClient;
use crate::service::{
    self, BuildRoadArgs, ControlTimeArgs, GetMetricsArgs, RenderMapArgs, ServiceError,
};

#[derive(Clone)]
pub struct Skyline {
    client: Arc<BridgeClient>,
}

impl Skyline {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            client: Arc::new(BridgeClient::new(base_url)),
        }
    }
}

fn tool_error(err: ServiceError) -> CallToolResult {
    CallToolResult::error(vec![Content::text(err.to_string())])
}

fn json_result(value: Value) -> Result<CallToolResult, Error> {
    Ok(CallToolResult::success(vec![Content::text(
        value.to_string(),
    )]))
}

#[tool(tool_box)]
impl Skyline {
    #[tool(description = "Summarise the city: tick, population, funds, traffic flow, network size.")]
    async fn get_city_overview(&self) -> Result<CallToolResult, Error> {
        match service::get_city_overview(&self.client).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(
        description = "Observe the playable area: network, buildings, zones, intersections, dead ends."
    )]
    async fn observe_area(&self) -> Result<CallToolResult, Error> {
        match service::observe_area(&self.client).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(description = "Render the road network to a PNG image.")]
    async fn render_map(
        &self,
        #[tool(aggr)] args: RenderMapArgs,
    ) -> Result<CallToolResult, Error> {
        match service::render_map(&self.client, args).await {
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

    #[tool(
        description = "Get city metrics, optionally filtered to groups: traffic, economy, population, services."
    )]
    async fn get_metrics(
        &self,
        #[tool(aggr)] args: GetMetricsArgs,
    ) -> Result<CallToolResult, Error> {
        match service::get_metrics(&self.client, args).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(description = "Build a road between two positions of a given road type.")]
    async fn build_road(
        &self,
        #[tool(aggr)] args: BuildRoadArgs,
    ) -> Result<CallToolResult, Error> {
        match service::build_road(&self.client, args).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(description = "List the available road types.")]
    async fn list_road_types(&self) -> Result<CallToolResult, Error> {
        match service::list_road_types(&self.client).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(description = "List the available zone types.")]
    async fn list_zone_types(&self) -> Result<CallToolResult, Error> {
        match service::list_zone_types(&self.client).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }

    #[tool(description = "Control simulation time: pause, resume, step, or set speed.")]
    async fn control_time(
        &self,
        #[tool(aggr)] args: ControlTimeArgs,
    ) -> Result<CallToolResult, Error> {
        match service::control_time(&self.client, args).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }
}

#[tool(tool_box)]
impl ServerHandler for Skyline {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "SkylineBench broker: observe and modify a city simulation via the bridge."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_all_eight_tools() {
        let tools = Skyline::tool_box().list();
        let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            vec![
                "build_road",
                "control_time",
                "get_city_overview",
                "get_metrics",
                "list_road_types",
                "list_zone_types",
                "observe_area",
                "render_map",
            ]
        );
    }
}
