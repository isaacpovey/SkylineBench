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
            tests.Add(new KeyValuePair<string, Action>("corridor: zoned categories are safe to bulldoze", ZonedCategories));
            tests.Add(new KeyValuePair<string, Action>("corridor: offset shifts away from a left-side building", OffsetAwayFromLeft));
            tests.Add(new KeyValuePair<string, Action>("corridor: offset shifts away from a right-side building", OffsetAwayFromRight));
            tests.Add(new KeyValuePair<string, Action>("corridor: far obstacle needs no offset", OffsetZeroWhenClear));
            tests.Add(new KeyValuePair<string, Action>("corridor: both-sides pinch cannot clear all", CombinedPinch));
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
            Assert.Equal(108.0, c.B.x); Assert.Equal(-8.0, c.B.y);  // B = end+pad-side
            Assert.Equal(108.0, c.C.x); Assert.Equal(8.0, c.C.y);   // C = end+pad+side
            Assert.Equal(-8.0, c.D.x); Assert.Equal(8.0, c.D.y);    // D = start-pad+side
            Assert.Equal(10.0, c.MinY);                             // 10 + 0
            Assert.Equal(22.0, c.MaxY);                             // 10 + 12
        }

        // Elevated ramp: Y band uses min/max of the endpoints' y plus the prefab band.
        // Leg runs +Z from (0,30,0) to (0,42,100), half-width 8, so dir=(0,1) in XZ.
        // perp=(-1,0); A = (start-dir*8-perp*8) = (8,-8).
        private static void Elevated()
        {
            var c = CollisionCorridor.Compute(new CorridorInput
            {
                Start = new Vector3(0f, 30f, 0f),
                End = new Vector3(0f, 42f, 100f),
                HalfWidth = 8f, MinHeight = -1f, MaxHeight = 11f,
            });
            Assert.Equal(8.0, c.A.x); Assert.Equal(-8.0, c.A.y);    // A = start-pad-side in XZ
            Assert.Equal(29.0, c.MinY);  // min(30,42) + (-1)
            Assert.Equal(53.0, c.MaxY);  // max(30,42) + 11
        }

        private static void ZonedCategories()
        {
            Assert.True(CollisionCorridor.IsZonedCategory("residential"), "residential");
            Assert.True(CollisionCorridor.IsZonedCategory("commercial"), "commercial");
            Assert.True(CollisionCorridor.IsZonedCategory("industrial"), "industrial");
            Assert.True(CollisionCorridor.IsZonedCategory("office"), "office");
            Assert.True(!CollisionCorridor.IsZonedCategory("service"), "service must route around");
            Assert.True(!CollisionCorridor.IsZonedCategory("other"), "other must route around");
        }

        static CorridorInput Eastbound()
        {
            return new CorridorInput
            {
                Start = new Vector3(0f, 0f, 0f),
                End = new Vector3(100f, 0f, 0f),
                HalfWidth = 8f, MinHeight = 0f, MaxHeight = 12f,
            };
        }

        static Obstacle At(float x, float z, float size)
        {
            return new Obstacle
            {
                Position = new Vector3(x, 0f, z),
                FootprintWidth = size,
                FootprintLength = size,
            };
        }

        // +X road, perp = +Z (left). Building 10 m to the left of the centreline.
        // 16×16 footprint → circumradius = 8√2 ≈ 11.31; needed = 8+11.31+2 ≈ 21.31;
        // overlap ≈ 11.31. Shift away from left = −perp = −Z.
        private static void OffsetAwayFromLeft()
        {
            var off = CollisionCorridor.LateralOffset(Eastbound(), At(50f, 10f, 16f));
            Assert.True(Math.Abs(off.x) < 1e-4, "lateral shift should be along Z, got x=" + off.x);
            Assert.True(off.y < -8.0, "should shift south (away from +Z building), got z=" + off.y);
        }

        private static void OffsetAwayFromRight()
        {
            var off = CollisionCorridor.LateralOffset(Eastbound(), At(50f, -10f, 16f));
            Assert.True(Math.Abs(off.x) < 1e-4, "lateral shift should be along Z, got x=" + off.x);
            Assert.True(off.y > 8.0, "should shift north (away from −Z building), got z=" + off.y);
        }

        private static void OffsetZeroWhenClear()
        {
            var off = CollisionCorridor.LateralOffset(Eastbound(), At(50f, 80f, 16f));
            Assert.True(Math.Abs(off.x) < 1e-4 && Math.Abs(off.y) < 1e-4, "far building needs no shift");
        }

        private static void CombinedPinch()
        {
            var advice = CollisionCorridor.CombinedOffset(Eastbound(), new Obstacle[]
            {
                At(50f, 10f, 16f),
                At(50f, -10f, 16f),
            });
            Assert.True(!advice.ClearsAll, "pinch from both sides cannot be cleared by one shift");
            Assert.True(Math.Abs(advice.X) < 1e-4, "combined shift still along Z");
            Assert.True(Math.Abs(advice.Z) > 8.0, "should still propose the larger-side shift");
        }
    }
}
