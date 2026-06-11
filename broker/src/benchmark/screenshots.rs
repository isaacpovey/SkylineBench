//! Real in-game screenshot persistence (spec: 2026-06-11 timelapse design).
//!
//! Best-effort telemetry: a failed capture logs once and disables the sink for
//! the rest of the run — never fails the tool call, never retries per-frame
//! against a mod that lacks the endpoint.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use tokio::sync::Mutex;

use crate::benchmark::state::RunState;
use crate::bridge_client::BridgeClient;
use crate::service::CameraShot;

#[derive(Clone, Copy)]
pub enum Stream {
    Overview,
    Action,
}

impl Stream {
    fn subdir(self) -> &'static str {
        match self {
            Stream::Overview => "overview",
            Stream::Action => "actions",
        }
    }
}

pub struct ScreenshotSink {
    dir: PathBuf,
    overview_seq: AtomicU64,
    action_seq: AtomicU64,
    disabled: AtomicBool,
}

impl ScreenshotSink {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            overview_seq: AtomicU64::new(0),
            action_seq: AtomicU64::new(0),
            disabled: AtomicBool::new(false),
        }
    }

    pub fn disabled(&self) -> bool {
        self.disabled.load(Ordering::Relaxed)
    }

    pub async fn capture(
        &self,
        client: &BridgeClient,
        state: &Mutex<RunState>,
        shot: CameraShot,
        stream: Stream,
        trigger: &str,
        caption: Option<String>,
    ) {
        if self.disabled() {
            return;
        }
        let png = match client.screenshot(shot.x, shot.z, shot.size, shot.top_down).await {
            Ok(png) => png,
            Err(e) => {
                eprintln!("benchmark: screenshot capture failed ({e}); disabling screenshots for this run");
                self.disabled.store(true, Ordering::Relaxed);
                return;
            }
        };
        let tick = client.health().await.map(|h| h.tick).unwrap_or(0);
        let seq = match stream {
            Stream::Overview => self.overview_seq.fetch_add(1, Ordering::Relaxed) + 1,
            Stream::Action => self.action_seq.fetch_add(1, Ordering::Relaxed) + 1,
        };
        let (changes, flow, congested) = {
            let s = state.lock().await;
            (s.num_changes, s.flow.mean(), (!s.congestion.is_empty()).then(|| s.congestion.mean()))
        };
        let dir = self.dir.join(stream.subdir());
        let name = format!("{seq:05}-tick{tick}.png");
        let written = std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(dir.join(&name), &png))
            .and_then(|()| {
                let action = matches!(stream, Stream::Action).then_some(trigger);
                let line = serde_json::json!({
                    "seq": seq, "file": name, "tick": tick, "trigger": trigger,
                    "changes": changes, "flow": flow, "congested": congested,
                    "action": action, "caption": caption,
                });
                let mut f = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join("index.jsonl"))?;
                use std::io::Write;
                writeln!(f, "{line}")
            });
        if let Err(e) = written {
            eprintln!("benchmark: screenshot persist failed ({e}); disabling screenshots for this run");
            self.disabled.store(true, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark::config::BenchConfig;
    use crate::benchmark::state::RunState;
    use crate::bridge_client::BridgeClient;
    use crate::mock;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    async fn sink_with_mock(dir: &std::path::Path) -> (ScreenshotSink, Arc<BridgeClient>, Arc<Mutex<RunState>>) {
        let (addr, server) = mock::bind("127.0.0.1:0".parse().unwrap()).await;
        tokio::spawn(server);
        let client = Arc::new(BridgeClient::new(format!("http://{addr}")));
        let state = Arc::new(Mutex::new(RunState::new(BenchConfig::default(), HashMap::new())));
        (ScreenshotSink::new(dir.to_path_buf()), client, state)
    }

    #[tokio::test]
    async fn persists_overview_and_action_frames_with_indexes() {
        let dir = std::env::temp_dir().join(format!("sb-shots-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let (sink, client, state) = sink_with_mock(&dir).await;

        let shot = crate::service::overview_shot(&crate::contract::Network { nodes: vec![], segments: vec![] });
        sink.capture(&client, &state, shot, Stream::Overview, "step", None).await;
        sink.capture(&client, &state, crate::service::closeup_shot(10.0, 20.0), Stream::Action, "build_road",
            Some("build_road: road".into())).await;

        let overview = std::fs::read_to_string(dir.join("overview/index.jsonl")).unwrap();
        let entry: serde_json::Value = serde_json::from_str(overview.lines().next().unwrap()).unwrap();
        assert_eq!(entry["seq"], 1);
        assert_eq!(entry["trigger"], "step");
        assert!(entry["tick"].is_u64());
        assert!(dir.join("overview").join(entry["file"].as_str().unwrap()).exists());

        let actions = std::fs::read_to_string(dir.join("actions/index.jsonl")).unwrap();
        let entry: serde_json::Value = serde_json::from_str(actions.lines().next().unwrap()).unwrap();
        assert_eq!(entry["action"], "build_road");
        assert_eq!(entry["caption"], "build_road: road");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn disables_itself_after_a_capture_failure() {
        let dir = std::env::temp_dir().join(format!("sb-shots-fail-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        // Point at a dead address: every capture errors.
        let client = Arc::new(BridgeClient::new("http://127.0.0.1:1"));
        let state = Arc::new(Mutex::new(RunState::new(BenchConfig::default(), HashMap::new())));
        let sink = ScreenshotSink::new(dir.clone());

        sink.capture(&client, &state, crate::service::closeup_shot(0.0, 0.0), Stream::Action, "bulldoze", None).await;
        assert!(sink.disabled(), "first failure disables the sink");
        sink.capture(&client, &state, crate::service::closeup_shot(0.0, 0.0), Stream::Action, "bulldoze", None).await;
        assert!(!dir.join("actions/index.jsonl").exists(), "no frames after disable");
        std::fs::remove_dir_all(&dir).ok();
    }
}
