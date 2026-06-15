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
