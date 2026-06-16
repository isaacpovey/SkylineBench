# MCP feedback & contract fixes — design

**Date:** 2026-06-16
**Scope:** the AI↔game interaction surface only — the MCP tools (`broker/src/tools.rs`),
the service layer (`broker/src/service.rs`), the wire contract (`broker/src/contract.rs`),
the bridge client (`broker/src/bridge_client.rs`), and the mod's HTTP/game layers
(`mod/src/http/*`, `mod/src/bridge/*`, `mod/src/dto/Dtos.cs`, `mod/src/json/Serialize.cs`).
The website and benchmark-running code are out of scope.

## Problem statement

A review of the agent-facing surface found that the agent can act on the city but is
partly blind to *why* an action failed and *where* a problem is, and that several
contract fields are advertised to the model but never populated or are misleading.

Confirmed findings:

1. `colliding_buildings` is declared (`Dtos.cs:46`, `contract.rs:248`) and serialized
   (`Serialize.cs:90`) but **never populated** — every `OBJECT_COLLISION` rejection is
   positionally blind.
2. Per-building problem flags are read but collapsed to city-wide counts
   (`GameReads.cs:135-156`); the agent cannot locate *which* building lost road access /
   a utility — the documented death-spiral precursor.
3. `employed` is hardcoded to `0` (`GameReads.cs:131`) yet exposed as a real field.
4. Zone-type vocabulary mismatch: reads emit `residential_low`/`commercial_low`
   (`GameReads.cs:205,207`) but `list_zone_types` advertises `residential`/`commercial`
   (`Handlers.cs:55`) and `set_zoning` validates against that list (`service.rs:341`), so
   the string the agent reads back is rejected when written.
5. `ActionError` advertises `INSUFFICIENT_FUNDS` and plain `COLLISION`, which the current
   build path (`needMoney:false`, emits `OBJECT_COLLISION`) can never produce.
6. `/action/validate-road` is fully implemented and wired in the bridge client but has no
   MCP tool — the agent has no build dry-run.
7. `bulldoze` node/building branches never check existence and return a phantom
   `ok:true` (`GameActions.cs:46-50`).
8. `RoadErrors` folds native `TooShort` and `InvalidShape` into `INVALID_ARGS`
   (`RoadErrors.cs:18-19`), losing actionable fidelity.

## Decisions (resolved with the user)

- **Per-building problems:** dedicated `query_problems` MCP tool (keep `observe_area` lean).
- **Economy:** money stays off by design → prune the dead error codes; do not re-enable.
- **`employed`:** drop from the contract.
- **Zone naming:** canonicalise on the `_low` suffix everywhere.
- **`colliding_buildings`:** ids only (positions resolve via `observe_area`).

## Work items

### A. Populate `colliding_buildings` on collision — DEFERRED

**Deferred to a follow-up** (tracked in
`docs/superpowers/research/2026-06-16-collision-locality-investigation.md`).

A flat 2-D corridor-overlap was considered and **rejected**: CS1 roads are 3-D, so an
elevated span can clear a building (a 2-D test would false-positive) while its bridge
**pillars** touch down at ground points the centreline never approaches (a 2-D test would
miss them). An engine-accurate id set requires disassembling how `NetTool.CreateNode`
flags `ObjectCollision` (clearance comparison + pillar footprints) — see the investigation
doc for the research checklist and candidate strategies. The wire contract already carries
`colliding_buildings` (`contract.rs:248`, `Serialize.cs:90`), so the follow-up is a
mod-side computation only and does not block items B–H.

### B. New `query_problems` MCP tool

**Mod side:**
- New `GameReads.Problems()` read that walks `BuildingManager.m_buildings`, and for each
  created building with any `m_problems.m_Problems1` flag set, emits
  `{ id, x, z, category, problems: [..] }` where `problems` is the list of normalised
  problem names (`road_not_connected`, `no_electricity`, `no_water`, `no_sewage`,
  `garbage_piling`, `no_fuel`, `abandoned`) — the same vocabulary already used for the
  `/metrics` counts, factored into a shared mapping so the two cannot drift.
- New DTO `ProblemBuildingDto` + `ProblemsDto`, serializer `Serialize.Problems`.
- New `GET /problems` route + handler.

**Broker side:**
- `BridgeClient.problems()` GET helper; `Problems`/`ProblemBuilding` contract types.
- `service::query_problems(client, args)` with optional `filter: Option<String>`
  (a single problem name) and `bounds: Option<Bounds>`, applied broker-side.
- `query_problems` MCP tool in `tools.rs`, described as: locate the specific buildings
  behind a problem-count spike (the leading death-spiral signal), so a severed building
  can be reconnected before it abandons.

