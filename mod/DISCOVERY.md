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
- **⚠️ `step` overshoots by ~2 ticks (D1):** `{"op":"step","ticks":256}` advanced `1442553 → 1442811` = **258** ticks, then correctly re-paused. The step polls the tick on the HTTP thread and stops at the first tick **≥** target, so it lands a frame or two past N. For a benchmark measuring over thousands of ticks this ±2 is noise; exact-N stepping would require an `OnBeforeSimulationTick` counter hook (more invasive). **Documented as a known imprecision, not fixed.**

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
- **City-wide traffic flow:** `VehicleManager` exposes `m_lastTrafficFlow : UInt32`, `m_maxTrafficFlow : UInt32`, `m_totalTrafficFlow : UInt32`. **`m_lastTrafficFlow` IS the 0..100 traffic-flow percentage** — confirmed against `Assembly-CSharp` IL: every 256 sim frames the game sets `m_lastTrafficFlow = min(100, m_totalTrafficFlow*100 / m_maxTrafficFlow)` and then resets `m_total`/`m_max` to 0. → `/metrics` `traffic.flow_percent` returns `m_lastTrafficFlow` **directly**.
- **⚠️ CORRECTED (Phase 2):** the original guess `m_lastTrafficFlow * 100 / m_maxTrafficFlow` was WRONG — it divides an already-percentage by the raw accumulator (which is mid-refill and reset every 256 frames), yielding ~0.01% on a busy city. Reading `m_lastTrafficFlow` directly matches the in-game UI. `m_total`/`m_max` are internal accumulators, not for direct use.

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

## Economy — RESOLVED ✅ (runtime-verified D1)
- `EconomyManager.LastCashAmount : Int64` → `/metrics` `economy.funds`. **D1 note:** under the *Unlimited Money* mod this returns `Int64.MaxValue` (`9223372036854775807`) — that is expected, not a bug; agents must treat a maxed `funds` as "money is not a constraint."
- **Income/expenses — now RESOLVED:** `EconomyManager.GetIncomeAndExpenses(Service.None, SubService.None, Level.None, out income, out expenses)` works. The mod computes `weekly_expenses = expenses + GetLoanExpenses() + GetPolicyExpenses()`. **D1 verified:** `weekly_income 1347518`, `weekly_expenses 1172296`.
- **⚠️ `balance` semantics:** `/metrics` `economy.balance` is the **weekly net** (`income − weekly_expenses`, e.g. `175222`), **NOT** cash-on-hand. Cash-on-hand is `funds`. This is a deliberate naming quirk worth keeping in mind — `balance` ≠ bank balance.

## Population & RCI demand — RESOLVED ✅ with a contract mismatch ⚠️
- `ZoneManager.m_actualResidentialDemand : Int32`, `m_actualCommercialDemand : Int32`, `m_actualWorkplaceDemand : Int32` (and non-actual `m_residentialDemand`/`m_commercialDemand`/`m_workplaceDemand`).
- **Contract mismatch:** the broker's `PopulationMetrics` assumed **four** demands (residential/commercial/industrial/office). CS1 exposes **three** (residential/commercial/**workplace**), where workplace = industrial+office combined. → **Spec-sync decision for 2b:** change the contract's `PopulationMetrics` to `residential_demand` / `commercial_demand` / `workplace_demand` (drop the industrial/office split), and update `broker/src/contract.rs` + the mock accordingly. (Total population itself lives elsewhere — `2b` reads it via the standard `CitizenManager`/district population; confirm the accessor in 2b.)
- **Total population — now RESOLVED:** `DistrictManager.m_districts.m_buffer[0].m_populationData.m_finalCount` works (**D1 verified:** `total 3380`). Happiness via `m_districts.m_buffer[0].m_finalHappiness`. **D1 verified** R/C/W demand `87 / 0 / 13`.
- **⚠️ `employed` is hardcoded to 0:** no single clean manager field exposes employment; the mod documents this gap rather than computing it from `CitizenManager`. Agents should ignore `population.employed` for now (Phase-2 candidate to compute properly).

## Network create/release — CONFIRMED (from research, build-verified)
- `int PrefabCollection<NetInfo>.PrefabCount()` (returns **int**, not uint — corrected during the foundation build), `GetPrefab(uint)`.
- `NetManager.CreateNode/CreateSegment(...)→bool`, `ReleaseSegment(ushort, bool keepNodes)`, buffer pattern `m_segments.m_buffer[id]` with `m_infoIndex`→`.Info.name`, `m_startNode`/`m_endNode`, lane chain `m_lanes`→`NetLane.m_nextLane`. (All compiled clean against 1.21.1-f9 assemblies.)

