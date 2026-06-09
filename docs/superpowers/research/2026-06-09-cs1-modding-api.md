# Cities: Skylines 1 Modding API — Research Findings

**Date:** 2026-06-09
**Purpose:** Pin real CS1 (Unity Mono / .NET 3.5) modding API signatures *before* writing the mod, so the discovery spike only needs to confirm what couldn't be verified offline. Produced by a multi-source, adversarially-verified research pass favoring primary sources (official Paradox modding wiki, and the source of real open-source mods).

**How to read this:** "CONFIRMED" = verified against a primary source (wiki signature or working mod source), cited. "OPEN" = not pinned by research; the discovery spike must confirm it in-game. Do not treat OPEN items as known.

---

## CONFIRMED

### Mod lifecycle & entry point
- Entry point implements `ICities.IUserMod` with `string Name { get; }` and `string Description { get; }`. Optional `OnEnabled()`, `OnDisabled()`, `OnSettingsUI(UIHelperBase)` are invoked by reflection (not on the interface). The game finds the mod by assembly-scanning for an `IUserMod` implementer.
  - Sources: Paradox wiki Modding_API / Modding_Basics; mabako/cities-skylines-new-game-plus `Mod.cs`; boformer gist 45e3e2999d.
- Load lifecycle via `LoadingExtensionBase` (implements `ICities.ILoadingExtension`): `OnCreated(ILoading loading)`, `OnLevelLoaded(LoadMode mode)`, `OnLevelUnloading()`, `OnReleased()`. `OnLevelLoaded` fires when a city finishes loading; `OnLevelUnloading` when unloading begins. `LoadMode` enum: `NewGame`, `LoadGame`, `NewMap`, `LoadMap`, `NewAsset`, `LoadAsset` + scenario/theme variants (and undocumented extras like `NewGameFromScenario`).
  - Sources: Paradox wiki Modding_API; boformer gist 45e3e2999d; bloodypenguin/Skylines-MoreNetworkStuff `LoadingExtension.cs`.
- **Use this for the mod's HTTP-server lifecycle:** start the listener in `OnLevelLoaded` when `mode` is `LoadGame`/`NewGame` (managers exist); stop it in `OnLevelUnloading`.

### Simulation thread & clock control
- `ThreadingExtensionBase` (implements `IThreadingExtension`) provides sim-thread hooks: `OnBeforeSimulationTick()` / `OnAfterSimulationTick()`, `OnBeforeSimulationFrame()` / `OnAfterSimulationFrame()` (60×/sec at 1× speed), and `OnUpdate(realTimeDelta, simulationTimeDelta)` (once per *rendered* frame).
- `IThreading` (reachable from the extension) exposes: `bool simulationPaused { get; set; }`, `int simulationSpeed { get; set; }` (**range 1..3**), `uint simulationFrame { get; }`, `uint simulationTick { get; }` (**updated 60×/sec even while paused**).
  - Sources: Paradox wiki Modding_API; mabako `Mod.cs`.
- **Use this for `/clock`:** `pause`/`resume` ↔ `simulationPaused`; `set-speed` ↔ `simulationSpeed` (clamp 1..3); read tick from `simulationTick`. This replaces the spec's earlier assumption that clock control needed `SimulationManager` poking.
- `SimulationManager.UpdateMode` enum (`NewGame`, `LoadGame`, `Undefined`, …) distinguishes a new game from a loaded save. Source: mabako `Mod.cs`.
- A mod *may* register a custom sim manager via `SimulationManager.RegisterSimulationManager(ISimulationManager)` whose `SimulationStep(int subStep)` runs on the sim thread — but this is from an old "hacky" repo; modern practice prefers `ThreadingExtensionBase`. Prefer the extension.

