# MCP feedback & contract fixes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the agent's feedback blind spots (locate building problems, dry-run builds, distinguish short-vs-malformed segments, honest bulldoze) and remove misleading/dead contract surface, without changing the (intentionally off) economy.

**Architecture:** Two layers move together. The Rust **broker** owns the MCP tools (`tools.rs`), the service layer (`service.rs`), the wire contract (`contract.rs`), the bridge HTTP client (`bridge_client.rs`), and a test **mock** of the mod (`mock.rs`). The C# **mod** owns the in-game HTTP server (`http/`), the game reads/actions (`bridge/`), DTOs (`dto/Dtos.cs`), and serializers (`json/Serialize.cs`). Pure C# (`Dtos.cs`, `Serialize.cs`, `ErrorCode.cs`, `RoadErrors.cs`) is unit-tested by `mod/test`; game-coupled C# is compile-verified and flagged for live verify.

**Tech Stack:** Rust (broker, `cargo test`, axum mock, rmcp tools), C#/.NET 3.5 against Cities: Skylines assemblies (mod, built via `mod/build.sh`, pure tests via `xbuild` + `mono`).

**Out of scope:** Item A of the spec (engine-accurate `colliding_buildings`) is **deferred** — see `docs/superpowers/research/2026-06-16-collision-locality-investigation.md`. This plan covers spec items B–H.

**Reference spec:** `docs/superpowers/specs/2026-06-16-mcp-feedback-contract-fixes-design.md`

## Commands

- Broker tests: `cd broker && cargo test`
- Mod pure tests: `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe`
- Mod compile (game-coupled): `cd mod && ./build.sh` (compiles + installs the dll; success = green build)

## File map

- `broker/src/contract.rs` — drop `employed`; remove `Collision`/`InsufficientFunds`; add `TooShort`/`InvalidShape`; add `ProblemBuilding`/`Problems`.
- `broker/src/mock.rs` — drop `employed`; `_low` zone vocab; add `/problems` endpoint.
- `broker/src/bridge_client.rs` — add `problems()` GET.
- `broker/src/service.rs` — add `validate_road`, `query_problems` (+ `QueryProblemsArgs`).
- `broker/src/tools.rs` — add `validate_road`, `query_problems` tools; update registration test.
- `mod/src/dto/Dtos.cs` — drop `Employed`; add `ProblemBuildingDto`/`ProblemsDto`.
- `mod/src/json/Serialize.cs` — drop `employed`; add `Problems`.
- `mod/src/bridge/ErrorCode.cs` — remove `Collision`/`InsufficientFunds`; add `TooShort`/`InvalidShape`.
- `mod/src/bridge/RoadErrors.cs` — map `TooShort`/`InvalidShape` to distinct codes.
- `mod/src/bridge/BuildingProblems.cs` — NEW shared problem-flag → name mapping.
- `mod/src/bridge/GameReads.cs` — use the shared mapping for counts; add `Problems()`.
- `mod/src/bridge/GameActions.cs` — bulldoze existence/bounds checks.
- `mod/src/http/Handlers.cs` — `_low` zone list; add `Problems` handler.
- `mod/src/http/Router.cs` — add `/problems` route.
- `mod/test/SerializeTests.cs` — update metrics expectation; add problems test.
- `mod/test/RoadErrorsTests.cs` — assert the two new codes.

---

### Task 1: (C) Drop the always-zero `employed` field

**Files:**
- Modify: `broker/src/contract.rs` (PopulationMetrics + `metrics_round_trips` test)
- Modify: `broker/src/mock.rs:139-145`
- Modify: `mod/src/dto/Dtos.cs:21`
- Modify: `mod/src/bridge/GameReads.cs:130-131`
- Modify: `mod/src/json/Serialize.cs:60-62`
- Modify: `mod/test/SerializeTests.cs:41,47`

- [ ] **Step 1: Remove `employed` from the broker contract**

In `broker/src/contract.rs`, the `PopulationMetrics` struct (around line 124) — delete the `employed` field and its doc-free line:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PopulationMetrics {
    pub total: u32,
    pub residential_demand: u8,
    pub commercial_demand: u8,
    /// CS1 exposes a single combined industrial+office ("workplace") demand,
    /// not separate industrial/office values — see mod DISCOVERY.md.
    pub workplace_demand: u8,
}
```

In the same file, the `metrics_round_trips` test (around line 513) — delete the `employed: 1500,` line from the `PopulationMetrics { ... }` literal.

- [ ] **Step 2: Remove `employed` from the mock**

In `broker/src/mock.rs`, the `metrics` handler `PopulationMetrics { ... }` literal (around line 139) — delete the `employed: 700,` line so it reads:

```rust
        population: PopulationMetrics {
            total: 1000,
            residential_demand: 50,
            commercial_demand: 40,
            workplace_demand: 30,
        },
```

- [ ] **Step 3: Run broker tests**

Run: `cd broker && cargo test`
Expected: PASS (compiles without `employed`; all tests green).

- [ ] **Step 4: Commit the broker side**

```bash
cd "$HOME/Documents/personal/SkylineBench"
git add broker/src/contract.rs broker/src/mock.rs
git commit -m "refactor(broker): drop always-zero population.employed from contract"
```

- [ ] **Step 5: Remove `Employed` from the mod DTO**

In `mod/src/dto/Dtos.cs`, the `MetricsDto` field line (line 21) — delete ` public uint Employed;` so the line ends at `public byte WorkplaceDemand;`:

```csharp
        public uint Population; public byte ResidentialDemand; public byte CommercialDemand; public byte WorkplaceDemand;
