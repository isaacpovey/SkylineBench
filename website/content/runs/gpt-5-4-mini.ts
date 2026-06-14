import { defineRun } from "@/lib/run";

export const gpt54mini = defineRun({
  slug: "gpt-5-4-mini",
  modelName: "GPT-5.4 mini",
  map: "gridlock-v1",
  runDir: "benchmark/runs/20260614-145729",
  score: 0.30282225141670915,
  verdict: `GPT-5.4 mini treated gridlock-v1 as a capacity problem on a single corridor and made just 3 changes — all road upgrades — spending only $31,737 and disturbing almost nothing. It read flow as improving and stopped, but that was a transient: after the harness settled the run, congestion was untouched (congested metres 5105 → 5300, jammed junctions 35 → 41, both slightly worse) and population fell ~10% to 28,473. With a congestion norm of 0.0, the composite (0.30) is almost entirely the reward for spending nearly nothing and changing nearly nothing rather than for fixing anything.`,
  metrics: {
    flow: { from: 57, to: 62 },
    congestedMetres: { from: 5105, to: 5300 },
    jammedJunctions: { from: 35, to: 41 },
    population: { from: 31552, to: 28473 },
    activeVehicles: { from: 2142, to: 1923 },
    changes: 3,
    spend: 31737,
  },
  flowSettling: { base: [67.0, 60.0, 57.0, 56.0, 55.0, 54.0, 52.0, 51.0], final: [60.0, 61.0, 64.0, 65.0, 65.0, 61.0, 61.0, 60.0] },
  spendSeries: [0, 6285, 8514, 31737],
  actions: [
    { type: "upgrade_road", count: 3, cost: 31737 },
  ],
  beats: [
    { title: `Survey: mapping the gridlock`, body: `The agent opened with get_metrics, a citywide query_segments, a city overview, and several render_map passes. The first reading caught a busy moment — flow 51%, congested road 5,970 m, 40 jammed junctions — but the recorded baseline was flow 57%, 5,105 congested metres, and 35 jammed junctions against a population of 31,552. Saturated segments sat at density 1.0 on a handful of critical connectors rather than scattered local streets.

A trace_route from x ≈ −850, z ≈ −1200 and a bounded query over the south-west cloverleaf (x −450..−200, z −950..−450) isolated two pressure points: the central north–south crossing and a worse-looking southern highway interchange. The agent reasoned aloud that it wanted to fix "the one that is actually limiting citywide flow instead of just repainting a busy corridor," then settled on the central east–west arterial as the clearest shared bottleneck.` },
    { title: `Loop 1 — arterial upgrade`, body: `After listing road types and pulling exact endpoints, the agent validated a two-segment plan: upgrade segments 24084 and 21009 to Large Road (estimated $6,285 + $2,229 = $8,514, both valid, no placement risk). It briefly validated a wider three-op variant adding elevated segment 2885 ($14,772) but discarded it, executing only the cheap two-segment plan.

The upgrade replaced 24084 → 28766 and 21009 → 17308, fronting 14 and 12 zoned buildings respectively. The agent then stepped time 585 ticks to let traffic settle.` },
    { title: `Loop 2 — second connector`, body: `Post-settle metrics looked encouraging: flow had climbed to 56.5% and the in-sim flow_percent sampled 60.0, population rose to 31,853, happiness held at 81, and congested metres eased to 5,799 — though still well above the 5,105 baseline, and abandoned buildings had crept from 1 to 3. The agent re-read the corridor, decided "the bottleneck has shifted onto the elevated approach feeding the same junction," and traced from x ≈ −343, z ≈ −532.

It validated and applied a single upgrade of the west elevated connector, segment 35459 → 34103, to Large Road Elevated at $23,223 — by far the run's largest spend, fronting only 3 buildings. Stepping 1,170 ticks afterward, the result was negative: congested metres ticked up to 5,819, jammed junctions rose to 43, and a fourth building abandoned. The agent recognised the move "did not produce a net congestion gain and only added disruption."` },
    { title: `Submission and settling`, body: `Judging the city still healthy and unwilling to risk a larger rebuild, the agent submitted after only 3 changes and $31,737 total spend, noting it "stopped before destabilizing the city further." At handoff it reported flow 57.4%, population 31,911, happiness 81, congested metres 5,819, and 43 jammed junctions.

The harness then settled and scored the run, and the settled state was less flattering than the handoff snapshot. Flow stabilised at 62% (settling samples [60, 61, 64, 65, 65, 61, 61, 60] vs. a baseline that decayed to the low 50s), but congestion ended worse than baseline — congested metres 5,105 → 5,300 and jammed junctions 35 → 41 — so the congestion norm scored 0.0. Population fell ~10% to 28,473 and active vehicles dropped 2,142 → 1,923, giving a health factor of 0.76. The composite 0.30 is carried almost entirely by the money norm (0.003 of budget used) and changes norm (just 3 edits) plus a 5.625 flow gain — credit for restraint and frugality, not for resolving the gridlock the benchmark set out.` },
  ],
});
