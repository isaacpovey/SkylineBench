use crate::benchmark::config::BenchConfig;
use crate::benchmark::record::{RunRecord, Score, ScoreNorms};

fn clamp01(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

pub fn score_run(record: &RunRecord, cfg: &BenchConfig) -> Score {
    debug_assert!(
        cfg.target_gain > 0.0 && cfg.budget > 0.0 && cfg.change_cap > 0.0,
        "BenchConfig normalization denominators must be positive"
    );
    let delta_flow = record.final_stats.flow_mean - record.baseline.flow_mean;
    let norm = ScoreNorms {
        flow: clamp01(delta_flow / cfg.target_gain),
        money: clamp01(record.tally.money_spent as f64 / cfg.budget),
        changes: clamp01(record.tally.num_changes as f64 / cfg.change_cap),
    };
    let weighted = ScoreNorms {
        flow: cfg.w_flow * norm.flow,
        money: cfg.w_money * (1.0 - norm.money),
        changes: cfg.w_changes * (1.0 - norm.changes),
    };
    let invalid = record.final_stats.active_vehicles_mean
        < cfg.guard_ratio * record.baseline.active_vehicles_mean;
    let score = if invalid {
        0.0
    } else {
        weighted.flow + weighted.money + weighted.changes
    };
    Score { norm, weighted, invalid, score }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark::config::BenchConfig;
    use crate::benchmark::record::*;

    fn record(flow_gain: f64, money: i64, changes: u32, veh_base: f64, veh_final: f64) -> RunRecord {
        RunRecord {
            schema_version: 1,
            config: BenchConfig::default(),
            map: MapInfo { id: "m".into(), source: "s".into(), game_version: "v".into() },
            started_at: "a".into(),
            ended_at: "b".into(),
            end_reason: EndReason::Submit,
            baseline: WindowStats { flow_mean: 10.0, active_vehicles_mean: veh_base, population: 100 },
            final_stats: WindowStats { flow_mean: 10.0 + flow_gain, active_vehicles_mean: veh_final, population: 100 },
            flow_samples: FlowSamples { baseline: vec![], final_samples: vec![] },
            tally: Tally { num_changes: changes, money_spent: money },
            actions: vec![],
        }
    }

    #[test]
    fn perfect_cheap_run_scores_high() {
        let cfg = BenchConfig::default();
        let s = score_run(&record(40.0, 0, 0, 240.0, 240.0), &cfg);
        assert!(!s.invalid);
        assert!((s.score - 1.0).abs() < 1e-9, "score = {}", s.score);
    }

    #[test]
    fn flow_dominates() {
        let cfg = BenchConfig::default();
        let s = score_run(&record(0.0, 0, 0, 240.0, 240.0), &cfg);
        assert!((s.score - 0.40).abs() < 1e-9, "score = {}", s.score);
    }

    #[test]
    fn norms_clamp_to_unit() {
        let cfg = BenchConfig::default();
        let s = score_run(&record(1000.0, 50_000_000, 1000, 240.0, 240.0), &cfg);
        assert_eq!(s.norm.flow, 1.0);
        assert_eq!(s.norm.money, 1.0);
        assert_eq!(s.norm.changes, 1.0);
        assert!((s.score - 0.60).abs() < 1e-9, "score = {}", s.score);
    }

    #[test]
    fn throughput_guard_zeroes_invalid_run() {
        let cfg = BenchConfig::default();
        let s = score_run(&record(40.0, 0, 0, 240.0, 200.0), &cfg);
        assert!(s.invalid);
        assert_eq!(s.score, 0.0);
    }

    #[test]
    fn guard_passes_at_exactly_the_ratio() {
        let cfg = BenchConfig::default();
        let s = score_run(&record(40.0, 0, 0, 240.0, 216.0), &cfg); // 216 == 0.9 * 240
        assert!(!s.invalid);
    }
}
