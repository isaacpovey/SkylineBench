import { normaliseSeries, polyline } from "@/lib/chart";

type Props = {
  base: number[];
  final: number[];
};

const PLOT_X = 40;
const PLOT_Y = 12;
const PLOT_WIDTH = 304;
const PLOT_HEIGHT = 150;

const shiftPoints = ({ dx, dy }: { dx: number; dy: number }) => (points: string): string =>
  points
    .split(" ")
    .map((pair) => {
      const [x, y] = pair.split(",").map(Number);
      return `${+(x + dx).toFixed(1)},${+(y + dy).toFixed(1)}`;
    })
    .join(" ");

export const SettlingChart = ({ base, final }: Props) => {
  const combined = normaliseSeries([...base, ...final]);
  const normBase = combined.slice(0, base.length);
  const normFinal = combined.slice(base.length);

  const rawBase = polyline({ width: PLOT_WIDTH, height: PLOT_HEIGHT })({ values: normBase });
  const rawFinal = polyline({ width: PLOT_WIDTH, height: PLOT_HEIGHT })({ values: normFinal });

  const shift = shiftPoints({ dx: PLOT_X, dy: PLOT_Y });
  const basePoints = shift(rawBase);
  const finalPoints = shift(rawFinal);

  const allValues = [...base, ...final];
  const maxVal = Math.max(...allValues);
  const minVal = Math.min(...allValues);

  return (
    <figure className="chart-card">
      <figcaption>Flow settling</figcaption>
      <svg
        viewBox="0 0 360 184"
        className="chart-svg"
        role="img"
        aria-label="Flow settling curves"
      >
        <text x="0" y={PLOT_Y + 4} className="c-axis">{Math.round(maxVal)}</text>
        <text x="0" y={PLOT_Y + PLOT_HEIGHT} className="c-axis">{Math.round(minVal)}</text>
        <polyline points={basePoints} className="c-line-base" />
        <polyline points={finalPoints} className="c-line-final" />
      </svg>
    </figure>
  );
};
