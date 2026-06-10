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
2. **Operator** runs `benchmark/run.sh --map <id>` (§11), which launches Claude Code (headless) wired to `broker benchmark` over stdio. The broker serves MCP exactly as in `serve` mode, plus instruments the run and controls the clock.
3. **Baseline window.** At broker startup (before the agent's first tool call), the broker ensures the sim is paused, then steps it across `W` ticks, sampling `flow_percent` and `active_vehicles`; records baseline means and `population`.
4. **Agent phase.** The agent connects over MCP and works using the existing tools (`observe_area`, `get_metrics`, `build_road`, `bulldoze`, `upgrade_road`, `set_zoning`, `control_time`, `render_map`, …). The broker counts mutating actions and computes the cost of each from the action itself (§8). Reads are unrestricted and unscored. Every response carries a `benchmark_progress` block (§7).
5. **Run ends on the first of three outs:**
   - the agent calls **`submit_solution`** (voluntary);
   - the **windowed `flow_percent` ≥ `FLOW_TARGET`** (auto-win);
   - **wall-clock ≥ 3h** (`WALL_CLOCK_CAP_S = 10800`) backstop.
6. **Settle window.** Broker steps the sim `S` ticks so traffic re-equilibrates around the new layout (post-edit flow is not yet representative).
7. **Final window.** Broker steps `W` ticks sampling `flow_percent` and `active_vehicles`; records final means and `population`.
8. **Guard + score.** Apply the throughput guard (§5), compute the score (§4) from the run-record, and emit `run-record.json` + `score.json`.

The 3h wall-clock is only a kill-switch and never enters the score. Reads are unscored — only built work (flow gain), money spent, and the number of write actions matter.

## 4. Scoring

Pure, deterministic function of the run-record (replayable). Three terms, each normalized to `[0, 1]` against fixed bounds, combined as a flow-dominant weighted sum (weights sum to 1).

```
Δflow        = final_flow_mean − baseline_flow_mean        # flow %-points
norm_flow    = clamp(Δflow / TARGET_GAIN,      0, 1)
norm_money   = clamp(money_spent / BUDGET,     0, 1)
norm_changes = clamp(num_changes / CHANGE_CAP, 0, 1)

score = 0.60 * norm_flow
      + 0.20 * (1 − norm_money)
      + 0.20 * (1 − norm_changes)
```

**Definitions:**
- `num_changes` = count of **mutating tool calls** (`build_road`, `bulldoze`, `upgrade_road`, `set_zoning`) during the agent phase. One call is one change regardless of how many segments/cells it touches. Read-only tools (`observe_area`, `get_metrics`, `render_map`, …) are never counted.
- `money_spent` = Σ of broker-computed per-action `cost` over the agent phase (§8); refunds reduce the total.

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
| `BUDGET` | money-spent normalization ceiling (spend this much → `norm_money` = 1); a scoring constant, not an in-game cash limit | TBD at calibration |
| `CHANGE_CAP` | mutating-action ceiling for normalization | 100 |
| `FLOW_TARGET` | auto-win flow % (closest achievable to 100) | 95 |
| `W` | measurement-window length (ticks) | 2048 |
| `S` | settle-window length (ticks) | 8192 |
| `WALL_CLOCK_CAP_S` | hard backstop | 10800 (3h) |
| guard ratio | min final/baseline `active_vehicles` | 0.9 |

Weights (`0.60 / 0.20 / 0.20`) are configurable but default flow-dominant. `BUDGET` is a normalization ceiling only — money is unlimited in-game (§9), so it never blocks building; it is purely a scoring penalty calibrated against a typical solution's spend.

## 7. Agent-facing progress telemetry

In benchmark mode, every tool response carries a piggybacked block (so checking progress never costs a tool call):

```json
"benchmark_progress": {
  "money_spent": 0,
  "num_changes": 0,
  "flow_current": 0.0,
  "flow_target": 95.0,
  "seconds_remaining": 0
}
```

The agent sees its **resources and goal**, never the composite score, weights, or normalization — exposing the aggregate would invite Goodharting (stopping exactly at the target, hugging the guard floor). The agent is told the rules in its task prompt (the three outs, the throughput guard, and that lower spend and fewer write actions score better); game state (`flow_percent`, `active_vehicles`, …) remains available via `get_metrics`. Reads are free, so there is no read counter to expose.

`flow_current` is a **rolling windowed mean** of `flow_percent` sampled whenever the agent advances the sim via `control_time` — raw post-edit flow is too noisy to compare against `FLOW_TARGET`. The `flow_target` out fires (at the next tool boundary) when this rolling mean over the most recent `W` ticks reaches `FLOW_TARGET`.

## 8. Contract changes (broker + mod + mock, in lockstep)

`broker/src/contract.rs` is the frozen wire format; changes touch both sides and the mock.

- **Add `construction_cost` to the road-type DTO.** The mod reflects `NetInfo.m_constructionCost` and surfaces it on the road-type listing so the broker has accurate, data-driven per-type costs (rather than a static table that can drift). This keeps the mod thin (one more reflected field) and is the only contract addition for cost.
- **New MCP tool `submit_solution`** (optional `note: string`), registered only in benchmark mode → triggers the voluntary out.

**Cost model (broker-side, computed from the action — not from funds).** Because money is unlimited in-game, `EconomyManager` funds never change; the broker computes spend itself from what it built:
- `build_road` → `construction_cost × built_length` (broker already knows the road type and computes segment length during geometry/assembly).
- `upgrade_road` → construction cost of the new road type × length (in-place swap).
- `bulldoze` → `0` for the minimal phase (no refund modeling); the `num_changes` term already penalizes the action. Refund modeling is a possible later refinement.
- `set_zoning` → `0`.

The exact length-scaling of `m_constructionCost` (CS1 charges per unit length off a base) is confirmed/calibrated during the verify step.

## 9. The starting map

Reuse an existing congested city. **Unlimited Money + Unlock All are enabled**, so cash never blocks building and all tiles/features are available — money is purely a *scoring* penalty (the broker computes spend per §8; `BUDGET` is just the normalization ceiling). Map sourcing is therefore unconstrained beyond:
- A copy **pinned in `benchmark/maps/`** with its source and game version documented.
- The save configured with Unlimited Money + Unlock All active.

Hand-built or scripted-degradation remain fallbacks if no suitable congested city is found.

## 10. Run-record & score artifacts

All artifacts for a run land together in a per-run directory `benchmark/runs/<run-id>/`: `run-record.json`, `score.json`, `transcript.jsonl`, and `transcript.md` (§11). Bundling them means a poor score is diagnosable from one place.

`run-record.json` (sufficient to recompute the score offline):

```
schema_version
map:        { id, source, game_version }
config:     { weights, TARGET_GAIN, BUDGET, CHANGE_CAP,
              FLOW_TARGET, W, S, WALL_CLOCK_CAP_S, guard_ratio }
started_at, ended_at
end_reason: "submit" | "flow_target" | "timeout"
baseline:   { flow_mean, active_vehicles_mean, population }
final:      { flow_mean, active_vehicles_mean, population }
tally:      { num_changes, money_spent }
actions:    [ { seq, tool, cost } ]            # per-action audit log (broker-computed cost)
flow_samples: { baseline: [...], final: [...] }
```

`score.json`:

```
norm:     { flow, money, changes }
weighted: { flow, money, changes }
invalid:  bool        # true if guard failed
score:    0.0 .. 1.0  # 0 if invalid
```

Scoring is a pure module: `run-record.json` → `score.json`, deterministic and re-runnable without the game.

## 11. Benchmark runner script

`benchmark/run.sh` — one command per run, launching Claude Code against the benchmark. After the operator has loaded the save (`city_loaded:true`), the script:

1. **Generates an MCP config** that has Claude Code spawn `broker benchmark --map <id> --mod-url http://127.0.0.1:8787` as a **stdio MCP server** — no separate broker process to manage.
2. **Launches Claude Code headless:** `claude -p "<prompt>" --mcp-config <generated> --allowedTools <benchmark MCP tools> --output-format stream-json --verbose`. The benchmark tools are pre-allowed so an unattended run never stalls on a permission prompt.
3. **Supplies the task prompt** from a versioned file `benchmark/prompt.md`: the goal (improve traffic flow), the available tools, the three outs (§3.5), the throughput guard (§5), that lower spend and fewer write actions score better while reads are free, and that the agent finishes by calling `submit_solution`. (The prompt never reveals the weights or composite score — §7.)
4. **Captures the full transcript:** the `stream-json` output is teed to `transcript.jsonl` (complete, machine-readable: every assistant message, tool call, and tool result), then rendered to a human-readable `transcript.md`.
5. **Bundles artifacts** into `benchmark/runs/<run-id>/`: `transcript.jsonl`, `transcript.md`, `run-record.json`, `score.json`.

**Flags:**
- `--watch` (alias `--interactive`): open a normal interactive Claude Code session with the same prompt + MCP for live observation/debugging instead of headless. The session is still captured (Claude Code persists its own session transcript, referenceable for review).
- `--map <id>` (required), `--out <dir>` (default `benchmark/runs/<run-id>/`).

**Why the transcript is a first-class artifact:** when a score is poor, the transcript next to the `run-record`/`score` lets you tell *why* — a harness problem (confusing tool results, bad errors, geometry rejections) versus a prompt problem (the agent misread the task) — and iterate on the harness or `benchmark/prompt.md` accordingly.

**Run-end mechanism (for the plan):** when an out fires, the broker stops accepting agent mutations, runs the settle + final windows itself (it controls the clock independently of the agent), writes `run-record.json`/`score.json`, and then exits — closing stdio and ending the Claude Code session.

## 12. Scope boundary (what Phase 2 does NOT do)

- **Single agent CLI (Claude Code) only.** The runner targets `claude`; a multi-agent runner (e.g. Codex) is future work, though the broker MCP is agent-agnostic.
- Mid-session reset remains unused (crashes).
- Deferred Phase 1.x items stay deferred: real `employed`, `/screenshot`, `setup`/`doctor` automation, fast-fail no-city guard.

## 13. Working conventions

Per the Phase 1 handoff: feature branch → merge to `main` locally (no PRs); commits end with the Co-Authored-By trailer; functional style where it maps to Rust/C#; keep the mod thin (no business logic in C#); change `contract.rs` + both sides together. Workflow: this spec → writing-plans → subagent-driven-development → finishing-a-development-branch.
