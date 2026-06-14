# Elevation-aware road tooling — design

**Date:** 2026-06-14
**Status:** approved (brainstorming), pending implementation plan
**Branch:** `improve-road-build-tools-for-height`

## Problem

Benchmark models underperform on structural traffic fixes (overpasses, off-ramps). The theory: they cannot **see height** and cannot **build elevated structures** correctly.

Confirmed gaps in the current code:
- `render_map` is purely 2-D top-down — it discards `y` entirely. There is **no agent-callable 3-D view**; the only angled screenshots are captured automatically for the human timelapse.
- The build path (`GameActions.BuildRoad`) uses raw `NetManager.CreateSegment` with whatever `y` is passed — it never sets `m_elevation`, never picks the elevated/bridge prefab, never places pillars. Elevated builds are fragile/floating.
- Validation (`BuildValidator`) only checks buildings/area/slope — it misses water and road-vs-road collisions, and there is no visual confirmation of a proposed plan.

## Spike results (runtime-verified 2026-06-14, CS1 1.21.1-f9)

All mechanisms proven against the live game via a throwaway `POST /spike/road` endpoint (`mod/src/bridge/RoadToolSpike.cs`). See memory `nettool-road-tooling-spike`.

The workhorse is `NetTool.CreateNode` (public static):
`ToolErrors CreateNode(NetInfo info, ControlPoint start, ControlPoint middle, ControlPoint end, FastList<NetTool.NodePosition> nodeBuffer, int maxSegments, bool test, bool visualize, bool autoFix, bool needMoney, bool invert, bool switchDir, ushort relocateBuildingID, out ushort node, out ushort segment, out int cost, out int productionRate)`.

| Capability | Mechanism | Verified |
|---|---|---|
| Native validation | `test:true` | ✅ real `ToolErrors`; ground road through a building → `ObjectCollision\|CannotBuildOnWater`; same span +12 m → `None` |
| Elevated build | `test:false` + elevation on control points | ✅ requested "Basic Road" → built "Basic Road Elevated" with pillars |
| Non-mutating ghost preview | `IRenderableManager` + `CreateNode(test:true, visualize:true)` in `EndRendering` | ✅ renders the proposed road, builds nothing, toggles off cleanly |
| 3-D view | `POST /screenshot` angled (`top_down:false`) | ✅ pillars/clearance clearly visible |

Build-then-rollback also works cleanly but is **not used** — a non-mutating preview avoids rollback side effects (despawn refunds, frontage/zone recalc, build-index churn).

## Decisions

1. **Build via `NetTool.CreateNode`**, retiring `BuildValidator`. One path gives elevation-aware building *and* the game's native validation.
2. **Per-endpoint elevation in metres above terrain** (`from_elevation`/`to_elevation`, default 0). Expresses flat (`0→0`), overpass (`12→12`), and ramp (`0→12`). The mod samples terrain and sets control points.
3. **`validate_plan` ghost preview is opt-in** via `preview:true` (native-validation JSON is always free).
4. **Delete `BuildValidator.cs`** (keep `Frontage` for `zoned_buildings_fronting`).
5. **Ramps handled by per-endpoint elevation + elevation-aware snapping** in v1; verify live, iterate.

## Architecture

### Mod side (C#)

**`RoadBuilder` helper** (shared by build + validate), replacing the raw-`CreateSegment` body of `GameActions.BuildRoad`/`ValidateRoad`:
- Construct three `ControlPoint`s from `start`/`end` + per-endpoint elevation: `m_position.y = TerrainManager.SampleDetailHeight(p) + elevation`, `m_elevation = elevation`, `m_direction = normalizeXZ(end-start)`.
- **Elevation-aware snapping**: keep 8 m XZ snap and the `≥8 segments → TooManyConnections` guard, but only snap to a node whose height is compatible with the endpoint's elevation (so a ramp foot snaps to ground, its top to the elevated road). Set `ControlPoint.m_node` on a snap.
- Call `CreateNode(test:false, visualize:false, autoFix:true, needMoney:false, invert:false, switchDir:false, relocateBuildingID:0, …)`. `test:true` for validate.
- Map `ToolErrors` → `ErrorCode` (see below). Return created node(s)/segment(s) and `zoned_buildings_fronting` via `Frontage`.

