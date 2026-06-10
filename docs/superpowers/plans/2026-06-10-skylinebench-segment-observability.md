# Segment Observability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose per-segment one-way/travel-direction and speed limit, fix the saturating density metric, restrict network reads to actual roads (no pipes/power lines), make `upgrade_road`'s ID renumbering explicit, give `observe_area` bounds, and add a `query_segments` "worst-N congestion" search tool.

**Architecture:** The mod (`GameReads.cs`) computes the new fields from `NetInfo` lane flags + `NetSegment.Flags.Invert` via a new pure `Direction` helper, filters to `ItemClass.Service.Road`, and normalizes density by 100 (the game caps `m_trafficDensity` at 100, so the old `/255` pinned everything at 0.39). The broker's `NetSegment` contract gains the fields with serde defaults; `query_segments` is a pure broker-side join of `/network` + `/metrics`. The mock mirrors the new fields so everything is testable without the game.

**Tech Stack:** C# (.NET 3.5 / Mono, Cities: Skylines modding API), Rust (serde, rmcp, axum mock).

**Evidence this matters:** run `20260609-210135` died because the agent upgraded the spine to one-way `Highway` prefabs and silently killed the southbound carriageway — direction is invisible in today's contract. Dozens of segments sat indistinguishably at density 0.39 (the `/255` cap), and stale segment IDs after `upgrade_road` crashed the agent's analysis scripts twice.

---

### Task 1: Pure direction helper in the mod

**Files:**
- Create: `mod/src/bridge/Direction.cs`
- Create: `mod/test/DirectionTests.cs`
- Modify: `mod/SkylineBenchMod.csproj` (add compile include)
- Modify: `mod/test/Tests.csproj` (add compile includes)
- Modify: `mod/test/TestRunner.cs` (register)

- [ ] **Step 1: Write the failing tests**

Create `mod/test/DirectionTests.cs`:

```csharp
using System;
using System.Collections.Generic;
using SkylineBench.Bridge;

namespace SkylineBench.Tests
{
    public static class DirectionTests
    {
        public static void Register(List<KeyValuePair<string, Action>> tests)
        {
            tests.Add(new KeyValuePair<string, Action>("direction: two-way is both", TwoWay));
            tests.Add(new KeyValuePair<string, Action>("direction: one-way follows lanes and invert", OneWay));
            tests.Add(new KeyValuePair<string, Action>("direction: laneless is both", Laneless));
        }

        static void TwoWay()
        {
            Assert.True(!Direction.IsOneWay(true, true), "fwd+bwd lanes is two-way");
            Assert.Equal(Direction.Both, Direction.Travel(true, true, false));
            Assert.Equal(Direction.Both, Direction.Travel(true, true, true));
        }

        static void OneWay()
        {
            Assert.True(Direction.IsOneWay(true, false), "fwd-only is one-way");
            Assert.Equal(Direction.StartToEnd, Direction.Travel(true, false, false));
            Assert.Equal(Direction.EndToStart, Direction.Travel(true, false, true));
            Assert.Equal(Direction.EndToStart, Direction.Travel(false, true, false));
            Assert.Equal(Direction.StartToEnd, Direction.Travel(false, true, true));
        }

        static void Laneless()
        {
            Assert.True(!Direction.IsOneWay(false, false), "no vehicle lanes is not one-way");
            Assert.Equal(Direction.Both, Direction.Travel(false, false, false));
        }
    }
}
```

`Assert.Equal(string, string)` already exists in `mod/test/TestRunner.cs`.

Register in `mod/test/TestRunner.cs` after `RequestParseTests.Register(tests);`:

```csharp
            DirectionTests.Register(tests);
```

Add to `mod/test/Tests.csproj` in the source `<ItemGroup>` (next to the other `..\src` includes):

```xml
    <Compile Include="..\src\bridge\Direction.cs"><Link>src\Direction.cs</Link></Compile>
```
and next to the other test files:
```xml
    <Compile Include="DirectionTests.cs" />
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `(cd mod/test && msbuild Tests.csproj && mono bin/Debug/Tests.exe)`
Expected: build FAILS — `Direction` does not exist.

- [ ] **Step 3: Implement Direction**

Create `mod/src/bridge/Direction.cs`:

```csharp
namespace SkylineBench.Bridge
{
    /// <summary>Travel direction of a road segment, derived from the prefab's
    /// vehicle-lane flags and the segment's Invert flag. A one-way prefab's lanes
    /// all run "forward" (start→end); Invert means the segment was drawn opposite
    /// to the prefab orientation, flipping the effective direction. Pure (no game
    /// references) so it is unit-testable in the no-game harness.</summary>
    public static class Direction
    {
        public const string Both = "both";
        public const string StartToEnd = "start_to_end";
        public const string EndToStart = "end_to_start";

