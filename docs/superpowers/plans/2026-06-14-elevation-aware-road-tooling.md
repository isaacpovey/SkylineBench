# Elevation-aware Road Tooling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let benchmark agents see road height in 3-D, build correct overpasses/ramps, and validate plans against the game's native checks with a non-mutating ghost preview.

**Architecture:** Route the mod's build/validate through `NetTool.CreateNode` (elevation-aware, native `ToolErrors`, auto elevated/bridge prefab + pillars), retiring the custom `BuildValidator`. Add per-endpoint elevation to the build contract, a `view_3d` angled-screenshot tool, and a `PreviewRenderer : IRenderableManager` that draws a proposed plan as a ghost (via `CreateNode(test:true, visualize:true)`) for an opt-in `validate_plan` preview screenshot.

**Tech Stack:** C# (Cities: Skylines 1 mod, Mono/net35), Rust (broker: rmcp MCP server, axum mock, reqwest bridge client), xbuild + cargo.

**Reference:** spec `docs/superpowers/specs/2026-06-14-elevation-aware-road-tooling-design.md`; memory `nettool-road-tooling-spike` (verified NetTool mechanisms + the throwaway `mod/src/bridge/RoadToolSpike.cs` whose proven code these tasks productionise).

---

## Conventions for this plan

- **Broker (Rust)** tests are standard `cargo test`, mostly via the in-process axum mock (`broker/src/mock.rs`). Run from `broker/`.
- **Mod (C#)** has two test surfaces:
  - **Pure tests** (no game): `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe`. Only pure files (no `UnityEngine`/`Assembly-CSharp` deps) can go here.
  - **Game-coupled logic** (anything touching `NetTool`, `NetManager`, rendering) cannot run headless. Its "test" is **live verification**: `cd mod && ./build.sh`, restart Cities: Skylines, reload a city, then `curl` the bridge. This matches the repo's DISCOVERY-style empirical verification. Build-compiles-against-real-assemblies is itself a strong check.
- Keep the throwaway spike (`RoadToolSpike.cs` + its route/handler/csproj entry) in place until **Task 6.2** so it can be diffed against the productionised code.

---

## File structure

**Mod (C#):**
- Create `mod/src/bridge/RoadErrors.cs` — pure `ToolErrors`-bits → reason-string map (unit-testable, no game deps).
- Create `mod/src/bridge/RoadBuilder.cs` — control-point construction + `NetTool.CreateNode` build/validate (game-coupled).
- Create `mod/src/bridge/PreviewRenderer.cs` — `IRenderableManager` ghost preview (game-coupled).
- Modify `mod/src/bridge/GameActions.cs` — `BuildRoad`/`ValidateRoad` delegate to `RoadBuilder`.
- Delete `mod/src/bridge/BuildValidator.cs`.
- Modify `mod/src/json/RequestParse.cs` — elevation fields on `BuildRoadReq`; add `PreviewReq` parsing.
- Modify `mod/src/http/Handlers.cs` + `Router.cs` — `/preview`, `/preview-clear` routes.
- Modify `mod/SkylineBenchMod.csproj` + `mod/test/Tests.csproj` — compile the new files.

**Broker (Rust):**
- Modify `broker/src/contract.rs` — add `CannotBuildOnWater`, `HeightTooHigh` to `ActionError`.
- Modify `broker/src/service.rs` — elevation on `BuildRoadArgs`; `view_3d` service fn.
- Modify `broker/src/bridge_client.rs` — elevation in build/validate bodies; `preview`/`preview_clear` methods.
- Modify `broker/src/benchmark/plan.rs` — elevation on `PlanOp::BuildRoad`/`BuildPolyline`; interpolate in `expand`.
- Modify `broker/src/benchmark/server.rs` + `broker/src/tools.rs` — `view_3d` tool; `preview` flag wiring in `apply_plan`.
- Modify `broker/src/mock.rs` — accept elevation; `/preview`, `/preview-clear` endpoints.

**Agent-facing:**
- Modify `benchmark/prompt.md` — overpass/ramp/`view_3d`/preview guidance.

---

## Phase 1 — Contract & elevation plumbing (no behavior change yet)

### Task 1.1: New error codes (broker)

**Files:**
- Modify: `broker/src/contract.rs:175-189` (the `ActionError` enum)

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `broker/src/contract.rs`:

```rust
    #[test]
    fn elevation_action_errors_serialize_screaming_snake() {
        assert_eq!(serde_json::to_string(&ActionError::CannotBuildOnWater).unwrap(), "\"CANNOT_BUILD_ON_WATER\"");
        assert_eq!(serde_json::to_string(&ActionError::HeightTooHigh).unwrap(), "\"HEIGHT_TOO_HIGH\"");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd broker && cargo test --lib contract::tests::elevation_action_errors`
Expected: FAIL to compile — `no variant named CannotBuildOnWater`.

- [ ] **Step 3: Add the variants**

In `broker/src/contract.rs`, inside `enum ActionError`, after `NetBufferFull,` add:

```rust
    CannotBuildOnWater,
    HeightTooHigh,
```

Update the doc comment above the enum to mention the two new placement codes.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd broker && cargo test --lib contract::tests::elevation_action_errors`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/contract.rs
git commit -m "feat(broker): add CANNOT_BUILD_ON_WATER and HEIGHT_TOO_HIGH action errors"
```

### Task 1.2: Mod error codes + pure ToolErrors mapping

**Files:**
- Modify: `mod/src/bridge/ErrorCode.cs:9-20`
- Create: `mod/src/bridge/RoadErrors.cs`
- Create: `mod/test/RoadErrorsTests.cs`
- Modify: `mod/test/Tests.csproj`, `mod/test/TestRunner.cs`, `mod/SkylineBenchMod.csproj`

- [ ] **Step 1: Add the two error constants**

In `mod/src/bridge/ErrorCode.cs`, after `public const string NetBufferFull = "NET_BUFFER_FULL";` add:

```csharp
        public const string CannotBuildOnWater = "CANNOT_BUILD_ON_WATER";
        public const string HeightTooHigh = "HEIGHT_TOO_HIGH";
```

- [ ] **Step 2: Write the failing test**

Create `mod/test/RoadErrorsTests.cs`:

```csharp
using System;
using System.Collections.Generic;
using SkylineBench.Bridge;

namespace SkylineBench.Tests
{
    public static class RoadErrorsTests
    {
        public static void Register(List<KeyValuePair<string, Action>> tests)
        {
            tests.Add(new KeyValuePair<string, Action>("roaderrors: none", None));
            tests.Add(new KeyValuePair<string, Action>("roaderrors: collision+water", CollisionWater));
            tests.Add(new KeyValuePair<string, Action>("roaderrors: height/slope/area/connections", Others));
        }

        static void None() { Assert.True(RoadErrors.Reason(0x0UL) == null, "None -> null"); }

        static void CollisionWater()
        {
            // 0x10 ObjectCollision wins over 0x2000 CannotBuildOnWater (priority order).
            Assert.Equal("OBJECT_COLLISION", RoadErrors.Reason(0x2010UL));
            Assert.Equal("CANNOT_BUILD_ON_WATER", RoadErrors.Reason(0x2000UL));
        }

        static void Others()
        {
            Assert.Equal("SLOPE_TOO_STEEP", RoadErrors.Reason(0x200UL));
            Assert.Equal("HEIGHT_TOO_HIGH", RoadErrors.Reason(0x800UL));
            Assert.Equal("OUT_OF_AREA", RoadErrors.Reason(0x20UL));
            Assert.Equal("TOO_MANY_CONNECTIONS", RoadErrors.Reason(0x40000UL));
            Assert.Equal("UNKNOWN", RoadErrors.Reason(0x10000000UL)); // Collapsed -> unmapped tail
        }
    }
}
```

- [ ] **Step 3: Create the pure mapping**

Create `mod/src/bridge/RoadErrors.cs` (operates on raw bits so it is testable without the `ToolBase.ToolErrors` game enum; callers pass `(ulong)toolErrors`):

```csharp
namespace SkylineBench.Bridge
{
    /// <summary>Pure map from ToolBase.ToolErrors bit flags to a normalized
    /// ErrorCode string. Takes the raw ulong so it has no game dependency and
    /// is unit-testable. Returns null when no error bits are set. Priority
    /// order: report the most actionable cause first.</summary>
    public static class RoadErrors
    {
        public static string Reason(ulong bits)
        {
            if (bits == 0UL) return null;
            if ((bits & 0x10UL) != 0) return ErrorCode.ObjectCollision;        // ObjectCollision
            if ((bits & 0x200UL) != 0) return ErrorCode.SlopeTooSteep;         // SlopeTooSteep
            if ((bits & 0x800UL) != 0) return ErrorCode.HeightTooHigh;         // HeightTooHigh
            if ((bits & 0x2000UL) != 0) return ErrorCode.CannotBuildOnWater;   // CannotBuildOnWater
            if ((bits & 0x20UL) != 0) return ErrorCode.OutOfArea;              // OutOfArea
            if ((bits & 0x40000UL) != 0) return ErrorCode.TooManyConnections;  // TooManyConnections
            if ((bits & 0x100UL) != 0) return ErrorCode.InvalidArgs;           // TooShort
            if ((bits & 0x80UL) != 0) return ErrorCode.InvalidArgs;            // InvalidShape
            return ErrorCode.Unknown;
        }
    }
}
```

- [ ] **Step 4: Register the test + compile both projects**

In `mod/test/TestRunner.cs`, add `RoadErrorsTests.Register(tests);` alongside the other `Register` calls (open the file to match the existing pattern).
In `mod/test/Tests.csproj`, add inside the source `ItemGroup`:
```xml
    <Compile Include="..\src\bridge\ErrorCode.cs"><Link>src\ErrorCode.cs</Link></Compile>
    <Compile Include="..\src\bridge\RoadErrors.cs"><Link>src\RoadErrors.cs</Link></Compile>
```
and in the test `ItemGroup`:
```xml
    <Compile Include="RoadErrorsTests.cs" />
```
In `mod/SkylineBenchMod.csproj`, after the `ErrorCode.cs` line add:
```xml
    <Compile Include="src\bridge\RoadErrors.cs" />
```

Note: `ErrorCode.cs` has `using ICities;` — the pure test project does not reference `ICities`. Remove the unused `using ICities;` line from `ErrorCode.cs` (it is not used by the constants or `Prefabs`... verify: `Prefabs` uses `PrefabCollection`/`ItemClass`/`NetInfo` from `Assembly-CSharp`/`ColossalManaged`, NOT `ICities`). If `ErrorCode.cs` pulls game types via `Prefabs`, **split** `Prefabs` into its own `mod/src/bridge/Prefabs.cs` and keep only `ErrorCode` + `RoadInfo` struct in `ErrorCode.cs` so the test project can compile it without game assemblies. Add the new `Prefabs.cs` to `SkylineBenchMod.csproj`.

- [ ] **Step 5: Run the pure tests**

Run: `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe`
Expected: all tests pass, including the three `roaderrors:` cases.

- [ ] **Step 6: Commit**

```bash
git add mod/src/bridge/ErrorCode.cs mod/src/bridge/RoadErrors.cs mod/src/bridge/Prefabs.cs mod/test/RoadErrorsTests.cs mod/test/Tests.csproj mod/test/TestRunner.cs mod/SkylineBenchMod.csproj
git commit -m "feat(mod): native ToolErrors->reason mapping + new error codes"
```

### Task 1.3: Elevation on the build contract (broker args + bridge client)

**Files:**
- Modify: `broker/src/service.rs:193-200` (`BuildRoadArgs`)
- Modify: `broker/src/bridge_client.rs:16-22, 90-130` (`BuildRoadBody`, `build_road`, `validate_road`)

- [ ] **Step 1: Write the failing test**

Add to `#[cfg(test)] mod tests` in `broker/src/bridge_client.rs`:

```rust
    #[tokio::test]
    async fn build_road_sends_elevation_fields() {
        // The mock echoes elevation back via node y (Task 5 wires the mock).
        let client = BridgeClient::new(start_mock().await);
        let res = client
            .build_road_elevated(
                Position { x: 0.0, y: 0.0, z: 0.0 },
                Position { x: 50.0, y: 0.0, z: 0.0 },
                "road", true, 12.0, 12.0,
            )
            .await
            .unwrap();
        assert!(res.ok);
        let net = client.network().await.unwrap();
        // Mock sets node.y to the requested elevation (see Task 5 mock change).
        assert!(net.nodes.iter().all(|n| (n.y - 12.0).abs() < 0.001));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd broker && cargo test --lib bridge_client::tests::build_road_sends_elevation`
Expected: FAIL to compile — `no method named build_road_elevated`.

- [ ] **Step 3: Add elevation to the bridge client**

In `broker/src/bridge_client.rs`, change `BuildRoadBody` to carry elevation:

```rust
#[derive(Serialize)]
struct BuildRoadBody<'a> {
    start: Position,
    end: Position,
    prefab: &'a str,
    snap_to_existing_nodes: bool,
    from_elevation: f32,
    to_elevation: f32,
}
```

Replace `build_road` and `validate_road` with elevation-aware versions (keep the old names as thin wrappers defaulting elevation to 0 so existing callers/tests still compile):

```rust
    pub async fn build_road(
        &self, start: Position, end: Position, prefab: &str, snap: bool,
    ) -> Result<ActionResult, BridgeError> {
        self.build_road_elevated(start, end, prefab, snap, 0.0, 0.0).await
    }

    pub async fn build_road_elevated(
        &self, start: Position, end: Position, prefab: &str, snap: bool,
        from_elevation: f32, to_elevation: f32,
    ) -> Result<ActionResult, BridgeError> {
        let body = BuildRoadBody {
            start, end, prefab, snap_to_existing_nodes: snap, from_elevation, to_elevation,
        };
        Ok(self.http.post(format!("{}/action/build-road", self.base))
            .json(&body).send().await?.error_for_status()?.json().await?)
    }

    pub async fn validate_road(
        &self, start: Position, end: Position, prefab: &str,
    ) -> Result<ActionResult, BridgeError> {
        self.validate_road_elevated(start, end, prefab, 0.0, 0.0).await
    }

    pub async fn validate_road_elevated(
        &self, start: Position, end: Position, prefab: &str,
        from_elevation: f32, to_elevation: f32,
    ) -> Result<ActionResult, BridgeError> {
        let body = BuildRoadBody {
            start, end, prefab, snap_to_existing_nodes: true, from_elevation, to_elevation,
        };
        Ok(self.http.post(format!("{}/action/validate-road", self.base))
            .json(&body).send().await?.error_for_status()?.json().await?)
    }
```

- [ ] **Step 4: Add elevation to BuildRoadArgs**

In `broker/src/service.rs`, extend `BuildRoadArgs`:

```rust
#[derive(Deserialize, schemars::JsonSchema)]
pub struct BuildRoadArgs {
    pub from: Position,
    pub to: Position,
    pub road_type: String,
    #[serde(default = "default_true")]
    pub snap: bool,
    /// Metres above terrain at the `from` end (0 = ground). Use a raised value
    /// for overpasses; differ from `to_elevation` for ramps.
    #[serde(default)]
    pub from_elevation: f32,
    /// Metres above terrain at the `to` end (0 = ground).
    #[serde(default)]
    pub to_elevation: f32,
}
```

In `service::build_road`, change the bridge call from `client.build_road(...)` to `client.build_road_elevated(args.from, args.to, &args.road_type, args.snap, args.from_elevation, args.to_elevation)`.

- [ ] **Step 5: Run test to verify it passes (after Task 5 mock wiring)**

This assertion depends on the mock echoing elevation (Task 5.1). Until then, run only the compile + the rest of the suite:
Run: `cd broker && cargo test --lib bridge_client`
Expected: compiles; pre-existing bridge_client tests PASS. Mark `build_road_sends_elevation_fields` with `#[ignore = "needs mock elevation echo (Task 5.1)"]` for now and remove the ignore in Task 5.1.

- [ ] **Step 6: Commit**

```bash
git add broker/src/service.rs broker/src/bridge_client.rs
git commit -m "feat(broker): thread per-endpoint elevation through build_road/validate_road"
```

### Task 1.4: Elevation on plan ops + interpolation in expand

**Files:**
- Modify: `broker/src/benchmark/plan.rs:20-30` (`PlanOp`), `:58-112` (`lerp_pos`/`expand`)

- [ ] **Step 1: Write the failing test**

Add to `#[cfg(test)] mod tests` in `broker/src/benchmark/plan.rs`:

```rust
    #[test]
    fn build_road_carries_elevation_into_exec() {
        let ops = vec![PlanOp::BuildRoad {
            from: pos(0.0, 0.0), to: pos(50.0, 0.0),
            road_type: "road".into(), snap: true,
            from_elevation: 0.0, to_elevation: 12.0,
        }];
        let exec = expand(&ops);
        assert_eq!(exec.len(), 1);
        match &exec[0].1 {
            ExecOp::Build { from_elevation, to_elevation, .. } => {
                assert_eq!(*from_elevation, 0.0);
                assert_eq!(*to_elevation, 12.0);
            }
            other => panic!("expected Build, got {other:?}"),
        }
    }

    #[test]
    fn polyline_interpolates_elevation_per_chunk() {
        // 360 m line split at 180 m => 2 chunks; elevations 0 -> 12 over the line.
        let ops = vec![PlanOp::BuildPolyline {
            points: vec![pos(0.0, 0.0), pos(360.0, 0.0)],
            road_type: "road".into(), snap: true,
            elevations: vec![0.0, 12.0],
        }];
        let exec = expand(&ops);
        assert_eq!(exec.len(), 2);
        let elevs: Vec<(f32, f32)> = exec.iter().map(|(_, op)| match op {
            ExecOp::Build { from_elevation, to_elevation, .. } => (*from_elevation, *to_elevation),
            _ => panic!(),
        }).collect();
        assert_eq!(elevs, vec![(0.0, 6.0), (6.0, 12.0)]);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd broker && cargo test --lib benchmark::plan`
Expected: FAIL to compile — `PlanOp::BuildRoad` has no `from_elevation`, `ExecOp::Build` has no elevation.

- [ ] **Step 3: Add elevation to PlanOp and ExecOp**

In `broker/src/benchmark/plan.rs`, extend the variants:

```rust
    BuildRoad { from: Position, to: Position, road_type: String, #[serde(default = "default_true")] snap: bool, #[serde(default)] from_elevation: f32, #[serde(default)] to_elevation: f32 },
    BuildPolyline { points: Vec<Position>, road_type: String, #[serde(default = "default_true")] snap: bool, #[serde(default)] elevations: Vec<f32> },
```

```rust
    Build { from: Position, to: Position, road_type: String, snap: bool, from_elevation: f32, to_elevation: f32 },
```

- [ ] **Step 4: Interpolate elevation in expand**

Add an elevation-aware split helper and update `expand`. Replace `split_span` usage so each chunk gets interpolated endpoint elevations:

```rust
/// Fraction (0..1) of the way from `from` to `to` for each chunk boundary.
fn chunk_fractions(from: Position, to: Position) -> Vec<f32> {
    let len = horizontal_distance(from, to);
    let n = (len / POLYLINE_CHUNK_M).ceil().max(1.0) as usize;
    (0..=n).map(|i| i as f32 / n as f32).collect()
}

fn lerp(a: f32, b: f32, t: f32) -> f32 { a + (b - a) * t }

/// Split `from..to` into elevation-aware Build ops; endpoint elevations
/// interpolate linearly between `from_elev` and `to_elev`.
fn build_chunks(from: Position, to: Position, road_type: &str, snap: bool, from_elev: f32, to_elev: f32) -> Vec<ExecOp> {
    let fr = chunk_fractions(from, to);
    fr.windows(2)
        .map(|w| ExecOp::Build {
            from: lerp_pos(from, to, w[0]),
            to: lerp_pos(from, to, w[1]),
            road_type: road_type.to_string(),
            snap,
            from_elevation: lerp(from_elev, to_elev, w[0]),
            to_elevation: lerp(from_elev, to_elev, w[1]),
        })
        .collect()
}
```

Update the `expand` match arms:

```rust
                PlanOp::BuildRoad { from, to, road_type, snap, from_elevation, to_elevation } =>
                    build_chunks(*from, *to, road_type, *snap, *from_elevation, *to_elevation)
                        .into_iter().map(|op| (i, op)).collect(),
                PlanOp::BuildPolyline { points, road_type, snap, elevations } => {
                    if points.len() < 2 { return vec![(i, ExecOp::Invalid)]; }
                    points.windows(2).enumerate().flat_map(|(leg, w)| {
                        // Per-point elevation, defaulting to 0 when the array is short.
                        let e0 = elevations.get(leg).copied().unwrap_or(0.0);
                        let e1 = elevations.get(leg + 1).copied().unwrap_or(0.0);
                        build_chunks(w[0], w[1], road_type, *snap, e0, e1)
                    }).map(|op| (i, op)).collect()
                }
```

Update `validate` and the `estimate`/execution sites in `server.rs` to destructure the new `ExecOp::Build { from, to, road_type, snap, from_elevation, to_elevation }` fields (the `..` patterns already in `validate`/`tool_name` keep working; the execution site in `server.rs` Task 5 reads the elevation fields).

- [ ] **Step 5: Run test to verify it passes**

Run: `cd broker && cargo test --lib benchmark::plan`
Expected: PASS (including pre-existing `expand_*` tests — update their `ExecOp::Build`/`PlanOp` literals to include the new fields where they construct them).

- [ ] **Step 6: Commit**

```bash
git add broker/src/benchmark/plan.rs
git commit -m "feat(broker): per-endpoint/per-point elevation on plan build ops with interpolation"
```

---

## Phase 2 — Mod: build & validate via NetTool

### Task 2.1: Parse elevation in the mod request

**Files:**
- Modify: `mod/src/json/RequestParse.cs:3` (`BuildRoadReq`), `:15-25` (`BuildRoad`)
- Modify: `mod/test/RequestParseTests.cs`

- [ ] **Step 1: Write the failing test**

In `mod/test/RequestParseTests.cs`, extend `BuildRoad()`:

```csharp
            var hi = RequestParse.BuildRoad(JsonReader.Parse(
                "{\"start\":{\"x\":0,\"y\":0,\"z\":0},\"end\":{\"x\":50,\"y\":0,\"z\":0},\"prefab\":\"Basic Road\",\"snap_to_existing_nodes\":true,\"from_elevation\":0,\"to_elevation\":12}"));
            Assert.Equal(0.0, hi.FromElevation);
            Assert.Equal(12.0, hi.ToElevation);
            // Missing fields default to 0.
            Assert.Equal(0.0, r.FromElevation);
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe`
Expected: compile error — `BuildRoadReq` has no `FromElevation`.

- [ ] **Step 3: Add the fields + parse them**

In `mod/src/json/RequestParse.cs`, extend the struct:

```csharp
    public struct BuildRoadReq { public float StartX, StartY, StartZ, EndX, EndY, EndZ; public string Prefab; public bool Snap; public float FromElevation, ToElevation; }
```

In `RequestParse.BuildRoad`, add to the returned object:

```csharp
                FromElevation = v["from_elevation"].IsNull ? 0f : (float)v["from_elevation"].AsDouble(),
                ToElevation = v["to_elevation"].IsNull ? 0f : (float)v["to_elevation"].AsDouble(),
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add mod/src/json/RequestParse.cs mod/test/RequestParseTests.cs
git commit -m "feat(mod): parse from_elevation/to_elevation on build requests"
```

### Task 2.2: RoadBuilder (build + validate via NetTool.CreateNode)

**Files:**
- Create: `mod/src/bridge/RoadBuilder.cs`
- Modify: `mod/src/bridge/GameActions.cs:15-71` (BuildRoad/ValidateRoad bodies)
- Delete: `mod/src/bridge/BuildValidator.cs`
- Modify: `mod/SkylineBenchMod.csproj`

This is game-coupled — verified live (Step 4), not headless. The code is the productionised, proven spike path (`RoadToolSpike.cs`).

- [ ] **Step 1: Create RoadBuilder**

Create `mod/src/bridge/RoadBuilder.cs`:

```csharp
using System.Collections.Generic;
using ColossalFramework;
using ColossalFramework.Math;
using UnityEngine;
using SkylineBench.Dto;
using SkylineBench.Json;

namespace SkylineBench.Bridge
{
    /// <summary>Builds and validates roads through the game's own NetTool, so
    /// elevation auto-selects the elevated/bridge prefab variant (with pillars)
    /// and validation uses the native ToolErrors (collisions vs roads AND
    /// buildings, water, slope, height, area). Replaces the hand-rolled
    /// BuildValidator + raw CreateSegment path. Must run on the simulation
    /// thread (build) — callers wrap in SimThread.Run.</summary>
    public static class RoadBuilder
    {
        private const float SnapToleranceM = 8f;
        private const float SnapHeightToleranceM = 4f;
        private const int MaxSegments = 1; // broker pre-splits spans under the segment cap

        public static ActionResultDto Build(BuildRoadReq req) { return Run(req, false); }

        public static ActionResultDto Validate(BuildRoadReq req) { return Run(req, true); }

        private static ActionResultDto Run(BuildRoadReq req, bool test)
        {
            var prefab = Prefabs.FindRoad(req.Prefab);
            if (prefab == null) return ActionResultDto.Fail(ErrorCode.InvalidPrefab);

            var nm = Singleton<NetManager>.instance;
            var tm = Singleton<TerrainManager>.instance;

            var startXZ = new Vector3(req.StartX, 0f, req.StartZ);
            var endXZ = new Vector3(req.EndX, 0f, req.EndZ);
            float lenXZ = VectorUtils.LengthXZ(endXZ - startXZ);
            if (lenXZ < 0.001f) return ActionResultDto.Fail(ErrorCode.InvalidArgs);
            Vector3 dir = VectorUtils.NormalizeXZ(endXZ - startXZ);

            float startY = tm.SampleDetailHeight(startXZ) + req.FromElevation;
            float endY = tm.SampleDetailHeight(endXZ) + req.ToElevation;
            var startPos = new Vector3(req.StartX, startY, req.StartZ);
            var endPos = new Vector3(req.EndX, endY, req.EndZ);
            var midPos = (startPos + endPos) * 0.5f;

            var startCp = MakeCp(nm, startPos, dir, req.FromElevation, req.Snap);
            var endCp = MakeCp(nm, endPos, dir, req.ToElevation, req.Snap);
            var midCp = Cp(midPos, dir, (req.FromElevation + req.ToElevation) * 0.5f, 0);

            ushort node, segment; int cost, prod;
            ToolBase.ToolErrors err = NetTool.CreateNode(
                prefab, startCp, midCp, endCp,
                new FastList<NetTool.NodePosition>(), MaxSegments,
                test, /*visualize*/ false, /*autoFix*/ true, /*needMoney*/ false,
                /*invert*/ false, /*switchDir*/ false, /*relocateBuildingID*/ 0,
                out node, out segment, out cost, out prod);

            if (err != ToolBase.ToolErrors.None)
                return ActionResultDto.Fail(RoadErrors.Reason((ulong)err));

            var result = new ActionResultDto { Ok = true };
            if (!test)
            {
                if (segment != 0) result.CreatedSegments.Add(segment);
                if (node != 0) result.CreatedNodes.Add(node);
                if (startCp.m_node != 0) result.SnappedNodes.Add(startCp.m_node);
                if (endCp.m_node != 0) result.SnappedNodes.Add(endCp.m_node);
            }
            result.ZonedBuildingsFronting = (int)Frontage.CountZonedBuildingsNear(startPos, endPos, prefab.m_halfWidth);
            return result;
        }

        /// <summary>Snap to the nearest existing node within tolerance whose
        /// height matches the requested elevation (so a ramp foot snaps to a
        /// ground node and its top to an elevated one); otherwise leave m_node=0
        /// so NetTool creates a fresh node at m_position.</summary>
        private static NetTool.ControlPoint MakeCp(NetManager nm, Vector3 pos, Vector3 dir, float elevation, bool snap)
        {
            ushort snapTo = snap ? NearestNode(nm, pos) : (ushort)0;
            var cp = Cp(pos, dir, elevation, snapTo);
            if (snapTo != 0) cp.m_position = nm.m_nodes.m_buffer[snapTo].m_position;
            return cp;
        }

        private static ushort NearestNode(NetManager nm, Vector3 p)
        {
            ushort best = 0; float bestD = SnapToleranceM;
            for (uint i = 1; i < nm.m_nodes.m_buffer.Length; i++)
            {
                var n = nm.m_nodes.m_buffer[i];
                if ((n.m_flags & NetNode.Flags.Created) == NetNode.Flags.None) continue;
                if (Mathf.Abs(n.m_position.y - p.y) > SnapHeightToleranceM) continue;
                float d = VectorUtils.LengthXZ(n.m_position - p);
                if (d <= bestD) { bestD = d; best = (ushort)i; }
            }
            return best;
        }

        private static NetTool.ControlPoint Cp(Vector3 pos, Vector3 dir, float elevation, ushort node)
        {
            return new NetTool.ControlPoint
            {
                m_position = pos, m_direction = dir,
                m_node = node, m_segment = 0,
                m_elevation = elevation, m_outside = false,
            };
        }
    }
}
```

Note: add a `ToZ()` helper is unnecessary — replace `req.ToZ()` with `req.EndZ`. (Lint: the draft above uses `req.EndZ` directly; ensure `endPos` uses `req.EndZ`.)

- [ ] **Step 2: Delegate GameActions to RoadBuilder**

In `mod/src/bridge/GameActions.cs`, replace the bodies of `BuildRoad` and `ValidateRoad` (remove the `MaxSegmentLengthM`/`BuildValidator`/`ResolveNode`/`NearestNode`/`FailReleasing` machinery they no longer need — keep `Bulldoze`/`UpgradeRoad`/`SetZone`/`Clock`/`Step` untouched):

```csharp
        public static ActionResultDto BuildRoad(BuildRoadReq req)
        {
            return SimThread.Run<ActionResultDto>(delegate { return RoadBuilder.Build(req); }, TimeoutMs);
        }

        public static ActionResultDto ValidateRoad(BuildRoadReq req)
        {
            return SimThread.Run<ActionResultDto>(delegate { return RoadBuilder.Validate(req); }, TimeoutMs);
        }
```

Keep `private const int TimeoutMs = 8000;`. Remove now-unused `SnapToleranceM`/`MaxSegmentLengthM` consts and the `ResolveNode`/`NearestNode`/`FailReleasing` helpers if no other method uses them (UpgradeRoad does not — verify).

- [ ] **Step 3: Delete BuildValidator and update csproj**

```bash
git rm mod/src/bridge/BuildValidator.cs
```
In `mod/SkylineBenchMod.csproj`: remove the `<Compile Include="src\bridge\BuildValidator.cs" />` line; add `<Compile Include="src\bridge\RoadBuilder.cs" />`.

- [ ] **Step 4: Build + live-verify**

```bash
cd mod && ./build.sh
```
Expected: `Build succeeded.` (compiles against the real assemblies — the primary correctness check).
Then restart Cities: Skylines, reload a city, and verify against the live bridge:
```bash
# ground road through a building -> native collision
curl -s -X POST http://127.0.0.1:8787/action/validate-road -d '{"start":{"x":-60,"y":0,"z":1},"end":{"x":20,"y":0,"z":1},"prefab":"Basic Road","from_elevation":0,"to_elevation":0}'
# expect {"ok":false,"reason":"OBJECT_COLLISION"...}
# same span elevated -> clean
curl -s -X POST http://127.0.0.1:8787/action/validate-road -d '{"start":{"x":-60,"y":0,"z":1},"end":{"x":20,"y":0,"z":1},"prefab":"Basic Road","from_elevation":12,"to_elevation":12}'
# expect {"ok":true,...}
# build the elevated overpass and confirm a segment is created
curl -s -X POST http://127.0.0.1:8787/action/build-road -d '{"start":{"x":-60,"y":0,"z":1},"end":{"x":20,"y":0,"z":1},"prefab":"Basic Road","from_elevation":12,"to_elevation":12,"snap_to_existing_nodes":true}'
# expect ok:true with created_segments; /network count +1 segment; then bulldoze it to clean up
```
Expected: matches the comments (these are the spike's verified results). If a result differs, debug before continuing.

- [ ] **Step 5: Commit**

```bash
git add mod/src/bridge/RoadBuilder.cs mod/src/bridge/GameActions.cs mod/SkylineBenchMod.csproj
git commit -m "feat(mod): build and validate roads via NetTool.CreateNode (elevation + native validation), retire BuildValidator"
```

---

## Phase 3 — Mod: non-mutating ghost preview

### Task 3.1: PreviewRenderer + /preview, /preview-clear

**Files:**
- Create: `mod/src/bridge/PreviewRenderer.cs`
- Modify: `mod/src/json/RequestParse.cs` (add `PreviewReq` + parse), `mod/src/http/Handlers.cs`, `mod/src/http/Router.cs`, `mod/SkylineBenchMod.csproj`

Game-coupled — verified live. Code is the productionised spike `PreviewRenderer`, extended to a list of ops.

- [ ] **Step 1: Create PreviewRenderer**

Create `mod/src/bridge/PreviewRenderer.cs`:

```csharp
using System.Collections.Generic;
using ColossalFramework;
using ColossalFramework.Math;
using UnityEngine;
using SkylineBench.Json;

namespace SkylineBench.Bridge
{
    /// <summary>An IRenderableManager that draws one or more proposed roads as a
    /// ghost each frame via the game's own CreateNode(test:true, visualize:true)
    /// path. test:true commits NOTHING — there is no segment to roll back.
    /// Registered once (no Unregister API exists); gated by Active.</summary>
    public sealed class PreviewRenderer : IRenderableManager
    {
        public struct Ghost { public NetInfo Prefab; public NetTool.ControlPoint A, Mid, B; }

        public static volatile bool Active;
        private static readonly List<Ghost> _ghosts = new List<Ghost>();
        private static readonly object _lock = new object();
        private static bool _registered;

        public static void SetGhosts(List<Ghost> ghosts)
        {
            lock (_lock) { _ghosts.Clear(); _ghosts.AddRange(ghosts); }
        }

        public static void Ensure()
        {
            if (_registered) return;
            RenderManager.RegisterRenderableManager(new PreviewRenderer());
            _registered = true;
        }

        /// <summary>Build a ghost (start/mid/end control points) from world XZ +
        /// per-endpoint elevation. Pure setup; no mutation.</summary>
        public static Ghost MakeGhost(NetInfo prefab, float sx, float sz, float ex, float ez, float fromElev, float toElev)
        {
            var tm = Singleton<TerrainManager>.instance;
            var sXZ = new Vector3(sx, 0f, sz);
            var eXZ = new Vector3(ex, 0f, ez);
            Vector3 dir = VectorUtils.NormalizeXZ(eXZ - sXZ);
            var a = new Vector3(sx, tm.SampleDetailHeight(sXZ) + fromElev, sz);
            var b = new Vector3(ex, tm.SampleDetailHeight(eXZ) + toElev, ez);
            return new Ghost
            {
                Prefab = prefab,
                A = Cp(a, dir, fromElev), Mid = Cp((a + b) * 0.5f, dir, (fromElev + toElev) * 0.5f), B = Cp(b, dir, toElev),
            };
        }

        private static NetTool.ControlPoint Cp(Vector3 pos, Vector3 dir, float elev)
        {
            return new NetTool.ControlPoint { m_position = pos, m_direction = dir, m_node = 0, m_segment = 0, m_elevation = elev, m_outside = false };
        }

        public string GetName() { return "SkylineBenchPreview"; }
        public DrawCallData GetDrawCallData() { return default(DrawCallData); }
        public void CheckReferences() { }
        public void InitRenderData() { }
        public bool CalculateGroupData(int groupX, int groupZ, int layer, ref int vertexCount, ref int triangleCount, ref int objectCount, ref RenderGroup.VertexArrays vertexArrays) { return false; }
        public void PopulateGroupData(int groupX, int groupZ, int layer, ref int vertexIndex, ref int triangleIndex, Vector3 groupPosition, RenderGroup.MeshData data, ref Vector3 min, ref Vector3 max, ref float maxRenderDistance, ref float maxInstanceDistance, ref bool requireSurfaceMaps) { }
        public void BeginRendering(RenderManager.CameraInfo cameraInfo) { }
        public void BeginOverlay(RenderManager.CameraInfo cameraInfo) { }
        public void UndergroundOverlay(RenderManager.CameraInfo cameraInfo) { }
        public void EndOverlay(RenderManager.CameraInfo cameraInfo) { }

        public void EndRendering(RenderManager.CameraInfo cameraInfo)
        {
            if (!Active) return;
            List<Ghost> snapshot;
            lock (_lock) { snapshot = new List<Ghost>(_ghosts); }
            foreach (var g in snapshot)
            {
                if (g.Prefab == null) continue;
                try
                {
                    ushort node, segment; int cost, prod;
                    NetTool.CreateNode(g.Prefab, g.A, g.Mid, g.B,
                        new FastList<NetTool.NodePosition>(), 1,
                        /*test*/ true, /*visualize*/ true, /*autoFix*/ true, /*needMoney*/ false,
                        false, false, 0, out node, out segment, out cost, out prod);
                }
                catch { }
            }
        }
    }
}
```

- [ ] **Step 2: Parse the preview request + handlers + routes**

In `mod/src/json/RequestParse.cs`, add:

```csharp
    public struct PreviewOp { public float StartX, StartZ, EndX, EndZ, FromElevation, ToElevation; public string Prefab; }
    public struct PreviewReq { public System.Collections.Generic.List<PreviewOp> Ops; }
```

```csharp
        public static PreviewReq Preview(JsonValue v)
        {
            var ops = new System.Collections.Generic.List<PreviewOp>();
            var arr = v["ops"];
            for (int i = 0; i < arr.Count; i++)
            {
                var o = arr[i]; var s = o["start"]; var e = o["end"];
                ops.Add(new PreviewOp
                {
                    StartX = (float)s["x"].AsDouble(), StartZ = (float)s["z"].AsDouble(),
                    EndX = (float)e["x"].AsDouble(), EndZ = (float)e["z"].AsDouble(),
                    FromElevation = o["from_elevation"].IsNull ? 0f : (float)o["from_elevation"].AsDouble(),
                    ToElevation = o["to_elevation"].IsNull ? 0f : (float)o["to_elevation"].AsDouble(),
                    Prefab = o["prefab"].AsString(),
                });
            }
            return new PreviewReq { Ops = ops };
        }
```

In `mod/src/http/Handlers.cs`, add (uses `CaptureBehaviour.RunOnMain` for the main-thread registration/set, mirroring the spike):

```csharp
        public static HttpReply Preview(string body)
        {
            var req = RequestParse.Preview(JsonReader.Parse(body));
            CaptureBehaviour.RunOnMain(delegate
            {
                var ghosts = new System.Collections.Generic.List<PreviewRenderer.Ghost>();
                foreach (var op in req.Ops)
                {
                    var prefab = Prefabs.FindRoad(op.Prefab);
                    if (prefab == null) continue;
                    ghosts.Add(PreviewRenderer.MakeGhost(prefab, op.StartX, op.StartZ, op.EndX, op.EndZ, op.FromElevation, op.ToElevation));
                }
                PreviewRenderer.SetGhosts(ghosts);
                PreviewRenderer.Ensure();
                PreviewRenderer.Active = true;
            }, 8000);
            var w = new JsonWriter();
            w.BeginObject().Name("ok").Value(true).Name("active").Value(true).EndObject();
            return HttpReply.Json(200, w.ToString());
        }

        public static HttpReply PreviewClear(string body)
        {
            CaptureBehaviour.RunOnMain(delegate { PreviewRenderer.Active = false; }, 8000);
            var w = new JsonWriter();
            w.BeginObject().Name("ok").Value(true).Name("active").Value(false).EndObject();
            return HttpReply.Json(200, w.ToString());
        }
```

(Add `using SkylineBench.Bridge;` — already present in Handlers.cs.)

In `mod/src/http/Router.cs`, add before the `default:`:

```csharp
                case "/preview": return method == "POST" ? Handlers.Preview(body) : MethodNotAllowed();
                case "/preview-clear": return method == "POST" ? Handlers.PreviewClear(body) : MethodNotAllowed();
```

In `mod/SkylineBenchMod.csproj`: add `<Compile Include="src\bridge\PreviewRenderer.cs" />`.

- [ ] **Step 3: Build + live-verify**

```bash
cd mod && ./build.sh
```
Expected: `Build succeeded.` Then restart the game, reload a city:
```bash
curl -s http://127.0.0.1:8787/network | python3 -c "import sys,json;d=json.load(sys.stdin);print(len(d['nodes']),len(d['segments']))" # baseline
curl -s -X POST http://127.0.0.1:8787/preview -d '{"ops":[{"start":{"x":-60,"z":1},"end":{"x":20,"z":1},"from_elevation":12,"to_elevation":12,"prefab":"Basic Road"}]}'
curl -s -X POST http://127.0.0.1:8787/screenshot -d '{"x":-20,"z":1,"size":250,"top_down":false}' -o /tmp/preview.png  # ghost visible
curl -s http://127.0.0.1:8787/network | python3 -c "import sys,json;d=json.load(sys.stdin);print(len(d['nodes']),len(d['segments']))" # UNCHANGED
curl -s -X POST http://127.0.0.1:8787/preview-clear -d '{}'
curl -s -X POST http://127.0.0.1:8787/screenshot -d '{"x":-20,"z":1,"size":250,"top_down":false}' -o /tmp/preview-off.png  # ghost gone
```
Expected: ghost in `/tmp/preview.png`, gone in `/tmp/preview-off.png`, counts unchanged (non-mutating). Inspect the PNGs.

- [ ] **Step 4: Commit**

```bash
git add mod/src/bridge/PreviewRenderer.cs mod/src/json/RequestParse.cs mod/src/http/Handlers.cs mod/src/http/Router.cs mod/SkylineBenchMod.csproj
git commit -m "feat(mod): non-mutating ghost preview via IRenderableManager (/preview, /preview-clear)"
```

---

## Phase 4 — Broker: view_3d tool

### Task 4.1: view_3d service fn + mock + tools

**Files:**
- Modify: `broker/src/service.rs` (add `view_3d` args + fn), `broker/src/tools.rs`, `broker/src/benchmark/server.rs`

- [ ] **Step 1: Write the failing test**

Add to `#[cfg(test)] mod tests` in `broker/src/service.rs`:

```rust
    #[tokio::test]
    async fn view_3d_returns_png() {
        let c = client().await;
        let png = view_3d(&c, ViewArgs { x: 0.0, z: 0.0, size: None, top_down: None }).await.unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd broker && cargo test --lib service::tests::view_3d_returns_png`
Expected: FAIL to compile — `ViewArgs`/`view_3d` undefined.

- [ ] **Step 3: Implement view_3d**

In `broker/src/service.rs`, add:

```rust
#[derive(Deserialize, schemars::JsonSchema)]
pub struct ViewArgs {
    /// World X to centre on (metres).
    pub x: f32,
    /// World Z to centre on (metres).
    pub z: f32,
    /// Vertical view extent in metres (larger = more zoomed out). Default 350.
    #[serde(default)]
    pub size: Option<f32>,
    /// true = straight-down; false (default) = 45° angled so road height,
    /// pillars and overpass clearance are visible.
    #[serde(default)]
    pub top_down: Option<bool>,
}

/// Angled (default) game screenshot centred on (x, z). Returns PNG bytes; the
/// rmcp layer wraps them as image content. This is how the agent perceives
/// elevation — overpasses, ramps, clearances — which the 2-D render_map cannot show.
pub async fn view_3d(client: &BridgeClient, args: ViewArgs) -> Result<Vec<u8>, ServiceError> {
    let shot = CameraShot {
        x: args.x,
        z: args.z,
        size: args.size.unwrap_or(CLOSEUP_SIZE_M),
        top_down: args.top_down.unwrap_or(false),
    };
    Ok(capture_screenshot(client, shot).await?)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd broker && cargo test --lib service::tests::view_3d_returns_png`
Expected: PASS (the mock `/screenshot` returns a PNG).

- [ ] **Step 5: Register the tool in both servers**

In `broker/src/tools.rs`, add a method (and import `ViewArgs` in the `use service::{...}` list):

```rust
    #[tool(description = "Angled 3-D screenshot of a location: a 45° game render showing road height, \
        bridges, pillars and overpass clearance — use it to SEE elevation that render_map (top-down) cannot. \
        Args: x, z (world metres), optional size (default 350; larger zooms out), top_down (default false).")]
    async fn view_3d(&self, Parameters(args): Parameters<service::ViewArgs>) -> Result<CallToolResult, ErrorData> {
        match service::view_3d(&self.client, args).await {
            Ok(png) => {
                let data = base64::engine::general_purpose::STANDARD.encode(png);
                Ok(CallToolResult::success(vec![Content::image(data, "image/png".to_string())]))
            }
            Err(e) => Ok(tool_error(e)),
        }
    }
```

Update the `registers_all_tools` test list in `tools.rs` to include `"view_3d"`.

In `broker/src/benchmark/server.rs`, add the analogous method (attach `city_status` like `render_map` does):

```rust
    #[tool(description = "Angled 3-D screenshot of a location: a 45° game render showing road height, \
        bridges, pillars and overpass clearance — use it to SEE elevation that render_map (top-down) cannot. \
        Args: x, z (world metres), optional size (default 350; larger zooms out), top_down (default false).")]
    async fn view_3d(&self, Parameters(args): Parameters<crate::service::ViewArgs>) -> Result<CallToolResult, ErrorData> {
        self.ensure_baseline().await;
        match crate::service::view_3d(&self.client, args).await {
            Ok(png) => {
                let data = base64::engine::general_purpose::STANDARD.encode(png);
                let progress = {
                    let mut s = self.state.lock().await;
                    s.check_timeout();
                    s.progress()
                };
                let status = serde_json::json!({ "city_status": progress }).to_string();
                Ok(CallToolResult::success(vec![
                    Content::image(data, "image/png".to_string()),
                    Content::text(status),
                ]))
            }
            Err(e) => Ok(tool_err(e)),
        }
    }
```

Update the `registers_tools_including_submit_excluding_reset` test list in `server.rs` to include `"view_3d"`.

- [ ] **Step 6: Run the tool-registration tests**

Run: `cd broker && cargo test --lib tools::tests::registers_all_tools benchmark::server::tests::registers_tools`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add broker/src/service.rs broker/src/tools.rs broker/src/benchmark/server.rs
git commit -m "feat(broker): add view_3d angled-screenshot tool to both MCP servers"
```

---

## Phase 5 — Broker: validate_plan ghost preview

### Task 5.1: Mock elevation echo + /preview, /preview-clear

**Files:**
- Modify: `broker/src/mock.rs` (`BuildRoadBody`, `build_road`, `router`, add preview endpoints)

- [ ] **Step 1: Write the failing test**

Add to `#[cfg(test)] mod tests` in `broker/src/mock.rs`:

```rust
    #[tokio::test]
    async fn preview_endpoints_respond_ok() {
        let (addr, server) = bind("127.0.0.1:0".parse().unwrap()).await;
        tokio::spawn(server);
        let client = reqwest::Client::new();
        let set: serde_json::Value = client.post(format!("http://{addr}/preview"))
            .json(&serde_json::json!({"ops": []})).send().await.unwrap().json().await.unwrap();
        assert_eq!(set["ok"], true);
        let clear: serde_json::Value = client.post(format!("http://{addr}/preview-clear"))
            .json(&serde_json::json!({})).send().await.unwrap().json().await.unwrap();
        assert_eq!(clear["active"], false);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd broker && cargo test --lib mock::tests::preview_endpoints_respond_ok`
Expected: FAIL — 404 (route missing), JSON decode error.

- [ ] **Step 3: Add elevation echo + preview endpoints to the mock**

In `broker/src/mock.rs`, extend `BuildRoadBody`:

```rust
#[derive(Deserialize)]
struct BuildRoadBody {
    start: Position,
    end: Position,
    prefab: String,
    snap_to_existing_nodes: bool,
    #[serde(default)]
    from_elevation: f32,
    #[serde(default)]
    to_elevation: f32,
}
```

In `build_road`, after resolving nodes, set the created nodes' `y` to the requested elevation so the elevation-threading test (Task 1.3) can assert it. Change `resolve_node` to take an elevation and store it as `y` for newly created nodes:

```rust
fn resolve_node(p: Position, elevation: f32, snap: bool, city: &mut City) -> (u32, bool) {
    if snap {
        if let Some(id) = nearest_node_within_tolerance(p, &city.nodes) {
            return (id, true);
        }
    }
    let id = city.next_id;
    city.next_id += 1;
    city.nodes.push(NetNode { id, x: p.x, y: elevation, z: p.z });
    (id, false)
}
```

Update the two calls: `resolve_node(body.start, body.from_elevation, snap, &mut c)` and `resolve_node(body.end, body.to_elevation, snap, &mut c)`.

Add the preview handlers + routes:

```rust
async fn preview(State(_s): State<MockState>, Json(_body): Json<serde_json::Value>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true, "active": true }))
}

async fn preview_clear(State(_s): State<MockState>, Json(_body): Json<serde_json::Value>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true, "active": false }))
}
```

In `router()`, add:

```rust
        .route("/preview", post(preview))
        .route("/preview-clear", post(preview_clear))
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd broker && cargo test --lib mock::tests::preview_endpoints_respond_ok`
Expected: PASS.
Now remove the `#[ignore]` from `bridge_client::tests::build_road_sends_elevation_fields` (Task 1.3) and run it:
Run: `cd broker && cargo test --lib bridge_client::tests::build_road_sends_elevation`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/mock.rs broker/src/bridge_client.rs
git commit -m "test(broker): mock echoes build elevation as node y; add /preview endpoints"
```

### Task 5.2: bridge_client preview methods

**Files:**
- Modify: `broker/src/bridge_client.rs`

- [ ] **Step 1: Write the failing test**

Add to `#[cfg(test)] mod tests` in `broker/src/bridge_client.rs`:

```rust
    #[tokio::test]
    async fn preview_set_and_clear() {
        let client = BridgeClient::new(start_mock().await);
        client.preview(&[(Position { x: 0.0, y: 0.0, z: 0.0 }, Position { x: 50.0, y: 0.0, z: 0.0 }, "road".to_string(), 12.0, 12.0)]).await.unwrap();
        client.preview_clear().await.unwrap();
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd broker && cargo test --lib bridge_client::tests::preview_set_and_clear`
Expected: FAIL to compile — no `preview`/`preview_clear`.

- [ ] **Step 3: Implement the methods**

In `broker/src/bridge_client.rs`:

```rust
    /// Set the non-mutating ghost preview to these build ops
    /// (from, to, prefab, from_elevation, to_elevation). Builds nothing.
    pub async fn preview(
        &self, ops: &[(Position, Position, String, f32, f32)],
    ) -> Result<(), BridgeError> {
        let ops_json: Vec<serde_json::Value> = ops.iter().map(|(from, to, prefab, fe, te)| {
            serde_json::json!({ "start": from, "end": to, "prefab": prefab, "from_elevation": fe, "to_elevation": te })
        }).collect();
        self.http.post(format!("{}/preview", self.base))
            .json(&serde_json::json!({ "ops": ops_json }))
            .send().await?.error_for_status()?;
        Ok(())
    }

    pub async fn preview_clear(&self) -> Result<(), BridgeError> {
        self.http.post(format!("{}/preview-clear", self.base))
            .json(&serde_json::json!({}))
            .send().await?.error_for_status()?;
        Ok(())
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd broker && cargo test --lib bridge_client::tests::preview_set_and_clear`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/bridge_client.rs
git commit -m "feat(broker): bridge client preview/preview_clear methods"
```

### Task 5.3: validate_plan preview wiring

**Files:**
- Modify: `broker/src/benchmark/server.rs:49-60` (`ApplyPlanArgs`), `:701-747` (the `validate_only` branch)

- [ ] **Step 1: Write the failing test**

Add to `#[cfg(test)] mod tests` in `broker/src/benchmark/server.rs` (uses the existing `bench_with_mock` + `plan_build` helpers; add a `plan_build` if absent matching the existing test usage):

```rust
    #[tokio::test]
    async fn validate_only_with_preview_returns_image() {
        let bench = bench_with_mock().await;
        let res = bench.apply_plan(Parameters(ApplyPlanArgs {
            ops: vec![PlanOp::BuildRoad {
                from: crate::contract::Position { x: 0.0, y: 0.0, z: 0.0 },
                to: crate::contract::Position { x: 50.0, y: 0.0, z: 0.0 },
                road_type: "road".into(), snap: true, from_elevation: 12.0, to_elevation: 12.0,
            }],
            validate_only: true, stop_on_error: true, preview: true,
        })).await.unwrap();
        // One image content block (the ghost) plus the JSON text block.
        assert!(res.content.iter().any(|c| c.as_image().is_some()), "expected a preview image");
    }
```

(Add `use crate::benchmark::plan::PlanOp;` to the test module if not present.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cd broker && cargo test --lib benchmark::server::tests::validate_only_with_preview`
Expected: FAIL to compile — `ApplyPlanArgs` has no `preview`.

- [ ] **Step 3: Add the preview flag + wiring**

In `ApplyPlanArgs`, add:

```rust
    /// When true together with validate_only, render a non-mutating ghost
    /// screenshot of the plan's build ops (angled 3-D) alongside the JSON.
    #[serde(default)]
    pub preview: bool,
```

In the `if args.validate_only || !all_valid { ... }` branch, after building `results`/`first_failed_at` and BEFORE the final `return self.finish(...)`, when `args.validate_only && args.preview` and there is at least one build op, collect the build ops, set the preview, screenshot framing them, clear, and return an extra image content block. Replace the `return self.finish(json!{...})` in that branch with:

```rust
            let payload = serde_json::json!({
                "ok": first_failed_at.is_none(),
                "validate_only": args.validate_only,
                "results": results,
                "total_estimated_cost": total_estimated_cost,
                "first_failed_at": first_failed_at,
            });
            if args.validate_only && args.preview {
                let builds: Vec<(crate::contract::Position, crate::contract::Position, String, f32, f32)> = exec.iter().filter_map(|(_, op)| match op {
                    ExecOp::Build { from, to, road_type, from_elevation, to_elevation, .. } =>
                        Some((*from, *to, road_type.clone(), *from_elevation, *to_elevation)),
                    _ => None,
                }).collect();
                if !builds.is_empty() {
                    let positions: Vec<(f32, f32)> = builds.iter()
                        .map(|(f, t, ..)| ((f.x + t.x) / 2.0, (f.z + t.z) / 2.0)).collect();
                    let _ = self.client.preview(&builds).await;
                    let shot = crate::service::region_shot(&positions);
                    let png = match shot {
                        Some(shot) => crate::service::capture_screenshot(&self.client, shot).await.ok(),
                        None => None,
                    };
                    let _ = self.client.preview_clear().await;
                    if let Some(png) = png {
                        let data = base64::engine::general_purpose::STANDARD.encode(png);
                        let merged = { let s = self.state.lock().await; with_progress(payload, &s) };
                        return Ok(CallToolResult::success(vec![
                            Content::image(data, "image/png".to_string()),
                            Content::text(merged.to_string()),
                        ]));
                    }
                }
            }
            return self.finish(payload).await;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd broker && cargo test --lib benchmark::server::tests::validate_only_with_preview`
Expected: PASS.

- [ ] **Step 5: Update apply_plan description + run full suite**

In the `apply_plan` `#[tool(description = ...)]`, append a sentence: "Set `preview:true` with `validate_only` to also get a non-mutating angled 3-D screenshot of the proposed roads (builds nothing)."
Run: `cd broker && cargo test`
Expected: all tests PASS.

- [ ] **Step 6: Commit**

```bash
git add broker/src/benchmark/server.rs
git commit -m "feat(broker): opt-in ghost-preview screenshot for validate_plan"
```

---

## Phase 6 — Agent-facing copy & spike removal

### Task 6.1: Prompt + tool guidance

**Files:**
- Modify: `benchmark/prompt.md`

- [ ] **Step 1: Add elevation/3-D guidance**

In `benchmark/prompt.md`, in the Observe list add `view_3d` and in the Modify list document elevation. Insert after the `build_road` note (line ~8):

```markdown
  Build elevated roads by setting `from_elevation` / `to_elevation` (metres above ground, 0 = on the ground):
  an **overpass** is a build with both ends raised (e.g. 12) crossing over another road; an **on/off-ramp** is a
  sloped build with one end on the ground and the other raised (e.g. 0 → 12) connecting a surface road to an
  elevated one. The game picks the elevated/bridge prefab and pillars automatically. Separating through-traffic
  onto an overpass is often the high-leverage fix for a jammed interchange.
- `view_3d` (free): an angled 3-D screenshot of a location showing real road height, bridges and clearance.
  `render_map` is top-down and cannot show elevation — use `view_3d` to understand a junction's vertical structure
  before and after a change.
```

In the `apply_plan` paragraph, add: "Pass `preview: true` with `validate_only` to get a non-mutating angled screenshot of the proposed roads (nothing is built) so you can see the geometry before committing."

- [ ] **Step 2: Commit**

```bash
git add benchmark/prompt.md
git commit -m "docs(benchmark): teach agents overpasses/ramps, view_3d, and validate preview"
```

### Task 6.2: Remove the throwaway spike

**Files:**
- Delete: `mod/src/bridge/RoadToolSpike.cs`
- Modify: `mod/src/http/Router.cs`, `mod/src/http/Handlers.cs`, `mod/SkylineBenchMod.csproj`

- [ ] **Step 1: Remove the spike code**

```bash
git rm mod/src/bridge/RoadToolSpike.cs
```
In `mod/src/http/Router.cs`, delete the `case "/spike/road": ...` line.
In `mod/src/http/Handlers.cs`, delete the `RoadSpike` handler method.
In `mod/SkylineBenchMod.csproj`, delete the `<Compile Include="src\bridge\RoadToolSpike.cs" />` line.

- [ ] **Step 2: Build to confirm nothing referenced the spike**

Run: `cd mod && ./build.sh`
Expected: `Build succeeded.`

- [ ] **Step 3: Commit**

```bash
git add mod/src/http/Router.cs mod/src/http/Handlers.cs mod/SkylineBenchMod.csproj
git commit -m "chore(mod): remove throwaway NetTool spike (productionised in RoadBuilder/PreviewRenderer)"
```

### Task 6.3: Full verification

- [ ] **Step 1: Broker tests**

Run: `cd broker && cargo test`
Expected: all PASS.

- [ ] **Step 2: Mod pure tests**

Run: `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe`
Expected: all PASS.

- [ ] **Step 3: Mod build**

Run: `cd mod && ./build.sh`
Expected: `Build succeeded.`

- [ ] **Step 4: Live end-to-end smoke (game running)**

Restart the game + reload a city, then exercise the full agent path against the live bridge: `view_3d` (angled PNG), `apply_plan` with `validate_only:true, preview:true` on an elevated overpass (JSON `ok:true` + a PNG, network counts unchanged), then a real `apply_plan` building the overpass (segment created), then bulldoze to clean up.
Expected: all behave as designed. Capture the screenshots and confirm the overpass renders elevated.

- [ ] **Step 5: Commit any fixes, then finish the branch**

Use superpowers:finishing-a-development-branch to open the PR. Note in the PR description that a full benchmark run is the real validation of the underperformance theory (do models now build overpasses and improve?).

---

## Self-review notes

- **Spec coverage:** mod NetTool build (2.2), native validation + new error codes (1.1/1.2/2.2), per-endpoint + per-point elevation (1.3/1.4/2.1), `view_3d` (4.1), non-mutating preview (3.1) + validate_plan wiring (5.3), prompt (6.1), spike removal (6.2), BuildValidator deletion (2.2). All spec sections map to a task.
- **Type consistency:** `from_elevation`/`to_elevation` used identically across broker args, plan ops, bridge client, mock, and mod `BuildRoadReq`; `ExecOp::Build` carries both; `ViewArgs`/`view_3d`/`PreviewRenderer.Ghost`/`RoadErrors.Reason` names are consistent between definition and use.
- **Out of scope (intentional):** the green/red validity *coloring* on the ghost — geometry preview + JSON validity suffices. Tunnels (negative elevation) work mechanically but aren't a v1 focus.
```
