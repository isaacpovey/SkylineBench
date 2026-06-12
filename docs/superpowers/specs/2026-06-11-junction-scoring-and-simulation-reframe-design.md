# Junction-aware scoring + "optimise the simulation" reframe

**Date:** 2026-06-11
**Status:** approved (design)

## Motivation

Two completed runs exposed the same pathology from different angles:

- Run `20260611-142831` (score 0.53): the agent only widened roads and never
  reworked intersections, because `congested_meters` (total length of segments
  at density ≥ 0.7) rewards spreading volume over more lanes along long
  stretches far more than fixing a geometrically-broken junction.
- Run `20260611-181751` (score 0.30): the agent's reasoning was saturated with
  formula-gaming — *"<80% = zero score," "safely valid ~92%," "each change
  carries a real score penalty"* — and it discovered that depopulation reduces
  congestion: *"congestion is tracking population almost 1:1 ... the apparent
  gain was the population dip."* It converted in-city arterials to limited-access
  Four Lane Highway, stripped building frontage, and nearly drove the city into
  a death spiral (population 32k→28k, ~84% of baseline) for a congestion gain
  that wasn't durable.

Both point at the scoring metric and at the fact that the agent optimises the
*scoreboard* rather than the *city*. This redesign:

1. adds a **congested-junction** signal so reworking intersections is rewarded,
   blended with the existing length signal;
2. makes the score **robust to depopulation** via a graded population-health
   factor that replaces the hard 80% cliff;
3. **hides the scoring formula** from the agent and reframes the task as
   *optimising a city traffic simulation* (not a benchmark), so the agent
   pursues genuine objectives instead of gaming a known boundary;
4. **splits the revert guidance** into fast (traffic re-routing) vs slow
   (livability decay) regimes — the previous single rule encouraged waiting
   out a multi-day settle while gradual abandonment compounded.

The score is still computed externally, so hiding the formula does not affect
reproducibility or fairness — it only removes the gaming incentive.

## Decisions (locked during brainstorm)

- Junction definition: a **node of degree ≥ 3** (a real intersection; the 200 m
  auto-split creates degree-2 nodes that are not intersections) is **congested**
  when **≥ 2 of its incident segments** are at/above the density threshold over
  the measurement window. The "≥ 2" captures an intersection that is itself a
  chokepoint (multiple jammed approaches), not a single congested road passing
  through. Tunable.
- Combine length + junctions by **blending inside one congestion term**
  (`a·meters_reduction + b·junction_reduction`, `a + b = 1`, start 0.5/0.5),
  keeping the existing 0.6 / 0.2 / 0.2 (congestion / money / changes) structure.
- Depopulation handled by a **graded, multiplicative population-health factor**
  that replaces the hard 80%-or-zero cliff (not a separate additive term, not
  per-capita normalisation).
- Agent is told the objectives **qualitatively**, never the formula, weights,
  caps, or thresholds; the task is framed as a **simulation**, not a benchmark.
- **No mod/C# change** — junctions are derived broker-side from existing
  `network()` topology joined with `metrics()` segment densities.

## Scoring function (hidden from the agent)

```
congestion_reward = a·meters_reduction + b·junction_reduction         (a + b = 1, start 0.5/0.5)

  meters_reduction   = clamp01((base_meters    − final_meters)    / base_meters)
  junction_reduction = clamp01((base_junctions − final_junctions) / base_junctions)

score = (0.6·congestion_reward + 0.2·(1 − money/budget) + 0.2·(1 − changes/cap)) · health
```

- **Junction reduction edge case:** if `base_junctions == 0`, set `a = 1`
  (meters only) so the term is well-defined.
- **Population health** (replaces the hard cliff): linear ramp on the population
  ratio `r = final_population / base_population`:
  - `health = 1.0` for `r ≥ health_full` (0.95)
  - `health = 0.0` for `r ≤ health_zero` (0.75)
  - linear in between: `health = clamp01((r − health_zero) / (health_full − health_zero))`
- **Invalid (score 0) cases:** `base_meters ≤ 0` (nothing to fix) keeps zeroing
  the run. Total population collapse now reaches `score ≈ 0` smoothly via
  `health → 0` rather than a discontinuous guard.
- **Sanity checks the new function must satisfy** (tests):
  - Do-nothing (no changes, population stable) ≈ 0.4, as today.
  - Clearing all congestion cheaply with a stable population ≈ 1.0.
  - The `20260611-181751` shape — ~16% population loss for a small, non-durable
    congestion gain — nets **below** do-nothing (health ≈ 0.4 → score well under
    0.4).
  - Worsened congestion still clamps the congestion term to 0, not negative.

### New / changed config constants (`config.rs`)

