export const normaliseSeries = (values: number[]): number[] => {
  const min = Math.min(...values);
  const max = Math.max(...values);
  const span = max - min;
  return values.map((v) => (span === 0 ? 0 : (v - min) / span));
};

export const scaleBars =
  ({ maxWidth }: { maxWidth: number }) =>
  ({ values }: { values: number[] }): number[] => {
    const max = Math.max(...values);
    return values.map((v) => (max === 0 ? 0 : (v / max) * maxWidth));
  };

export const polyline =
  ({ width, height }: { width: number; height: number }) =>
  ({ values }: { values: number[] }): string => {
    const step = values.length > 1 ? width / (values.length - 1) : 0;
    return values
      .map((v, i) => `${+(i * step).toFixed(2)},${+((1 - v) * height).toFixed(2)}`)
      .join(" ");
  };

export const shiftPoints =
  ({ dx, dy }: { dx: number; dy: number }) =>
  (points: string): string =>
    points
      .split(" ")
      .map((pair) => {
        const [x, y] = pair.split(",").map(Number);
        return `${+(x + dx).toFixed(1)},${+(y + dy).toFixed(1)}`;
      })
      .join(" ");
