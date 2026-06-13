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

/// Collapse the action log into `(tool, count, total_cost)` in first-seen order.
fn group_actions(actions: &[ActionEntry]) -> Vec<(String, u32, i64)> {
    actions.iter().fold(Vec::new(), |mut acc, a| {
        match acc.iter_mut().find(|(tool, _, _)| *tool == a.tool) {
            Some(entry) => {
                entry.1 += 1;
                entry.2 += a.cost;
            }
            None => acc.push((a.tool.clone(), 1, a.cost)),
        }
        acc
    })
}

/// One horizontal bar per tool, sized by call count, labelled count + cost.
fn chart_action_breakdown(r: &RunRecord) -> String {
    let groups = group_actions(&r.actions);
    let max = groups.iter().map(|(_, c, _)| *c).max().unwrap_or(1).max(1) as f64;
    let (top, row_h, x0, w) = (10.0_f64, 44.0_f64, 120.0_f64, 150.0_f64);
    let body = groups
        .iter()
        .enumerate()
        .map(|(i, (tool, count, cost))| {
            let y = top + i as f64 * row_h;
            let bw = *count as f64 / max * w;
            format!(
                concat!(
                    r#"<text x="0" y="{ly:.1}" class="c-axis">{tool}</text>"#,
                    r#"<rect x="{x0:.1}" y="{yb:.1}" width="{bw:.1}" height="14" rx="3" class="c-final"/>"#,
                    r#"<text x="{tx:.1}" y="{ty:.1}" class="c-val">{count} · {cost}</text>"#,
                ),
                ly = y + 9.0,
                tool = esc(tool),
                x0 = x0,
                yb = y,
                bw = bw,
                tx = x0 + bw + 6.0,
                ty = y + 11.0,
                count = count,
                cost = fmt_money(*cost),
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let height = top + groups.len() as f64 * row_h + 4.0;
    format!(
        r#"<svg viewBox="0 0 360 {height:.0}" class="chart-svg" role="img" aria-label="Actions by type">{body}</svg>"#
    )
}

/// Cumulative money spent over the action sequence. The x-axis is the action
/// number, which is also the running change count, so one line conveys both.
fn chart_cumulative_spend(r: &RunRecord) -> String {
    let (x0, w, y0, h) = (8.0_f64, 300.0_f64, 12.0_f64, 150.0_f64);
    let total: i64 = r.actions.iter().map(|a| a.cost).sum();
    let n = r.actions.len().max(1) as f64;
    let max = (total as f64).max(1.0);
    let pts = r
        .actions
        .iter()
        .scan(0_i64, |cum, a| {
            *cum += a.cost;
            Some(*cum)
        })
        .enumerate()
        .map(|(i, cum)| {
            let x = x0 + (i as f64 + 1.0) / n * w;
            let y = y0 + h - cum as f64 / max * h;
            format!("{x:.1},{y:.1}")
        })
        .collect::<Vec<_>>()
        .join(" ");
    let line = format!(r#"<polyline points="{x0:.1},{base:.1} {pts}" class="c-line-final"/>"#, base = y0 + h);
    let label = format!(
        r#"<text x="{lx:.1}" y="{ly:.1}" class="c-val c-val-final">{money} · {count} changes</text>"#,
        lx = x0,
        ly = y0 + h + 18.0,
        money = fmt_money(total),
        count = r.actions.len(),
    );
    format!(
        r#"<svg viewBox="0 0 360 188" class="chart-svg" role="img" aria-label="Cumulative spend">{line}{label}</svg>"#
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

fn beats_html(beats: &[Beat]) -> String {
    beats
        .iter()
        .map(|b| {
            let paras = b
                .body
                .split("\n\n")
                .filter(|p| !p.trim().is_empty())
                .map(|p| format!("<p>{}</p>", esc(p.trim())))
                .collect::<Vec<_>>()
                .join("");
            format!(
                r#"<li class="beat"><h3>{title}</h3>{paras}</li>"#,
                title = esc(&b.title),
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

fn chip(label: &str, value: &str) -> String {
    format!(
        r#"<div class="chip"><span class="chip-v">{value}</span><span class="chip-l">{label}</span></div>"#,
        value = esc(value),
        label = esc(label),
    )
}

fn chart_card(title: &str, svg: String) -> String {
    format!(
        r#"<figure class="chart-card"><figcaption>{title}</figcaption>{svg}</figure>"#,
        title = esc(title),
    )
}

/// Assemble the full static run-detail HTML document.
pub fn render_page(narrative: &Narrative, record: &RunRecord, score: &Score) -> String {
    let b = &record.baseline;
    let f = &record.final_stats;
    let chips = [
        chip("flow", &format!("{} → {}", fmt_num(b.flow_mean), fmt_num(f.flow_mean))),
        chip("congested metres", &pct(b.congested_meters, f.congested_meters)),
        chip("jammed junctions", &format!("{} → {}", b.congested_junctions, f.congested_junctions)),
        chip("population", &format!("{} → {}", fmt_num(b.population as f64), fmt_num(f.population as f64))),
        chip("changes", &record.tally.num_changes.to_string()),
        chip("spent", &fmt_money(record.tally.money_spent)),
    ]
    .join("");
    let charts = [
        chart_card("Before → after", chart_before_after(record)),
        chart_card("Flow settling", chart_flow_settling(record)),
        chart_card("Cumulative spend", chart_cumulative_spend(record)),
        chart_card("Actions by type", chart_action_breakdown(record)),
    ]
    .join("");
    format!(
        r##"<!DOCTYPE html>
<html lang="en" class="ds dark">
<head>
<meta charset="UTF-8" />
<meta name="viewport" content="width=device-width, initial-scale=1.0" />
<title>{model} · SkylineBench run</title>
<link rel="icon" type="image/svg+xml" href="../assets/favicon.svg" />
<link rel="stylesheet" href="../colors_and_type.css" />
<link rel="stylesheet" href="../styles.css" />
<link rel="stylesheet" href="../runs.css" />
</head>
<body class="ds dark">
<nav class="nav" data-scrolled="true"><div class="wrap">
  <a class="brand" href="../index.html#top"><span><b>Skyline</b><span class="slash">Bench</span></span></a>
  <div class="nav-links">
    <a class="nav-link" href="../index.html#results">← Back to results</a>
    <a class="btn btn-outline btn-sm" href="https://github.com/isaacpovey/SkylineBench" target="_blank" rel="noopener">GitHub</a>
  </div>
</div></nav>

<header class="run-hero"><div class="wrap-narrow">
  <p class="eyebrow">Run detail · <span class="mono">{map}</span></p>
  <h1 class="display">{model}</h1>
  <div class="run-score"><span class="rs-val">{score:.2}</span><span class="rs-of">/ 1.00 composite</span></div>
  <p class="lead">{verdict}</p>
  <figure class="media-frame">
    <div class="media-stage">
      <video muted loop playsinline preload="none" data-video-src="../assets/runs/{slug}.mp4"></video>
      <div class="media-placeholder"><div class="ph-title">timelapse</div></div>
    </div>
  </figure>
</div></header>

<section class="section"><div class="wrap">
  <div class="chips">{chips}</div>
  <div class="chart-grid">{charts}</div>
</div></section>

<section class="section section-soft"><div class="wrap-narrow">
  <div class="section-head"><p class="eyebrow">What the agent did</p>
  <h2 class="section-title">Step by step.</h2></div>
  <ol class="timeline">{beats}</ol>
</div></section>

<footer class="footer"><div class="wrap"><div class="footer-base">
  <span>Built by <a href="https://www.linkedin.com/in/isaacpovey/" target="_blank" rel="noopener">Isaac Povey</a></span>
  <span class="mono">GPLv3 · Cities: Skylines is a trademark of its respective owners</span>
</div></div></footer>

<script>
  (function () {{
    const v = document.querySelector('video[data-video-src]');
    if (!v) return;
    const ph = v.parentElement.querySelector('.media-placeholder');
    const s = document.createElement('source'); s.src = v.dataset.videoSrc; s.type = 'video/mp4';
    v.appendChild(s); v.load();
    v.addEventListener('loadeddata', () => {{ if (ph) ph.style.display = 'none'; }});
    v.addEventListener('error', () => {{ if (ph) ph.style.display = ''; }});
    if ('IntersectionObserver' in window) {{
      new IntersectionObserver((es) => es.forEach((e) => e.isIntersecting ? v.play().catch(() => {{}}) : v.pause()), {{ threshold: 0.3 }}).observe(v);
    }} else {{ v.play().catch(() => {{}}); }}
  }})();
</script>
</body>
</html>
"##,
        model = esc(&narrative.model_name),
        map = esc(&narrative.map),
        score = score.score,
        verdict = esc(&narrative.verdict),
        slug = esc(&narrative.slug),
        chips = chips,
        charts = charts,
        beats = beats_html(&narrative.beat),
    )
}

/// Read the narrative TOML + the run's record/score, render the page, write it
/// to `out` (default `website/runs/<slug>.html`), and copy the run's
/// `timelapse.mp4` into `assets_dir/<slug>.mp4` when present. Returns the
/// written HTML path.
pub fn build(
    narrative_path: &Path,
    out: Option<PathBuf>,
    assets_dir: &Path,
) -> anyhow::Result<PathBuf> {
    let narrative: Narrative = toml::from_str(&std::fs::read_to_string(narrative_path)?)?;
    anyhow::ensure!(
        !narrative.slug.is_empty()
            && narrative.slug.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
        "slug must be non-empty and contain only lowercase letters, digits, and hyphens (got {:?})",
        narrative.slug
    );
    let run_dir = Path::new(&narrative.run_dir);
    let record: RunRecord =
        serde_json::from_str(&std::fs::read_to_string(run_dir.join("run-record.json"))?)?;
    let score: Score =
        serde_json::from_str(&std::fs::read_to_string(run_dir.join("score.json"))?)?;

    let html = render_page(&narrative, &record, &score);
    let out = out.unwrap_or_else(|| PathBuf::from(format!("website/runs/{}.html", narrative.slug)));
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&out, html)?;

    let timelapse = run_dir.join("timelapse.mp4");
    if timelapse.exists() {
        std::fs::create_dir_all(assets_dir)?;
        std::fs::copy(&timelapse, assets_dir.join(format!("{}.mp4", narrative.slug)))?;
    }
    Ok(out)
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

    fn sample_score() -> Score {
        use crate::benchmark::record::ScoreNorms;
        Score {
            norm: ScoreNorms { congestion: 0.6476, money: 0.1239, changes: 0.6567 },
            weighted: ScoreNorms { congestion: 0.3886, money: 0.1752, changes: 0.0687 },
            invalid: false,
            flow_gain: 13.25,
            meters_reduction: 0.6380,
            junction_reduction: 0.6571,
            health: 1.0,
            score: 0.632437,
        }
    }

    fn sample_narrative() -> Narrative {
        Narrative {
            slug: "fable-5".into(),
            model_name: "Claude Fable 5".into(),
            map: "gridlock-v1".into(),
            run_dir: "benchmark/runs/20260612-121219".into(),
            verdict: "Cut jammed road by two-thirds with surgical upgrades.".into(),
            beat: vec![
                Beat { title: "Survey".into(), body: "Read the city before touching it.".into() },
                Beat { title: "Submit".into(), body: "Stepped the sim and submitted.".into() },
            ],
        }
    }

    #[test]
    fn render_page_includes_score_numbers_and_beats() {
        let html = render_page(&sample_narrative(), &sample_record(), &sample_score());
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("Claude Fable 5"));
        assert!(html.contains("0.63"));                       // composite score
        assert!(html.contains("Cut jammed road by two-thirds"));
        assert!(html.contains("assets/runs/fable-5.mp4"));    // hero video src
        assert!(html.contains("runs.css"));
        assert!(html.contains(">Survey<"));                   // beat title
        assert!(html.contains("Read the city before touching it."));
        assert!(html.contains("index.html#results"));         // back link
        // all four charts present
        assert_eq!(html.matches("class=\"chart-svg\"").count(), 4);
    }

    #[test]
    fn action_breakdown_groups_by_tool() {
        let svg = chart_action_breakdown(&sample_record());
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("bulldoze"));
        assert!(svg.contains("build_road"));
        assert!(svg.contains("upgrade_road"));
        // upgrade_road: 1 call, $1.18M in the sample
        assert!(svg.contains("$1.18M"));
    }

    #[test]
    fn cumulative_spend_labels_totals() {
        let svg = chart_cumulative_spend(&sample_record());
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("<polyline"));
        // total cost of the sample actions = 0 + 57,790 + 1,181,328
        assert!(svg.contains("$1.24M"));
        assert!(svg.contains("3 changes"));
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
    fn build_rejects_bad_slug() {
        let dir = std::env::temp_dir().join("skylinebench_page_test_badslug");
        std::fs::create_dir_all(&dir).unwrap();
        let toml_path = dir.join("bad.toml");
        std::fs::write(
            &toml_path,
            "slug = \"../evil\"\nmodel_name = \"X\"\nmap = \"m\"\nrun_dir = \"/nonexistent\"\nverdict = \"v\"\n",
        )
        .unwrap();
        let result = build(&toml_path, None, &dir);
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(msg.contains("slug"), "expected slug error, got: {msg}");
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
