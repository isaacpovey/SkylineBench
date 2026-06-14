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
    /// Render a captured JSONL transcript to readable markdown.
    RenderTranscript {
        #[arg(long)]
        input: std::path::PathBuf,
        #[arg(long)]
        out: std::path::PathBuf,
        #[arg(long, default_value = "claude")]
        harness: String,
    },
    /// Read JSONL on stdin and print a human-readable line per event
    /// (for live console display during a run).
    FormatStream {
        #[arg(long, default_value = "claude")]
        harness: String,
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
        /// Directory for per-run render frames (timelapse). Omit to disable.
        #[arg(long)]
        renders_dir: Option<std::path::PathBuf>,
        /// Directory for real in-game screenshot frames (timelapse). Omit to disable.
        #[arg(long)]
        screenshots_dir: Option<std::path::PathBuf>,
    },
    /// Assemble a run's frames (screenshots, or renders as fallback) into an
    /// annotated timelapse mp4. Requires ffmpeg.
    Timelapse {
        run_dir: std::path::PathBuf,
        #[arg(long, default_value_t = 4)]
        fps: u32,
        /// Output path (default: <run_dir>/timelapse.mp4).
        #[arg(long)]
        out: Option<std::path::PathBuf>,
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
    /// Generate a static run-detail page (website/runs/<slug>.html) from a
    /// curated narrative TOML plus the run's run-record.json + score.json.
    BuildPage {
        #[arg(long)]
        narrative: std::path::PathBuf,
        /// Output HTML path (default: website/runs/<slug>.html).
        #[arg(long)]
        out: Option<std::path::PathBuf>,
        #[arg(long, default_value = "website/assets/runs")]
        assets_dir: std::path::PathBuf,
    },
    /// Detect this host's sandbox backend, write its profile + sandbox.argv
    /// (NUL-delimited wrapper prefix) into --session-dir, print the backend
    /// name to stdout, and warn on stderr if unsandboxed.
    SandboxPrepare {
        #[arg(long)]
        root: std::path::PathBuf,
        #[arg(long)]
        session_dir: std::path::PathBuf,
    },
    /// Resolve a harness launch plan: write its config files and emit
    /// launch.argv (NUL-delimited), launch.env (NUL-delimited KEY=VALUE), and
    /// launch.required-env (newline-delimited) into --session-dir.
    HarnessPrepare {
        #[arg(long)]
        harness: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        prompt_file: std::path::PathBuf,
        #[arg(long)]
        mcp_shell: String,
        #[arg(long)]
        session_dir: std::path::PathBuf,
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
        Command::RenderTranscript { input, out, harness } => {
            let harness = skylinebench::benchmark::Harness::parse(&harness)
                .ok_or_else(|| anyhow::anyhow!("unknown harness: {harness}"))?;
            let jsonl = std::fs::read_to_string(&input)?;
            std::fs::write(&out, skylinebench::benchmark::render_transcript(harness, &jsonl))?;
        }
        Command::FormatStream { harness } => {
            use std::io::{BufRead, Write};
            let harness = skylinebench::benchmark::Harness::parse(&harness)
                .ok_or_else(|| anyhow::anyhow!("unknown harness: {harness}"))?;
            let stdin = std::io::stdin();
            let mut out = std::io::stdout();
            for line in stdin.lock().lines() {
                let line = line?;
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(text) = skylinebench::benchmark::format_event_live(harness, &v) {
                        writeln!(out, "{text}")?;
                        out.flush()?;
                    }
                }
            }
        }
        Command::Benchmark { mod_url, map, map_source, out, renders_dir, screenshots_dir } => {
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

            let server = {
                let s = BenchmarkServer::new(client, state.clone()).with_persist(persister.clone());
                let s = match renders_dir {
                    Some(dir) => s.with_renders_dir(dir),
                    None => s,
                };
                match screenshots_dir {
                    Some(dir) => s.with_screenshots_dir(dir),
                    None => s,
                }
            }
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
        Command::Timelapse { run_dir, fps, out } => {
            let out = out.unwrap_or_else(|| run_dir.join("timelapse.mp4"));
            skylinebench::timelapse::assemble(&run_dir, fps, &out)?;
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

            // Record the end flyby into <out>/screenshots/flyby/end_* while the
            // game is still live (screenshots were moved here after the session).
            {
                use skylinebench::service::highway_flyby_path;
                let base = out.join("screenshots").join("flyby");
                if let Ok(net) = client.network().await {
                    let path = highway_flyby_path(&net);
                    for (suffix, kfs) in [("ns", &path.ns), ("we", &path.we)] {
                        if kfs.is_empty() {
                            continue;
                        }
                        let dir = base.join(format!("end_{suffix}"));
                        let dir_str = dir.to_string_lossy().to_string();
                        if let Err(e) = client.flyby(kfs, 6.0, 12, &dir_str).await {
                            eprintln!("benchmark-finalize: end flyby '{suffix}' failed ({e}); skipping");
                            break;
                        }
                    }
                }
            }

            finalize(&client, end, &out).await?;
            eprintln!("benchmark-finalize: wrote run-record.json + score.json to {}", out.display());
        }
        Command::BuildPage { narrative, out, assets_dir } => {
            let written = skylinebench::page::build(&narrative, out, &assets_dir)?;
            eprintln!("build-page: wrote {}", written.display());
        }
        Command::SandboxPrepare { root, session_dir } => {
            use skylinebench::benchmark::{select_sandbox, Os, SandboxInputs};

            fn on_path(bin: &str) -> bool {
                std::env::var_os("PATH")
                    .map(|paths| {
                        std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file())
                    })
                    .unwrap_or(false)
            }

            let os = if cfg!(target_os = "macos") {
                Os::Mac
            } else if cfg!(target_os = "linux") {
                Os::Linux
            } else {
                Os::Other
            };

            let inputs = SandboxInputs {
                os,
                sandbox_exec_available: on_path("sandbox-exec"),
                bwrap_available: on_path("bwrap"),
                firejail_available: on_path("firejail"),
                repo_root: root,
                session_dir: session_dir.clone(),
            };
            let plan = select_sandbox(&inputs);

            if let Some(cf) = &plan.profile_file {
                std::fs::write(&cf.path, &cf.contents)?;
            }
            let blob: Vec<u8> = plan
                .wrapper_argv
                .iter()
                .flat_map(|a| a.as_bytes().iter().copied().chain(std::iter::once(0u8)))
                .collect();
            std::fs::write(session_dir.join("sandbox.argv"), blob)?;

            if let Some(w) = &plan.warning {
                eprintln!("WARNING: {w}");
            }
            println!("{}", plan.backend.as_str());
        }
        Command::HarnessPrepare { harness, model, prompt_file, mcp_shell, session_dir } => {
            let harness = skylinebench::benchmark::Harness::parse(&harness)
                .ok_or_else(|| anyhow::anyhow!("unknown harness: {harness}"))?;
            let prompt = std::fs::read_to_string(&prompt_file)?;
            let inputs = skylinebench::benchmark::LaunchInputs {
                model,
                prompt,
                mcp_shell,
                session_dir: session_dir.clone(),
            };
            let spec = skylinebench::benchmark::build_launch(harness, &inputs);

            for cf in &spec.config_files {
                if let Some(parent) = cf.path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&cf.path, &cf.contents)?;
            }

            let argv_blob: Vec<u8> =
                spec.argv.iter().flat_map(|a| a.as_bytes().iter().copied().chain(std::iter::once(0u8))).collect();
            std::fs::write(session_dir.join("launch.argv"), argv_blob)?;

            let env_blob: Vec<u8> = spec
                .env
                .iter()
                .flat_map(|(k, v)| format!("{k}={v}").into_bytes().into_iter().chain(std::iter::once(0u8)))
                .collect();
            std::fs::write(session_dir.join("launch.env"), env_blob)?;

            std::fs::write(session_dir.join("launch.required-env"), spec.required_env.join("\n"))?;
        }
    }
    Ok(())
}