```

- [ ] **Step 6: Remove the `Employed` write from GameReads and Serialize**

In `mod/src/bridge/GameReads.cs`, delete these two lines (around 130-131):

```csharp
                // Employment isn't cleanly exposed by a single manager field; left at 0.
                dto.Employed = 0;
```

In `mod/src/json/Serialize.cs`, the population group (around line 60-62) — remove the `employed` name/value so it reads:

```csharp
            w.Name("population").BeginObject().Name("total").Value((long)m.Population).Name("residential_demand").Value((long)m.ResidentialDemand)
                .Name("commercial_demand").Value((long)m.CommercialDemand).Name("workplace_demand").Value((long)m.WorkplaceDemand)
                .EndObject();
```

- [ ] **Step 7: Update the metrics serialize test**

In `mod/test/SerializeTests.cs`, `Metrics()` (line 41) — remove `Employed = 1500,` from the `MetricsDto { ... }` initializer. Then update the population assertion (line 47) to drop `employed`:

```csharp
            Assert.True(json.Contains("\"population\":{\"total\":2000,\"residential_demand\":50,\"commercial_demand\":40,\"workplace_demand\":30}"), "population group");
```

- [ ] **Step 8: Run the mod pure tests**

Run: `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe`
Expected: `... passed, 0 failed`.

- [ ] **Step 9: Compile the mod assembly**

Run: `cd mod && ./build.sh`
Expected: build succeeds, `Installed SkylineBenchMod.dll`.

- [ ] **Step 10: Commit the mod side**

```bash
cd "$HOME/Documents/personal/SkylineBench"
git add mod/src/dto/Dtos.cs mod/src/bridge/GameReads.cs mod/src/json/Serialize.cs mod/test/SerializeTests.cs
git commit -m "refactor(mod): drop always-zero employed from metrics"
```

---

### Task 2: (E) Prune dead error codes (`COLLISION`, `INSUFFICIENT_FUNDS`)

The build path runs `needMoney:false` and emits `OBJECT_COLLISION`, so plain `COLLISION` and `INSUFFICIENT_FUNDS` can never be produced. Remove them from the advertised surface.

**Files:**
- Modify: `broker/src/contract.rs` (ActionError enum + doc comment + `action_result_error_serializes_reason` test)
- Modify: `mod/src/bridge/ErrorCode.cs:7-8`

- [ ] **Step 1: Remove the two variants from the broker enum**

In `broker/src/contract.rs`, the `ActionError` enum (around line 208) — delete the `Collision,` and `InsufficientFunds,` lines. Also update the doc comment above it (around line 201) to drop the `COLLISION`/`INSUFFICIENT_FUNDS` references:

```rust
/// Normalised failure reasons for actions. Mod-side placement codes
/// (`OBJECT_COLLISION`, `SLOPE_TOO_STEEP`, `OUT_OF_AREA`, `TOO_MANY_CONNECTIONS`,
/// `NET_BUFFER_FULL`, `TOO_SHORT`, `INVALID_SHAPE`) come from the mod's RoadErrors;
/// the elevation codes (`CANNOT_BUILD_ON_WATER`, `HEIGHT_TOO_HIGH`) come from NetTool;
/// broker-side pre-validation adds `OUT_OF_BOUNDS`, `SEGMENT_TOO_LONG`,
/// `INVALID_PREFAB`, `DEGENERATE_SEGMENT`, `INVALID_ARGS`. Building costs are not
/// enforced, so no funds-related reason exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActionError {
    OutOfBounds,
    InvalidPrefab,
    SegmentTooLong,
    DegenerateSegment,
    InvalidArgs,
    Unknown,
    ObjectCollision,
    SlopeTooSteep,
    OutOfArea,
    TooManyConnections,
    NetBufferFull,
    CannotBuildOnWater,
    HeightTooHigh,
}
```

Keep the existing derive list verbatim (the real enum derives `Serialize, Deserialize`, not `schemars::JsonSchema`) — only the variants and the doc comment change.

- [ ] **Step 2: Fix the test that referenced `Collision`**

In `broker/src/contract.rs`, the `action_result_error_serializes_reason` test (around line 374) uses `ActionError::Collision`. Change it to `ActionError::ObjectCollision` and the expected string to `OBJECT_COLLISION`:

```rust
            reason: Some(ActionError::ObjectCollision),
```
```rust
        assert!(json.contains("\"reason\":\"OBJECT_COLLISION\""), "got {json}");
```

- [ ] **Step 3: Run broker tests**

Run: `cd broker && cargo test`
Expected: PASS. (If the compiler flags any other use of the removed variants, fix that reference to the appropriate remaining variant.)

- [ ] **Step 4: Commit the broker side**

```bash
cd "$HOME/Documents/personal/SkylineBench"
git add broker/src/contract.rs
git commit -m "refactor(broker): drop unreachable COLLISION/INSUFFICIENT_FUNDS reasons"
```

- [ ] **Step 5: Confirm the mod constants are unused, then remove them**

Run: `cd "$HOME/Documents/personal/SkylineBench" && grep -rn "ErrorCode.Collision\b\|ErrorCode.InsufficientFunds" mod/src`
Expected: no matches (only the declarations themselves are in `ErrorCode.cs`).

In `mod/src/bridge/ErrorCode.cs`, delete lines 7-8:

```csharp
        public const string Collision = "COLLISION";
        public const string InsufficientFunds = "INSUFFICIENT_FUNDS";
