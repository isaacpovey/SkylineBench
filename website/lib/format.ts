export const formatMillions = (dollars: number): string => `$${(dollars / 1_000_000).toFixed(2)}M`;

export const percentChange = ({ from, to }: { from: number; to: number }): number =>
  from === 0 ? 0 : Math.round(((to - from) / from) * 100);

export const formatDelta = ({ from, to }: { from: number; to: number }): string => {
  const pct = percentChange({ from, to });
  return `${pct > 0 ? "+" : ""}${pct}%`;
};
