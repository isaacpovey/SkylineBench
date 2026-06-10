use std::path::Path;

use crate::benchmark::config::BenchConfig;
use crate::benchmark::record::{EndState, FlowSamples, RunRecord, WindowStats};
use crate::benchmark::score::score_run;
use crate::bridge_client::{BridgeClient, BridgeError};

pub struct WindowMeasurement {
    pub stats: WindowStats,
    pub samples: Vec<f64>,
}

/// Step the sim across `window_ticks` in `window_samples` chunks, sampling
/// metrics after each chunk, and return the means (spec §3 baseline/final).
/// Leaves the sim paused (the mod re-pauses after a stepped `clock` op).
pub async fn measure_window(
    client: &BridgeClient,
    cfg: &BenchConfig,
) -> Result<WindowMeasurement, BridgeError> {
    let n_samples = cfg.window_samples.max(1);
    let chunk = (cfg.window_ticks / n_samples).max(1);
    // Best-effort: request max sim speed (the mod clamps to 1..3) to shorten
    // window wall-clock. For a compute-bound (large, gridlocked) city the per-
    // tick cost dominates and this may not help; the generous MCP timeouts set
    // by run.sh are what actually keep the long baseline/settle windows from
    // being killed. Speed changes how fast simulated time passes, not the
    // steady-state flow %.
    client.clock("set-speed", None, Some(3)).await?;
    client.clock("pause", None, None).await?;

    let mut flow_sum = 0.0_f64;
    let mut veh_sum = 0.0_f64;
    let mut last_pop = 0u32;
    let mut samples = Vec::with_capacity(n_samples as usize);
    for _ in 0..n_samples {
        client.clock("step", Some(chunk), None).await?;
        let m = client.metrics().await?;
        let flow = m.traffic.flow_percent as f64;
        flow_sum += flow;
        veh_sum += m.traffic.active_vehicles as f64;
        last_pop = m.population.total;
        samples.push(flow);
    }
    let n = n_samples as f64;
    Ok(WindowMeasurement {
        stats: WindowStats {
            flow_mean: flow_sum / n,
            active_vehicles_mean: veh_sum / n,
            population: last_pop,
        },
        samples,
    })
}

/// Settle the sim, measure the final window, compute the score, and write
/// `run-record.json` + `score.json` into `out_dir` (spec §3 steps 6–8, §10).
/// Runs from an `EndState` snapshot so it can execute in a separate process
/// after the agent session (and the MCP server) has exited.
pub async fn finalize(client: &BridgeClient, end: EndState, out_dir: &Path) -> anyhow::Result<()> {
    let cfg = end.config.clone();

    // Baseline is normally captured on the agent's first tool call. If the run
    // ended with no tool calls at all, fall back to measuring it now (the city
    // is then still untouched, so it's a valid baseline).
    let (baseline, baseline_flow_samples) = match end.baseline {
        Some(b) => (b, end.baseline_flow_samples),
        None => {
            let m = measure_window(client, &cfg).await?;
            (m.stats, m.samples)
        }
    };

    let settle_cfg = BenchConfig { window_ticks: cfg.settle_ticks, window_samples: 1, ..cfg.clone() };
    let _ = measure_window(client, &settle_cfg).await?;
    let final_m = measure_window(client, &cfg).await?;

    let record = RunRecord {
        schema_version: 1,
        config: cfg.clone(),
        map: end.map,
        started_at: end.started_at,
        ended_at: end.ended_at,
        end_reason: end.end_reason,
        baseline,
        final_stats: final_m.stats,
        flow_samples: FlowSamples { baseline: baseline_flow_samples, final_samples: final_m.samples },
        tally: end.tally,
        actions: end.actions,
    };
    let score = score_run(&record, &cfg);

    // Blocking I/O is acceptable here — finalize runs once at end of run.
    std::fs::create_dir_all(out_dir)?;
    std::fs::write(out_dir.join("run-record.json"), serde_json::to_string_pretty(&record)?)?;
    std::fs::write(out_dir.join("score.json"), serde_json::to_string_pretty(&score)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark::config::BenchConfig;
    use crate::bridge_client::BridgeClient;
    use crate::mock;

    async fn client() -> BridgeClient {
        let (addr, server) = mock::bind("127.0.0.1:0".parse().unwrap()).await;
        tokio::spawn(server);
        BridgeClient::new(format!("http://{addr}"))
    }

    #[tokio::test]
    async fn measures_window_stats_on_empty_city() {
        let c = client().await;
        let cfg = BenchConfig::default();
        let m = measure_window(&c, &cfg).await.unwrap();
        assert_eq!(m.stats.flow_mean, 100.0);
        assert_eq!(m.stats.active_vehicles_mean, 0.0);
        assert_eq!(m.samples.len(), cfg.window_samples as usize);
    }

    #[tokio::test]
    async fn finalize_writes_record_and_score_from_end_state() {
        use crate::benchmark::record::{ActionEntry, EndReason, EndState, MapInfo, Tally, WindowStats};

        let c = client().await;
        let end = EndState {
            schema_version: 1,
            config: BenchConfig::default(),
            map: MapInfo { id: "gridlock-v1".into(), source: "test".into(), game_version: "1.21.1-f9".into() },
            started_at: "t0".into(),
            ended_at: "t1".into(),
            end_reason: EndReason::Submit,
            baseline: Some(WindowStats { flow_mean: 80.0, active_vehicles_mean: 0.0, population: 0 }),
            baseline_flow_samples: vec![80.0],
            tally: Tally { num_changes: 2, money_spent: 5_000 },
            actions: vec![ActionEntry { seq: 1, tool: "build_road".into(), cost: 5_000 }],
        };

        let dir = std::env::temp_dir().join(format!("sb-finalize-{}", std::process::id()));
        finalize(&c, end, &dir).await.unwrap();

        let rec: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("run-record.json")).unwrap()).unwrap();
        assert_eq!(rec["end_reason"], "submit");
        assert_eq!(rec["started_at"], "t0");
        assert_eq!(rec["ended_at"], "t1");
        assert_eq!(rec["tally"]["num_changes"], 2);
        let score: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("score.json")).unwrap()).unwrap();
        assert!(score["score"].is_number());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn finalize_measures_missing_baseline() {
        use crate::benchmark::record::{EndReason, EndState, MapInfo, Tally};

        let c = client().await;
        let end = EndState {
            schema_version: 1,
            config: BenchConfig::default(),
            map: MapInfo { id: "m".into(), source: "test".into(), game_version: "v".into() },
            started_at: "t0".into(),
            ended_at: "t1".into(),
            end_reason: EndReason::Disconnect,
            baseline: None,
            baseline_flow_samples: vec![],
            tally: Tally { num_changes: 0, money_spent: 0 },
            actions: vec![],
        };

        let dir = std::env::temp_dir().join(format!("sb-finalize-nb-{}", std::process::id()));
        finalize(&c, end, &dir).await.unwrap();

        let rec: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("run-record.json")).unwrap()).unwrap();
        // Mock city flow is 100; the measured fallback baseline must be present.
        assert_eq!(rec["baseline"]["flow_mean"], 100.0);
        assert_eq!(rec["end_reason"], "disconnect");
        std::fs::remove_dir_all(&dir).ok();
    }
}
