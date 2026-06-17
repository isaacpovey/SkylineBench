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
    }
}
