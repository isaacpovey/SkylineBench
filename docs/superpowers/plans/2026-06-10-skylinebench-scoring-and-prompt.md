# Scoring Config & Prompt Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Raise the change cap (100 → 300) and budget ($5M → $10M), and rewrite the prompt so the agent knows the scoring constants it is optimized against, banks progress instead of riding regressions, and understands `upgrade_road`'s ID renumbering.

**Architecture:** One constant change in `BenchConfig::default()` plus prompt/README text. Scoring math (`score.rs`) is untouched — the cap and budget are pure normalization denominators.

**Tech Stack:** Rust, markdown.

**Evidence this matters:** run `20260609-210135` made 168 changes against an invisible cap of 100 (zeroing the 20 % changes term), never called `submit_solution` (losing a +4.9 flow peak), and kept a flow-destroying batch in place for 25 minutes instead of reverting.

---

### Task 1: Raise change cap and budget

**Files:**
- Modify: `broker/src/benchmark/config.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `broker/src/benchmark/config.rs`:

```rust
    #[test]
    fn default_resource_envelope() {
        let c = BenchConfig::default();
        assert_eq!(c.budget, 10_000_000.0);
        assert_eq!(c.change_cap, 300.0);
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test --manifest-path broker/Cargo.toml default_resource_envelope`
Expected: FAIL — budget is `5_000_000.0`, change_cap `100.0`.

- [ ] **Step 3: Update the defaults**

In `BenchConfig::default()` in `broker/src/benchmark/config.rs` change:

```rust
            budget: 10_000_000.0,
            change_cap: 300.0,
```

(The observed real run spent $1.41M / 168 changes; structural rebuilds plus per-segment batch accounting need headroom, while 300/$10M keeps the terms meaningful.)

- [ ] **Step 4: Run the whole suite**

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: ALL PASS. (`score.rs::norms_clamp_to_unit` uses 50M/1000 which still clamps; no other test pins these constants.)

- [ ] **Step 5: Commit**

```bash
git add broker/src/benchmark/config.rs
git commit -m "feat(benchmark): raise change cap to 300 and budget to 10M"
```

---

### Task 2: Prompt rewrite — publish constants, revert discipline, ID renumbering

**Files:**
- Modify: `benchmark/prompt.md`
- Modify: `benchmark/README.md` (scoring section)

- [ ] **Step 1: Replace the scoring section of the prompt**

In `benchmark/prompt.md`, replace the block starting `How you are scored` (lines 13–16) with:

```markdown
How you are scored (you will NOT see your score during the run):
- `score = 0.60·min(1, flow_gain/40) + 0.20·(1 − min(1, money_spent/10,000,000)) + 0.20·(1 − min(1, changes/300))`.
- `flow_gain` is the FINAL settled flow minus the baseline — peaks along the way do not count.
  If a change makes flow worse, leaving it in place costs you score; revert it.
- Every successful modifying call is one change; reads are free — observe as much as you like.
- INVALID RUN: if you reduce the number of active vehicles too far (below 90% of baseline —
  you must not "fix" traffic by depopulating the city), the run scores zero. Keep the city alive.
```

- [ ] **Step 2: Strengthen the work-method loop with checkpoint/revert discipline**

In `benchmark/prompt.md`, replace step 4 of the "Work method" list (the line beginning `4. **Validate.**`) with:

```markdown
4. **Validate.** Re-measure flow against your pre-change reading. Treat your last good
   measurement as a checkpoint: if flow lands meaningfully below it, revert that batch
   FIRST (bulldoze what you built, rebuild what you removed) before trying anything new —
   the final settled state is what is scored, not your best moment.
```

- [ ] **Step 3: Document upgrade_road ID renumbering**

In `benchmark/prompt.md`, append to the "Modify" bullet (line 5):

```markdown
- Modify (these are your "changes"): `build_road`, `bulldoze`, `upgrade_road`, `set_zoning`.
  Note: `upgrade_road` re-creates the segment under a NEW id (the response maps old → new);
  refresh any segment ids you cached before reusing them.
```

- [ ] **Step 4: Update the README scoring section**

In `benchmark/README.md`, replace the Scoring section body with:

```markdown
## Scoring (spec §4)
`score = 0.60·norm(Δflow) + 0.20·(1−norm(money)) + 0.20·(1−norm(changes))`,
zeroed if final active vehicles drop below 90% of baseline. Normalization:
Δflow against a 40-point target gain, money against a $10,000,000 budget,
changes against a 300-change cap. Constants live in
`broker/src/benchmark/config.rs` and are tuned against the map. The agent
prompt now states these constants explicitly — keep prompt.md in sync when
retuning them.
```

- [ ] **Step 5: Sanity-check the prompt renders**

Run: `cat benchmark/prompt.md`
Expected: the three edits read coherently in sequence; no duplicated "How you are scored" block; markdown list numbering intact.

- [ ] **Step 6: Commit**

```bash
git add benchmark/prompt.md benchmark/README.md
git commit -m "docs(benchmark): publish scoring constants, revert discipline, upgrade id note"
```
