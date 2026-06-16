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
            Assert.Equal("TOO_SHORT", RoadErrors.Reason(0x100UL));
            Assert.Equal("INVALID_SHAPE", RoadErrors.Reason(0x80UL));
            Assert.Equal("UNKNOWN", RoadErrors.Reason(0x10000000UL)); // Unmapped tail
        }
    }
}
