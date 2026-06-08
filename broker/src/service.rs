use serde::Deserialize;
use serde_json::{json, Value};

use crate::bridge_client::{BridgeClient, BridgeError};
use crate::contract::{ActionError, Bounds, Position};
use crate::geometry::playable_bounds;
use crate::graph::build_connectivity;
use crate::render::{render_network, RenderOptions};
use crate::validate::validate_build_road;

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error(transparent)]
    Bridge(#[from] BridgeError),
}

pub async fn get_city_overview(client: &BridgeClient) -> Result<Value, ServiceError> {
    let health = client.health().await?;
    let metrics = client.metrics().await?;
    let net = client.network().await?;
    Ok(json!({
        "tick": health.tick,
        "paused": health.paused,
        "population": metrics.population.total,
        "funds": metrics.economy.funds,
        "traffic_flow_percent": metrics.traffic.flow_percent,
        "node_count": net.nodes.len(),
        "segment_count": net.segments.len(),
    }))
}

pub async fn observe_area(client: &BridgeClient) -> Result<Value, ServiceError> {
    let net = client.network().await?;
    let buildings = client.buildings().await?;
    let zones = client.zones().await?;
    let connectivity = build_connectivity(&net);
    Ok(json!({
        "network": net,
        "buildings": buildings.buildings,
        "zones": zones.cells,
        "intersections": connectivity.intersections(),
        "dead_ends": connectivity.dead_ends(),
    }))
}

#[derive(Deserialize)]
pub struct GetMetricsArgs {
    /// Optional subset of groups: "traffic","economy","population","services".
    #[serde(default)]
    pub groups: Vec<String>,
}

pub async fn get_metrics(client: &BridgeClient, args: GetMetricsArgs) -> Result<Value, ServiceError> {
    let m = client.metrics().await?;
    let want = |g: &str| args.groups.is_empty() || args.groups.iter().any(|x| x == g);
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
    Ok(out)
}

#[derive(Deserialize)]
pub struct RenderMapArgs {
    #[serde(default)]
    pub bounds: Option<Bounds>,
    #[serde(default = "default_size")]
    pub width_px: u32,
    #[serde(default = "default_size")]
    pub height_px: u32,
}

fn default_size() -> u32 {
    512
}

/// Returns the rendered PNG bytes (the rmcp layer wraps these as an image
/// content block).
pub async fn render_map(client: &BridgeClient, args: RenderMapArgs) -> Result<Vec<u8>, ServiceError> {
    let net = client.network().await?;
    let opts = RenderOptions {
        bounds: args.bounds.unwrap_or_else(playable_bounds),
        width_px: args.width_px,
        height_px: args.height_px,
    };
    Ok(render_network(&net, &opts))
}

#[derive(Deserialize)]
pub struct BuildRoadArgs {
    pub from: Position,
    pub to: Position,
    pub road_type: String,
    #[serde(default = "default_true")]
    pub snap: bool,
}

fn default_true() -> bool {
    true
}

pub async fn build_road(client: &BridgeClient, args: BuildRoadArgs) -> Result<Value, ServiceError> {
    let road_types = client.road_types().await?.road_types;
    if let Err(reason) = validate_build_road(args.from, args.to, &args.road_type, &road_types) {
        return Ok(action_error_value(reason));
    }
    let res = client.build_road(args.from, args.to, &args.road_type, args.snap).await?;
    Ok(serde_json::to_value(res).unwrap())
}

pub async fn list_road_types(client: &BridgeClient) -> Result<Value, ServiceError> {
    Ok(json!({ "road_types": client.road_types().await?.road_types }))
}

pub async fn list_zone_types(client: &BridgeClient) -> Result<Value, ServiceError> {
    Ok(json!({ "zone_types": client.zone_types().await?.zone_types }))
}

#[derive(Deserialize)]
pub struct ControlTimeArgs {
    pub op: String,
    #[serde(default)]
    pub ticks: Option<u32>,
    #[serde(default)]
    pub speed: Option<u8>,
}

pub async fn control_time(client: &BridgeClient, args: ControlTimeArgs) -> Result<Value, ServiceError> {
    let state = client.clock(&args.op, args.ticks, args.speed).await?;
    Ok(serde_json::to_value(state).unwrap())
}

fn action_error_value(reason: ActionError) -> Value {
    json!({ "ok": false, "reason": reason })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock;

    async fn client() -> BridgeClient {
        let (addr, server) = mock::bind("127.0.0.1:0".parse().unwrap()).await;
        tokio::spawn(server);
        BridgeClient::new(format!("http://{addr}"))
    }

    #[tokio::test]
    async fn overview_reports_empty_city() {
        let c = client().await;
        let v = get_city_overview(&c).await.unwrap();
        assert_eq!(v["segment_count"], 0);
        assert_eq!(v["traffic_flow_percent"], 100.0);
    }

    #[tokio::test]
    async fn get_metrics_filters_groups() {
        let c = client().await;
        let v = get_metrics(&c, GetMetricsArgs { groups: vec!["traffic".into()] }).await.unwrap();
        assert!(v.get("traffic").is_some());
        assert!(v.get("economy").is_none());
    }

    #[tokio::test]
    async fn build_road_rejects_unknown_type_before_hitting_mod() {
        let c = client().await;
        let v = build_road(&c, BuildRoadArgs {
            from: Position { x: 0.0, y: 0.0, z: 0.0 },
            to: Position { x: 50.0, y: 0.0, z: 0.0 },
            road_type: "teleporter".into(),
            snap: true,
        }).await.unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["reason"], "INVALID_PREFAB");
    }

    #[tokio::test]
    async fn build_road_succeeds_and_observe_sees_it() {
        let c = client().await;
        let built = build_road(&c, BuildRoadArgs {
            from: Position { x: 0.0, y: 0.0, z: 0.0 },
            to: Position { x: 50.0, y: 0.0, z: 0.0 },
            road_type: "road".into(),
            snap: true,
        }).await.unwrap();
        assert_eq!(built["ok"], true);
        let obs = observe_area(&c).await.unwrap();
        assert_eq!(obs["network"]["segments"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn render_map_returns_png_bytes() {
        let c = client().await;
        let png = render_map(&c, RenderMapArgs { bounds: None, width_px: 64, height_px: 64 }).await.unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }
}
