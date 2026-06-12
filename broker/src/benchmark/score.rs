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
    let baseline_j = record.baseline.congested_junctions;
    let final_j = record.final_stats.congested_junctions;

    let meters_reduction =
        if baseline_cm > 0.0 { clamp01((baseline_cm - final_cm) / baseline_cm) } else { 0.0 };
    let junction_reduction = if baseline_j > 0 {
        clamp01((f64::from(baseline_j) - f64::from(final_j)) / f64::from(baseline_j))
    } else {
        0.0
    };
    // No baseline junctions => the junction signal is meaningless; weight the
    // whole congestion term onto meters.
    let (a, b) = if baseline_j > 0 { (cfg.blend_meters, cfg.blend_junctions) } else { (1.0, 0.0) };
    let congestion_reward = a * meters_reduction + b * junction_reduction;

    let pop_ratio = if record.baseline.population > 0 {
        f64::from(record.final_stats.population) / f64::from(record.baseline.population)
    } else {
        1.0
    };
    let health = clamp01((pop_ratio - cfg.health_zero) / (cfg.health_full - cfg.health_zero));

    let norm = ScoreNorms {
        congestion: congestion_reward,
        money: clamp01(record.tally.money_spent as f64 / cfg.budget),
        changes: clamp01(record.tally.num_changes as f64 / cfg.change_cap),
    };
    let weighted = ScoreNorms {
        congestion: cfg.w_congestion * norm.congestion,
        money: cfg.w_money * (1.0 - norm.money),
        changes: cfg.w_changes * (1.0 - norm.changes),
    };
    // A map with no congestion to fix is unscorable. Total population collapse
    // now zeroes the score smoothly through `health`, not a hard cliff.
    let invalid = baseline_cm <= 0.0;
    let score = if invalid {
        0.0
    } else {
        (weighted.congestion + weighted.money + weighted.changes) * health
    };
    Score {
        norm,
        weighted,
        invalid,
        flow_gain: record.final_stats.flow_mean - record.baseline.flow_mean,
        meters_reduction,
        junction_reduction,
        health,
        score,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark::config::BenchConfig;
    use crate::benchmark::record::*;

    fn record(baseline_cm: f64, final_cm: f64, money: i64, changes: u32, pop_base: u32, pop_final: u32) -> RunRecord {
        record_j(baseline_cm, final_cm, 0, 0, money, changes, pop_base, pop_final)
    }

    #[allow(clippy::too_many_arguments)]
    fn record_j(
        baseline_cm: f64, final_cm: f64, baseline_junctions: u32, final_junctions: u32,
        money: i64, changes: u32, pop_base: u32, pop_final: u32,
    ) -> RunRecord {
        RunRecord {
            schema_version: SCHEMA_VERSION,
            config: BenchConfig::default(),
            map: MapInfo { id: "m".into(), source: "s".into(), game_version: "v".into() },
            started_at: "a".into(),
            ended_at: "b".into(),
            end_reason: EndReason::Submit,
            baseline: WindowStats { flow_mean: 55.0, active_vehicles_mean: 2000.0, population: pop_base, congested_meters: baseline_cm, congested_junctions: baseline_junctions },
            final_stats: WindowStats { flow_mean: 60.0, active_vehicles_mean: 2000.0, population: pop_final, congested_meters: final_cm, congested_junctions: final_junctions },
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
    fn total_population_collapse_drives_health_and_score_to_zero() {
        let s = score_run(&record(1000.0, 0.0, 0, 0, 30_000, 110), &BenchConfig::default());
        assert!((s.health - 0.0).abs() < 1e-9);
        assert!((s.score - 0.0).abs() < 1e-9);
    }

    #[test]
    fn stable_population_has_full_health() {
        let s = score_run(&record(1000.0, 0.0, 0, 0, 30_000, 30_000), &BenchConfig::default());
        assert!((s.health - 1.0).abs() < 1e-9);
    }

    #[test]
    fn depopulation_for_a_small_congestion_gain_nets_below_do_nothing() {
        let depop = score_run(&record_j(1000.0, 800.0, 10, 9, 0, 20, 30_000, 25_200), &BenchConfig::default());
        let do_nothing = score_run(&record(1000.0, 1000.0, 0, 0, 30_000, 30_000), &BenchConfig::default());
        assert!((do_nothing.score - 0.40).abs() < 1e-9, "do-nothing = {}", do_nothing.score);
        assert!(depop.score < do_nothing.score, "depop {} must be < do-nothing {}", depop.score, do_nothing.score);
    }

    #[test]
    fn congestion_term_blends_meters_and_junctions_5050() {
        let s = score_run(&record_j(1000.0, 600.0, 10, 2, 0, 0, 30_000, 30_000), &BenchConfig::default());
        assert!((s.meters_reduction - 0.4).abs() < 1e-9);
        assert!((s.junction_reduction - 0.8).abs() < 1e-9);
        assert!((s.norm.congestion - 0.6).abs() < 1e-9, "blended = {}", s.norm.congestion);
        assert!((s.score - 0.76).abs() < 1e-9, "score = {}", s.score);
    }

    #[test]
    fn zero_baseline_junctions_falls_back_to_meters_only() {
        let s = score_run(&record_j(1000.0, 500.0, 0, 0, 0, 0, 30_000, 30_000), &BenchConfig::default());
        assert!((s.norm.congestion - 0.5).abs() < 1e-9, "meters-only = {}", s.norm.congestion);
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
