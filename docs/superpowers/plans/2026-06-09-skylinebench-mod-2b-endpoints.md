# SkylineBench Mod 2b — Contract Endpoints & Verification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **GAME-GATED at the end.** Phases A–C are ordinary code/TDD (Rust broker sync; mod serializers/parsers are pure + Mono-testable; the GameBridge compiles against the local CS1 assemblies). **Phase D (final manual verify) requires the human to run Cities: Skylines.** Signatures are pinned from `mod/DISCOVERY.md` + offline `ikdasm`; a few deep internals are flagged to confirm with `ikdasm` during their task (the assemblies are local).

**Goal:** Implement every remaining SkylineBench contract endpoint in the CS1 mod (network/buildings/zones/metrics reads; build-road/bulldoze/upgrade-road/set-zone/clock/load-save actions) so an agent can run the full observe→build→step→observe loop against the real game through the unchanged broker — plus the small broker contract-sync the discovery surfaced.

**Architecture:** Per the mod spec §2: pure DTO structs + pure JSON serializers/parsers (Mono-unit-tested off-game), and a `GameBridge` that is the only code touching CS1 managers (every mutation marshalled through `SimThread.Run`, drains-while-paused confirmed in 2a). The broker is unchanged except a contract field rename the discovery required.

**Tech Stack:** C# net35 (mod, build with `xbuild`), the existing pure helpers (`JsonWriter`/`JsonReader`/`HttpQuery`), CS1 managers (`NetManager`/`BuildingManager`/`ZoneManager`/`VehicleManager`/`EconomyManager`/`DistrictManager`/`SimulationManager`/`LoadingManager`); Rust for the broker contract-sync. Game assemblies at `$HOME/Library/Application Support/Steam/steamapps/common/Cities_Skylines/Cities.app/Contents/Resources/Data/Managed`.

Implements `docs/superpowers/specs/2026-06-09-skylinebench-mod-design.md` §5/§6, building on 2a (foundation) and `mod/DISCOVERY.md` (resolved signatures). The broker's `broker/src/contract.rs` is the frozen wire format — this plan changes ONE struct in it (PopulationMetrics) and the mod matches it.

---

## Signatures pinned (from DISCOVERY.md + ikdasm on CS1 1.21.1-f9) — use these, don't re-derive

- **Net read:** `NetManager.instance.m_segments.m_buffer[id]` → `NetSegment` `{ m_startNode, m_endNode (ushort), m_infoIndex (ushort)→.Info.name, m_averageLength (float), m_trafficDensity (byte), m_flags (NetSegment.Flags; Created bit = exists), m_lanes }`; `m_nodes.m_buffer[id]` → `NetNode { m_position (Vector3), m_flags }`. Counts: `m_segmentCount`/`m_nodeCount` are *unreliable*; iterate the buffer and test `(m_flags & Created) != 0`.
- **Net write:** `CreateNode(out ushort id, ref Randomizer, NetInfo, Vector3 pos, uint buildIndex)`; `CreateSegment(out ushort id, ref Randomizer, NetInfo, ushort start, ushort end, Vector3 startDir, Vector3 endDir, uint buildIndex, uint buildIndex2, bool invert) → bool`; `ReleaseSegment(ushort, bool keepNodes)`; `ReleaseNode(ushort)`. Straight seg: `startDir = VectorUtils.NormalizeXZ(endPos-startPos)`, `endDir = -startDir`, `buildIndex = SimulationManager.instance.m_currentBuildIndex` then `+= 2` on success.
- **Prefabs:** `int PrefabCollection<NetInfo>.PrefabCount()`, `GetPrefab(uint)`; names have spaces (e.g. "Basic Road").
- **Traffic:** per-segment `NetSegment.m_trafficDensity` (byte 0–255); city flow `VehicleManager.instance` `m_lastTrafficFlow`/`m_maxTrafficFlow` (uint) → `flow% = maxFlow==0 ? 100 : lastFlow*100/maxFlow`; active vehicles `VehicleManager.instance.cityVehicleCount` (uint prop).
- **Buildings:** `BuildingManager.instance.m_buildings.m_buffer[id]` → `Building { m_position (Vector3), m_infoIndex (ushort)→.Info, m_flags (Building.Flags; Created=exists), m_level (byte) }`; category via `building.Info.m_class.m_service`/`m_subService` (ItemClass).
- **Zones:** `ZoneManager.instance.m_blocks.m_buffer[id]` → `ZoneBlock { m_position (Vector3), m_angle (float), m_zone1/m_zone2 (ulong, 4-bit nibbles per cell; block = 4 cols × 8 rows of 8 m cells), m_valid (ulong) }`; cell zone = `ItemClass.Zone` nibble. **CONFIRM the exact nibble layout & write API with ikdasm in Task C-zones.**
- **Economy:** funds `EconomyManager.instance.LastCashAmount` (Int64, displayed cash); weekly income/expenses `GetIncomeAndExpenses(ItemClass.Service.None, ItemClass.SubService.None, ItemClass.Level.None, out long income, out long expenses)` + add `GetLoanExpenses()`+`GetPolicyExpenses()` to expenses.
- **Population:** `DistrictManager.instance.m_districts.m_buffer[0].m_populationData.m_finalCount` (uint32). **RCI demand** `ZoneManager.instance` `m_actualResidentialDemand`/`m_actualCommercialDemand`/`m_actualWorkplaceDemand` (Int32) — **3 values (R/C/Workplace)**, see Phase A.
- **Clock:** via `IThreading` (`ModRuntime.Threading`): `simulationPaused`, `simulationSpeed` (1..3), `simulationTick`. `step` = unpause, busy-wait N ticks via `simulationTick`, re-pause. Drains-while-paused confirmed in 2a.
- **Load:** `LoadingManager.instance.LoadLevel(Package.Asset, "Game", null, new SimulationMetaData(), false)` — locate the save Asset by name. **CONFIRM the asset lookup with ikdasm in Task C-load.**

---

## File structure

Broker (Rust): `broker/src/contract.rs`, `broker/src/mock.rs`, `broker/src/service.rs` (Phase A only).

Mod (C#, all under `mod/`):

| File | Responsibility |
|---|---|
| `src/dto/Dtos.cs` | Plain structs the GameBridge produces: `NetworkDto`, `BuildingsDto`, `ZonesDto`, `MetricsDto`, `ActionResultDto`. No game types. |
| `src/json/Serialize.cs` | Pure: DTO → JSON string (via `JsonWriter`), exactly matching `contract.rs`. Unit-tested. |
| `src/json/RequestParse.cs` | Pure: `JsonValue` → action arg structs (`BuildRoadReq`, `BulldozeReq`, `UpgradeRoadReq`, `SetZoneReq`, `ClockReq`, `LoadSaveReq`). Unit-tested. |
| `src/bridge/ErrorCode.cs` | The normalized reason strings + a mapping helper. |
| `src/bridge/GameReads.cs` | `GameBridge` read methods: managers → DTO (network/buildings/zones/metrics), via `SimThread.Run`. Game-coupled. |
| `src/bridge/GameActions.cs` | `GameBridge` write methods: build/bulldoze/upgrade/set-zone/clock/load-save, via `SimThread.Run`. Game-coupled. |
| `src/http/Handlers.cs` | Extended: add the contract endpoint handlers. |
| `src/http/Router.cs` | Extended: add the contract routes. |
| `test/SerializeTests.cs`, `test/RequestParseTests.cs` | Pure unit tests (added to `Tests.csproj`). |

---

## Phase A — Broker contract-sync (PopulationMetrics → R/C/Workplace)

Discovery found CS1 exposes 3 demands (residential/commercial/workplace), not the 4 the contract assumed. Align the frozen contract before the mod implements `/metrics`.

### Task A1: Rename PopulationMetrics demands in the broker

**Files:**
- Modify: `broker/src/contract.rs` (PopulationMetrics struct)
- Modify: `broker/src/mock.rs` (PopulationMetrics construction)

- [ ] **Step 1: Update the contract struct**

In `broker/src/contract.rs`, replace the `PopulationMetrics` struct's demand fields:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PopulationMetrics {
    pub total: u32,
    pub residential_demand: u8,
    pub commercial_demand: u8,
    pub workplace_demand: u8,
    pub employed: u32,
}
```
(Removes `industrial_demand` + `office_demand`; adds `workplace_demand`.)

- [ ] **Step 2: Update the mock**

In `broker/src/mock.rs`, find the `PopulationMetrics { ... }` construction in the `metrics` handler and replace the demand fields:

```rust
        population: PopulationMetrics { total: 1000, residential_demand: 50, commercial_demand: 40, workplace_demand: 30, employed: 700 },
