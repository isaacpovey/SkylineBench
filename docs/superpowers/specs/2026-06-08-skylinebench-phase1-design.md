# SkylineBench — Phase 1 Design (The Harness)

**Date:** 2026-06-08
**Status:** Approved for implementation planning
**Scope:** Phase 1 only — the MCP harness that lets an AI agent play Cities: Skylines 1. The benchmark itself (Phase 2) is a separate spec built on top of this tool surface.

---

## 1. Goal & guiding constraints

Build a harness/MCP server that enables AI coding agents (Claude Code, Codex, any MCP client) to *play* Cities: Skylines 1 (the 2015 Unity game): observe the city, build and destroy road networks, control simulation time, and read metrics — all programmatically.

The north star is a **rigorous evaluation artifact**. Every decision favours programmatic state access, deterministic-where-possible control, structured/legible data, and reproducible resets over visual fidelity or convenience.

**Hard constraints:**
- **Cross-platform:** must work on macOS, Windows, and Linux.
- **MCP server language: Rust** (`rmcp`). Chosen by the user for learning and possible future embedding of heavy local computation; speed is *not* a factor — the server is an I/O-bound broker.
- **In-game bridge language: C#** — non-negotiable; CS1 mods are C#/Unity-Mono.
- The harness owns the modding complexity; the user is new to Unity/C# modding.

**Known constraint (surfaced, not solved here):** CS1's simulation is **not perfectly deterministic** — internal randomness means bit-identical replays from a save are not guaranteed. This does not break a rigorous benchmark; Phase 2 will measure *metric deltas from a fixed savegame over enough sim time* rather than expecting identical replays. Phase 1 only needs clean time-control and reset primitives.

---

## 2. Architecture

Three processes on one machine:

```
┌─────────────┐   MCP/stdio   ┌──────────────────────┐  localhost HTTP  ┌──────────────────────┐
│ Agent       │ ────────────▶ │ Rust MCP server      │ ───────────────▶ │ C# mod (in CS1)      │
│ (Claude     │ ◀──────────── │ "the broker"         │ ◀─────────────── │ "the bridge"         │
│  Code/Codex)│  tools/results │                      │   JSON + data    │ HTTP listener on     │
└─────────────┘               │ • MCP tool defs      │                  │ 127.0.0.1:<port>     │
                              │ • graph assembly      │                  │ • reads sim managers │
                              │ • coordinate math     │                  │ • enqueues actions   │
                              │ • schematic renderer  │                  │   onto sim thread    │
                              │ • validation          │                  │ • clock control      │
                              └──────────────────────┘                  │ • savegame load      │
                                                                         └──────────────────────┘
```

**Chosen approach: thin mod + smart broker (Approach A).** The hard-to-test, slow-to-iterate, game-coupled code is confined to the mod's thin verb layer. Everything testable (graph, geometry, rendering, protocol, validation) lives in Rust and runs without the game.

