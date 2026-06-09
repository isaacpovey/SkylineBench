use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::benchmark::config::BenchConfig;
use crate::benchmark::record::{EndReason, MapInfo, RunRecord, Tally, WindowStats};
use crate::benchmark::score::score_run;
use crate::benchmark::state::RunState;
use crate::bridge_client::{BridgeClient, BridgeError};

/// Step the sim across `window_ticks` in `window_samples` chunks, sampling
/// metrics after each chunk, and return the means (spec §3 baseline/final).
/// Leaves the sim paused (the mod re-pauses after a stepped `clock` op).
pub async fn measure_window(
    client: &BridgeClient,
    cfg: &BenchConfig,
) -> Result<WindowStats, BridgeError> {
    let samples = cfg.window_samples.max(1);
    let chunk = (cfg.window_ticks / samples).max(1);
    client.clock("pause", None, None).await?;

    let mut flow_sum = 0.0_f64;
    let mut veh_sum = 0.0_f64;
    let mut last_pop = 0u32;
    for _ in 0..samples {
        client.clock("step", Some(chunk), None).await?;
        let m = client.metrics().await?;
        flow_sum += m.traffic.flow_percent as f64;
        veh_sum += m.traffic.active_vehicles as f64;
        last_pop = m.population.total;
    }
    let n = samples as f64;
    Ok(WindowStats {
        flow_mean: flow_sum / n,
        active_vehicles_mean: veh_sum / n,
        population: last_pop,
    })
}

/// Settle the sim, measure the final window, compute the score, and write
/// `run-record.json` + `score.json` into `out_dir` (spec §3 steps 6–8, §10).
/// Returns once both files are written.
pub async fn finalize(
    client: &BridgeClient,
    state: Arc<Mutex<RunState>>,
    out_dir: &Path,
    map: MapInfo,
    started_at: String,
    ended_at: String,
) -> anyhow::Result<()> {
    let (cfg, baseline, tally, actions, end_reason) = {
        let s = state.lock().await;
        (
            s.config.clone(),
            s.baseline.clone(),
            Tally { num_changes: s.num_changes, money_spent: s.money_spent },
            s.actions.clone(),
            s.end_reason.unwrap_or(EndReason::Submit),
        )
    };

    let settle_cfg = BenchConfig { window_ticks: cfg.settle_ticks, window_samples: 1, ..cfg.clone() };
    measure_window(client, &settle_cfg).await?;
    let final_stats = measure_window(client, &cfg).await?;

    let record = RunRecord {
        schema_version: 1,
        map,
        started_at,
        ended_at,
        end_reason,
        baseline,
        final_stats,
        tally,
        actions,
    };
    let score = score_run(&record, &cfg);

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
        let stats = measure_window(&c, &cfg).await.unwrap();
        assert_eq!(stats.flow_mean, 100.0);
        assert_eq!(stats.active_vehicles_mean, 0.0);
    }

    #[tokio::test]
    async fn finalize_writes_record_and_score() {
        use crate::benchmark::record::{EndReason, MapInfo, WindowStats};
        use crate::benchmark::state::RunState;
        use std::collections::HashMap;
        use std::sync::Arc;
        use tokio::sync::Mutex;

        let c = client().await;
        let cfg = BenchConfig::default();
        let baseline = WindowStats { flow_mean: 80.0, active_vehicles_mean: 0.0, population: 0 };
        let state = Arc::new(Mutex::new(RunState::new(
            cfg.clone(),
            "gridlock-v1".into(),
            baseline,
            HashMap::new(),
        )));
        state.lock().await.end_reason = Some(EndReason::Submit);

        let dir = std::env::temp_dir().join(format!("sb-finalize-{}", std::process::id()));
        let map = MapInfo { id: "gridlock-v1".into(), source: "test".into(), game_version: "1.21.1-f9".into() };
        finalize(&c, state, &dir, map, "t0".into(), "t1".into()).await.unwrap();

        let rec: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("run-record.json")).unwrap()).unwrap();
        assert_eq!(rec["end_reason"], "submit");
        assert_eq!(rec["started_at"], "t0");
        assert_eq!(rec["ended_at"], "t1");
        let score: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("score.json")).unwrap()).unwrap();
        assert!(score["score"].is_number());
        std::fs::remove_dir_all(&dir).ok();
    }
}