```

- [ ] **Step 3: Build + test the broker**

Run: `cd broker && cargo test --quiet 2>&1 | grep "test result" && cargo clippy --all-targets --quiet -- -D warnings && echo clippy-ok`
Expected: all tests pass (the `metrics_round_trips` contract test exercises the new shape), clippy clean.

- [ ] **Step 4: Commit**

```bash
git add broker/src/contract.rs broker/src/mock.rs
git commit -m "feat(broker): align PopulationMetrics to CS1's R/C/Workplace demand (3 not 4)"
```

> Spec-sync: also update spec §6 wording in a docs pass at the end (Task D2).

---

## Phase B — Mod DTOs, serializers, request parsers (TDD, no game)

Pure code: produced by `GameBridge`, serialized to the exact wire shapes, and request bodies parsed to arg structs. All Mono-unit-testable. Build/run tests with `xbuild`/`mono` as in 2a (cosmetic runtime warning is harmless).

### Task B1: DTO structs

**Files:**
- Create: `mod/src/dto/Dtos.cs`
- Modify: `mod/SkylineBenchMod.csproj` (add the new Compile entries for dto/json files created in Phase B — add as each lands)

- [ ] **Step 1: Create the DTOs**

Create `mod/src/dto/Dtos.cs` — plain data, no game types (so serializers are pure-testable):

```csharp
using System.Collections.Generic;

namespace SkylineBench.Dto
{
    public struct NodeDto { public uint Id; public float X; public float Y; public float Z; }
    public struct SegmentDto { public uint Id; public uint StartNode; public uint EndNode; public string Prefab; public byte Lanes; public float Length; }
    public sealed class NetworkDto { public List<NodeDto> Nodes = new List<NodeDto>(); public List<SegmentDto> Segments = new List<SegmentDto>(); }

    public struct BuildingDto { public uint Id; public string Prefab; public string Category; public float X; public float Y; public float Z; public float FootprintWidth; public float FootprintLength; public byte Level; }
    public sealed class BuildingsDto { public List<BuildingDto> Buildings = new List<BuildingDto>(); }

    public struct ZoneCellDto { public float X; public float Z; public string ZoneType; }
    public sealed class ZonesDto { public List<ZoneCellDto> Cells = new List<ZoneCellDto>(); }

    public struct SegmentLoadDto { public uint SegmentId; public float Density; }
    public sealed class MetricsDto
    {
        public ulong Tick;
        public float FlowPercent; public uint ActiveVehicles; public List<SegmentLoadDto> SegmentLoads = new List<SegmentLoadDto>();
        public long Balance; public long WeeklyIncome; public long WeeklyExpenses; public long Funds;
        public uint Population; public byte ResidentialDemand; public byte CommercialDemand; public byte WorkplaceDemand; public uint Employed;
        public byte Happiness;
    }

    /// <summary>Result of a mutation. Ok==true ⇒ diff fields meaningful; else Reason set (a normalized code).</summary>
    public sealed class ActionResultDto
    {
        public bool Ok;
        public List<uint> CreatedNodes = new List<uint>();
        public List<uint> CreatedSegments = new List<uint>();
        public List<uint> SnappedNodes = new List<uint>();
        public List<uint> Destroyed = new List<uint>();
        public string Reason; // null when Ok
        public static ActionResultDto Fail(string reason) { return new ActionResultDto { Ok = false, Reason = reason }; }
    }
}
```

Add to `mod/SkylineBenchMod.csproj` `<ItemGroup>`: `<Compile Include="src\dto\Dtos.cs" />`.

- [ ] **Step 2: Build the mod to confirm it compiles** (no test yet — DTOs are used by serializers next)

Run: `cd mod && xbuild /p:Configuration=Release SkylineBenchMod.csproj` → 0 errors.

- [ ] **Step 3: Commit**

```bash
git add mod/src/dto/Dtos.cs mod/SkylineBenchMod.csproj
git commit -m "feat(mod): add response/action DTO structs"
```

### Task B2: Response serializers (TDD)

**Files:**
- Create: `mod/src/json/Serialize.cs`
- Create: `mod/test/SerializeTests.cs`
- Modify: `mod/test/Tests.csproj` (compile `Serialize.cs` + `Dtos.cs` + `SerializeTests.cs`); `mod/SkylineBenchMod.csproj` (compile `Serialize.cs`)
- Modify: `mod/test/TestRunner.cs` (register `SerializeTests`)

- [ ] **Step 1: Wire the test registrar**

In `mod/test/TestRunner.cs`, add `SerializeTests.Register(tests);` after the existing registrations.

- [ ] **Step 2: Write failing tests**

Create `mod/test/SerializeTests.cs`:

```csharp
using System;
using System.Collections.Generic;
using SkylineBench.Dto;
using SkylineBench.Json;

namespace SkylineBench.Tests
{
    public static class SerializeTests
    {
        public static void Register(List<KeyValuePair<string, Action>> tests)
        {
            tests.Add(new KeyValuePair<string, Action>("serialize: network", Network));
            tests.Add(new KeyValuePair<string, Action>("serialize: metrics shape", Metrics));
            tests.Add(new KeyValuePair<string, Action>("serialize: action ok", ActionOk));
            tests.Add(new KeyValuePair<string, Action>("serialize: action error omits diff", ActionErr));
        }

        static void Network()
        {
            var net = new NetworkDto();
            net.Nodes.Add(new NodeDto { Id = 1, X = -50f, Y = 0f, Z = 10f });
            net.Segments.Add(new SegmentDto { Id = 7, StartNode = 1, EndNode = 2, Prefab = "Basic Road", Lanes = 2, Length = 100f });
            Assert.Equal(
                "{\"nodes\":[{\"id\":1,\"x\":-50,\"y\":0,\"z\":10}],\"segments\":[{\"id\":7,\"start_node\":1,\"end_node\":2,\"prefab\":\"Basic Road\",\"lanes\":2,\"length\":100}]}",
                Serialize.Network(net));
        }

