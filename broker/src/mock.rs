use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tokio::net::TcpListener;

use crate::contract::*;
use crate::geometry::{horizontal_distance, nearest_node_within_tolerance};

#[derive(Default)]
struct City {
    nodes: Vec<NetNode>,
    segments: Vec<NetSegment>,
    zones: Vec<ZoneCell>,
    next_id: u32,
    tick: u64,
    paused: bool,
    funds: i64,
}

#[derive(Clone)]
pub struct MockState {
    city: Arc<Mutex<City>>,
}

impl MockState {
    fn new() -> Self {
        MockState {
            city: Arc::new(Mutex::new(City {
                next_id: 1,
                funds: 100_000,
                paused: true,
                ..City::default()
            })),
        }
    }
}

fn road_types() -> Vec<RoadType> {
    vec![
        RoadType { name: "road".into(), construction_cost: 1000 },
        RoadType { name: "oneway".into(), construction_cost: 1500 },
        RoadType { name: "highway".into(), construction_cost: 5000 },
    ]
}

fn zone_types() -> Vec<String> {
    vec![
        "residential".into(),
        "commercial".into(),
        "industrial".into(),
        "office".into(),
    ]
}

async fn health(State(s): State<MockState>) -> Json<Health> {
    let c = s.city.lock().unwrap();
    Json(Health {
        mod_version: "mock-0.1.0".into(),
        game_version: "mock".into(),
        city_loaded: true,
        paused: c.paused,
        forced_paused: false,
        tick: c.tick,
    })
}

async fn network(State(s): State<MockState>) -> Json<Network> {
    let c = s.city.lock().unwrap();
    Json(Network {
        nodes: c.nodes.clone(),
        segments: c.segments.clone(),
    })
}

async fn buildings(State(_s): State<MockState>) -> Json<Buildings> {
    Json(Buildings { buildings: vec![] })
}

async fn zones(State(s): State<MockState>) -> Json<Zones> {
    let c = s.city.lock().unwrap();
    Json(Zones {
        cells: c.zones.clone(),
    })
}

async fn metrics(State(s): State<MockState>) -> Json<Metrics> {
    let c = s.city.lock().unwrap();
    // Mock heuristic: flow degrades as the network grows, so the broker's
    // observe→build→step→observe loop sees a real, deterministic metric change.
    let flow = (100.0 - c.segments.len() as f32 * 5.0).max(0.0);
    Json(Metrics {
        tick: c.tick,
        traffic: TrafficMetrics {
            flow_percent: flow,
            active_vehicles: c.segments.len() as u32 * 10,
            segment_loads: c
                .segments
                .iter()
                .map(|sg| SegmentLoad {
                    segment_id: sg.id,
                    density: (sg.id % 10) as f32 / 10.0,
                    length: sg.length,
                })
                .collect(),
        },
        economy: EconomyMetrics {
            balance: 0,
            weekly_income: 1000,
            weekly_expenses: 800,
            funds: c.funds,
        },
        population: PopulationMetrics {
            total: 1000,
            residential_demand: 50,
            commercial_demand: 40,
            workplace_demand: 30,
            employed: 700,
        },
        services: ServiceMetrics { happiness: 75, abandoned_buildings: 0 },
    })
}

async fn road_types_ep() -> Json<RoadTypes> {
    Json(RoadTypes { road_types: road_types() })
}

async fn zone_types_ep() -> Json<ZoneTypes> {
    Json(ZoneTypes {
        zone_types: zone_types(),
    })
}

#[derive(Deserialize)]
struct BuildRoadBody {
    start: Position,
    end: Position,
    prefab: String,
    snap_to_existing_nodes: bool,
}

/// Resolves a position to an existing node id (when snapping) or allocates a
/// new node, returning `(node_id, was_snapped)`. Splitting this out of
/// `build_road` avoids a closure that would need to capture both `created_nodes`
/// and `&mut City` simultaneously, which the borrow checker rejects.
fn resolve_node(p: Position, snap: bool, city: &mut City) -> (u32, bool) {
    if snap {
        if let Some(id) = nearest_node_within_tolerance(p, &city.nodes) {
            return (id, true);
        }
    }
    let id = city.next_id;
    city.next_id += 1;
    city.nodes.push(NetNode {
        id,
        x: p.x,
        y: p.y,
        z: p.z,
    });
    (id, false)
}

