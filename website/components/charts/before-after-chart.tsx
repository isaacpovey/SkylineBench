import { scaleBars } from "@/lib/chart";

type Row = {
  label: string;
  base: number;
  final: number;
  format?: (n: number) => string;
};

type Props = {
  rows: Row[];
};

const defaultFormat = (n: number): string => n.toLocaleString("en-US");

const ROW_SPACING = 40;
const FIRST_ROW_TOP = 12;
const BAR_X = 112;
const MAX_BAR_WIDTH = 200;

export const BeforeAfterChart = ({ rows }: Props) => {
  // last final-bar bottom = lastRowTop + 12 + 8; add 16px breathing room
  const height = FIRST_ROW_TOP + (rows.length - 1) * ROW_SPACING + 12 + 8 + 16;

  const renderedRows = rows.map(({ label, base, final, format }, i) => {
    const rowTop = FIRST_ROW_TOP + i * ROW_SPACING;
    const fmt = format ?? defaultFormat;
    const [baseW, finalW] = scaleBars({ maxWidth: MAX_BAR_WIDTH })({ values: [base, final] });
    const baseEnd = BAR_X + baseW;
    const finalEnd = BAR_X + finalW;

    return (
      <g key={label}>
        <text x="0" y={rowTop + 9} className="c-axis">{label}</text>
        <rect x={BAR_X} y={rowTop} width={baseW} height="8" rx="2" className="c-base" />
        <text x={baseEnd + 5} y={rowTop + 7} className="c-val">{fmt(base)}</text>
        <rect x={BAR_X} y={rowTop + 12} width={finalW} height="8" rx="2" className="c-final" />
        <text x={finalEnd + 5} y={rowTop + 19} className="c-val c-val-final">{fmt(final)}</text>
      </g>
    );
  });

  return (
    <figure className="chart-card">
      <figcaption>Before → after</figcaption>
      <svg
        viewBox={`0 0 360 ${height}`}
        className="chart-svg"
        role="img"
        aria-label="Before and after metrics"
      >
        {renderedRows}
      </svg>
    </figure>
  );
};