        static void Metrics()
        {
            var m = new MetricsDto { Tick = 42, FlowPercent = 73.5f, ActiveVehicles = 120, Balance = 0, WeeklyIncome = 500, WeeklyExpenses = 400, Funds = 50000, Population = 2000, ResidentialDemand = 50, CommercialDemand = 40, WorkplaceDemand = 30, Employed = 1500, Happiness = 80 };
            m.SegmentLoads.Add(new SegmentLoadDto { SegmentId = 7, Density = 0.5f });
            string json = Serialize.Metrics(m);
            // Spot-check structure + the contract-critical groups.
            Assert.True(json.StartsWith("{\"tick\":42,"), "starts with tick");
            Assert.True(json.Contains("\"traffic\":{\"flow_percent\":73.5,\"active_vehicles\":120,\"segment_loads\":[{\"segment_id\":7,\"density\":0.5}]}"), "traffic group: " + json);
            Assert.True(json.Contains("\"economy\":{\"balance\":0,\"weekly_income\":500,\"weekly_expenses\":400,\"funds\":50000}"), "economy group");
            Assert.True(json.Contains("\"population\":{\"total\":2000,\"residential_demand\":50,\"commercial_demand\":40,\"workplace_demand\":30,\"employed\":1500}"), "population group");
            Assert.True(json.Contains("\"services\":{\"happiness\":80}"), "services group");
        }

        static void ActionOk()
        {
            var r = new ActionResultDto { Ok = true };
            r.CreatedNodes.Add(1); r.CreatedNodes.Add(2); r.CreatedSegments.Add(7); r.SnappedNodes.Add(1);
            Assert.Equal(
                "{\"ok\":true,\"created_nodes\":[1,2],\"created_segments\":[7],\"snapped_nodes\":[1],\"destroyed\":[]}",
                Serialize.Action(r));
        }

        static void ActionErr()
        {
            Assert.Equal("{\"ok\":false,\"reason\":\"INVALID_PREFAB\"}", Serialize.Action(ActionResultDto.Fail("INVALID_PREFAB")));
        }
    }
}
```

- [ ] **Step 3: Implement the serializers**

Create `mod/src/json/Serialize.cs`:

```csharp
using SkylineBench.Dto;

namespace SkylineBench.Json
{
    /// <summary>Pure DTO → JSON, matching the broker's contract.rs wire shapes exactly.</summary>
    public static class Serialize
    {
        public static string Network(NetworkDto net)
        {
            var w = new JsonWriter();
            w.BeginObject();
            w.Name("nodes").BeginArray();
            foreach (var n in net.Nodes)
                w.BeginObject().Name("id").Value((long)n.Id).Name("x").Value(n.X).Name("y").Value(n.Y).Name("z").Value(n.Z).EndObject();
            w.EndArray();
            w.Name("segments").BeginArray();
            foreach (var s in net.Segments)
                w.BeginObject().Name("id").Value((long)s.Id).Name("start_node").Value((long)s.StartNode).Name("end_node").Value((long)s.EndNode)
                    .Name("prefab").Value(s.Prefab).Name("lanes").Value((long)s.Lanes).Name("length").Value(s.Length).EndObject();
            w.EndArray();
            w.EndObject();
            return w.ToString();
        }

        public static string Buildings(BuildingsDto b)
        {
            var w = new JsonWriter();
            w.BeginObject().Name("buildings").BeginArray();
            foreach (var x in b.Buildings)
                w.BeginObject().Name("id").Value((long)x.Id).Name("prefab").Value(x.Prefab).Name("category").Value(x.Category)
                    .Name("x").Value(x.X).Name("y").Value(x.Y).Name("z").Value(x.Z)
                    .Name("footprint_width").Value(x.FootprintWidth).Name("footprint_length").Value(x.FootprintLength)
                    .Name("level").Value((long)x.Level).EndObject();
            w.EndArray().EndObject();
            return w.ToString();
        }

        public static string Zones(ZonesDto z)
        {
            var w = new JsonWriter();
            w.BeginObject().Name("cells").BeginArray();
            foreach (var c in z.Cells)
                w.BeginObject().Name("x").Value(c.X).Name("z").Value(c.Z).Name("zone_type").Value(c.ZoneType).EndObject();
            w.EndArray().EndObject();
            return w.ToString();
        }

        public static string Metrics(MetricsDto m)
        {
            var w = new JsonWriter();
            w.BeginObject().Name("tick").Value((long)m.Tick);
            w.Name("traffic").BeginObject().Name("flow_percent").Value(m.FlowPercent).Name("active_vehicles").Value((long)m.ActiveVehicles)
                .Name("segment_loads").BeginArray();
            foreach (var sl in m.SegmentLoads)
                w.BeginObject().Name("segment_id").Value((long)sl.SegmentId).Name("density").Value(sl.Density).EndObject();
            w.EndArray().EndObject();
            w.Name("economy").BeginObject().Name("balance").Value(m.Balance).Name("weekly_income").Value(m.WeeklyIncome)
                .Name("weekly_expenses").Value(m.WeeklyExpenses).Name("funds").Value(m.Funds).EndObject();
            w.Name("population").BeginObject().Name("total").Value((long)m.Population).Name("residential_demand").Value((long)m.ResidentialDemand)
                .Name("commercial_demand").Value((long)m.CommercialDemand).Name("workplace_demand").Value((long)m.WorkplaceDemand)
                .Name("employed").Value((long)m.Employed).EndObject();
            w.Name("services").BeginObject().Name("happiness").Value((long)m.Happiness).EndObject();
            w.EndObject();
            return w.ToString();
        }

        public static string Action(ActionResultDto r)
        {
            var w = new JsonWriter();
            w.BeginObject().Name("ok").Value(r.Ok);
            if (r.Ok)
            {
                WriteUintArray(w, "created_nodes", r.CreatedNodes);
                WriteUintArray(w, "created_segments", r.CreatedSegments);
                WriteUintArray(w, "snapped_nodes", r.SnappedNodes);
                WriteUintArray(w, "destroyed", r.Destroyed);
            }
            else { w.Name("reason").Value(r.Reason); }
            w.EndObject();
            return w.ToString();
        }

        private static void WriteUintArray(JsonWriter w, string name, System.Collections.Generic.List<uint> xs)
        {
            w.Name(name).BeginArray();
            foreach (var x in xs) w.Value((long)x);
            w.EndArray();
        }
    }
}
```

Add `Serialize.cs` (+ `Dtos.cs`) to **both** `mod/SkylineBenchMod.csproj` and the test `mod/test/Tests.csproj` Compile lists (the test project needs `Dtos.cs` + `Serialize.cs` linked, like the other pure files).

- [ ] **Step 4: Build + run tests**

Run: `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe ; echo "exit=$?"`
Expected: prior 13 + 4 new serialize tests pass → `17 passed, 0 failed`, exit 0.

- [ ] **Step 5: Commit**

```bash
git add mod/src/json/Serialize.cs mod/test/SerializeTests.cs mod/test/TestRunner.cs mod/test/Tests.csproj mod/SkylineBenchMod.csproj
git commit -m "feat(mod): add pure response serializers with tests"
```

### Task B3: Request parsers (TDD)

**Files:**
- Create: `mod/src/json/RequestParse.cs`
- Create: `mod/test/RequestParseTests.cs`
- Modify: `mod/test/Tests.csproj`, `mod/SkylineBenchMod.csproj`, `mod/test/TestRunner.cs`

- [ ] **Step 1: Register + write failing tests**

In `mod/test/TestRunner.cs` add `RequestParseTests.Register(tests);`. Create `mod/test/RequestParseTests.cs`:

```csharp
using System;
using System.Collections.Generic;
using SkylineBench.Json;

namespace SkylineBench.Tests
{
    public static class RequestParseTests
    {
        public static void Register(List<KeyValuePair<string, Action>> tests)
        {
            tests.Add(new KeyValuePair<string, Action>("parse: build-road", BuildRoad));
            tests.Add(new KeyValuePair<string, Action>("parse: clock step", Clock));
            tests.Add(new KeyValuePair<string, Action>("parse: set-zone rect", SetZone));
        }

