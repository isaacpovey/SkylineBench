# SkylineBench Phase 2 — Traffic-Improvement Benchmark (Design Spec)

**Date:** 2026-06-09
**Status:** Approved design, pre-implementation.
**Builds on:** Phase 1 harness (mod + broker), merged to `main`. See `docs/superpowers/2026-06-09-phase2-context.md` and `mod/DISCOVERY.md`.

## 1. Goal

A minimal, rigorous benchmark that scores an AI coding agent (Claude Code, Codex) on improving traffic in a deliberately-bad-traffic Cities: Skylines 1 city. A run produces one absolute, reproducible score in `[0, 1]`.

## 2. Operating model

**Human-in-loop reset, automated run.** Reset is always a **main-menu load** of a pinned save — never a mid-session `LoadLevel`, which crashes the game (`mod/DISCOVERY.md` §load-save). The operator performs the one manual step (load the save); everything after is automated by the broker in benchmark mode.

## 3. Run lifecycle

1. **Operator** loads the pinned bad-traffic save from CS1's main menu; confirms `GET /health` → `city_loaded:true`.
2. **Operator** starts `broker benchmark --map <id>`. The broker serves MCP exactly as in `serve` mode, plus instruments the run and controls the clock.
3. **Baseline window.** Broker ensures the sim is paused, then steps it across `W` ticks, sampling `flow_percent` and `active_vehicles`; records baseline means and `population`.
4. **Agent phase.** The agent connects over MCP and works using the existing tools (`observe_area`, `get_metrics`, `build_road`, `bulldoze`, `upgrade_road`, `set_zoning`, `control_time`, `render_map`, …). The broker tallies every tool call, counts mutating actions, and sums per-action `cost`. Every response carries a `benchmark_progress` block (§7).
5. **Run ends on the first of three outs:**
   - the agent calls **`submit_solution`** (voluntary);
   - the **windowed `flow_percent` ≥ `FLOW_TARGET`** (auto-win);
   - **wall-clock ≥ 3h** (`WALL_CLOCK_CAP_S = 10800`) backstop.
6. **Settle window.** Broker steps the sim `S` ticks so traffic re-equilibrates around the new layout (post-edit flow is not yet representative).
7. **Final window.** Broker steps `W` ticks sampling `flow_percent` and `active_vehicles`; records final means and `population`.
8. **Guard + score.** Apply the throughput guard (§5), compute the score (§4) from the run-record, and emit `run-record.json` + `score.json`.

The scored **time** term is tool-call count, not wall-clock — reproducible across hardware/model/latency. The 3h wall-clock is only a kill-switch and never enters the score.

## 4. Scoring

Pure, deterministic function of the run-record (replayable). Four terms, each normalized to `[0, 1]` against fixed bounds, combined as a flow-dominant weighted sum (weights sum to 1).

```
Δflow        = final_flow_mean − baseline_flow_mean        # flow %-points
norm_flow    = clamp(Δflow / TARGET_GAIN,    0, 1)
norm_money   = clamp(money_spent / BUDGET,   0, 1)
norm_calls   = clamp(tool_calls / CALL_CAP,  0, 1)
norm_changes = clamp(num_changes / CHANGE_CAP, 0, 1)

score = 0.55 * norm_flow
      + 0.15 * (1 − norm_money)
      + 0.15 * (1 − norm_changes)
      + 0.15 * (1 − norm_calls)
```

**Definitions:**
- `num_changes` = count of **mutating tool calls** (`build_road`, `bulldoze`, `upgrade_road`, `set_zoning`). One call is one change regardless of how many segments/cells it touches.
- `tool_calls` = count of all agent MCP tool invocations during the **agent phase** (read and write). Broker-internal metric reads during baseline/final windows do not count. `submit_solution` counts as one call (negligible).
- `money_spent` = Σ of per-action `cost` over the agent phase (negative costs / refunds reduce the total).

## 5. Anti-cheat guard

`flow_percent` measures how freely vehicles move, not how many — so razing demand would inflate it. Guard against winning by depopulating:

```
if final.active_vehicles_mean < 0.9 * baseline.active_vehicles_mean:
    run is INVALID  → score = 0
```

The run-record always logs baseline and final `active_vehicles` and `population` for audit, so borderline gaming is visible on review even when the guard passes.

## 6. Calibration constants

Tunable; calibrated against the chosen map during the implementation verify step. Placeholders:

