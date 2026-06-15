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