        static void BuildRoad()
        {
            var r = RequestParse.BuildRoad(JsonReader.Parse(
                "{\"start\":{\"x\":-50,\"y\":0,\"z\":10},\"end\":{\"x\":50,\"y\":0,\"z\":10},\"prefab\":\"Basic Road\",\"snap_to_existing_nodes\":true}"));
            Assert.Equal(-50.0, r.StartX); Assert.Equal(50.0, r.EndX); Assert.Equal(10.0, r.StartZ);
            Assert.Equal("Basic Road", r.Prefab);
            Assert.True(r.Snap, "snap");
        }

        static void Clock()
        {
            var r = RequestParse.Clock(JsonReader.Parse("{\"op\":\"step\",\"ticks\":256}"));
            Assert.Equal("step", r.Op);
            Assert.True(r.Ticks == 256, "ticks");
        }

        static void SetZone()
        {
            var r = RequestParse.SetZone(JsonReader.Parse("{\"rect\":{\"min_x\":0,\"min_z\":0,\"max_x\":16,\"max_z\":16},\"zone_type\":\"residential\"}"));
            Assert.Equal(0.0, r.MinX); Assert.Equal(16.0, r.MaxZ);
            Assert.Equal("residential", r.ZoneType);
        }
    }
}
```

- [ ] **Step 2: Implement the parsers**

Create `mod/src/json/RequestParse.cs`:

```csharp
namespace SkylineBench.Json
{
    public struct BuildRoadReq { public float StartX, StartY, StartZ, EndX, EndY, EndZ; public string Prefab; public bool Snap; }
    public struct BulldozeReq { public string TargetType; public uint Id; }
    public struct UpgradeRoadReq { public uint SegmentId; public string Prefab; }
    public struct SetZoneReq { public float MinX, MinZ, MaxX, MaxZ; public string ZoneType; }
    public struct ClockReq { public string Op; public int Ticks; public int Speed; }
    public struct LoadSaveReq { public string SaveName; }

    /// <summary>Pure: JsonValue (parsed request body) → typed action arg structs.
    /// Field names match the broker's bridge_client JSON bodies.</summary>
    public static class RequestParse
    {
        public static BuildRoadReq BuildRoad(JsonValue v)
        {
            var s = v["start"]; var e = v["end"];
            return new BuildRoadReq
            {
                StartX = (float)s["x"].AsDouble(), StartY = (float)s["y"].AsDouble(), StartZ = (float)s["z"].AsDouble(),
                EndX = (float)e["x"].AsDouble(), EndY = (float)e["y"].AsDouble(), EndZ = (float)e["z"].AsDouble(),
                Prefab = v["prefab"].AsString(),
                Snap = !v["snap_to_existing_nodes"].IsNull && v["snap_to_existing_nodes"].AsBool()
            };
        }

        public static BulldozeReq Bulldoze(JsonValue v)
        {
            return new BulldozeReq { TargetType = v["target_type"].AsString(), Id = (uint)v["id"].AsDouble() };
        }

        public static UpgradeRoadReq UpgradeRoad(JsonValue v)
        {
            return new UpgradeRoadReq { SegmentId = (uint)v["segment_id"].AsDouble(), Prefab = v["prefab"].AsString() };
        }

        public static SetZoneReq SetZone(JsonValue v)
        {
            var r = v["rect"];
            return new SetZoneReq
            {
                MinX = (float)r["min_x"].AsDouble(), MinZ = (float)r["min_z"].AsDouble(),
                MaxX = (float)r["max_x"].AsDouble(), MaxZ = (float)r["max_z"].AsDouble(),
                ZoneType = v["zone_type"].AsString()
            };
        }

        public static ClockReq Clock(JsonValue v)
        {
            return new ClockReq
            {
                Op = v["op"].AsString(),
                Ticks = v["ticks"].IsNull ? 0 : (int)v["ticks"].AsDouble(),
                Speed = v["speed"].IsNull ? 0 : (int)v["speed"].AsDouble()
            };
        }

        public static LoadSaveReq LoadSave(JsonValue v) { return new LoadSaveReq { SaveName = v["save_name"].AsString() }; }
    }
}
```

Add `RequestParse.cs` to both csproj Compile lists.

- [ ] **Step 3: Build + run**

Run: `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe ; echo "exit=$?"`
Expected: `20 passed, 0 failed`, exit 0.

- [ ] **Step 4: Commit**

```bash
git add mod/src/json/RequestParse.cs mod/test/RequestParseTests.cs mod/test/TestRunner.cs mod/test/Tests.csproj mod/SkylineBenchMod.csproj
git commit -m "feat(mod): add pure request parsers with tests"
```

---

## Phase C — GameBridge (game-coupled) + error mapping + routing

These touch CS1 managers and compile against the local assemblies. They are verified at runtime in Phase D. Use the pinned signatures. Each method runs inside `SimThread.Run` (sim-thread safety; drains-while-paused per 2a). After each task, build with `xbuild` to catch API mismatches; fix against `ikdasm` where flagged.

### Task C1: ErrorCode + prefab helper

**Files:**
- Create: `mod/src/bridge/ErrorCode.cs`

- [ ] **Step 1: Implement** `mod/src/bridge/ErrorCode.cs`:

```csharp
using ICities;

namespace SkylineBench.Bridge
{
    /// <summary>Normalized action failure reasons (spec §5). HTTP layer returns these
    /// at HTTP 200 with {ok:false,reason}.</summary>
    public static class ErrorCode
    {
        public const string Collision = "COLLISION";
        public const string InsufficientFunds = "INSUFFICIENT_FUNDS";
        public const string OutOfBounds = "OUT_OF_BOUNDS";
        public const string InvalidPrefab = "INVALID_PREFAB";
        public const string SegmentTooLong = "SEGMENT_TOO_LONG";
        public const string InvalidArgs = "INVALID_ARGS";
        public const string Unknown = "UNKNOWN";
    }

    public static class Prefabs
    {
        /// <summary>Find a NetInfo road prefab by exact name (e.g. "Basic Road"). null if absent.</summary>
        public static NetInfo FindRoad(string name)
        {
            int count = PrefabCollection<NetInfo>.PrefabCount();
            for (uint i = 0; i < count; i++)
            {
                var p = PrefabCollection<NetInfo>.GetPrefab(i);
                if (p != null && p.name == name) return p;
            }
            return null;
        }

        public static System.Collections.Generic.List<string> RoadNames()
        {
            var list = new System.Collections.Generic.List<string>();
            int count = PrefabCollection<NetInfo>.PrefabCount();
            for (uint i = 0; i < count; i++)
            {
                var p = PrefabCollection<NetInfo>.GetPrefab(i);
                // Roads have a vehicle lane of service Road; filter to those whose class service is Road.
                if (p != null && p.name != null && p.m_class != null && p.m_class.m_service == ItemClass.Service.Road)
                    list.Add(p.name);
            }
            return list;
        }
    }
}
```

> `ItemClass.Service.Road` filters road prefabs from the 331 (excludes rail/metro/pedestrian/canal). Confirm `m_class.m_service` compiles (it does per the API reference); adjust if needed.

- [ ] **Step 2: Add to csproj, build, commit**

Add `<Compile Include="src\bridge\ErrorCode.cs" />`. `cd mod && xbuild /p:Configuration=Release SkylineBenchMod.csproj` → 0 errors.
```bash
git add mod/src/bridge/ErrorCode.cs mod/SkylineBenchMod.csproj && git commit -m "feat(mod): add normalized error codes + prefab lookup"
```

### Task C2: GameReads — network, buildings, zones, metrics

**Files:**
- Create: `mod/src/bridge/GameReads.cs`

- [ ] **Step 1: Implement** `mod/src/bridge/GameReads.cs`. Each public method wraps a sim-thread snapshot via `SimThread.Run` (5000 ms timeout) and returns a DTO.

```csharp
using System.Collections.Generic;
using ColossalFramework;
using UnityEngine;
using SkylineBench.Dto;