async fn build_road(
    State(s): State<MockState>,
    Json(body): Json<BuildRoadBody>,
) -> Json<ActionResult> {
    let mut c = s.city.lock().unwrap();
    if !road_types().iter().any(|r| r.name == body.prefab) {
        return Json(ActionResult {
            ok: false,
            created_nodes: vec![],
            created_segments: vec![],
            snapped_nodes: vec![],
            destroyed: vec![],
            reason: Some(ActionError::InvalidPrefab),
            zoned_buildings_fronting: None,
            colliding_buildings: vec![],
        });
    }

    let snap = body.snap_to_existing_nodes;
    let (start_id, start_snapped) = resolve_node(body.start, snap, &mut c);
    let (end_id, end_snapped) = resolve_node(body.end, snap, &mut c);

    let mut created_nodes = vec![];
    let mut snapped_nodes = vec![];

    if start_snapped {
        snapped_nodes.push(start_id);
    } else {
        created_nodes.push(start_id);
    }
    if end_snapped {
        snapped_nodes.push(end_id);
    } else {
        created_nodes.push(end_id);
    }

    let seg_id = c.next_id;
    c.next_id += 1;
    let length = horizontal_distance(body.start, body.end);
    let (one_way, speed_limit) = match body.prefab.as_str() {
        "oneway" => (true, 1.2),
        "highway" => (true, 2.0),
        _ => (false, 1.0),
    };
    c.segments.push(NetSegment {
        id: seg_id,
        start_node: start_id,
        end_node: end_id,
        prefab: body.prefab,
        lanes: 2,
        length,
        one_way,
        travel_direction: if one_way { "start_to_end".into() } else { "both".into() },
        speed_limit,
    });

    Json(ActionResult {
        ok: true,
        created_nodes,
        created_segments: vec![seg_id],
        snapped_nodes,
        destroyed: vec![],
        reason: None,
        zoned_buildings_fronting: Some(0),
        colliding_buildings: vec![],
    })
}

async fn validate_road(
    State(_s): State<MockState>,
    Json(body): Json<BuildRoadBody>,
) -> Json<ActionResult> {
    let known = road_types().iter().any(|r| r.name == body.prefab);
    Json(ActionResult {
        ok: known,
        created_nodes: vec![],
        created_segments: vec![],
        snapped_nodes: vec![],
        destroyed: vec![],
        reason: if known { None } else { Some(ActionError::InvalidPrefab) },
        zoned_buildings_fronting: if known { Some(0) } else { None },
        colliding_buildings: vec![],
    })
}

#[derive(Deserialize)]
struct BulldozeBody {
    target_type: String,
    id: u32,
}

async fn bulldoze(
    State(s): State<MockState>,
    Json(body): Json<BulldozeBody>,
) -> Json<ActionResult> {
    let mut c = s.city.lock().unwrap();
    let removed = match body.target_type.as_str() {
        "segment" => {
            let before = c.segments.len();
            c.segments.retain(|sg| sg.id != body.id);
            before != c.segments.len()
        }
        "node" => {
            let before = c.nodes.len();
            c.nodes.retain(|n| n.id != body.id);
            before != c.nodes.len()
        }
        _ => false,
    };
    if removed {
        Json(ActionResult {
            ok: true,
            created_nodes: vec![],
            created_segments: vec![],
            snapped_nodes: vec![],
            destroyed: vec![body.id],
            reason: None,
            zoned_buildings_fronting: None,
            colliding_buildings: vec![],
        })
    } else {
        Json(ActionResult {
            ok: false,
            created_nodes: vec![],
            created_segments: vec![],
            snapped_nodes: vec![],
            destroyed: vec![],
            reason: Some(ActionError::InvalidArgs),
            zoned_buildings_fronting: None,
            colliding_buildings: vec![],
        })
    }
}

