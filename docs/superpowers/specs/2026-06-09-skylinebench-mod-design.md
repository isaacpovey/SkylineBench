# SkylineBench — C# Mod Design (Phase 1, mod half)

**Date:** 2026-06-09
**Status:** Approved for implementation planning
**Scope:** The Cities: Skylines 1 in-game C# mod (the "bridge") that implements the HTTP contract the Rust broker already speaks to — plus a minimal macOS build/install path and manual end-to-end verification. This is the second half of Phase 1; the broker half is complete and merged.

Builds on: `docs/superpowers/specs/2026-06-08-skylinebench-phase1-design.md` (the authoritative Phase-1 spec; §3 defines the HTTP contract, §5 the error set, §6 the metrics, §8 setup, §9 testing). The broker's `contract.rs` is the concrete, frozen source of truth for the wire format.

**Research basis:** the concrete CS1 API signatures this spec relies on were pinned ahead of implementation by a multi-source, adversarially-verified research pass, recorded in `docs/superpowers/research/2026-06-09-cs1-modding-api.md` (the "API reference"). That document is the source of truth for what is **CONFIRMED** (cited signatures) vs **OPEN** (must be confirmed by the in-game discovery spike). This spec defers to it: where it says CONFIRMED, the implementation uses the cited call; where OPEN, the spike resolves it first. The research narrowed the spike substantially — the riskiest structural area (network read/create/release), the lifecycle hooks, clock control, prefab enumeration, and the build/install path are all confirmed; the spike now focuses on the OPEN list below.

---

## 1. Goal & constraints

Implement the in-game mod so that `skylinebench serve --mod-url http://127.0.0.1:8787` — currently pointed at the broker's mock — can be pointed at the **real running game** with no broker changes, and an AI agent can observe the city, control time, and build/modify the road network end to end.

**Hard constraints:**
- **Runtime:** CS1 runs Unity's old Mono (.NET Framework 3.5). The mod targets **`net35`** and may use only what that BCL + the game assemblies provide — no modern HTTP or JSON libraries.
- **Contract is frozen.** The mod must implement exactly the §3 endpoints with the JSON shapes the broker's `contract.rs` parses. The contract is not open for redesign here.
- **Thin mod (Phase-1 Approach A).** All graph assembly, validation, and rendering live in the broker. The mod only reports raw state and executes accepted actions.
- **macOS-first.** The user owns CS1 on a Mac and is new to modding; the build/install path targets macOS, with Windows/Linux paths documented for later.
- **Verification requires the user.** The mod can only be validated by running CS1 locally; execution is staged and user-in-the-loop.

---

## 2. Architecture & project layout

A single `net35` class-library DLL loaded by CS1, organized into three internal layers:

1. **HTTP/dispatch** — listener, routing, JSON in/out. Knows nothing about CS1 managers directly.
2. **`GameBridge`** — the *only* code touching CS1 managers. Every mutation is marshalled onto the sim thread through one centralized helper. API surprises from discovery are confined here.
3. **Pure helpers** (`json/`, parsing) — zero game dependency; the only unit-testable surface.

```
mod/
├── SkylineBenchMod.csproj      # net35; HintPaths into the game's Managed/ dir
├── build.sh                    # macOS: detect game, compile (Mono msbuild), install DLL
├── README.md                   # build + install (Mac) + Content Manager enable + verify checklist
├── DISCOVERY.md                # spike output: confirmed API, HTTP decision, prefab lists, constants
├── src/
│   ├── Mod.cs                  # IUserMod + lifecycle (start/stop the HTTP server on city load/unload)
│   ├── http/
│   │   ├── HttpServer.cs        # HttpListener (or TcpListener fallback) + accept loop
│   │   ├── Router.cs            # method+path → handler dispatch (404 unknown route, 405 bad method)
│   │   └── Handlers.cs          # one thin handler per endpoint: parse req → GameBridge → JSON
│   ├── bridge/
│   │   ├── GameBridge.cs         # ONLY code touching CS1 managers
│   │   └── SimThread.cs          # marshal a closure onto the sim thread, block for the result
│   ├── json/
│   │   ├── JsonWriter.cs         # minimal serializer (pure, testable)
│   │   └── JsonReader.cs         # minimal parser for request bodies (pure, testable)
│   └── probe/
│       └── Probe.cs              # discovery dump; stays as a /probe debug endpoint
└── test/                        # pure test project: references only json/ + parsing (no game assemblies)
```