namespace SkylineBench.Bridge
{
    public static class GameReads
    {
        private const int TimeoutMs = 5000;

        public static NetworkDto Network()
        {
            return SimThread.Run<NetworkDto>(delegate
            {
                var dto = new NetworkDto();
                var nm = Singleton<NetManager>.instance;
                for (uint i = 0; i < nm.m_nodes.m_buffer.Length; i++)
                {
                    var n = nm.m_nodes.m_buffer[i];
                    if ((n.m_flags & NetNode.Flags.Created) == NetNode.Flags.None) continue;
                    dto.Nodes.Add(new NodeDto { Id = i, X = n.m_position.x, Y = n.m_position.y, Z = n.m_position.z });
                }
                for (uint i = 0; i < nm.m_segments.m_buffer.Length; i++)
                {
                    var s = nm.m_segments.m_buffer[i];
                    if ((s.m_flags & NetSegment.Flags.Created) == NetSegment.Flags.None) continue;
                    var info = s.Info;
                    dto.Segments.Add(new SegmentDto
                    {
                        Id = i, StartNode = s.m_startNode, EndNode = s.m_endNode,
                        Prefab = info != null ? info.name : "",
                        Lanes = (byte)(info != null && info.m_lanes != null ? info.m_lanes.Length : 0),
                        Length = s.m_averageLength
                    });
                }
                return dto;
            }, TimeoutMs);
        }

        public static BuildingsDto Buildings()
        {
            return SimThread.Run<BuildingsDto>(delegate
            {
                var dto = new BuildingsDto();
                var bm = Singleton<BuildingManager>.instance;
                for (uint i = 0; i < bm.m_buildings.m_buffer.Length; i++)
                {
                    var b = bm.m_buildings.m_buffer[i];
                    if ((b.m_flags & Building.Flags.Created) == Building.Flags.None) continue;
                    var info = b.Info;
                    dto.Buildings.Add(new BuildingDto
                    {
                        Id = i, Prefab = info != null ? info.name : "",
                        Category = Category(info),
                        X = b.m_position.x, Y = b.m_position.y, Z = b.m_position.z,
                        FootprintWidth = info != null ? info.m_cellWidth * 8f : 0f,
                        FootprintLength = info != null ? info.m_cellLength * 8f : 0f,
                        Level = (byte)b.m_level
                    });
                }
                return dto;
            }, TimeoutMs);
        }

        private static string Category(BuildingInfo info)
        {
            if (info == null || info.m_class == null) return "other";
            switch (info.m_class.m_service)
            {
                case ItemClass.Service.Residential: return "residential";
                case ItemClass.Service.Commercial: return "commercial";
                case ItemClass.Service.Industrial: return "industrial";
                case ItemClass.Service.Office: return "office";
                default: return "service";
            }
        }

        public static MetricsDto Metrics()
        {
            return SimThread.Run<MetricsDto>(delegate
            {
                var dto = new MetricsDto();
                var sm = Singleton<SimulationManager>.instance;
                dto.Tick = sm.m_currentTickIndex;

                var vm = Singleton<VehicleManager>.instance;
                dto.ActiveVehicles = vm.cityVehicleCount;
                dto.FlowPercent = vm.m_maxTrafficFlow == 0u ? 100f : (vm.m_lastTrafficFlow * 100f / vm.m_maxTrafficFlow);

                var nm = Singleton<NetManager>.instance;
                for (uint i = 0; i < nm.m_segments.m_buffer.Length; i++)
                {
                    var s = nm.m_segments.m_buffer[i];
                    if ((s.m_flags & NetSegment.Flags.Created) == NetSegment.Flags.None) continue;
                    dto.SegmentLoads.Add(new SegmentLoadDto { SegmentId = i, Density = s.m_trafficDensity / 255f });
                }

                var em = Singleton<EconomyManager>.instance;
                dto.Funds = em.LastCashAmount;
                long income, expenses;
                em.GetIncomeAndExpenses(ItemClass.Service.None, ItemClass.SubService.None, ItemClass.Level.None, out income, out expenses);
                dto.WeeklyIncome = income;
                dto.WeeklyExpenses = expenses + em.GetLoanExpenses() + em.GetPolicyExpenses();
                dto.Balance = income - dto.WeeklyExpenses;

                var zm = Singleton<ZoneManager>.instance;
                dto.ResidentialDemand = (byte)Mathf.Clamp(zm.m_actualResidentialDemand, 0, 100);
                dto.CommercialDemand = (byte)Mathf.Clamp(zm.m_actualCommercialDemand, 0, 100);
                dto.WorkplaceDemand = (byte)Mathf.Clamp(zm.m_actualWorkplaceDemand, 0, 100);

                var dm = Singleton<DistrictManager>.instance;
                dto.Population = dm.m_districts.m_buffer[0].m_populationData.m_finalCount;
                dto.Employed = 0; // employment not directly exposed; left 0 (see DISCOVERY follow-up)
                dto.Happiness = (byte)Mathf.Clamp((int)dm.m_districts.m_buffer[0].m_finalHappiness, 0, 100);
                return dto;
            }, TimeoutMs);
        }

        public static ZonesDto Zones()
        {
            return SimThread.Run<ZonesDto>(delegate
            {
                // Zone reading decodes ZoneBlock.m_zone1/m_zone2 nibbles. CONFIRM the exact
                // bit layout with ikdasm (ZoneBlock.GetZone / the nibble math) in this task before
                // finalizing; the structure below is the standard 4-col x 8-row, 4-bit-per-cell layout.
                var dto = new ZonesDto();
                var zm = Singleton<ZoneManager>.instance;
                for (int b = 0; b < zm.m_blocks.m_buffer.Length; b++)
                {
                    var block = zm.m_blocks.m_buffer[b];
                    if ((block.m_flags & 0x00000001u) == 0u) continue; // Created bit; confirm ZoneBlock.Flags.Created value via ikdasm
                    DecodeBlock(block, dto);
                }
                return dto;
            }, TimeoutMs);
        }

        private static void DecodeBlock(ZoneBlock block, ZonesDto dto)
        {
            // Each block: rows along m_angle direction; 4 columns. Cell zone = 4-bit nibble in m_zone1 (rows 0-7) / m_zone2 (rows 8-15).
            // Cell world position derived from m_position + row/col * 8m along the block's basis vectors.
            Vector3 pos = block.m_position;
            float a = block.m_angle;
            Vector3 right = new Vector3(Mathf.Cos(a), 0f, Mathf.Sin(a));
            Vector3 forward = new Vector3(-Mathf.Sin(a), 0f, Mathf.Cos(a));
            int rows = block.RowCount;
            for (int row = 0; row < rows && row < 8; row++)
            {
                ulong zoneBits = block.m_zone1;
                for (int col = 0; col < 4; col++)
                {
                    int shift = (row * 4 + col) * 4; // CONFIRM packing order with ikdasm
                    int z = (int)((zoneBits >> shift) & 0xFUL);
                    string zt = ZoneTypeName((ItemClass.Zone)z);
                    if (zt == null) continue;
                    Vector3 cell = pos + right * ((col - 1.5f) * 8f) + forward * ((row + 0.5f) * 8f);
                    dto.Cells.Add(new ZoneCellDto { X = cell.x, Z = cell.z, ZoneType = zt });
                }
            }
        }

