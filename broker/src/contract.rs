use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Position {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Bounds {
    pub min_x: f32,
    pub min_z: f32,
    pub max_x: f32,
    pub max_z: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Health {
    pub mod_version: String,
    pub game_version: String,
    pub city_loaded: bool,
    pub paused: bool,
    pub tick: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetNode {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetSegment {
    pub id: u32,
    pub start_node: u32,
    pub end_node: u32,
    pub prefab: String,
    pub lanes: u8,
    pub length: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Network {
    pub nodes: Vec<NetNode>,
    pub segments: Vec<NetSegment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Building {
    pub id: u32,
    pub prefab: String,
    pub category: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub footprint_width: f32,
    pub footprint_length: f32,
    pub level: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Buildings {
    pub buildings: Vec<Building>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoneCell {
    pub x: f32,
    pub z: f32,
    pub zone_type: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Zones {
    pub cells: Vec<ZoneCell>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentLoad {
    pub segment_id: u32,
    pub density: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrafficMetrics {
    pub flow_percent: f32,
    pub active_vehicles: u32,
    pub segment_loads: Vec<SegmentLoad>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EconomyMetrics {
    pub balance: i64,
    pub weekly_income: i64,
    pub weekly_expenses: i64,
    pub funds: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PopulationMetrics {
    pub total: u32,
    pub residential_demand: u8,
    pub commercial_demand: u8,
    /// CS1 exposes a single combined industrial+office ("workplace") demand,
    /// not separate industrial/office values — see mod DISCOVERY.md.
    pub workplace_demand: u8,
    pub employed: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceMetrics {
    pub happiness: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    pub tick: u64,
    pub traffic: TrafficMetrics,
    pub economy: EconomyMetrics,
    pub population: PopulationMetrics,
    pub services: ServiceMetrics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoadType {
    pub name: String,
    pub construction_cost: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoadTypes {
    pub road_types: Vec<RoadType>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoneTypes {
    pub zone_types: Vec<String>,
}

/// Normalised failure reasons for actions. Extends spec §5's mod-side set
/// (`COLLISION`, `INSUFFICIENT_FUNDS`, `OUT_OF_BOUNDS`, `INVALID_PREFAB`,
/// `SEGMENT_TOO_LONG`, `UNKNOWN`) with broker-side pre-validation reasons
/// (`DEGENERATE_SEGMENT`, `INVALID_ARGS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActionError {
    Collision,
    InsufficientFunds,
    OutOfBounds,
    InvalidPrefab,
    SegmentTooLong,
    DegenerateSegment,
    InvalidArgs,
    Unknown,
}

/// Result of a mutating action. `ok == true` ⇒ the diff fields are meaningful;
/// `ok == false` ⇒ `reason` is set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionResult {
    pub ok: bool,
    #[serde(default)]
    pub created_nodes: Vec<u32>,
    #[serde(default)]
    pub created_segments: Vec<u32>,
    #[serde(default)]
    pub snapped_nodes: Vec<u32>,
    #[serde(default)]
    pub destroyed: Vec<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reason: Option<ActionError>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClockState {
    pub ok: bool,
    pub paused: bool,
    pub tick: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadResult {
    pub ok: bool,
    pub city_loaded: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_error_serializes_screaming_snake() {
        let json = serde_json::to_string(&ActionError::SegmentTooLong).unwrap();
        assert_eq!(json, "\"SEGMENT_TOO_LONG\"");
    }

    #[test]
    fn action_result_round_trips() {
        let original = ActionResult {
            ok: true,
            created_nodes: vec![1, 2],
            created_segments: vec![10],
            snapped_nodes: vec![1],
            destroyed: vec![],
            reason: None,
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: ActionResult = serde_json::from_str(&json).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn action_result_error_serializes_reason() {
        let err = ActionResult {
            ok: false,
            created_nodes: vec![],
            created_segments: vec![],
            snapped_nodes: vec![],
            destroyed: vec![],
            reason: Some(ActionError::Collision),
        };
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"reason\":\"COLLISION\""), "got {json}");
        let parsed: ActionResult = serde_json::from_str(&json).unwrap();
        assert_eq!(err, parsed);
    }

    #[test]
    fn action_result_deserializes_from_sparse_payload() {
        // A mod success payload that omits the diff arrays and reason must
        // default the Vecs to empty and reason to None (exercises #[serde(default)]).
        let parsed: ActionResult = serde_json::from_str("{\"ok\":true}").unwrap();
        assert!(parsed.ok);
        assert!(parsed.created_nodes.is_empty());
        assert!(parsed.created_segments.is_empty());
        assert!(parsed.snapped_nodes.is_empty());
        assert!(parsed.destroyed.is_empty());
        assert_eq!(parsed.reason, None);
    }

    #[test]
    fn road_types_round_trips_with_cost() {
        let original = RoadTypes {
            road_types: vec![
                RoadType { name: "Basic Road".into(), construction_cost: 1200 },
                RoadType { name: "Highway".into(), construction_cost: 8000 },
            ],
        };
        let json = serde_json::to_string(&original).unwrap();
        assert!(
            json.contains("\"name\":\"Basic Road\",\"construction_cost\":1200"),
            "got {json}"
        );
        let parsed: RoadTypes = serde_json::from_str(&json).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn metrics_round_trips() {
        let m = Metrics {
            tick: 42,
            traffic: TrafficMetrics {
                flow_percent: 73.5,
                active_vehicles: 120,
                segment_loads: vec![SegmentLoad {
                    segment_id: 5,
                    density: 0.8,
                }],
            },
            economy: EconomyMetrics {
                balance: 1000,
                weekly_income: 500,
                weekly_expenses: 400,
                funds: 50000,
            },
            population: PopulationMetrics {
                total: 2000,
                residential_demand: 50,
                commercial_demand: 40,
                workplace_demand: 30,
                employed: 1500,
            },
            services: ServiceMetrics { happiness: 80 },
        };
        let json = serde_json::to_string(&m).unwrap();
        let parsed: Metrics = serde_json::from_str(&json).unwrap();
        assert_eq!(m, parsed);
    }
}