| Constant | Meaning | Placeholder |
|---|---|---|
| `TARGET_GAIN` | Δflow (%-points) that maxes the flow term | 40 |
| `BUDGET` | the save's starting balance (spend it all → `norm_money` = 1) | = save balance |
| `CALL_CAP` | tool-call ceiling for normalization | 300 |
| `CHANGE_CAP` | mutating-action ceiling for normalization | 100 |
| `FLOW_TARGET` | auto-win flow % (closest achievable to 100) | 95 |
| `W` | measurement-window length (ticks) | 2048 |
| `S` | settle-window length (ticks) | 8192 |
| `WALL_CLOCK_CAP_S` | hard backstop | 10800 (3h) |
| guard ratio | min final/baseline `active_vehicles` | 0.9 |

Weights (`0.55 / 0.15 / 0.15 / 0.15`) are configurable but default flow-dominant.

## 7. Agent-facing progress telemetry

In benchmark mode, every tool response carries a piggybacked block (so checking progress never costs a tool call):

```json
"benchmark_progress": {
  "money_spent": 0,
  "budget_remaining": 0,
  "tool_calls_used": 0,
  "num_changes": 0,
  "flow_current": 0.0,
  "flow_target": 95.0,
  "seconds_remaining": 0
}
```

The agent sees its **resources and goal**, never the composite score, weights, or normalization — exposing the aggregate would invite Goodharting (stopping exactly at the target, hugging the guard floor). The agent is told the rules in its task prompt (the three outs, the throughput guard, and that lower spend/changes/calls scores better); game state (`flow_percent`, `active_vehicles`, …) remains available via `get_metrics`.

`flow_current` is a **rolling windowed mean** of `flow_percent` sampled whenever the agent advances the sim via `control_time` — raw post-edit flow is too noisy to compare against `FLOW_TARGET`. The `flow_target` out fires (at the next tool boundary) when this rolling mean over the most recent `W` ticks reaches `FLOW_TARGET`.

## 8. Contract changes (broker + mod + mock, in lockstep)

`broker/src/contract.rs` is the frozen wire format; changes touch both sides and the mock.

- **Add `cost: i64` to `ActionResult`.** The mod reads `EconomyManager.LastCashAmount` immediately before and after each marshaled `build_road` / `bulldoze` / `upgrade_road` on the sim thread and returns the delta (negative = refund). Income over a single instantaneous action is ≈ 0, so this isolates construction cost from economy drift. Keeps the mod thin (a before/after read, no business logic). `set_zoning` → `cost: 0`.
- **New MCP tool `submit_solution`** (optional `note: string`), registered only in benchmark mode → triggers the voluntary out.

## 9. The starting map

Reuse an existing congested city, with these constraints:
- **Base-game assets only** (or explicitly-pinned DLC) so it loads without Workshop dependencies.
- **Limited money with a fixed starting balance** (not unlimited-money mode) so per-action `cost` is non-zero and `BUDGET` is defined.
- A copy **pinned in `benchmark/maps/`** with its source, game version, and the starting balance documented.

If a suitable existing city proves hard to source under these constraints, revisit (hand-built or scripted-degradation were the alternatives considered).

## 10. Run-record & score artifacts

`run-record.json` (sufficient to recompute the score offline):

```
schema_version
map:        { id, source, game_version, starting_balance }
config:     { weights, TARGET_GAIN, BUDGET, CALL_CAP, CHANGE_CAP,
              FLOW_TARGET, W, S, WALL_CLOCK_CAP_S, guard_ratio }
started_at, ended_at
end_reason: "submit" | "flow_target" | "timeout"
baseline:   { flow_mean, active_vehicles_mean, population }
final:      { flow_mean, active_vehicles_mean, population }
tally:      { tool_calls, num_changes, money_spent }
actions:    [ { seq, tool, cost } ]            # per-action audit log
flow_samples: { baseline: [...], final: [...] }
```

`score.json`:

```
norm:     { flow, money, calls, changes }
weighted: { flow, money, changes, calls }
invalid:  bool        # true if guard failed
score:    0.0 .. 1.0  # 0 if invalid
```

Scoring is a pure module: `run-record.json` → `score.json`, deterministic and re-runnable without the game.

## 11. Scope boundary (what Phase 2 does NOT do)

- **Agent launch stays manual.** The operator is already in-loop for reset; Phase 2 documents how to point `claude`/`codex` at the broker MCP with the benchmark task prompt. A scripted multi-agent runner is explicit future work.
- Mid-session reset remains unused (crashes).
- Deferred Phase 1.x items stay deferred: real `employed`, `/screenshot`, `setup`/`doctor` automation, fast-fail no-city guard.

## 12. Working conventions

Per the Phase 1 handoff: feature branch → merge to `main` locally (no PRs); commits end with the Co-Authored-By trailer; functional style where it maps to Rust/C#; keep the mod thin (no business logic in C#); change `contract.rs` + both sides together. Workflow: this spec → writing-plans → subagent-driven-development → finishing-a-development-branch.
