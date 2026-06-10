# SkylineBench Improvements — Plan Overview

Derived from the failure analysis of run `20260609-210135` (flow 56.0 → peak 60.9 → crashed at
51.4, below baseline; agent never submitted; fatal error was upgrading the spine to one-way
Highway prefabs it could not see the direction of). Six plans, each independently shippable.

## Execution order and dependencies

| # | Plan | Depends on | Why this order |
|---|------|-----------|----------------|
| 1 | [harness-reliability](2026-06-10-skylinebench-harness-reliability.md) | — | Protects every future run (sleep, concurrent runs, step timeouts). |
| 2 | [scoring-and-prompt](2026-06-10-skylinebench-scoring-and-prompt.md) | — | Tiny; raises change cap/budget and publishes scoring constants. |
| 3 | [segment-observability](2026-06-10-skylinebench-segment-observability.md) | — | One-way/direction exposure, density fix, road-only filter, `query_segments`. Foundation for 4 & 6. |
| 4 | [render-timelapse](2026-06-10-skylinebench-render-timelapse.md) | 3 | Congestion-coloured map with direction arrows + persisted per-run render frames. |
| 5 | [batch-actions](2026-06-10-skylinebench-batch-actions.md) | — (better after 3) | `apply_plan` op envelope with validate-only dry-run and polyline expansion. |
| 6 | [trace-route](2026-06-10-skylinebench-trace-route.md) | 3 | Broker-side route estimation ("why is my new road unused?"). |

Plans 4–6 each add MCP tools, so each updates the `registers_*_tools` test lists in
`broker/src/tools.rs` and `broker/src/benchmark/server.rs`. The expected lists written in each
plan assume the order above; if executed out of order, keep previously added tool names in the
sorted list.

## Premise guard

Every change gives the agent better *information* or cheaper *mechanics*, never decisions:
the spatial reasoning (which junction is broken, what geometry fixes it, what the knock-on
effects are) remains entirely the agent's. Long-term these tool shapes (op envelope, queryable
segments, route tracing, persisted renders) extend to transit lines, district re-zoning, and
city-from-scratch scenarios without redesign.