Rejected alternatives:
- **Smart mod + thin Rust passthrough** — puts most logic in the slowest-to-iterate, least-testable place (C# inside a running game). Every change means recompiling the mod and relaunching CS1.
- **File-based IPC** — dead simple but high-latency, awkward request/response, poor binary/image return path. Acceptable only as a fallback.

**Transport:** localhost HTTP (debuggable with `curl`, cross-platform, clean request/response and binary return).

### 2.1 Components

1. **C# mod ("the bridge")** — minimal. Implements `IUserMod`, starts a lightweight HTTP listener on `127.0.0.1:<port>`. Exposes low-level verbs (read state, execute action, control clock, load save, health). Marshals every mutating action onto the game's **simulation thread** via `SimulationManager.AddAction` and waits for completion (sim state cannot be touched from the HTTP thread). Returns raw structured data; **zero business logic**.

2. **Rust MCP server ("the broker")** — the brains. Speaks MCP (`rmcp`) to the agent over stdio; speaks HTTP to the mod. Owns the agent-facing tool definitions, graph/building/zone/metric assembly, coordinate & geometry math, server-side schematic rendering, and request validation.

3. **Agent** — any MCP client. Sees only the clean tool surface.

### 2.2 Data flow (a typical benchmark-mode step)

`observe_area` → broker GETs raw state from mod → assembles graph + renders PNG → returns structured obs + image. `build_road(...)` → broker validates + translates to a mod verb → mod enqueues on sim thread, applies, returns result. `control_time(step, N)` → mod advances N ticks then re-pauses → agent calls `observe_*` to see the effect.

---

## 3. The C# mod bridge — low-level HTTP API

Small and stable. Localhost-only. JSON in/out. The mod returns **data, not images** (rendering is the broker's job). All coordinates are **world coordinates** (floats, metres); the broker owns any grid/tile conversion.

**Read:**
- `GET /health` → mod version, game version, city-loaded flag, sim paused state, current tick.
- `GET /network?bounds=…&types=…` → raw network: nodes (id, x, y, z), segments (id, startNode, endNode, prefab/road type, lanes, length); filterable by world-coordinate bounding box and net type.
- `GET /buildings?bounds=…` → buildings: id, prefab name, category (residential/commercial/industrial/office/service), position, footprint, level.
- `GET /zones?bounds=…` → zone grid cells with zone type.
- `GET /metrics` → city-wide + traffic metrics snapshot (see §6), taken on the sim thread at a consistent tick.

**Mutate (always marshalled onto the sim thread, synchronous reply):**
- `POST /action/build-road` → `{ startPos, endPos, prefab, snapToExistingNodes }` → created node/segment ids or structured error.
- `POST /action/bulldoze` → `{ targetType: "segment"|"node"|"building", id }` (or by position) → result.
- `POST /action/upgrade-road` → `{ segmentId, prefab }` → result.
- `POST /action/set-zone` → `{ cells | rect, zoneType }` → result. *(Zoning included because it drives traffic demand.)*

**Clock & lifecycle:**
- `POST /clock` → `{ op: "pause"|"resume"|"set-speed"|"step", ticks?, speed? }`.
- `POST /load-save` → `{ saveName }` → loads a named savegame (the reset primitive).
- `POST /screenshot` *(deferred, behind a flag)* → game-camera PNG; stubbed in Phase 1.

**Contracts:**
- Every mutating endpoint returns a **structured result or structured error** (e.g. `{ ok: false, reason: "INVALID_PREFAB" }`) — never a raw exception.
- The mod decides only whether the game API *accepted* an action, never whether it "makes sense."

---

## 4. The Rust broker — agent-facing MCP tools

Fewer, higher-level tools than the raw mod verbs; the broker does assembly, validation, and translation.

**Observation:**
- `get_city_overview` → population, budget, headline traffic metrics, bounds of built area. Cheap, low-token "glance."
- `observe_area({ center, radius | bounds })` → assembled **network graph** (nodes + segments with road types, lanes, connectivity) + buildings + zones in region, as structured JSON. Token-bounded by requested area.
- `render_map({ bounds, layers, scale })` → top-down **schematic PNG** rendered by the broker (roads by type/congestion, buildings by category, optional zone overlay). Returned as an MCP image.
- `get_metrics({ groups })` → metrics snapshot (§6), selectable by group for token control.

**Action:**
- `build_road({ from, to, road_type, snap })` → validated, then calls the mod. Returns created ids + diff summary.
- `bulldoze({ target })` → by id or position.
- `upgrade_road({ segment, road_type })`.
- `set_zoning({ area, zone_type })`.

**Control:**
- `control_time({ op, ticks?, speed? })` → pause / resume / set_speed / step. Supports both **step mode** (benchmark) and **free-running mode** (exploration).
- `reset_scenario({ save })` → reload a named save.
- `list_road_types()` / `list_zone_types()` → enumerate valid prefab identifiers so agents use correct internal names (which they cannot otherwise know).

**Internal modules (the testable core, not tools):**
- `graph` — raw nodes/segments → clean connectivity graph (adjacency, intersections, dead-ends).
- `geometry` — coordinate math, bounds, snapping helpers, distance.
- `render` — graph → PNG (e.g. `tiny-skia`/`raqote`); pure function, golden-image testable.
- `bridge_client` — typed HTTP client for the mod; the only I/O module.
- `validate` — pre-flight checks on action args.

**Token discipline:** observation tools are area/group-scoped so the agent pulls detail only where needed, keeping per-step context bounded — important for fair measurement.

---

## 5. Coordinate & build model

**Coordinate system.** CS1 world space is in **metres**, centred near origin; the playable area is roughly **±8,640 m** on X/Z (a 9×9 grid of 1,920 m tiles), Y is elevation. The broker exposes raw world coordinates to the agent — coordinates read from `observe_area` are the same ones passed to `build_road`. *(Exact playable extent will be confirmed against the live API; the value above is the working assumption.)*

**Roads are node→segment graphs.** A road is a **segment** between two **nodes**; nodes are junctions/turns. `build_road({ from, to, road_type })` creates or reuses nodes at `from`/`to` and a segment between them. **Phase 1 builds straight segments only**; agents chain them for longer/curved paths. Curves/bezier control points are a deliberate Phase-1 cut.

**Snapping.** `snap: true` (default) reuses an existing node within a tolerance of `from`/`to` instead of creating a floating duplicate — this is what makes new roads actually connect to the network. The broker resolves snapping from the graph it already holds, then instructs the mod to attach to an existing node id or create one.

**Validation before the game sees it.** The broker rejects, with structured errors, builds that: use an unknown `road_type`, fall outside playable bounds, are degenerate (`from ≈ to`), or exceed the game's single-segment length limit (suggesting the agent split the road).

**Build result is a diff.** Every successful action returns what changed — new node/segment ids, what was snapped, what was destroyed — so the agent updates its model without a full re-observe.

**Normalised error set:** in-game failures the broker cannot fully predict (collisions, terrain, funds) come back from the mod and are normalised into a small enumerated set: `COLLISION`, `INSUFFICIENT_FUNDS`, `OUT_OF_BOUNDS`, `INVALID_PREFAB`, `SEGMENT_TOO_LONG`, `UNKNOWN`.

---

## 6. Metrics & observations

A single `/metrics` snapshot is taken **on the sim thread at a consistent tick** (numbers not torn across a tick boundary) and **stamped with the sim tick**. `get_metrics({ groups })` selects groups for token control.

**`traffic` (core for Phase 2):**
- City-wide **traffic flow %** — CS1's headline congestion indicator (≈ average actual-speed ÷ free-flow speed); lower = worse.
- **Active vehicle count** on the network.
- **Per-segment traffic density / load** — read per segment; the broker aggregates (top-N congested segments) and feeds congestion colouring in `render_map`. This is the granular signal for locating jams.
- **Intersection / flow hotspots** — derived in the broker from per-segment data + graph (worst nodes by incoming load).

**`economy`:** budget balance, weekly income/expenses, total funds.

**`population`:** total population; residential/commercial/industrial/office demand; employment.

**`services` (light):** overall happiness + a few headline coverage indicators — included because they can confound a traffic experiment, so they stay visible.

**Honest caveat:** exact internal availability of some signals (notably clean per-segment *flow* vs *density*) will be confirmed against the live CS1 managers during implementation. The set above is the *intended* surface; the implementation plan must verify each field and note any fallback. No field is promised that hasn't been confirmed against the API.

---

## 7. Project layout (high-level)

The repo has two top-level build artifacts and supporting directories. This is the *skeleton only* — concrete crate names, module files, `.csproj`/MSBuild details, and dependency versions are decided in the implementation plan, not here.

```
SkylineBench/
├── broker/          # Rust workspace — the `skylinebench` binary
│                    #   single entry point for both serving and tooling:
│                    #     skylinebench serve   (MCP server over stdio)
│                    #     skylinebench setup   (locate game, build+install mod)
│                    #     skylinebench doctor  (health-check the full chain)
│                    #   internal modules from §4: graph, geometry, render,
│                    #     bridge_client, validate
├── mod/             # C# net35 project → the bridge DLL (HTTP listener + verbs)
├── fixtures/        # captured real-mod responses for the mock-mod tests (§9)
└── docs/            # this spec + the setup README
```

**Notes:**
- The **Rust binary is the single entry point** for both serving the MCP tools and the setup/doctor tooling (no separate shell scripts) — making the setup flow in §8 explicit at the structure level.
- The **mod is one C# project** producing a single DLL; its thinness (§2.1, §9) means it stays small.
- Anything finer-grained — module-to-file mapping, the HTTP server/listener crate choices, `rmcp`/`tiny-skia`/`raqote` versions, the C# HTTP listener choice — lands in the plan.

---

## 8. Setup & cross-platform story

The harness makes setup turnkey for a modding newcomer across all three OSes.

**Mod build constraint.** CS1 uses Unity's old Mono runtime, so the mod targets **.NET Framework 3.5 (`net35`)** and compiles against the game's bundled assemblies (`ICities.dll`, `ColossalManaged.dll`, `Assembly-CSharp.dll`, `UnityEngine*.dll`), located per OS:

| | Game `Managed/` (reference DLLs) | Mod install dir |
|---|---|---|
| **Windows** | `…/steamapps/common/Cities_Skylines/Cities_Data/Managed/` | `%LOCALAPPDATA%\Colossal Order\Cities_Skylines\Addons\Mods\` |
| **macOS** | `…/Cities_Skylines/Cities.app/Contents/Resources/Data/Managed/` | `~/Library/Application Support/Colossal Order/Cities_Skylines/Addons/Mods/` |
| **Linux** | `…/steamapps/common/Cities_Skylines/Cities_Data/Managed/` | `~/.local/share/Colossal Order/Cities_Skylines/Addons/Mods/` |

**Setup flow (tooling built into the Rust binary — no extra runtime dependency):**
1. `skylinebench setup` — detects OS, locates the Steam install (with an override flag), verifies reference assemblies exist, and installs the mod.
2. **Mod build** — via `dotnet`/`msbuild` targeting `net35`, `<HintPath>`s resolved from the detected `Managed/` dir; the build copies the DLL into the OS-specific mods folder.
3. **Enable the mod** in CS1's Content Manager — manual one-time step (cannot be scripted from outside the game), documented with exact clicks.
4. **Register the MCP server** with the agent — documented stdio config snippets for Claude Code and Codex pointing at the `skylinebench` binary.
5. `skylinebench doctor` — calls `/health` and confirms the chain (binary → mod → loaded city) is live.

**Cross-platform isolation:** the Rust broker and the C# mod DLL are identical everywhere; only *paths* and *Steam-detection logic* branch per OS, isolated in one module. No per-OS code in tool logic.

**Honest caveats:**
- CS1 has **no headless mode** — the game must run with a display. Fine for local eval; a consideration for any future CI/remote runner.
- Enabling the mod and the initial city load are **manual one-time steps**; everything after is programmatic.

---

## 9. Testing strategy

CS1 cannot run in CI, so the strategy is to make **most of the system testable without the game** and keep the untestable part tiny.

**Rust broker (the bulk, TDD):**
- `graph`, `geometry`, `validate` — pure-function unit tests (connectivity assembly, snapping tolerance, bounds/length validation, error mapping).
- `render` — **golden-image tests**: fixed graph in, assert PNG matches a committed reference; regenerate deliberately on intended change.
- `bridge_client` + tool handlers — tested against a **mock mod** (local fake HTTP server returning canned fixtures), exercising the entire broker end-to-end with zero game dependency.

**Fixtures kept honest:** canned responses are captured from a **real running mod** once and committed; a documented capture command refreshes them when the mod's output changes.

**C# mod (thin, light testing):** almost no logic to unit-test by design. Covered by (a) a couple of pure helper tests where logic exists (e.g. coordinate parsing) and (b) the integration smoke test below. Keeping it thin *is* the test strategy.

**Integration smoke test (opt-in, game required):** one scripted scenario — `load_save → observe → build a known road → step → observe the diff/metric change` — run manually or behind `SKYLINEBENCH_LIVE=1`, never in CI. The single place the real game contract is verified end-to-end; `doctor` is its lightweight cousin.

**CI runs:** Rust unit + golden + mock-integration tests, and the C# mod *compiles* (against a cached/stubbed `Managed/` set). No live game.

---

## 10. Phase 1 definition of done

Phase 1 is complete when **an agent, through MCP alone, can observe, act, and see the consequence** — the loop Phase 2 will score.

**Functional acceptance — an agent (Claude Code or Codex) can, end to end:**
1. `reset_scenario` to load a known savegame.
2. `get_city_overview` + `get_metrics({traffic})` returning consistent, tick-stamped numbers.
3. `observe_area` returning a clean network graph + buildings/zones; `render_map` returning a legible top-down PNG.
4. `list_road_types` / `list_zone_types`, then `build_road`, `upgrade_road`, `set_zoning`, `bulldoze` — each returning a structured diff or a normalised structured error.
5. `control_time` to pause / step N ticks / resume, in both step and free-running modes.
6. After a build + step, observe a **measurable change** in the relevant metric (proof the loop is real).

**Quality bar:**
- Rust core (`graph`/`geometry`/`render`/`validate`) unit + golden tested; broker tested against the mock mod; the live smoke scenario passes once on a real install.
- `skylinebench setup` and `doctor` work on the user's macOS machine, with Windows/Linux paths implemented and documented (verified where access exists; clearly marked if untested).
- Every action error is one of the enumerated structured codes — no raw game exceptions leak to the agent.

**Explicitly OUT of scope for Phase 1 (deferred):**
- The benchmark itself — scoring, run orchestration, the bad-traffic starting map, agent harness/leaderboard (all Phase 2).
- Curved/multi-segment road tools, terrain editing, service buildings, districts, policies, public-transport placement.
- Game-camera screenshots (stubbed behind a flag).
- Any headless/remote/CI execution of the actual game.

**Deliverable:** a `skylinebench` Rust binary (MCP server + setup/doctor), a compiled CS1 mod + its source, fixtures, tests, and a setup README — such that Phase 2 can be built entirely on top of this tool surface without touching the mod again.
