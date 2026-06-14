import { defineRun } from "@/lib/run";

export const gpt55 = defineRun({
  slug: "gpt-5-5",
  modelName: "GPT-5.5",
  map: "gridlock-v1",
  runDir: "benchmark/runs/20260614-142734",
  score: 0.29148260367795353,
  verdict: `GPT-5.5 correctly diagnosed gridlock-v1 as a west-interchange weave plus a saturated urban/elevated corridor, but every durable fix it built was ultimately torn back out, so the network it submitted is the original map with only renumbered segments. Its one structural idea — an elevated highway bypass — was first built in the wrong directed orientation, rebuilt reversed for a second $155k, then bulldozed entirely; a $157k corridor widening to Large Road was reverted within two in-game days when abandonment climbed. The "improvement" the harness rewarded is illusory: across the run population fell ~9% (31,571 → 28,696) and active vehicles dropped, so flow climbed to a reported 100% on emptier roads while congested metres actually rose (5086 → 6225) and jammed junctions went from 35 to 38. With congestion norm at 0.0, $526,978 spent for no lasting network change, and the health factor (0.79) dragging on a degenerate flow_gain, the composite lands at 0.29.`,
  metrics: {
    flow: { from: 57, to: 100 },
    congestedMetres: { from: 5086, to: 6225 },
    jammedJunctions: { from: 35, to: 38 },
    population: { from: 31571, to: 28696 },
    activeVehicles: { from: 2160, to: 2365 },
    changes: 34,
    spend: 526978,
  },
  flowSettling: { base: [67.0, 61.0, 57.0, 56.0, 54.0, 53.0, 52.0, 53.0], final: [100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0] },
  spendSeries: [0, 38863, 77726, 116589, 155452, 155452, 155452, 155452, 155452, 194315, 233178, 272041, 310904, 331177, 356587, 388908, 406710, 433712, 443438, 452871, 461004, 468529, 475727, 485432, 497699, 504162, 515413, 518806, 522331, 525085, 526978, 526978, 526978, 526978, 526978],
  actions: [
    { type: "upgrade_road", count: 18, cost: 216074 },
    { type: "build_road", count: 8, cost: 310904 },
    { type: "bulldoze", count: 8, cost: 0 },
  ],
  beats: [
    { title: `Survey: two clusters, a 53% baseline`, body: `Opening reads put traffic flow at 53% with 35 congested junctions at start (40 on the first live sample) and 5,086 congested metres. After get_city_overview, get_metrics, list_road_types, and several query_segments / observe_area / render_map passes, the agent isolated two structural hotspots: a dense ramp/highway knot around x ≈ −300, z ≈ −800, and an urban/elevated cluster around x ≈ −100, z ≈ −1850. It read the west knot as an interchange-geometry problem — through traffic and turning traffic forced through a tight weave — rather than a simple lane shortage, and chose to test a bypass before any rebuild.` },
    { title: `Loop 1 — elevated bypass at the west weave`, body: `The agent validated then built a Highway Elevated through-link from (−699.5, −871.0) to (−148.6, −826.4), snapping into the existing eastbound highway at nodes 25913, 19015, and 16029 with two new intermediate elevated nodes. Four segments (2048, 19727, 12925, 4629) at $38,863 each — $155,452 total — with zero fronting buildings touched.

A 1,755-tick step nudged flow 53% → 56% and trimmed congested metres slightly, but abandonment rose 1 → 3 and happiness slipped to 81. Population still grew, so the agent kept going.` },
    { title: `Loop 2 — reverse link blocked by collisions`, body: `Wanting a matching bypass for the opposite through-movement, the agent validated a build_polyline along the reverse alignment. Two of the six segments failed validation with OBJECT_COLLISION against buildings 37023 and 42940. Unwilling to bulldoze service/ramp objects for a speculative link, it abandoned the reverse build entirely.` },
    { title: `Loop 3 — the bypass was pointing the wrong way`, body: `trace_route then exposed the real problem with Loop 1: the bypass had been built in the wrong directed orientation. It dead-ended for the west-to-east path and only made sense reversed for the heavy 2662 → 25913 movement. The agent bulldozed all four segments (2048, 19727, 12925, 4629) at $0 and rebuilt the same corridor reversed for another $155,452 (new segments 17470, 10690, 11248, 36686), bringing spend to $310,904.

This time it worked as intended: congested junctions fell 40 → 36 and flow held at 57% while population kept rising. Congested metres were still above the baseline, so the agent turned to the urban cluster.` },
    { title: `Loop 4 — corridor widening on the urban cluster`, body: `The remaining hotspot was a saturated north-south surface corridor (21 segments reading density 1.0) plus short elevated connectors. Rather than cut a new road through neighborhoods, the agent upgraded nine segments in place — five to Large Road Elevated and four to Large Road Decoration Trees (segments 2816, 3211, 7147, 11811, 13434, 18030, 26700, 26921, 27862) — for $157,625, total spend $468,529.

Validation flagged 8–17 zoned buildings fronting most of these segments. Immediate junction congestion dropped 36 → 31, but the agent prudently stepped only two in-game days before committing further.` },
    { title: `Loop 5 — abandonment climbs, upgrade reverted`, body: `The short step undid the gain: congested junctions snapped back to 36 and abandoned buildings rose to 6. Reading the corridor widening as the cause, the agent reverted all nine segments back to Basic Road / Basic Road Elevated for $58,449, pushing total spend to $526,978 across 30 changes.` },
    { title: `Loop 6 — the degenerate flow reading`, body: `A longer settle first looked encouraging — flow climbed to ~63% then ~67%/~73% and junctions briefly hit 28 — but the next interval revealed the truth: population fell 31,854 → 30,163 and happiness dropped to 79. Tellingly, get_metrics reported flow_percent 100.0 while active vehicles had fallen to 1,857; the corridor was emptying, not clearing. The agent recognised that traffic was "improving" only because the city was shrinking and resolved to remove its last durable addition if the slide continued.` },
    { title: `Loop 7 — strip everything, submit the original network`, body: `The decline persisted, so the agent bulldozed the surviving bypass as well — segments 10690, 11248, 17470, 36686 at $0 — taking changes to 34 with no durable road structure remaining. Final settling drove population down to 27,926 and happiness to 76 while flow rose to 89% then a city-status reading of 94.75 (flow_percent 100, active vehicles ~1,882). Concluding that every intervention with plausible livability impact had been undone and the population drop was unresolved, the agent submitted a network restored to its original road types.

The harness settled the run at a flat 100% flow, but the underlying state was worse than baseline: congested metres 6,225 and jammed junctions 38, against 5,086 / 35 at start. Congestion norm scored 0.0, money norm 0.05, changes norm 0.11; with health factor 0.79 against an inflated flow_gain of 43.4, the composite came to 0.29 — a half-million dollars spent to hand back the original map on a depopulated city.` },
  ],
});