## Mid-session `load-save` — IMPLEMENTED but UNSTABLE ⚠️ (experimental)
- The mod resolves a save by name (`PackageManager.FilterAssets(UserAssetType.SaveGameMetaData)`, matching `asset.name`/cityName/fullName) and triggers a mid-session load.
- **D1 result:** `/load-save` with a real save name returned `{"ok":true,"city_loaded":true}` — i.e. the call path works — **but the game CRASHED shortly after** during the mid-session `LoadLevel`. Mid-session loading is therefore **not reliable**.
- **Decision (Phase 1):** keep the endpoint but mark it **experimental/unstable**. The harness should prefer loading a city from the **main menu**, not via this endpoint. A robust scenario-reset (relaunch-to-known-save, or load-at-startup) is **Phase 2's** concern — that's where the benchmark needs a dependable reset, and it can own the more careful approach.
- **No-city guard (new D1 finding):** when `health.city_loaded == false`, sim-thread jobs (build/bulldoze/metrics/etc.) **time out after 8000 ms** (`"internal: sim-thread job timed out"`) rather than failing fast, because `SimThread.DrainOnSimThread` never runs without a live simulation. **Agents must check `/health` for `city_loaded:true` before issuing actions.** A fast-fail guard when no city is loaded is a reasonable Phase-1.1 polish.

## Constants — partially open ⚠️
- **Map extent / max segment length were NOT measured by this probe** (the probe didn't compute them). Working values stand: playable extent ≈ **±8,640 m** (consistent with a 9×9 grid of 1,920 m tiles → 17,280 m full span) and broker's `MAX_SEGMENT_LENGTH_M = 200` is a conservative guess. → **2b: measure** (e.g. `TerrainManager`/`NetManager` extents, and the game's segment-length limit) or accept the working values and tune from build failures.
- **macOS `Managed/` path confirmed:** `…/Cities.app/Contents/Resources/Data/Managed` with `ICities.dll`, `ColossalManaged.dll`, `Assembly-CSharp.dll`, `UnityEngine.dll`. Mod installs to `~/Library/Application Support/Colossal Order/Cities_Skylines/Addons/Mods/SkylineBench/`. `BuildConfig.applicationVersion` works (returned `"1.21.1-f9"`).

## Screenshot camera — IMPLEMENTED, calibration PROVISIONAL ⚠️
- `POST /screenshot` manipulates `CameraController` fields directly: `m_targetPosition` / `m_currentPosition` (world position), `m_targetSize` / `m_currentSize` (vertical view extent in metres), `m_targetAngle` / `m_currentAngle` (pitch/yaw vector), `m_freeCamera` (bool — hides game UI chrome).
- The broker size constants in `broker/src/service.rs` are **provisional pending in-game calibration**: `OVERVIEW_MIN_SIZE_M = 1200`, `CLOSEUP_SIZE_M = 350`, `OVERVIEW_MARGIN = 1.15`. These have not yet been verified against the actual game view. → **Calibrate**: load the city, call `POST /screenshot` with a range of `size` values, compare the resulting frames to the expected scene extent, and update the constants.

---

## Summary for Plan 2b
**Green (implement directly):** HTTP (HttpListener), clock (IThreading, drains-while-paused), network read/create/release, prefab enumeration, per-segment + city traffic (`m_trafficDensity` + `VehicleManager` flow fields), active vehicles, buildings (`m_buildings` + `BuildingInfo.m_class`), zoning read/write (`ZoneBlock.m_zone1/2`, 8 m cells), funds (`EconomyManager.LastCashAmount`), R/C/W demand.

**Needs a small follow-up methods-probe in 2b (non-blocking):** income/expense accessor; total population accessor; mid-session `load-save` method (or document the constraint); map extent + max segment length (or accept working values).

**Spec-sync (fold back into the broker/contract):** change `PopulationMetrics` demands to residential/commercial/**workplace** (3, not 4); record `MAX_SEGMENT_LENGTH_M`/playable extent once measured; the `int` `PrefabCount()` correction.

---

## Runtime verification (D1) — 2026-06-09, game 1.21.1-f9, live city

Every wire endpoint exercised against the running game via raw HTTP to `127.0.0.1:8787`:

| Endpoint | Result |
|---|---|
| `GET /health` | ✅ exact `Health` shape; `city_loaded` flag reliable |
| `GET /metrics` | ✅ traffic + economy + population + services all populated (flow 6.1%, 240 vehicles, pop 3380, happiness 82) |
| `GET /network` | ✅ 994 nodes / 1005 segments enumerated |
| `GET /road-types` | ✅ road-class prefabs only (Basic/Medium/Large/Oneway/Highway/…) |
| `GET /zones` | ✅ decoded cells, geometry correct (see set-zone) |
| `POST /action/build-road` | ✅ returns `created_nodes`/`created_segments`; snapping honored |
| `POST /action/bulldoze` | ✅ `{"target_type":"segment"\|"node"\|"building","id":N}` → `destroyed:[N]`; malformed body → `INVALID_ARGS` |
| `POST /action/upgrade-road` | ✅ in-place swap: `destroyed:[old]`, `created_segments:[new]` |
| `POST /action/set-zone` | ✅ painted 64×64 m → 80 `residential_low` cells read back at expected coords |
| `POST /clock` (pause/step) | ✅ exact `ClockState` shape; step re-pauses (overshoot +2, documented above) |
| `POST /load-save` | ⚠️ returns `ok:true` then **crashes** mid-session — experimental (see load-save section) |
| `POST /screenshot` | ⚠️ implemented; in-game calibration of camera size constants pending (see Screenshot camera section) |

**Critical-fix confirmed:** `/clock` and `/load-save` return their dedicated `ClockState`/`LoadResult` shapes (not the generic `ActionResult`), so the broker — which has no serde defaults on those — deserializes them correctly. This was the one Critical finding from the final pre-merge review; it round-trips against the real game.
