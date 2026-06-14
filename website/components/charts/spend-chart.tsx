import { normaliseSeries, polyline, shiftPoints } from "@/lib/chart";

type Props = {
  series: number[];
  total: number;
  changes: number;
};

const PLOT_X = 8;
const PLOT_Y = 12;
const PLOT_WIDTH = 300;
const PLOT_HEIGHT = 150;

const formatMillions = (dollars: number): string => {
  const m = dollars / 1_000_000;
  return `$${m.toFixed(2)}M`;
};

export const SpendChart = ({ series, total, changes }: Props) => {
  const rawPoints = polyline({ width: PLOT_WIDTH, height: PLOT_HEIGHT })({
    values: normaliseSeries(series),
  });
  const points = shiftPoints({ dx: PLOT_X, dy: PLOT_Y })(rawPoints);

  return (
    <figure className="chart-card">
      <figcaption>Cumulative spend</figcaption>
      <svg
        viewBox="0 0 360 188"
        className="chart-svg"
        role="img"
        aria-label="Cumulative spend"
      >
        <polyline points={points} className="c-line-final" />
        <text x={PLOT_X} y="180" className="c-val c-val-final">
          {formatMillions(total)} · {changes} changes
        </text>
      </svg>
    </figure>
  );
};