        private static string ZoneTypeName(ItemClass.Zone z)
        {
            switch (z)
            {
                case ItemClass.Zone.ResidentialLow: return "residential_low";
                case ItemClass.Zone.ResidentialHigh: return "residential_high";
                case ItemClass.Zone.CommercialLow: return "commercial_low";
                case ItemClass.Zone.CommercialHigh: return "commercial_high";
                case ItemClass.Zone.Industrial: return "industrial";
                case ItemClass.Zone.Office: return "office";
                default: return null; // Unzoned / Distant / None
            }
        }
    }
}
```

> **ikdasm confirm in this task** (the assemblies are local): (a) `ZoneBlock.Flags.Created` value and whether a `ZoneBlock.GetZone(int x,int z)` helper exists (prefer it over hand-decoding nibbles — `monodis Assembly-CSharp.dll | grep "ZoneBlock::GetZone"`); (b) `District.m_finalHappiness` field name; (c) `BuildingInfo.m_cellWidth`/`m_cellLength`. If `ZoneBlock.GetZone` exists, REPLACE `DecodeBlock` with calls to it (simpler + correct). Adjust and note what you found.

- [ ] **Step 2: Add to csproj, build, commit**

Add `<Compile Include="src\bridge\GameReads.cs" />`. Build `cd mod && xbuild /p:Configuration=Release SkylineBenchMod.csproj` → fix any field-name mismatches via ikdasm, 0 errors.
```bash
git add mod/src/bridge/GameReads.cs mod/SkylineBenchMod.csproj && git commit -m "feat(mod): add GameBridge read methods (network/buildings/zones/metrics)"
```

### Task C3: GameActions — build/bulldoze/upgrade/set-zone/clock/load-save

**Files:**
- Create: `mod/src/bridge/GameActions.cs`

- [ ] **Step 1: Implement** `mod/src/bridge/GameActions.cs`:

```csharp
using ColossalFramework;
using ColossalFramework.Math;
using UnityEngine;
using SkylineBench.Dto;
using SkylineBench.Json;

namespace SkylineBench.Bridge
{
    public static class GameActions
    {
        private const int TimeoutMs = 8000;
        private const float SnapToleranceM = 8f;
        private const float MaxSegmentLengthM = 200f; // working value; tune from DISCOVERY/spec

        public static ActionResultDto BuildRoad(BuildRoadReq req)
        {
            var prefab = Prefabs.FindRoad(req.Prefab);
            if (prefab == null) return ActionResultDto.Fail(ErrorCode.InvalidPrefab);
            var startPos = new Vector3(req.StartX, req.StartY, req.StartZ);
            var endPos = new Vector3(req.EndX, req.EndY, req.EndZ);
            float len = VectorUtils.LengthXZ(endPos - startPos);
            if (len < 0.001f) return ActionResultDto.Fail(ErrorCode.InvalidArgs);
            if (len > MaxSegmentLengthM) return ActionResultDto.Fail(ErrorCode.SegmentTooLong);

            return SimThread.Run<ActionResultDto>(delegate
            {
                var nm = Singleton<NetManager>.instance;
                var sm = Singleton<SimulationManager>.instance;
                var rand = new Randomizer(sm.m_currentBuildIndex);
                var result = new ActionResultDto { Ok = true };

                ushort startId, endId;
                if (!ResolveNode(nm, startPos, prefab, req.Snap, ref rand, sm, out startId, result, true)) return ActionResultDto.Fail(ErrorCode.Unknown);
                if (!ResolveNode(nm, endPos, prefab, req.Snap, ref rand, sm, out endId, result, false)) return ActionResultDto.Fail(ErrorCode.Unknown);

                Vector3 dir = VectorUtils.NormalizeXZ(endPos - startPos);
                ushort segId;
                bool ok = nm.CreateSegment(out segId, ref rand, prefab, startId, endId, dir, -dir,
                    sm.m_currentBuildIndex, sm.m_currentBuildIndex, false);
                if (!ok) return ActionResultDto.Fail(ErrorCode.Collision);
                sm.m_currentBuildIndex += 2u;
                result.CreatedSegments.Add(segId);
                return result;
            }, TimeoutMs);
        }

        private static bool ResolveNode(NetManager nm, Vector3 p, NetInfo prefab, bool snap, ref Randomizer rand,
            SimulationManager sm, out ushort id, ActionResultDto result, bool isStart)
        {
            id = 0;
            if (snap)
            {
                ushort near = NearestNode(nm, p, SnapToleranceM);
                if (near != 0) { id = near; result.SnappedNodes.Add(near); return true; }
            }
            if (!nm.CreateNode(out id, ref rand, prefab, p, sm.m_currentBuildIndex)) return false;
            result.CreatedNodes.Add(id);
            return true;
        }

        private static ushort NearestNode(NetManager nm, Vector3 p, float tol)
        {
            ushort best = 0; float bestD = tol;
            for (uint i = 1; i < nm.m_nodes.m_buffer.Length; i++)
            {
                var n = nm.m_nodes.m_buffer[i];
                if ((n.m_flags & NetNode.Flags.Created) == NetNode.Flags.None) continue;
                float d = VectorUtils.LengthXZ(n.m_position - p);
                if (d <= bestD) { bestD = d; best = (ushort)i; }
            }
            return best;
        }

        public static ActionResultDto Bulldoze(BulldozeReq req)
        {
            return SimThread.Run<ActionResultDto>(delegate
            {
                var nm = Singleton<NetManager>.instance;
                switch (req.TargetType)
                {
                    case "segment": nm.ReleaseSegment((ushort)req.Id, false); break;
                    case "node": nm.ReleaseNode((ushort)req.Id); break;
                    case "building": Singleton<BuildingManager>.instance.ReleaseBuilding((ushort)req.Id); break;
                    default: return ActionResultDto.Fail(ErrorCode.InvalidArgs);
                }
                var r = new ActionResultDto { Ok = true };
                r.Destroyed.Add(req.Id);
                return r;
            }, TimeoutMs);
        }

        public static ActionResultDto UpgradeRoad(UpgradeRoadReq req)
        {
            var prefab = Prefabs.FindRoad(req.Prefab);
            if (prefab == null) return ActionResultDto.Fail(ErrorCode.InvalidPrefab);
            return SimThread.Run<ActionResultDto>(delegate
            {
                var nm = Singleton<NetManager>.instance;
                var s = nm.m_segments.m_buffer[req.SegmentId];
                if ((s.m_flags & NetSegment.Flags.Created) == NetSegment.Flags.None) return ActionResultDto.Fail(ErrorCode.InvalidArgs);
                ushort startN = s.m_startNode, endN = s.m_endNode;
                Vector3 sd = s.m_startDirection, ed = s.m_endDirection;
                var sm = Singleton<SimulationManager>.instance;
                var rand = new Randomizer(sm.m_currentBuildIndex);
                nm.ReleaseSegment((ushort)req.SegmentId, true); // keep nodes
                ushort segId;
                bool ok = nm.CreateSegment(out segId, ref rand, prefab, startN, endN, sd, ed,
                    sm.m_currentBuildIndex, sm.m_currentBuildIndex, false);
                if (!ok) return ActionResultDto.Fail(ErrorCode.Collision);
                sm.m_currentBuildIndex += 2u;
                var r = new ActionResultDto { Ok = true };
                r.CreatedSegments.Add(segId); r.Destroyed.Add(req.SegmentId);
                return r;
            }, TimeoutMs);
        }

