use std::collections::HashMap;
use std::time::Instant;

use serde_json::{json, Value};

use crate::benchmark::config::BenchConfig;
use crate::benchmark::congestion::{congested_junctions, instant_congested_meters, Topology};
use crate::benchmark::cost::road_cost;
use crate::benchmark::record::{
    ActionEntry, EndReason, EndState, MapInfo, Tally, WindowStats, SCHEMA_VERSION,
};
use crate::benchmark::rolling_window::RollingWindow;
use crate::contract::{Metrics, Network};

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
    pub last_happiness: Option<u8>,
    pub topology: Option<Topology>,
    pub last_densities: HashMap<u32, f64>,
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
            last_happiness: None,
            topology: None,
            last_densities: HashMap::new(),
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

    /// Install the baseline and restart telemetry windows so the end condition
    /// is evaluated only on post-baseline samples.
    pub fn set_baseline(&mut self, stats: WindowStats, samples: Vec<f64>) {
        self.baseline = Some(stats);
        self.baseline_flow_samples = samples;
        self.congestion = RollingWindow::new(self.config.window_samples as usize);
        self.flow = RollingWindow::new(self.config.window_samples as usize);
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

    pub fn observe_metrics(&mut self, m: &Metrics) {
        self.flow.push(m.traffic.flow_percent as f64);
        self.congestion
            .push(instant_congested_meters(&m.traffic.segment_loads, self.config.congestion_threshold));
        self.last_population = Some(m.population.total);
        self.last_abandoned_buildings = Some(m.services.abandoned_buildings);
        self.last_happiness = Some(m.services.happiness);
        self.last_densities = m
            .traffic
            .segment_loads
            .iter()
            .map(|l| (l.segment_id, f64::from(l.density)))
            .collect();
    }

    /// Cache the road graph so the live readout can count congested junctions.
    pub fn observe_network(&mut self, net: &Network) {
        self.topology = Some(Topology::from_network(net));
    }

    fn live_congested_junctions(&self) -> Option<u32> {
        self.topology.as_ref().map(|t| {
            congested_junctions(
                t,
                |id| self.last_densities.get(&id).copied(),
                self.config.congestion_threshold,
                self.config.junction_min_degree as usize,
                self.config.junction_min_congested as usize,
            )
        })
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

    /// Neutral simulation readout merged into every tool response — no scoring
    /// formula, weights, caps, or thresholds, just observable city facts.
    pub fn progress(&self) -> Value {
        json!({
            "money_spent": self.money_spent,
            "changes_made": self.num_changes,
            "congested_road_meters": (!self.congestion.is_empty()).then(|| self.congestion.mean()),
            "congested_road_meters_at_start": self.baseline.as_ref().map(|b| b.congested_meters),
            "congested_junctions": self.live_congested_junctions(),
            "congested_junctions_at_start": self.baseline.as_ref().map(|b| b.congested_junctions),
            "traffic_flow": (!self.flow.is_empty()).then(|| self.flow.mean()),
            "population": self.last_population,
            "abandoned_buildings": self.last_abandoned_buildings,
            "happiness": self.last_happiness,
            "time_remaining": self.seconds_remaining(),
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

    fn sample_metrics(density: f32) -> crate::contract::Metrics {
        use crate::contract::*;
        Metrics {
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
        }
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
        let mut s = state();
        let p = s.progress();
        assert!(p["congested_road_meters"].is_null(), "no samples yet");
        assert!(p["traffic_flow"].is_null(), "no samples yet");
        assert!(p["congested_road_meters_at_start"].is_null(), "no baseline yet");
        assert!(p["time_remaining"].as_u64().unwrap() <= 10_800);
        assert!(p.get("score").is_none());
        assert!(p.get("composite_score").is_none());
        assert!(p.get("weights").is_none());
        assert!(p.get("congested_meters_target").is_none(), "scoring target must not leak");
        assert!(p["happiness"].is_null(), "no happiness before first sample");

        s.observe_metrics(&sample_metrics(0.9));
        let p = s.progress();
        assert!(p["congested_road_meters"].is_number(), "current appears after first sample");
        assert!(p["traffic_flow"].is_number());
        assert_eq!(p["happiness"], 80, "happiness surfaced from the latest sample");
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