### Network read / create / release (CONFIRMED — the riskiest structural area, now de-risked)
- **Buffer pattern:** `Singleton<NetManager>.instance.m_segments.m_buffer[segmentId]` (Array16). `NetSegment` fields: `m_infoIndex` (NetInfo prefab index; resolve name via `.Info.name`), `m_flags` (`NetSegment.Flags`: `None`/`Created`/`Untouchable`/…), `m_startNode`, `m_endNode`, `m_startDirection`/`m_endDirection` (Vector3), `m_lanes` (head of a lane linked list traversed via `NetLane.m_nextLane`). `NetNode` likewise via `m_nodes.m_buffer[nodeId]` with a position.
  - Sources: TM:PE wiki "Nodes,-Segments,-Lanes"; cslmodding.info; boformer gist cf7a59a71a; TM:PE `CustomRoadAI.cs`.
- **Create node:** `NetManager.CreateNode(out ushort nodeId, ref Randomizer r, NetInfo info, Vector3 position, uint buildIndex)`.
- **Create segment:** `NetManager.CreateSegment(out ushort segmentId, ref Randomizer r, NetInfo info, ushort startNodeId, ushort endNodeId, Vector3 startDirection, Vector3 endDirection, uint buildIndex, uint buildIndex2, bool invert)` → **returns `bool` success**. For a straight segment: `startDirection = VectorUtils.NormalizeXZ(endPos - startPos)`, `endDirection = -startDirection`; `buildIndex = Singleton<SimulationManager>.instance.m_currentBuildIndex`, and increment `m_currentBuildIndex += 2u` after a successful create.
  - Source: lxteo/Cities-Skylines-Mapper `RoadMaker.cs` (working mod); cross-confirmed in TM:PE.
- **Release segment:** `NetManager.ReleaseSegment(ushort segment, bool keepNodes)` → `void` (no success value observed; check effects instead). Source: boformer gist cf7a59a71a.
- **Changing a segment's road type** (for `upgrade-road`): set `m_segments.m_buffer[id].m_infoIndex = newNetInfoIndex` (and clear `Untouchable` flag) — boformer gist cf7a59a71a shows this pattern. *(Confirm this is sufficient vs needing recreate during the spike.)*

### Prefab / road-type enumeration (CONFIRMED)
- Iterate: `for (uint i=0; i < PrefabCollection<NetInfo>.PrefabCount(); i++) { NetInfo p = PrefabCollection<NetInfo>.GetPrefab(i); … }` (also `GetLoaded(index)`/`LoadedCount()`). Prefab `.name` values contain spaces — e.g. `"Basic Road"`, `"Highway"`; mods commonly match on `name.Replace(" ", "")` against identifiers like `BasicRoad`, `GravelRoad`, `Highway`, `HighwayRamp`.
  - Sources: lxteo `RoadMaker.cs`/`RoadMapping.cs`; boformer gist (same pattern for `BuildingInfo`).
