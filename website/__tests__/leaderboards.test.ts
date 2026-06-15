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
