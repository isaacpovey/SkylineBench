# Prompt + harness tuning: structural moves, side-effect anticipation, memory

**Date:** 2026-06-11
**Status:** approved (design)

## Motivation

Run `20260611-142831` (score 0.53, 32% congestion reduction) surfaced three
issues. The benchmark's purpose is to measure whether an agent can handle
**knock-on changes** and **balance multiple parameters** on its own, so fixes
must avoid teaching-to-the-test: no map-specific hints, no naming the specific
failure modes (noise, livability) the agent is supposed to discover.

The fixes are generic prompt framing, the removal of backfiring steering, and
one neutral telemetry signal — not domain hints.

## Findings (from the run transcript)

- **#1 No structural changes, only road-widening.** The agent *knew*
  structural was the answer — it wrote *"a structural rebuild would risk the
  stable result"* — and stopped at 32% with ~2h of budget unused. The prompt
  already says "think like a traffic engineer / structural moves," but two
  competing instructions scared it off: the Validate step's *"if congested
  meters land meaningfully above it, revert that batch FIRST … before trying
  anything new,"* combined with final-state scoring, made a stable mediocre
  result feel safer than a bold attempt. A structural rebuild legitimately
  spikes congestion before it settles, so the revert rule reads as "abandon
  structural experiments on the first bad measurement."
- **#3 Failed to anticipate side-effects.** The agent upgraded residential
  streets to Large Road, triggered noise-driven abandonment (5→149 buildings)
  and a population crash, then reverted. It learned reactively, not
  predictively. The benchmark surfaced the failure correctly; the goal is for
  the agent to anticipate it.
- **#2 Memory.** The agent used Claude Code's native memory feature, writing to
  `claude-config/projects/<per-run-workspace-path>/memory/` at the very end,
  right before submit — a dead artifact, too late to help itself. The write
  succeeded (the sandbox only denies repo reads). Memory is already naturally
  per-run because the project path is the per-run workspace, which is wiped.
  Desired behaviour: **within-run only** (clean slate each run); just make the
  habit useful and explicitly permitted.

## Decisions

- Memory scope: **within-run only.** No cross-run persistence — every run stays
  a clean-slate capability test.
- Acceptable levers for #1/#3: soften backfiring steering, generic
  consequence-framing, surface telemetry (not instructions), and budget
  pressure. All four are in scope.
- #3 framing stays **fully generic** — no "noise"/"livability"/road-type words.
- The optional stale-memory-dir cleanup (#6) is **out of scope** (tiny disk
  leak, not a correctness issue).

## Changes

All prompt edits target `benchmark/prompt.md`. The telemetry edit also touches
the broker; `benchmark/README.md` stays in sync where it documents the progress
block.

### #1 — structural avoidance (prompt only)

1. **Reframe the revert rule** (current Validate step, ~lines 45–48). Keep the
   checkpoint discipline, but make revert conditional on a *sustained,
   settled* regression rather than the first bad measurement. State explicitly
   that structural rebuilds legitimately spike before they settle, so they must
   be given settle-time (step a day or more) before being judged. Reverting
   pre-emptively kills exactly the high-leverage moves the agent should be
   attempting.
2. **Budget pressure** (traffic-engineer paragraph, ~line 50). Add: stopping
   with congestion well above target and budget remaining leaves score
   unclaimed; and a corridor that stops responding to widening is itself
   evidence the bottleneck is geometric — a signal to change the geometry, not
   to stop. Names no specific junction or tactic.

### #3 — anticipate side-effects

3. **Generic consequence-framing** (prompt). One domain-neutral sentence: every
   modification has consequences beyond vehicle flow; reason about what is
   adjacent to a change *before* committing, not after. No mention of noise,
   livability, or road-type specifics.
4. **Telemetry (broker).** Add `happiness` (already read in `MetricsDto`, not
   yet surfaced) to the `benchmark_progress` block emitted on tool responses.
   It drops before abandonment cascades, giving a neutral early-warning the
   agent can watch without being told what causes the drop. Update the prompt's
   progress-block field list (~line 39) and `benchmark/README.md` to mention it.

### #2 — memory (specific instructions OK)

5. **Prompt.** Explicitly permit and encourage keeping a running record of what
   worked and what failed *as the run progresses* (memory or scratch files in
   the workspace), so later decisions in the same run benefit — rather than only
   summarising at submit time.

## Out of scope

- Cross-run memory persistence.
- Per-segment noise / land-value telemetry (would need new mod C# game-reads and
  edges toward teaching-to-the-test).
- Stale per-run memory-dir cleanup in the persistent config dir (#6).
- Any change to the scoring formula or its constants.

## Verification

- A dry-run prompt diff review: confirm no map-specific or named-failure-mode
  hints leaked in.
- Broker builds; `happiness` appears in the `benchmark_progress` JSON (covered
  by the contract/serialization tests that already assert the progress block).
- README and prompt progress-block descriptions list the same fields.
- A live `--watch` run (optional) to sanity-check the agent attempts at least
  one structural move and reacts to the happiness signal.
