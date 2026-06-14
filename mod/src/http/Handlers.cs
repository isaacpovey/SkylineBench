using System;
using SkylineBench.Bridge;
using SkylineBench.Json;

namespace SkylineBench.Http
{
    public static class Handlers
    {
        public static HttpReply Health()
        {
            var h = GameAccess.ReadHealth();
            var w = new JsonWriter();
            w.BeginObject()
                .Name("mod_version").Value("0.1.0")
                .Name("game_version").Value(h.GameVersion)
                .Name("city_loaded").Value(h.CityLoaded)
                .Name("paused").Value(h.Paused)
                .Name("forced_paused").Value(h.ForcedPaused)
                .Name("tick").Value((long)h.Tick)
             .EndObject();
            return HttpReply.Json(200, w.ToString());
        }

        public static HttpReply Probe()
        {
            return HttpReply.Json(200, SkylineBench.Probe.BuildDump());
        }

        public static HttpReply Network() { return HttpReply.Json(200, Serialize.Network(GameReads.Network())); }
        public static HttpReply Buildings() { return HttpReply.Json(200, Serialize.Buildings(GameReads.Buildings())); }
        public static HttpReply Zones() { return HttpReply.Json(200, Serialize.Zones(GameReads.Zones())); }
        public static HttpReply Metrics() { return HttpReply.Json(200, Serialize.Metrics(GameReads.Metrics())); }

        public static HttpReply RoadTypes()
        {
            var w = new JsonWriter(); w.BeginObject().Name("road_types").BeginArray();
            foreach (var r in Prefabs.Roads())
            {
                w.BeginObject().Name("name").Value(r.Name).Name("construction_cost").Value(r.ConstructionCost).EndObject();
            }
            w.EndArray().EndObject(); return HttpReply.Json(200, w.ToString());
        }

        public static HttpReply ZoneTypes()
        {
            var w = new JsonWriter(); w.BeginObject().Name("zone_types").BeginArray();
            foreach (var z in new string[] { "residential", "residential_high", "commercial", "commercial_high", "industrial", "office" }) w.Value(z);
            w.EndArray().EndObject(); return HttpReply.Json(200, w.ToString());
        }

        public static HttpReply BuildRoad(string body) { return HttpReply.Json(200, Serialize.Action(GameActions.BuildRoad(RequestParse.BuildRoad(JsonReader.Parse(body))))); }
        public static HttpReply ValidateRoad(string body) { return HttpReply.Json(200, Serialize.Action(GameActions.ValidateRoad(RequestParse.BuildRoad(JsonReader.Parse(body))))); }
        public static HttpReply Bulldoze(string body) { return HttpReply.Json(200, Serialize.Action(GameActions.Bulldoze(RequestParse.Bulldoze(JsonReader.Parse(body))))); }
        public static HttpReply UpgradeRoad(string body) { return HttpReply.Json(200, Serialize.Action(GameActions.UpgradeRoad(RequestParse.UpgradeRoad(JsonReader.Parse(body))))); }
        public static HttpReply SetZone(string body) { return HttpReply.Json(200, Serialize.Action(GameActions.SetZone(RequestParse.SetZone(JsonReader.Parse(body))))); }
        public static HttpReply Clock(string body) { return HttpReply.Json(200, Serialize.Clock(GameActions.Clock(RequestParse.Clock(JsonReader.Parse(body))))); }
        public static HttpReply LoadSave(string body) { return HttpReply.Json(200, Serialize.Load(SaveLoader.Load(RequestParse.LoadSave(JsonReader.Parse(body)).SaveName))); }
        public static HttpReply Saves() { return HttpReply.Json(200, Serialize.Saves(SaveLoader.ListSaves())); }

        public static HttpReply Preview(string body)
        {
            var req = RequestParse.Preview(JsonReader.Parse(body));
            CaptureBehaviour.RunOnMain(delegate
            {
                var ghosts = new System.Collections.Generic.List<PreviewRenderer.Ghost>();
                foreach (var op in req.Ops)
                {
                    var prefab = Prefabs.FindRoad(op.Prefab);
                    if (prefab == null) continue;
                    ghosts.Add(PreviewRenderer.MakeGhost(prefab, op.StartX, op.StartZ, op.EndX, op.EndZ, op.FromElevation, op.ToElevation));
                }
                PreviewRenderer.SetGhosts(ghosts);
                PreviewRenderer.Ensure();
                PreviewRenderer.Active = true;
            }, 8000);
            var w = new JsonWriter();
            w.BeginObject().Name("ok").Value(true).Name("active").Value(true).EndObject();
            return HttpReply.Json(200, w.ToString());
        }

        public static HttpReply PreviewClear(string body)
        {
            CaptureBehaviour.RunOnMain(delegate { PreviewRenderer.Active = false; }, 8000);
            var w = new JsonWriter();
            w.BeginObject().Name("ok").Value(true).Name("active").Value(false).EndObject();
            return HttpReply.Json(200, w.ToString());
        }

        public static HttpReply Screenshot(string body)
        {
            var req = RequestParse.Screenshot(JsonReader.Parse(body));
            try
            {
                byte[] png = CaptureBehaviour.Capture(req.X, req.Z, req.Size, req.Yaw, req.Pitch, req.InfoView, 5000);
                return HttpReply.Png(png);
            }
            catch (Exception e)
            {
                var w = new JsonWriter();
                w.BeginObject().Name("error").Value("capture_failed").Name("message").Value(e.Message).EndObject();
                return HttpReply.Json(500, w.ToString());
            }
        }

        public static HttpReply Flyby(string body)
        {
            var req = RequestParse.Flyby(JsonReader.Parse(body));
            try
            {
                var fb = new FlybyRequest
                {
                    Keyframes = req.Keyframes,
                    DurationS = req.DurationS,
                    CaptureFps = req.CaptureFps,
                    OutDir = req.OutDir,
                };
                CaptureBehaviour.Flyby(fb, (int)(req.DurationS * 1000) + 30000);
                var w = new JsonWriter();
                w.BeginObject().Name("ok").Value(true).EndObject();
                return HttpReply.Json(200, w.ToString());
            }
            catch (Exception e)
            {
                var w = new JsonWriter();
                w.BeginObject().Name("error").Value("flyby_failed").Name("message").Value(e.Message).EndObject();
                return HttpReply.Json(500, w.ToString());
            }
        }
    }
}