```

- [ ] **Step 6: Run mod pure tests + compile**

Run: `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe`
Expected: `... passed, 0 failed`.
Run: `cd mod && ./build.sh`
Expected: build succeeds.

- [ ] **Step 7: Commit the mod side**

```bash
cd "$HOME/Documents/personal/SkylineBench"
git add mod/src/bridge/ErrorCode.cs
git commit -m "refactor(mod): remove unused COLLISION/INSUFFICIENT_FUNDS codes"
```

---

### Task 3: (H) Distinguish `TOO_SHORT` and `INVALID_SHAPE`

`RoadErrors` currently folds native `TooShort` (0x100) and `InvalidShape` (0x80) into `INVALID_ARGS`. Give them distinct codes. This is unit-testable (TDD).

**Files:**
- Modify: `mod/test/RoadErrorsTests.cs:25-32`
- Modify: `mod/src/bridge/ErrorCode.cs`
- Modify: `mod/src/bridge/RoadErrors.cs:18-19`
- Modify: `broker/src/contract.rs` (ActionError + a serialization test)

- [ ] **Step 1: Write the failing mod test**

In `mod/test/RoadErrorsTests.cs`, replace the `Others()` body (lines 25-32) with:

```csharp
        static void Others()
        {
            Assert.Equal("SLOPE_TOO_STEEP", RoadErrors.Reason(0x200UL));
            Assert.Equal("HEIGHT_TOO_HIGH", RoadErrors.Reason(0x800UL));
            Assert.Equal("OUT_OF_AREA", RoadErrors.Reason(0x20UL));
            Assert.Equal("TOO_MANY_CONNECTIONS", RoadErrors.Reason(0x40000UL));
            Assert.Equal("TOO_SHORT", RoadErrors.Reason(0x100UL));
            Assert.Equal("INVALID_SHAPE", RoadErrors.Reason(0x80UL));
            Assert.Equal("UNKNOWN", RoadErrors.Reason(0x10000000UL)); // Unmapped tail
        }
```

- [ ] **Step 2: Run mod tests to see it fail**

Run: `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe`
Expected: FAIL on `roaderrors: height/slope/area/connections` (`TOO_SHORT` expected, got `INVALID_ARGS`) — or a compile error referencing the missing constants once Step 3 adds the mapping. (If it fails to compile because `ErrorCode.TooShort` doesn't exist yet, that's fine — proceed.)

- [ ] **Step 3: Add the two constants**

In `mod/src/bridge/ErrorCode.cs`, add after `HeightTooHigh`:

```csharp
        public const string TooShort = "TOO_SHORT";
        public const string InvalidShape = "INVALID_SHAPE";
```

- [ ] **Step 4: Map the bits to the new codes**

In `mod/src/bridge/RoadErrors.cs`, replace lines 18-19:

```csharp
            if ((bits & 0x100UL) != 0) return ErrorCode.TooShort;              // TooShort
            if ((bits & 0x80UL) != 0) return ErrorCode.InvalidShape;           // InvalidShape
```

- [ ] **Step 5: Run mod tests to verify pass**

Run: `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe`
Expected: `... passed, 0 failed`.

- [ ] **Step 6: Compile the mod assembly + commit**

Run: `cd mod && ./build.sh` → build succeeds.

```bash
cd "$HOME/Documents/personal/SkylineBench"
git add mod/src/bridge/ErrorCode.cs mod/src/bridge/RoadErrors.cs mod/test/RoadErrorsTests.cs
git commit -m "feat(mod): distinguish TOO_SHORT and INVALID_SHAPE build errors"
```

- [ ] **Step 7: Add the broker variants + serialization test**

In `broker/src/contract.rs`, add `TooShort,` and `InvalidShape,` to the `ActionError` enum (after `HeightTooHigh`). Then add this test inside the `contract.rs` `mod tests` block:

```rust
    #[test]
    fn short_and_shape_errors_serialize_screaming_snake() {
        assert_eq!(serde_json::to_string(&ActionError::TooShort).unwrap(), "\"TOO_SHORT\"");
        assert_eq!(serde_json::to_string(&ActionError::InvalidShape).unwrap(), "\"INVALID_SHAPE\"");
    }