#[derive(Deserialize)]
struct UpgradeBody {
    segment_id: u32,
    prefab: String,
}

async fn upgrade_road(
    State(s): State<MockState>,
    Json(body): Json<UpgradeBody>,
) -> Json<ActionResult> {
    let mut c = s.city.lock().unwrap();
    if !road_types().iter().any(|r| r.name == body.prefab) {
        return Json(ActionResult {
            ok: false,
            created_nodes: vec![],
            created_segments: vec![],
            snapped_nodes: vec![],
            destroyed: vec![],
            reason: Some(ActionError::InvalidPrefab),
            zoned_buildings_fronting: None,
            colliding_buildings: vec![],
        });
    }
    match c.segments.iter_mut().find(|sg| sg.id == body.segment_id) {
        Some(sg) => {
            sg.prefab = body.prefab;
            Json(ActionResult {
                ok: true,
                created_nodes: vec![],
                created_segments: vec![body.segment_id],
                snapped_nodes: vec![],
                destroyed: vec![],
                reason: None,
                zoned_buildings_fronting: None,
                colliding_buildings: vec![],
            })
        }
        None => Json(ActionResult {
            ok: false,
            created_nodes: vec![],
            created_segments: vec![],
            snapped_nodes: vec![],
            destroyed: vec![],
            reason: Some(ActionError::InvalidArgs),
            zoned_buildings_fronting: None,
            colliding_buildings: vec![],
        }),
    }
}

#[derive(Deserialize)]
struct SetZoneBody {
    rect: Bounds,
    zone_type: String,
}

async fn set_zone(State(s): State<MockState>, Json(body): Json<SetZoneBody>) -> Json<ActionResult> {
    let mut c = s.city.lock().unwrap();
    if !zone_types().contains(&body.zone_type) {
        return Json(ActionResult {
            ok: false,
            created_nodes: vec![],
            created_segments: vec![],
            snapped_nodes: vec![],
            destroyed: vec![],
            reason: Some(ActionError::InvalidArgs),
            zoned_buildings_fronting: None,
            colliding_buildings: vec![],
        });
    }
    c.zones.push(ZoneCell {
        x: (body.rect.min_x + body.rect.max_x) / 2.0,
        z: (body.rect.min_z + body.rect.max_z) / 2.0,
        zone_type: body.zone_type,
    });
    Json(ActionResult {
        ok: true,
        created_nodes: vec![],
        created_segments: vec![],
        snapped_nodes: vec![],
        destroyed: vec![],
        reason: None,
        zoned_buildings_fronting: None,
        colliding_buildings: vec![],
    })
}

#[derive(Deserialize)]
struct LoadSaveBody {
    #[allow(dead_code)]
    save_name: String,
}

async fn load_save(
    State(s): State<MockState>,
    Json(_body): Json<LoadSaveBody>,
) -> Json<LoadResult> {
    let mut c = s.city.lock().unwrap();
    c.nodes.clear();
    c.segments.clear();
    c.zones.clear();
    c.tick = 0;
    c.next_id = 1;
    Json(LoadResult {
        ok: true,
        city_loaded: true,
    })
}

#[derive(Deserialize)]
struct ClockBody {
    op: String,
    ticks: Option<u32>,
    #[allow(dead_code)]
    speed: Option<u8>,
}

/// Test-only sentinel: a step of exactly this many ticks makes the mock report
/// `forced_paused: true`, simulating a game modal dialog holding
/// SimulationManager.ForcedSimulationPaused. The value must fit in a single
/// step chunk (the benchmark server splits steps into 585-tick days and caps
/// requests at 4095), so a larger magic number would never reach the mock.
const FORCED_PAUSE_SENTINEL_TICKS: u32 = 424;