        public static bool IsOneWay(bool hasForwardLanes, bool hasBackwardLanes)
        {
            return hasForwardLanes != hasBackwardLanes;
        }

        public static string Travel(bool hasForwardLanes, bool hasBackwardLanes, bool inverted)
        {
            if (!IsOneWay(hasForwardLanes, hasBackwardLanes)) return Both;
            return (hasForwardLanes != inverted) ? StartToEnd : EndToStart;
        }
    }
}
```

Add to `mod/SkylineBenchMod.csproj` in the `<ItemGroup>` of compile includes (next to the other `src\bridge` entries):

```xml
    <Compile Include="src\bridge\Direction.cs" />
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `(cd mod/test && msbuild Tests.csproj && mono bin/Debug/Tests.exe)`
Expected: all tests pass, including the 3 new `direction:` tests.

- [ ] **Step 5: Commit**

```bash
git add mod/src/bridge/Direction.cs mod/test/DirectionTests.cs mod/test/Tests.csproj mod/test/TestRunner.cs mod/SkylineBenchMod.csproj
git commit -m "feat(mod): pure travel-direction helper from lane flags + invert"
```

---

### Task 2: Mod wire format — new segment fields, road-only filter, density fix

**Files:**
- Modify: `mod/src/dto/Dtos.cs:6` (SegmentDto)
- Modify: `mod/src/json/Serialize.cs:16-20` (Network)
- Modify: `mod/src/bridge/GameReads.cs:11-38` (Network), `GameReads.cs:91-97` (Metrics segment loads)
- Modify: `mod/test/SerializeTests.cs:21-29`

- [ ] **Step 1: Update the serializer test to the new wire shape (failing)**

In `mod/test/SerializeTests.cs`, replace the `Network()` test method with:

```csharp
        static void Network()
        {
            var net = new NetworkDto();
            net.Nodes.Add(new NodeDto { Id = 1, X = -50f, Y = 0f, Z = 10f });
            net.Segments.Add(new SegmentDto { Id = 7, StartNode = 1, EndNode = 2, Prefab = "Basic Road", Lanes = 2, Length = 100f, OneWay = true, TravelDirection = "start_to_end", SpeedLimit = 2f });
            Assert.Equal(
                "{\"nodes\":[{\"id\":1,\"x\":-50,\"y\":0,\"z\":10}],\"segments\":[{\"id\":7,\"start_node\":1,\"end_node\":2,\"prefab\":\"Basic Road\",\"lanes\":2,\"length\":100,\"one_way\":true,\"travel_direction\":\"start_to_end\",\"speed_limit\":2}]}",
                Serialize.Network(net));
        }
```

- [ ] **Step 2: Run to verify it fails**

Run: `(cd mod/test && msbuild Tests.csproj && mono bin/Debug/Tests.exe)`
Expected: build FAILS — `SegmentDto` has no `OneWay`.

- [ ] **Step 3: Extend the DTO and serializer**

In `mod/src/dto/Dtos.cs` replace the `SegmentDto` line with:

```csharp
    public struct SegmentDto { public uint Id; public uint StartNode; public uint EndNode; public string Prefab; public byte Lanes; public float Length; public bool OneWay; public string TravelDirection; public float SpeedLimit; }
```

In `mod/src/json/Serialize.cs`, in `Network`, replace the segment-writing loop body with:

```csharp
            foreach (var s in net.Segments)
                w.BeginObject().Name("id").Value((long)s.Id).Name("start_node").Value((long)s.StartNode).Name("end_node").Value((long)s.EndNode)
                    .Name("prefab").Value(s.Prefab).Name("lanes").Value((long)s.Lanes).Name("length").Value(s.Length)
                    .Name("one_way").Value(s.OneWay).Name("travel_direction").Value(s.TravelDirection).Name("speed_limit").Value(s.SpeedLimit).EndObject();
```

- [ ] **Step 4: Run to verify it passes**

Run: `(cd mod/test && msbuild Tests.csproj && mono bin/Debug/Tests.exe)`
Expected: PASS.

- [ ] **Step 5: Populate the fields in GameReads and filter to roads**

In `mod/src/bridge/GameReads.cs`, replace the `Network()` method with:

