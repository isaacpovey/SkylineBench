You are competing in SkylineBench: improve the traffic in this Cities: Skylines city.

You have MCP tools to observe and modify the city:
- Observe (free, unlimited, unscored): `get_city_overview`, `observe_area`, `get_metrics`, `list_road_types`, `list_zone_types`, `render_map`.
- Modify (these are your "changes"): `build_road`, `bulldoze`, `upgrade_road`, `set_zoning`.
- Time: `control_time` (pause / resume / step / speed). Build while paused, then `step` to let traffic respond before measuring. A `step` with no `ticks` advances one in-game day (585 ticks). The maximum step is 3 days (1755 ticks) — traffic patterns repeat daily, so longer waits only burn your wall-clock budget.
- Finish: `submit_solution` when you are satisfied. It returns immediately; the city is settled and scored after your session ends, so finish your turn once it succeeds.

You may write and execute scratch code in your working directory (e.g. scripts that parse tool output or plan a batch of changes). But the city is reachable ONLY through the MCP tools, and you must not attempt to read the benchmark's implementation, its scoring code, or any files outside your working directory — runs that try are disqualified.

Goal: raise city-wide `traffic.flow_percent` (higher = freer-flowing) as much as you can.

How you are scored (you will NOT see your score during the run):
- Most of the score is how much you improve flow.
- You are rewarded for spending less money and for making fewer modifying actions. Reads are free — observe as much as you like.
- INVALID RUN: if you reduce the number of active vehicles too far (you must not "fix" traffic by depopulating the city), the run scores zero. Keep the city alive.

The run ends when any of these happens:
1. You call `submit_solution`.
2. Flow reaches the target shown as `flow_target` in `benchmark_progress`.
3. A 3-hour time limit is reached.

If any tool response contains `run_ended: true`, the run is already over — stop and finish your turn; further calls are pointless.

Every tool response includes a `benchmark_progress` block (money spent, changes made, current flow vs target, seconds remaining). Use it to pace yourself.

Work method — repeat this loop:
1. **Explore.** Survey the whole network (`render_map` at several zooms, `observe_area`, `get_metrics`) and find where traffic actually loses time — chokepoints, bad interchanges, missing links — not just where density looks high.
2. **Plan.** For the worst problem, write a concrete multi-step plan: which segments to bulldoze, what to build in their place, and how the new geometry will route traffic. Scratch scripts in your workspace are useful for this.
3. **Execute.** Pause, apply the whole plan as a batch, then `step` a day or more so traffic re-routes onto the new layout.
4. **Validate.** Re-measure flow against your pre-change reading. Keep what worked, revise or revert what didn't, then loop back to exploring.

Think like a traffic engineer, not a road painter: upgrading a road in place rarely fixes congestion caused by bad geometry. The high-leverage moves are structural — bulldoze a failing junction and rebuild it simpler, add a bypass or a missing crossing, separate through-traffic from local traffic. One coherent rebuild that fixes a real bottleneck beats many scattered single-segment upgrades, and is worth its change count. Expect some interventions to do nothing — diagnose why and change strategy rather than giving up; you have hours of budget and only spend score on modifications, not attempts. Submit when you have evidence further changes won't pay for themselves.
