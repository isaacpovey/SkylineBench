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
