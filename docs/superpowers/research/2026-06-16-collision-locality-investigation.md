# Investigation (deferred): engine-accurate `colliding_buildings`

**Date:** 2026-06-16
**Status:** DEFERRED — to be picked up after the other MCP feedback/contract fixes
(items B–H of `docs/superpowers/specs/2026-06-16-mcp-feedback-contract-fixes-design.md`)
land.

## Goal

Populate `ActionResultDto.CollidingBuildings` / the broker `colliding_buildings` field
(currently declared and serialized but never filled) so that an `OBJECT_COLLISION`
rejection tells the agent *which* buildings the road hit, by id, with positions
resolvable via `observe_area`. This closes the [[collision-feedback-blind]] gap (≈65% of
`apply_plan` ops rejected `OBJECT_COLLISION` with zero positional info).

## Why this is harder than it looks — and why the obvious fix is wrong

The first-draft design proposed a flat 2-D corridor-overlap test (the start→end line
expanded by `prefab.m_halfWidth`, reusing `Frontage`'s 2-D distance-to-segment math).
**Rejected.** Roads in CS1 are three-dimensional:

- An **elevated/bridge** span can pass *over* a building with no collision at all — a 2-D
  overlap would report a false collision.
- Conversely, an overpass's **support pillars** touch down at discrete points along the
  span. Those pillars have their own footprints and DO collide with whatever is beneath
  them — at ground locations the segment centreline may not pass near. A 2-D corridor
  test centred on the road line would *miss* these real collisions.

So the colliding set depends on per-point elevation/clearance and on computed pillar
positions — i.e. it must mirror the engine's actual 3-D determination, not a planar
approximation.

## Current build/collision path (what exists today)

- `RoadBuilder.Run` (`mod/src/bridge/RoadBuilder.cs`) builds via
  `NetTool.CreateNode(prefab, startCp, midCp, endCp, …, test, visualize:false,
  autoFix:true, needMoney:false, …, out node, out segment, out cost, out prod)` and gets
  back a `ToolBase.ToolErrors` bitmask. `RoadErrors.Reason` (`mod/src/bridge/RoadErrors.cs`)
  maps the bits to a string code. The bitmask does **not** identify colliding objects.
- `PreviewRenderer` (`mod/src/bridge/PreviewRenderer.cs`) already drives the same
  `CreateNode(test:true, visualize:true)` path for ghost rendering, and the elevation
  work (`docs/superpowers/specs/2026-06-14-elevation-aware-road-tooling-design.md`)
  confirmed `CreateNode` auto-selects the elevated/bridge prefab variant **with pillars**.
- `Frontage.CountZonedBuildingsNear` is a deliberately 2-D approximation for the neutral
  "buildings fronting this span" count — NOT suitable as a collision oracle.

## Tooling (confirmed available 2026-06-16)

- `monodis` and `ikdasm` both on PATH (`/opt/homebrew/bin/...`); `mono` present.
- Local game assembly:
  `~/Library/Application Support/Steam/steamapps/common/Cities_Skylines/Cities.app/Contents/Resources/Data/Managed/Assembly-CSharp.dll`
- Prior reverse-engineering precedent in this repo: `SaveLoader.cs` (`LoadPanel.Load`) and
  `ZoneWriter.cs` (`ZoneBlock.RefreshZoning`) were both pinned via `monodis` on this dll.
  `docs/superpowers/2026-06-09-phase2-context.md` notes "disassembling beats guessing".

## Open questions to answer by disassembly (the follow-up's research checklist)

1. **`ToolBase.ToolErrors` bit values** — verify the constants `RoadErrors` already
   assumes: `ObjectCollision=0x10`, `SlopeTooSteep=0x200`, `HeightTooHigh=0x800`,
   `CannotBuildOnWater=0x2000`, `OutOfArea=0x20`, `TooManyConnections=0x40000`,
   `TooShort=0x100`, `InvalidShape=0x80`.
2. **Where building collision is computed** — disassemble `NetTool.CreateNode` and the
   `CheckBuildPosition` chain (`NetAI` + `RoadBaseAI`/`RoadAI`/`PlayerNetAI` overrides);
   identify the exact method/geometry that sets `ObjectCollision` for *building* overlaps
   (vs net-vs-net). Candidates: building-grid scan (`BuildingManager.m_buildingGrid` +
   `Building.m_nextGridBuilding`), `Quad2`/`Quad3` overlap, `Building.CalculateSegment`,
   `Building.OverlapQuad`.
3. **The 3-D/clearance comparison** — which heights are compared so an elevated road
   clears a building (`Building.m_height`, `BuildingInfo.m_collisionHeight`/`m_size.y`,
   `NetInfo.m_buildHeight`, segment/lane y, any clearance constant). Need the actual
   inequality to replicate it.
4. **Bridge/elevated pillars** — which `NetAI` fields define the pillar prefab + spacing
   /offset (`m_bridgePillarInfo`/`m_bridgePillarOffset`/`m_middlePillarInfo`/
   `m_middlePillarOffset`), how pillar world positions are computed along the bezier, and
   whether pillar footprints are part of the build's `ObjectCollision` pass or tested
   separately.
5. **A reusable overlap query** — is there a callable (public/internal) method that, given
   a quad/bounds, enumerates overlapping building ids? If not, document the minimal
   building-grid traversal (grid resolution constant, quad→cell mapping, `m_nextGridBuilding`
   walk) plus the per-building quad+height test from (2)–(3).

## Candidate implementation strategies (to decide during the follow-up)

- **(Preferred, pending research) Replicate the engine's building-grid query** using the
  real proposed-segment geometry (bezier + half-width + per-point y) plus computed pillar
  quads, then return exact ids. Most faithful; needs (2)–(5) pinned.
- **Reuse a callable game overlap method** if (5) finds one — least code, most accurate.
- **Build-for-real then roll back** (`test:false` → read collisions → `ReleaseSegment`) —
  rejected unless the above fail, because committing even briefly risks sim side effects.

## Integration point when resumed

`RoadBuilder.Run`, only on the `ObjectCollision` branch: compute the id set and put it in
`ActionResultDto.CollidingBuildings`. Serialization (`Serialize.cs:90`) and the broker
contract (`contract.rs:248`) already carry the field, so no wire-format change is needed —
only the mod-side computation. Cannot be unit-tested against the broker mock (no buildings);
correctness needs a live-game verify.

---

# Findings — RESOLVED 2026-06-17 (via `monodis` on Assembly-CSharp.dll)

Status flips from DEFERRED to **research-complete**. All five open questions are answered
below from the actual IL. Disassembly artifact: `monodis "$DLL" > /tmp/cs1-disasm/full.il`
(1,719,372 lines). IL line numbers cited are from that dump and are only reproducible
within a session — the IL offsets / class+method names are the durable references.

## Q1 — `ToolBase/ToolErrors` bit values: ALL CONFIRMED, zero drift

Enum def at IL class `ToolBase/ToolErrors` (`[Flags]`, `unsigned int64` backing). Every
constant `mod/src/bridge/RoadErrors.cs` assumes is correct:

| Name | Value | RoadErrors assumed | ✓ |
|------|-------|--------------------|---|
| `ObjectCollision` | `0x10` | `0x10` | ✓ |
| `OutOfArea` | `0x20` | `0x20` | ✓ |
| `InvalidShape` | `0x80` | `0x80` | ✓ |
| `TooShort` | `0x100` | `0x100` | ✓ |
| `SlopeTooSteep` | `0x200` | `0x200` | ✓ |
| `HeightTooHigh` | `0x800` | `0x800` | ✓ |
| `CannotBuildOnWater` | `0x2000` | `0x2000` | ✓ |
| `TooManyConnections` | `0x40000` | `0x40000` | ✓ |

Also useful (build path uses `needMoney:false`): `NotEnoughMoney=0x40`, `RaycastFailed=0x1`,
`Pending=0x2`, `CannotUpgrade=0x10000`, `AlreadyExists=0x20000`. No action needed on
`RoadErrors`.

## Q5 (answer first — it subsumes Q2) — the engine exposes a reusable building-overlap query

`BuildingManager.OverlapQuad(Quad2 quad, float minY, float maxY, ItemClass/CollisionType
collisionType, ItemClass/Layer layers, ushort ignoreBuilding, ushort ignoreNode1,
ushort ignoreNode2, ulong[] buildingMask) -> bool` is **public** and does exactly what we
need. Decoded body:

- Maps the quad's XZ AABB to building-grid cells and walks each cell's linked list.
  **Grid constants (pinned):** resolution **270×270**, cell size **64 m**, cell index
  `row*270 + col`, cell-from-world `cell = clamp( (coord ± 72)/64 + 135, 0, 269 )`
  (the `±72` pads by the max building half-extent so big buildings in neighbour cells are
  caught; `+135` = 270/2 centre). Grid array: `BuildingManager.m_buildingGrid` (`ushort[]`),
  link field `Building.m_nextGridBuilding`, building store `m_buildings.m_buffer`.
- Per building: skip if `Info==null`; if `layers!=0` skip when `(Info.m_class.m_layer &
  layers)==0`; skip if `IgnoreOverlap(id, ignoreBuilding, ignoreNode1, ignoreNode2)`; else
  call `Building.OverlapQuad(id, quad, minY, maxY, collisionType)` (Q3 below).
- **The `buildingMask` out-param is the prize:** when non-null, on every hit it sets
  `buildingMask[id >> 6] |= 1UL << (id & 0x3f)` and **keeps scanning** (collects ALL
  colliders); when null it early-returns on first hit. So passing a
  `ulong[ (BuildingManager.MAX_BUILDING_COUNT + 63) / 64 ]` mask yields the complete set of
  colliding building ids — iterate set bits to recover ids, then `m_buildings.m_buffer[id]
  .m_position` for the world position (already `observe_area`-resolvable).

This means **we do NOT hand-roll the grid traversal** (the deferred plan's fallback). We
call the engine's own method. The only thing we must reconstruct is the *query geometry*
(the swept quad + Y band), which Q2/Q3/Q4 nail down.

## Q2 — where building `ObjectCollision` is set in the net-build path

`NetTool.CreateNode` (public static, the exact method `RoadBuilder.Run` calls; IL class
`NetTool`, method `CreateNode`) builds the collision query itself. Per swept sub-segment it:

1. derives `collisionType = info.m_netAI.GetCollisionType()`,
   `layers = info.m_netAI.<collision-layers>()`,
   `halfWidth = info.m_netAI.GetCollisionHalfWidth()` — **all from `m_netAI`, on the
   already-selected elevated/bridge `NetInfo` variant** (loc 99). So elevation is baked into
   the inputs, not bolted on afterwards.
2. builds **two** `Quad2`s (`Quad2.XZ(...)`, locals 134/135) from the swept `Quad3` corridor
   expanded perpendicular by `halfWidth` (with the `0.8`/`0.6` directional shaping and end
   clip you see at IL ~`IL_2151`–`IL_2201`),
3. calls `NetManager.OverlapQuad(...)` (net-vs-net, result → `collidingSegmentBuffer` loc 18),
   then `BuildingManager.OverlapQuad(quad, minY, maxY, collisionType, layers,
   GetIgnoredBuilding(cp), cp.m_node, 0, collidingBuildingBuffer)` (net-vs-building, result
   → `collidingBuildingBuffer` loc 19).

The `bool` returns are `pop`ped — collisions accumulate into the two local buffers, and
`ObjectCollision` is raised later iff `collidingBuildingBuffer` is non-empty. **Critically,
loc 19 is a CreateNode-local `ulong[]` and is never returned** → we cannot read the engine's
own answer through the public `CreateNode` signature; we must re-run the query (next para).
`NetTool.TestNodeBuilding(..., ulong[] collidingSegmentBuffer, ulong[] collidingBuildingBuffer)`
is the *node*-occupancy variant (building sitting on the junction point) and is `private
static` — same buffer convention, also not exposed.

## Q3 — the 3-D clearance comparison (why elevated spans clear buildings)

`minY`/`maxY` passed to `OverlapQuad` (CreateNode locals 132/133) =
`segmentEndY + NetInfo.m_minHeight` / `segmentEndY + NetInfo.m_maxHeight`. `segmentEndY`
already carries elevation (the control point's y), and `m_minHeight`/`m_maxHeight` are the
prefab's vertical collision band relative to the road surface.

The per-building test `Building.OverlapQuad(id, quad, minY, maxY, collisionType)` does:

1. reject if `Width==0 || Length==0` (uninstantiated);
2. cheap AABB reject using radius `r = Min(72, (Width+Length)*4)` against the quad bounds;
3. compute the **building's** Y band: `bMin = m_position.y - m_baseHeight`,
   `bMax = m_position.y + BuildingInfo.m_collisionHeight` (with a special case: when the
   building's own collision type `== 4`, `bMin = m_position.y + generatedInfo.m_min.y`);
4. **`ItemClass.CheckCollisionType(bMin, bMax, minY, maxY, roadCollisionType,
   buildingCollisionType)`** — this is THE clearance gate: it requires the road's
   `[minY,maxY]` and the building's `[bMin,bMax]` to overlap *and* the collision types to be
   mutually blocking. An elevated road's band sits above a ground building's band → no Y
   overlap → returns false → no collision. This is exactly the "elevated passes over"
   behaviour, and it is decided here, not by any 2-D logic;
5. only if (4) passes does it build the building's oriented footprint quad (rotated by
   `m_angle`, sized `Width*4 × Length*4`, with placement-mode / `m_circular` tweaks) and
   return `quad.Intersect(buildingQuad)`.

So replicating clearance = supplying the right `minY`/`maxY` and `collisionType`; the
engine's `Building.OverlapQuad` handles the rest. **The first-draft flat-2-D corridor test
was correctly rejected** — but the fix is not to model 3-D ourselves, it's to feed the real
Y band into `BuildingManager.OverlapQuad`.

## Q4 — bridge pillars are NOT part of the build-time collision pass

Pillar fields live on the *bridge* AIs: `RoadBridgeAI.m_bridgePillarInfo` /
`m_middlePillarInfo` / `m_bridgePillarOffset` / `m_middlePillarOffset` (same set on
`MetroTrackBridgeAI`, etc.). **`NetTool.CreateNode`'s collision region contains zero pillar
references** — pillars are emitted as node-attached sub-buildings via
`*BridgeAI.GetNodeBuilding(nodeID, ...)` during segment/node creation and rendering, *after*
validation. They do **not** generate `ObjectCollision` at build time.

Consequence: the deferred doc's worry that "pillars touch down and cause collisions a
centreline test would miss" does **not** apply to build-time validation — the engine itself
does not test pillar footprints against buildings when deciding `ObjectCollision`. We can
ignore pillars entirely for the `colliding_buildings` computation and still match the
engine's verdict. (Pillars can still *visually* clip buildings post-build; that's a separate,
non-blocking concern and out of scope.)

## Decision — recommended implementation (supersedes the three candidate strategies above)

**Reconstruct the query geometry, call `BuildingManager.OverlapQuad` with our own
`buildingMask`.** No grid hand-rolling (Q5 gives the callable), no pillar math (Q4), no
build-and-rollback (Q3 gives clearance for free).

In `RoadBuilder.Run`, only on the `ObjectCollision` branch, for each leg:

1. `var ai = info.m_netAI;` → `collisionType = ai.GetCollisionType()`,
   `halfWidth = ai.GetCollisionHalfWidth()`, `layers = ai`'s collision layers,
   using the **same elevated/bridge `NetInfo` variant** `CreateNode` resolved (`PreviewRenderer`
   already obtains this variant — reuse that selection).
2. Build the swept `Quad2`(s) from the leg's control points expanded by `halfWidth`. A single
   straight-leg quad is the cheap correct case; for a curved/multi-`NodePosition` leg, sample
   the bezier into sub-quads exactly as `CreateNode`'s loop does (or N coarse sub-quads — a
   slight over-approximation only ever *adds* candidate ids, acceptable for feedback).
3. `minY = legMinY + info.m_minHeight`, `maxY = legMaxY + info.m_maxHeight`.
4. `var mask = new ulong[(BuildingManager.MAX_BUILDING_COUNT + 63) / 64];`
   `Singleton<BuildingManager>.instance.OverlapQuad(quad, minY, maxY, collisionType, layers,
   ignoreBuilding, ignoreNode, 0, mask);` (OR the masks across all sub-quads/legs).
5. Walk set bits → ids → `m_buildings.m_buffer[id].m_position` → populate
   `ActionResultDto.CollidingBuildings` (id + position). Wire format already carries it
   (`Serialize.cs:90`, broker `contract.rs:248`).

Faithfulness note: results match the engine's `ObjectCollision` set **as long as we feed the
same variant / `GetCollisionHalfWidth` / `m_min/maxHeight` / `collisionType`**. Differences
only arise from how finely we sample a curved leg (we may report a superset, never a subset —
the safe direction for agent feedback). Worth `assert`ing during live-verify that whenever the
real `CreateNode` returns `ObjectCollision`, our pass returns ≥1 building (else fall back to
listing buildings whose grid cells the corridor crosses, so feedback is never empty).

Still requires a **live-game verify** (broker mock has no buildings).