async fn clock(State(s): State<MockState>, Json(body): Json<ClockBody>) -> Json<ClockState> {
    let mut c = s.city.lock().unwrap();
    let forced_paused =
        body.op == "step" && body.ticks == Some(FORCED_PAUSE_SENTINEL_TICKS);
    match body.op.as_str() {
        "pause" => c.paused = true,
        "resume" => c.paused = false,
        // Mirrors the real mod's bail under a forced pause: Step returns
        // immediately and the tick does not move.
        "step" if !forced_paused => c.tick += body.ticks.unwrap_or(0) as u64,
        _ => {}
    }
    Json(ClockState {
        ok: true,
        paused: c.paused,
        tick: c.tick,
        forced_paused,
    })
}

async fn screenshot(
    State(s): State<MockState>,
    Json(_body): Json<serde_json::Value>,
) -> impl axum::response::IntoResponse {
    let net = {
        let c = s.city.lock().unwrap();
        Network { nodes: c.nodes.clone(), segments: c.segments.clone() }
    };
    let opts = crate::render::RenderOptions {
        bounds: crate::geometry::playable_bounds(),
        width_px: 64,
        height_px: 64,
        grid_spacing_m: 0.0,
    };
    let png = crate::render::render_network(&net, &std::collections::HashMap::new(), &opts);
    ([(axum::http::header::CONTENT_TYPE, "image/png")], png)
}

pub fn router() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/network", get(network))
        .route("/buildings", get(buildings))
        .route("/zones", get(zones))
        .route("/metrics", get(metrics))
        .route("/road-types", get(road_types_ep))
        .route("/zone-types", get(zone_types_ep))
        .route("/action/build-road", post(build_road))
        .route("/action/validate-road", post(validate_road))
        .route("/action/bulldoze", post(bulldoze))
        .route("/action/upgrade-road", post(upgrade_road))
        .route("/action/set-zone", post(set_zone))
        .route("/load-save", post(load_save))
        .route("/clock", post(clock))
        .route("/screenshot", post(screenshot))
        .with_state(MockState::new())
}

/// Bind to `addr` (use port 0 for an ephemeral port) and return the actual
/// bound address plus a future that serves until the process ends.
pub async fn bind(addr: SocketAddr) -> (SocketAddr, impl std::future::Future<Output = ()>) {
    let listener = TcpListener::bind(addr).await.expect("bind mock");
    let local = listener.local_addr().unwrap();
    let fut = async move {
        axum::serve(listener, router()).await.unwrap();
    };
    (local, fut)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn road_types_endpoint_includes_costs() {
        let (addr, server) = bind("127.0.0.1:0".parse().unwrap()).await;
        tokio::spawn(server);
        let body: crate::contract::RoadTypes = reqwest::Client::new()
            .get(format!("http://{addr}/road-types"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let road = body.road_types.iter().find(|r| r.name == "road").unwrap();
        assert!(road.construction_cost > 0);
        let highway = body.road_types.iter().find(|r| r.name == "highway").unwrap();
        assert!(highway.construction_cost > road.construction_cost);
    }

    #[tokio::test]
    async fn mock_serves_health() {
        let (addr, server) = bind("127.0.0.1:0".parse().unwrap()).await;
        tokio::spawn(server);
        let url = format!("http://{addr}/health");
        let resp: Health = reqwest::get(&url).await.unwrap().json().await.unwrap();
        assert!(resp.city_loaded);
        assert!(resp.paused);
    }

    #[tokio::test]
    async fn build_then_metrics_changes_flow() {
        let (addr, server) = bind("127.0.0.1:0".parse().unwrap()).await;
        tokio::spawn(server);
        let client = reqwest::Client::new();

        let before: Metrics = client
            .get(format!("http://{addr}/metrics"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        let body = serde_json::json!({
            "start": {"x": 0.0, "y": 0.0, "z": 0.0},
            "end": {"x": 50.0, "y": 0.0, "z": 0.0},
            "prefab": "road",
            "snap_to_existing_nodes": true
        });
        let res: ActionResult = client
            .post(format!("http://{addr}/action/build-road"))
            .json(&body)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(res.ok);
        assert_eq!(res.created_segments.len(), 1);

        let after: Metrics = client
            .get(format!("http://{addr}/metrics"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(after.traffic.flow_percent < before.traffic.flow_percent);
    }
}
