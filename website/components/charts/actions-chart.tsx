import { scaleBars } from "@/lib/chart";

type Action = {
  type: string;
  count: number;
  cost: number;
};

type Props = {
  actions: Action[];
};

const FIRST_ROW_TOP = 10;
const ROW_SPACING = 44;
const BAR_X = 120;
const MAX_BAR_WIDTH = 150;
const BAR_HEIGHT = 14;

const formatCost = (dollars: number): string => {
  if (dollars === 0) return "$0";
  if (dollars >= 1_000_000) return `$${(dollars / 1_000_000).toFixed(2)}M`;
  if (dollars >= 1_000) return `$${+(dollars / 1_000).toFixed(1)}k`;
  return `$${dollars}`;
};

export const ActionsChart = ({ actions }: Props) => {
  const widths = scaleBars({ maxWidth: MAX_BAR_WIDTH })({ values: actions.map((a) => a.count) });
  // height: last bar bottom + 34px breathing room
  const height = FIRST_ROW_TOP + (actions.length - 1) * ROW_SPACING + BAR_HEIGHT + 34;

  const renderedRows = actions.map(({ type, count, cost }, i) => {
    const rowTop = FIRST_ROW_TOP + i * ROW_SPACING;
    const barWidth = widths[i] ?? 0;
    const barEnd = BAR_X + barWidth;

    return (
      <g key={type}>
        <text x="0" y={rowTop + 9} className="c-axis">{type}</text>
        <rect x={BAR_X} y={rowTop} width={barWidth} height={BAR_HEIGHT} rx="3" className="c-final" />
        <text x={barEnd + 6} y={rowTop + 11} className="c-val">
          {count} · {formatCost(cost)}
        </text>
      </g>
    );
  });

  return (
    <figure className="chart-card">
      <figcaption>Actions by type</figcaption>
      <svg
        viewBox={`0 0 360 ${height}`}
        className="chart-svg"
        role="img"
        aria-label="Actions by type"
      >
        {renderedRows}
      </svg>
    </figure>
  );
};
