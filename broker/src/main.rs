use clap::{Parser, Subcommand};

use skylinebench::mock;
use skylinebench::tools::Skyline;

#[derive(Parser)]
#[command(
    name = "skylinebench",
    about = "Cities: Skylines 1 MCP harness (broker)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the MCP server over stdio, talking to the mod at --mod-url.
    Serve {
        #[arg(long, default_value = "http://127.0.0.1:8787")]
        mod_url: String,
    },
    /// Run the in-memory mock mod (for development/testing) on --addr.
    Mock {
        #[arg(long, default_value = "127.0.0.1:8787")]
        addr: String,
    },
    /// Run a benchmark session: serve MCP (instrumented) against the mod and
    /// score the run when the agent finishes.
    Benchmark {
        #[arg(long, default_value = "http://127.0.0.1:8787")]
        mod_url: String,
        #[arg(long)]
        map: String,
        #[arg(long, default_value = "test")]
        map_source: String,
        #[arg(long)]
        out: std::path::PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Mock { addr } => {
            let (bound, server) = mock::bind(addr.parse()?).await;
            eprintln!("mock mod listening on http://{bound}");
            server.await;
        }
        Command::Serve { mod_url } => {
            use rmcp::ServiceExt;
            let server = Skyline::new(mod_url)
                .serve((tokio::io::stdin(), tokio::io::stdout()))
                .await?;
            server.waiting().await?;
        }
        Command::Benchmark { mod_url, map, map_source, out } => {
            use std::collections::HashMap;
            use std::sync::Arc;
            use tokio::sync::Mutex;
            use skylinebench::benchmark::{
                finalize, measure_window, BenchConfig, BenchmarkServer, MapInfo, RunState,
            };
            use skylinebench::bridge_client::BridgeClient;
            use rmcp::ServiceExt;

            fn epoch_secs() -> String {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs().to_string())
                    .unwrap_or_default()
            }

            let client = Arc::new(BridgeClient::new(mod_url));
            let health = client.health().await?;
            anyhow::ensure!(health.city_loaded, "no city loaded — load the benchmark save first");
            let started_at = epoch_secs();

            let cfg = BenchConfig::default();
            let road_costs: HashMap<String, i64> = client
                .road_types()
                .await?
                .road_types
                .into_iter()
                .map(|r| (r.name, r.construction_cost))
                .collect();

            eprintln!("benchmark: measuring baseline…");
            let baseline = measure_window(&client, &cfg).await?;
            eprintln!("benchmark: baseline flow {:.1}%", baseline.flow_mean);

            let state = Arc::new(Mutex::new(RunState::new(cfg.clone(), baseline, road_costs)));

            let watch_client = client.clone();
            let watch_state = state.clone();
            let game_version = health.game_version.clone();
            let started_at = started_at.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let ended = {
                        let mut s = watch_state.lock().await;
                        s.check_timeout();
                        s.end_reason.is_some()
                    };
                    if ended {
                        let ended_at = epoch_secs();
                        let map_info = MapInfo {
                            id: map.clone(),
                            source: map_source.clone(),
                            game_version: game_version.clone(),
                        };
                        let code = match finalize(&watch_client, watch_state.clone(), &out, map_info, started_at.clone(), ended_at).await {
                            Ok(()) => {
                                eprintln!("benchmark: wrote artifacts to {}", out.display());
                                0
                            }
                            Err(e) => {
                                eprintln!("benchmark: finalize error: {e}");
                                1
                            }
                        };
                        std::process::exit(code);
                    }
                }
            });

            let server = BenchmarkServer::new(client, state)
                .serve((tokio::io::stdin(), tokio::io::stdout()))
                .await?;
            server.waiting().await?;
        }
    }
    Ok(())
}