```csharp
        public static NetworkDto Network()
        {
            return SimThread.Run<NetworkDto>(delegate
            {
                var dto = new NetworkDto();
                var nm = Singleton<NetManager>.instance;
                var roadNodeIds = new System.Collections.Generic.HashSet<uint>();
                for (uint i = 0; i < nm.m_segments.m_buffer.Length; i++)
                {
                    var s = nm.m_segments.m_buffer[i];
                    if ((s.m_flags & NetSegment.Flags.Created) == NetSegment.Flags.None) continue;
                    var info = s.Info;
                    // Roads only: water pipes and power lines are also NetSegments
                    // and previously polluted the network dump and rendered map.
                    if (info == null || info.m_class == null || info.m_class.m_service != ItemClass.Service.Road) continue;
                    bool hasFwd = info.m_hasForwardVehicleLanes;
                    bool hasBwd = info.m_hasBackwardVehicleLanes;
                    bool inverted = (s.m_flags & NetSegment.Flags.Invert) != NetSegment.Flags.None;
                    dto.Segments.Add(new SegmentDto
                    {
                        Id = i, StartNode = s.m_startNode, EndNode = s.m_endNode,
                        Prefab = info.name,
                        Lanes = (byte)(info.m_lanes != null ? info.m_lanes.Length : 0),
                        Length = s.m_averageLength,
                        OneWay = Direction.IsOneWay(hasFwd, hasBwd),
                        TravelDirection = Direction.Travel(hasFwd, hasBwd, inverted),
                        SpeedLimit = info.m_averageVehicleLaneSpeed
                    });
                    roadNodeIds.Add(s.m_startNode);
                    roadNodeIds.Add(s.m_endNode);
                }
                for (uint i = 0; i < nm.m_nodes.m_buffer.Length; i++)
                {
                    var n = nm.m_nodes.m_buffer[i];
                    if ((n.m_flags & NetNode.Flags.Created) == NetNode.Flags.None) continue;
                    if (!roadNodeIds.Contains(i)) continue;
                    dto.Nodes.Add(new NodeDto { Id = i, X = n.m_position.x, Y = n.m_position.y, Z = n.m_position.z });
                }
                return dto;
            }, TimeoutMs);
        }
```

(`NetInfo.m_averageVehicleLaneSpeed` is the prefab's average vehicle-lane speed in game units, ~1.0 ≈ 50 km/h. If the Release build fails on that member name, use the explicit fallback inside the loop instead — compute it from the lanes array:

```csharp
                    float speedSum = 0f; int speedN = 0;
                    if (info.m_lanes != null)
                        for (int li = 0; li < info.m_lanes.Length; li++)
                            if (info.m_lanes[li] != null && info.m_lanes[li].m_laneType == NetInfo.LaneType.Vehicle)
                            { speedSum += info.m_lanes[li].m_speedLimit; speedN++; }
                    // then: SpeedLimit = speedN > 0 ? speedSum / speedN : 0f
```
)

In `GameReads.Metrics()`, replace the segment-loads loop with (road filter + the density cap fix — the game rolls `m_trafficDensity` up to a max of 100, so `/255` pinned everything at 0.39):

```csharp
                for (uint i = 0; i < nm.m_segments.m_buffer.Length; i++)
                {
                    var s = nm.m_segments.m_buffer[i];
                    if ((s.m_flags & NetSegment.Flags.Created) == NetSegment.Flags.None) continue;
                    var sInfo = s.Info;
                    if (sInfo == null || sInfo.m_class == null || sInfo.m_class.m_service != ItemClass.Service.Road) continue;
                    dto.SegmentLoads.Add(new SegmentLoadDto { SegmentId = i, Density = Mathf.Min(1f, s.m_trafficDensity / 100f) });
                }
```

- [ ] **Step 6: Build the mod against the game assemblies**

Run: `./mod/build.sh`
Expected: compiles and installs. If `m_averageVehicleLaneSpeed` is rejected, apply the lane-average fallback from Step 5 and rebuild.

- [ ] **Step 7: Run the no-game tests once more and commit**

Run: `(cd mod/test && msbuild Tests.csproj && mono bin/Debug/Tests.exe)`
Expected: PASS.

```bash
git add mod/src/dto/Dtos.cs mod/src/json/Serialize.cs mod/src/bridge/GameReads.cs mod/test/SerializeTests.cs
git commit -m "feat(mod): expose one-way/direction/speed per segment, road-only reads, un-saturated density"
```

---

### Task 3: Broker contract — new NetSegment fields everywhere

