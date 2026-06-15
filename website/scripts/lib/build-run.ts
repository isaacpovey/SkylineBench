import type { Run } from "@/lib/run";

export type WindowStats = {
  flow_mean: number;
  active_vehicles_mean: number;
  population: number;
  congested_meters: number;
  congested_junctions: number;
};

export type ActionEntry = { seq: number; tool: string; cost: number };

export type RunRecord = {
  map: { id: string; source: string; game_version: string };
  baseline: WindowStats;
  final: WindowStats;
  flow_samples: { baseline: number[]; final: number[] };
  tally: { num_changes: number; money_spent: number };
  actions: ActionEntry[];
};

export type ScoreFile = { score: number };

export type GroupedAction = { type: string; count: number; cost: number };

export const groupActions = (actions: ActionEntry[]): GroupedAction[] =>
  actions.reduce<GroupedAction[]>((acc, a) => {
    const existing = acc.find((g) => g.type === a.tool);
    return existing
      ? acc.map((g) => (g === existing ? { ...g, count: g.count + 1, cost: g.cost + a.cost } : g))
      : [...acc, { type: a.tool, count: 1, cost: a.cost }];
  }, []);

export const cumulativeSpend = (actions: ActionEntry[]): number[] =>
  actions.reduce<number[]>((acc, a) => [...acc, acc[acc.length - 1] + a.cost], [0]);

export type Beat = { title: string; body: string };

export type MapInput = {
  record: RunRecord;
  score: ScoreFile;
  slug: string;
  modelName: string;
  harnessVersion: string;
  runDir: string;
  verdict: string;
  beats: Beat[];
};

export const mapRecordToRun = ({
  record, score, slug, modelName, harnessVersion, runDir, verdict, beats,
}: MapInput): Run => ({
  slug,
  modelName,
  map: record.map.id,
  harnessVersion,
  runDir,
  score: score.score,
  verdict,
  metrics: {
    flow: { from: Math.round(record.baseline.flow_mean), to: Math.round(record.final.flow_mean) },
    congestedMetres: { from: Math.round(record.baseline.congested_meters), to: Math.round(record.final.congested_meters) },
    jammedJunctions: { from: record.baseline.congested_junctions, to: record.final.congested_junctions },
    population: { from: record.baseline.population, to: record.final.population },
    activeVehicles: { from: Math.round(record.baseline.active_vehicles_mean), to: Math.round(record.final.active_vehicles_mean) },
    changes: record.tally.num_changes,
    spend: record.tally.money_spent,
  },
  flowSettling: { base: record.flow_samples.baseline, final: record.flow_samples.final },
  spendSeries: cumulativeSpend(record.actions),
  actions: groupActions(record.actions),
  beats,
});
