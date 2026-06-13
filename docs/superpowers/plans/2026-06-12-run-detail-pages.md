# Run Detail Pages + Results Refresh Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Rust `build-page` broker subcommand that generates a static run-detail page (hero + 4 inline-SVG charts + curated narrative) from a run's `run-record.json`/`score.json` plus a curated narrative TOML, then wire the landing page's Results section to it (Fable 5, Opus 4.8, Haiku 4.5, Sonnet 4.5; Fable 5 scored and clickable).

**Architecture:** A new `broker/src/page.rs` module holds pure functions — narrative TOML types, four chart-SVG builders, and a `render_page` assembler — all unit-testable on in-memory structs. A thin `BuildPage` arm in `main.rs` does the file I/O (read inputs, write HTML, copy the timelapse asset). The site keeps its zero-runtime-dependency shape: charts are static inline SVG styled by a new `website/runs.css`, no JS chart library.

**Tech Stack:** Rust (clap, serde, serde_json, new `toml` dep), reusing `benchmark::record::{RunRecord, Score}`; static HTML/CSS for the site.

---

## File Structure

- `broker/Cargo.toml` — add `toml = "0.8"` dependency.
- `broker/src/lib.rs` — add `pub mod page;`.
- `broker/src/page.rs` — **new.** Narrative types, formatting helpers, four chart builders, `render_page`, and the `build` I/O entry point. Plus its unit tests.
- `broker/src/main.rs` — add the `BuildPage` subcommand variant + dispatch arm.
- `website/runs-src/fable-5.toml` — **new.** Curated narrative source for the Fable 5 run.
- `website/runs.css` — **new.** Detail-page styles (hero, stat chips, chart cards, SVG classes, timeline).
- `website/runs/fable-5.html` — **generated** by `build-page` (committed output).
- `website/assets/runs/fable-5.mp4` — **copied** by `build-page` from the run dir (committed output).
- `website/styles.css` — add `a.result-card` link + `.status-pill.scored` + scored-score styles.
- `website/index.html` — reorder/relabel the four result cards; make Fable 5 a scored link.

Reference run for all data/expected values: `benchmark/runs/20260612-121219` (model `fable`, score `0.632`).

---

## Task 1: Add `toml` dependency and the `page` module skeleton with narrative parsing

**Files:**
- Modify: `broker/Cargo.toml` (`[dependencies]`)
- Modify: `broker/src/lib.rs:12`
- Create: `broker/src/page.rs`

- [ ] **Step 1: Add the `toml` dependency**

In `broker/Cargo.toml`, under `[dependencies]`, after the `base64 = "0.22"` line add:

```toml
toml = "0.8"
```

- [ ] **Step 2: Register the module**

In `broker/src/lib.rs`, after the `pub mod timelapse;` line (line 12) add:

```rust
pub mod page;
```

- [ ] **Step 3: Write `page.rs` with the narrative types and a failing parse test**

Create `broker/src/page.rs`:

```rust
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
```

- [ ] **Step 4: Run the test to verify it passes (compiles + parses)**

Run: `cargo test --manifest-path broker/Cargo.toml page::tests::narrative_parses_from_toml`
Expected: PASS (`test page::tests::narrative_parses_from_toml ... ok`)

- [ ] **Step 5: Commit**

```bash
git add broker/Cargo.toml broker/Cargo.lock broker/src/lib.rs broker/src/page.rs
git commit -m "feat(page): narrative TOML model + toml dep"
```

---

## Task 2: Formatting helpers (`fmt_money`, `fmt_num`, `pct`, `esc`)

**Files:**
- Modify: `broker/src/page.rs`

- [ ] **Step 1: Write the failing tests**

In `broker/src/page.rs`, inside the existing `mod tests`, add:

```rust
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path broker/Cargo.toml page::tests::fmt`
Expected: FAIL — compile error, `cannot find function fmt_money in this scope`.

- [ ] **Step 3: Implement the helpers**

In `broker/src/page.rs`, after the `Narrative` struct (before `#[cfg(test)]`), add:

```rust
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path broker/Cargo.toml page::tests::fmt && cargo test --manifest-path broker/Cargo.toml page::tests::pct && cargo test --manifest-path broker/Cargo.toml page::tests::esc`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/page.rs
git commit -m "feat(page): number/percent/escape formatting helpers"
```

---

## Task 3: Chart 1 — before → after bars

**Files:**
- Modify: `broker/src/page.rs`

- [ ] **Step 1: Write the failing test**

Add to `mod tests`:

```rust
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
    fn before_after_has_metric_rows_and_values() {
        let svg = chart_before_after(&sample_record());
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("Population"));
        assert!(svg.contains("31,640"));   // baseline population
        assert!(svg.contains("71"));        // final flow, rounded
        assert!(svg.contains("class=\"c-final\""));
        assert!(svg.contains("class=\"c-base\""));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --manifest-path broker/Cargo.toml page::tests::before_after`
Expected: FAIL — `cannot find function chart_before_after`.

- [ ] **Step 3: Implement `chart_before_after`**

Add to `page.rs` (after the helpers):

```rust
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
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --manifest-path broker/Cargo.toml page::tests::before_after`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/page.rs
git commit -m "feat(page): before/after metrics chart"
```

---

## Task 4: Chart 2 — flow settling curves

**Files:**
- Modify: `broker/src/page.rs`

- [ ] **Step 1: Write the failing test**

Add to `mod tests`:

```rust
    #[test]
    fn flow_settling_has_two_lines() {
        let svg = chart_flow_settling(&sample_record());
        assert!(svg.starts_with("<svg"));
        assert_eq!(svg.matches("<polyline").count(), 2);
        assert!(svg.contains("class=\"c-line-base\""));
        assert!(svg.contains("class=\"c-line-final\""));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --manifest-path broker/Cargo.toml page::tests::flow_settling`
Expected: FAIL — `cannot find function chart_flow_settling`.

- [ ] **Step 3: Implement `chart_flow_settling` and its `polyline` helper**

Add to `page.rs`:

```rust
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
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --manifest-path broker/Cargo.toml page::tests::flow_settling`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/page.rs
git commit -m "feat(page): flow settling curves chart"
```

---

## Task 5: Chart 3 — cumulative spend

**Files:**
- Modify: `broker/src/page.rs`

- [ ] **Step 1: Write the failing test**

Add to `mod tests`:

```rust
    #[test]
    fn cumulative_spend_labels_totals() {
        let svg = chart_cumulative_spend(&sample_record());
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("<polyline"));
        // total cost of the sample actions = 0 + 57,790 + 1,181,328
        assert!(svg.contains("$1.24M"));
        assert!(svg.contains("3 changes"));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --manifest-path broker/Cargo.toml page::tests::cumulative_spend`
Expected: FAIL — `cannot find function chart_cumulative_spend`.

- [ ] **Step 3: Implement `chart_cumulative_spend`**

Add to `page.rs`:

```rust
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
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --manifest-path broker/Cargo.toml page::tests::cumulative_spend`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/page.rs
git commit -m "feat(page): cumulative spend chart"
```

---

## Task 6: Chart 4 — action-type breakdown

**Files:**
- Modify: `broker/src/page.rs`

- [ ] **Step 1: Write the failing test**

Add to `mod tests`:

```rust
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
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --manifest-path broker/Cargo.toml page::tests::action_breakdown`
Expected: FAIL — `cannot find function chart_action_breakdown`.

- [ ] **Step 3: Implement `group_actions` + `chart_action_breakdown`**

Add to `page.rs`:

```rust
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
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --manifest-path broker/Cargo.toml page::tests::action_breakdown`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/page.rs
git commit -m "feat(page): action-type breakdown chart"
```

---

## Task 7: `render_page` — assemble the full HTML document

**Files:**
- Modify: `broker/src/page.rs`

- [ ] **Step 1: Write the failing test**

Add to `mod tests`:

```rust
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
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --manifest-path broker/Cargo.toml page::tests::render_page`
Expected: FAIL — `cannot find function render_page`.

- [ ] **Step 3: Implement `render_page`**

Add to `page.rs`. Builds the key-numbers chips and the chart grid from the four chart functions, then the document. Paragraphs in a beat body are split on blank lines.

```rust
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
pub fn render_page(n: &Narrative, r: &RunRecord, s: &Score) -> String {
    let b = &r.baseline;
    let f = &r.final_stats;
    let chips = [
        chip("flow", &format!("{} → {}", fmt_num(b.flow_mean), fmt_num(f.flow_mean))),
        chip("congested metres", &pct(b.congested_meters, f.congested_meters)),
        chip("jammed junctions", &format!("{} → {}", b.congested_junctions, f.congested_junctions)),
        chip("population", &format!("{} → {}", fmt_num(b.population as f64), fmt_num(f.population as f64))),
        chip("changes", &r.tally.num_changes.to_string()),
        chip("spent", &fmt_money(r.tally.money_spent)),
    ]
    .join("");
    let charts = [
        chart_card("Before → after", chart_before_after(r)),
        chart_card("Flow settling", chart_flow_settling(r)),
        chart_card("Cumulative spend", chart_cumulative_spend(r)),
        chart_card("Actions by type", chart_action_breakdown(r)),
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
  (async function () {{
    const v = document.querySelector('video[data-video-src]');
    if (!v) return;
    const src = v.dataset.videoSrc;
    try {{ const res = await fetch(src, {{ method: 'HEAD' }}); if (!res.ok) return; }} catch (e) {{ return; }}
    const s = document.createElement('source'); s.src = src; s.type = 'video/mp4';
    v.appendChild(s); v.load();
    const ph = v.parentElement.querySelector('.media-placeholder');
    v.addEventListener('loadeddata', () => {{ if (v.videoWidth > 0 && ph) ph.style.display = 'none'; }});
    if ('IntersectionObserver' in window) {{
      new IntersectionObserver((es) => es.forEach((e) => e.isIntersecting ? v.play().catch(() => {{}}) : v.pause()), {{ threshold: 0.3 }}).observe(v);
    }} else {{ v.play().catch(() => {{}}); }}
  }})();
</script>
</body>
</html>
"##,
        model = esc(&n.model_name),
        map = esc(&n.map),
        score = s.score,
        verdict = esc(&n.verdict),
        slug = esc(&n.slug),
        chips = chips,
        charts = charts,
        beats = beats_html(&n.beat),
    )
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --manifest-path broker/Cargo.toml page::tests::render_page`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/page.rs
git commit -m "feat(page): assemble full run-detail HTML document"
```

---

## Task 8: `build` I/O entry point + `BuildPage` subcommand

**Files:**
- Modify: `broker/src/page.rs`
- Modify: `broker/src/main.rs`

- [ ] **Step 1: Implement the `build` entry point**

Add to `broker/src/page.rs` (outside `mod tests`), after `render_page`:

```rust
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
```

- [ ] **Step 2: Add the `BuildPage` subcommand variant**

In `broker/src/main.rs`, inside `enum Command` (after the `BenchmarkFinalize { .. }` variant, before the closing `}` at line 75) add:

```rust
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
```

- [ ] **Step 3: Add the dispatch arm**

In `broker/src/main.rs`, in the `match cli.command` block, after the `Command::BenchmarkFinalize { .. } => { .. }` arm (ends ~line 226, before the closing `}` of the match) add:

```rust
        Command::BuildPage { narrative, out, assets_dir } => {
            let written = skylinebench::page::build(&narrative, out, &assets_dir)?;
            eprintln!("build-page: wrote {}", written.display());
        }
```

- [ ] **Step 4: Build to verify it compiles**

Run: `cargo build --manifest-path broker/Cargo.toml`
Expected: builds clean (warnings ok), no errors.

- [ ] **Step 5: Commit**

```bash
git add broker/src/page.rs broker/src/main.rs
git commit -m "feat(page): build-page subcommand wiring"
```

---

## Task 9: Detail-page stylesheet (`runs.css`)

**Files:**
- Create: `website/runs.css`

- [ ] **Step 1: Write `website/runs.css`**

Create the file. Reuses existing CSS variables from `colors_and_type.css`; the SVG `class` names match those emitted by the chart functions.

```css
/* Run-detail page styles. Variables come from colors_and_type.css. */

.run-hero { padding: clamp(90px, 14vw, 150px) 0 40px; }
.run-hero .display { margin: 10px 0 14px; }
.run-score { display: flex; align-items: baseline; gap: 10px; margin-bottom: 14px; }
.run-score .rs-val { font-family: var(--font-heading); font-size: clamp(2.6rem, 7vw, 4rem); font-weight: 600; letter-spacing: -0.03em; color: var(--skl-blue-bright, #5b9dff); font-variant-numeric: tabular-nums; }
.run-score .rs-of { font-family: var(--font-mono); font-size: 14px; color: var(--muted-foreground); }

.chips { display: flex; flex-wrap: wrap; gap: 10px; margin-bottom: 40px; }
.chip { display: flex; flex-direction: column; gap: 2px; padding: 12px 16px; border-radius: 12px; background: var(--card); box-shadow: 0 0 0 1px color-mix(in oklab, var(--foreground) 10%, transparent); min-width: 96px; }
.chip-v { font-family: var(--font-heading); font-size: 1.15rem; font-weight: 600; font-variant-numeric: tabular-nums; }
.chip-l { font-family: var(--font-mono); font-size: 11px; color: var(--muted-foreground); }

.chart-grid { display: grid; grid-template-columns: repeat(2, 1fr); gap: 18px; }
@media (max-width: 720px) { .chart-grid { grid-template-columns: 1fr; } }
.chart-card { margin: 0; padding: 18px 20px 20px; border-radius: 14px; background: var(--card); box-shadow: 0 0 0 1px color-mix(in oklab, var(--foreground) 10%, transparent); }
.chart-card figcaption { font-family: var(--font-heading); font-weight: 600; font-size: 0.95rem; margin-bottom: 12px; }
.chart-svg { width: 100%; height: auto; overflow: visible; }

/* SVG element classes (styled here so the inline SVG stays data-only) */
.c-base { fill: color-mix(in oklab, var(--foreground) 28%, transparent); }
.c-final { fill: var(--skl-blue-bright, #5b9dff); }
.c-line-base { fill: none; stroke: color-mix(in oklab, var(--foreground) 30%, transparent); stroke-width: 2; }
.c-line-final { fill: none; stroke: var(--skl-blue-bright, #5b9dff); stroke-width: 2.5; }
.c-axis { fill: var(--muted-foreground); font-family: var(--font-mono); font-size: 11px; }
.c-val { fill: var(--muted-foreground); font-family: var(--font-mono); font-size: 10.5px; }
.c-val-final { fill: var(--foreground); }

.timeline { list-style: none; margin: 28px 0 0; padding: 0; counter-reset: beat; }
.beat { position: relative; padding: 0 0 26px 36px; border-left: 2px solid var(--border); }
.beat:last-child { border-left-color: transparent; }
.beat::before { counter-increment: beat; content: counter(beat); position: absolute; left: -15px; top: -2px; width: 28px; height: 28px; border-radius: 50%; display: grid; place-items: center; background: var(--card); box-shadow: 0 0 0 1px var(--border); font-family: var(--font-mono); font-size: 12px; color: var(--muted-foreground); }
.beat h3 { font-family: var(--font-heading); font-size: 1.05rem; font-weight: 600; margin: 0 0 6px; }
.beat p { color: var(--muted-foreground); margin: 0 0 8px; }
```

- [ ] **Step 2: Commit**

```bash
git add website/runs.css
git commit -m "feat(website): run-detail page stylesheet"
```

---

## Task 10: Author the Fable 5 narrative and generate the page

**Files:**
- Create: `website/runs-src/fable-5.toml`
- Generate: `website/runs/fable-5.html`, `website/assets/runs/fable-5.mp4`

- [ ] **Step 1: Read the run transcript for narrative detail**

Read `benchmark/runs/20260612-121219/transcript.md` (it is large — skim the agent's reasoning at the start, around its first batch of edits, around its sim-stepping, and at submit). Note the phases the agent actually went through to ground the beats below in what it really did. The hard numbers are fixed: baseline flow 57→71, congested metres 5,122→1,854 (−64%), jammed junctions 35→12, population 31,640→31,174, 197 changes (6 bulldoze, 11 build_road, 180 upgrade_road), $1.24M spent, composite 0.63, health 1.0.

- [ ] **Step 2: Write `website/runs-src/fable-5.toml`**

Create the file. Refine the `body` prose against what the transcript shows, keeping these data-true anchors:

```toml
slug = "fable-5"
model_name = "Claude Fable 5"
map = "gridlock-v1"
run_dir = "benchmark/runs/20260612-121219"
verdict = "Fable 5 treated gridlock-v1 as a capacity problem and fixed it almost entirely by upgrading existing roads rather than rebuilding — cutting jammed road-metres by 64% and jammed junctions from 35 to 12 while holding population essentially flat. Composite 0.63."

[[beat]]
title = "Survey the city"
body = """
Before changing anything, the agent pulled the city overview, the metrics, and a rendered map, and traced where traffic was actually backing up. The baseline window measured 57 mean flow, 5,122 metres of congested road, and 35 jammed junctions across a population of ~31,640.
"""

[[beat]]
title = "Upgrade, don't rebuild"
body = """
The plan leaned overwhelmingly on widening what was already there: 180 road upgrades against just 11 new road segments and 6 bulldozes. Most of the $1.24M spend went into upgrades ($1.18M), targeting the worst junctions rather than re-laying the network.
"""

[[beat]]
title = "Step the sim and let it settle"
body = """
Rather than reacting to the first post-change numbers, the agent stepped time forward and let traffic re-route. The flow settling window climbed from the mid-50s into the low-70s as cars found the widened corridors.
"""

[[beat]]
title = "Submit"
body = """
The agent submitted with congested metres down to 1,854 (−64%) and jammed junctions down to 12. Population held at ~31,174, so the health factor stayed at 1.0 and none of the congestion gains were clawed back. Final composite: 0.63.
"""
```

- [ ] **Step 3: Generate the page**

From the repo root:

Run: `cargo run --manifest-path broker/Cargo.toml -- build-page --narrative website/runs-src/fable-5.toml`
Expected: prints `build-page: wrote website/runs/fable-5.html`, and creates `website/assets/runs/fable-5.mp4`.

- [ ] **Step 4: Verify the generated artifacts**

Run: `test -f website/runs/fable-5.html && test -f website/assets/runs/fable-5.mp4 && grep -c "chart-svg" website/runs/fable-5.html`
Expected: prints `4` (no error), confirming both files exist and all four charts rendered.

- [ ] **Step 5: Commit**

```bash
git add website/runs-src/fable-5.toml website/runs/fable-5.html website/assets/runs/fable-5.mp4
git commit -m "feat(website): Fable 5 run-detail page + narrative"
```

---

## Task 11: Refresh the Results section on `index.html`

**Files:**
- Modify: `website/styles.css`
- Modify: `website/index.html:392-480` (the four result cards)

- [ ] **Step 1: Add card-link + scored styles to `styles.css`**

In `website/styles.css`, immediately after the `.result-card { ... }` rule (ends line 337), add:

```css
a.result-card { text-decoration: none; color: inherit; transition: transform 0.15s ease, box-shadow 0.15s ease; }
a.result-card:hover { transform: translateY(-3px); box-shadow: 0 8px 30px color-mix(in oklab, var(--foreground) 16%, transparent); }
.status-pill.scored { color: var(--skl-blue-bright, #5b9dff); border-color: color-mix(in oklab, var(--skl-blue-bright, #5b9dff) 40%, transparent); }
.result-score .val.scored { color: var(--foreground); }
```

- [ ] **Step 2: Replace the four result cards**

In `website/index.html`, replace the entire block from `<!-- Card 1 -->` through the close of `<!-- Card 4 -->`'s `</article>` (lines 393–479) with the following. Card 1 (Fable 5) is an `<a>` link with the real score; the rest stay `pending` `<article>`s with updated names and video slugs:

```html
      <!-- Card 1 -->
      <a class="result-card reveal" href="runs/fable-5.html">
        <div class="result-thumb">
          <video muted loop playsinline preload="none" poster="" data-video-src="assets/runs/fable-5.mp4"></video>
          <div class="ph"><svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg><span>timelapse</span></div>
        </div>
        <div class="result-body">
          <div class="result-top">
            <div class="result-model">
              <span class="mico"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2 2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5"/></svg></span>
              <span class="name">Claude Fable 5<small>gridlock-v1</small></span>
            </div>
            <span class="status-pill scored">view run →</span>
          </div>
          <div class="result-score">
            <span class="val scored">0.63</span>
            <span class="of">/ 1.00</span>
            <span class="lbl">composite score</span>
          </div>
        </div>
      </a>

      <!-- Card 2 -->
      <article class="result-card reveal">
        <div class="result-thumb">
          <video muted loop playsinline preload="none" poster="" data-video-src="assets/runs/opus-4-8.mp4"></video>
          <div class="ph"><svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg><span>timelapse</span></div>
        </div>
        <div class="result-body">
          <div class="result-top">
            <div class="result-model">
              <span class="mico"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2 2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5"/></svg></span>
              <span class="name">Claude Opus 4.8<small>gridlock-v1</small></span>
            </div>
            <span class="status-pill">pending</span>
          </div>
          <div class="result-score">
            <span class="val">·</span>
            <span class="of">/ 1.00</span>
            <span class="lbl">composite score</span>
          </div>
        </div>
      </article>

      <!-- Card 3 -->
      <article class="result-card reveal">
        <div class="result-thumb">
          <video muted loop playsinline preload="none" poster="" data-video-src="assets/runs/haiku-4-5.mp4"></video>
          <div class="ph"><svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg><span>timelapse</span></div>
        </div>
        <div class="result-body">
          <div class="result-top">
            <div class="result-model">
              <span class="mico"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2 2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5"/></svg></span>
              <span class="name">Claude Haiku 4.5<small>gridlock-v1</small></span>
            </div>
            <span class="status-pill">pending</span>
          </div>
          <div class="result-score">
            <span class="val">·</span>
            <span class="of">/ 1.00</span>
            <span class="lbl">composite score</span>
          </div>
        </div>
      </article>

      <!-- Card 4 -->
      <article class="result-card reveal">
        <div class="result-thumb">
          <video muted loop playsinline preload="none" poster="" data-video-src="assets/runs/sonnet-4-5.mp4"></video>
          <div class="ph"><svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg><span>timelapse</span></div>
        </div>
        <div class="result-body">
          <div class="result-top">
            <div class="result-model">
              <span class="mico"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2 2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5"/></svg></span>
              <span class="name">Claude Sonnet 4.5<small>gridlock-v1</small></span>
            </div>
            <span class="status-pill">pending</span>
          </div>
          <div class="result-score">
            <span class="val">·</span>
            <span class="of">/ 1.00</span>
            <span class="lbl">composite score</span>
          </div>
        </div>
      </article>
```

- [ ] **Step 3: Verify the edits**

Run: `grep -c "result-card" website/index.html && grep -o "Claude [A-Za-z]* [0-9.]*" website/index.html`
Expected: `5` (4 cards + the `a.result-card` mention is in styles.css not index, so this counts the 4 card class uses plus none extra — confirm it is `4`); model names list **Claude Fable 5, Claude Opus 4.8, Claude Haiku 4.5, Claude Sonnet 4.5** in that order.

> Note: if the count differs, recount — there should be exactly 4 `result-card` occurrences in `index.html`.

- [ ] **Step 4: Commit**

```bash
git add website/index.html website/styles.css
git commit -m "feat(website): results cards → Fable 5 (scored, linked), Opus 4.8, Haiku 4.5, Sonnet 4.5"
```

---

## Task 12: Full test + manual verification

**Files:** none (verification only)

- [ ] **Step 1: Run the full broker test suite**

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: all tests pass, including the new `page::tests::*`.

- [ ] **Step 2: Manual browser check**

Open `website/index.html` in a browser. Confirm:
- The Results cards read Fable 5, Opus 4.8, Haiku 4.5, Sonnet 4.5 in order.
- The Fable 5 card shows `0.63`, a blue "view run →" pill, lifts on hover, and its timelapse plays.
- The other three cards show `·` / `pending` and are not clickable.

Then open `website/runs/fable-5.html`. Confirm:
- Hero shows "Claude Fable 5", `0.63 / 1.00`, the verdict, and the timelapse plays.
- The key-numbers chips read flow 57 → 71, congested metres −64%, jammed junctions 35 → 12, population 31,640 → 31,174, 197 changes, $1.24M.
- All four charts render in a 2×2 grid with bars/lines visible.
- The narrative timeline shows the beats in order.
- "← Back to results" returns to `index.html#results`.

- [ ] **Step 3: Final commit (if any verification tweaks were needed)**

```bash
git add -A
git commit -m "fix(website): run-detail page verification tweaks"
```

(Skip if nothing changed.)

---

## Self-Review Notes

- **Spec coverage:** results refresh + ordering (Task 11), Fable 0.63 scored/clickable (Tasks 10–11), detail page hero/chips/charts/narrative/footer (Task 7), four charts (Tasks 3–6), Rust `build-page` subcommand reading run-record/score + narrative TOML and copying the timelapse (Tasks 1, 8, 10), `runs.css` (Task 9), text-only narrative + 2×2 grid (Tasks 7, 9), testing (Tasks 1–7, 12). All spec sections map to tasks.
- **Type consistency:** `Narrative`/`Beat` fields, `RunRecord`/`Score` (reused from `benchmark::record`), and function names (`chart_before_after`, `chart_flow_settling`, `chart_cumulative_spend`, `chart_action_breakdown`, `group_actions`, `render_page`, `build`) are used identically across tasks. SVG `class` names (`c-base`, `c-final`, `c-line-base`, `c-line-final`, `c-axis`, `c-val`, `c-val-final`, `chart-svg`) match between the chart builders (Tasks 3–6) and `runs.css` (Task 9).
- **No placeholders:** every code/edit step contains complete content; the only judgement call is refining the Fable narrative prose against the transcript in Task 10, which ships with complete, data-true default text.
```
