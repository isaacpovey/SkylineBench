# SkylineBench Mod — Discovery Findings

**Captured:** 2026-06-09, against Cities: Skylines **1.21.1-f9** (macOS, Steam), city loaded & paused.
**Method:** the probe mod (`/probe`) reflecting over the live game managers + enumerating prefabs. Raw dump: `/tmp/skylinebench-probe.json` (19,615 bytes) at capture time.

This resolves the OPEN items from the API reference (`docs/superpowers/research/2026-06-09-cs1-modding-api.md`) so Plan 2b can implement the contract endpoints with real field names. Where a value is marked **still open**, 2b must probe methods (the probe dumped fields/properties only, not methods) or accept the noted constraint.

---

## HTTP transport — RESOLVED ✅
- **`HttpListener` works in CS1's Mono.** Both `GET /health` and `GET /probe` responded over `http://127.0.0.1:8787` with no TcpListener fallback needed. No macOS firewall prompt blocked the localhost bind. → **Plan 2b keeps the `HttpListener` implementation; the `TcpListener` fallback is not required.**
- `/health` returned: `{"mod_version":"0.1.0","game_version":"1.21.1-f9","city_loaded":true,"paused":true,"tick":6030222}` — exact contract `Health` shape, working end to end.

## Sim thread & clock — RESOLVED ✅
- `IThreading.simulationPaused` / `simulationTick` / `simulationSpeed` all work (clock section showed `paused:true, tick:6030224, speed:1`).
- **`simulationTick` advances even while paused** (6030222 → 6030224 across two calls, both while paused) — consistent with the "60×/sec even when paused" note.
- **`ThreadingExtensionBase.OnBeforeSimulationTick` fires even while paused** — proven because `ModRuntime.Threading` (set only inside `OnBeforeSimulationTick`) was non-null and the clock values were live during a paused probe. Since `SimThread.DrainOnSimThread()` is called from that same hook, **queued sim-thread jobs DO drain while paused.**
  → **Plan 2b does NOT need the unpause-to-flush workaround.** `SimThread.Run` works as-is in step mode (build while paused, then `step`).

## Road prefab identifiers (for `list_road_types` / `build_road`) — RESOLVED ✅
331 `NetInfo` prefabs enumerated. The relevant *buildable surface road* names (note: names contain spaces; match exactly or strip spaces):
- Basic: `"Basic Road"`, `"Basic Road Elevated"`, `"Basic Road Bridge"`, `"Basic Road Tunnel"`, `"Basic Road Slope"`, `"Gravel Road"`
- Medium: `"Medium Road"`, `"Small 4 Lane Road"`, (+ Elevated/Bridge/Tunnel/Slope variants)
- Large: `"Large Road"`, `"Large Road with Median"`, `"Large Oneway"`
- Oneway: `"Oneway Road"`, `"Small 3 Lane 1 Way Road"`, `"Small 4 Lane 1 Way Road"`
- Highway: `"Highway"`, `"HighwayRamp"`, `"Two Lane Highway"`, `"Two Lane Highway Twoway"`, `"Four Lane Highway"`
- (Full list incl. rail/metro/pedestrian/canal/decoration variants is in the raw dump; Plan 2b/`list_road_types` should filter to road `ItemClass` and exclude rail/metro/pedestrian/canal/power/pipe/decoration-only variants.)
- **2b note:** `build_road`'s default road types likely map to `"Basic Road"`, `"Medium Road"`, `"Large Road"`, `"Oneway Road"`, `"Highway"`. The mod should expose the real names; the broker already calls `list_road_types` so the agent uses valid values.

## Per-segment & city traffic — RESOLVED ✅ (better than expected)
The API reference flagged "no single vanilla traffic flow field." The probe found usable fields:
- **Per-segment load:** `NetSegment.m_trafficDensity` **exists** as a `Byte` (0–255). → `/metrics` `segment_loads[].density` reads this directly (normalize /255). No lane-chain computation needed.
- **City-wide traffic flow:** `VehicleManager` exposes `m_lastTrafficFlow : UInt32`, `m_maxTrafficFlow : UInt32`, `m_totalTrafficFlow : UInt32`. The UI "traffic flow %" ≈ `m_lastTrafficFlow * 100 / m_maxTrafficFlow` (guard divide-by-zero → 100 when `m_maxTrafficFlow == 0`). → `/metrics` `traffic.flow_percent` computes from these.
- **2b note:** confirm the exact flow-% formula against the in-game UI value during 2b's manual verify; the fields are present and the ratio is the standard derivation.

## Active vehicle count — RESOLVED ✅
- `VehicleManager.cityVehicleCount` (property, `UInt32`) — the moving-vehicle count; or `m_vehicleCount : Int32`. → `/metrics` `traffic.active_vehicles` uses `cityVehicleCount`.

## Buildings — RESOLVED ✅
- `BuildingManager.m_buildings : Array16<Building>` (buffer), `m_buildingCount : Int32`, `m_buildingGrid : UInt16[]` (spatial grid). Iterate the buffer like the net buffers.
- Category: from `building.Info` (`BuildingInfo`) → `m_class` (`ItemClass`) `m_service` / `m_subService` (and `m_zone`). → map `ItemClass.Service` Residential/Commercial/Industrial/Office/(others)→"service". *(BuildingInfo.m_class confirmed by community API; the `m_buildings` buffer + grid are confirmed here.)*