```

- [ ] **Step 8: Run broker tests + commit**

Run: `cd broker && cargo test`
Expected: PASS.

```bash
cd "$HOME/Documents/personal/SkylineBench"
git add broker/src/contract.rs
git commit -m "feat(broker): add TOO_SHORT/INVALID_SHAPE action reasons"
```

---

### Task 4: (D) Canonicalise zone vocabulary on the `_low` suffix

`observe_area` emits `residential_low`/`commercial_low`, but `list_zone_types` advertised `residential`/`commercial`, so the read string was rejected on write. Align the advertised list (and the mock) with what reads emit. `GameActions.ParseZone` already accepts the `_low` forms (and keeps the bare aliases), so no parse change is needed.

**Files:**
- Modify: `broker/src/mock.rs:73-80` and `broker/src/service.rs` (one test)
- Modify: `mod/src/http/Handlers.cs:55`

- [ ] **Step 1: Update the mock zone vocabulary**

In `broker/src/mock.rs`, the `zone_types()` function (lines 73-80):

```rust
fn zone_types() -> Vec<String> {
    vec![
        "residential_low".into(),
        "residential_high".into(),
        "commercial_low".into(),
        "commercial_high".into(),
        "industrial".into(),
        "office".into(),
    ]
}
```

- [ ] **Step 2: Update the set_zoning over-the-wire test**

In `broker/src/service.rs`, the `set_zoning_adds_a_zone_cell_over_the_wire` test — change `zone_type: "residential".into(),` to `zone_type: "residential_low".into(),`. (The `set_zoning_rejects_unknown_zone` test uses `"spaceport"`, which is still invalid — leave it.)

- [ ] **Step 3: Run broker tests**

Run: `cd broker && cargo test`
Expected: PASS.

- [ ] **Step 4: Commit the broker side**

```bash
cd "$HOME/Documents/personal/SkylineBench"
git add broker/src/mock.rs broker/src/service.rs
git commit -m "fix(broker): zone vocabulary uses _low suffix to round-trip with reads"
```

- [ ] **Step 5: Update the mod's advertised zone list**

In `mod/src/http/Handlers.cs`, the `ZoneTypes()` handler (line 55) — replace the string array:

```csharp
            foreach (var z in new string[] { "residential_low", "residential_high", "commercial_low", "commercial_high", "industrial", "office" }) w.Value(z);