**Files:**
- Modify: `broker/src/contract.rs:35-43` (NetSegment)
- Modify: `broker/src/mock.rs` (build_road segment creation ~line 201, metrics density ~line 99-106)
- Modify: `broker/src/graph.rs:64-73` (test helper)
- Modify: `broker/src/render.rs:98-115` (test fixtures)
- Modify: `broker/examples/gen_golden.rs:31-46`

- [ ] **Step 1: Write the failing round-trip test**

Add to the `tests` module in `broker/src/contract.rs`:

```rust
    #[test]
    fn net_segment_defaults_direction_fields() {
        // Wire payload from an older mod without the new fields must default.
        let parsed: NetSegment = serde_json::from_str(
            "{\"id\":7,\"start_node\":1,\"end_node\":2,\"prefab\":\"road\",\"lanes\":2,\"length\":100.0}",
        )
        .unwrap();
        assert!(!parsed.one_way);
        assert_eq!(parsed.travel_direction, "both");
        assert_eq!(parsed.speed_limit, 0.0);

        let full: NetSegment = serde_json::from_str(
            "{\"id\":7,\"start_node\":1,\"end_node\":2,\"prefab\":\"hw\",\"lanes\":4,\"length\":100.0,\"one_way\":true,\"travel_direction\":\"end_to_start\",\"speed_limit\":2.0}",
        )
        .unwrap();
        assert!(full.one_way);
        assert_eq!(full.travel_direction, "end_to_start");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --manifest-path broker/Cargo.toml net_segment_defaults`
Expected: FAIL — unknown fields on `NetSegment`.

- [ ] **Step 3: Extend the contract struct**

In `broker/src/contract.rs` replace `NetSegment` with:

```rust
fn default_travel_direction() -> String {
    "both".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetSegment {
    pub id: u32,
    pub start_node: u32,
    pub end_node: u32,
    pub prefab: String,
    pub lanes: u8,
    pub length: f32,
    #[serde(default)]
    pub one_way: bool,
    /// "both" | "start_to_end" | "end_to_start"
    #[serde(default = "default_travel_direction")]
    pub travel_direction: String,
    /// Game speed units (~1.0 ≈ 50 km/h); 0.0 when unknown.
    #[serde(default)]
    pub speed_limit: f32,
}
```

- [ ] **Step 4: Fix every struct literal**

`cargo build` will now fail on missing fields. Apply these exact updates:

`broker/src/graph.rs` test helper `seg`:
```rust
    fn seg(id: u32, a: u32, b: u32) -> NetSegment {
        NetSegment {
            id,
            start_node: a,
            end_node: b,
            prefab: "road".into(),
            lanes: 2,
            length: 10.0,
            one_way: false,
            travel_direction: "both".into(),
            speed_limit: 1.0,
        }
    }
```

`broker/src/render.rs` `sample_network()` — add to BOTH segment literals:
```rust
                    one_way: false,
                    travel_direction: "both".into(),
                    speed_limit: 1.0,
```

`broker/examples/gen_golden.rs` — add the same three fields to both segment literals (this file must mirror `render.rs::sample_network` exactly; the golden PNG does not change because rendering ignores the new fields until the render plan lands).

`broker/src/mock.rs` `build_road` — replace the `c.segments.push(...)` block with:
```rust
    let seg_id = c.next_id;
    c.next_id += 1;
    let length = horizontal_distance(body.start, body.end);
    let (one_way, speed_limit) = match body.prefab.as_str() {
        "oneway" => (true, 1.2),
        "highway" => (true, 2.0),
        _ => (false, 1.0),
    };
    c.segments.push(NetSegment {
        id: seg_id,
        start_node: start_id,
        end_node: end_id,
        prefab: body.prefab,
        lanes: 2,
        length,
        one_way,
        travel_direction: if one_way { "start_to_end".into() } else { "both".into() },
        speed_limit,
    });
```

`broker/src/mock.rs` `metrics` — make per-segment density vary so sorting is testable; replace the `segment_loads` mapping with:
```rust
            segment_loads: c
                .segments
                .iter()
                .map(|sg| SegmentLoad {
                    segment_id: sg.id,
                    density: (sg.id % 10) as f32 / 10.0,
                })
                .collect(),
```

- [ ] **Step 5: Run the whole suite**

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: ALL PASS (golden render unchanged — render ignores the new fields for now).

- [ ] **Step 6: Commit**

```bash
git add broker/src/contract.rs broker/src/graph.rs broker/src/render.rs broker/src/mock.rs broker/examples/gen_golden.rs
git commit -m "feat(broker): one_way/travel_direction/speed_limit on the NetSegment contract"
```