The aggregate counts in `/metrics` are unchanged (they remain the cheap leading
indicator); `query_problems` answers "which / where".

### C. Drop `employed`

Remove `employed` from:
- `PopulationMetrics` (`contract.rs:132`) and its round-trip test (`metrics_round_trips`).
- `MetricsDto` (`Dtos.cs`), `GameReads.Metrics` (the `dto.Employed = 0;` line and comment),
  and `Serialize.Metrics` (`Serialize.cs:62`).

### D. Zone vocabulary → `_low` everywhere

- `Handlers.ZoneTypes` emits: `residential_low`, `residential_high`, `commercial_low`,
  `commercial_high`, `industrial`, `office`.
- `GameActions.ParseZone` already accepts the `_low` forms — no change needed there
  (the legacy non-`_low` aliases may stay as accepted input for tolerance, but are no
  longer advertised).
- `set_zoning` (`service.rs:339`) validates against the list, which now contains the
  same strings `observe_area` emits, so the read→write round-trip succeeds.
- No broker logic change beyond the list now matching.

### E. Prune dead error codes

- Remove `Collision` and `InsufficientFunds` from `ActionError` (`contract.rs:210-226`)
  and their `SCREAMING_SNAKE` serialization assertions.
- Scrub tool descriptions / comments that imply builds cost money or that plain
  `COLLISION` can occur. (`OBJECT_COLLISION` remains the real collision code.)
- Confirm no code path still references the removed variants (the mod emits
  `OBJECT_COLLISION` via `RoadErrors`, never plain `COLLISION`; `needMoney:false`
  means `INSUFFICIENT_FUNDS` is unreachable).

### F. Expose `validate_road` as an MCP tool

- `service::validate_road(client, args)` mirroring `build_road`'s args
  (`from`, `to`, `road_type`, `snap`, `from_elevation`, `to_elevation`), running the
  broker-side `validate_build_road` pre-check first, then calling the existing
  `bridge_client.validate_road_elevated`.
- `validate_road` MCP tool in `tools.rs`, described as a free dry-run: test placement
  (collisions, slope, water, height, bounds) without committing or creating ids.
- Returns the `ActionResult` shape; on success no segment is created so the diff arrays
  are empty.

### G. Bulldoze existence check

In `GameActions.Bulldoze` (`GameActions.cs:24-52`):
- Bounds-check `req.Id` against the relevant buffer length before the `(ushort)` cast.
- For each branch, verify the target carries its `Created` flag
  (`NetSegment.Flags.Created` / `NetNode.Flags.Created` / `Building.Flags.Created`);
  if not, return `ActionResultDto.Fail(ErrorCode.InvalidArgs)` rather than a phantom
  `ok:true` with `destroyed:[id]`.
- The segment branch already reads `Created` for the fronting count; reuse that guard to
  gate the release itself.

### H. Error-code fidelity for short / malformed segments

- Add `TOO_SHORT` and `INVALID_SHAPE` to `ErrorCode` (mod) and `ActionError`
  (`contract.rs`, with `SCREAMING_SNAKE` serialization).
- `RoadErrors.Reason` maps native `TooShort` (`0x100`) → `TOO_SHORT` and `InvalidShape`
  (`0x80`) → `INVALID_SHAPE` instead of both → `INVALID_ARGS` (`RoadErrors.cs:18-19`).
- Update `RoadErrorsTests` accordingly.

## Testing

- **Broker (Rust unit tests + mock):** items C, E, F, and the `query_problems` service
  filtering are covered. Extend `broker/src/mock.rs` with a `/problems` endpoint and a
  `/action/validate-road` response so `service.rs` tests can exercise shape, filtering,
  and the validate tool end-to-end against the mock. Update `contract.rs` round-trip
  tests for the dropped `employed` field and removed error variants, and
  `tools.rs::registers_all_tools` for the two new tools.
- **Mod (C# TestRunner):** items D (ParseZone / zone-list vocabulary), G (bulldoze
  existence — as far as the pure logic is unit-testable), and H (`RoadErrorsTests`
  bit→code mapping) are covered by the existing `mod/test` runner.
- **Live-game only (documented limits):** the `query_problems` read (B) depends on a
  loaded city and real building problem flags; it can be shape-tested against the mock
  but its correctness needs a live run. (Item A's engine-accurate collision is deferred —
  see the investigation doc.)

## Out of scope

- Re-enabling the economy / build costs.
- Computing a real `employed` figure.
- Surfacing collision building positions inline (agent resolves via `observe_area`).
- Any website or benchmark-runner change.
