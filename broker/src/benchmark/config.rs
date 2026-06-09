use serde::{Deserialize, Serialize};

/// Benchmark scoring + protocol constants (spec §6). Values are calibration
/// placeholders tuned against the chosen map during the verify step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchConfig {
    pub w_flow: f64,
    pub w_money: f64,
    pub w_changes: f64,
    pub target_gain: f64,
    pub budget: f64,
    pub change_cap: f64,
    pub flow_target: f64,
    pub window_ticks: u32,
    pub settle_ticks: u32,
    pub window_samples: u32,
    pub wall_clock_cap_secs: u64,
    pub guard_ratio: f64,
    /// Length (m) over which a road's `construction_cost` is charged once
    /// (cost = construction_cost · length / cost_base_length_m). Calibrated.
    pub cost_base_length_m: f64,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            w_flow: 0.60,
            w_money: 0.20,
            w_changes: 0.20,
            target_gain: 40.0,
            budget: 5_000_000.0,
            change_cap: 100.0,
            flow_target: 95.0,
            window_ticks: 2048,
            settle_ticks: 8192,
            window_samples: 8,
            wall_clock_cap_secs: 10_800,
            guard_ratio: 0.9,
            cost_base_length_m: 64.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_weights_sum_to_one() {
        let c = BenchConfig::default();
        let sum = c.w_flow + c.w_money + c.w_changes;
        assert!((sum - 1.0).abs() < 1e-9, "weights sum to {sum}");
    }

    #[test]
    fn default_flow_dominant() {
        let c = BenchConfig::default();
        assert!(c.w_flow > c.w_money && c.w_flow > c.w_changes);
        assert_eq!(c.flow_target, 95.0);
        assert_eq!(c.wall_clock_cap_secs, 10_800);
        assert_eq!(c.guard_ratio, 0.9);
    }
}
