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
                .Name("tick").Value((long)h.Tick)
             .EndObject();
            return HttpReply.Json(200, w.ToString());
        }

        public static HttpReply Probe()
        {
            return HttpReply.Json(200, SkylineBench.Probe.BuildDump());
        }
    }
}