```

- [ ] **Step 6: Compile the mod + commit**

Run: `cd mod && ./build.sh` → build succeeds.

```bash
cd "$HOME/Documents/personal/SkylineBench"
git add mod/src/http/Handlers.cs
git commit -m "fix(mod): advertise _low zone types so observe->set round-trips"
```

> **Live verify (after a game run):** `set_zoning` with `residential_low` over a road-adjacent rectangle, then `observe_area` returns cells whose `zone_type` is `residential_low` — and the same string is accepted, not `INVALID_ARGS`.

---

### Task 5: (F) Expose `validate_road` as a dry-run MCP tool

The mod endpoint (`/action/validate-road`), the bridge client (`validate_road_elevated`), and the mock endpoint all exist already. Add the service function and the MCP tool.

**Files:**
- Modify: `broker/src/service.rs` (new `validate_road` + 2 tests; reuses `BuildRoadArgs`)
- Modify: `broker/src/tools.rs` (new tool + registration test)

- [ ] **Step 1: Add the service function**

In `broker/src/service.rs`, after `build_road` (around line 241), add:

```rust
/// Dry-run a build: broker-side pre-validation, then the mod's native
/// `validate-road` (test-mode NetTool) — no segment is created.
pub async fn validate_road(client: &BridgeClient, args: BuildRoadArgs) -> Result<Value, ServiceError> {
    let road_types = client.road_types().await?.road_types;
    if let Err(reason) = validate_build_road(args.from, args.to, &args.road_type, &road_types) {
        return Ok(action_error_value(reason));
    }
    let res = client
        .validate_road_elevated(args.from, args.to, &args.road_type, args.snap, args.from_elevation, args.to_elevation)
        .await?;
    Ok(serde_json::to_value(res).unwrap())
}
```

- [ ] **Step 2: Add service tests**

In `broker/src/service.rs` `mod tests`, add:

```rust
    #[tokio::test]
    async fn validate_road_accepts_valid_placement_without_committing() {
        let c = client().await;
        let v = validate_road(
            &c,
            BuildRoadArgs {
                from: Position { x: 0.0, y: 0.0, z: 0.0 },
                to: Position { x: 50.0, y: 0.0, z: 0.0 },
                road_type: "road".into(),
                snap: true,
                from_elevation: 0.0,
                to_elevation: 0.0,
            },
        )
        .await
        .unwrap();
        assert_eq!(v["ok"], true);
        // No segment was created — validate is a dry-run.
        let obs = observe_area(&c, ObserveAreaArgs { bounds: None }).await.unwrap();
        assert_eq!(obs["network"]["segments"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn validate_road_rejects_unknown_type() {
        let c = client().await;
        let v = validate_road(
            &c,
            BuildRoadArgs {
                from: Position { x: 0.0, y: 0.0, z: 0.0 },
                to: Position { x: 50.0, y: 0.0, z: 0.0 },
                road_type: "teleporter".into(),
                snap: true,
                from_elevation: 0.0,
                to_elevation: 0.0,
            },
        )
        .await
        .unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["reason"], "INVALID_PREFAB");
    }
```

- [ ] **Step 3: Run the new service tests**

Run: `cd broker && cargo test validate_road`
Expected: 2 passed.

- [ ] **Step 4: Add the MCP tool**

In `broker/src/tools.rs`, after the `build_road` tool (around line 133), add:

```rust
    #[tool(description = "Dry-run a road build: test placement (collisions, slope, water, height, bounds) \
        WITHOUT committing or creating any segment. Same args as build_road. Use it to check a placement \
        before build_road commits it.")]
    async fn validate_road(
        &self,
        Parameters(args): Parameters<BuildRoadArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match service::validate_road(&self.client, args).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }
```

(`BuildRoadArgs` is already imported in `tools.rs`.)

- [ ] **Step 5: Update the registration test**

In `broker/src/tools.rs`, the `registers_all_tools` test — insert `"validate_road"` into the sorted `vec![...]` (alphabetically, after `upgrade_road`, before `view_3d`):

```rust
                "upgrade_road",
                "validate_road",
                "view_3d",
```

- [ ] **Step 6: Run broker tests + commit**

Run: `cd broker && cargo test`
Expected: PASS.

```bash
cd "$HOME/Documents/personal/SkylineBench"
git add broker/src/service.rs broker/src/tools.rs
git commit -m "feat(broker): expose validate_road MCP tool (build dry-run)"
```

---

### Task 6: (G) Make `bulldoze` validate the target exists

Node and building bulldozes currently return a phantom `ok:true` for any id. Guard each branch on the buffer length and the `Created` flag. Game-coupled — compile-verified + live verify.

**Files:**
- Modify: `mod/src/bridge/GameActions.cs:24-52`

- [ ] **Step 1: Rewrite the `Bulldoze` delegate**

In `mod/src/bridge/GameActions.cs`, replace the body of `Bulldoze` (the `SimThread.Run<...>(delegate { switch ... }` block, lines ~26-51) with:

```csharp
            return SimThread.Run<ActionResultDto>(delegate
            {
                switch (req.TargetType)
                {
                    case "segment":
                    {
                        var nm = Singleton<NetManager>.instance;
                        if (req.Id >= nm.m_segments.m_buffer.Length) return ActionResultDto.Fail(ErrorCode.InvalidArgs);
                        var seg = nm.m_segments.m_buffer[req.Id];
                        if ((seg.m_flags & NetSegment.Flags.Created) == NetSegment.Flags.None) return ActionResultDto.Fail(ErrorCode.InvalidArgs);
                        int fronting = -1;
                        if (seg.Info != null)
                        {
                            Vector3 aPos = nm.m_nodes.m_buffer[seg.m_startNode].m_position;
                            Vector3 bPos = nm.m_nodes.m_buffer[seg.m_endNode].m_position;
                            fronting = (int)Frontage.CountZonedBuildingsNear(aPos, bPos, seg.Info.m_halfWidth);
                        }
                        nm.ReleaseSegment((ushort)req.Id, false);
                        var res = new ActionResultDto { Ok = true, ZonedBuildingsFronting = fronting };
                        res.Destroyed.Add(req.Id);
                        return res;
                    }
                    case "node":
                    {
                        var nm = Singleton<NetManager>.instance;
                        if (req.Id >= nm.m_nodes.m_buffer.Length) return ActionResultDto.Fail(ErrorCode.InvalidArgs);
                        if ((nm.m_nodes.m_buffer[req.Id].m_flags & NetNode.Flags.Created) == NetNode.Flags.None) return ActionResultDto.Fail(ErrorCode.InvalidArgs);
                        nm.ReleaseNode((ushort)req.Id);
                        break;
                    }
                    case "building":
                    {
                        var bm = Singleton<BuildingManager>.instance;
                        if (req.Id >= bm.m_buildings.m_buffer.Length) return ActionResultDto.Fail(ErrorCode.InvalidArgs);
                        if ((bm.m_buildings.m_buffer[req.Id].m_flags & Building.Flags.Created) == Building.Flags.None) return ActionResultDto.Fail(ErrorCode.InvalidArgs);
                        bm.ReleaseBuilding((ushort)req.Id);
                        break;
                    }
                    default: return ActionResultDto.Fail(ErrorCode.InvalidArgs);
                }
                var r = new ActionResultDto { Ok = true }; r.Destroyed.Add(req.Id); return r;
            }, TimeoutMs);
```

- [ ] **Step 2: Compile the mod + commit**

Run: `cd mod && ./build.sh` → build succeeds.

```bash
cd "$HOME/Documents/personal/SkylineBench"
git add mod/src/bridge/GameActions.cs
git commit -m "fix(mod): bulldoze validates target exists instead of phantom ok"
```

> **Live verify (after a game run):** bulldoze a real segment id → `ok:true, destroyed:[id]`; bulldoze a bogus id (e.g. 999999) for each of `segment`/`node`/`building` → `{ok:false, reason:"INVALID_ARGS"}`.

---

### Task 7a: (B) query_problems — pure DTO + serializer (TDD)

**Files:**
- Modify: `mod/src/dto/Dtos.cs`
- Modify: `mod/src/json/Serialize.cs`
- Modify: `mod/test/SerializeTests.cs`

- [ ] **Step 1: Add the DTOs**

In `mod/src/dto/Dtos.cs`, after the `ZonesDto` definitions (around line 13), add:

```csharp
    public struct ProblemBuildingDto { public uint Id; public float X; public float Z; public string Category; public List<string> Problems; }
    public sealed class ProblemsDto { public List<ProblemBuildingDto> Buildings = new List<ProblemBuildingDto>(); }
```

- [ ] **Step 2: Write the failing serializer test**

In `mod/test/SerializeTests.cs`, register a new test in `Register` (after the saves entries):

```csharp
            tests.Add(new KeyValuePair<string, Action>("serialize: problems", Problems));
```

And add the method:

```csharp
        static void Problems()
        {
            var p = new ProblemsDto();
            var pb = new ProblemBuildingDto { Id = 5, X = 10f, Z = 20f, Category = "residential", Problems = new List<string>() };
            pb.Problems.Add("road_not_connected");
            pb.Problems.Add("no_fuel");
            p.Buildings.Add(pb);
            Assert.Equal(
                "{\"buildings\":[{\"id\":5,\"x\":10,\"z\":20,\"category\":\"residential\",\"problems\":[\"road_not_connected\",\"no_fuel\"]}]}",
                Serialize.Problems(p));
        }
```

- [ ] **Step 3: Run mod tests to see it fail**

Run: `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe`
Expected: compile error — `Serialize.Problems` does not exist yet.

- [ ] **Step 4: Implement the serializer**

In `mod/src/json/Serialize.cs`, after the `Zones` method (around line 47), add:

```csharp
        public static string Problems(ProblemsDto p)
        {
            var w = new JsonWriter();
            w.BeginObject().Name("buildings").BeginArray();
            foreach (var b in p.Buildings)
            {
                w.BeginObject().Name("id").Value((long)b.Id).Name("x").Value(b.X).Name("z").Value(b.Z)
                    .Name("category").Value(b.Category).Name("problems").BeginArray();
                foreach (var pr in b.Problems) w.Value(pr);
                w.EndArray().EndObject();
            }
            w.EndArray().EndObject();
            return w.ToString();
        }
```

- [ ] **Step 5: Run mod tests to verify pass**

Run: `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe`
Expected: `... passed, 0 failed` (including `serialize: problems`).

- [ ] **Step 6: Commit**

```bash
cd "$HOME/Documents/personal/SkylineBench"
git add mod/src/dto/Dtos.cs mod/src/json/Serialize.cs mod/test/SerializeTests.cs
git commit -m "feat(mod): add ProblemsDto + Serialize.Problems"
```

---

### Task 7b: (B) query_problems — shared mapping + game reads + route

Game-coupled (references `Building`/`Notification` enums) — compile-verified + live verify. The shared `BuildingProblems.Names` is the single source of truth for both the `/metrics` counts and `/problems`.

**Files:**
- Create: `mod/src/bridge/BuildingProblems.cs`
- Modify: `mod/src/bridge/GameReads.cs` (use shared mapping in `Metrics`; add `Problems`)
- Modify: `mod/src/http/Handlers.cs`
- Modify: `mod/src/http/Router.cs`

- [ ] **Step 1: Create the shared problem-name mapping**

Create `mod/src/bridge/BuildingProblems.cs`:

```csharp
using System.Collections.Generic;

namespace SkylineBench.Bridge
{
    /// <summary>Single source of truth mapping a building's problem flags to the
    /// normalised problem-name vocabulary, shared by the /metrics counts and the
    /// /problems read so the two cannot drift. Must run on the simulation thread
    /// (reads live Building state).</summary>
    public static class BuildingProblems
    {
        public static List<string> Names(ref Building b)
        {
            var names = new List<string>();
            if ((b.m_flags & Building.Flags.Abandoned) != Building.Flags.None) names.Add("abandoned");
            // Building problem flags live in ProblemStruct.m_Problems1 (this game
            // version split the old flat Notification.Problem enum in two).
            var p = b.m_problems.m_Problems1;
            if (Has(p, Notification.Problem1.RoadNotConnected)) names.Add("road_not_connected");
            if (Has(p, Notification.Problem1.Electricity) || Has(p, Notification.Problem1.ElectricityNotConnected)) names.Add("no_electricity");
            if (Has(p, Notification.Problem1.Water) || Has(p, Notification.Problem1.WaterNotConnected)) names.Add("no_water");
            if (Has(p, Notification.Problem1.Sewage)) names.Add("no_sewage");
            if (Has(p, Notification.Problem1.Garbage)) names.Add("garbage_piling");
            if (Has(p, Notification.Problem1.NoFuel)) names.Add("no_fuel");
            return names;
        }

        private static bool Has(Notification.Problem1 flags, Notification.Problem1 flag)
        {
            return (flags & flag) != Notification.Problem1.None;
        }
    }
}
```

- [ ] **Step 2: Route the `/metrics` counts through the shared mapping**

In `mod/src/bridge/GameReads.cs`, replace the building-problem counting loop inside `Metrics()` (the block from `uint abandoned = 0, ...` through the `dto.NoFuel = noFuel;` assignments, lines ~134-156) with a version that derives counts from `BuildingProblems.Names`:

```csharp
                var bm = Singleton<BuildingManager>.instance;
                uint abandoned = 0, roadNotConnected = 0, noElec = 0, noWater = 0, noSewage = 0, garbage = 0, noFuel = 0;
                for (uint i = 0; i < bm.m_buildings.m_buffer.Length; i++)
                {
                    var b = bm.m_buildings.m_buffer[i];
                    if ((b.m_flags & Building.Flags.Created) == Building.Flags.None) continue;
                    var names = BuildingProblems.Names(ref b);
                    if (names.Contains("abandoned")) abandoned++;
                    if (names.Contains("road_not_connected")) roadNotConnected++;
                    if (names.Contains("no_electricity")) noElec++;
                    if (names.Contains("no_water")) noWater++;
                    if (names.Contains("no_sewage")) noSewage++;
                    if (names.Contains("garbage_piling")) garbage++;
                    if (names.Contains("no_fuel")) noFuel++;
                }
                dto.AbandonedBuildings = abandoned;
                dto.RoadNotConnected = roadNotConnected;
                dto.NoElectricity = noElec;
                dto.NoWater = noWater;
                dto.NoSewage = noSewage;
                dto.GarbagePiling = garbage;
                dto.NoFuel = noFuel;
```

Then delete the now-unused private `Has(Notification.Problem1, Notification.Problem1)` helper in `GameReads.cs` (lines ~161-164) — the shared one in `BuildingProblems` replaces it. (Confirm with `grep -n "private static bool Has" mod/src/bridge/GameReads.cs` that it's no longer referenced before deleting.)

- [ ] **Step 3: Add the `Problems()` read**

In `mod/src/bridge/GameReads.cs`, add a method (next to `Buildings()`):

```csharp
        public static ProblemsDto Problems()
        {
            return SimThread.Run<ProblemsDto>(delegate
            {
                var dto = new ProblemsDto();
                var bm = Singleton<BuildingManager>.instance;
                for (uint i = 0; i < bm.m_buildings.m_buffer.Length; i++)
                {
                    var b = bm.m_buildings.m_buffer[i];
                    if ((b.m_flags & Building.Flags.Created) == Building.Flags.None) continue;
                    var names = BuildingProblems.Names(ref b);
                    if (names.Count == 0) continue;
                    dto.Buildings.Add(new ProblemBuildingDto
                    {
                        Id = i, X = b.m_position.x, Z = b.m_position.z,
                        Category = Category(b.Info), Problems = names,
                    });
                }
                return dto;
            }, TimeoutMs);
        }
```

(`Category(BuildingInfo)` is the existing private helper in the same class.)

- [ ] **Step 4: Add the handler + route**

In `mod/src/http/Handlers.cs`, next to the other read handlers (around line 29-32), add:

```csharp
        public static HttpReply Problems() { return HttpReply.Json(200, Serialize.Problems(GameReads.Problems())); }
```

In `mod/src/http/Router.cs`, add a case after `/zones` (line ~21):

```csharp
                case "/problems": return method == "GET" ? Handlers.Problems() : MethodNotAllowed();
```

(Note: this is `/problems`, distinct from the existing `/probe`.)

- [ ] **Step 5: Compile the mod + commit**

Run: `cd mod && ./build.sh` → build succeeds.

```bash
cd "$HOME/Documents/personal/SkylineBench"
git add mod/src/bridge/BuildingProblems.cs mod/src/bridge/GameReads.cs mod/src/http/Handlers.cs mod/src/http/Router.cs
git commit -m "feat(mod): /problems read + shared building-problem name mapping"
```

> **Live verify (after a game run):** `GET /problems` returns created buildings carrying a problem with `{id,x,z,category,problems:[...]}`; `GET /metrics` services counts equal the per-name tallies from `/problems` (e.g. number of `road_not_connected` entries == `services.road_not_connected`).

---

### Task 7c: (B) query_problems — broker contract, client, mock, service (TDD)

**Files:**
- Modify: `broker/src/contract.rs` (new `ProblemBuilding`/`Problems` + round-trip test)
- Modify: `broker/src/bridge_client.rs` (new `problems()`)
- Modify: `broker/src/mock.rs` (new `/problems` endpoint + route)
- Modify: `broker/src/service.rs` (new `QueryProblemsArgs` + `query_problems` + tests)

- [ ] **Step 1: Add the contract types**

In `broker/src/contract.rs`, after the `Zones` definitions (around line 98), add:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProblemBuilding {
    pub id: u32,
    pub x: f32,
    pub z: f32,
    pub category: String,
    /// Normalised problem names, e.g. "road_not_connected", "no_fuel", "abandoned".
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Problems {
    pub buildings: Vec<ProblemBuilding>,
}
```

Add a round-trip test in `contract.rs` `mod tests`:

```rust
    #[test]
    fn problems_round_trips() {
        let p = Problems {
            buildings: vec![ProblemBuilding {
                id: 7, x: 1.0, z: 2.0, category: "residential".into(),
                problems: vec!["road_not_connected".into(), "no_fuel".into()],
            }],
        };
        let json = serde_json::to_string(&p).unwrap();
        let parsed: Problems = serde_json::from_str(&json).unwrap();
        assert_eq!(p, parsed);
    }
```

- [ ] **Step 2: Add the bridge client method**

In `broker/src/bridge_client.rs`, next to `zones()` (around line 76), add:

```rust
    pub async fn problems(&self) -> Result<Problems, BridgeError> {
        self.get_json("/problems").await
    }
```

(`use crate::contract::*;` at the top already brings `Problems` into scope.)

- [ ] **Step 3: Add the mock endpoint**

In `broker/src/mock.rs`, add the handler (next to `buildings`):

```rust
async fn problems(State(_s): State<MockState>) -> Json<Problems> {
    Json(Problems {
        buildings: vec![
            ProblemBuilding { id: 11, x: 100.0, z: 100.0, category: "residential".into(), problems: vec!["road_not_connected".into()] },
            ProblemBuilding { id: 12, x: 200.0, z: 50.0, category: "service".into(), problems: vec!["no_fuel".into()] },
        ],
    })
}
```

And register the route in `router()` (after `/zones`):

```rust
        .route("/problems", get(problems))
```

- [ ] **Step 4: Add the service function + args**

In `broker/src/service.rs`, add (e.g. after `query_segments`):

```rust
#[derive(Deserialize, schemars::JsonSchema)]
pub struct QueryProblemsArgs {
    /// Keep only buildings that currently have this problem (e.g. "road_not_connected").
    #[serde(default)]
    pub filter: Option<String>,
    /// Keep only buildings inside this rectangle.
    #[serde(default)]
    pub bounds: Option<Bounds>,
}

pub async fn query_problems(
    client: &BridgeClient,
    args: QueryProblemsArgs,
) -> Result<Value, ServiceError> {
    let p = client.problems().await?;
    let buildings: Vec<&crate::contract::ProblemBuilding> = p
        .buildings
        .iter()
        .filter(|b| {
            args.filter
                .as_deref()
                .is_none_or(|f| b.problems.iter().any(|pr| pr == f))
        })
        .filter(|b| {
            args.bounds
                .is_none_or(|bd| in_bounds(Position { x: b.x, y: 0.0, z: b.z }, bd))
        })
        .collect();
    let total = buildings.len();
    Ok(json!({ "buildings": buildings, "total_matching": total }))
}
```

(`in_bounds` and `Position` are already imported at the top of `service.rs`.)

- [ ] **Step 5: Add service tests**

In `broker/src/service.rs` `mod tests`, add:

```rust
    #[tokio::test]
    async fn query_problems_lists_problem_buildings() {
        let c = client().await;
        let v = query_problems(&c, QueryProblemsArgs { filter: None, bounds: None }).await.unwrap();
        assert_eq!(v["total_matching"], 2);
        assert!(v["buildings"][0]["problems"].is_array());
    }

    #[tokio::test]
    async fn query_problems_filters_by_problem_name() {
        let c = client().await;
        let v = query_problems(&c, QueryProblemsArgs { filter: Some("no_fuel".into()), bounds: None })
            .await
            .unwrap();
        assert_eq!(v["total_matching"], 1);
        assert_eq!(v["buildings"][0]["id"], 12);
    }

    #[tokio::test]
    async fn query_problems_filters_by_bounds() {
        let c = client().await;
        let v = query_problems(
            &c,
            QueryProblemsArgs {
                filter: None,
                bounds: Some(crate::contract::Bounds { min_x: 150.0, min_z: 0.0, max_x: 250.0, max_z: 100.0 }),
            },
        )
        .await
        .unwrap();
        assert_eq!(v["total_matching"], 1);
        assert_eq!(v["buildings"][0]["id"], 12);
    }
```

- [ ] **Step 6: Run broker tests + commit**

Run: `cd broker && cargo test`
Expected: PASS (contract round-trip + 3 query_problems tests + existing).

```bash
cd "$HOME/Documents/personal/SkylineBench"
git add broker/src/contract.rs broker/src/bridge_client.rs broker/src/mock.rs broker/src/service.rs
git commit -m "feat(broker): query_problems service + /problems client/mock/contract"
```

---

### Task 7d: (B) query_problems — MCP tool

**Files:**
- Modify: `broker/src/tools.rs` (import, tool, registration test)

- [ ] **Step 1: Import the args type**

In `broker/src/tools.rs`, add `QueryProblemsArgs` to the `use crate::service::{...}` list (alphabetical, after `QuerySegmentsArgs`).

- [ ] **Step 2: Add the tool**

In `broker/src/tools.rs`, after the `query_segments` tool, add:

```rust
    #[tool(
        description = "Locate the specific buildings behind a problem-count spike (the leading \
            death-spiral signal in get_metrics `services`): which buildings lost road access or a \
            utility, and where (id + position + problem list). Optional `filter` (a single problem \
            name like \"road_not_connected\") and `bounds`."
    )]
    async fn query_problems(
        &self,
        Parameters(args): Parameters<QueryProblemsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match service::query_problems(&self.client, args).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }
```

- [ ] **Step 3: Update the registration test**

In `broker/src/tools.rs`, the `registers_all_tools` test — insert `"query_problems"` into the sorted `vec![...]` (after `query_segments`... no: alphabetical order is `query_problems` then `query_segments`, so insert `"query_problems"` BEFORE `"query_segments"`):

```rust
                "observe_area",
                "query_problems",
                "query_segments",
                "render_map",
```

- [ ] **Step 4: Run broker tests + commit**

Run: `cd broker && cargo test`
Expected: PASS (the registration test now lists 17 tools: the original 15 + `validate_road` + `query_problems`).

```bash
cd "$HOME/Documents/personal/SkylineBench"
git add broker/src/tools.rs
git commit -m "feat(broker): expose query_problems MCP tool"
```

> **Live verify (after a game run):** drive `get_metrics` until a `services` problem count is non-zero, then `query_problems` (optionally `filter` to that problem) returns matching buildings with positions an agent can act on; `query_problems` count for that name matches the metric.

---

## Final verification

- [ ] `cd broker && cargo test` → all green.
- [ ] `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe` → `0 failed`.
- [ ] `cd mod && ./build.sh` → build succeeds.
- [ ] Live-game pass (operator) covering the four "Live verify" notes above: zone round-trip (D), bulldoze bad id (G), `/problems` ↔ metrics consistency (B), and a `validate_road` dry-run that does not create a segment (F).

## Notes for the executor

- **Deferred:** spec item A (`colliding_buildings`) is intentionally not in this plan; the field stays declared-but-empty until the follow-up in the investigation doc.
- **Live-only items (B, D, F-game-side, G):** the mod's game-coupled code can't run in the pure test harness; treat a clean `./build.sh` plus the documented live-verify note as the bar, and surface the live checks to the operator.
- Keep functions in the `(dependencies) => (arguments)` spirit already used here (free functions taking `&BridgeClient` first); match the surrounding style in each file.
