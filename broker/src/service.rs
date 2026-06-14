use serde::Deserialize;
use serde_json::{json, Value};

use crate::bridge_client::{BridgeClient, BridgeError};
use crate::contract::{ActionError, Bounds, Position};
use crate::geometry::{in_bounds, playable_bounds};
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
    let isolated = res.ok && res.snapped_nodes.is_empty();
    let mut v = serde_json::to_value(res).unwrap();
    if isolated {
        if let Value::Object(ref mut map) = v {
            map.insert("isolated_island".into(), json!(true));
            map.insert(
                "warning".into(),
                json!("Neither endpoint snapped to an existing node — this road is disconnected from the network. Traffic will not use it. To connect, place endpoints within 8 m of existing network nodes (use node positions from observe_area or start_node_pos/end_node_pos from query_segments, not midpoints)."),
            );
        }
    }
    Ok(v)
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
    // Guard: refuse to demolish a segment whose endpoints fall outside the buildable
    // area. The game allows such segments (outside connections, edge roads) but
    // build_road rejects those coordinates, making demolition irreversible.
    if args.target_type == "segment" {
        let net = client.network().await?;
        if let Some(seg) = net.segments.iter().find(|s| s.id == args.id) {
            let bounds = playable_bounds();
            let node_out = |nid: u32| {
                net.nodes
                    .iter()
                    .find(|n| n.id == nid)
                    .map(|n| !in_bounds(Position { x: n.x, y: n.y, z: n.z }, bounds))
                    .unwrap_or(false)
            };
            if node_out(seg.start_node) || node_out(seg.end_node) {
                return Ok(json!({
                    "ok": false,
                    "reason": "OUT_OF_BOUNDS",
                    "warning": "Refused: one or both endpoints of this segment lie outside the buildable area. Demolition would be irreversible — build_road cannot recreate roads at those coordinates."
                }));
            }
        }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoView {
    None,
    Traffic,
}

impl InfoView {
    pub fn as_str(self) -> &'static str {
        match self {
            InfoView::None => "none",
            InfoView::Traffic => "traffic",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CameraShot {
    pub x: f32,
    pub z: f32,
    pub size: f32,
    /// Camera heading in degrees (0 = north-up).
    pub yaw: f32,
    /// Camera tilt in degrees (90 = straight down, 45 = default game tilt).
    pub pitch: f32,
    pub info_view: InfoView,
}

/// Floor for the overview zoom so tiny networks aren't framed from 10 m up.
const OVERVIEW_MIN_SIZE_M: f32 = 600.0;
const OVERVIEW_MARGIN: f32 = 1.08;
/// Screen aspect (≈16:9 at the 720p the game runs). The camera `size` is the
/// vertical half-extent in metres; the horizontal half-extent is `size * aspect`.
const OVERVIEW_ASPECT: f32 = 16.0 / 9.0;
/// Fraction of node coordinates trimmed from each end per axis, so the
/// outside-connection highways running to the map edge don't drag the frame
/// off the city or zoom it out to the whole map.
const OVERVIEW_TRIM: f32 = 0.10;
/// Close-up zoom: wide enough to show an intersection plus surroundings.
const CLOSEUP_SIZE_M: f32 = 350.0;
const CLOSEUP_MARGIN: f32 = 1.3;

fn trimmed_bounds(values: impl Iterator<Item = f32>) -> Option<(f32, f32)> {
    let sorted = {
        let mut v: Vec<f32> = values.collect();
        v.sort_by(f32::total_cmp);
        v
    };
    let last = sorted.len().checked_sub(1)?;
    let lo = (last as f32 * OVERVIEW_TRIM).floor() as usize;
    let hi = (last as f32 * (1.0 - OVERVIEW_TRIM)).ceil() as usize;
    Some((sorted[lo], sorted[hi]))
}

pub fn overview_shot(net: &crate::contract::Network) -> CameraShot {
    let bounds = trimmed_bounds(net.nodes.iter().map(|n| n.x))
        .zip(trimmed_bounds(net.nodes.iter().map(|n| n.z)));
    match bounds {
        None => CameraShot { x: 0.0, z: 0.0, size: 2000.0, yaw: 0.0, pitch: 90.0, info_view: InfoView::Traffic },
        Some(((min_x, max_x), (min_z, max_z))) => {
            let dx = max_x - min_x;
            let dz = max_z - min_z;
            // size needed to fit (vertical_span, horizontal_span) in the 16:9 frame.
            // `size` is the vertical half-extent; horizontal half-extent = size * aspect.
            let size_for = |vertical: f32, horizontal: f32| {
                (vertical.max(horizontal / OVERVIEW_ASPECT) * OVERVIEW_MARGIN / 2.0).max(OVERVIEW_MIN_SIZE_M)
            };
            // yaw 0 (north-up): z spans vertically, x spans horizontally.
            let north = size_for(dz, dx);
            // yaw 90: axes swap — x spans vertically, z spans horizontally.
            let rotated = size_for(dx, dz);
            let (yaw, size) = if rotated < north { (90.0, rotated) } else { (0.0, north) };
            CameraShot {
                x: (min_x + max_x) / 2.0,
                z: (min_z + max_z) / 2.0,
                size,
                yaw,
                pitch: 90.0,
                info_view: InfoView::Traffic,
            }
        }
    }
}

pub fn closeup_shot(x: f32, z: f32) -> CameraShot {
    CameraShot { x, z, size: CLOSEUP_SIZE_M, yaw: 0.0, pitch: 45.0, info_view: InfoView::None }
}

/// Frame a set of edit locations in one shot: a plain close-up for a single
/// point, zoomed out just enough to contain all of them otherwise. Replaces
/// averaging the positions, which aimed the camera at empty land whenever a
/// plan's ops were scattered.
pub fn region_shot(positions: &[(f32, f32)]) -> Option<CameraShot> {
    let (min_x, max_x, min_z, max_z) = positions.iter().fold(None, |acc, &(x, z)| {
        let (min_x, max_x, min_z, max_z) = acc.unwrap_or((x, x, z, z));
        Some((min_x.min(x), max_x.max(x), min_z.min(z), max_z.max(z)))
    })?;
    Some(CameraShot {
        x: (min_x + max_x) / 2.0,
        z: (min_z + max_z) / 2.0,
        size: ((max_x - min_x).max(max_z - min_z) * CLOSEUP_MARGIN / 2.0).max(CLOSEUP_SIZE_M),
        yaw: 0.0,
        pitch: 45.0,
        info_view: InfoView::None,
    })
}

/// Flyby tuning (single source of truth — adjust after the first real run).
pub const FLYBY_KEYFRAMES_PER_PASS: usize = 8;
pub const FLYBY_SIZE_M: f32 = 500.0;
pub const FLYBY_PITCH_DEG: f32 = 32.0;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct CameraKeyframe {
    pub x: f32,
    pub z: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub size: f32,
}

#[derive(Debug, Clone, Default)]
pub struct FlybyPath {
    pub ns: Vec<CameraKeyframe>,
    pub we: Vec<CameraKeyframe>,
}

fn is_highway(prefab: &str) -> bool {
    prefab.to_lowercase().contains("highway")
}

/// Reduce a point cloud to a smoothed centerline of keyframes along one axis.
/// `along_z` true: bucket by z (south→north), median x per bucket, yaw 0.
/// false: bucket by x (west→east), median z per bucket, yaw 90.
fn flyby_pass(mut pts: Vec<(f32, f32)>, along_z: bool) -> Vec<CameraKeyframe> {
    if pts.is_empty() {
        return vec![];
    }
    let along = |p: &(f32, f32)| if along_z { p.1 } else { p.0 };
    let cross = |p: &(f32, f32)| if along_z { p.0 } else { p.1 };
    pts.sort_by(|a, b| along(a).total_cmp(&along(b)));
    let n = pts.len();
    let bins = FLYBY_KEYFRAMES_PER_PASS.min(n).max(1);
    (0..bins)
        .map(|i| {
            let lo = i * n / bins;
            let hi = (((i + 1) * n / bins).max(lo + 1)).min(n);
            let slice = &pts[lo..hi];
            let along_mean = slice.iter().map(along).sum::<f32>() / slice.len() as f32;
            let mut cs: Vec<f32> = slice.iter().map(cross).collect();
            cs.sort_by(f32::total_cmp);
            let cross_med = cs[cs.len() / 2];
            let (x, z) = if along_z { (cross_med, along_mean) } else { (along_mean, cross_med) };
            CameraKeyframe {
                x,
                z,
                yaw: if along_z { 0.0 } else { 90.0 },
                pitch: FLYBY_PITCH_DEG,
                size: FLYBY_SIZE_M,
            }
        })
        .collect()
}

/// Build N/S and W/E flyby keyframe passes along the main highways. Falls back
/// to the whole network when the city has no highway segments.
pub fn highway_flyby_path(net: &crate::contract::Network) -> FlybyPath {
    let node_xz: std::collections::HashMap<u32, (f32, f32)> =
        net.nodes.iter().map(|n| (n.id, (n.x, n.z))).collect();
    let collect = |highway_only: bool| -> Vec<(f32, f32)> {
        net.segments
            .iter()
            .filter(|s| !highway_only || is_highway(&s.prefab))
            .flat_map(|s| [s.start_node, s.end_node])
            .filter_map(|id| node_xz.get(&id).copied())
            .collect()
    };
    let pts = {
        let hw = collect(true);
        if hw.is_empty() {
            collect(false)
        } else {
            hw
        }
    };
    FlybyPath {
        ns: flyby_pass(pts.clone(), true),
        we: flyby_pass(pts, false),
    }
}

pub async fn capture_screenshot(
    client: &BridgeClient,
    shot: CameraShot,
) -> Result<Vec<u8>, ServiceError> {
    Ok(client.screenshot(shot.x, shot.z, shot.size, shot.yaw, shot.pitch, shot.info_view.as_str()).await?)
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
                        "start_node_pos": { "x": ax, "z": az },
                        "end_node": s.end_node,
                        "end_node_pos": { "x": bx, "z": bz },
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

    #[test]
    fn camera_shots_carry_yaw_pitch_and_info_view() {
        let cu = closeup_shot(10.0, 20.0);
        assert_eq!(cu.pitch, 45.0, "close-ups use the angled game tilt");
        assert_eq!(cu.yaw, 0.0);
        assert!(matches!(cu.info_view, InfoView::None), "close-ups stay a clean render");
        assert_eq!(InfoView::Traffic.as_str(), "traffic");
        assert_eq!(InfoView::None.as_str(), "none");
    }

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
    async fn bulldoze_blocks_segment_with_out_of_bounds_node() {
        // Plant an out-of-bounds segment by calling the bridge client directly —
        // the mock has no bounds check, so this succeeds where service::build_road
        // would reject it. This simulates a game-placed "outside connection" road.
        let (addr, server) = crate::mock::bind("127.0.0.1:0".parse().unwrap()).await;
        tokio::spawn(server);
        let raw = BridgeClient::new(format!("http://{addr}"));
        let oob = raw
            .build_road(
                Position { x: 0.0, y: 0.0, z: 0.0 },
                Position { x: 20000.0, y: 0.0, z: 0.0 }, // well outside ±8640
                "road",
                false,
            )
            .await
            .unwrap();
        assert!(oob.ok);
        let seg_id = oob.created_segments[0];

        // Service-layer bulldoze must refuse because the endpoint is outside bounds.
        let c = BridgeClient::new(format!("http://{addr}"));
        let res = bulldoze(&c, BulldozeArgs { target_type: "segment".into(), id: seg_id })
            .await
            .unwrap();
        assert_eq!(res["ok"], false, "out-of-bounds segment must be refused");
        assert_eq!(res["reason"], "OUT_OF_BOUNDS");
        assert!(res["warning"].as_str().unwrap().contains("irreversible"));
    }

    #[tokio::test]
    async fn bulldoze_allows_segment_within_bounds() {
        let c = client().await;
        let built = build_road(
            &c,
            BuildRoadArgs {
                from: Position { x: 0.0, y: 0.0, z: 0.0 },
                to: Position { x: 50.0, y: 0.0, z: 0.0 },
                road_type: "road".into(),
                snap: false,
            },
        )
        .await
        .unwrap();
        let seg_id = built["created_segments"][0].as_u64().unwrap() as u32;
        let res = bulldoze(&c, BulldozeArgs { target_type: "segment".into(), id: seg_id })
            .await
            .unwrap();
        assert_eq!(res["ok"], true, "in-bounds segment must be bulldozable");
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
                save: "gridlock-v1".into(),
            },
        )
        .await
        .unwrap();
        let obs = observe_area(&c, ObserveAreaArgs { bounds: None }).await.unwrap();
        assert_eq!(obs["network"]["segments"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn reset_scenario_reports_resolved_identity() {
        let c = client().await;
        let res = reset_scenario(
            &c,
            ResetScenarioArgs {
                save: "gridlock-v1".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(res["ok"], true);
        assert_eq!(res["resolved"]["name"], "gridlock-v1");
    }

    #[tokio::test]
    async fn reset_scenario_unknown_save_lists_available() {
        let c = client().await;
        let res = reset_scenario(
            &c,
            ResetScenarioArgs {
                save: "nope".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(res["ok"], false);
        assert_eq!(res["available"][0]["name"], "gridlock-v1");
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

    #[test]
    fn overview_shot_frames_the_network_with_margin() {
        let net = crate::contract::Network {
            nodes: vec![
                crate::contract::NetNode { id: 1, x: -1000.0, y: 0.0, z: -500.0 },
                crate::contract::NetNode { id: 2, x: 1000.0, y: 0.0, z: 500.0 },
            ],
            segments: vec![],
        };
        let shot = overview_shot(&net);
        assert_eq!(shot.x, 0.0);
        assert_eq!(shot.z, 0.0);
        assert_eq!(shot.pitch, 90.0);
        // dx=2000, dz=1000. City is wider in x — x already maps to the wide horizontal axis at yaw 0.
        // north   = size_for(dz=1000, dx=2000) = max(1000, 2000/1.777…) * 1.08/2
        //         = max(1000, 1125) * 0.54 = 1125 * 0.54 = 607.5.
        // rotated = size_for(dx=2000, dz=1000) = max(2000, 1000/1.777…) * 0.54
        //         = max(2000, 562.5) * 0.54 = 2000 * 0.54 = 1080.
        // north(607.5) < rotated(1080) → yaw=0, size=607.5.
        assert_eq!(shot.yaw, 0.0, "wider network stays north-up; x already fills the wide frame");
        assert_eq!(shot.size, 607.5);
        assert!(matches!(shot.info_view, InfoView::Traffic));
    }

    #[test]
    fn overview_shot_of_empty_network_uses_default_frame() {
        let net = crate::contract::Network { nodes: vec![], segments: vec![] };
        let shot = overview_shot(&net);
        assert_eq!((shot.x, shot.z), (0.0, 0.0));
        assert_eq!(shot.size, 2000.0);
    }

    #[test]
    fn closeup_shot_targets_the_location() {
        let shot = closeup_shot(150.0, -75.0);
        assert_eq!((shot.x, shot.z), (150.0, -75.0));
        assert_eq!(shot.pitch, 45.0);
        assert_eq!(shot.size, 350.0);
    }

    #[test]
    fn overview_shot_ignores_outlying_highway_nodes() {
        let cluster = (0..40).map(|i| crate::contract::NetNode {
            id: i,
            x: (i % 8) as f32 * 100.0,
            y: 0.0,
            z: (i / 8) as f32 * 100.0,
        });
        let outliers = [(100, -8000.0, -8000.0), (101, 8000.0, 8000.0)]
            .into_iter()
            .map(|(id, x, z)| crate::contract::NetNode { id, x, y: 0.0, z });
        let net = crate::contract::Network { nodes: cluster.chain(outliers).collect(), segments: vec![] };
        let shot = overview_shot(&net);
        // Trimming drops the two map-edge nodes: centred on the 700×400 city
        // grid, not the 16 km outlier span, and at the minimum height floor.
        assert!((shot.x - 350.0).abs() < 100.0, "x {} should be near the cluster centre", shot.x);
        assert!((shot.z - 200.0).abs() < 100.0, "z {} should be near the cluster centre", shot.z);
        assert_eq!(shot.size, 600.0);
    }

    #[test]
    fn overview_keeps_wide_city_north_up_with_traffic() {
        // dx=2000, dz=200. Wide city — x already maps to the wide horizontal screen axis at yaw 0.
        // north   = size_for(dz=200, dx=2000) = max(200, 1125) * 0.54 = 607.5.
        // rotated = size_for(dx=2000, dz=200) = max(2000, 112.5) * 0.54 = 1080.
        // north(607.5) < rotated(1080) → yaw=0.
        use crate::contract::{NetNode, Network};
        let net = Network {
            nodes: vec![
                NetNode { id: 0, x: -1000.0, y: 0.0, z: -100.0 },
                NetNode { id: 1, x: 1000.0, y: 0.0, z: 100.0 },
            ],
            segments: vec![],
        };
        let ov = overview_shot(&net);
        assert_eq!(ov.yaw, 0.0, "wide city stays north-up; its long axis already fills the wide frame");
        assert_eq!(ov.pitch, 90.0, "overview stays top-down");
        assert!(matches!(ov.info_view, InfoView::Traffic), "overview carries the traffic layer");
    }

    #[test]
    fn overview_rotates_tall_city_into_the_wide_frame() {
        // dx=200, dz=2000. Tall city — rotating 90° lays the long z-axis along the wide frame.
        // north   = size_for(dz=2000, dx=200) = max(2000, 112.5) * 0.54 = 1080.
        // rotated = size_for(dx=200, dz=2000) = max(200, 1125) * 0.54 = 607.5.
        // rotated(607.5) < north(1080) → yaw=90.
        use crate::contract::{NetNode, Network};
        let net = Network {
            nodes: vec![
                NetNode { id: 0, x: -100.0, y: 0.0, z: -1000.0 },
                NetNode { id: 1, x: 100.0, y: 0.0, z: 1000.0 },
            ],
            segments: vec![],
        };
        assert_eq!(overview_shot(&net).yaw, 90.0, "tall city rotates so its long z-axis fills the wide frame");
    }

    #[test]
    fn region_shot_of_single_position_is_a_closeup() {
        let shot = region_shot(&[(150.0, -75.0)]).unwrap();
        assert_eq!((shot.x, shot.z), (150.0, -75.0));
        assert_eq!(shot.size, 350.0);
        assert_eq!(shot.pitch, 45.0);
    }

    #[test]
    fn region_shot_frames_scattered_positions() {
        let shot = region_shot(&[(0.0, 0.0), (1000.0, 400.0)]).unwrap();
        assert_eq!((shot.x, shot.z), (500.0, 200.0));
        // span 1000m * 1.3 margin / 2 = 650 — wide enough to contain both edits.
        assert_eq!(shot.size, 650.0);
        assert!(region_shot(&[]).is_none());
    }

    #[test]
    fn highway_flyby_path_orders_ns_by_z_and_we_by_x() {
        use crate::contract::{NetNode, NetSegment, Network};
        let node = |id: u32, x: f32, z: f32| NetNode { id, x, y: 0.0, z };
        let seg = |id: u32, a: u32, b: u32| NetSegment {
            id, start_node: a, end_node: b, prefab: "Highway".into(),
            lanes: 4, length: 100.0, one_way: false, travel_direction: "both".into(), speed_limit: 2.0,
        };
        let net = Network {
            nodes: vec![node(0, 0.0, -500.0), node(1, 10.0, 0.0), node(2, -10.0, 500.0),
                        node(3, -500.0, 5.0), node(4, 500.0, -5.0)],
            segments: vec![seg(0, 0, 1), seg(1, 1, 2), seg(2, 3, 4)],
        };
        let path = highway_flyby_path(&net);
        assert!(!path.ns.is_empty() && !path.we.is_empty());
        assert!(path.ns.windows(2).all(|w| w[0].z <= w[1].z), "ns ascends south->north");
        assert!(path.we.windows(2).all(|w| w[0].x <= w[1].x), "we ascends west->east");
        assert_eq!(path.ns[0].yaw, 0.0);
        assert_eq!(path.we[0].yaw, 90.0);
        assert_eq!(path.ns[0].pitch, FLYBY_PITCH_DEG);
        assert_eq!(path.ns[0].size, FLYBY_SIZE_M);
    }

    #[test]
    fn highway_flyby_path_falls_back_to_all_segments_without_highways() {
        use crate::contract::{NetNode, NetSegment, Network};
        let net = Network {
            nodes: vec![NetNode { id: 0, x: 0.0, y: 0.0, z: 0.0 }, NetNode { id: 1, x: 100.0, y: 0.0, z: 100.0 }],
            segments: vec![NetSegment {
                id: 0, start_node: 0, end_node: 1, prefab: "Basic Road".into(),
                lanes: 2, length: 100.0, one_way: false, travel_direction: "both".into(), speed_limit: 1.0,
            }],
        };
        assert!(!highway_flyby_path(&net).ns.is_empty(), "falls back to all segments");
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
