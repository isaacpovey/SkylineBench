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
            city: Arc::new(Mutex::new(City { next_id: 1, funds: 100_000, paused: true, ..City::default() })),
        }
    }
}

fn road_types() -> Vec<String> {
    vec!["road".into(), "highway".into(), "oneway".into()]
}

fn zone_types() -> Vec<String> {
    vec!["residential".into(), "commercial".into(), "industrial".into(), "office".into()]
}

async fn health(State(s): State<MockState>) -> Json<Health> {
    let c = s.city.lock().unwrap();
    Json(Health {
        mod_version: "mock-0.1.0".into(),
        game_version: "mock".into(),
        city_loaded: true,
        paused: c.paused,
        tick: c.tick,
    })
}

async fn network(State(s): State<MockState>) -> Json<Network> {
    let c = s.city.lock().unwrap();
    Json(Network { nodes: c.nodes.clone(), segments: c.segments.clone() })
}

async fn buildings(State(_s): State<MockState>) -> Json<Buildings> {
    Json(Buildings { buildings: vec![] })
}

async fn zones(State(_s): State<MockState>) -> Json<Zones> {
    Json(Zones { cells: vec![] })
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
                .map(|sg| SegmentLoad { segment_id: sg.id, density: 0.5 })
                .collect(),
        },
        economy: EconomyMetrics { balance: 0, weekly_income: 1000, weekly_expenses: 800, funds: c.funds },
        population: PopulationMetrics {
            total: 1000,
            residential_demand: 50,
            commercial_demand: 40,
            industrial_demand: 30,
            office_demand: 20,
            employed: 700,
        },
        services: ServiceMetrics { happiness: 75 },
    })
}

async fn road_types_ep() -> Json<RoadTypes> {
    Json(RoadTypes { road_types: road_types() })
}

async fn zone_types_ep() -> Json<ZoneTypes> {
    Json(ZoneTypes { zone_types: zone_types() })
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
    city.nodes.push(NetNode { id, x: p.x, y: p.y, z: p.z });
    (id, false)
}

async fn build_road(State(s): State<MockState>, Json(body): Json<BuildRoadBody>) -> Json<ActionResult> {
    let mut c = s.city.lock().unwrap();
    if !road_types().contains(&body.prefab) {
        return Json(ActionResult {
            ok: false,
            created_nodes: vec![],
            created_segments: vec![],
            snapped_nodes: vec![],
            destroyed: vec![],
            reason: Some(ActionError::InvalidPrefab),
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
    c.segments.push(NetSegment { id: seg_id, start_node: start_id, end_node: end_id, prefab: body.prefab, lanes: 2, length });

    Json(ActionResult { ok: true, created_nodes, created_segments: vec![seg_id], snapped_nodes, destroyed: vec![], reason: None })
}

#[derive(Deserialize)]
struct ClockBody {
    op: String,
    ticks: Option<u32>,
    #[allow(dead_code)]
    speed: Option<u8>,
}

async fn clock(State(s): State<MockState>, Json(body): Json<ClockBody>) -> Json<ClockState> {
    let mut c = s.city.lock().unwrap();
    match body.op.as_str() {
        "pause" => c.paused = true,
        "resume" => c.paused = false,
        "step" => c.tick += body.ticks.unwrap_or(0) as u64,
        _ => {}
    }
    Json(ClockState { ok: true, paused: c.paused, tick: c.tick })
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
        .route("/clock", post(clock))
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

        let before: Metrics =
            client.get(format!("http://{addr}/metrics")).send().await.unwrap().json().await.unwrap();

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

        let after: Metrics =
            client.get(format!("http://{addr}/metrics")).send().await.unwrap().json().await.unwrap();
        assert!(after.traffic.flow_percent < before.traffic.flow_percent);
    }
}
