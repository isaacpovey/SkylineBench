# Task 1: CollisionCorridor Test Coverage Gaps

## Fix Summary
Added missing corner assertions in `mod/test/CollisionCorridorTests.cs`:

**AxisAligned test**: Added B=(108,-8) and D=(-8,8) assertions to cover all four corners of the XZ rectangle.

**Elevated test**: Added A=(8,-8) XZ geometry assertion to validate perpendicular direction (-X) on the +Z leg.

All expected values derived from `Compute` math: directional vector rotation, end-padding, and perpendicular offset.

## Verification
```
cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe
```
Result: **44 passed, 0 failed** (includes `corridor: axis-aligned leg corners + Y band` and `corridor: elevated leg lifts the Y band`)

## Commit
`b606dd4 test(CollisionCorridor): assert corners B,D in AxisAligned and A in Elevated`
