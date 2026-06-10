use std::path::PathBuf;

use crate::benchmark::record::MapInfo;
use crate::benchmark::state::RunState;

pub fn epoch_secs() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

/// Writes the current `RunState` snapshot to `<out_dir>/end-state.json` so
/// `benchmark-finalize` can score the run in a separate process. Written on
/// every tool response: claude kills the MCP server on exit (no graceful
/// stdin close), so the snapshot must already be on disk by the time the
/// final tool result reaches the client.
pub struct EndStatePersister {
    pub out_dir: PathBuf,
    pub map: MapInfo,
    pub started_at: String,
}

impl EndStatePersister {
    pub fn write(&self, state: &RunState) -> anyhow::Result<()> {
        let end = state.end_state(self.map.clone(), self.started_at.clone(), epoch_secs());
        let tmp = self.out_dir.join("end-state.json.tmp");
        let dest = self.out_dir.join("end-state.json");
        std::fs::create_dir_all(&self.out_dir)?;
        std::fs::write(&tmp, serde_json::to_string_pretty(&end)?)?;
        std::fs::rename(&tmp, &dest)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark::config::BenchConfig;
    use crate::benchmark::record::EndReason;
    use std::collections::HashMap;

    #[test]
    fn writes_end_state_snapshot_atomically() {
        let dir = std::env::temp_dir().join(format!("sb-persist-unit-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();

        let mut state = RunState::new(BenchConfig::default(), HashMap::new());
        state.end_reason = Some(EndReason::Submit);

        let persister = EndStatePersister {
            out_dir: dir.clone(),
            map: MapInfo { id: "m".into(), source: "test".into(), game_version: "v".into() },
            started_at: "t0".into(),
        };
        persister.write(&state).unwrap();

        let end: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("end-state.json")).unwrap()).unwrap();
        assert_eq!(end["end_reason"], "submit");
        assert!(!dir.join("end-state.json.tmp").exists(), "tmp file must be renamed away");

        std::fs::remove_dir_all(&dir).ok();
    }
}
