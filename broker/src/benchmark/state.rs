use std::collections::HashMap;
use std::time::Instant;

use serde_json::{json, Value};

use crate::benchmark::config::BenchConfig;
use crate::benchmark::congestion::instant_congested_meters;
use crate::benchmark::cost::road_cost;
use crate::benchmark::record::{
    ActionEntry, EndReason, EndState, MapInfo, Tally, WindowStats, SCHEMA_VERSION,
};
use crate::benchmark::rolling_window::RollingWindow;
use crate::contract::Metrics;

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
    pub flow: RollingWindow,
    pub congestion: RollingWindow,
    pub last_population: Option<u32>,
    pub last_abandoned_buildings: Option<u32>,
    pub start: Instant,
    pub end_reason: Option<EndReason>,
    pub render_seq: u32,
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
            flow: RollingWindow::new(window),
            congestion: RollingWindow::new(window),
            last_population: None,
            last_abandoned_buildings: None,
            start: Instant::now(),
            end_reason: None,
            render_seq: 0,
        }
    }

    pub fn next_render_seq(&mut self) -> u32 {
        self.render_seq += 1;
        self.render_seq
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

    /// Fold one metrics sample into the run telemetry. The congestion end
    /// condition needs the baseline (for the ratio), so it only arms once the
    /// baseline window has been measured.
    pub fn observe_metrics(&mut self, m: &Metrics) {
        self.flow.push(m.traffic.flow_percent as f64);
        self.congestion
            .push(instant_congested_meters(&m.traffic.segment_loads, self.config.congestion_threshold));
        self.last_population = Some(m.population.total);
        self.last_abandoned_buildings = Some(m.services.abandoned_buildings);
        let baseline_cm = self.baseline.as_ref().map(|b| b.congested_meters);
        if let Some(base) = baseline_cm {
            if base > 0.0
                && self.congestion.is_full()
                && self.congestion.mean() <= self.config.congestion_end_ratio * base
                && self.end_reason.is_none()
            {
                self.end_reason = Some(EndReason::CongestionTarget);
            }
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
            schema_version: SCHEMA_VERSION,
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

    /// Agent-facing telemetry (spec §7 + 2026-06-10 §3): resources, the scored
    /// congestion signal, and neutral city-health facts. Never the score.
    pub fn progress(&self) -> Value {
        let baseline_cm = self.baseline.as_ref().map(|b| b.congested_meters);
        json!({
            "money_spent": self.money_spent,
            "num_changes": self.num_changes,
            "congested_meters_current": self.congestion.mean(),
            "congested_meters_baseline": baseline_cm,
            "congested_meters_target": baseline_cm.map(|b| self.config.congestion_end_ratio * b),
            "flow_current": self.flow.mean(),
            "population": self.last_population,
            "abandoned_buildings": self.last_abandoned_buildings,
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
        assert!(p["congested_meters_current"].is_number());
        assert!(p["congested_meters_baseline"].is_null(), "no baseline yet");
        assert!(p["seconds_remaining"].as_u64().unwrap() <= 10_800);
        assert!(p.get("score").is_none());
        assert!(p.get("composite_score").is_none());
        assert!(p.get("weights").is_none());
    }

    #[test]
    fn congestion_end_condition_arms_only_with_baseline() {
        use crate::benchmark::record::{EndReason, WindowStats};
        use crate::contract::*;

        let metrics = |density: f32| Metrics {
            tick: 0,
            traffic: TrafficMetrics {
                flow_percent: 50.0,
                active_vehicles: 100,
                segment_loads: vec![SegmentLoad { segment_id: 1, density, length: 100.0 }],
            },
            economy: EconomyMetrics { balance: 0, weekly_income: 0, weekly_expenses: 0, funds: 0 },
            population: PopulationMetrics {
                total: 1000,
                residential_demand: 0,
                commercial_demand: 0,
                workplace_demand: 0,
                employed: 0,
            },
            services: ServiceMetrics { happiness: 80, abandoned_buildings: 2 },
        };

        let mut s = state();
        // Without a baseline, even a fully clear window must not end the run.
        for _ in 0..10 {
            s.observe_metrics(&metrics(0.0));
        }
        assert_eq!(s.end_reason, None);

        s.baseline = Some(WindowStats {
            flow_mean: 50.0,
            active_vehicles_mean: 100.0,
            population: 1000,
            congested_meters: 100.0,
        });
        for _ in 0..10 {
            s.observe_metrics(&metrics(0.0)); // 0 ≤ 0.05 × 100
        }
        assert_eq!(s.end_reason, Some(EndReason::CongestionTarget));
        assert_eq!(s.progress()["population"], 1000);
        assert_eq!(s.progress()["abandoned_buildings"], 2);
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
