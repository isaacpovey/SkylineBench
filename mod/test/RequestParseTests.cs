using System;
using System.Collections.Generic;
using SkylineBench.Json;

namespace SkylineBench.Tests
{
    public static class RequestParseTests
    {
        public static void Register(List<KeyValuePair<string, Action>> tests)
        {
            tests.Add(new KeyValuePair<string, Action>("parse: build-road", BuildRoad));
            tests.Add(new KeyValuePair<string, Action>("parse: clock step", Clock));
            tests.Add(new KeyValuePair<string, Action>("parse: set-zone rect", SetZone));
        }

        static void BuildRoad()
        {
            var r = RequestParse.BuildRoad(JsonReader.Parse(
                "{\"start\":{\"x\":-50,\"y\":0,\"z\":10},\"end\":{\"x\":50,\"y\":0,\"z\":10},\"prefab\":\"Basic Road\",\"snap_to_existing_nodes\":true}"));
            Assert.Equal(-50.0, r.StartX); Assert.Equal(50.0, r.EndX); Assert.Equal(10.0, r.StartZ);
            Assert.Equal("Basic Road", r.Prefab);
            Assert.True(r.Snap, "snap");
        }

        static void Clock()
        {
            var r = RequestParse.Clock(JsonReader.Parse("{\"op\":\"step\",\"ticks\":256}"));
            Assert.Equal("step", r.Op);
            Assert.True(r.Ticks == 256, "ticks");
        }

        static void SetZone()
        {
            var r = RequestParse.SetZone(JsonReader.Parse("{\"rect\":{\"min_x\":0,\"min_z\":0,\"max_x\":16,\"max_z\":16},\"zone_type\":\"residential\"}"));
            Assert.Equal(0.0, r.MinX); Assert.Equal(16.0, r.MaxZ);
            Assert.Equal("residential", r.ZoneType);
        }
    }
}