---

### Task 4: observe_area bounds filter

**Files:**
- Modify: `broker/src/service.rs:32-44` (+ args struct, + tests)
- Modify: `broker/src/tools.rs:62-70`
- Modify: `broker/src/benchmark/server.rs:117-124`
- Modify: `broker/tests/broker_e2e.rs:74` (call site)

- [ ] **Step 1: Write the failing service test**

Add to `broker/src/service.rs` tests:

```rust
    #[tokio::test]
    async fn observe_area_filters_by_bounds() {
        let c = client().await;
        for (x0, x1) in [(0.0_f32, 50.0_f32), (1000.0, 1050.0)] {
            build_road(
                &c,
                BuildRoadArgs {
                    from: Position { x: x0, y: 0.0, z: 0.0 },
                    to: Position { x: x1, y: 0.0, z: 0.0 },
                    road_type: "road".into(),
                    snap: true,
                },
            )
            .await
            .unwrap();
        }
        let all = observe_area(&c, ObserveAreaArgs { bounds: None }).await.unwrap();
        assert_eq!(all["network"]["segments"].as_array().unwrap().len(), 2);

        let near = observe_area(
            &c,
            ObserveAreaArgs {
                bounds: Some(crate::contract::Bounds { min_x: -10.0, min_z: -10.0, max_x: 100.0, max_z: 10.0 }),
            },
        )
        .await
        .unwrap();
        assert_eq!(near["network"]["segments"].as_array().unwrap().len(), 1);
        assert_eq!(near["network"]["nodes"].as_array().unwrap().len(), 2);
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --manifest-path broker/Cargo.toml observe_area_filters`
Expected: FAIL — `observe_area` takes one argument / `ObserveAreaArgs` not found.

- [ ] **Step 3: Implement the bounds filter**

In `broker/src/service.rs`, replace `observe_area` with:

```rust
#[derive(Deserialize, schemars::JsonSchema)]
pub struct ObserveAreaArgs {
    /// Restrict the observation to this rectangle (world metres). Omit for the
    /// whole map. A segment is included when either endpoint is inside.
    #[serde(default)]
    pub bounds: Option<Bounds>,
}

pub async fn observe_area(
    client: &BridgeClient,
    args: ObserveAreaArgs,
) -> Result<Value, ServiceError> {
    let net = client.network().await?;
    let buildings = client.buildings().await?;
    let zones = client.zones().await?;
    let net = match args.bounds {
        None => net,
        Some(b) => {
            let inside = |x: f32, z: f32| {
                crate::geometry::in_bounds(Position { x, y: 0.0, z }, b)
            };
            let node_in: std::collections::HashMap<u32, bool> =
                net.nodes.iter().map(|n| (n.id, inside(n.x, n.z))).collect();
            let segments: Vec<_> = net
                .segments
                .into_iter()
                .filter(|s| {
                    node_in.get(&s.start_node).copied().unwrap_or(false)
                        || node_in.get(&s.end_node).copied().unwrap_or(false)
                })
                .collect();
            let kept: std::collections::HashSet<u32> = segments
                .iter()
                .flat_map(|s| [s.start_node, s.end_node])
                .collect();
            crate::contract::Network {
                nodes: net.nodes.into_iter().filter(|n| kept.contains(&n.id)).collect(),
                segments,
            }
        }
    };
    let buildings: Vec<_> = match args.bounds {
        None => buildings.buildings,
        Some(b) => buildings
            .buildings
            .into_iter()
            .filter(|bd| crate::geometry::in_bounds(Position { x: bd.x, y: 0.0, z: bd.z }, b))
            .collect(),
    };
    let zones: Vec<_> = match args.bounds {
        None => zones.cells,
        Some(b) => zones
            .cells
            .into_iter()
            .filter(|zc| crate::geometry::in_bounds(Position { x: zc.x, y: 0.0, z: zc.z }, b))
            .collect(),
    };
    let connectivity = build_connectivity(&net);
    Ok(json!({
        "network": net,
        "buildings": buildings,
        "zones": zones,
        "intersections": connectivity.intersections(),
        "dead_ends": connectivity.dead_ends(),
    }))
}
```

Update the wrappers:

`broker/src/tools.rs` — replace the `observe_area` tool with:
```rust
    #[tool(
        description = "Observe the playable area: road network, buildings, zones, intersections, dead ends. \
            Optional `bounds` restricts to a rectangle."
    )]
    async fn observe_area(
        &self,
        Parameters(args): Parameters<ObserveAreaArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match service::observe_area(&self.client, args).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }
```
and add `ObserveAreaArgs` to the `crate::service::{...}` import list.