**`ToolErrors` → reason mapping** (mod `ErrorCode` + broker `ActionError`, SCREAMING_SNAKE):
- `ObjectCollision → OBJECT_COLLISION`
- `CannotBuildOnWater → CANNOT_BUILD_ON_WATER` *(new)*
- `SlopeTooSteep → SLOPE_TOO_STEEP`
- `HeightTooHigh → HEIGHT_TOO_HIGH` *(new — the deferred HEIGHT_LIMIT check)*
- `OutOfArea → OUT_OF_AREA`
- `TooManyConnections → TOO_MANY_CONNECTIONS`
- `TooShort`/`InvalidShape` → `DEGENERATE_SEGMENT`/`INVALID_ARGS`
- anything else → `UNKNOWN`, plus raw `error_bits` for debugging.

**`PreviewRenderer : IRenderableManager`** (productionised from the spike):
- Holds a **list** of proposed segments (a plan has many build ops).
- `EndRendering(cameraInfo)` loops, rendering each via `CreateNode(test:true, visualize:true)`.
- Registered once (`RenderManager.RegisterRenderableManager`; no Unregister — gate with `Active`).
- Interface members: `GetName`, `GetDrawCallData` (return `default`), `CheckReferences`, `InitRenderData`, `bool CalculateGroupData(...)` (return false), `void PopulateGroupData(...)`, `BeginRendering`, `EndRendering`, `BeginOverlay`, `EndOverlay`, `UndergroundOverlay` — all no-ops except `EndRendering`.
- Endpoints: `POST /preview {ops:[{from,to,from_elevation,to_elevation,prefab}]}` sets the list + `Active=true`; `POST /preview-clear` sets `Active=false`.

### Broker side (Rust)

**Contract / args:**
- `BuildRoadArgs` gains `from_elevation: f32 = 0`, `to_elevation: f32 = 0`.
- `PlanOp::BuildRoad` gains the same; `PlanOp::BuildPolyline` gains optional `elevations: Vec<f32>` (len == points; default all 0) for a sloped profile. `plan::expand` interpolates per-chunk elevation when splitting a span.
- `bridge_client.build_road`/`validate_road` send the elevation fields.
- `validate.rs` keeps structural pre-checks (bounds, length, prefab); native checks come from the mod.

**`view_3d` tool** (in `tools.rs` Skyline + `benchmark/server.rs` BenchmarkServer):
- Args: `x`, `z`, `size?` (default 350), `top_down?` (default false).
- Wraps `capture_screenshot`/`CameraShot`; returns image content + a short legend; benchmark server attaches `city_status` (like `render_map`).

**`validate_plan` preview** (extend `apply_plan(validate_only:true)`):
- After native validation, when `preview:true` and there are build ops: `POST /preview` with all build ops → `region_shot` screenshot framing them → `POST /preview-clear` → return the ghost image beside the JSON results.

### Agent-facing
- `benchmark/prompt.md`: a paragraph on overpasses/ramps (separate through-traffic; off-ramp via `0→12`), `view_3d` (inspect structure in 3-D), and validating-with-preview before building.
- Tool descriptions updated for the new params/tools.

## Testing & rollout
- **Broker (unit):** elevation arg parsing + threading; `BuildPolyline` elevation interpolation in `expand`; `view_3d` against the mock; `validate_plan` preview wiring (extend mock with `/preview`, `/preview-clear`); `ToolErrors`→reason round-trips.
- **Mod:** pure `ToolErrors→reason` mapping unit-tested; game-coupled logic verified live (DISCOVERY-style) and documented.
- **Remove the spike**: `RoadToolSpike.cs`, the `/spike/road` route + handler, and the csproj entry.
- **Rollout:** rebuild mod → benchmark run; confirm models build overpasses/ramps and improve — the validation of the theory.

## Out of scope (v1)
- Dedicated interchange/ramp-builder tooling beyond per-endpoint elevation.
- The green/red validity *coloring* overlay on the ghost (geometry preview + JSON validity is enough; revisit if needed).
- Tunnels (negative elevation) — the model supports it mechanically, but not a v1 focus.