        public static ActionResultDto SetZone(SetZoneReq req)
        {
            // Writing zones manipulates ZoneBlock cells. CONFIRM the write API with ikdasm
            // (ZoneBlock.SetZone / zone bit manipulation) before finalizing — see Task C-zones note.
            ItemClass.Zone zone = ParseZone(req.ZoneType);
            if (zone == ItemClass.Zone.None) return ActionResultDto.Fail(ErrorCode.InvalidArgs);
            return SimThread.Run<ActionResultDto>(delegate
            {
                // Implementation finalized against the confirmed ZoneBlock write API (Task C3 Step 2).
                ZoneWriter.SetZoneOverRect(req.MinX, req.MinZ, req.MaxX, req.MaxZ, zone);
                return new ActionResultDto { Ok = true };
            }, TimeoutMs);
        }

        private static ItemClass.Zone ParseZone(string z)
        {
            switch (z)
            {
                case "residential": case "residential_low": return ItemClass.Zone.ResidentialLow;
                case "residential_high": return ItemClass.Zone.ResidentialHigh;
                case "commercial": case "commercial_low": return ItemClass.Zone.CommercialLow;
                case "commercial_high": return ItemClass.Zone.CommercialHigh;
                case "industrial": return ItemClass.Zone.Industrial;
                case "office": return ItemClass.Zone.Office;
                default: return ItemClass.Zone.None;
            }
        }

        public static ActionResultDto Clock(ClockReq req)
        {
            var t = ModRuntime.Threading;
            if (t == null) return ActionResultDto.Fail(ErrorCode.Unknown);
            switch (req.Op)
            {
                case "pause": t.simulationPaused = true; break;
                case "resume": t.simulationPaused = false; break;
                case "set-speed": t.simulationSpeed = Mathf.Clamp(req.Speed, 1, 3); break;
                case "step": Step(t, req.Ticks); break;
                default: return ActionResultDto.Fail(ErrorCode.InvalidArgs);
            }
            return new ActionResultDto { Ok = true };
        }

        private static void Step(ICities.IThreading t, int ticks)
        {
            uint target = t.simulationTick + (uint)(ticks < 0 ? 0 : ticks);
            bool wasPaused = t.simulationPaused;
            t.simulationPaused = false;
            int guard = 0;
            while (t.simulationTick < target && guard < 600000) { System.Threading.Thread.Sleep(1); guard++; }
            t.simulationPaused = wasPaused ? true : t.simulationPaused;
            if (wasPaused) t.simulationPaused = true;
        }
    }
}
```

> Notes: `VectorUtils.LengthXZ`/`NormalizeXZ`, `Randomizer`, `Singleton<T>` are in `ColossalFramework`/`ColossalFramework.Math` (confirmed common API). `ReleaseBuilding`/`ReleaseNode` signatures — confirm via ikdasm. `Step` busy-waits on the sim thread advancing (the HTTP thread is not the sim thread, so sleeping here is fine; the sim runs on its own thread). The `ZoneWriter` is implemented in Step 2.

- [ ] **Step 2: Implement ZoneWriter against the confirmed API** — create `mod/src/bridge/ZoneWriter.cs`.

Run `export PATH="/opt/homebrew/bin:$PATH"; monodis "$MANAGED/Assembly-CSharp.dll" | grep -iE "ZoneBlock::(SetZone|GetZone|CalculateBlock)|ZoneManager::CreateBlock"` (set `$MANAGED` to the game Managed path) to find the real read/write helpers. Implement `ZoneWriter.SetZoneOverRect(minX,minZ,maxX,maxZ, ItemClass.Zone)` using the confirmed `ZoneBlock`/`ZoneManager` API to set cell zones over the rect and mark blocks updated. Because the exact zone-write API is the single fiddliest internal, **this sub-step is where you pin it from the disassembly and the `GameReads.DecodeBlock` read path; keep both consistent.** If a clean per-cell setter isn't available, the minimum viable implementation flips the relevant nibbles in the covering blocks' `m_zone1`/`m_zone2` and calls the block's update/refresh method. Document exactly what you implemented.

- [ ] **Step 3: Add to csproj, build, commit**

Add `<Compile Include="src\bridge\GameActions.cs" />` and `<Compile Include="src\bridge\ZoneWriter.cs" />`. Build → 0 errors.
```bash
git add mod/src/bridge/GameActions.cs mod/src/bridge/ZoneWriter.cs mod/SkylineBenchMod.csproj && git commit -m "feat(mod): add GameBridge action methods (build/bulldoze/upgrade/zone/clock)"
```

### Task C4: load-save (reset_scenario)

**Files:**
- Create: `mod/src/bridge/SaveLoader.cs`

- [ ] **Step 1: Implement** `mod/src/bridge/SaveLoader.cs` using `LoadingManager.LoadLevel`.

Run `monodis "$MANAGED/Assembly-CSharp.dll" | grep -iE "LoadingManager::LoadLevel|PackageManager|Package/Asset"` and the saves-package lookup to find how a save name maps to a `Package.Asset` (saves live in the saves package; the menu's load enumerates `PackageManager.FilterAssets(UserAssetType.SaveGame)`). Implement:

```csharp
using ColossalFramework.Packaging;
using ColossalFramework;
using SkylineBench.Dto;

namespace SkylineBench.Bridge
{
    public static class SaveLoader
    {
        public static ActionResultDto Load(string saveName)
        {
            Package.Asset target = null;
            foreach (var asset in PackageManager.FilterAssets(UserAssetType.SaveGame))
                if (asset != null && asset.name == saveName) { target = asset; break; }
            if (target == null) return ActionResultDto.Fail(ErrorCode.InvalidArgs);

            // LoadLevel tears down + reloads on the main thread; invoke on the sim/main thread.
            SimThread.Run(delegate
            {
                Singleton<LoadingManager>.instance.LoadLevel(target, "Game", "InGame", new SimulationMetaData(), false);
            }, 8000);
            return new ActionResultDto { Ok = true };
        }
    }
}
```

> **CONFIRM with ikdasm/monodis**: `PackageManager.FilterAssets(UserAssetType.SaveGame)`, `UserAssetType.SaveGame`, and the exact `LoadLevel(Asset, string, string, SimulationMetaData, bool)` overload + the right scene name args ("Game"/"InGame"). Mid-session load is heavyweight (full reload) — **the manual verify (Phase D) confirms it actually works mid-session**; if it doesn't, document the constraint in DISCOVERY.md and return a clear `UNKNOWN`/limitation rather than faking it.

- [ ] **Step 2: Add to csproj, build, commit**

Add `<Compile Include="src\bridge\SaveLoader.cs" />`. Build → 0 errors.
```bash
git add mod/src/bridge/SaveLoader.cs mod/SkylineBenchMod.csproj && git commit -m "feat(mod): add savegame loader for reset_scenario"
```

### Task C5: Wire handlers + routes

**Files:**
- Modify: `mod/src/http/Handlers.cs`, `mod/src/http/Router.cs`

- [ ] **Step 1: Add handlers** to `mod/src/http/Handlers.cs` (alongside the existing `Health`/`Probe`):

```csharp
        public static HttpReply Network() { return HttpReply.Json(200, Serialize.Network(GameReads.Network())); }
        public static HttpReply Buildings() { return HttpReply.Json(200, Serialize.Buildings(GameReads.Buildings())); }
        public static HttpReply Zones() { return HttpReply.Json(200, Serialize.Zones(GameReads.Zones())); }
        public static HttpReply Metrics() { return HttpReply.Json(200, Serialize.Metrics(GameReads.Metrics())); }
        public static HttpReply RoadTypes()
        {
            var w = new JsonWriter(); w.BeginObject().Name("road_types").BeginArray();
            foreach (var n in Prefabs.RoadNames()) w.Value(n);
            w.EndArray().EndObject(); return HttpReply.Json(200, w.ToString());
        }
        public static HttpReply ZoneTypes()
        {
            var w = new JsonWriter(); w.BeginObject().Name("zone_types").BeginArray();
            foreach (var z in new string[] { "residential", "residential_high", "commercial", "commercial_high", "industrial", "office" }) w.Value(z);
            w.EndArray().EndObject(); return HttpReply.Json(200, w.ToString());
        }
        public static HttpReply BuildRoad(string body) { return Act(Serialize.Action(GameActions.BuildRoad(RequestParse.BuildRoad(JsonReader.Parse(body))))); }
        public static HttpReply Bulldoze(string body) { return Act(Serialize.Action(GameActions.Bulldoze(RequestParse.Bulldoze(JsonReader.Parse(body))))); }
        public static HttpReply UpgradeRoad(string body) { return Act(Serialize.Action(GameActions.UpgradeRoad(RequestParse.UpgradeRoad(JsonReader.Parse(body))))); }
        public static HttpReply SetZone(string body) { return Act(Serialize.Action(GameActions.SetZone(RequestParse.SetZone(JsonReader.Parse(body))))); }
        public static HttpReply Clock(string body) { return Act(Serialize.Action(GameActions.Clock(RequestParse.Clock(JsonReader.Parse(body))))); }
        public static HttpReply LoadSave(string body) { return Act(Serialize.Action(SaveLoader.Load(RequestParse.LoadSave(JsonReader.Parse(body)).SaveName))); }
        private static HttpReply Act(string json) { return HttpReply.Json(200, json); }