**Layer dependencies:** `http/` → `json/` + `bridge/`; `bridge/` → CS1 managers + `SimThread`; `json/` and parsing → nothing. `Mod.cs` owns lifecycle. `Probe.cs` is the discovery output, kept as a cheap debug aid.

**The broker is unchanged.** Pointing `serve --mod-url` at this mod instead of the mock is the only switch.

---

## 3. The discovery spike (first deliverable)

The spike is built first and is the foundation the real mod keeps. It proves load + HTTP + manager access simultaneously and resolves every flagged unknown against the actual game before endpoints are implemented.

Because the research pass already confirmed the structural API (lifecycle, sim-thread hooks, clock control, network read/create/release, prefab enumeration, build/install), the spike is now **narrow** — it confirms only the **OPEN** items from the API reference. The probe mod still builds the full `http/` scaffolding + `SimThread` + lifecycle (using the CONFIRMED hooks), so it doubles as the foundation.

**What it is:** a minimal mod with the `http/` scaffolding, `SimThread`, lifecycle, and one endpoint — `GET /probe` — that returns (and also writes to the mod log file, as a fallback if HTTP isn't working yet) a structured dump resolving each OPEN item:

- **HTTP viability:** whether `HttpListener` works in CS1's Mono, or we use the `TcpListener` fallback (incl. macOS firewall-prompt behavior on first bind). **Decision made here.**
- **Traffic source:** does `NetSegment.m_trafficDensity` exist and what does it mean? If usable, emit it; otherwise the mod computes a TM:PE-style per-segment load from the lane chain (`m_lanes`→`NetLane.m_nextLane`) and a city aggregate. (No vanilla "flow %" field exists — confirmed by research.)
- **Manager field names** not pinned by research: `VehicleManager` active count, `BuildingManager.m_buildings` + `BuildingInfo` name/category (`ItemClass.Service`/`SubService`), `EconomyManager` money/income/expense, population + RCI demand fields.
- **Zoning:** how zones are represented (`ZoneBlock`, the cell grid), how to read a cell's type, how to **set** zoning over an area, and the cell size (assumed 8 m).
- **`load-save` mid-session:** whether a save can be loaded from a running game via the API, or only from the menu — and constraints for `reset_scenario`.
- **Constants:** real **playable extent** (vs the ±8,640 m assumption), real **single-segment length limit**, the exact macOS `Managed/` path + assembly names on the user's install.
- **Paused-action behavior:** whether work queued for the sim thread runs while `simulationPaused == true`; if not, confirm the flush approach (briefly toggling `simulationPaused`) — critical for step-mode build-then-step (§4).
- **Prefab/zone lists:** dump the real road-type and zone-type identifier strings (`PrefabCollection<NetInfo>` enumeration is confirmed; the spike captures the actual names).
- **Sample reads:** a few live nodes/segments/buildings + a metrics snapshot, to see real value ranges and shapes.

**Output:** `mod/DISCOVERY.md` records the resolved OPEN items (HTTP decision, traffic source, field names, zoning API, load-save constraints, constants, prefab/zone lists). Every subsequent endpoint references the API reference (for CONFIRMED calls) and `DISCOVERY.md` (for the resolved OPEN ones) instead of guessing. The broker's deferred items (playable extent, segment-length threshold, per-segment flow-vs-density, the two extra error codes) are resolved here with real values and **synced back into the Phase-1 spec**.

**User loop:** documented exact steps to build the spike DLL, drop it in the mods folder, enable it in Content Manager, load a city, and either hit `/probe` or paste the log; we iterate from that output.

---

## 4. Threading & sim-thread marshalling

CS1 runs its simulation on a dedicated thread; the HTTP listener runs on its own thread(s). Touching simulation state off the sim thread corrupts/crashes the game.

**The rule:** all reads and writes of simulation state happen **on the sim thread**; the HTTP handler thread blocks until the result is ready.

**`SimThread.Run(closure)` — the single centralized primitive:**
- Enqueues work onto the sim thread via `SimulationManager.AddAction`.
- The HTTP handler thread waits on a `ManualResetEvent` (or equivalent) for a return value or a captured exception.
- A **timeout** (a few seconds) prevents a wedged/paused sim from hanging an HTTP request forever; on timeout the handler returns a structured timeout/`UNKNOWN` error.
- Exceptions inside the closure are caught and surfaced as structured errors (§6), never raw.

**Reads vs writes:**
- **Mutations** (build/bulldoze/upgrade/set-zone, clock ops) always go through `SimThread.Run`.
- **Reads:** the spike confirms which manager reads are safe to snapshot directly vs which must run on the sim thread. `/metrics` (and likely `/network`) run inside one `SimThread.Run` closure that copies out plain data before returning, guaranteeing a **single consistent snapshot at one tick** (no torn numbers), as §6 requires.

**Clock control (CONFIRMED API):** `/clock` ops use `IThreading` (reachable from a `ThreadingExtensionBase`): `pause`/`resume` ↔ `simulationPaused`, `set-speed` ↔ `simulationSpeed` (clamped to **1..3**), and the current tick is read from `simulationTick` (valid even while paused). The mod registers a `ThreadingExtensionBase` for this access and as the sim-thread execution context.

**Paused-action interaction (OPEN):** in step mode the sim is paused between turns. The spike verifies whether `SimulationManager.AddAction` work (or the threading-extension frame hook) drains while `simulationPaused == true`; if it does not, `SimThread.Run` briefly toggles `simulationPaused` to flush the queued mutation, then restores the paused state. The whole step-mode build-then-step loop depends on this, so it's an explicit spike check.

**Lifecycle (CONFIRMED API):** `Mod.cs`/`LoadingExtensionBase` starts the HTTP server in `OnLevelLoaded(LoadMode.LoadGame|NewGame)` (managers exist) and stops it in `OnLevelUnloading`.

---

## 5. Endpoint implementation (contract → GameBridge)

Each endpoint is one thin handler that parses the request and calls one `GameBridge` method returning plain data. The mapping below is the *intended* CS1 source; every entry marked "spike confirms" is verified against `DISCOVERY.md` before that endpoint is implemented. Any endpoint whose real API differs materially is re-scoped at that point rather than guessed here.

**Reads:**
- `GET /health` → mod/game version, `simulationTick` + `simulationPaused`/`simulationSpeed` (CONFIRMED via `IThreading`), city-loaded flag. (Low risk.)
- `GET /network` → iterate `NetManager.m_segments.m_buffer`/`m_nodes.m_buffer` (CONFIRMED Array16 pattern) → id, `NetNode` position, `m_startNode`/`m_endNode`, prefab name via `m_infoIndex`→`.Info.name`, length, lane count (traverse the `m_lanes`→`NetLane.m_nextLane` chain); optional `bounds`/`types` filter in `GameBridge`. *(All reads CONFIRMED by research; the spike just sanity-checks value ranges.)*
- `GET /buildings` → `BuildingManager` buildings → id, prefab name, category, position, footprint, level. *(OPEN: exact `m_buildings` access + `BuildingInfo` name/category derivation via `ItemClass.Service`/`SubService` confirmed in the spike.)*
- `GET /zones` → `ZoneManager` grid cells → cell coords + zone type. *(OPEN: cell representation + cell size confirmed in the spike.)*
- `GET /metrics` → one sim-thread snapshot: traffic load, active vehicle count, per-segment load, economy, population, happiness headline. **There is no vanilla "traffic flow %" field (CONFIRMED by research)** — the mod computes a TM:PE-style per-segment load from the lane chain and a city aggregate, unless the spike finds `NetSegment.m_trafficDensity` usable. *(OPEN: traffic source + `VehicleManager`/`EconomyManager`/population field names resolved in the spike.)*
- `GET /road-types`, `GET /zone-types` → enumerate via `PrefabCollection<NetInfo>.GetPrefab`/`PrefabCount` (CONFIRMED); the actual identifier strings are dumped by the spike into `DISCOVERY.md`.

**Mutations (all via `SimThread.Run`):**
- `POST /action/build-road` → resolve/create start & end nodes (honoring `snap_to_existing_nodes`), create a straight segment. **CONFIRMED:** `NetManager.CreateNode(out id, ref Randomizer, NetInfo, Vector3, buildIndex)` then `CreateSegment(...) → bool` with `startDir = NormalizeXZ(end-start)`, `endDir = -startDir`, `buildIndex = SimulationManager.instance.m_currentBuildIndex` (`+= 2` on success). Return created/snapped ids; snapping logic is the mod's own (find/reuse a nearby node).
- `POST /action/bulldoze` → `NetManager.ReleaseSegment(id, keepNodes)` (CONFIRMED, returns void → verify by effect), node/building release. *(ReleaseNode signature is medium-confidence; spike confirms node/building release.)*
- `POST /action/upgrade-road` → set `m_segments.m_buffer[id].m_infoIndex` to the new prefab index, preserving endpoints (CONFIRMED pattern from boformer; spike confirms it's sufficient vs requiring recreate).
- `POST /action/set-zone` → write zone type over the rect's cells. *(OPEN: zoning write API confirmed in the spike.)*
- `POST /clock` → `pause`/`resume` ↔ `simulationPaused`, `set-speed` ↔ `simulationSpeed` (1..3), `step` advances N ticks then re-pauses (CONFIRMED clock API via `IThreading`; paused-action flush per §4).
- `POST /load-save` → load a named savegame (the reset primitive). *(OPEN: spike confirms whether mid-session loading is possible via the API; if constrained, the limitation is documented for the broker's `reset_scenario` rather than faked.)*

**Diff results:** every mutation returns the contract's `{ ok, created_nodes, created_segments, snapped_nodes, destroyed, reason }` — the same `ActionResult` the broker parses.

---

## 6. Error mapping (never leak a raw exception)

Every endpoint returns structured JSON, never a raw exception or a stack-trace 500. Two channels, matching what the broker expects:

1. **Action-level failures** (game refused a valid-looking request) → HTTP **200** with `{ ok: false, reason: "<CODE>" }`, `<CODE>` from the §5 set the broker recognizes: `COLLISION`, `INSUFFICIENT_FUNDS`, `OUT_OF_BOUNDS`, `INVALID_PREFAB`, `SEGMENT_TOO_LONG`, `UNKNOWN`. **Action failures are HTTP 200, not 4xx/5xx** — the broker's `bridge_client` uses `error_for_status()` only for transport failures, so a non-2xx would break the normalized-error path. (This is the implicit contract point the broker's final review flagged.)
2. **Transport/protocol failures** (malformed request → 400; unknown route → 404; genuine mod bug → 500). Rare; indicate a bug, not a game refusal.

**Mapping:** CS1 build/bulldoze APIs return a status enum or boolean+reason; `GameBridge` translates to the closest contract code. **The exact game-outcome→code table is filled in during the spike.** Anything unrecognized or an exception caught in `SimThread.Run` → `UNKNOWN`, with the real message logged to the mod log (never in the HTTP `reason`, which stays a broker-matchable enum).

**Broker-side codes** (`DEGENERATE_SEGMENT`, `INVALID_ARGS`) are pre-validation reasons the broker emits before calling the mod; the mod need not produce them but must behave sanely if it receives such a request. The spike's findings finalize the full code set, synced back into spec §5.

---

## 7. JSON & HTTP

Both minimal, since the contract is small and fixed.

**JSON (`json/`, pure, unit-tested):**
- `JsonWriter` — a tiny serializer producing exactly the contract's snake_case shapes (`start_node`, `flow_percent`, …). Explicit per-type writers (no reflection) keep the wire format unambiguous and matched to `contract.rs`. Empty arrays/optional fields may be omitted (the broker uses `#[serde(default)]`), but are emitted explicitly for clarity.
- `JsonReader` — a minimal parser for the small, known request bodies (objects, arrays, strings, numbers, bools, null).
- Round-trip tests per request/response type assert the exact wire shape, using fixtures shared against the broker's mock output (both sides must agree).

**HTTP (`http/`):**
- `HttpServer` — `HttpListener` on `127.0.0.1:<port>` (default **8787**, matching the broker's default `--mod-url`), or the `TcpListener` + minimal HTTP/1.1 fallback if the spike shows `HttpListener` is unavailable in CS1's Mono. The surface above it is identical either way.
- `Router` — exact method+path match; unknown route → 404, wrong method → 405.
- Handlers: read body → `JsonReader` → `GameBridge` → `JsonWriter` → response. Query-string parsing (`bounds`/`types`) is a small pure, testable helper.
- **Port configurability:** the listen port comes from a simple config (mod settings file or env), defaulting to 8787, so it can match a non-default `--mod-url`.

---

## 8. Build & install (macOS-first)

**Toolchain:** mods target **.NET Framework 3.5 / Unity Mono**. On macOS the reliable compiler is **Mono's `msbuild`** (the .NET SDK's net35 support on Mac is finicky). `build.sh` checks for Mono and, if missing, prints the exact install step (`brew install mono`) rather than failing cryptically.

**Reference assemblies** (CONFIRMED set; `<HintPath>` in the `.csproj`), from the Mac bundle:
`…/steamapps/common/Cities_Skylines/Cities.app/Contents/Resources/Data/Managed/` — `ICities.dll`, `ColossalManaged.dll`, `Assembly-CSharp.dll`, `UnityEngine.dll` (optionally `UnityEngine.UI.dll`, `System.Core`). Research confirmed this net35 reference set (TM:PE's `.csproj`); the spike confirms the exact macOS `Managed/` path + precise assembly filenames on the user's install.

**No Harmony / no patching.** The mod reads state and calls public `NetManager`/`SimulationManager` APIs rather than intercepting game methods, so it needs **no `CitiesHarmony` dependency** — just the game assemblies above. (Revisit only if the spike finds a needed capability that requires patching.)

**`build.sh` (macOS-first):** detects the Steam game path (with an override env var), verifies the `Managed/` assemblies exist, runs `msbuild` to produce `SkylineBenchMod.dll`, and copies it into:
`~/Library/Application Support/Colossal Order/Cities_Skylines/Addons/Mods/SkylineBench/`.

**One manual step** (can't be scripted from outside the game): enabling the mod in CS1's **Content Manager → Mods**. The README documents the exact clicks and where to find the mod log for debugging.

**Cross-OS:** the `.csproj` and source are OS-independent (one DLL everywhere). Only paths differ; `build.sh` is Mac-first, with Windows/Linux `Managed/` and mods-folder paths documented in the README and fully automated later in the deferred `setup` plan.

---

## 9. Testing & verification

Most of the mod can't run without CS1, so the thin testable surface is tested mechanically and the rest is verified end-to-end by hand.

- **Pure unit tests (no game):** `json/` writer/reader round-trips asserting exact contract wire shapes (fixtures shared with the broker mock's output), query/body parsing, and the error-code mapping table. Runnable under Mono without CS1 — the one automatable layer.
- **The spike is a test:** `/probe` succeeding proves load + HTTP + manager access on the real game.
- **Manual end-to-end (user, with the game):** build & enable the mod, load a city, run `skylinebench serve --mod-url http://127.0.0.1:8787` against the *real* mod, and run the broker's loop by hand — reset/observe → `build_road` → `control_time step` → observe the metric change → `bulldoze`. A README checklist walks through it. This is the real acceptance gate.
- **`GameBridge` stays thin** precisely because it's the untested layer — keeping logic out of it is the test strategy.

---

## 10. Definition of done & scope

**Done when:**
1. `DISCOVERY.md` records the confirmed API, HTTP decision, prefab/zone lists, and real constants; the broker's deferred items + the spec §5 code set are resolved and synced back into the Phase-1 spec.
2. The mod implements every contract endpoint, returning the exact JSON shapes `contract.rs` parses, with structured errors (HTTP 200 for action failures).
3. `build.sh` compiles the net35 DLL and installs it on macOS; the README documents the Content-Manager enable + the verification checklist.
4. Pure unit tests (json/parsing/error-map) pass under Mono.
5. **Acceptance:** the user completes the manual observe→build→step→observe loop against the real game through the unchanged broker.

**Out of scope (deferred to a small follow-up plan):**
- Rust `setup`/`doctor` automation (Steam autodetect, cross-OS path handling, health-check chain) and an automated/scripted live smoke test.
- Game-camera screenshots (`POST /screenshot`, stubbed in Phase 1).

**Still out (unchanged Phase-1 cuts):** curves/multi-segment roads, terrain editing, service buildings, districts, policies, public transport, and any headless/CI execution of the game.

**Deliverable:** the `mod/` project (source + `build.sh` + README + `DISCOVERY.md`), a pure test project, and a real, playable CS1 ↔ broker ↔ agent loop on the user's machine.

---

## Notes for spec sync (resolved during the spike, fold back into the Phase-1 spec)

- Real **playable extent** (replaces the ±8,640 m working assumption in §5).
- Real **single-segment length limit** (replaces the broker's `MAX_SEGMENT_LENGTH_M = 200.0` working value).
- **Per-segment flow vs density** availability for `/metrics` (§6).
- The finalized **`ActionError` code set** including the broker-side `DEGENERATE_SEGMENT`/`INVALID_ARGS` and the game-outcome→code mapping.
- Whether **mid-session `load-save`** is possible, and any constraint on `reset_scenario`.
