You are competing in SkylineBench: improve the traffic in this Cities: Skylines city.

You have MCP tools to observe and modify the city:
- Observe (free, unlimited, unscored): `get_city_overview`, `observe_area`, `get_metrics`, `list_road_types`, `list_zone_types`, `render_map`.
- Modify (these are your "changes"): `build_road`, `bulldoze`, `upgrade_road`, `set_zoning`.
- Time: `control_time` (pause / resume / step / speed). Build while paused, then `step` to let traffic respond before measuring.
- Finish: `submit_solution` when you are satisfied.

Goal: raise city-wide `traffic.flow_percent` (higher = freer-flowing) as much as you can.

How you are scored (you will NOT see your score during the run):
- Most of the score is how much you improve flow.
- You are rewarded for spending less money and for making fewer modifying actions. Reads are free — observe as much as you like.
- INVALID RUN: if you reduce the number of active vehicles too far (you must not "fix" traffic by depopulating the city), the run scores zero. Keep the city alive.

The run ends when any of these happens:
1. You call `submit_solution`.
2. Flow reaches the target shown as `flow_target` in `benchmark_progress`.
3. A 3-hour time limit is reached.

Every tool response includes a `benchmark_progress` block (money spent, changes made, current flow vs target, seconds remaining). Use it to decide when to stop — a good-enough solution submitted early beats an expensive one.

Work method: observe the network and metrics, find the congestion, make targeted road/zoning changes, step the simulation to let traffic respond, re-measure, and iterate. When the gains plateau, `submit_solution`.
