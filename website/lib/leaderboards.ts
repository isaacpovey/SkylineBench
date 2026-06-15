import type { Run } from "@/lib/run";
import { runs as allRuns } from "@/content/runs";
import { CURRENT_HARNESS_VERSION } from "@/lib/version";

export type Leaderboard = {
  map: string;
  harnessVersion: string;
  label: string;
  runs: Run[];
};

export const buildLeaderboards = (runs: Run[]): Leaderboard[] => {
  const groups = runs.reduce<Map<string, Run[]>>((acc, run) => {
    const key = `${run.map}::${run.harnessVersion}`;
    return acc.set(key, [...(acc.get(key) ?? []), run]);
  }, new Map());

  return Array.from(groups.entries()).map(([key, groupRuns]) => {
    const [map, harnessVersion] = key.split("::");
    return {
      map,
      harnessVersion,
      label: `${map} · ${harnessVersion}`,
      runs: [...groupRuns].sort((a, b) => b.score - a.score),
    };
  });
};

export const pickCurrent = (boards: Leaderboard[], version: string): Leaderboard =>
  boards.find((b) => b.harnessVersion === version) ?? boards[0];

export const leaderboards = buildLeaderboards(allRuns);
export const currentLeaderboard = pickCurrent(leaderboards, CURRENT_HARNESS_VERSION);
