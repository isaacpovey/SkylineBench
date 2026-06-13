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

fn fmt_money(n: i64) -> String {
    let a = n.abs();
    if a >= 1_000_000 {
        format!("${:.2}M", n as f64 / 1_000_000.0)
    } else if a >= 1_000 {
        format!("${:.1}k", n as f64 / 1_000.0)
    } else {
        format!("${n}")
    }
}

fn fmt_num(v: f64) -> String {
    let n = v.round() as i64;
    let digits = n.abs().to_string();
    let grouped = digits
        .as_bytes()
        .rchunks(3)
        .rev()
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect::<Vec<_>>()
        .join(",");
    if n < 0 {
        format!("-{grouped}")
    } else {
        grouped
    }
}

/// Signed percentage change `from → to`, or an em-dash when `from` is zero.
fn pct(from: f64, to: f64) -> String {
    if from == 0.0 {
        return "—".to_string();
    }
    format!("{:+.0}%", (to - from) / from * 100.0)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_money_scales() {
        assert_eq!(fmt_money(1_239_118), "$1.24M");
        assert_eq!(fmt_money(57_790), "$57.8k");
        assert_eq!(fmt_money(0), "$0");
    }

    #[test]
    fn fmt_num_rounds_and_groups() {
        assert_eq!(fmt_num(31_640.0), "31,640");
        assert_eq!(fmt_num(70.625), "71");
        assert_eq!(fmt_num(57.375), "57");
    }

    #[test]
    fn pct_signed() {
        assert_eq!(pct(5121.72, 1853.89), "-64%");
        assert_eq!(pct(57.375, 70.625), "+23%");
        assert_eq!(pct(0.0, 5.0), "—");
    }

    #[test]
    fn esc_escapes_markup() {
        assert_eq!(esc("a & b < c > d"), "a &amp; b &lt; c &gt; d");
    }

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
