# SkylineBench: Congestion-Based Scoring, Consequence Observability, and Build Validation

Date: 2026-06-10
Status: Approved (design review with operator)
Supersedes: scoring section (§4) and guard (§4.3) of `2026-06-09-skylinebench-phase2-benchmark-design.md`. All other phase-2 behavior is unchanged.

## 1. Motivation & evidence

Two experiments on the `gridlock-v1` map (save `BasicTrafficScenario.crp`, game 1.21.1-f9) drove this design.

**Run `20260610-150754` (agent run, score 0, invalid).** The agent advanced ~35,100 ticks (~60 in-game days) and made 72 changes, yet windowed `flow_percent` never left the 50–57 band during the agent phase. Population collapsed 31,552 → 15,585 mid-run → 110 after settle. Cause: the agent upgraded zoned streets to `Medium Road Elevated`, `Large Oneway Elevated`, `Highway Elevated`, and `Highway` — road types with no zoning support. Every building fronting those segments lost road access and abandoned. The final flow of 90% was an empty network. The agent received no signal about any of this until scoring.

**Null-control (60 in-game days, zero changes, `benchmark/experiments/null_control.py`).**

| Signal | Observed behavior | Implication |
| --- | --- | --- |
| Population | 31.4k → 32.2k → 28.0k (natural lifecycle dip, days 25–40) → recovering | Map is stable; no inherent death spiral. Collapse in agent runs is agent-caused. |
| `flow_percent` (global) | 55–67, no trend, ±5–6 day-to-day noise | Too noisy and insensitive to be the primary score: noise is ~15% of the +40-point target. |
| Active vehicles | 1,852–2,195; dipped below 0.9 × the agent run's baseline mean with **zero changes** | Current 0.9 vehicle guard can invalidate an honest run on timing luck. |
| Worst-decile segment density | 0.85–0.91, ±0.03, stable across the lifecycle dip | Per-segment density is a low-noise, meaningful congestion signal. |

Conclusions: (a) score congestion directly from segment data, not global flow; (b) the agent needs to *see* consequences (abandonment, population) while they are recoverable, but as neutral facts — the benchmark tests decision quality, not compliance with warnings; (c) builds currently bypass the game's own validation, so the agent neither faces vanilla constraints nor learns why an action is impossible.

## 2. Scoring

### 2.1 Congestion metric

- Mod: add `length` (from `NetSegment.m_averageLength`) to each `segment_loads` entry in `/metrics`. Density stays as-is: `min(1, m_trafficDensity / 100)`.
- Broker, per measurement window (baseline and final): for each road segment, take the mean density across the window's 8 samples, then

  `congested_meters = Σ length(segment) for segments with window-mean density ≥ congestion_threshold` (default **0.7**, in `BenchConfig`).

- Segments are keyed by id within a window. A segment absent from a sample (bulldozed/replaced mid-window) contributes only the samples where it exists; windows run after the agent phase, so this is an edge case, not the norm.

### 2.2 Score formula

```
norm_congestion = clamp((baseline_cm − final_cm) / baseline_cm, 0, 1)
norm_money      = clamp(money_spent / budget, 0, 1)        # unchanged, 10M
norm_changes    = clamp(num_changes / change_cap, 0, 1)    # unchanged, 300

score = 0.60 * norm_congestion
      + 0.20 * (1 − norm_money)
      + 0.20 * (1 − norm_changes)
```

- Self-normalizing per map; the `target_gain = 40` flow constant is retired.
- Absolute congested meters, not share of total road length: building unused roads cannot dilute the metric.
- If `baseline_cm == 0` the map is unsuitable; the run aborts at baseline with a config error rather than producing a meaningless score.
- `Δflow` (baseline vs final `flow_percent` means) stays in `run-record.json` and `score.json` as an unweighted diagnostic.

### 2.3 Guard

- Replace the active-vehicles guard with population: **invalid (score 0) if `final_population < 0.8 × baseline_population`**.
- Rationale: null-run population floor was ~87% of peak under zero intervention, so 0.8 tolerates natural lifecycle waves; vehicles dipped below the old 0.9 line on their own. Population is also the direct anti-cheat target (the vehicle guard existed to stop depopulation cheese).
- Baseline/final population are already recorded per window; the guard compares those values.

### 2.4 End conditions