| Constant | Value | Meaning |
|---|---|---|
| `blend_meters` (a) | 0.5 | weight on length-reduction inside the congestion term |
| `blend_junctions` (b) | 0.5 | weight on junction-reduction inside the congestion term |
| `junction_min_degree` | 3 | minimum node degree to count as an intersection |
| `junction_min_congested` | 2 | min incident segments ≥ threshold for a junction to be "congested" |
| `health_full` | 0.95 | population ratio at/above which health = 1.0 |
| `health_zero` | 0.75 | population ratio at/below which health = 0.0 |
| `pop_guard_ratio` | removed | superseded by the graded health factor |

## Junction metric (broker, no mod change)

- A junction count is computed from **network topology** (`Network` — nodes plus
  segments carrying `start_node` / `end_node`) joined with **per-segment mean
  density** over a window (`WindowAccum`) by shared `segment_id`.
- For each node: degree = count of incident segments; congested-approaches =
  count of incident segments whose window-mean density ≥ threshold. A node is a
  **congested junction** when `degree ≥ junction_min_degree` and
  `congested_approaches ≥ junction_min_congested`.
- `measure_window` (`measure.rs`) fetches `client.network()` once per window
  (topology is static while the sim is paused/settling) and computes
  `congested_junctions` from the window's mean densities.
- **Live readout:** `RunState` caches the most recent `network()` so the rolling
  `city_status` junction count can be recomputed when metrics are observed; the
  authoritative count for scoring is the measurement-window one.

## Agent-facing reframe (`benchmark/prompt.md`)

Full rewrite:

- **Framing:** "You are a traffic engineer optimising this city's traffic in a
  simulation." No benchmark/competition language, no formula, no weights, no
  numeric caps or thresholds.
- **Objectives (qualitative):**
  - Reduce traffic congestion — both congested road length and congested
    junctions (intersections where traffic backs up).
  - Keep the city healthy — residents must stay, stay employed, stay reasonably
    happy; an emptying city is a failure, not a shortcut.
  - Be cost-effective and avoid needless disruption — but don't be afraid to
    tear out and rebuild when that's what fixes the problem; bold structural
    change is fine as long as it won't send the city into a death spiral.
- **`city_status` block** (renamed from `benchmark_progress`):
  - Add: `congested_junctions` (now / at-start).
  - Keep: `congested_road_meters` (now / at-start), `population`, `happiness`,
    `abandoned_buildings`, `traffic_flow`, `money_spent`, `changes_made`,
    `time_remaining`.
  - Drop: `congested_meters_target` (benchmark artifact that leaks goal shape).
- **Done condition:** work until further changes won't sustainably reduce
  congestion, then `submit_solution`; a limited time budget applies. Keep submit
  + wall-clock timeout; **drop the auto-stop-at-5%-of-baseline** end condition.
- **Revert guidance — fast vs slow:**
  - Traffic re-routing: congestion can spike then settle over several in-game
    days after a structural change; let it settle before judging or reverting.
  - Livability decay: if population/happiness is sliding or abandonment climbing,
    that is not a settling transient — it compounds; act immediately to find and
    undo the cause rather than waiting out a multi-day settle.
- **Anti-cheat:** keep "the simulation is reachable only through these tools;
  don't read the harness's files," framed neutrally (no "disqualified/benchmark"
  wording).

## Implementation surface

- `broker/src/benchmark/congestion.rs` — junction counting (topology join,
  degree filter, ≥2-approach rule) + tests.
- `broker/src/benchmark/record.rs` — `WindowStats.congested_junctions`; schema
  bump **v2 → v3**.
- `broker/src/benchmark/measure.rs` — fetch `network()` per window; compute
  `congested_junctions`.
- `broker/src/benchmark/score.rs` — blend + multiplicative health; remove hard
  `pop_guard` cliff; tests.
- `broker/src/benchmark/config.rs` — new constants above.
- `broker/src/benchmark/state.rs` + `server.rs` — rename block to `city_status`,
  add junctions, drop target, cache `network()` for live junction count.
- `broker/src/benchmark/transcript.rs` — read `city_status`, fall back to
  `benchmark_progress` for old runs.
- `benchmark/prompt.md` — full rewrite.
- `benchmark/README.md` — keep the real scoring formula (operator-facing), note
  the agent is no longer told it, drop the "prompt mirrors the constants" note.

## Out of scope

- Per-capita / per-demand congestion normalisation (rejected in favour of the
  health factor).
- Any mod/C# change.
- Rotating maps or cross-run memory.
- Retuning the 0.6 / 0.2 / 0.2 outer weights or the budget/change-cap values.

## Verification

- `cargo test --manifest-path broker/Cargo.toml` green, including new junction,
  blend, and health tests and the four scoring sanity checks above.
- Release binary builds.
- Prompt review: no formula, weights, caps, thresholds, or benchmark/competition
  language; no map-specific hints.
- A live `--watch` run on `gridlock-v1`: agent sees `city_status` with
  `congested_junctions`, attempts at least one intersection rework, and does not
  reason about a scoring boundary.
