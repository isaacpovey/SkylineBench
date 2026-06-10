use std::collections::HashMap;
use std::time::Instant;

use serde_json::{json, Value};

use crate::benchmark::config::BenchConfig;
use crate::benchmark::cost::road_cost;
use crate::benchmark::flow_window::FlowWindow;
use crate::benchmark::record::{ActionEntry, EndReason, EndState, MapInfo, Tally, WindowStats};

pub struct RunState {
    pub config: BenchConfig,
    /// Measured lazily on the agent's first tool call (None until then) so the
    /// MCP `initialize` handshake isn't blocked by the slow baseline window.
    pub baseline: Option<WindowStats>,
    pub baseline_flow_samples: Vec<f64>,
    pub road_costs: HashMap<String, i64>,
    pub num_changes: u32,
    pub money_spent: i64,
    pub actions: Vec<ActionEntry>,
    pub flow: FlowWindow,
    pub start: Instant,
    pub end_reason: Option<EndReason>,
}

impl RunState {
    pub fn new(config: BenchConfig, road_costs: HashMap<String, i64>) -> Self {
        let window = config.window_samples as usize;
        Self {
            config,
            baseline: None,
            baseline_flow_samples: Vec::new(),
            road_costs,
            num_changes: 0,
            money_spent: 0,
            actions: Vec::new(),
            flow: FlowWindow::new(window),
            start: Instant::now(),
            end_reason: None,
        }
    }

    pub fn build_cost(&self, road_type: &str, length_m: f32) -> i64 {
        match self.road_costs.get(road_type) {
            Some(&c) => road_cost(c, length_m, &self.config),
            None => 0,
        }
    }

    pub fn record_mutation(&mut self, tool: &str, cost: i64) {
        self.num_changes += 1;
        self.money_spent += cost;
        self.actions.push(ActionEntry {
            seq: self.num_changes,
            tool: tool.to_string(),
            cost,
        });
    }

    pub fn push_flow(&mut self, sample: f64) {
        self.flow.push(sample);
        if self.flow.target_reached(self.config.flow_target) && self.end_reason.is_none() {
            self.end_reason = Some(EndReason::FlowTarget);
        }
    }

    pub fn seconds_remaining(&self) -> u64 {
        self.config
            .wall_clock_cap_secs
            .saturating_sub(self.start.elapsed().as_secs())
    }

    pub fn check_timeout(&mut self) {
        if self.seconds_remaining() == 0 && self.end_reason.is_none() {
            self.end_reason = Some(EndReason::Timeout);
        }
    }

    pub fn end_state(&self, map: MapInfo, started_at: String, ended_at: String) -> EndState {
        EndState {
            schema_version: 1,
            config: self.config.clone(),
            map,
            started_at,
            ended_at,
            end_reason: self.end_reason.unwrap_or(EndReason::Disconnect),
            baseline: self.baseline.clone(),
            baseline_flow_samples: self.baseline_flow_samples.clone(),
            tally: Tally { num_changes: self.num_changes, money_spent: self.money_spent },
            actions: self.actions.clone(),
        }
    }

    /// Agent-facing telemetry (spec §7): resources + goal, never the score.
    pub fn progress(&self) -> Value {
        json!({
            "money_spent": self.money_spent,
            "num_changes": self.num_changes,
            "flow_current": self.flow.mean(),
            "flow_target": self.config.flow_target,
            "seconds_remaining": self.seconds_remaining(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark::config::BenchConfig;
    use std::collections::HashMap;

    fn state() -> RunState {
        let mut costs = HashMap::new();
        costs.insert("road".to_string(), 1000i64);
        RunState::new(BenchConfig::default(), costs)
    }

    #[test]
    fn records_changes_and_cost() {
        let mut s = state();
        s.record_mutation("build_road", 12_000);
        s.record_mutation("set_zoning", 0);
        assert_eq!(s.num_changes, 2);
        assert_eq!(s.money_spent, 12_000);
        assert_eq!(s.actions.len(), 2);
        assert_eq!(s.actions[0].seq, 1);
    }

    #[test]
    fn progress_omits_score_fields() {
        let s = state();
        let p = s.progress();
        assert_eq!(p["flow_target"], 95.0);
        assert!(p["seconds_remaining"].as_u64().unwrap() <= 10_800);
        assert!(p.get("score").is_none());
        assert!(p.get("composite_score").is_none());
        assert!(p.get("weights").is_none());
    }

    #[test]
    fn end_state_snapshots_run_and_defaults_to_disconnect() {
        use crate::benchmark::record::{EndReason, MapInfo};

        let mut s = state();
        s.record_mutation("build_road", 12_000);
        let map = MapInfo { id: "m".into(), source: "test".into(), game_version: "v".into() };
        let e = s.end_state(map, "t0".into(), "t1".into());
        assert_eq!(e.end_reason, EndReason::Disconnect);
        assert_eq!(e.tally.num_changes, 1);
        assert_eq!(e.tally.money_spent, 12_000);
        assert_eq!(e.actions.len(), 1);
        assert!(e.baseline.is_none());

        s.end_reason = Some(EndReason::Submit);
        let map = MapInfo { id: "m".into(), source: "test".into(), game_version: "v".into() };
        let e = s.end_state(map, "t0".into(), "t1".into());
        assert_eq!(e.end_reason, EndReason::Submit);
    }

    #[test]
    fn road_cost_lookup_uses_table_and_config() {
        let s = state();
        assert_eq!(s.build_cost("road", 64.0), 1000);
        assert_eq!(s.build_cost("missing", 64.0), 0);
    }
}
