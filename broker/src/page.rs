//! Static run-detail page generator: reads a curated narrative TOML plus a
//! run's `run-record.json` + `score.json` and emits `website/runs/<slug>.html`
//! with inline-SVG charts. Sibling to the post-run `timelapse` tooling.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::benchmark::record::{ActionEntry, RunRecord, Score};

/// One phase of the curated play-by-play. Text only (design 2026-06-12).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Beat {
    pub title: String,
    pub body: String,
}

/// Curated, hand-authored content for one run. Metrics, charts, score and the
/// timelapse all come from `run_dir`; this file only carries the human story.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Narrative {
    pub slug: String,
    pub model_name: String,
    pub map: String,
    pub run_dir: String,
    pub verdict: String,
    #[serde(default)]
    pub beat: Vec<Beat>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrative_parses_from_toml() {
        let src = r#"
slug = "fable-5"
model_name = "Claude Fable 5"
map = "gridlock-v1"
run_dir = "benchmark/runs/20260612-121219"
verdict = "Cut jammed road by two-thirds with surgical upgrades."

[[beat]]
title = "Survey"
body = "Read the city before touching it."

[[beat]]
title = "Submit"
body = "Stepped the sim, watched it settle, then submitted."
"#;
        let n: Narrative = toml::from_str(src).unwrap();
        assert_eq!(n.slug, "fable-5");
        assert_eq!(n.model_name, "Claude Fable 5");
        assert_eq!(n.beat.len(), 2);
        assert_eq!(n.beat[0].title, "Survey");
    }
}