`broker/src/benchmark/server.rs` — same change to its `observe_area` (keep `ensure_baseline` and `finish`), adding `ObserveAreaArgs` to its service import list:
```rust
    #[tool(description = "Observe the playable area: road network, buildings, zones, intersections, dead ends. \
        Optional `bounds` restricts to a rectangle.")]
    async fn observe_area(&self, Parameters(args): Parameters<ObserveAreaArgs>) -> Result<CallToolResult, ErrorData> {
        self.ensure_baseline().await;
        match service::observe_area(&self.client, args).await {
            Ok(v) => self.finish(v).await,
            Err(e) => Ok(tool_err(e)),
        }
    }
```

`broker/tests/broker_e2e.rs:74` — change the call to:
```rust
    let obs = service::observe_area(&client, service::ObserveAreaArgs { bounds: None }).await.unwrap();
```

Existing `service.rs` test call sites (`observe_area(&c)` in `build_road_succeeds_and_observe_sees_it`, `bulldoze_removes_a_segment`, `reset_scenario_clears_the_city`, `upgrade_road_changes_segment_type_over_the_wire`, `set_zoning_adds_a_zone_cell_over_the_wire`) become `observe_area(&c, ObserveAreaArgs { bounds: None })`.

- [ ] **Step 4: Run the whole suite**

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: ALL PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/service.rs broker/src/tools.rs broker/src/benchmark/server.rs broker/tests/broker_e2e.rs
git commit -m "feat(broker): observe_area takes optional bounds"
```

---

### Task 5: Explicit old→new ID mapping on upgrade_road

**Files:**
- Modify: `broker/src/service.rs:175-185` (+ test)
- Modify: `broker/src/tools.rs:153-164`, `broker/src/benchmark/server.rs:242-268` (descriptions only)

- [ ] **Step 1: Write the failing test**

Add to `broker/src/service.rs` tests:

```rust
    #[tokio::test]
    async fn upgrade_road_reports_replaced_ids() {
        let c = client().await;
        let built = build_road(
            &c,
            BuildRoadArgs {
                from: Position { x: 0.0, y: 0.0, z: 0.0 },
                to: Position { x: 50.0, y: 0.0, z: 0.0 },
                road_type: "road".into(),
                snap: true,
            },
        )
        .await
        .unwrap();
        let seg_id = built["created_segments"][0].as_u64().unwrap();
        let res = upgrade_road(
            &c,
            UpgradeRoadArgs { segment: seg_id as u32, road_type: "highway".into() },
        )
        .await
        .unwrap();
        assert_eq!(res["ok"], true);
        assert_eq!(res["replaced"]["old_segment_id"], seg_id);
        assert!(res["replaced"]["new_segment_id"].is_u64());
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --manifest-path broker/Cargo.toml upgrade_road_reports_replaced`
Expected: FAIL — no `replaced` field.

- [ ] **Step 3: Implement**

In `broker/src/service.rs`, replace the tail of `upgrade_road` (after the prefab check) with:

```rust
    let res = client.upgrade_road(args.segment, &args.road_type).await?;
    let new_id = res.created_segments.first().copied();
    let mut v = serde_json::to_value(res).unwrap();
    if let (Some(new_id), Value::Object(map)) = (new_id, &mut v) {
        map.insert(
            "replaced".into(),
            json!({ "old_segment_id": args.segment, "new_segment_id": new_id }),
        );
    }
    Ok(v)
```

Update both tool descriptions (`broker/src/tools.rs` and `broker/src/benchmark/server.rs`) to:

```rust
    #[tool(description = "Change an existing road segment's type. The segment is re-created \
        under a NEW id — `replaced` in the response maps old_segment_id to new_segment_id; \
        refresh any cached ids.")]
```

- [ ] **Step 4: Run the whole suite, then commit**

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: ALL PASS.

```bash
git add broker/src/service.rs broker/src/tools.rs broker/src/benchmark/server.rs
git commit -m "feat(broker): upgrade_road responses map old segment id to new"
```

---

### Task 6: query_segments tool

**Files:**
- Modify: `broker/src/service.rs` (new args struct + function + tests)
- Modify: `broker/src/tools.rs` (register; tool-count test → 13)
- Modify: `broker/src/benchmark/server.rs` (register; tool-count test → 13)

- [ ] **Step 1: Write the failing service tests**

Add to `broker/src/service.rs` tests:

```rust
    async fn build_three_roads(c: &BridgeClient) {
        // Mock ids increment per node/segment; densities derive from id % 10,
        // so three spaced roads get three distinct densities.
        for (x0, x1) in [(0.0_f32, 50.0_f32), (1000.0, 1050.0), (2000.0, 2050.0)] {
            build_road(
                c,
                BuildRoadArgs {
                    from: Position { x: x0, y: 0.0, z: 0.0 },
                    to: Position { x: x1, y: 0.0, z: 0.0 },
                    road_type: "road".into(),
                    snap: true,
                },
            )
            .await
            .unwrap();
        }
    }

    #[tokio::test]
    async fn query_segments_sorts_by_density_and_limits() {
        let c = client().await;
        build_three_roads(&c).await;
        let v = query_segments(
            &c,
            QuerySegmentsArgs { sort_by: None, limit: Some(2), min_density: None, bounds: None, prefab_contains: None },
        )
        .await
        .unwrap();
        let rows = v["segments"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(v["total_matching"], 3);
        let d0 = rows[0]["density"].as_f64().unwrap();
        let d1 = rows[1]["density"].as_f64().unwrap();
        assert!(d0 >= d1, "descending density: {d0} vs {d1}");
        assert!(rows[0]["midpoint"]["x"].is_number());
        assert!(rows[0]["travel_direction"].is_string());
    }

    #[tokio::test]
    async fn query_segments_filters_by_bounds_and_min_density() {
        let c = client().await;
        build_three_roads(&c).await;
        let v = query_segments(
            &c,
            QuerySegmentsArgs {
                sort_by: None,
                limit: None,
                min_density: None,
                bounds: Some(crate::contract::Bounds { min_x: -10.0, min_z: -10.0, max_x: 100.0, max_z: 10.0 }),
                prefab_contains: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(v["segments"].as_array().unwrap().len(), 1);

        let none = query_segments(
            &c,
            QuerySegmentsArgs { sort_by: None, limit: None, min_density: Some(0.95), bounds: None, prefab_contains: None },
        )
        .await
        .unwrap();
        assert_eq!(none["segments"].as_array().unwrap().len(), 0);
        assert_eq!(none["total_matching"], 0);
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --manifest-path broker/Cargo.toml query_segments`
Expected: FAIL — `query_segments` not found.

- [ ] **Step 3: Implement**

Add to `broker/src/service.rs`:

```rust
#[derive(Deserialize, schemars::JsonSchema)]
pub struct QuerySegmentsArgs {
    /// Sort key, descending: "density" (default), "length", or "speed_limit".
    #[serde(default)]
    pub sort_by: Option<String>,
    /// Max rows returned (default 20, capped at 200).
    #[serde(default)]
    pub limit: Option<usize>,
    /// Keep only segments at or above this density (0..1).
    #[serde(default)]
    pub min_density: Option<f32>,
    /// Keep only segments with an endpoint inside this rectangle.
    #[serde(default)]
    pub bounds: Option<Bounds>,
    /// Case-insensitive substring match on the prefab name.
    #[serde(default)]
    pub prefab_contains: Option<String>,
}

pub async fn query_segments(
    client: &BridgeClient,
    args: QuerySegmentsArgs,
) -> Result<Value, ServiceError> {
    let net = client.network().await?;
    let metrics = client.metrics().await?;
    let density: std::collections::HashMap<u32, f32> = metrics
        .traffic
        .segment_loads
        .iter()
        .map(|l| (l.segment_id, l.density))
        .collect();
    let node_pos: std::collections::HashMap<u32, (f32, f32)> =
        net.nodes.iter().map(|n| (n.id, (n.x, n.z))).collect();
    let needle = args.prefab_contains.as_deref().map(str::to_lowercase);

    let mut rows: Vec<(f32, Value)> = net
        .segments
        .iter()
        .filter_map(|s| {
            let (ax, az) = node_pos.get(&s.start_node).copied()?;
            let (bx, bz) = node_pos.get(&s.end_node).copied()?;
            let d = density.get(&s.id).copied().unwrap_or(0.0);
            let in_bounds = args.bounds.is_none_or(|b| {
                crate::geometry::in_bounds(Position { x: ax, y: 0.0, z: az }, b)
                    || crate::geometry::in_bounds(Position { x: bx, y: 0.0, z: bz }, b)
            });
            let dense_enough = args.min_density.is_none_or(|m| d >= m);
            let prefab_match = needle
                .as_deref()
                .is_none_or(|n| s.prefab.to_lowercase().contains(n));
            (in_bounds && dense_enough && prefab_match).then(|| {
                let key = match args.sort_by.as_deref() {
                    Some("length") => s.length,
                    Some("speed_limit") => s.speed_limit,
                    _ => d,
                };
                (
                    key,
                    json!({
                        "segment_id": s.id,
                        "prefab": s.prefab,
                        "density": d,
                        "one_way": s.one_way,
                        "travel_direction": s.travel_direction,
                        "lanes": s.lanes,
                        "speed_limit": s.speed_limit,
                        "length": s.length,
                        "start_node": s.start_node,
                        "end_node": s.end_node,
                        "midpoint": { "x": (ax + bx) / 2.0, "z": (az + bz) / 2.0 },
                    }),
                )
            })
        })
        .collect();
    rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let total = rows.len();
    let limit = args.limit.unwrap_or(20).min(200);
    let segments: Vec<Value> = rows.into_iter().take(limit).map(|(_, v)| v).collect();
    Ok(json!({ "segments": segments, "total_matching": total }))
}
```

(If the toolchain predates `Option::is_none_or`, use `.map_or(true, |…| …)` with the same closures.)

Register in `broker/src/tools.rs` (add `QuerySegmentsArgs` to the service imports):

```rust
    #[tool(
        description = "Query road segments sorted by congestion (default) — the 'worst N segments' \
            search. Optional filters: min_density, bounds, prefab_contains; sort_by length or \
            speed_limit instead. Returns density, direction, lanes, and midpoint per segment."
    )]
    async fn query_segments(
        &self,
        Parameters(args): Parameters<QuerySegmentsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match service::query_segments(&self.client, args).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }
```

Register in `broker/src/benchmark/server.rs` (read-only, free — same pattern as `get_metrics` without the flow push):

```rust
    #[tool(description = "Query road segments sorted by congestion (default) — the 'worst N segments' \
        search. Optional filters: min_density, bounds, prefab_contains; sort_by length or \
        speed_limit instead. Returns density, direction, lanes, and midpoint per segment.")]
    async fn query_segments(&self, Parameters(args): Parameters<QuerySegmentsArgs>) -> Result<CallToolResult, ErrorData> {
        self.ensure_baseline().await;
        match service::query_segments(&self.client, args).await {
            Ok(v) => self.finish(v).await,
            Err(e) => Ok(tool_err(e)),
        }
    }
```

Update the tool-count tests: in `broker/src/tools.rs` rename `registers_all_twelve_tools` to `registers_all_tools` and insert `"query_segments"` into the sorted expected list (between `"observe_area"` and `"render_map"`); same insertion in `broker/src/benchmark/server.rs`'s `registers_twelve_tools_including_submit_excluding_reset` (rename to `registers_tools_including_submit_excluding_reset`).

- [ ] **Step 4: Run the whole suite**

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: ALL PASS.

- [ ] **Step 5: Allow the tool in run.sh**

In `benchmark/run.sh`, add `,mcp__skylinebench__query_segments` to the `ALLOWED` list.

- [ ] **Step 6: Commit**

```bash
git add broker/src/service.rs broker/src/tools.rs broker/src/benchmark/server.rs benchmark/run.sh
git commit -m "feat(broker): query_segments worst-N congestion search"
```

---

### Task 7: Live verification against the game

- [ ] **Step 1: Launch the game with the rebuilt mod and the benchmark save** (manual, per `benchmark/README.md`).

- [ ] **Step 2: Verify the new wire fields**

Run: `curl -s http://127.0.0.1:8787/network | python3 -c "import json,sys; segs=json.load(sys.stdin)['segments']; print(len(segs)); print({s['travel_direction'] for s in segs}); print([s for s in segs[:3]])"`
Expected: only road segments (count noticeably lower than before — pipes/power gone); `travel_direction` values include `both` and (on the highway spine) `start_to_end`/`end_to_start`; `speed_limit` > 0.

Run: `curl -s http://127.0.0.1:8787/metrics | python3 -c "import json,sys; loads=json.load(sys.stdin)['traffic']['segment_loads']; ds=[l['density'] for l in loads]; print(max(ds), sorted(ds)[-10:])"`
Expected: max density approaches 1.0 in the gridlocked city (not 0.39), with a spread among the top segments.

- [ ] **Step 3: Cross-check direction truth** — pick one known one-way highway segment on screen, note its on-screen arrow direction, and confirm `travel_direction` + node positions agree with it. This validates the `Invert` interpretation; if reversed, flip the ternary in `Direction.Travel` and re-run Task 1's tests (they encode the contract, so update their expectations to match reality, not the other way around).
