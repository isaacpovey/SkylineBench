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

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ObserveAreaArgs {
    /// Restrict the observation to this rectangle (world metres). Omit for the
    /// whole map. A segment is included when either endpoint is inside.
    #[serde(default)]
    pub bounds: Option<Bounds>,
}

pub async fn observe_area(
    client: &BridgeClient,
    args: ObserveAreaArgs,
) -> Result<Value, ServiceError> {
    let net = client.network().await?;
    let buildings = client.buildings().await?;
    let zones = client.zones().await?;
    let net = match args.bounds {
        None => net,
        Some(b) => {
            let inside = |x: f32, z: f32| {
                crate::geometry::in_bounds(Position { x, y: 0.0, z }, b)
            };
            let node_in: std::collections::HashSet<u32> = net
                .nodes
                .iter()
                .filter(|n| inside(n.x, n.z))
                .map(|n| n.id)
                .collect();
            let segments: Vec<_> = net
                .segments
                .into_iter()
                .filter(|s| node_in.contains(&s.start_node) || node_in.contains(&s.end_node))
                .collect();
            let kept: std::collections::HashSet<u32> = segments
                .iter()
                .flat_map(|s| [s.start_node, s.end_node])
                .collect();
            crate::contract::Network {
                nodes: net.nodes.into_iter().filter(|n| kept.contains(&n.id)).collect(),
                segments,
            }
        }
    };
    let buildings: Vec<_> = match args.bounds {
        None => buildings.buildings,
        Some(b) => buildings
            .buildings
            .into_iter()
            .filter(|bd| crate::geometry::in_bounds(Position { x: bd.x, y: 0.0, z: bd.z }, b))
            .collect(),
    };
    let zones: Vec<_> = match args.bounds {
        None => zones.cells,
        Some(b) => zones
            .cells
            .into_iter()
            .filter(|zc| crate::geometry::in_bounds(Position { x: zc.x, y: 0.0, z: zc.z }, b))
            .collect(),
    };
    let connectivity = build_connectivity(&net);
    Ok(json!({
        "network": net,
        "buildings": buildings,
        "zones": zones,
        "intersections": connectivity.intersections(),
        "dead_ends": connectivity.dead_ends(),
    }))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct GetMetricsArgs {
    /// Optional subset of groups: "traffic","economy","population","services".
    #[serde(default)]
    pub groups: Vec<String>,
}

pub async fn get_metrics(
    client: &BridgeClient,
    args: GetMetricsArgs,
) -> Result<Value, ServiceError> {
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

#[derive(Deserialize, schemars::JsonSchema)]
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
pub async fn render_map(
    client: &BridgeClient,
    args: RenderMapArgs,
) -> Result<Vec<u8>, ServiceError> {
    let net = client.network().await?;
    let opts = RenderOptions {
        bounds: args.bounds.unwrap_or_else(playable_bounds),
        width_px: args.width_px,
        height_px: args.height_px,
    };
    Ok(render_network(&net, &opts))
}

#[derive(Deserialize, schemars::JsonSchema)]
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
    let res = client
        .build_road(args.from, args.to, &args.road_type, args.snap)
        .await?;
    Ok(serde_json::to_value(res).unwrap())
}

pub async fn list_road_types(client: &BridgeClient) -> Result<Value, ServiceError> {
    Ok(json!({ "road_types": client.road_types().await?.road_types }))
}

pub async fn list_zone_types(client: &BridgeClient) -> Result<Value, ServiceError> {
    Ok(json!({ "zone_types": client.zone_types().await?.zone_types }))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ControlTimeArgs {
    pub op: String,
    #[serde(default)]
    pub ticks: Option<u32>,
    #[serde(default)]
    pub speed: Option<u8>,
}

pub async fn control_time(
    client: &BridgeClient,
    args: ControlTimeArgs,
) -> Result<Value, ServiceError> {
    let state = client.clock(&args.op, args.ticks, args.speed).await?;
    Ok(serde_json::to_value(state).unwrap())
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct BulldozeArgs {
    pub target_type: String,
    pub id: u32,
}

pub async fn bulldoze(client: &BridgeClient, args: BulldozeArgs) -> Result<Value, ServiceError> {
    // "building" is valid against the real mod even though the mock has no
    // buildings to remove (it returns INVALID_ARGS for an unknown id).
    if !matches!(args.target_type.as_str(), "segment" | "node" | "building") {
        return Ok(action_error_value(ActionError::InvalidArgs));
    }
    let res = client.bulldoze(&args.target_type, args.id).await?;
    Ok(serde_json::to_value(res).unwrap())
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct UpgradeRoadArgs {
    pub segment: u32,
    pub road_type: String,
}

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

#[derive(Deserialize, schemars::JsonSchema)]
pub struct SetZoningArgs {
    pub area: Bounds,
    pub zone_type: String,
}

pub async fn set_zoning(client: &BridgeClient, args: SetZoningArgs) -> Result<Value, ServiceError> {
    let zone_types = client.zone_types().await?.zone_types;
    if !zone_types.contains(&args.zone_type) {
        return Ok(action_error_value(ActionError::InvalidArgs));
    }
    let res = client.set_zone(args.area, &args.zone_type).await?;
    Ok(serde_json::to_value(res).unwrap())
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ResetScenarioArgs {
    pub save: String,
}

pub async fn reset_scenario(
    client: &BridgeClient,
    args: ResetScenarioArgs,
) -> Result<Value, ServiceError> {
    let res = client.load_save(&args.save).await?;
    Ok(serde_json::to_value(res).unwrap())
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
        let v = get_metrics(
            &c,
            GetMetricsArgs {
                groups: vec!["traffic".into()],
            },
        )
        .await
        .unwrap();
        assert!(v.get("traffic").is_some());
        assert!(v.get("economy").is_none());
    }

    #[tokio::test]
    async fn build_road_rejects_unknown_type_before_hitting_mod() {
        let c = client().await;
        let v = build_road(
            &c,
            BuildRoadArgs {
                from: Position {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                to: Position {
                    x: 50.0,
                    y: 0.0,
                    z: 0.0,
                },
                road_type: "teleporter".into(),
                snap: true,
            },
        )
        .await
        .unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["reason"], "INVALID_PREFAB");
    }

    #[tokio::test]
    async fn build_road_succeeds_and_observe_sees_it() {
        let c = client().await;
        let built = build_road(
            &c,
            BuildRoadArgs {
                from: Position {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                to: Position {
                    x: 50.0,
                    y: 0.0,
                    z: 0.0,
                },
                road_type: "road".into(),
                snap: true,
            },
        )
        .await
        .unwrap();
        assert_eq!(built["ok"], true);
        let obs = observe_area(&c, ObserveAreaArgs { bounds: None }).await.unwrap();
        assert_eq!(obs["network"]["segments"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn render_map_returns_png_bytes() {
        let c = client().await;
        let png = render_map(
            &c,
            RenderMapArgs {
                bounds: None,
                width_px: 64,
                height_px: 64,
            },
        )
        .await
        .unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }

    #[tokio::test]
    async fn bulldoze_removes_a_segment() {
        let c = client().await;
        let built = build_road(
            &c,
            BuildRoadArgs {
                from: Position {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                to: Position {
                    x: 50.0,
                    y: 0.0,
                    z: 0.0,
                },
                road_type: "road".into(),
                snap: true,
            },
        )
        .await
        .unwrap();
        let seg_id = built["created_segments"][0].as_u64().unwrap() as u32;
        let res = bulldoze(
            &c,
            BulldozeArgs {
                target_type: "segment".into(),
                id: seg_id,
            },
        )
        .await
        .unwrap();
        assert_eq!(res["ok"], true);
        let obs = observe_area(&c, ObserveAreaArgs { bounds: None }).await.unwrap();
        assert_eq!(obs["network"]["segments"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn set_zoning_rejects_unknown_zone() {
        let c = client().await;
        let res = set_zoning(
            &c,
            SetZoningArgs {
                area: crate::contract::Bounds {
                    min_x: 0.0,
                    min_z: 0.0,
                    max_x: 10.0,
                    max_z: 10.0,
                },
                zone_type: "spaceport".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(res["ok"], false);
        assert_eq!(res["reason"], "INVALID_ARGS");
    }

    #[tokio::test]
    async fn reset_scenario_clears_the_city() {
        let c = client().await;
        build_road(
            &c,
            BuildRoadArgs {
                from: Position {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                to: Position {
                    x: 50.0,
                    y: 0.0,
                    z: 0.0,
                },
                road_type: "road".into(),
                snap: true,
            },
        )
        .await
        .unwrap();
        reset_scenario(
            &c,
            ResetScenarioArgs {
                save: "anything".into(),
            },
        )
        .await
        .unwrap();
        let obs = observe_area(&c, ObserveAreaArgs { bounds: None }).await.unwrap();
        assert_eq!(obs["network"]["segments"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn upgrade_road_changes_segment_type_over_the_wire() {
        let c = client().await;
        let built = build_road(
            &c,
            BuildRoadArgs {
                from: Position {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                to: Position {
                    x: 50.0,
                    y: 0.0,
                    z: 0.0,
                },
                road_type: "road".into(),
                snap: true,
            },
        )
        .await
        .unwrap();
        let seg_id = built["created_segments"][0].as_u64().unwrap() as u32;
        let res = upgrade_road(
            &c,
            UpgradeRoadArgs {
                segment: seg_id,
                road_type: "highway".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(res["ok"], true);
        let obs = observe_area(&c, ObserveAreaArgs { bounds: None }).await.unwrap();
        assert_eq!(obs["network"]["segments"][0]["prefab"], "highway");
    }

    #[tokio::test]
    async fn set_zoning_adds_a_zone_cell_over_the_wire() {
        let c = client().await;
        let res = set_zoning(
            &c,
            SetZoningArgs {
                area: crate::contract::Bounds {
                    min_x: 0.0,
                    min_z: 0.0,
                    max_x: 16.0,
                    max_z: 16.0,
                },
                zone_type: "residential".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(res["ok"], true);
        let obs = observe_area(&c, ObserveAreaArgs { bounds: None }).await.unwrap();
        assert_eq!(obs["zones"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn observe_area_filters_by_bounds() {
        let c = client().await;
        for (x0, x1) in [(0.0_f32, 50.0_f32), (1000.0, 1050.0)] {
            build_road(
                &c,
                BuildRoadArgs {
                    from: Position { x: x0, y: 0.0, z: 0.0 },
                    to: Position { x: x1, y: 0.0, z: 0.0 },
                    road_type: "road".into(),
                    snap: true,
                },
            )
            .await
            .unwrap();
        }
        let all = observe_area(&c, ObserveAreaArgs { bounds: None }).await.unwrap();
        assert_eq!(all["network"]["segments"].as_array().unwrap().len(), 2);

        let near = observe_area(
            &c,
            ObserveAreaArgs {
                bounds: Some(crate::contract::Bounds { min_x: -10.0, min_z: -10.0, max_x: 100.0, max_z: 10.0 }),
            },
        )
        .await
        .unwrap();
        assert_eq!(near["network"]["segments"].as_array().unwrap().len(), 1);
        assert_eq!(near["network"]["nodes"].as_array().unwrap().len(), 2);

        // Half-crossing: one endpoint inside the rectangle, one outside.
        build_road(
            &c,
            BuildRoadArgs {
                from: Position { x: 50.0, y: 0.0, z: 0.0 },
                to: Position { x: 200.0, y: 0.0, z: 0.0 },
                road_type: "road".into(),
                snap: true,
            },
        )
        .await
        .unwrap();
        let crossing = observe_area(
            &c,
            ObserveAreaArgs {
                bounds: Some(crate::contract::Bounds { min_x: -10.0, min_z: -10.0, max_x: 100.0, max_z: 10.0 }),
            },
        )
        .await
        .unwrap();
        assert_eq!(crossing["network"]["segments"].as_array().unwrap().len(), 2);
    }
}