## Zoning — RESOLVED ✅ (read + write path identified)
- `ZoneManager.m_blocks : Array16<ZoneBlock>` (+ `m_blockCount`), `m_zoneGrid : UInt16[]` (the cell→block grid).
- `ZoneBlock` fields: `m_position : Vector3`, `m_angle : Single`, `m_zone1 : UInt64`, `m_zone2 : UInt64` (cell zone types packed as nibbles — a block is 4 columns × 8 rows of 8 m cells), `m_valid : UInt64`, `m_occupied1/2 : UInt64`, `m_segment : UInt16`, `RowCount` (prop). Zone cell size = **8 m** (standard CS1 ZoneBlock granularity).
- **Read** `/zones`: decode `m_zone1`/`m_zone2` nibbles per valid cell of each block → `ItemClass.Zone` enum value.
- **Write** `/action/set-zone`: set the cell nibbles in `m_zone1`/`m_zone2` and mark the block updated. **2b note:** this is the fiddliest write — implement carefully against `ZoneBlock` semantics; verify in-game.

## Economy — RESOLVED ✅ (funds) / partial (income/expenses)
- `EconomyManager.LastCashAmount : Int64` (displayed cash) and `InternalCashAmount : Int64` (internal, money×100). → `/metrics` `economy.funds` = `LastCashAmount` (or `InternalCashAmount/100`).
- **Income/expenses — still open:** not exposed as a simple field. CS1 provides `EconomyManager.GetIncomeAndExpenses(...)` (a method; the probe dumped fields/properties only). → **2b must probe methods** for the weekly income/expense accessor, or derive from `m_finalCashCollecting`/`m_finalMonthlyEarned`. Non-blocking: funds + balance are available now.

## Population & RCI demand — RESOLVED ✅ with a contract mismatch ⚠️
- `ZoneManager.m_actualResidentialDemand : Int32`, `m_actualCommercialDemand : Int32`, `m_actualWorkplaceDemand : Int32` (and non-actual `m_residentialDemand`/`m_commercialDemand`/`m_workplaceDemand`).
- **Contract mismatch:** the broker's `PopulationMetrics` assumed **four** demands (residential/commercial/industrial/office). CS1 exposes **three** (residential/commercial/**workplace**), where workplace = industrial+office combined. → **Spec-sync decision for 2b:** change the contract's `PopulationMetrics` to `residential_demand` / `commercial_demand` / `workplace_demand` (drop the industrial/office split), and update `broker/src/contract.rs` + the mock accordingly. (Total population itself lives elsewhere — `2b` reads it via the standard `CitizenManager`/district population; confirm the accessor in 2b.)
- **2b note:** total population field/accessor wasn't in this probe's manager set — add `DistrictManager`/`CitizenManager` to a follow-up probe in 2b, or read `DistrictManager.m_districts.m_buffer[0].m_populationData` (community-known) and confirm.

## Network create/release — CONFIRMED (from research, build-verified)
- `int PrefabCollection<NetInfo>.PrefabCount()` (returns **int**, not uint — corrected during the foundation build), `GetPrefab(uint)`.
- `NetManager.CreateNode/CreateSegment(...)→bool`, `ReleaseSegment(ushort, bool keepNodes)`, buffer pattern `m_segments.m_buffer[id]` with `m_infoIndex`→`.Info.name`, `m_startNode`/`m_endNode`, lane chain `m_lanes`→`NetLane.m_nextLane`. (All compiled clean against 1.21.1-f9 assemblies.)

## Mid-session `load-save` — STILL OPEN ⚠️
- `LoadingManager` fields are all loading-*state* flags (`m_currentlyLoading`, `m_loadingComplete`, `m_loadedEnvironment`, …) — no load-a-save *method* appears in the field/property dump (methods weren't dumped).
- CS1 loads saves via a metadata + `LoadingManager`/`SimulationManager` flow (community approach: build `SaveGameMetaData` + call the load), typically only cleanly from the main menu. → **2b: probe `LoadingManager`/`SimulationManager` methods**; if mid-session load isn't cleanly supported, **document the constraint** for the broker's `reset_scenario` (e.g. it may require returning to a known autosave, or be a no-op that the harness handles by relaunching) rather than faking it.

## Constants — partially open ⚠️
- **Map extent / max segment length were NOT measured by this probe** (the probe didn't compute them). Working values stand: playable extent ≈ **±8,640 m** (consistent with a 9×9 grid of 1,920 m tiles → 17,280 m full span) and broker's `MAX_SEGMENT_LENGTH_M = 200` is a conservative guess. → **2b: measure** (e.g. `TerrainManager`/`NetManager` extents, and the game's segment-length limit) or accept the working values and tune from build failures.
- **macOS `Managed/` path confirmed:** `…/Cities.app/Contents/Resources/Data/Managed` with `ICities.dll`, `ColossalManaged.dll`, `Assembly-CSharp.dll`, `UnityEngine.dll`. Mod installs to `~/Library/Application Support/Colossal Order/Cities_Skylines/Addons/Mods/SkylineBench/`. `BuildConfig.applicationVersion` works (returned `"1.21.1-f9"`).

---

## Summary for Plan 2b
**Green (implement directly):** HTTP (HttpListener), clock (IThreading, drains-while-paused), network read/create/release, prefab enumeration, per-segment + city traffic (`m_trafficDensity` + `VehicleManager` flow fields), active vehicles, buildings (`m_buildings` + `BuildingInfo.m_class`), zoning read/write (`ZoneBlock.m_zone1/2`, 8 m cells), funds (`EconomyManager.LastCashAmount`), R/C/W demand.

**Needs a small follow-up methods-probe in 2b (non-blocking):** income/expense accessor; total population accessor; mid-session `load-save` method (or document the constraint); map extent + max segment length (or accept working values).

**Spec-sync (fold back into the broker/contract):** change `PopulationMetrics` demands to residential/commercial/**workplace** (3, not 4); record `MAX_SEGMENT_LENGTH_M`/playable extent once measured; the `int` `PrefabCount()` correction.
