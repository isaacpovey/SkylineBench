export const formatMillions = (dollars: number): string => `$${(dollars / 1_000_000).toFixed(2)}M`;
