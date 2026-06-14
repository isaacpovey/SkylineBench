# Website Versioning, Run Generator, Updates & Changelog — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tag every benchmark run with the benchmark version that produced it, view the leaderboard per `(scenario, version)`, generate run files (metrics + AI verdict/beats + timelapse) from a run directory, and restructure the site to an `/updates` page and a `/changelog` page.

**Architecture:** All work is inside `website/` (a Next.js SSG app). Run content lives in `content/runs/*.ts` validated by a Zod schema; leaderboards are derived by grouping runs. The generator is a `tsx` CLI under `website/scripts/` split into pure, unit-tested helpers (record→Run mapping, serialization, index insertion) plus thin I/O wiring (file reads, Anthropic API call, timelapse shell-out). Updates and changelog are data-driven pages.

**Tech Stack:** Next.js 16, React 19, TypeScript, Zod, Vitest, `tsx`, `@anthropic-ai/sdk` (model `claude-opus-4-8`).

**Spec:** `docs/superpowers/specs/2026-06-14-website-versioning-and-generator-design.md`

**Working directory for all commands:** `website/` unless stated otherwise.

**Prerequisite (run once before Task 1):**

```bash
cd website && npm install
```

Expected: dependencies installed, `node_modules/` present, `npm test` runnable.

---

### Task 1: Version constant, schema field, run backfill, test fixture

**Files:**
- Create: `website/lib/version.ts`
- Modify: `website/lib/run.ts`
- Modify: `website/content/runs/fable-5.ts`, `sonnet-4-5.ts`, `opus-4-8.ts`, `haiku-4-5.ts`, `gpt-5-5.ts`, `gpt-5-4-mini.ts`
- Modify: `website/__tests__/content.test.ts`

- [ ] **Step 1: Create the version constant**

Create `website/lib/version.ts`:

```ts
export const CURRENT_HARNESS_VERSION = "v0.1";
```

- [ ] **Step 2: Add `harnessVersion` to the schema (failing test first)**

Modify `website/__tests__/content.test.ts` — add `harnessVersion` to the `valid` fixture so the schema test exercises the new field:

```ts
const valid = {
  slug: "demo", modelName: "Demo", map: "gridlock-v1", harnessVersion: "v0.1", runDir: "benchmark/runs/x",
  score: 0.5, verdict: "ok",
  metrics: {
    flow: { from: 57, to: 71 }, congestedMetres: { from: 5122, to: 1854 },
    jammedJunctions: { from: 35, to: 12 }, population: { from: 31640, to: 31174 },
    activeVehicles: { from: 2112, to: 1709 }, changes: 197, spend: 1240000,
  },
  flowSettling: { base: [57, 56], final: [57, 71] },
  spendSeries: [0, 1000, 1240000],
  actions: [{ type: "upgrade_road", count: 180, cost: 1180000 }],
  beats: [{ title: "Survey", body: "read the map" }],
};
```

Also add a new assertion inside the `describe("runSchema", ...)` block:

```ts
  it("requires harnessVersion", () => {
    const { harnessVersion: _omit, ...withoutVersion } = valid;
    expect(() => runSchema.parse(withoutVersion)).toThrow();
  });
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `npm test -- content`
Expected: FAIL — `runSchema` does not yet have `harnessVersion`, so the new `requires harnessVersion` test fails (parse does not throw), and the existing run-content test fails because the authored runs lack the field.

- [ ] **Step 4: Add the field to the schema**

Modify `website/lib/run.ts` — add `harnessVersion` right after `map`:

```ts
export const runSchema = z.object({
  slug: z.string(),
  modelName: z.string(),
  map: z.string(),
  harnessVersion: z.string(),
  runDir: z.string(),
  score: z.number().min(0).max(1),
  verdict: z.string(),
  // ...unchanged below...
```

- [ ] **Step 5: Backfill `harnessVersion: "v0.1"` into all six run files**

In each of `website/content/runs/fable-5.ts`, `sonnet-4-5.ts`, `opus-4-8.ts`, `haiku-4-5.ts`, `gpt-5-5.ts`, `gpt-5-4-mini.ts`, add the line immediately after the `map:` line. Example for `opus-4-8.ts`:

```ts
  slug: "opus-4-8",
  modelName: "Claude Opus 4.8",
  map: "gridlock-v1",
  harnessVersion: "v0.1",
  runDir: "benchmark/runs/20260612-161516",
```

Do the same edit (insert `harnessVersion: "v0.1",` after the `map:` line) in the other five files.

- [ ] **Step 6: Fix the run-count assertion in the content test**

The repo has six run files but `content.test.ts` still asserts `4`. Modify `website/__tests__/content.test.ts`:

```ts
  it("all authored runs are valid and ranked by score", () => {
    expect(runs.length).toBe(6);
    expect(runs[0].slug).toBe("fable-5");
    runs.forEach((r) => expect(() => runSchema.parse(r)).not.toThrow());
  });
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `npm test -- content`
Expected: PASS — all `runSchema` tests pass, all six runs validate, count is 6.

If `runs[0].slug` is not `fable-5`, the run order changed; set it to the slug of the highest-scoring run reported by the failure.

- [ ] **Step 8: Commit**

```bash
git add website/lib/version.ts website/lib/run.ts website/content/runs/*.ts website/__tests__/content.test.ts
git commit -m "feat(website): add harnessVersion to runs and version constant"
```

---

### Task 2: Leaderboards grouping module

**Files:**
- Create: `website/lib/leaderboards.ts`
- Test: `website/__tests__/leaderboards.test.ts`

- [ ] **Step 1: Write the failing test**

Create `website/__tests__/leaderboards.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import type { Run } from "@/lib/run";
import { buildLeaderboards, pickCurrent } from "@/lib/leaderboards";

const run = (over: Partial<Run>): Run =>
  ({
    slug: "x", modelName: "X", map: "gridlock-v1", harnessVersion: "v0.1", runDir: "d",
    score: 0.5, verdict: "v",
    metrics: {
      flow: { from: 1, to: 2 }, congestedMetres: { from: 1, to: 2 },
      jammedJunctions: { from: 1, to: 2 }, population: { from: 1, to: 2 },
      activeVehicles: { from: 1, to: 2 }, changes: 1, spend: 1,
    },
    flowSettling: { base: [1], final: [1] }, spendSeries: [0], actions: [],
    beats: [], ...over,
  });

describe("buildLeaderboards", () => {
  it("groups by (map, harnessVersion) and sorts runs by score desc", () => {
    const runs = [
      run({ slug: "a", score: 0.2, harnessVersion: "v0.1" }),
      run({ slug: "b", score: 0.8, harnessVersion: "v0.1" }),
      run({ slug: "c", score: 0.5, harnessVersion: "v0.2" }),
    ];
    const boards = buildLeaderboards(runs);
    expect(boards).toHaveLength(2);
    const v01 = boards.find((b) => b.harnessVersion === "v0.1")!;
    expect(v01.label).toBe("gridlock-v1 · v0.1");
    expect(v01.runs.map((r) => r.slug)).toEqual(["b", "a"]);
  });

  it("picks the leaderboard matching the current version", () => {
    const boards = buildLeaderboards([
      run({ slug: "a", harnessVersion: "v0.1" }),
      run({ slug: "c", harnessVersion: "v0.2" }),
    ]);
    expect(pickCurrent(boards, "v0.2").harnessVersion).toBe("v0.2");
    expect(pickCurrent(boards, "v9.9").harnessVersion).toBe("v0.1"); // falls back to first
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npm test -- leaderboards`
Expected: FAIL — `@/lib/leaderboards` does not exist.

- [ ] **Step 3: Write the implementation**

Create `website/lib/leaderboards.ts`:

```ts
import type { Run } from "@/lib/run";
import { runs as allRuns } from "@/content/runs";
import { CURRENT_HARNESS_VERSION } from "@/lib/version";

export type Leaderboard = {
  map: string;
  harnessVersion: string;
  label: string;
  runs: Run[];
};

export const buildLeaderboards = (runs: Run[]): Leaderboard[] => {
  const groups = runs.reduce<Map<string, Run[]>>((acc, run) => {
    const key = `${run.map}::${run.harnessVersion}`;
    return acc.set(key, [...(acc.get(key) ?? []), run]);
  }, new Map());

  return Array.from(groups.entries()).map(([key, groupRuns]) => {
    const [map, harnessVersion] = key.split("::");
    return {
      map,
      harnessVersion,
      label: `${map} · ${harnessVersion}`,
      runs: [...groupRuns].sort((a, b) => b.score - a.score),
    };
  });
};

export const pickCurrent = (boards: Leaderboard[], version: string): Leaderboard =>
  boards.find((b) => b.harnessVersion === version) ?? boards[0];

export const leaderboards = buildLeaderboards(allRuns);
export const currentLeaderboard = pickCurrent(leaderboards, CURRENT_HARNESS_VERSION);
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `npm test -- leaderboards`
Expected: PASS — both tests green.

- [ ] **Step 5: Commit**

```bash
git add website/lib/leaderboards.ts website/__tests__/leaderboards.test.ts
git commit -m "feat(website): derive leaderboards grouped by scenario and version"
```

---

### Task 3: Results section leaderboard selector

**Files:**
- Modify: `website/components/sections/results.tsx`

- [ ] **Step 1: Convert Results to a client component driven by leaderboards**

Replace the entire contents of `website/components/sections/results.tsx`:

```tsx
"use client";

import { useState } from "react";
import { Layers, Clock } from "lucide-react";
import { leaderboards, currentLeaderboard } from "@/lib/leaderboards";
import { formatDelta, formatMillions, percentChange } from "@/lib/format";
import { Card } from "@/components/ui/card";

export const Results = () => {
  const [selected, setSelected] = useState(currentLeaderboard.label);
  const board = leaderboards.find((b) => b.label === selected) ?? currentLeaderboard;

  return (
    <section className="section section-soft" id="results">
      <div className="wrap">
        <div className="results-head reveal">
          <div className="section-head" style={{ margin: 0 }}>
            <p className="eyebrow">Results</p>
            <h2 className="section-title">How the models did.</h2>
            <p className="lead">
              Every model runs the same <span className="mono">{board.map}</span> scenario under
              identical scoring on harness <span className="mono">{board.harnessVersion}</span>, ranked
              by composite score. Open a run to see how it got there.
            </p>
          </div>
          {leaderboards.length > 1 && (
            <label className="leaderboard-select">
              <span className="hide-sm">Leaderboard</span>
              <select value={selected} onChange={(e) => setSelected(e.target.value)}>
                {leaderboards.map((b) => (
                  <option key={b.label} value={b.label}>{b.label}</option>
                ))}
              </select>
            </label>
          )}
        </div>

        <div className="results-grid">
          {board.runs.map((run, index) => {
            const junctionsGood = percentChange(run.metrics.jammedJunctions) < 0;
            const popGood = percentChange(run.metrics.population) >= 0;
            return (
              <Card asChild className="result-card reveal" key={run.slug}>
                <a href={`/runs/${run.slug}`}>
                  <div className="result-body">
                    <div className="result-top">
                      <div className="result-model">
                        <span className="result-rank">{index + 1}</span>
                        <span className="mico"><Layers /></span>
                        <span className="name">{run.modelName}<small>{run.map}</small></span>
                      </div>
                      <span className="status-pill scored">view run &#x2192;</span>
                    </div>
                    <div className="result-score">
                      <span className="val scored">{run.score.toFixed(2)}</span>
                      <span className="of">/ 1.00</span>
                      <span className="result-metrics">
                        <span className="metric"><span className={`m-val ${junctionsGood ? "good" : "bad"}`}>{formatDelta(run.metrics.jammedJunctions)}</span><span className="m-lbl">junctions</span></span>
                        <span className="metric"><span className={`m-val ${popGood ? "good" : "bad"}`}>{formatDelta(run.metrics.population)}</span><span className="m-lbl">population</span></span>
                        <span className="metric"><span className="m-val">{formatMillions(run.metrics.spend)}</span><span className="m-lbl">spent</span></span>
                      </span>
                    </div>
                  </div>
                </a>
              </Card>
            );
          })}
        </div>

        <Card asChild className="coming-soon reveal">
          <div>
            <span className="cs-ico"><Clock /></span>
            <div>
              <h4>More models, coming soon</h4>
              <p>Other frontier models will run the same {board.map} scenario under identical scoring. Their results land here as the runs complete.</p>
            </div>
          </div>
        </Card>
      </div>
    </section>
  );
};
```

- [ ] **Step 2: Verify it builds**

Run: `npm run build`
Expected: build succeeds; the homepage Results section renders the current leaderboard (no selector shown while there is only one).

- [ ] **Step 3: Commit**

```bash
git add website/components/sections/results.tsx
git commit -m "feat(website): leaderboard selector in results section"
```

---

### Task 4: Show version on the run detail page

**Files:**
- Modify: `website/app/runs/[slug]/page.tsx`

- [ ] **Step 1: Add the version to the eyebrow**

In `website/app/runs/[slug]/page.tsx`, change the eyebrow line:

```tsx
          <p className="eyebrow">
            Run detail · <span className="mono">{run.map}</span> · <span className="mono">{run.harnessVersion}</span>
          </p>
```

- [ ] **Step 2: Verify it builds**

Run: `npm run build`
Expected: build succeeds; a run page shows e.g. `Run detail · gridlock-v1 · v0.1`.

- [ ] **Step 3: Commit**

```bash
git add website/app/runs/[slug]/page.tsx
git commit -m "feat(website): show harness version on run detail page"
```

---

### Task 5: Add generator dependencies

**Files:**
- Modify: `website/package.json` (via npm)

- [ ] **Step 1: Install tsx and the Anthropic SDK**

Run (in `website/`):

```bash
npm install -D tsx
npm install @anthropic-ai/sdk
```

Expected: both added to `package.json`; `node_modules/.bin/tsx` exists.

- [ ] **Step 2: Add a generate script alias**

Add to the `scripts` block in `website/package.json`:

```json
    "generate-run": "tsx scripts/generate-run.ts"
```

- [ ] **Step 3: Commit**

```bash
git add website/package.json website/package-lock.json
git commit -m "chore(website): add tsx and @anthropic-ai/sdk for run generator"
```

---

### Task 6: Run-record → Run mapping helpers

**Files:**
- Create: `website/scripts/lib/build-run.ts`
- Test: `website/__tests__/build-run.test.ts`

- [ ] **Step 1: Write the failing test**

Create `website/__tests__/build-run.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { runSchema } from "@/lib/run";
import { groupActions, cumulativeSpend, mapRecordToRun } from "@/scripts/lib/build-run";

const record = {
  map: { id: "gridlock-v1", source: "test", game_version: "1.21.1-f9" },
  baseline: { flow_mean: 57.4, active_vehicles_mean: 2112.1, population: 31640, congested_meters: 5121.7, congested_junctions: 35 },
  final: { flow_mean: 70.6, active_vehicles_mean: 1708.6, population: 31174, congested_meters: 1853.9, congested_junctions: 12 },
  flow_samples: { baseline: [67, 62], final: [67, 75] },
  tally: { num_changes: 3, money_spent: 1239118 },
  actions: [
    { seq: 1, tool: "bulldoze", cost: 0 },
    { seq: 2, tool: "build_road", cost: 57790 },
    { seq: 3, tool: "build_road", cost: 1181328 },
  ],
};
const score = { score: 0.6324 };

describe("groupActions", () => {
  it("collapses by tool in first-seen order with summed cost", () => {
    expect(groupActions(record.actions)).toEqual([
      { type: "bulldoze", count: 1, cost: 0 },
      { type: "build_road", count: 2, cost: 1239118 },
    ]);
  });
});

describe("cumulativeSpend", () => {
  it("returns a leading 0 then the running total", () => {
    expect(cumulativeSpend(record.actions)).toEqual([0, 0, 57790, 1239118]);
  });
});

describe("mapRecordToRun", () => {
  it("produces a schema-valid run", () => {
    const run = mapRecordToRun({
      record, score, slug: "opus-4-8", modelName: "Claude Opus 4.8",
      harnessVersion: "v0.1", runDir: "benchmark/runs/x",
      verdict: "did a thing", beats: [{ title: "Survey", body: "read the map" }],
    });
    expect(() => runSchema.parse(run)).not.toThrow();
    expect(run.map).toBe("gridlock-v1");
    expect(run.harnessVersion).toBe("v0.1");
    expect(run.score).toBe(0.6324);
    expect(run.metrics.flow).toEqual({ from: 57, to: 71 });
    expect(run.metrics.jammedJunctions).toEqual({ from: 35, to: 12 });
    expect(run.metrics.changes).toBe(3);
    expect(run.flowSettling).toEqual({ base: [67, 62], final: [67, 75] });
    expect(run.spendSeries).toEqual([0, 0, 57790, 1239118]);
    expect(run.actions).toEqual([
      { type: "bulldoze", count: 1, cost: 0 },
      { type: "build_road", count: 2, cost: 1239118 },
    ]);
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npm test -- build-run`
Expected: FAIL — `@/scripts/lib/build-run` does not exist.

- [ ] **Step 3: Write the implementation**

Create `website/scripts/lib/build-run.ts`:

```ts
import type { Run } from "@/lib/run";

export type WindowStats = {
  flow_mean: number;
  active_vehicles_mean: number;
  population: number;
  congested_meters: number;
  congested_junctions: number;
};

export type ActionEntry = { seq: number; tool: string; cost: number };

export type RunRecord = {
  map: { id: string; source: string; game_version: string };
  baseline: WindowStats;
  final: WindowStats;
  flow_samples: { baseline: number[]; final: number[] };
  tally: { num_changes: number; money_spent: number };
  actions: ActionEntry[];
};

export type ScoreFile = { score: number };

export type GroupedAction = { type: string; count: number; cost: number };

export const groupActions = (actions: ActionEntry[]): GroupedAction[] =>
  actions.reduce<GroupedAction[]>((acc, a) => {
    const existing = acc.find((g) => g.type === a.tool);
    return existing
      ? acc.map((g) => (g === existing ? { ...g, count: g.count + 1, cost: g.cost + a.cost } : g))
      : [...acc, { type: a.tool, count: 1, cost: a.cost }];
  }, []);

export const cumulativeSpend = (actions: ActionEntry[]): number[] =>
  actions.reduce<number[]>((acc, a) => [...acc, acc[acc.length - 1] + a.cost], [0]);

export type Beat = { title: string; body: string };

export type MapInput = {
  record: RunRecord;
  score: ScoreFile;
  slug: string;
  modelName: string;
  harnessVersion: string;
  runDir: string;
  verdict: string;
  beats: Beat[];
};

export const mapRecordToRun = ({
  record, score, slug, modelName, harnessVersion, runDir, verdict, beats,
}: MapInput): Run => ({
  slug,
  modelName,
  map: record.map.id,
  harnessVersion,
  runDir,
  score: score.score,
  verdict,
  metrics: {
    flow: { from: Math.round(record.baseline.flow_mean), to: Math.round(record.final.flow_mean) },
    congestedMetres: { from: Math.round(record.baseline.congested_meters), to: Math.round(record.final.congested_meters) },
    jammedJunctions: { from: record.baseline.congested_junctions, to: record.final.congested_junctions },
    population: { from: record.baseline.population, to: record.final.population },
    activeVehicles: { from: Math.round(record.baseline.active_vehicles_mean), to: Math.round(record.final.active_vehicles_mean) },
    changes: record.tally.num_changes,
    spend: record.tally.money_spent,
  },
  flowSettling: { base: record.flow_samples.baseline, final: record.flow_samples.final },
  spendSeries: cumulativeSpend(record.actions),
  actions: groupActions(record.actions),
  beats,
});
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `npm test -- build-run`
Expected: PASS — all three describe blocks green.

- [ ] **Step 5: Commit**

```bash
git add website/scripts/lib/build-run.ts website/__tests__/build-run.test.ts
git commit -m "feat(website): run-record to Run mapping helpers"
```

---

### Task 7: Serialization and index-registry helpers

**Files:**
- Create: `website/scripts/lib/emit.ts`
- Test: `website/__tests__/emit.test.ts`

- [ ] **Step 1: Write the failing test**

Create `website/__tests__/emit.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { camelVarName, serializeRun, addRunToIndex } from "@/scripts/lib/emit";
import type { Run } from "@/lib/run";

const run: Run = {
  slug: "opus-4-8", modelName: "Claude Opus 4.8", map: "gridlock-v1", harnessVersion: "v0.1",
  runDir: "benchmark/runs/x", score: 0.21, verdict: "a verdict",
  metrics: {
    flow: { from: 58, to: 63 }, congestedMetres: { from: 5133, to: 4682 },
    jammedJunctions: { from: 38, to: 36 }, population: { from: 31562, to: 26787 },
    activeVehicles: { from: 2120, to: 2463 }, changes: 23, spend: 208434,
  },
  flowSettling: { base: [67], final: [67] }, spendSeries: [0, 100], actions: [],
  beats: [{ title: "Survey", body: "line one\n\nline two" }],
};

describe("camelVarName", () => {
  it("strips hyphens", () => {
    expect(camelVarName("opus-4-8")).toBe("opus48");
    expect(camelVarName("gpt-5-4-mini")).toBe("gpt54mini");
  });
});

describe("serializeRun", () => {
  it("emits a defineRun module that re-imports the type", () => {
    const out = serializeRun(run);
    expect(out).toContain('import { defineRun } from "@/lib/run";');
    expect(out).toContain("export const opus48 = defineRun(");
    expect(out).toContain('"harnessVersion": "v0.1"');
  });
});

describe("addRunToIndex", () => {
  const base = `import type { Run } from "@/lib/run";
import { fable5 } from "./fable-5";

export const runs: Run[] = [fable5].sort((a, b) => b.score - a.score);

export const getRun = (slug: string): Run | undefined => runs.find((r) => r.slug === slug);
`;

  it("adds an import and array entry", () => {
    const out = addRunToIndex(base, { slug: "opus-4-8", varName: "opus48" });
    expect(out).toContain('import { opus48 } from "./opus-4-8";');
    expect(out).toContain("[fable5, opus48]");
  });

  it("is idempotent when already present", () => {
    const once = addRunToIndex(base, { slug: "opus-4-8", varName: "opus48" });
    const twice = addRunToIndex(once, { slug: "opus-4-8", varName: "opus48" });
    expect(twice).toBe(once);
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npm test -- emit`
Expected: FAIL — `@/scripts/lib/emit` does not exist.

- [ ] **Step 3: Write the implementation**

Create `website/scripts/lib/emit.ts`:

```ts
import type { Run } from "@/lib/run";

export const camelVarName = (slug: string): string => slug.replace(/-/g, "");

export const serializeRun = (run: Run): string => {
  const body = JSON.stringify(run, null, 2);
  return `import { defineRun } from "@/lib/run";

export const ${camelVarName(run.slug)} = defineRun(${body});
`;
};

export const addRunToIndex = (
  source: string,
  { slug, varName }: { slug: string; varName: string },
): string => {
  if (source.includes(`from "./${slug}"`)) return source;

  const importLine = `import { ${varName} } from "./${slug}";`;
  const lines = source.split("\n");
  const lastImportIdx = lines.reduce(
    (last, line, i) => (line.startsWith("import ") ? i : last),
    0,
  );
  const withImport = [
    ...lines.slice(0, lastImportIdx + 1),
    importLine,
    ...lines.slice(lastImportIdx + 1),
  ].join("\n");

  return withImport.replace(/\[([^\]]*)\]\.sort/, (_m, inner: string) => {
    const trimmed = inner.trim();
    return `[${trimmed.length ? `${trimmed}, ${varName}` : varName}].sort`;
  });
};
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `npm test -- emit`
Expected: PASS — all describe blocks green.

- [ ] **Step 5: Commit**

```bash
git add website/scripts/lib/emit.ts website/__tests__/emit.test.ts
git commit -m "feat(website): run-file serialization and index insertion helpers"
```

---

### Task 8: AI summary module

**Files:**
- Create: `website/scripts/lib/summarize.ts`
- Test: `website/__tests__/summarize.test.ts`

- [ ] **Step 1: Write the failing test (dependency-injected, no network)**

Create `website/__tests__/summarize.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { summarizeRun, summaryShape } from "@/scripts/lib/summarize";

describe("summaryShape", () => {
  it("validates verdict + beats", () => {
    expect(() => summaryShape.parse({ verdict: "x", beats: [{ title: "t", body: "b" }] })).not.toThrow();
    expect(() => summaryShape.parse({ verdict: "x" })).toThrow();
  });
});

describe("summarizeRun", () => {
  it("passes transcript + metrics to the injected model and returns parsed output", async () => {
    const captured: { prompt?: string } = {};
    const result = await summarizeRun({
      createSummary: async ({ prompt }) => {
        captured.prompt = prompt;
        return { verdict: "v", beats: [{ title: "Survey", body: "b" }] };
      },
    })({
      transcript: "TRANSCRIPT TEXT",
      modelName: "Claude Opus 4.8",
      metrics: { score: 0.5, changes: 3, spend: 100 },
    });
    expect(result.verdict).toBe("v");
    expect(captured.prompt).toContain("TRANSCRIPT TEXT");
    expect(captured.prompt).toContain("Claude Opus 4.8");
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npm test -- summarize`
Expected: FAIL — `@/scripts/lib/summarize` does not exist.

- [ ] **Step 3: Write the implementation**

Create `website/scripts/lib/summarize.ts` (all imports at the top so the lint step stays clean — the Anthropic imports are unused until Step 5 but harmless):

```ts
import { z } from "zod";
import Anthropic from "@anthropic-ai/sdk";
import { zodOutputFormat } from "@anthropic-ai/sdk/helpers/zod";

export const summaryShape = z.object({
  verdict: z.string(),
  beats: z.array(z.object({ title: z.string(), body: z.string() })),
});

export type Summary = z.infer<typeof summaryShape>;

export type SummarizeInput = {
  transcript: string;
  modelName: string;
  metrics: Record<string, number>;
};

export type CreateSummary = (args: { prompt: string }) => Promise<Summary>;

export const buildPrompt = ({ transcript, modelName, metrics }: SummarizeInput): string =>
  `You are writing the post-run writeup for a SkylineBench benchmark run by ${modelName}.
The benchmark drops an AI agent into a congested Cities: Skylines II city and scores how well it reduces congestion without harming the city.

Write in a precise, factual, lightly narrative voice grounded ONLY in the transcript and metrics below — no speculation.

Final metrics (composite score out of 1.00 and tallies):
${JSON.stringify(metrics, null, 2)}

Produce:
- verdict: one paragraph (2-4 sentences) summarizing what the agent did and why it scored as it did.
- beats: a chronological list of titled sections describing what the agent did, in order. Each beat has a short title and a body of one or more paragraphs (separate paragraphs with a blank line).

Transcript:
${transcript}`;

export const summarizeRun =
  (deps: { createSummary: CreateSummary }) =>
  async (input: SummarizeInput): Promise<Summary> => {
    const out = await deps.createSummary({ prompt: buildPrompt(input) });
    return summaryShape.parse(out);
  };
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `npm test -- summarize`
Expected: PASS — both describe blocks green.

- [ ] **Step 5: Create the real Anthropic-backed `createSummary` factory**

Append this function to the end of `website/scripts/lib/summarize.ts` (imports already added in Step 3):

```ts
export const anthropicCreateSummary =
  (client: Anthropic): CreateSummary =>
  async ({ prompt }) => {
    const response = await client.messages.parse({
      model: "claude-opus-4-8",
      max_tokens: 16000,
      thinking: { type: "adaptive" },
      messages: [
        {
          role: "user",
          content: [{ type: "text", text: prompt, cache_control: { type: "ephemeral" } }],
        },
      ],
      output_config: { format: zodOutputFormat(summaryShape, "run_summary") },
    });
    if (!response.parsed_output) {
      throw new Error(`model returned no structured summary (stop_reason: ${response.stop_reason})`);
    }
    return response.parsed_output;
  };
```

- [ ] **Step 6: Run the tests again to confirm nothing broke**

Run: `npm test -- summarize`
Expected: PASS — the injected-deps tests still pass (the real factory is not exercised by tests).

- [ ] **Step 7: Commit**

```bash
git add website/scripts/lib/summarize.ts website/__tests__/summarize.test.ts
git commit -m "feat(website): AI run-summary module (verdict + beats)"
```

---

### Task 9: Generator CLI wiring

**Files:**
- Create: `website/scripts/generate-run.ts`

This task is I/O orchestration; verification is a manual end-to-end run.

- [ ] **Step 1: Write the CLI**

Create `website/scripts/generate-run.ts`:

```ts
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import fs from "node:fs";
import path from "node:path";
import Anthropic from "@anthropic-ai/sdk";
import type { RunRecord, ScoreFile } from "@/scripts/lib/build-run";
import { mapRecordToRun } from "@/scripts/lib/build-run";
import { camelVarName, serializeRun, addRunToIndex } from "@/scripts/lib/emit";
import { summarizeRun, anthropicCreateSummary } from "@/scripts/lib/summarize";
import { CURRENT_HARNESS_VERSION } from "@/lib/version";

type Args = {
  runDir: string;
  slug: string;
  modelName: string;
  harnessVersion: string;
  repoRoot: string;
  skipTimelapse: boolean;
};

const parseArgs = (argv: string[]): Args => {
  const get = (flag: string): string | undefined => {
    const i = argv.indexOf(flag);
    return i >= 0 ? argv[i + 1] : undefined;
  };
  const scriptDir = path.dirname(fileURLToPath(import.meta.url)); // website/scripts
  const runDir = get("--run-dir");
  const slug = get("--slug");
  const modelName = get("--model-name");
  if (!runDir || !slug || !modelName) {
    throw new Error("usage: generate-run --run-dir <dir> --slug <slug> --model-name <name> [--harness-version v0.1] [--skip-timelapse] [--repo-root <path>]");
  }
  return {
    runDir,
    slug,
    modelName,
    harnessVersion: get("--harness-version") ?? CURRENT_HARNESS_VERSION,
    repoRoot: get("--repo-root") ?? path.resolve(scriptDir, "../.."),
    skipTimelapse: argv.includes("--skip-timelapse"),
  };
};

const readJson = <T>(file: string): T => JSON.parse(fs.readFileSync(file, "utf8")) as T;

const buildTimelapse = (repoRoot: string, runDir: string): string => {
  const binary = path.join(repoRoot, "broker/target/release/skylinebench");
  if (!fs.existsSync(binary)) {
    console.error("building broker (release)…");
    execFileSync("cargo", ["build", "--release", "--manifest-path", path.join(repoRoot, "broker/Cargo.toml")], { stdio: "inherit" });
  }
  console.error(`generating timelapse for ${runDir}…`);
  execFileSync(binary, ["timelapse", runDir], { stdio: "inherit" });
  return path.join(runDir, "timelapse.mp4");
};

const main = async () => {
  const args = parseArgs(process.argv.slice(2));
  const scriptDir = path.dirname(fileURLToPath(import.meta.url));
  const websiteRoot = path.resolve(scriptDir, "..");

  const record = readJson<RunRecord>(path.join(args.runDir, "run-record.json"));
  const score = readJson<ScoreFile>(path.join(args.runDir, "score.json"));
  const transcript = fs.readFileSync(path.join(args.runDir, "transcript.md"), "utf8");

  console.error("summarizing run via Anthropic API…");
  const client = new Anthropic();
  const summary = await summarizeRun({ createSummary: anthropicCreateSummary(client) })({
    transcript,
    modelName: args.modelName,
    metrics: { score: score.score, changes: record.tally.num_changes, spend: record.tally.money_spent },
  });

  const run = mapRecordToRun({
    record, score, slug: args.slug, modelName: args.modelName,
    harnessVersion: args.harnessVersion, runDir: args.runDir,
    verdict: summary.verdict, beats: summary.beats,
  });

  const runFile = path.join(websiteRoot, "content/runs", `${args.slug}.ts`);
  if (fs.existsSync(runFile)) console.error(`note: overwriting existing ${runFile}`);
  fs.writeFileSync(runFile, serializeRun(run));

  const indexFile = path.join(websiteRoot, "content/runs/index.ts");
  fs.writeFileSync(indexFile, addRunToIndex(fs.readFileSync(indexFile, "utf8"), { slug: args.slug, varName: camelVarName(args.slug) }));

  const publicMp4 = path.join(websiteRoot, "public/runs", `${args.slug}.mp4`);
  const timelapse = args.skipTimelapse
    ? path.join(args.runDir, "timelapse.mp4")
    : buildTimelapse(args.repoRoot, args.runDir);
  if (fs.existsSync(timelapse)) {
    if (fs.existsSync(publicMp4)) console.error(`note: overwriting existing ${publicMp4}`);
    fs.copyFileSync(timelapse, publicMp4);
  } else {
    console.error(`warning: no timelapse at ${timelapse}; skipping mp4 copy`);
  }

  console.error(`done. Review content/runs/${args.slug}.ts before committing.`);
};

main().catch((err) => {
  console.error(err instanceof Error ? err.message : err);
  process.exitCode = 1;
});
```

- [ ] **Step 2: Type-check the script**

Run: `npx tsc --noEmit`
Expected: no type errors. If `@anthropic-ai/sdk/helpers/zod` types are missing, confirm the installed SDK version exports `zodOutputFormat`; otherwise fall back to `client.messages.create` with `output_config.format` built from `zodOutputFormat` per the SDK's current README.

- [ ] **Step 3: Manual end-to-end verification (against a real run dir)**

Pick an existing run directory that has `run-record.json`, `score.json`, and `transcript.md` (e.g. the opus-4-8 run). With `ANTHROPIC_API_KEY` set:

```bash
ANTHROPIC_API_KEY=... npm run generate-run -- \
  --run-dir ../benchmark/runs/<ts> \
  --slug opus-4-8-test \
  --model-name "Claude Opus 4.8" \
  --skip-timelapse
```

Expected: `content/runs/opus-4-8-test.ts` written and validating; `content/runs/index.ts` gains the import + array entry. Then verify:

```bash
npm test -- content
```

Expected: PASS (the new run validates). Clean up the throwaway run:

```bash
git checkout website/content/runs/index.ts && rm website/content/runs/opus-4-8-test.ts
```

- [ ] **Step 4: Commit**

```bash
git add website/scripts/generate-run.ts
git commit -m "feat(website): run generator CLI (metrics + AI summary + timelapse)"
```

---

### Task 10: Updates page; remove Learnings from homepage

**Files:**
- Create: `website/content/updates.ts`
- Create: `website/app/updates/page.tsx`
- Modify: `website/app/page.tsx`
- Delete: `website/components/sections/learnings.tsx`

- [ ] **Step 1: Create the updates content (learnings text moved verbatim)**

Create `website/content/updates.ts`:

```ts
export type UpdateCard = { title: string; body: string };

export type UpdateEntry = {
  title: string;
  date: string;
  intro?: string;
  cards: UpdateCard[];
};

export const updates: UpdateEntry[] = [
  {
    title: "Learnings from the first version",
    date: "v0.1",
    intro: "Pretty much every design decision in the prompt, the scoring, and the sandbox came from something the agent broke first.",
    cards: [
      {
        title: "It read the answer key",
        body: "The first run had no sandbox. The agent noticed it was running in the same directory as the repository, found the harness code, read the scoring function, and sidestepped the benchmark. Its solution: delete everything. No city, no traffic. A perfect congestion score. It took about five minutes to find the loophole I hadn't thought to close. This is why the sandbox exists.",
      },
      {
        title: "When you close a loophole, it finds the margin.",
        body: "The population floor was the first version of this fix: a minimum the population couldn't fall below, supplied in the prompt. The agent found the floor and parked exactly on it. It reduced the population to the minimum viable number and held it there, treating the floor as a target rather than a guardrail, since it figured this was easier than fixing the actual structural problems. The lesson was that a hard limit just tells the agent where the limit is. The fix was to make the penalty a gradient, not a cliff.",
      },
      {
        title: "Without pressure, it took the easy road.",
        body: "Early runs showed a consistent pattern: the agent only widened roads. It would find a bottleneck, upgrade the segment, and call it done. The problem is that widening a road doesn't fix congestion. It moves it. Cars that couldn't get through one junction pile up at the next. The agent knew this, described it in its own reasoning, and did it anyway, because upgrading an existing road is reversible and cheap. Risk aversion looks like competence until you measure outcomes. The change-count penalty exists to force a commitment. This led to changing the scoring function to look at blocked junctions rather than overall flow rate or total metres of congestion.",
      },
    ],
  },
];
```

- [ ] **Step 2: Create the updates page**

Create `website/app/updates/page.tsx`:

```tsx
import type { Metadata } from "next";
import { Nav } from "@/components/layout/nav";
import { Footer } from "@/components/layout/footer";
import { Card } from "@/components/ui/card";
import { updates } from "@/content/updates";

export const metadata: Metadata = { title: "Updates · SkylineBench" };

const UpdatesPage = () => (
  <>
    <Nav variant="run" />
    <header className="run-hero">
      <div className="wrap-narrow">
        <p className="eyebrow">Updates</p>
        <h1 className="display">What changed, and what we learned.</h1>
      </div>
    </header>
    {updates.map((entry) => (
      <section className="section" key={entry.title}>
        <div className="wrap">
          <div className="section-head reveal">
            <p className="eyebrow">{entry.date}</p>
            <h2 className="section-title">{entry.title}</h2>
            {entry.intro && <p className="lead">{entry.intro}</p>}
          </div>
          <div className="choices" style={{ gridTemplateColumns: "1fr" }}>
            {entry.cards.map((card, i) => (
              <Card asChild className="choice reveal" key={card.title}>
                <article>
                  <span className="num">{String(i + 1).padStart(2, "0")}</span>
                  <h3>{card.title}</h3>
                  <p>{card.body}</p>
                </article>
              </Card>
            ))}
          </div>
        </div>
      </section>
    ))}
    <Footer links={false} />
  </>
);

export default UpdatesPage;
```

- [ ] **Step 2b: Confirm Footer accepts a `links` prop**

Run: `grep -n "links" website/components/layout/footer.tsx`
Expected: shows a `links` prop (the run detail page uses `<Footer links={false} />`). If the prop name differs, match the run detail page's usage.

- [ ] **Step 3: Remove Learnings from the homepage**

In `website/app/page.tsx`, delete the `import { Learnings } from "@/components/sections/learnings";` line and the `<Learnings />` element from the JSX.

- [ ] **Step 4: Delete the now-unused Learnings section**

```bash
git rm website/components/sections/learnings.tsx
```

- [ ] **Step 5: Verify it builds**

Run: `npm run build`
Expected: build succeeds; `/updates` renders the entry; the homepage no longer shows the Learnings section.

- [ ] **Step 6: Commit**

```bash
git add website/content/updates.ts website/app/updates/page.tsx website/app/page.tsx
git commit -m "feat(website): move learnings to an /updates page"
```

---

### Task 11: Changelog page

**Files:**
- Create: `website/content/changelog.ts`
- Create: `website/app/changelog/page.tsx`

- [ ] **Step 1: Create the changelog content**

Create `website/content/changelog.ts`:

```ts
export type ChangelogEntry = {
  version: string;
  date: string;
  summary: string;
  changes: string[];
};

export const changelog: ChangelogEntry[] = [
  {
    version: "v0.1",
    date: "2026-06",
    summary: "First public version: the gridlock-v1 scenario, junction-aware congestion scoring, and the anti-cheat sandbox.",
    changes: [
      "gridlock-v1 scenario: a congested city the agent must unblock without harming population or happiness.",
      "Junction-aware composite scoring blending congested metres and jammed junctions, with a graded population-health factor.",
      "Change-count and spend penalties to discourage timid, reversible-only edits.",
      "Deny-repo-read sandbox so the agent cannot read the scoring code.",
    ],
  },
];
```

- [ ] **Step 2: Create the changelog page**

Create `website/app/changelog/page.tsx`:

```tsx
import type { Metadata } from "next";
import { Nav } from "@/components/layout/nav";
import { Footer } from "@/components/layout/footer";
import { Card } from "@/components/ui/card";
import { changelog } from "@/content/changelog";

export const metadata: Metadata = { title: "Changelog · SkylineBench" };

const ChangelogPage = () => (
  <>
    <Nav variant="run" />
    <header className="run-hero">
      <div className="wrap-narrow">
        <p className="eyebrow">Changelog</p>
        <h1 className="display">What changed between versions.</h1>
      </div>
    </header>
    <section className="section">
      <div className="wrap-narrow">
        <ol className="timeline">
          {changelog.map((entry) => (
            <Card asChild className="beat" key={entry.version}>
              <li>
                <h3>{entry.version} <span className="mono">· {entry.date}</span></h3>
                <p>{entry.summary}</p>
                <ul>
                  {entry.changes.map((change) => (
                    <li key={change}>{change}</li>
                  ))}
                </ul>
              </li>
            </Card>
          ))}
        </ol>
      </div>
    </section>
    <Footer links={false} />
  </>
);

export default ChangelogPage;
```

- [ ] **Step 3: Verify it builds**

Run: `npm run build`
Expected: build succeeds; `/changelog` renders the v0.1 entry.

- [ ] **Step 4: Commit**

```bash
git add website/content/changelog.ts website/app/changelog/page.tsx
git commit -m "feat(website): add a /changelog page"
```

---

### Task 12: Navigation links for Updates and Changelog

**Files:**
- Modify: `website/lib/nav-sections.ts`
- Modify: `website/components/layout/nav.tsx`

- [ ] **Step 1: Mark nav entries as anchor vs route**

Replace `website/lib/nav-sections.ts`:

```ts
export type NavSection = { href: string; label: string; route?: boolean };

export const navSections: NavSection[] = [
  { href: "#thesis", label: "Thesis" },
  { href: "#how", label: "How it works" },
  { href: "#scoring", label: "Scoring" },
  { href: "#built", label: "Architecture" },
  { href: "#future", label: "Roadmap" },
  { href: "#results", label: "Results" },
  { href: "/updates", label: "Updates", route: true },
  { href: "/changelog", label: "Changelog", route: true },
];
```

(The `#learnings` entry is removed; `/updates` and `/changelog` are added as routes.)

- [ ] **Step 2: Render route links unconditionally in the nav**

In `website/components/layout/nav.tsx`, the landing variant currently maps `navSections` to anchor links. Anchor links (`href` starting with `#`) only resolve on the landing page, so prefix them with `/` when not on the landing page is unnecessary here — instead, render route entries as-is and anchor entries as-is (anchors are only shown on the landing variant already). Update the `.map` in the `variant === "landing"` branch so route entries always render and survive:

```tsx
              {navSections.map((section) => (
                <a key={section.href} className="nav-link hide-sm" href={section.href}>
                  {section.label}
                </a>
              ))}
```

This is unchanged structurally; the route entries (`/updates`, `/changelog`) render as normal links. Additionally, in the `run` variant branch (the `else` that currently renders only "← Back to results"), add the two route links so they are reachable from `/updates`, `/changelog`, and run pages:

```tsx
          ) : (
            <>
              <a className="nav-link" href="/#results">← Back to results</a>
              <a className="nav-link hide-sm" href="/updates">Updates</a>
              <a className="nav-link hide-sm" href="/changelog">Changelog</a>
            </>
          )}
```

- [ ] **Step 3: Verify it builds**

Run: `npm run build`
Expected: build succeeds; the homepage nav shows Updates and Changelog links (and no Learnings); `/updates` and `/changelog` nav can return to results and reach each other.

- [ ] **Step 4: Commit**

```bash
git add website/lib/nav-sections.ts website/components/layout/nav.tsx
git commit -m "feat(website): nav links for updates and changelog"
```

---

### Task 13: Full verification

- [ ] **Step 1: Run the whole test suite**

Run: `npm test`
Expected: PASS — content, leaderboards, build-run, emit, summarize, and the existing chart test all green.

- [ ] **Step 2: Production build**

Run: `npm run build`
Expected: build succeeds with `/`, `/updates`, `/changelog`, and `/runs/[slug]` routes generated.

- [ ] **Step 3: Lint**

Run: `npm run lint`
Expected: no errors (warnings acceptable if pre-existing).

- [ ] **Step 4: Final commit if any fixups were needed**

```bash
git add -A
git commit -m "chore(website): verification fixups" || echo "nothing to commit"
```

---

## Notes for the implementer

- **Run all commands from `website/`** unless a step says otherwise (the generator's `--repo-root` defaults to two levels up from `website/scripts`).
- **`@/` alias** resolves to the `website/` root in both `tsconfig` and Vitest. The generator runs under `tsx`, which honors `tsconfig` paths; if a `@/` import fails to resolve at runtime under `tsx`, switch that script's imports to relative paths (`../lib/...`).
- **The AI summary is a draft** — review and edit `content/runs/<slug>.ts` (verdict + beats) before committing a real run.
- **`ANTHROPIC_API_KEY`** must be set in the environment for the generator's summary step; it is never read by the website build or tests.
