use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    Submit,
    FlowTarget,
    Timeout,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapInfo {
    pub id: String,
    pub source: String,
    pub game_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowStats {
    pub flow_mean: f64,
    pub active_vehicles_mean: f64,
    pub population: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tally {
    pub num_changes: u32,
    pub money_spent: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionEntry {
    pub seq: u32,
    pub tool: String,
    pub cost: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRecord {
    pub schema_version: u32,
    pub map: MapInfo,
    pub started_at: String,
    pub ended_at: String,
    pub end_reason: EndReason,
    pub baseline: WindowStats,
    #[serde(rename = "final")]
    pub final_stats: WindowStats,
    pub tally: Tally,
    pub actions: Vec<ActionEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreNorms {
    pub flow: f64,
    pub money: f64,
    pub changes: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Score {
    pub norm: ScoreNorms,
    pub weighted: ScoreNorms,
    pub invalid: bool,
    pub score: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_reason_serializes_snake() {
        assert_eq!(serde_json::to_string(&EndReason::FlowTarget).unwrap(), "\"flow_target\"");
        assert_eq!(serde_json::to_string(&EndReason::Submit).unwrap(), "\"submit\"");
        assert_eq!(serde_json::to_string(&EndReason::Timeout).unwrap(), "\"timeout\"");
    }

    #[test]
    fn run_record_round_trips() {
        let r = RunRecord {
            schema_version: 1,
            map: MapInfo { id: "gridlock-v1".into(), source: "workshop:123".into(), game_version: "1.21.1-f9".into() },
            started_at: "2026-06-09T00:00:00Z".into(),
            ended_at: "2026-06-09T01:00:00Z".into(),
            end_reason: EndReason::Submit,
            baseline: WindowStats { flow_mean: 6.0, active_vehicles_mean: 240.0, population: 3380 },
            final_stats: WindowStats { flow_mean: 41.0, active_vehicles_mean: 230.0, population: 3375 },
            tally: Tally { num_changes: 12, money_spent: 250_000 },
            actions: vec![ActionEntry { seq: 1, tool: "build_road".into(), cost: 12000 }],
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: RunRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }
}
