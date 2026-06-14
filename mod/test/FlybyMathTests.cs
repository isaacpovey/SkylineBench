using System;
using System.Collections.Generic;
using SkylineBench.Bridge;
using UnityEngine;

namespace SkylineBench.Tests
{
    public static class FlybyMathTests
    {
        public static void Register(List<KeyValuePair<string, Action>> tests)
        {
            tests.Add(new KeyValuePair<string, Action>("flyby: catmull endpoints", Endpoints));
            tests.Add(new KeyValuePair<string, Action>("flyby: catmull midpoint", Midpoint));
        }

        static void Endpoints()
        {
            var pts = new Vector2[] { new Vector2(0, 0), new Vector2(10, 0), new Vector2(20, 0) };
            var a = FlybyMath.Sample(pts, 0f);
            var b = FlybyMath.Sample(pts, 1f);
            Assert.True(Mathf.Abs(a.x - 0f) < 0.001f, "u=0 is the first point");
            Assert.True(Mathf.Abs(b.x - 20f) < 0.001f, "u=1 is the last point");
        }

        static void Midpoint()
        {
            var pts = new Vector2[] { new Vector2(0, 0), new Vector2(10, 0), new Vector2(20, 0) };
            var m = FlybyMath.Sample(pts, 0.5f);
            Assert.True(Mathf.Abs(m.x - 10f) < 0.001f, "u=0.5 is the middle control point");
        }
    }
}
