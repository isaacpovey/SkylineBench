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
