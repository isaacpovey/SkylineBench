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

fn default_travel_direction() -> String {
    "both".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetSegment {
    pub id: u32,
    pub start_node: u32,
    pub end_node: u32,
    pub prefab: String,
    pub lanes: u8,
    pub length: f32,
    #[serde(default)]
    pub one_way: bool,
    /// "both" | "start_to_end" | "end_to_start"
    #[serde(default = "default_travel_direction")]
    pub travel_direction: String,
    /// Game speed units (~1.0 ≈ 50 km/h); 0.0 when unknown.
    #[serde(default)]
    pub speed_limit: f32,
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
    /// Metres. Defaults to 0 for payloads from a mod predating the field.
    #[serde(default)]
    pub length: f32,
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
    /// Buildings flagged Abandoned — a lagging signal that parts of the city
    /// have lost road access or services.
    #[serde(default)]
    pub abandoned_buildings: u32,
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
/// (`DEGENERATE_SEGMENT`, `INVALID_ARGS`). The placement-validation codes
/// (`OBJECT_COLLISION`, `SLOPE_TOO_STEEP`, `OUT_OF_AREA`, `TOO_MANY_CONNECTIONS`,
/// `NET_BUFFER_FULL`) come from the mod's BuildValidator.
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
    ObjectCollision,
    SlopeTooSteep,
    OutOfArea,
    TooManyConnections,
    NetBufferFull,
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
    /// Zoned (RCIO) buildings fronting the affected segment — a neutral fact
    /// for the agent, not a warning. None when the mod didn't compute it.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub zoned_buildings_fronting: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub colliding_buildings: Vec<u32>,
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
    fn net_segment_defaults_direction_fields() {
        // Wire payload from an older mod without the new fields must default.
        let parsed: NetSegment = serde_json::from_str(
            "{\"id\":7,\"start_node\":1,\"end_node\":2,\"prefab\":\"road\",\"lanes\":2,\"length\":100.0}",
        )
        .unwrap();
        assert!(!parsed.one_way);
        assert_eq!(parsed.travel_direction, "both");
        assert_eq!(parsed.speed_limit, 0.0);

        let full: NetSegment = serde_json::from_str(
            "{\"id\":7,\"start_node\":1,\"end_node\":2,\"prefab\":\"hw\",\"lanes\":4,\"length\":100.0,\"one_way\":true,\"travel_direction\":\"end_to_start\",\"speed_limit\":2.0}",
        )
        .unwrap();
        assert!(full.one_way);
        assert_eq!(full.travel_direction, "end_to_start");
    }

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
            zoned_buildings_fronting: None,
            colliding_buildings: vec![],
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: ActionResult = serde_json::from_str(&json).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn action_result_omits_optional_keys_when_absent() {
        // Wire format: zoned_buildings_fronting: None and empty colliding_buildings
        // must be absent from the JSON (skip_serializing_if guards the contract).
        let result = ActionResult {
            ok: true,
            created_nodes: vec![],
            created_segments: vec![],
            snapped_nodes: vec![],
            destroyed: vec![],
            reason: None,
            zoned_buildings_fronting: None,
            colliding_buildings: vec![],
        };
        let json = serde_json::to_string(&result).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(!v.as_object().unwrap().contains_key("zoned_buildings_fronting"), "key must be absent when None: {json}");
        assert!(!v.as_object().unwrap().contains_key("colliding_buildings"), "key must be absent when empty: {json}");
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
            zoned_buildings_fronting: None,
            colliding_buildings: vec![],
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
    fn segment_load_defaults_length_for_old_mod_payloads() {
        let l: SegmentLoad = serde_json::from_str(r#"{"segment_id": 3, "density": 0.9}"#).unwrap();
        assert_eq!(l.length, 0.0);
        let l: SegmentLoad =
            serde_json::from_str(r#"{"segment_id": 3, "density": 0.9, "length": 52.5}"#).unwrap();
        assert_eq!(l.length, 52.5);
    }

    #[test]
    fn service_metrics_default_abandoned_buildings() {
        let s: ServiceMetrics = serde_json::from_str(r#"{"happiness": 80}"#).unwrap();
        assert_eq!(s.abandoned_buildings, 0);
    }

    #[test]
    fn action_result_defaults_new_consequence_fields() {
        let r: ActionResult = serde_json::from_str(r#"{"ok": true}"#).unwrap();
        assert_eq!(r.zoned_buildings_fronting, None);
        assert!(r.colliding_buildings.is_empty());
    }

    #[test]
    fn new_action_errors_serialize_screaming_snake() {
        assert_eq!(serde_json::to_string(&ActionError::ObjectCollision).unwrap(), "\"OBJECT_COLLISION\"");
        assert_eq!(serde_json::to_string(&ActionError::SlopeTooSteep).unwrap(), "\"SLOPE_TOO_STEEP\"");
        assert_eq!(serde_json::to_string(&ActionError::OutOfArea).unwrap(), "\"OUT_OF_AREA\"");
        assert_eq!(serde_json::to_string(&ActionError::TooManyConnections).unwrap(), "\"TOO_MANY_CONNECTIONS\"");
        assert_eq!(serde_json::to_string(&ActionError::NetBufferFull).unwrap(), "\"NET_BUFFER_FULL\"");
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
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["road_types"][0]["name"], "Basic Road");
        assert_eq!(v["road_types"][0]["construction_cost"], 1200);
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
                    length: 0.0,
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
            services: ServiceMetrics { happiness: 80, abandoned_buildings: 0 },
        };
        let json = serde_json::to_string(&m).unwrap();
        let parsed: Metrics = serde_json::from_str(&json).unwrap();
        assert_eq!(m, parsed);
    }
}
