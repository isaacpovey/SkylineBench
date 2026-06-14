import { describe, it, expect } from "vitest";
import { scaleBars, polyline, normaliseSeries } from "@/lib/chart";

describe("scaleBars", () => {
  it("maps the largest value to full width", () => {
    const bars = scaleBars({ maxWidth: 200 })({ values: [50, 100] });
    expect(bars).toEqual([100, 200]);
  });
});

describe("normaliseSeries", () => {
  it("maps min to 0 and max to 1", () => {
    expect(normaliseSeries([10, 20])).toEqual([0, 1]);
  });
  it("returns zeros for a flat series", () => {
    expect(normaliseSeries([5, 5])).toEqual([0, 0]);
  });
});

describe("polyline", () => {
  it("spreads points across the width and inverts y", () => {
    const pts = polyline({ width: 100, height: 100 })({ values: [0, 1] });
    expect(pts).toBe("0,100 100,0");
  });
});
