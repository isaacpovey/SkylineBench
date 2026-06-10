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
    /// Render a captured stream-json transcript to readable markdown.
    RenderTranscript {
        #[arg(long)]
        input: std::path::PathBuf,
        #[arg(long)]
        out: std::path::PathBuf,
    },
    /// Read stream-json on stdin and print a human-readable line per event
    /// (for live console display during a run).
    FormatStream,
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
    /// Finalize a finished benchmark run: read end-state.json from --out, run
    /// the settle + final measurement against the mod, and write
    /// run-record.json + score.json. Run this AFTER the agent session exits.
    BenchmarkFinalize {
        #[arg(long, default_value = "http://127.0.0.1:8787")]
        mod_url: String,
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
        Command::RenderTranscript { input, out } => {
            let jsonl = std::fs::read_to_string(&input)?;
            std::fs::write(&out, skylinebench::benchmark::render_transcript(&jsonl))?;
        }
        Command::FormatStream => {
            use std::io::{BufRead, Write};
            let stdin = std::io::stdin();
            let mut out = std::io::stdout();
            for line in stdin.lock().lines() {
                let line = line?;
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(text) = skylinebench::benchmark::format_event_live(&v) {
                        writeln!(out, "{text}")?;
                        out.flush()?;
                    }
                }
            }
        }
        Command::Benchmark { mod_url, map, map_source, out } => {
            use std::collections::HashMap;
            use std::sync::Arc;
            use tokio::sync::Mutex;
            use skylinebench::benchmark::{persist, BenchConfig, BenchmarkServer, EndStatePersister, MapInfo, RunState};
            use skylinebench::bridge_client::BridgeClient;
            use rmcp::ServiceExt;

            let client = Arc::new(BridgeClient::new(mod_url));
            let health = client.health().await?;
            anyhow::ensure!(health.city_loaded, "no city loaded — load the benchmark save first");
            let started_at = persist::epoch_secs();

            let cfg = BenchConfig::default();
            let road_costs: HashMap<String, i64> = client
                .road_types()
                .await?
                .road_types
                .into_iter()
                .map(|r| (r.name, r.construction_cost))
                .collect();

            // The baseline is measured lazily on the agent's first tool call, NOT
            // here — doing it before serving would block the MCP `initialize`
            // handshake (which has its own ~60s request timeout) for the whole
            // slow window on a large city. Serve immediately instead.
            eprintln!("benchmark: serving MCP; baseline measured on first tool call…");
            let state = Arc::new(Mutex::new(RunState::new(cfg, road_costs)));
            let map_info = MapInfo {
                id: map,
                source: map_source,
                game_version: health.game_version,
            };
            let persister = Arc::new(EndStatePersister {
                out_dir: out.clone(),
                map: map_info,
                started_at,
            });

            // Watchdog: the wall-clock cap is the only end reason that must
            // force the process down mid-session (a submit ends the session
            // naturally — claude exits and kills us; the snapshot was already
            // persisted eagerly on the submit response). Finalize (settle +
            // measure + score) happens in `benchmark-finalize`, run by run.sh
            // AFTER claude exits, so it can't be killed by client teardown or
            // timeouts.
            let watch_state = state.clone();
            let watch_persister = persister.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let timed_out = {
                        let mut s = watch_state.lock().await;
                        s.check_timeout();
                        s.end_reason == Some(skylinebench::benchmark::record::EndReason::Timeout)
                    };
                    if timed_out {
                        let code = match watch_persister.write(&*watch_state.lock().await) {
                            Ok(()) => {
                                eprintln!("benchmark: wall-clock cap hit; wrote end-state.json");
                                0
                            }
                            Err(e) => {
                                eprintln!("benchmark: end-state write error: {e}");
                                1
                            }
                        };
                        std::process::exit(code);
                    }
                }
            });

            let server = BenchmarkServer::new(client, state.clone())
                .with_persist(persister.clone())
                .serve((tokio::io::stdin(), tokio::io::stdout()))
                .await?;
            server.waiting().await?;

            // Graceful teardown (stdin closed). Best-effort final snapshot:
            // claude normally kills the process instead of reaching here, but
            // by then the eager per-response persistence has already written
            // the latest snapshot. end_reason None (the agent quit without
            // submitting) is recorded as `disconnect`.
            persister.write(&*state.lock().await)?;
            eprintln!("benchmark: session ended; wrote end-state.json to {}", out.display());
        }
        Command::BenchmarkFinalize { mod_url, out } => {
            use skylinebench::benchmark::{finalize, EndState};
            use skylinebench::bridge_client::BridgeClient;

            let path = out.join("end-state.json");
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| anyhow::anyhow!("cannot read {}: {e} — did the benchmark session run?", path.display()))?;
            let end: EndState = serde_json::from_str(&raw)
                .map_err(|e| anyhow::anyhow!("invalid {}: {e}", path.display()))?;

            let client = BridgeClient::new(mod_url);
            let health = client.health().await?;
            anyhow::ensure!(health.city_loaded, "no city loaded — cannot run the settle/final measurement");

            eprintln!("benchmark-finalize: settle + final window (this takes several minutes)…");
            finalize(&client, end, &out).await?;
            eprintln!("benchmark-finalize: wrote run-record.json + score.json to {}", out.display());
        }
    }
    Ok(())
}
