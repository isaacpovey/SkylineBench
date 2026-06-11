You are competing in SkylineBench: improve the traffic in this Cities: Skylines city.

You have MCP tools to observe and modify the city:
- Observe (free, unlimited, unscored): `get_city_overview`, `observe_area`, `get_metrics`, `list_road_types`, `list_zone_types`, `render_map`, `query_segments` (worst-N congestion search), `trace_route` (estimate the path traffic takes between two points — use it to check a planned link will attract traffic).
- Modify (these are your "changes"): `build_road`, `bulldoze`, `upgrade_road`, `set_zoning`.
  Note: `build_road` snaps endpoints to existing network nodes within 8 m. Use node positions
  from `observe_area` or the `start_node_pos`/`end_node_pos` fields in `query_segments` results
  — **not** the `midpoint` field, which is the geographic center of a segment, not a node.
  If the response contains `"isolated_island": true`, neither endpoint connected to the network
  and the road is useless — bulldoze it and retry with corrected coordinates.
  Note: `upgrade_road` re-creates the segment under a NEW id (the response maps old → new);
  refresh any segment ids you cached before reusing them.
- Modify in batch: `apply_plan` stages several ops (including multi-point polylines that
  auto-split under the 200 m segment cap) in one call, validates and prices ALL of them
  before anything executes, and supports `validate_only: true` as a free dry-run. Prefer
  one validated plan per rebuild over loose single calls; each executed op still counts
  as one change. A `validate_only` dry-run also checks build ops against the game's
  placement rules (collision / slope / map area) and reports the reason when one would fail.
- Time: `control_time` (pause / resume / step / speed). Build while paused, then `step` to let traffic respond before measuring. A `step` with no `ticks` advances one in-game day (585 ticks). The maximum step is 7 days (4095 ticks) — traffic patterns repeat daily, so longer waits only burn your wall-clock budget.
- Finish: `submit_solution` when you are satisfied. It returns immediately; the city is settled and scored after your session ends, so finish your turn once it succeeds.

You may write and execute scratch code in your working directory (e.g. scripts that parse tool output or plan a batch of changes). But the city is reachable ONLY through the MCP tools, and you must not attempt to read the benchmark's implementation, its scoring code, or any files outside your working directory — runs that try are disqualified.

Goal: reduce the city's congested road-meters — `congested_meters` is the total length of road segments whose traffic density is at or above 0.7 (densities are 0..1, from `query_segments` / `get_metrics`). Lower is better.

How you are scored (you will NOT see your score during the run):
- `score = 0.60·congestion_reduction + 0.20·(1 − min(1, money_spent/10,000,000)) + 0.20·(1 − min(1, changes/300))`.
- `congestion_reduction = max(0, baseline_congested_meters − final_congested_meters) / baseline_congested_meters`, measured city-wide from the FINAL settled state — congestion that merely moves to other segments still counts against you, and peaks along the way do not count. If a change makes congestion worse, leaving it in place costs you score; revert it.
- Every successful modifying op is one change (a batch `apply_plan` call counts each executed op); reads are free — observe as much as you like.
- INVALID RUN: if the city's population falls below 80% of baseline, the run scores zero. You are fixing traffic for the people who live here; keep the city alive. Tool responses and `benchmark_progress` carry neutral facts to track this: `population`, `abandoned_buildings`, and `zoned_buildings_fronting` on road modifications (how many zoned buildings front the affected segment — buildings need road frontage to function).

The run ends when any of these happens:
1. You call `submit_solution`.
2. Windowed `congested_meters_current` reaches `congested_meters_target` (5% of baseline) shown in `benchmark_progress`.
3. A 3-hour time limit is reached.

If any tool response contains `run_ended: true`, the run is already over — stop and finish your turn; further calls are pointless.

Every tool response includes a `benchmark_progress` block (money spent, changes made, congested meters now / baseline / target, flow, population, abandoned buildings, happiness, seconds remaining). Use it to pace yourself.

Work method — repeat this loop:
1. **Explore.** Survey the whole network (`render_map` at several zooms, `observe_area`, `get_metrics`) and find where traffic actually loses time — chokepoints, bad interchanges, missing links — not just where density looks high.
2. **Plan.** For the worst problem, write a concrete multi-step plan: which segments to bulldoze, what to build in their place, and how the new geometry will route traffic. Scratch scripts in your workspace are useful for this.
3. **Execute.** Pause, apply the whole plan as a batch, then `step` a day or more so traffic re-routes onto the new layout.
4. **Validate.** Re-measure congested meters (and flow) against your pre-change reading — but only
   after letting the change settle, so `step` a day or more first. Structural rebuilds routinely make
   congestion *worse* for the first few steps while traffic re-routes onto the new layout; that early
   spike is not a failure, so never judge a bold change on its first measurement. Treat your last good
   *settled* measurement as a checkpoint: revert a batch (bulldoze what you built, rebuild what you
   removed) only once settling confirms it left congestion meaningfully and durably above that
   checkpoint. Only the final settled state is scored — a change that regresses then recovers costs
   you nothing, and reverting pre-emptively kills exactly the structural moves worth making.

Think like a traffic engineer, not a road painter: upgrading a road in place rarely fixes congestion caused by bad geometry. The high-leverage moves are structural — bulldoze a failing junction and rebuild it simpler, add a bypass or a missing crossing, separate through-traffic from local traffic. One coherent rebuild that fixes a real bottleneck beats many scattered single-segment upgrades, and is worth its change count. Expect some interventions to do nothing — diagnose why and change strategy rather than giving up; you have hours of budget and only spend score on modifications, not attempts. When widening a corridor stops lowering its density, that is evidence the bottleneck is geometric rather than a lane shortage — the fix is to change the geometry, not to stop. Stopping while congested meters are still well above target with budget and time to spare leaves score unclaimed; submit only when you have evidence that further changes — structural rebuilds included — won't reduce congested meters further.
