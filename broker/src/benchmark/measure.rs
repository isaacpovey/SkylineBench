use crate::benchmark::config::BenchConfig;
use crate::benchmark::record::WindowStats;
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
}
