# SkylineBench Phase 2 — Context & Handoff

> **Purpose:** This is a fresh-context handoff. Phase 1 is complete and merged to `main`. Read this end-to-end before starting Phase 2 — it captures everything the conversation history holds so you can pick up cold.
>
> **First action for Phase 2:** invoke the **`superpowers:brainstorming`** skill. Phase 2 (the benchmark) has NOT been designed yet — do not jump to implementation. The flow is: brainstorm → write spec → write plan → subagent-driven execution.

---

## 1. What SkylineBench is

A benchmark/harness for evaluating AI coding agents (Claude Code, Codex) by having them play **Cities: Skylines 1** (2015, Unity/Mono, C#/.NET 3.5) — specifically **traffic improvement**. An agent observes a city, builds/destroys roads and zoning, steps the simulation, and is scored on how much it improves traffic.

- **Phase 1 (DONE, merged to `main`):** the harness — an in-game mod + an MCP broker that together expose city state and actions to an agent.
- **Phase 2 (NEXT, this doc):** a minimal **traffic-improvement benchmark** — a deliberately bad-traffic starting map, a scoring metric, a run harness, and a reset mechanism.

## 2. Current state — Phase 1 (what you can build on)

Architecture: **thin C# mod (the bridge)** ↔ localhost HTTP ↔ **smart Rust broker (MCP server)** ↔ agent.

- The **mod** runs inside CS1, exposes raw wire endpoints on `http://127.0.0.1:8787`, has **zero business logic**, and marshals every game mutation onto the simulation thread.
- The **broker** is the agent-facing MCP server. It does assembly, validation, geometry, and translation, and renders map PNGs. It can also run a **mock mod** in-memory for dev/test.
- `broker/src/contract.rs` is the **frozen wire format** — the source of truth both sides serialize to. Changing it means changing both sides.

**All 14 wire endpoints are implemented and runtime-verified against game `1.21.1-f9`.** Both test suites are green (broker **37** + 1 doctest, mod **23**).

### Agent-facing MCP tools (broker)
`get_city_overview`, `observe_area`, `get_metrics`, `list_road_types`, `list_zone_types`, `build_road`, `bulldoze`, `upgrade_road`, `set_zoning`, `control_time`, `reset_scenario`, `render_map`.

### Metrics available (per `/metrics`)
- **traffic:** `flow_percent` (0–100, city-wide), `active_vehicles`, `segment_loads[]` (per-segment density 0–1)
- **economy:** `balance` (⚠️ *weekly net*, not cash), `weekly_income`, `weekly_expenses`, `funds` (cash; `Int64.Max` under unlimited money)
- **population:** `total`, `residential_demand`, `commercial_demand`, `workplace_demand` (CS1 has 3 demands, not 4 — workplace = industrial+office), `employed` (⚠️ hardcoded 0)
- **services:** `happiness`

## 3. Known limitations Phase 2 MUST design around

These are documented in `mod/DISCOVERY.md`; the load-save one is the big one for a benchmark:

1. **`reset_scenario` / `/load-save` is UNSTABLE.** It returns `{ok:true, city_loaded:true}` but mid-session `LoadLevel` **crashed the game** in testing. **A benchmark needs a dependable reset between runs — this is the single most important Phase 2 design problem.** Options to brainstorm: (a) load-at-startup only (relaunch the game per run), (b) snapshot/restore via save files managed by the harness out-of-process, (c) investigate/fix the mid-session crash, (d) run each agent against a fresh game process.
2. **No-city guard:** when `health.city_loaded == false`, sim-thread jobs time out after 8000 ms. The harness must check `/health` for `city_loaded:true` before acting.
3. **`employed` is always 0** — don't use it in scoring without computing it (Phase-2 candidate, via `CitizenManager`).
4. **`clock` step overshoots by ~2 ticks** (polls tick ≥ target). Fine for long runs; don't assume exact-N.
5. **`balance` ≠ cash on hand** (it's weekly net). Use `funds` for cash.

## 4. Repo map

```
broker/                      Rust MCP broker (the smart side)
  src/contract.rs            FROZEN wire types — source of truth, both sides match this
  src/tools.rs               MCP tool definitions (rmcp)
  src/service.rs             tool logic: validation, assembly, translation
  src/bridge_client.rs       HTTP client to the mod
  src/mock.rs                in-memory mock mod (axum) for dev/test
  src/render.rs              map PNG renderer (tiny-skia)
  src/geometry.rs graph.rs validate.rs   pure helpers
  src/main.rs                CLI: `serve` (real) / `mock` (dev)
mod/                         C# mod (the thin bridge, runs in CS1)
  src/Mod.cs                 IUserMod / lifecycle entry
  src/http/                  HttpServer, Router, Handlers, HttpQuery
  src/bridge/                GameReads, GameActions, ZoneWriter, SaveLoader,
                             SimThread (sim-thread marshaling), GameAccess, ErrorCode
  src/json/                  hand-rolled JsonReader/Writer/Serialize/RequestParse (net35, zero deps)
  src/dto/Dtos.cs            DTO structs
  src/probe/Probe.cs         the discovery probe (reflects over live managers)
  build.sh                   compile + install DLL to the game's Mods folder
  test/                      zero-dep test runner (23 tests)
  DISCOVERY.md               ⭐ real CS1 API findings + all runtime caveats — READ THIS
docs/superpowers/
  specs/2026-06-08-skylinebench-phase1-design.md    Phase 1 spec (contract §3, errors §5, metrics §6)
  specs/2026-06-09-skylinebench-mod-design.md        mod design spec
  research/2026-06-09-cs1-modding-api.md             cited CS1 API reference (CONFIRMED vs OPEN)
  plans/2026-06-08-skylinebench-broker.md            broker plan (executed)
  plans/2026-06-09-skylinebench-mod-2a-foundation.md mod foundation plan (executed)
  plans/2026-06-09-skylinebench-mod-2b-endpoints.md  mod endpoints plan (executed)
  2026-06-09-phase2-context.md                       THIS doc
```

## 5. Build / run / test

**Broker (Rust):**
```bash
cd broker
cargo test                                   # run tests (37 + 1 doctest)
cargo run -- mock                             # run in-memory mock mod on 127.0.0.1:8787 (no game needed)
cargo run -- serve --mod-url http://127.0.0.1:8787   # run MCP server (stdio) against the real mod
```

**Mod (C#, Mono):**
```bash
cd mod
./build.sh                                    # compile (Release) + install DLL to the game's Mods folder
                                              # override game path with MANAGED_DLL_PATH=...
# run tests (NOTE: Homebrew mono has NO msbuild — use xbuild):
xbuild test/Tests.csproj /p:Configuration=Debug && mono test/bin/Debug/Tests.exe
```
After `./build.sh`: **restart CS1**, enable "SkylineBench Bridge" in Content Manager → Mods, load a city. Then `curl http://127.0.0.1:8787/health` should show `city_loaded:true`.

**Toolchain gotchas:**
- Homebrew Mono ships **`xbuild`, not `msbuild`** — use `xbuild` for the mod and tests.
- `ikdasm` / `monodis` disassemble game assemblies to confirm signatures offline.
- Disassembling beats guessing — several Phase 1 bugs were API-signature mismatches caught this way.

## 6. Phase 2 — goal & open design questions (resolve in brainstorming)

**Goal:** a minimal, rigorous traffic-improvement benchmark on a bad-traffic starting map.

Open questions a brainstorm should settle (don't pre-decide — these are prompts):
- **Reset mechanism** (see §3.1) — the crux. How does each run start from an identical bad-traffic city?
- **The starting map** — hand-built bad-traffic save? procedurally degraded? How is it versioned/distributed?
- **Scoring metric** — `flow_percent` delta? travel-time? a composite (flow + happiness + throughput)? over how many ticks? How to make it stable/reproducible given sim nondeterminism and the ±2 tick step?
- **Run protocol** — pause→act→step loop vs free-running; budget (actions? wall-clock? ticks?); how an agent signals "done."
- **Harness** — what drives the agent and collects the score? Where does it live (new top-level dir? extend the broker)?
- **Anti-cheat / constraints** — e.g. unlimited money is on; should the benchmark constrain budget? bulldozing everything?

## 7. Deferred Phase 1.x polish (optional, brainstorm whether Phase 2 needs them)
- Rust `setup`/`doctor` automation + an opt-in live smoke test (`SKYLINEBENCH_LIVE=1`) — the one place the real game contract is checked end-to-end.
- Fast-fail guard when `city_loaded:false` (instead of 8000 ms timeout).
- Compute real `employed`.
- `/screenshot` endpoint (currently deferred/stubbed) — game-camera PNG, distinct from broker's `render_map`.

## 8. Working conventions (carry these forward)
- **Workflow:** superpowers — brainstorming → writing-plans → subagent-driven-development (fresh implementer subagent per task + spec-compliance review then code-quality review) → finishing-a-development-branch. Research the API before building when uncertain.
- **Git:** work on a feature branch; **merge each completed branch to `main` locally** (user's standing preference — no PRs). Commit messages end with the Co-Authored-By trailer.
- **Code style (user's global CLAUDE.md):** functional principles (no mutation; prefer reduce over for-loops/let); no `as`-casts or `any` except in test code; minimal comments (self-documenting code); functions as `(dependencies) => (arguments)` with object destructuring for multiple args. *(These are TS idioms — honor immutability/clarity in Rust/C# where they map; the literal syntax rules are TS-specific.)*
- **Contract discipline:** if Phase 2 needs new data, change `broker/src/contract.rs` and both sides together; keep the mod thin (no business logic in C#).
- The user runs the game manually and pastes endpoint output for in-game verification; the assistant can `curl` `127.0.0.1:8787` directly since it's on the same Mac.
