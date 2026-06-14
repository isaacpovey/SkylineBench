import { z } from "zod";

const pair = z.object({ from: z.number(), to: z.number() });

export const runSchema = z.object({
  slug: z.string(),
  modelName: z.string(),
  map: z.string(),
  runDir: z.string(),
  score: z.number().min(0).max(1),
  verdict: z.string(),
  metrics: z.object({
    flow: pair,
    congestedMetres: pair,
    jammedJunctions: pair,
    population: pair,
    activeVehicles: pair,
    changes: z.number(),
    spend: z.number(),
  }),
  flowSettling: z.object({ base: z.array(z.number()), final: z.array(z.number()) }),
  spendSeries: z.array(z.number()),
  actions: z.array(z.object({ type: z.string(), count: z.number(), cost: z.number() })),
  beats: z.array(z.object({ title: z.string(), body: z.string() })),
});

export type Run = z.infer<typeof runSchema>;

export const defineRun = (run: Run): Run => runSchema.parse(run);
