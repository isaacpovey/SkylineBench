use crate::benchmark::config::BenchConfig;

/// Cost charged for building `length_m` of a road whose NetInfo
/// `construction_cost` is `construction_cost` (spec §8). Computed from the
/// action, never from in-game funds (money is unlimited in benchmark mode).
pub fn road_cost(construction_cost: i64, length_m: f32, cfg: &BenchConfig) -> i64 {
    let raw = construction_cost as f64 * (length_m as f64) / cfg.cost_base_length_m;
    raw.round().max(0.0) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark::config::BenchConfig;

    #[test]
    fn cost_scales_with_length() {
        let cfg = BenchConfig::default(); // cost_base_length_m = 64
        assert_eq!(road_cost(1000, 64.0, &cfg), 1000);
        assert_eq!(road_cost(1000, 128.0, &cfg), 2000);
    }

    #[test]
    fn cost_rounds_to_nearest_and_is_non_negative() {
        let cfg = BenchConfig::default();
        assert_eq!(road_cost(1000, 32.0, &cfg), 500);
        assert_eq!(road_cost(0, 100.0, &cfg), 0);
        assert_eq!(road_cost(1000, 0.0, &cfg), 0);
    }
}
