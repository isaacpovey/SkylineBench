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
            tests.Add(new KeyValuePair<string, Action>("parse: screenshot", Screenshot));
            tests.Add(new KeyValuePair<string, Action>("parse: flyby", Flyby));
        }

        static void BuildRoad()
        {
            var r = RequestParse.BuildRoad(JsonReader.Parse(
                "{\"start\":{\"x\":-50,\"y\":0,\"z\":10},\"end\":{\"x\":50,\"y\":0,\"z\":10},\"prefab\":\"Basic Road\",\"snap_to_existing_nodes\":true}"));
            Assert.Equal(-50.0, r.StartX); Assert.Equal(50.0, r.EndX); Assert.Equal(10.0, r.StartZ);
            Assert.Equal("Basic Road", r.Prefab);
            Assert.True(r.Snap, "snap");

            var hi = RequestParse.BuildRoad(JsonReader.Parse(
                "{\"start\":{\"x\":0,\"y\":0,\"z\":0},\"end\":{\"x\":50,\"y\":0,\"z\":0},\"prefab\":\"Basic Road\",\"snap_to_existing_nodes\":true,\"from_elevation\":0,\"to_elevation\":12}"));
            Assert.Equal(0.0, hi.FromElevation);
            Assert.Equal(12.0, hi.ToElevation);
            // Missing fields default to 0.
            Assert.Equal(0.0, r.FromElevation);
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

        static void Screenshot()
        {
            var r = RequestParse.Screenshot(JsonReader.Parse(
                "{\"x\":-120.5,\"z\":340,\"size\":500,\"yaw\":90,\"pitch\":32,\"info_view\":\"traffic\"}"));
            Assert.Equal(-120.5, r.X); Assert.Equal(340.0, r.Z); Assert.Equal(500.0, r.Size);
            Assert.Equal(90.0, r.Yaw); Assert.Equal(32.0, r.Pitch);
            Assert.Equal("traffic", r.InfoView);

            var d = RequestParse.Screenshot(JsonReader.Parse("{\"x\":0,\"z\":0}"));
            Assert.Equal(1000.0, d.Size);
            Assert.Equal(90.0, d.Pitch);
            Assert.Equal(0.0, d.Yaw);
            Assert.Equal("none", d.InfoView);
        }

        static void Flyby()
        {
            var r = RequestParse.Flyby(JsonReader.Parse(
                "{\"keyframes\":[{\"x\":1,\"z\":2,\"yaw\":0,\"pitch\":32,\"size\":500},{\"x\":3,\"z\":4,\"yaw\":0,\"pitch\":32,\"size\":500}],\"duration_s\":6,\"capture_fps\":12,\"out_dir\":\"/tmp/fly\"}"));
            Assert.True(r.Keyframes.Length == 2, "two keyframes");
            Assert.Equal(1.0, r.Keyframes[0].X); Assert.Equal(4.0, r.Keyframes[1].Z);
            Assert.Equal(6.0, r.DurationS); Assert.True(r.CaptureFps == 12, "fps");
            Assert.Equal("/tmp/fly", r.OutDir);
        }
    }
}
