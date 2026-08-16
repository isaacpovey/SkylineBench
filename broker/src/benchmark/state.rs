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

/// Leading-indicator counts of buildings reporting a connectivity/utility
/// problem (cut off from the road network or starved of a service). Surfaced in
/// city_status so the agent sees a stranded depot/plant immediately, rather
/// than only when abandonment catches up many in-game days later.
#[derive(Clone, Copy, Default)]
pub struct ServiceProblems {
    pub road_not_connected: u32,
    pub no_electricity: u32,
    pub no_water: u32,
    pub no_sewage: u32,
    pub garbage_piling: u32,
    pub no_fuel: u32,
}

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
    pub last_service_problems: Option<ServiceProblems>,
    pub topology: Option<Topology>,
    pub last_densities: HashMap<u32, f64>,
    pub start: Instant,
    pub end_reason: Option<EndReason>,
    pub render_seq: u32,
    pub overview_camera: Option<crate::service::CameraShot>,
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
            last_service_problems: None,
            topology: None,
            last_densities: HashMap::new(),
            start: Instant::now(),
            end_reason: None,
            render_seq: 0,
            overview_camera: None,
        }
    }

    /// Overview camera, locked to the first network it is asked about (the
    /// untouched baseline) and reused for every later frame. Recomputing it per
    /// frame let the chosen yaw flip 90° mid-run as the agent's edits shifted
    /// the city's aspect ratio, so the timelapse appeared to lose its rotation;
    /// locking it keeps one orientation and frame throughout the run.
    pub fn locked_overview_shot(
        &mut self,
        net: &crate::contract::Network,
    ) -> crate::service::CameraShot {
        *self
            .overview_camera
            .get_or_insert_with(|| crate::service::overview_shot(net))
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
        self.money_spent += cost;
        if Self::counts_toward_change_cap(tool) {
            self.num_changes += 1;
        }
        self.actions.push(ActionEntry {
            seq: self.actions.len() as u32 + 1,
            tool: tool.to_string(),
            cost,
        });
    }

    /// Only adding or upgrading road counts against the change budget. Bulldoze
    /// (demolition, including clearing abandoned-building blight) and re-zoning
    /// are still recorded for cost and audit, but do not consume the change cap —
    /// they are cleanup, not the network churn the cap is meant to discourage.
    fn counts_toward_change_cap(tool: &str) -> bool {
        matches!(tool, "build_road" | "upgrade_road")
    }

    pub fn observe_metrics(&mut self, m: &Metrics) {
        self.flow.push(m.traffic.flow_percent as f64);
        self.congestion.push(instant_congested_meters(
            &m.traffic.segment_loads,
            self.config.congestion_threshold,
        ));
        self.last_population = Some(m.population.total);
        self.last_abandoned_buildings = Some(m.services.abandoned_buildings);
        self.last_happiness = Some(m.services.happiness);
        self.last_service_problems = Some(ServiceProblems {
            road_not_connected: m.services.road_not_connected,
            no_electricity: m.services.no_electricity,
            no_water: m.services.no_water,
            no_sewage: m.services.no_sewage,
            garbage_piling: m.services.garbage_piling,
            no_fuel: m.services.no_fuel,
        });
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

    pub(crate) fn live_congested_junctions(&self) -> Option<u32> {
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
            tally: Tally {
                num_changes: self.num_changes,
                money_spent: self.money_spent,
            },
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
            "population": self.last_population,
            "abandoned_buildings": self.last_abandoned_buildings,
            "happiness": self.last_happiness,
            // Leading connectivity/utility signal: nonzero counts mean a recent
            // change cut buildings off from the road network or a service (a
            // limited-access highway over a connector strands the depots/plants
            // it served). Null until the first metrics read.
            "service_problems": self.last_service_problems.map(|p| json!({
                "road_not_connected": p.road_not_connected,
                "no_electricity": p.no_electricity,
                "no_water": p.no_water,
                "no_sewage": p.no_sewage,
                "garbage_piling": p.garbage_piling,
                "no_fuel": p.no_fuel,
            })),
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
                segment_loads: vec![SegmentLoad {
                    segment_id: 1,
                    density,
                    length: 100.0,
                }],
            },
            economy: EconomyMetrics {
                balance: 0,
                weekly_income: 0,
                weekly_expenses: 0,
                funds: 0,
            },
            population: PopulationMetrics {
                total: 1000,
                residential_demand: 0,
                commercial_demand: 0,
                workplace_demand: 0,
            },
            services: ServiceMetrics {
                happiness: 80,
                abandoned_buildings: 2,
                ..Default::default()
            },
        }
    }

    #[test]
    fn records_changes_and_cost() {
        let mut s = state();
        s.record_mutation("build_road", 12_000);
        s.record_mutation("upgrade_road", 3_000);
        s.record_mutation("bulldoze", 0);
        s.record_mutation("set_zoning", 0);
        // Only build_road + upgrade_road count toward the change cap; bulldoze and
        // set_zoning are recorded for cost/audit but don't consume it.
        assert_eq!(s.num_changes, 2);
        assert_eq!(s.money_spent, 15_000);
        assert_eq!(s.actions.len(), 4);
        assert_eq!(s.actions[0].seq, 1);
        assert_eq!(s.actions[3].seq, 4);
    }

    #[test]
    fn overview_camera_locks_to_baseline_and_ignores_later_reshaping() {
        use crate::contract::{NetNode, Network};
        let node = |id, x, z| NetNode { id, x, y: 0.0, z };
        // Wide baseline city → a fresh overview picks yaw 0 (north-up).
        let wide = Network {
            nodes: vec![node(0, -1000.0, -100.0), node(1, 1000.0, 100.0)],
            segments: vec![],
        };
        // After the agent's edits the network is tall → a fresh overview would
        // pick yaw 90, which is exactly the mid-run flip we want to prevent.
        let tall = Network {
            nodes: vec![node(0, -100.0, -1000.0), node(1, 100.0, 1000.0)],
            segments: vec![],
        };
        assert_ne!(
            crate::service::overview_shot(&wide).yaw,
            crate::service::overview_shot(&tall).yaw,
            "the two shapes pick different yaw when computed fresh"
        );

        let mut s = state();
        let locked = s.locked_overview_shot(&wide);
        let after = s.locked_overview_shot(&tall);
        assert_eq!(
            after.yaw, locked.yaw,
            "orientation stays put after reshaping"
        );
        assert_eq!(after.x, locked.x, "frame center stays put");
        assert_eq!(after.size, locked.size, "zoom stays put");
    }

    #[test]
    fn progress_omits_score_fields() {
        let mut s = state();
        let p = s.progress();
        assert!(p["congested_road_meters"].is_null(), "no samples yet");
        assert!(
            p["traffic_flow"].is_null(),
            "flow is never surfaced to the agent"
        );
        assert!(
            p["congested_road_meters_at_start"].is_null(),
            "no baseline yet"
        );
        assert!(p["time_remaining"].as_u64().unwrap() <= 10_800);
        assert!(p.get("score").is_none());
        assert!(p.get("composite_score").is_none());
        assert!(p.get("weights").is_none());
        assert!(
            p.get("congested_meters_target").is_none(),
            "scoring target must not leak"
        );
        assert!(p["happiness"].is_null(), "no happiness before first sample");

        // service_problems is null until the first metrics sample.
        assert!(
            p.get("service_problems").map_or(true, Value::is_null),
            "no service_problems before first sample"
        );

        s.observe_metrics(&sample_metrics(0.9));
        let p = s.progress();
        assert!(
            p["congested_road_meters"].is_number(),
            "current appears after first sample"
        );
        assert!(
            p["traffic_flow"].is_null(),
            "flow stays hidden from the agent even after sampling"
        );
        assert_eq!(
            p["happiness"], 80,
            "happiness surfaced from the latest sample"
        );
    }

    #[test]
    fn service_problems_surface_in_progress() {
        let mut s = state();
        let mut m = sample_metrics(0.5);
        m.services.road_not_connected = 7;
        m.services.garbage_piling = 12;
        m.services.no_fuel = 1;
        s.observe_metrics(&m);
        let p = s.progress();
        let sp = &p["service_problems"];
        assert_eq!(
            sp["road_not_connected"], 7,
            "stranded buildings must surface immediately"
        );
        assert_eq!(sp["garbage_piling"], 12);
        assert_eq!(sp["no_fuel"], 1, "power-plant fuel starvation must surface");
        assert_eq!(sp["no_electricity"], 0);
    }

    #[test]
    fn live_congested_junctions_flows_into_progress() {
        use crate::contract::{NetNode, NetSegment, Network};
        let mut s = state();
        // congested_junctions is null until topology has been observed.
        s.observe_metrics(&sample_metrics(0.9));
        assert!(
            s.progress()["congested_junctions"].is_null(),
            "null before topology observed"
        );

        let node = |id| NetNode {
            id,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let seg = |id, a, b| NetSegment {
            id,
            start_node: a,
            end_node: b,
            prefab: "road".into(),
            lanes: 2,
            length: 100.0,
            one_way: false,
            travel_direction: "both".into(),
            speed_limit: 1.0,
        };
        // Node 1 is a 3-way junction; segments 10 and 11 are congested, 12 is not.
        let net = Network {
            nodes: vec![node(1), node(3), node(4), node(5)],
            segments: vec![seg(10, 1, 3), seg(11, 1, 4), seg(12, 1, 5)],
        };
        s.observe_network(&net);

        use crate::contract::*;
        let m = Metrics {
            tick: 0,
            traffic: TrafficMetrics {
                flow_percent: 50.0,
                active_vehicles: 100,
                segment_loads: vec![
                    SegmentLoad {
                        segment_id: 10,
                        density: 0.9,
                        length: 100.0,
                    },
                    SegmentLoad {
                        segment_id: 11,
                        density: 0.9,
                        length: 100.0,
                    },
                    SegmentLoad {
                        segment_id: 12,
                        density: 0.2,
                        length: 100.0,
                    },
                ],
            },
            economy: EconomyMetrics {
                balance: 0,
                weekly_income: 0,
                weekly_expenses: 0,
                funds: 0,
            },
            population: PopulationMetrics {
                total: 1000,
                residential_demand: 0,
                commercial_demand: 0,
                workplace_demand: 0,
            },
            services: ServiceMetrics {
                happiness: 80,
                abandoned_buildings: 0,
                ..Default::default()
            },
        };
        s.observe_metrics(&m);
        assert_eq!(
            s.progress()["congested_junctions"],
            1,
            "node 1 has 2 congested approaches"
        );
    }

    #[test]
    fn end_state_snapshots_run_and_defaults_to_disconnect() {
        use crate::benchmark::record::{EndReason, MapInfo};

        let mut s = state();
        s.record_mutation("build_road", 12_000);
        let map = MapInfo {
            id: "m".into(),
            source: "test".into(),
            game_version: "v".into(),
        };
        let e = s.end_state(map, "t0".into(), "t1".into());
        assert_eq!(e.end_reason, EndReason::Disconnect);
        assert_eq!(e.tally.num_changes, 1);
        assert_eq!(e.tally.money_spent, 12_000);
        assert_eq!(e.actions.len(), 1);
        assert!(e.baseline.is_none());

        s.end_reason = Some(EndReason::Submit);
        let map = MapInfo {
            id: "m".into(),
            source: "test".into(),
            game_version: "v".into(),
        };
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