```

Add the needed `using SkylineBench.Bridge;` if not present. **Action failures return HTTP 200** (the `ActionResultDto` carries `ok:false`), per spec §6.

- [ ] **Step 2: Add routes** to `mod/src/http/Router.cs` `Route` switch:

```csharp
                case "/network": return method == "GET" ? Handlers.Network() : MethodNotAllowed();
                case "/buildings": return method == "GET" ? Handlers.Buildings() : MethodNotAllowed();
                case "/zones": return method == "GET" ? Handlers.Zones() : MethodNotAllowed();
                case "/metrics": return method == "GET" ? Handlers.Metrics() : MethodNotAllowed();
                case "/road-types": return method == "GET" ? Handlers.RoadTypes() : MethodNotAllowed();
                case "/zone-types": return method == "GET" ? Handlers.ZoneTypes() : MethodNotAllowed();
                case "/action/build-road": return method == "POST" ? Handlers.BuildRoad(body) : MethodNotAllowed();
                case "/action/bulldoze": return method == "POST" ? Handlers.Bulldoze(body) : MethodNotAllowed();
                case "/action/upgrade-road": return method == "POST" ? Handlers.UpgradeRoad(body) : MethodNotAllowed();
                case "/action/set-zone": return method == "POST" ? Handlers.SetZone(body) : MethodNotAllowed();
                case "/clock": return method == "POST" ? Handlers.Clock(body) : MethodNotAllowed();
                case "/load-save": return method == "POST" ? Handlers.LoadSave(body) : MethodNotAllowed();
```

- [ ] **Step 3: Build + run pure tests (no regression) + commit**

Run: `cd mod && xbuild /p:Configuration=Release SkylineBenchMod.csproj` → 0 errors; `cd test && xbuild Tests.csproj && mono bin/Debug/Tests.exe` → `20 passed, 0 failed`.
```bash
git add mod/src/http/Handlers.cs mod/src/http/Router.cs && git commit -m "feat(mod): wire all contract endpoints into the router"
```

---

## Phase D — Manual end-to-end verification (HUMAN) + spec sync

### Task D1: Human end-to-end loop against the real game

> Requires the human to run Cities: Skylines.

- [ ] **Step 1: Rebuild + install** the mod: `cd mod && ./build.sh`. Restart CS1 if it was running; ensure **SkylineBench Bridge** is enabled; load a city.

- [ ] **Step 2: Start the broker against the real mod** (in a terminal):
`cd broker && cargo run --release -- serve --mod-url http://127.0.0.1:8787`
(or test the mod directly with curl — see below.)

- [ ] **Step 3: Run the loop directly via curl** (fastest to validate the mod):
```bash
H=http://127.0.0.1:8787
curl -s $H/metrics | python3 -m json.tool | head -30
curl -s $H/road-types | python3 -c "import sys,json;print(len(json.load(sys.stdin)['road_types']),'road types')"
curl -s "$H/network" | python3 -c "import sys,json;d=json.load(sys.stdin);print('nodes',len(d['nodes']),'segments',len(d['segments']))"
# build a road between two points (use coords from /network or an open area), then re-read
curl -s -XPOST $H/action/build-road -d '{"start":{"x":0,"y":0,"z":0},"end":{"x":96,"y":0,"z":0},"prefab":"Basic Road","snap_to_existing_nodes":true}'
curl -s -XPOST $H/clock -d '{"op":"step","ticks":256}'
curl -s $H/metrics | python3 -c "import sys,json;print('flow',json.load(sys.stdin)['traffic']['flow_percent'])"
```
Expected: metrics returns the full shape; road-types lists ~dozens; build-road returns `{"ok":true,...created_segments...}`; the segment appears in `/network`; clock step advances; metrics still returns. Verify `bulldoze`, `upgrade-road`, `set-zone`, and `load-save` similarly.

- [ ] **Step 4 (acceptance):** confirm the full **observe → build → step → observe** loop works and a built road is visible in-game. Note any endpoint that misbehaves; fix in the relevant Phase-C file (the assemblies are local, so iterate build→install→retest).

- [ ] **Step 5: Capture results** — paste the curl outputs; record any endpoint that needed a fix and what the real API was.

### Task D2: Spec & DISCOVERY sync

**Files:**
- Modify: `docs/superpowers/specs/2026-06-08-skylinebench-phase1-design.md`, `docs/superpowers/specs/2026-06-09-skylinebench-mod-design.md`, `mod/DISCOVERY.md`

- [ ] **Step 1: Fold confirmed facts back into the specs:**
  - Phase-1 spec §6: PopulationMetrics demands are residential/commercial/**workplace** (3, not 4).
  - Phase-1 spec §5: record the real `MAX_SEGMENT_LENGTH_M` and playable extent if measured during D1 (else keep the working values and note them as still-working).
  - `DISCOVERY.md`: append the resolved zone-write API, `District.m_finalHappiness`/employment availability, `LoadLevel` save-asset lookup, and any endpoint fixes from D1.

- [ ] **Step 2: Commit**
```bash
git add docs/ mod/DISCOVERY.md
git commit -m "docs: sync specs + DISCOVERY with confirmed 2b findings"
```

---

## Done criteria

- Broker: PopulationMetrics aligned to R/C/Workplace; `cargo test` + clippy green.
- Mod: pure serializer/parser tests pass (`20 passed`); the mod DLL builds against the assemblies with all endpoints wired.
- In-game (D1): the full observe → build → step → observe loop works against the real game through the mod (and the broker), with bulldoze/upgrade/set-zone/load-save verified.
- Specs + DISCOVERY synced.

**This completes Phase 1**: a real, playable CS1 ↔ broker ↔ agent loop. Remaining deferred (separate small plan): the Rust `setup`/`doctor` automation + an automated smoke test (spec §10), and Phase 2 (the traffic benchmark).

## Notes / residual risks
- The **zone read/write nibble layout** and the **save-asset lookup** are the two fiddliest internals — both are flagged to pin via `ikdasm`/`monodis` during their tasks (assemblies are local) and confirmed behaviorally in D1.
- **Employment** isn't cleanly exposed; `MetricsDto.Employed` is 0 for now (documented), to be resolved in a follow-up if Phase 2 needs it.
- `Step`'s busy-wait advances real sim ticks; in step-mode benchmarking this is the intended "advance N ticks then re-pause" behavior.
