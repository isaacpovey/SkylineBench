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
  const groups = runs.reduce<Map<string, { map: string; harnessVersion: string; runs: Run[] }>>(
    (acc, run) => {
      const key = `${run.map}::${run.harnessVersion}`;
      const existing = acc.get(key);
      return acc.set(key, {
        map: run.map,
        harnessVersion: run.harnessVersion,
        runs: [...(existing?.runs ?? []), run],
      });
    },
    new Map(),
  );

  return Array.from(groups.values()).map(({ map, harnessVersion, runs: groupRuns }) => ({
    map,
    harnessVersion,
    label: `${map} · ${harnessVersion}`,
    runs: [...groupRuns].sort((a, b) => b.score - a.score),
  }));
};

export const pickCurrent = (boards: Leaderboard[], version: string): Leaderboard => {
  const match = boards.find((b) => b.harnessVersion === version) ?? boards[0];
  if (!match) throw new Error("pickCurrent: no leaderboards to choose from");
  return match;
};

export const leaderboards = buildLeaderboards(allRuns);
export const currentLeaderboard = pickCurrent(leaderboards, CURRENT_HARNESS_VERSION);