- Early-success: rolling windowed `congested_meters ≤ 0.05 × baseline_cm` replaces the flow ≥ 95 target. The rolling window mirrors the current flow window (8 samples, fed on each `control_time` step and `get_metrics` call).
- `submit_solution` and the 3h wall-clock cap are unchanged.

## 3. Consequence observability (neutral facts, no coaching)

- `get_metrics` services group gains `abandoned_buildings`: count of buildings with `Building.Flags.Abandoned` (single pass over the building buffer, same pattern as existing reads).
- `benchmark_progress` (returned on every `control_time` step) gains `population`, `abandoned_buildings`, and `congested_meters_current` (rolling-window mean). The agent passively sees the scored metric and the city-health trend each step; interpreting them is its job.
- Mutation results (`build_road`, `upgrade_road`, `bulldoze`, and per-op in `apply_plan`, including `validate_only`) gain `zoned_buildings_fronting: N` — the count of buildings whose access edge is the affected segment(s). Factual, no advice attached.

## 4. Build validation & error fidelity

Today `build_road` calls `NetManager.CreateSegment` directly — the low-level constructor that bypasses NetTool validation. Builds the game UI would refuse (through buildings, too steep, out of area) silently succeed; the failures that do occur are mislabeled (node-create failure → `UNKNOWN`, any `CreateSegment` failure → `COLLISION`).

- Before creating, run the game's own placement validation in test mode and refuse the build if it fails, mirroring vanilla rules. Primary candidate API: the `NetTool`/`NetAI` test path returning `ToolBase.ToolErrors`; if that proves awkward on 1.21, fall back to `NetAI.CheckBuildPosition`. Confirm the exact signature against the game DLLs (monodis, as done for `SaveLoader`).
- Expand `ErrorCode` to map ToolErrors distinctly: `OBJECT_COLLISION`, `SLOPE_TOO_STEEP`, `OUT_OF_AREA`, `HEIGHT_LIMIT`, `TOO_MANY_CONNECTIONS`, plus `NET_BUFFER_FULL` for genuine buffer exhaustion. `COLLISION` is only returned when validation says so. Where the collision is with buildings, include the colliding building ids (or count, if ids are impractical).
- Consequence: a road through a building now *fails with the reason*; the agent must bulldoze first — an explicit, costed, visible decision.
- `apply_plan` with `validate_only: true` runs the same checks per op, making the free dry-run a genuine collision/feasibility test.
- `upgrade_road` gets the same validation where applicable (e.g. elevated replacement colliding with structures).

## 5. Minor changes

- `max_step_days`: 3 → 7. Ergonomics only; the chunked-step machinery and `partial: true` responses already handle wall-clock limits.
- `benchmark/prompt.md`: describe the congestion metric (what `congested_meters` is, threshold, normalization), the population guard, and the new observable fields. Neutral wording — no road-type coaching.

## 6. Config summary (`BenchConfig`)

| Field | Old | New |
| --- | --- | --- |
| `w_flow` | 0.6 | removed (flow diagnostic only) |
| `w_congestion` | — | 0.6 |
| `congestion_threshold` | — | 0.7 |
| `target_gain` | 40.0 | removed |
| `flow_target` | 95.0 | replaced by `congestion_end_ratio = 0.05` |
| `guard_ratio` (vehicles) | 0.9 | replaced by `pop_guard_ratio = 0.8` |
| `max_step_days` | 3 | 7 |

`w_money`, `w_changes`, `budget`, `change_cap`, window/settle ticks, wall-clock cap: unchanged.

## 7. Validation plan

1. **Null control re-run** with the new metric: expect substantial, stable `baseline_cm` (worst-decile density was 0.85–0.91 for 60 days) and a near-zero norm_congestion for a do-nothing run; expect the population guard NOT to trip.
2. **Scripted known-good fix**: apply a hand-authored improvement to the gridlock map, verify `congested_meters` responds and the score moves.
3. **Validation-path checks**: attempt a build through a building, a too-steep build, and an out-of-area build; each must fail with its specific code. Same plan via `apply_plan validate_only` must report the failures without spending changes.
4. **Full benchmark run** end-to-end on the new scoring.

## 8. Out of scope (future phases)

- Service/happiness tools for the agent (deliberately deferred; consequences are observable but not yet fixable beyond roads/zoning).
- Multi-map calibration of `congestion_threshold`.
- Refund model for bulldozing.