- **Use this for `list_road_types`** (and validating `build_road`'s `road_type`). The exact full list of relevant road prefab names is dumped by the spike's `/probe`.

### Build toolchain & reference assemblies (CONFIRMED)
- Target **`.NET Framework v3.5`**. Reference `ICities.dll`, `ColossalManaged.dll`, `Assembly-CSharp.dll`, `UnityEngine.dll` (optionally `UnityEngine.UI.dll`, `System.Core`) from the game's `Managed/` directory.
  - Source: TM:PE `TLM.csproj` (`<TargetFrameworkVersion>v3.5</TargetFrameworkVersion>`, HintPaths to those DLLs); Paradox wiki Folder_Structure.
- **macOS mod install folder (verbatim confirmed):** `~/Library/Application Support/Colossal Order/Cities_Skylines/Addons/Mods`. Source: Paradox wiki Folder_Structure; citiesskylinesmoddingguide.
- macOS `Managed/` path is the Unity convention `Cities.app/Contents/Resources/Data/Managed/` (high confidence, but the literal Mac path wasn't quoted by a source — spike confirms the exact path on the user's install).

### Harmony (likely NOT needed)
- If patching were needed, the standard is boformer's **CitiesHarmony**: reference only `CitiesHarmony.API.dll`, bootstrap via `HarmonyHelper.DoOnHarmonyReady(...)` in `OnEnabled`. Source: boformer/CitiesHarmony README.
- **For SkylineBench we read state and call public `NetManager`/`SimulationManager` APIs — no game-method patching — so we likely avoid Harmony entirely.** Revisit only if a needed capability requires intercepting a game method.

---

## OPEN — discovery spike must confirm in-game

These were NOT pinned by research and must be treated as unknown until the probe confirms them:

1. **Traffic metrics (highest-stakes).** There is **no single vanilla "traffic flow %" field.** TM:PE *computes* per-lane load itself in `OnBeforeSimulationStep` by traversing the lane chain (`segment.m_lanes` → `NetLane.m_nextLane`), accumulating a `trafficBuffer` count + `accumulatedSpeeds`, normalized to a 0..10000 scale (`REF_REL_SPEED = 10000`). `NetSegment.m_trafficDensity`'s existence/semantics is **unconfirmed**. → The spike must determine: does `NetSegment.m_trafficDensity` exist and what does it mean? If usable, emit it; otherwise the mod computes a TM:PE-style per-segment load and the city flow % is an aggregate the mod computes. Source for TM:PE method: TM:PE `TrafficMeasurementManager.cs`.
2. **Field names for the other managers** — unverified, need decompile/probe: `VehicleManager` active count (`m_vehicleCount`?), `BuildingManager.m_buildings` buffer + `BuildingInfo` name/category (`ItemClass.Service`/`SubService`), `EconomyManager` money/income/expense fields, population + RCI demand (`ZoneManager.m_actualResidentialDemand` etc. or `DistrictManager`).
3. **Zoning** — representation (`ZoneBlock`, the 8m cell grid), how to *read* a cell's zone type, and how to *set* zoning over an area. Zone cell size (assumed 8m) unconfirmed.
4. **`load-save` mid-session** — whether a savegame can be loaded programmatically from a running game (via `LoadingManager`/`SimulationManager`) or only from the main menu, and constraints. Affects the broker's `reset_scenario`.
5. **Map extent & max segment length** — real playable world extent in metres (the ±8,640 m / 9×9×1920 m assumption is unconfirmed) and the game's single-segment length limit (broker's working `MAX_SEGMENT_LENGTH_M = 200.0`).
6. **`HttpListener` viability in CS1's Mono** — no confirmation it works; the one large mod studied (CSM) uses LiteNetLib/UDP for a different purpose. → The spike decides `HttpListener` vs the `TcpListener` fallback, and checks macOS firewall-prompt behavior on first bind.
7. **Actions while paused** — whether work queued for the sim thread runs while `simulationPaused == true`, and if not, the supported way to flush (e.g. briefly toggling `simulationPaused`). Critical for step-mode build-then-step.

---

## Key sources (primary)
- Paradox modding wiki: Modding_API, Modding_Basics, Folder_Structure, CS_Assemblies.
- TM:PE (CitiesSkylinesMods/TMPE): `TLM.csproj`, `TrafficMeasurementManager.cs`, `CustomRoadAI.cs`, Nodes/Segments/Lanes wiki.
- lxteo/Cities-Skylines-Mapper: `RoadMaker.cs`, `RoadMapping.cs` (node/segment creation + prefab enum).
- boformer gists cf7a59a71a (segment infoIndex/release) and 45e3e2999d (IUserMod/building enum); boformer/CitiesHarmony.
- mabako/cities-skylines-new-game-plus `Mod.cs` (lifecycle, RegisterSimulationManager, UpdateMode).
- bloodypenguin/Skylines-MoreNetworkStuff `LoadingExtension.cs`.
- CSM multiplayer `Server.cs` (networking-library choice, indirect).
