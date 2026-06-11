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
        "forced_paused": health.forced_paused,
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

#[derive(Deserialize, schemars::JsonSchema)]
pub struct RenderMapArgs {
    #[serde(default)]
    pub bounds: Option<Bounds>,
    #[serde(default = "default_size")]
    pub width_px: u32,
    #[serde(default = "default_size")]
    pub height_px: u32,
    /// World metres between gridlines (default 1000; 0 disables the grid).
    #[serde(default)]
    pub grid_spacing_m: Option<f32>,
}

fn default_size() -> u32 {
    512
}

/// Returns the rendered PNG bytes plus a JSON legend describing the encoding
/// (the rmcp layer returns both as image + text content blocks).
pub async fn render_map(
    client: &BridgeClient,
    args: RenderMapArgs,
) -> Result<(Vec<u8>, Value), ServiceError> {
    let net = client.network().await?;
    let loads: std::collections::HashMap<u32, f32> = client
        .metrics()
        .await?
        .traffic
        .segment_loads
        .iter()
        .map(|l| (l.segment_id, l.density))
        .collect();
    // Clamp: a tiny spacing would draw millions of gridlines; 0 disables.
    let grid_spacing_m = args.grid_spacing_m.unwrap_or(1000.0);
    let grid_spacing_m = if grid_spacing_m <= 0.0 || !grid_spacing_m.is_finite() { 0.0 } else { grid_spacing_m.max(100.0) };
    let opts = RenderOptions {
        bounds: args.bounds.unwrap_or_else(playable_bounds),
        width_px: args.width_px,
        height_px: args.height_px,
        grid_spacing_m,
    };
    let legend = json!({
        "bounds": opts.bounds,
        "width_px": opts.width_px,
        "height_px": opts.height_px,
        "grid_spacing_m": opts.grid_spacing_m,
        "encoding": {
            "color": "segment congestion: green = free, yellow = busy, red = saturated, gray = no data",
            "line_width": "scales with lane count",
            "arrows": "white chevron = one-way travel direction",
            "orientation": "+x right, +z up; gridlines every grid_spacing_m world metres, brighter lines are the x=0 / z=0 axes",
        },
    });
    Ok((render_network(&net, &loads, &opts), legend))
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
    let new_id = res.created_segments.first().copied();
    let mut v = serde_json::to_value(res).unwrap();
    if let (Some(new_id), Value::Object(map)) = (new_id, &mut v) {
        map.insert(
            "replaced".into(),
            json!({ "old_segment_id": args.segment, "new_segment_id": new_id }),
        );
    }
    Ok(v)
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

#[derive(Deserialize, schemars::JsonSchema)]
pub struct TraceRouteArgs {
    pub from: Position,
    pub to: Position,
}

pub async fn trace_route(
    client: &BridgeClient,
    args: TraceRouteArgs,
) -> Result<Value, ServiceError> {
    let net = client.network().await?;
    let nearest = |p: Position| {
        net.nodes
            .iter()
            .map(|n| {
                (
                    n.id,
                    crate::geometry::horizontal_distance(p, Position { x: n.x, y: 0.0, z: n.z }),
                )
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    };
    let (Some((from_node, from_dist)), Some((to_node, to_dist))) =
        (nearest(args.from), nearest(args.to))
    else {
        return Ok(json!({ "ok": false, "reason": "EMPTY_NETWORK" }));
    };
    let route = crate::route::shortest_route(&net, from_node, to_node);
    let note = "broker-side estimate from segment lengths, speed limits and one-way directions; \
                the game's own pathfinding also weighs congestion and lane changes";
    Ok(match route {
        Some(r) => json!({
            "ok": true,
            "reachable": true,
            "from_node": from_node,
            "from_snap_distance_m": from_dist,
            "to_node": to_node,
            "to_snap_distance_m": to_dist,
            "nodes": r.nodes,
            "segments": r.segments,
            "total_length_m": r.length_m,
            "note": note,
        }),
        None => json!({
            "ok": true,
            "reachable": false,
            "from_node": from_node,
            "to_node": to_node,
            "note": "no directed path exists — check one-way directions and disconnected components",
        }),
    })
}

fn action_error_value(reason: ActionError) -> Value {
    json!({ "ok": false, "reason": reason })
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct QuerySegmentsArgs {
    /// Sort key, descending: "density" (default), "length", or "speed_limit".
    #[serde(default)]
    pub sort_by: Option<String>,
    /// Max rows returned (default 20, capped at 200).
    #[serde(default)]
    pub limit: Option<usize>,
    /// Keep only segments at or above this density (0..1).
    #[serde(default)]
    pub min_density: Option<f32>,
    /// Keep only segments with an endpoint inside this rectangle.
    #[serde(default)]
    pub bounds: Option<Bounds>,
    /// Case-insensitive substring match on the prefab name.
    #[serde(default)]
    pub prefab_contains: Option<String>,
}

pub async fn query_segments(
    client: &BridgeClient,
    args: QuerySegmentsArgs,
) -> Result<Value, ServiceError> {
    let net = client.network().await?;
    let metrics = client.metrics().await?;
    let density: std::collections::HashMap<u32, f32> = metrics
        .traffic
        .segment_loads
        .iter()
        .map(|l| (l.segment_id, l.density))
        .collect();
    let node_pos: std::collections::HashMap<u32, (f32, f32)> =
        net.nodes.iter().map(|n| (n.id, (n.x, n.z))).collect();
    let needle = args.prefab_contains.as_deref().map(str::to_lowercase);

    let mut rows: Vec<(f32, Value)> = net
        .segments
        .iter()
        .filter_map(|s| {
            let (ax, az) = node_pos.get(&s.start_node).copied()?;
            let (bx, bz) = node_pos.get(&s.end_node).copied()?;
            let d = density.get(&s.id).copied().unwrap_or(0.0);
            let in_bounds = args.bounds.is_none_or(|b| {
                crate::geometry::in_bounds(Position { x: ax, y: 0.0, z: az }, b)
                    || crate::geometry::in_bounds(Position { x: bx, y: 0.0, z: bz }, b)
            });
            let dense_enough = args.min_density.is_none_or(|m| d >= m);
            let prefab_match = needle
                .as_deref()
                .is_none_or(|n| s.prefab.to_lowercase().contains(n));
            (in_bounds && dense_enough && prefab_match).then(|| {
                let key = match args.sort_by.as_deref() {
                    Some("length") => s.length,
                    Some("speed_limit") => s.speed_limit,
                    _ => d,
                };
                (
                    key,
                    json!({
                        "segment_id": s.id,
                        "prefab": s.prefab,
                        "density": d,
                        "one_way": s.one_way,
                        "travel_direction": s.travel_direction,
                        "lanes": s.lanes,
                        "speed_limit": s.speed_limit,
                        "length": s.length,
                        "start_node": s.start_node,
                        "end_node": s.end_node,
                        "midpoint": { "x": (ax + bx) / 2.0, "z": (az + bz) / 2.0 },
                    }),
                )
            })
        })
        .collect();
    rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let total = rows.len();
    let limit = args.limit.unwrap_or(20).min(200);
    let segments: Vec<Value> = rows.into_iter().take(limit).map(|(_, v)| v).collect();
    Ok(json!({ "segments": segments, "total_matching": total }))
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
        assert_eq!(v["forced_paused"], false);
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
    async fn render_map_returns_png_and_legend() {
        let c = client().await;
        let (png, legend) = render_map(
            &c,
            RenderMapArgs { bounds: None, width_px: 64, height_px: 64, grid_spacing_m: None },
        )
        .await
        .unwrap();
        assert_eq!(&png[1..4], b"PNG");
        assert_eq!(legend["grid_spacing_m"], 1000.0);
        assert!(legend["bounds"]["min_x"].is_number());
        assert!(legend["encoding"]["color"].is_string());

        let (_, clamped) = render_map(
            &c,
            RenderMapArgs { bounds: None, width_px: 64, height_px: 64, grid_spacing_m: Some(1.0) },
        )
        .await
        .unwrap();
        assert_eq!(clamped["grid_spacing_m"], 100.0);
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
    async fn upgrade_road_reports_replaced_ids() {
        let c = client().await;
        let built = build_road(
            &c,
            BuildRoadArgs {
                from: Position { x: 0.0, y: 0.0, z: 0.0 },
                to: Position { x: 50.0, y: 0.0, z: 0.0 },
                road_type: "road".into(),
                snap: true,
            },
        )
        .await
        .unwrap();
        let seg_id = built["created_segments"][0].as_u64().unwrap();
        let res = upgrade_road(
            &c,
            UpgradeRoadArgs { segment: seg_id as u32, road_type: "highway".into() },
        )
        .await
        .unwrap();
        assert_eq!(res["ok"], true);
        assert_eq!(res["replaced"]["old_segment_id"], seg_id);
        assert!(res["replaced"]["new_segment_id"].is_u64());
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

    async fn build_three_roads(c: &BridgeClient) {
        // Mock ids increment per node/segment; densities derive from id % 10,
        // so three spaced roads get three distinct densities.
        for (x0, x1) in [(0.0_f32, 50.0_f32), (1000.0, 1050.0), (2000.0, 2050.0)] {
            build_road(
                c,
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
    }

    #[tokio::test]
    async fn query_segments_sorts_by_density_and_limits() {
        let c = client().await;
        build_three_roads(&c).await;
        let v = query_segments(
            &c,
            QuerySegmentsArgs { sort_by: None, limit: Some(2), min_density: None, bounds: None, prefab_contains: None },
        )
        .await
        .unwrap();
        let rows = v["segments"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(v["total_matching"], 3);
        let d0 = rows[0]["density"].as_f64().unwrap();
        let d1 = rows[1]["density"].as_f64().unwrap();
        assert!(d0 >= d1, "descending density: {d0} vs {d1}");
        assert!(rows[0]["midpoint"]["x"].is_number());
        assert!(rows[0]["travel_direction"].is_string());
    }

    #[tokio::test]
    async fn query_segments_filters_by_bounds_and_min_density() {
        let c = client().await;
        build_three_roads(&c).await;
        let v = query_segments(
            &c,
            QuerySegmentsArgs {
                sort_by: None,
                limit: None,
                min_density: None,
                bounds: Some(crate::contract::Bounds { min_x: -10.0, min_z: -10.0, max_x: 100.0, max_z: 10.0 }),
                prefab_contains: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(v["segments"].as_array().unwrap().len(), 1);

        let none = query_segments(
            &c,
            QuerySegmentsArgs { sort_by: None, limit: None, min_density: Some(0.95), bounds: None, prefab_contains: None },
        )
        .await
        .unwrap();
        assert_eq!(none["segments"].as_array().unwrap().len(), 0);
        assert_eq!(none["total_matching"], 0);
    }

    #[tokio::test]
    async fn query_segments_sorts_by_length_and_speed_limit() {
        let c = client().await;
        for (x1, road_type) in [(40.0_f32, "road"), (180.0, "highway")] {
            build_road(
                &c,
                BuildRoadArgs {
                    from: Position { x: 0.0, y: 0.0, z: 0.0 },
                    to: Position { x: x1, y: 0.0, z: 0.0 },
                    road_type: road_type.into(),
                    snap: false,
                },
            )
            .await
            .unwrap();
        }
        let by_length = query_segments(
            &c,
            QuerySegmentsArgs { sort_by: Some("length".into()), limit: None, min_density: None, bounds: None, prefab_contains: None },
        )
        .await
        .unwrap();
        let lengths: Vec<f64> = by_length["segments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["length"].as_f64().unwrap())
            .collect();
        assert_eq!(lengths.len(), 2);
        assert!(lengths[0] > lengths[1], "descending length: {lengths:?}");

        let by_speed = query_segments(
            &c,
            QuerySegmentsArgs { sort_by: Some("speed_limit".into()), limit: None, min_density: None, bounds: None, prefab_contains: None },
        )
        .await
        .unwrap();
        let speeds: Vec<f64> = by_speed["segments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["speed_limit"].as_f64().unwrap())
            .collect();
        assert!(speeds[0] > speeds[1], "descending speed: {speeds:?}");
        assert_eq!(by_speed["segments"][0]["prefab"], "highway");
    }

    #[tokio::test]
    async fn trace_route_follows_the_network() {
        let c = client().await;
        // Two roads sharing a middle node at x=50 (snap tolerance joins them).
        for (x0, x1) in [(0.0_f32, 50.0_f32), (50.0, 100.0)] {
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
        let v = trace_route(
            &c,
            TraceRouteArgs {
                from: Position { x: 2.0, y: 0.0, z: 0.0 },
                to: Position { x: 99.0, y: 0.0, z: 0.0 },
            },
        )
        .await
        .unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["reachable"], true);
        assert_eq!(v["segments"].as_array().unwrap().len(), 2);
        assert_eq!(v["total_length_m"].as_f64().unwrap().round(), 100.0);
        assert!(v["note"].as_str().unwrap().contains("estimate"));
    }

    #[tokio::test]
    async fn trace_route_reports_unreachable() {
        let c = client().await;
        for (x0, x1) in [(0.0_f32, 50.0_f32), (5000.0, 5050.0)] {
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
        let v = trace_route(
            &c,
            TraceRouteArgs {
                from: Position { x: 0.0, y: 0.0, z: 0.0 },
                to: Position { x: 5050.0, y: 0.0, z: 0.0 },
            },
        )
        .await
        .unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["reachable"], false);
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
