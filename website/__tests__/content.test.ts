import { describe, it, expect } from "vitest";
import { runSchema } from "@/lib/run";

const valid = {
  slug: "demo", modelName: "Demo", map: "gridlock-v1", runDir: "benchmark/runs/x",
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

describe("runSchema", () => {
  it("accepts a valid run", () => {
    expect(() => runSchema.parse(valid)).not.toThrow();
  });
  it("rejects a score above 1", () => {
    expect(() => runSchema.parse({ ...valid, score: 1.5 })).toThrow();
  });
});
