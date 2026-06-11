use crate::benchmark::config::BenchConfig;
use crate::benchmark::record::{RunRecord, Score, ScoreNorms};

fn clamp01(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

pub fn score_run(record: &RunRecord, cfg: &BenchConfig) -> Score {
    debug_assert!(
        cfg.budget > 0.0 && cfg.change_cap > 0.0,
        "BenchConfig normalization denominators must be positive"
    );
    let baseline_cm = record.baseline.congested_meters;
    let final_cm = record.final_stats.congested_meters;
    let norm = ScoreNorms {
        congestion: if baseline_cm > 0.0 {
            clamp01((baseline_cm - final_cm) / baseline_cm)
        } else {
            0.0
        },
        money: clamp01(record.tally.money_spent as f64 / cfg.budget),
        changes: clamp01(record.tally.num_changes as f64 / cfg.change_cap),
    };
    let weighted = ScoreNorms {
        congestion: cfg.w_congestion * norm.congestion,
        money: cfg.w_money * (1.0 - norm.money),
        changes: cfg.w_changes * (1.0 - norm.changes),
    };
    // Two ways to be invalid: depopulating the city (the anti-cheat guard) or
    // a map with no congestion to fix (scores would be meaningless).
    let pop_guard_failed = (record.final_stats.population as f64)
        < cfg.pop_guard_ratio * record.baseline.population as f64;
    let invalid = pop_guard_failed || baseline_cm <= 0.0;
    let score = if invalid {
        0.0
    } else {
        weighted.congestion + weighted.money + weighted.changes
    };
    Score {
        norm,
        weighted,
        invalid,
        flow_gain: record.final_stats.flow_mean - record.baseline.flow_mean,
        score,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark::config::BenchConfig;
    use crate::benchmark::record::*;

    fn record(
        baseline_cm: f64,
        final_cm: f64,
        money: i64,
        changes: u32,
        pop_base: u32,
        pop_final: u32,
    ) -> RunRecord {
        RunRecord {
            schema_version: SCHEMA_VERSION,
            config: BenchConfig::default(),
            map: MapInfo { id: "m".into(), source: "s".into(), game_version: "v".into() },
            started_at: "a".into(),
            ended_at: "b".into(),
            end_reason: EndReason::Submit,
            baseline: WindowStats {
                flow_mean: 55.0,
                active_vehicles_mean: 2000.0,
                population: pop_base,
                congested_meters: baseline_cm,
            },
            final_stats: WindowStats {
                flow_mean: 60.0,
                active_vehicles_mean: 2000.0,
                population: pop_final,
                congested_meters: final_cm,
            },
            flow_samples: FlowSamples { baseline: vec![], final_samples: vec![] },
            tally: Tally { num_changes: changes, money_spent: money },
            actions: vec![],
        }
    }

    #[test]
    fn clearing_all_congestion_cheaply_scores_one() {
        let s = score_run(&record(1000.0, 0.0, 0, 0, 30_000, 30_000), &BenchConfig::default());
        assert!(!s.invalid);
        assert!((s.score - 1.0).abs() < 1e-9, "score = {}", s.score);
    }

    #[test]
    fn unchanged_congestion_scores_only_resource_components() {
        let s = score_run(&record(1000.0, 1000.0, 0, 0, 30_000, 30_000), &BenchConfig::default());
        assert!((s.score - 0.40).abs() < 1e-9, "score = {}", s.score);
    }

    #[test]
    fn worsened_congestion_clamps_to_zero_not_negative() {
        let s = score_run(&record(1000.0, 2000.0, 0, 0, 30_000, 30_000), &BenchConfig::default());
        assert_eq!(s.norm.congestion, 0.0);
    }

    #[test]
    fn population_guard_zeroes_collapsed_runs() {
        let s = score_run(&record(1000.0, 0.0, 0, 0, 30_000, 110), &BenchConfig::default());
        assert!(s.invalid);
        assert_eq!(s.score, 0.0);
    }

    #[test]
    fn population_guard_passes_at_exactly_the_ratio() {
        let s = score_run(&record(1000.0, 0.0, 0, 0, 30_000, 24_000), &BenchConfig::default());
        assert!(!s.invalid, "24_000 == 0.8 * 30_000 must pass");
    }

    #[test]
    fn zero_baseline_congestion_is_invalid() {
        let s = score_run(&record(0.0, 0.0, 0, 0, 30_000, 30_000), &BenchConfig::default());
        assert!(s.invalid);
        assert_eq!(s.score, 0.0);
    }

    #[test]
    fn flow_gain_is_reported_as_diagnostic() {
        let s = score_run(&record(1000.0, 500.0, 0, 0, 30_000, 30_000), &BenchConfig::default());
        assert!((s.flow_gain - 5.0).abs() < 1e-9);
    }

    #[test]
    fn resource_norms_clamp_to_unit() {
        let s = score_run(
            &record(1000.0, 0.0, 50_000_000, 1000, 30_000, 30_000),
            &BenchConfig::default(),
        );
        assert_eq!(s.norm.money, 1.0);
        assert_eq!(s.norm.changes, 1.0);
        assert!((s.score - 0.60).abs() < 1e-9, "score = {}", s.score);
    }
}
