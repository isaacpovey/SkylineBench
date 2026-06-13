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

fn polyline(samples: &[f64], lo: f64, hi: f64, class: &str) -> String {
    let n = samples.len().max(2) as f64;
    let (x0, w, y0, h) = (40.0_f64, 304.0_f64, 12.0_f64, 150.0_f64);
    let span = (hi - lo).max(1.0);
    let pts = samples
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let x = x0 + i as f64 / (n - 1.0) * w;
            let y = y0 + h - (v - lo) / span * h;
            format!("{x:.1},{y:.1}")
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!(r#"<polyline points="{pts}" class="{class}"/>"#)
}

/// Overlaid baseline vs final flow over the 8-sample settle windows.
fn chart_flow_settling(r: &RunRecord) -> String {
    let bs = &r.flow_samples.baseline;
    let fs = &r.flow_samples.final_samples;
    let all = bs.iter().chain(fs.iter()).copied();
    let hi = all.clone().fold(f64::MIN, f64::max).max(1.0);
    let lo = all.fold(f64::MAX, f64::min).min(hi);
    let base = polyline(bs, lo, hi, "c-line-base");
    let fin = polyline(fs, lo, hi, "c-line-final");
    let y_top = 12.0_f64;
    let y_bot = y_top + 150.0;
    format!(
        concat!(
            r#"<svg viewBox="0 0 360 184" class="chart-svg" role="img" aria-label="Flow settling curves">"#,
            r#"<text x="0" y="{ty:.1}" class="c-axis">{hi}</text>"#,
            r#"<text x="0" y="{by:.1}" class="c-axis">{lo}</text>"#,
            r#"{base}{fin}</svg>"#,
        ),
        ty = y_top + 4.0,
        hi = fmt_num(hi),
        by = y_bot,
        lo = fmt_num(lo),
        base = base,
        fin = fin,
    )
}

/// Paired baseline/final horizontal bars for the five snapshot metrics. Each
/// metric is scaled to its own max so the very different magnitudes stay
/// readable; absolute values are labelled at each bar's end.
fn chart_before_after(r: &RunRecord) -> String {
    let b = &r.baseline;
    let f = &r.final_stats;
    let rows: [(&str, f64, f64); 5] = [
        ("Flow", b.flow_mean, f.flow_mean),
        ("Congested m", b.congested_meters, f.congested_meters),
        ("Jammed junctions", b.congested_junctions as f64, f.congested_junctions as f64),
        ("Active vehicles", b.active_vehicles_mean, f.active_vehicles_mean),
        ("Population", b.population as f64, f.population as f64),
    ];
    let (top, row_h, x0, w) = (8.0_f64, 40.0_f64, 112.0_f64, 200.0_f64);
    let body = rows
        .iter()
        .enumerate()
        .map(|(i, (name, base, fin))| {
            let y = top + i as f64 * row_h;
            let max = base.max(*fin).max(1.0);
            let bw = base / max * w;
            let fw = fin / max * w;
            format!(
                concat!(
                    r#"<text x="0" y="{ly:.1}" class="c-axis">{name}</text>"#,
                    r#"<rect x="{x0:.1}" y="{yb:.1}" width="{bw:.1}" height="8" rx="2" class="c-base"/>"#,
                    r#"<text x="{tbx:.1}" y="{tby:.1}" class="c-val">{bv}</text>"#,
                    r#"<rect x="{x0:.1}" y="{yf:.1}" width="{fw:.1}" height="8" rx="2" class="c-final"/>"#,
                    r#"<text x="{tfx:.1}" y="{tfy:.1}" class="c-val c-val-final">{fv}</text>"#,
                ),
                ly = y + 13.0,
                name = esc(name),
                x0 = x0,
                yb = y + 4.0,
                bw = bw,
                tbx = x0 + bw + 5.0,
                tby = y + 11.0,
                bv = fmt_num(*base),
                yf = y + 16.0,
                fw = fw,
                tfx = x0 + fw + 5.0,
                tfy = y + 23.0,
                fv = fmt_num(*fin),
            )
        })
        .collect::<Vec<_>>()
        .join("");
    format!(
        r#"<svg viewBox="0 0 360 208" class="chart-svg" role="img" aria-label="Before and after metrics">{body}</svg>"#
    )
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

    fn sample_record() -> RunRecord {
        use crate::benchmark::config::BenchConfig;
        use crate::benchmark::record::{
            EndReason, FlowSamples, MapInfo, Tally, WindowStats,
        };
        RunRecord {
            schema_version: 3,
            config: BenchConfig::default(),
            map: MapInfo { id: "gridlock-v1".into(), source: "test".into(), game_version: "1.21.1-f9".into() },
            started_at: "1781291539".into(),
            ended_at: "1781296316".into(),
            end_reason: EndReason::Submit,
            baseline: WindowStats { flow_mean: 57.375, active_vehicles_mean: 2112.125, population: 31640, congested_meters: 5121.721, congested_junctions: 35 },
            final_stats: WindowStats { flow_mean: 70.625, active_vehicles_mean: 1708.625, population: 31174, congested_meters: 1853.891, congested_junctions: 12 },
            flow_samples: FlowSamples {
                baseline: vec![67.0, 62.0, 57.0, 57.0, 54.0, 54.0, 53.0, 55.0],
                final_samples: vec![67.0, 67.0, 70.0, 70.0, 71.0, 72.0, 73.0, 75.0],
            },
            tally: Tally { num_changes: 197, money_spent: 1_239_118 },
            actions: vec![
                ActionEntry { seq: 1, tool: "bulldoze".into(), cost: 0 },
                ActionEntry { seq: 2, tool: "build_road".into(), cost: 57_790 },
                ActionEntry { seq: 3, tool: "upgrade_road".into(), cost: 1_181_328 },
            ],
        }
    }

    #[test]
    fn flow_settling_has_two_lines() {
        let svg = chart_flow_settling(&sample_record());
        assert!(svg.starts_with("<svg"));
        assert_eq!(svg.matches("<polyline").count(), 2);
        assert!(svg.contains("class=\"c-line-base\""));
        assert!(svg.contains("class=\"c-line-final\""));
    }

    #[test]
    fn before_after_has_metric_rows_and_values() {
        let svg = chart_before_after(&sample_record());
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("Population"));
        assert!(svg.contains("31,640"));   // baseline population
        assert!(svg.contains("71"));        // final flow, rounded
        assert!(svg.contains("class=\"c-final\""));
        assert!(svg.contains("class=\"c-base\""));
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
