# Colliding-Buildings Feedback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** On an `OBJECT_COLLISION` build rejection, populate `colliding_buildings` with the ids of the buildings the road actually hit, so the agent stops doing blind coordinate trial-and-error.

**Architecture:** Mirror the query `NetTool.CreateNode` runs internally — build the same swept XZ quad + `[minY,maxY]` vertical band and call the engine's public `BuildingManager.OverlapQuad(...)` with a `ulong[] buildingMask` out-param (the engine writes every colliding building id into the mask), then read the set bits back to ids. Split into a **pure, unit-testable geometry helper** (`CollisionCorridor`, UnityEngine math only) and a **thin game-API caller** (`BuildingCollision`, live-verified). Wire the caller into `RoadBuilder.Run`'s existing failure branch. No wire-format or broker change — `ActionResultDto.CollidingBuildings`, `Serialize.Action`, and broker `contract.rs` already carry the field.

**Tech Stack:** C# (.NET 3.5 / Mono / Unity), Cities: Skylines modding API (`NetTool`, `BuildingManager`, `NetAI`, `Quad2`). Build via `./build.sh`; pure tests via `xbuild` + `mono`.

## Global Constraints

- **Research basis:** `docs/superpowers/research/2026-06-16-collision-locality-investigation.md` (RESOLVED section). Engine call: `BuildingManager.OverlapQuad(Quad2 quad, float minY, float maxY, ItemClass.CollisionType, ItemClass.Layer, ushort ignoreBuilding, ushort ignoreNode1, ushort ignoreNode2, ulong[] buildingMask)`. Mask bit convention: `mask[id >> 6] |= 1UL << (id & 0x3f)`.
- **Collision parameters come from the prefab's `m_netAI`** (verbatim, matches CreateNode): `GetCollisionHalfWidth()`, `GetCollisionType()`, `GetCollisionLayers()`. Y band: `minY = segY + NetInfo.m_minHeight`, `maxY = segY + NetInfo.m_maxHeight`.
- **Pillars are NOT build-time collision-tested** — ignore them entirely (research Q4).
- **Superset, never subset:** a simpler rectangular corridor (vs CreateNode's bezier-shaped quad) may report a few extra buildings but must never report fewer than the engine. Acceptable for feedback.
- **ids only, no positions:** the wire contract (`colliding_buildings: Vec<u32>`) is ids only; the agent resolves positions via `observe_area`. Do not change the wire format.
- **Sim-thread only:** `RoadBuilder.Run` already executes on the simulation thread (callers wrap in `SimThread.Run`); `BuildingManager` reads are safe there.
- **Test seam:** `mod/test/Tests.csproj` references `UnityEngine.dll` but **not** ColossalManaged. Unit-testable code may use `Vector2/Vector3/Mathf` only — never `Quad2`/`VectorUtils` (Colossal). Game-API code is verified in-game, not unit-tested (the broker mock has no buildings).
- **C# idiom:** match surrounding code (e.g. `RoadBuilder.NearestNode` uses `for` + locals). Do not impose JS/TS functional idioms; honor "objects not positional args" via input structs where natural.

---

### Task 1: Pure collision-corridor geometry (`CollisionCorridor`)

**Files:**
- Create: `mod/src/bridge/CollisionCorridor.cs`
- Create: `mod/test/CollisionCorridorTests.cs`
- Modify: `mod/test/Tests.csproj` (add the two files to the compile list)
- Modify: `mod/test/TestRunner.cs:38-46` (register the new tests)

**Interfaces:**
- Produces:
  - `struct CorridorInput { Vector3 Start; Vector3 End; float HalfWidth; float MinHeight; float MaxHeight; }`
  - `struct Corridor { Vector2 A, B, C, D; float MinY, MaxY; }` (XZ rectangle corners + vertical band)
  - `static Corridor CollisionCorridor.Compute(CorridorInput input)`

- [ ] **Step 1: Write the failing tests**

Create `mod/test/CollisionCorridorTests.cs`:

```csharp
using System;
using System.Collections.Generic;
using UnityEngine;
using SkylineBench.Bridge;

namespace SkylineBench.Tests
{
    public static class CollisionCorridorTests
    {
        public static void Register(List<KeyValuePair<string, Action>> tests)
        {
            tests.Add(new KeyValuePair<string, Action>("corridor: axis-aligned leg corners + Y band", AxisAligned));
            tests.Add(new KeyValuePair<string, Action>("corridor: elevated leg lifts the Y band", Elevated));
        }

        // Leg runs +X from (0,10,0) to (100,10,0), half-width 8, end pad = half-width.
        // perp is +Z, so x in [-8,108], z in [-8,8]; Y band = segY + [minHeight,maxHeight].
        private static void AxisAligned()
        {
            var c = CollisionCorridor.Compute(new CorridorInput
            {
                Start = new Vector3(0f, 10f, 0f),
                End = new Vector3(100f, 10f, 0f),
                HalfWidth = 8f, MinHeight = 0f, MaxHeight = 12f,
            });
            Assert.Equal(-8.0, c.A.x); Assert.Equal(-8.0, c.A.y);   // A = start-pad-side
            Assert.Equal(108.0, c.C.x); Assert.Equal(8.0, c.C.y);   // C = end+pad+side
            Assert.Equal(10.0, c.MinY);                             // 10 + 0
            Assert.Equal(22.0, c.MaxY);                             // 10 + 12
        }

        // Elevated ramp: Y band uses min/max of the endpoints' y plus the prefab band.
        private static void Elevated()
        {
            var c = CollisionCorridor.Compute(new CorridorInput
            {
                Start = new Vector3(0f, 30f, 0f),
                End = new Vector3(0f, 42f, 100f),
                HalfWidth = 8f, MinHeight = -1f, MaxHeight = 11f,
            });
            Assert.Equal(29.0, c.MinY);  // min(30,42) + (-1)
            Assert.Equal(53.0, c.MaxY);  // max(30,42) + 11
        }
    }
}
```

- [ ] **Step 2: Register the tests and add to the build**

In `mod/test/TestRunner.cs`, after the line `RoadErrorsTests.Register(tests);` (currently line 46), add:

```csharp
            CollisionCorridorTests.Register(tests);
```

In `mod/test/Tests.csproj`, inside the second `<ItemGroup>`, after the `RoadErrors.cs` line add the source under test, and after the `RoadErrorsTests.cs` line add the test file:

```xml
    <Compile Include="..\src\bridge\CollisionCorridor.cs"><Link>src\CollisionCorridor.cs</Link></Compile>
```
```xml
    <Compile Include="CollisionCorridorTests.cs" />
```

- [ ] **Step 3: Run tests to verify they fail to compile**

Run: `cd mod/test && xbuild Tests.csproj`
Expected: BUILD FAILED — `CollisionCorridor`/`CorridorInput`/`Corridor` not found (type does not exist yet).

- [ ] **Step 4: Write the minimal implementation**

Create `mod/src/bridge/CollisionCorridor.cs`:

```csharp
using UnityEngine;

namespace SkylineBench.Bridge
{
    /// <summary>Pure geometry for the swept collision corridor of a straight road
    /// leg: the XZ rectangle the road occupies plus the [MinY,MaxY] vertical band the
    /// engine tests buildings against. Mirrors NetTool.CreateNode's collision query
    /// (swept quad + segY+m_minHeight..segY+m_maxHeight). A plain rectangle is an
    /// intentional superset of CreateNode's bezier-shaped quad — it never reports fewer
    /// buildings than the engine. No Colossal types, so it is unit-testable. The broker
    /// pre-splits spans and the builder uses MaxSegments=1, so each leg is one straight
    /// segment.</summary>
    public struct CorridorInput
    {
        public Vector3 Start;
        public Vector3 End;
        public float HalfWidth;
        public float MinHeight;   // NetInfo.m_minHeight (collision band relative to road surface)
        public float MaxHeight;   // NetInfo.m_maxHeight
    }

    public struct Corridor
    {
        public Vector2 A, B, C, D; // XZ rectangle corners
        public float MinY, MaxY;
    }

    public static class CollisionCorridor
    {
        public static Corridor Compute(CorridorInput input)
        {
            var s = new Vector2(input.Start.x, input.Start.z);
            var e = new Vector2(input.End.x, input.End.z);
            Vector2 along = e - s;
            float len = along.magnitude;
            Vector2 dir = len > 1e-4f ? along / len : new Vector2(1f, 0f);
            Vector2 perp = new Vector2(-dir.y, dir.x);
            // Pad the ends by a half-width so a building sitting exactly at an endpoint
            // is still caught (superset safety).
            Vector2 s0 = s - dir * input.HalfWidth;
            Vector2 e0 = e + dir * input.HalfWidth;
            Vector2 side = perp * input.HalfWidth;
            return new Corridor
            {
                A = s0 - side, B = e0 - side, C = e0 + side, D = s0 + side,
                MinY = Mathf.Min(input.Start.y, input.End.y) + input.MinHeight,
                MaxY = Mathf.Max(input.Start.y, input.End.y) + input.MaxHeight,
            };
        }
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe`
Expected: PASS — output includes `ok   - corridor: axis-aligned leg corners + Y band` and `ok   - corridor: elevated leg lifts the Y band`, and the final line reports `0 failed`.

- [ ] **Step 6: Commit**

```bash
git add mod/src/bridge/CollisionCorridor.cs mod/test/CollisionCorridorTests.cs mod/test/Tests.csproj mod/test/TestRunner.cs
git commit -m "feat(mod): pure CollisionCorridor geometry for collision feedback"
```

---

### Task 2: Game-side colliding-building query (`BuildingCollision`)

**Files:**
- Create: `mod/src/bridge/BuildingCollision.cs`
- Build check: `mod/SkylineBenchMod.csproj` (compiled via `./build.sh`)

**Interfaces:**
- Consumes (Task 1): `CollisionCorridor.Compute(CorridorInput)` → `Corridor`.
- Produces: `static List<uint> BuildingCollision.Find(NetInfo prefab, Vector3 startPos, Vector3 endPos)` — building ids the leg collides with (empty list if none / prefab invalid).

> No unit test: this calls live game managers; the broker mock has no buildings. Correctness is established by the compile check (Step 2) and the live verify in Task 3. Keep the file dependency-light so the only risk is the game-API call itself.

- [ ] **Step 1: Write the implementation**

Create `mod/src/bridge/BuildingCollision.cs`:

```csharp
using System.Collections.Generic;
using ColossalFramework;
using ColossalFramework.Math;
using UnityEngine;

namespace SkylineBench.Bridge
{
    /// <summary>Lists the buildings a proposed road leg collides with by mirroring the
    /// query NetTool.CreateNode runs internally: build the same swept quad + [minY,maxY]
    /// band (CollisionCorridor) and call BuildingManager.OverlapQuad with a building
    /// bitmask out-param — the engine sets a bit per colliding building id — then read
    /// the set bits back to ids. Collision parameters come from the prefab's own m_netAI
    /// so the verdict matches the engine. Pillars are not build-time collision-tested, so
    /// they are intentionally ignored. Must run on the simulation thread (BuildingManager
    /// read). Verified in-game, not unit-tested (the broker mock has no buildings).</summary>
    public static class BuildingCollision
    {
        public static List<uint> Find(NetInfo prefab, Vector3 startPos, Vector3 endPos)
        {
            var result = new List<uint>();
            if (prefab == null || prefab.m_netAI == null) return result;

            var corridor = CollisionCorridor.Compute(new CorridorInput
            {
                Start = startPos,
                End = endPos,
                HalfWidth = prefab.m_netAI.GetCollisionHalfWidth(),
                MinHeight = prefab.m_minHeight,
                MaxHeight = prefab.m_maxHeight,
            });
            var quad = new Quad2 { a = corridor.A, b = corridor.B, c = corridor.C, d = corridor.D };

            var bm = Singleton<BuildingManager>.instance;
            int count = bm.m_buildings.m_buffer.Length;
            var mask = new ulong[(count + 63) / 64];
            bm.OverlapQuad(
                quad, corridor.MinY, corridor.MaxY,
                prefab.m_netAI.GetCollisionType(), prefab.m_netAI.GetCollisionLayers(),
                /*ignoreBuilding*/ (ushort)0, /*ignoreNode1*/ (ushort)0, /*ignoreNode2*/ (ushort)0,
                mask);

            for (uint id = 1; id < count; id++)
            {
                if ((mask[id >> 6] & (1UL << (int)(id & 0x3f))) != 0UL) result.Add(id);
            }
            return result;
        }
    }
}
```

- [ ] **Step 2: Build the mod to verify it compiles against the game API**

Run: `cd mod && ./build.sh`
Expected: BUILD SUCCEEDED, `SkylineBenchMod.dll` compiled and copied to the Addons/Mods path. This confirms `BuildingManager.OverlapQuad`, `NetAI.GetCollisionHalfWidth/Type/Layers`, `NetInfo.m_minHeight/m_maxHeight`, and `Quad2.a/b/c/d` resolve with the exact signatures used. Fix any type mismatch before continuing (e.g. if `GetCollisionLayers` is unavailable on the prefab variant, fall back to `prefab.m_class.m_layer`).

- [ ] **Step 3: Commit**

```bash
git add mod/src/bridge/BuildingCollision.cs
git commit -m "feat(mod): BuildingCollision.Find lists colliding building ids via OverlapQuad"
```

---

### Task 3: Wire colliding buildings into the failure response

**Files:**
- Modify: `mod/src/bridge/RoadBuilder.cs:58-59` (the `ToolErrors` failure branch)
- Build check: `mod/SkylineBenchMod.csproj` via `./build.sh`
- Live verify: in-game with the gridlock save

**Interfaces:**
- Consumes (Task 2): `BuildingCollision.Find(prefab, startPos, endPos)` → `List<uint>`.
- Consumes (existing): `ActionResultDto.CollidingBuildings` (`List<uint>`), `ErrorCode.ObjectCollision` (`"OBJECT_COLLISION"`), `RoadErrors.Reason(ulong)`. Serialization is already in place at `Serialize.cs:90` (`colliding_buildings` emitted on failure when non-empty); broker parses it at `contract.rs:248`.

- [ ] **Step 1: Modify the failure branch in `RoadBuilder.Run`**

In `mod/src/bridge/RoadBuilder.cs`, replace the current failure return (lines 58-59):

```csharp
            if (err != ToolBase.ToolErrors.None)
                return ActionResultDto.Fail(RoadErrors.Reason((ulong)err));
```

with:

```csharp
            if (err != ToolBase.ToolErrors.None)
            {
                string reason = RoadErrors.Reason((ulong)err);
                var fail = ActionResultDto.Fail(reason);
                if (reason == ErrorCode.ObjectCollision)
                    fail.CollidingBuildings = BuildingCollision.Find(prefab, startPos, endPos);
                return fail;
            }
```

(`prefab`, `startPos`, `endPos` are all already in scope above this branch — `prefab` at line 28, `startPos`/`endPos` at lines 42-43.)

- [ ] **Step 2: Build the mod**

Run: `cd mod && ./build.sh`
Expected: BUILD SUCCEEDED. (Pure-test suite is unaffected — `RoadBuilder.cs` is not in `Tests.csproj` — but optionally re-run `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe` to confirm `0 failed`.)

- [ ] **Step 3: Commit**

```bash
git add mod/src/bridge/RoadBuilder.cs
git commit -m "feat(mod): attach colliding_buildings to OBJECT_COLLISION rejections"
```

- [ ] **Step 4: Live verify in-game (manual — requires the running game)**

Cannot be unit-tested (no buildings in the broker mock). Verify against the gridlock save:

1. Launch the game with the rebuilt mod and load the gridlock save (the one where `apply_plan` was ~65% `OBJECT_COLLISION`).
2. Via the broker, issue a `build_road` (or `validate_road`) whose geometry deliberately runs a **ground** road through an existing building. Confirm the response is `{"ok":false,"reason":"OBJECT_COLLISION","colliding_buildings":[...]}` with a **non-empty** id list.
3. Cross-check: for each returned id, `observe_area` (or `/network`/building reads) at the building's position should show it sitting under the road corridor. No obviously-unrelated buildings far from the corridor should appear (a few adjacent ones are acceptable — superset).
4. **Elevated clearance check (guards against false positives):** issue an **elevated** road (high `from_elevation`/`to_elevation`) directly over the same building. If it clears (no `OBJECT_COLLISION`), good. If it does collide for another reason, confirm `colliding_buildings` is consistent with the Y band — an elevated span should not list a short building beneath it.
5. **Non-empty guarantee:** confirm that whenever `CreateNode` returns `OBJECT_COLLISION` for a building hit, `Find` returns ≥1 id. If you observe an `OBJECT_COLLISION` with an empty list, the corridor under-approximates — file it; the safe interim fix is to widen the corridor (larger end pad / sample sub-quads) so feedback is never empty.

Record the outcome (and a sample response) in `docs/superpowers/research/2026-06-16-collision-locality-investigation.md` under a new "Live verify" note.

---

## Self-Review

**Spec coverage** (against the research doc's recommended implementation, steps 1-5):
- Resolve variant/collision params from `m_netAI` → Task 2 Step 1 (`GetCollisionHalfWidth/Type/Layers`). ✓
- Build swept `Quad2`(s) expanded by half-width → Task 1 (`Compute`) + Task 2 (`Quad2` from corners). ✓ (single rectangle; curved-leg sub-quad sampling explicitly deferred as acceptable superset — Global Constraints + Task 3 Step 4.5 fallback.)
- `minY/maxY = segY + m_minHeight/m_maxHeight` → Task 1 `Compute`. ✓
- Allocate `ulong[]` mask sized to building buffer + call `BuildingManager.OverlapQuad` → Task 2. ✓
- Walk set bits → ids → populate `CollidingBuildings` → Task 2 (walk) + Task 3 (assign). ✓
- Wire format already carries the field → confirmed `Serialize.cs:90`, `contract.rs:248`; no change. ✓
- Live verify required → Task 3 Step 4. ✓
- **Out of scope (noted, not in this plan):** fix #2 best-effort `apply_plan` (`stop_on_error:false`) is a separate broker change.

**Placeholder scan:** No TBD/TODO/"handle edge cases"/"similar to Task N". All code blocks complete; one explicit, conditional fallback (`m_class.m_layer`) is given with its trigger, not as a placeholder.

**Type consistency:** `CorridorInput`/`Corridor`/`Compute` signatures identical across Task 1 (def) and Task 2 (use). `BuildingCollision.Find(NetInfo, Vector3, Vector3) → List<uint>` matches Task 3's call. `CollidingBuildings` is `List<uint>` (Dtos.cs:46) — `Find` returns `List<uint>`. `ErrorCode.ObjectCollision` is the string `"OBJECT_COLLISION"` returned by `RoadErrors.Reason` for bit `0x10`. Mask bit math (`id >> 6`, `1UL << (id & 0x3f)`) matches the engine's write convention exactly.

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-06-17-collision-feedback-buildings.md`.
