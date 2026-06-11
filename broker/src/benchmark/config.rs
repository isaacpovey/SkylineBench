use serde::{Deserialize, Serialize};

/// Benchmark scoring + protocol constants (spec 2026-06-10 §6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchConfig {
    pub w_congestion: f64,
    pub w_money: f64,
    pub w_changes: f64,
    /// Segment density (0..1) at or above which a segment counts as congested.
    pub congestion_threshold: f64,
    pub budget: f64,
    pub change_cap: f64,
    /// Early-success: the run ends when windowed congested meters fall to this
    /// fraction of the baseline.
    pub congestion_end_ratio: f64,
    pub window_ticks: u32,
    pub settle_ticks: u32,
    pub window_samples: u32,
    pub wall_clock_cap_secs: u64,
    /// Invalid run when final population < pop_guard_ratio × baseline. The
    /// null-control run (benchmark/experiments) showed a natural population
    /// floor of ~87% of peak, so 0.8 tolerates lifecycle waves.
    pub pop_guard_ratio: f64,
    /// Length (m) over which a road's `construction_cost` is charged once
    /// (cost = construction_cost · length / cost_base_length_m). Calibrated.
    pub cost_base_length_m: f64,
    /// Ticks in one in-game calendar day (the CS1 game week is 4096 sim
    /// frames, so a day is 4096/7 ≈ 585). `control_time` steps default to
    /// one day and are capped at `max_step_days`.
    pub day_ticks: u32,
    pub max_step_days: u32,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            w_congestion: 0.60,
            w_money: 0.20,
            w_changes: 0.20,
            congestion_threshold: 0.7,
            budget: 10_000_000.0,
            change_cap: 300.0,
            congestion_end_ratio: 0.05,
            window_ticks: 2048,
            settle_ticks: 8192,
            window_samples: 8,
            wall_clock_cap_secs: 10_800,
            pop_guard_ratio: 0.8,
            cost_base_length_m: 64.0,
            day_ticks: 585,
            max_step_days: 7,
        }
    }
}

impl BenchConfig {
    pub fn max_step_ticks(&self) -> u32 {
        self.day_ticks * self.max_step_days
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_weights_sum_to_one() {
        let c = BenchConfig::default();
        let sum = c.w_congestion + c.w_money + c.w_changes;
        assert!((sum - 1.0).abs() < 1e-9, "weights sum to {sum}");
    }

    #[test]
    fn congestion_dominates_and_guards_are_calibrated() {
        let c = BenchConfig::default();
        assert!(c.w_congestion > c.w_money && c.w_congestion > c.w_changes);
        assert_eq!(c.congestion_threshold, 0.7);
        assert_eq!(c.congestion_end_ratio, 0.05);
        assert_eq!(c.pop_guard_ratio, 0.8);
        assert_eq!(c.wall_clock_cap_secs, 10_800);
    }

    #[test]
    fn step_cap_is_seven_days() {
        let c = BenchConfig::default();
        assert_eq!(c.day_ticks, 585);
        assert_eq!(c.max_step_days, 7);
        assert_eq!(c.max_step_ticks(), 4095);
    }

    #[test]
    fn default_resource_envelope() {
        let c = BenchConfig::default();
        assert_eq!(c.budget, 10_000_000.0);
        assert_eq!(c.change_cap, 300.0);
    }
}
