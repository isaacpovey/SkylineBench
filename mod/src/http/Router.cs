using UnityEngine;

namespace SkylineBench.Http
{
    public static class Log
    {
        public static void Info(string m) { Debug.Log("[SkylineBench] " + m); }
        public static void Error(string m) { Debug.LogError("[SkylineBench] " + m); }
    }

    public static class Router
    {
        public static HttpReply Route(string method, string path, HttpQuery query, string body)
        {
            switch (path)
            {
                case "/health": return method == "GET" ? Handlers.Health() : MethodNotAllowed();
                case "/probe":  return method == "GET" ? Handlers.Probe()  : MethodNotAllowed();
                case "/network": return method == "GET" ? Handlers.Network() : MethodNotAllowed();
                case "/buildings": return method == "GET" ? Handlers.Buildings() : MethodNotAllowed();
                case "/zones": return method == "GET" ? Handlers.Zones() : MethodNotAllowed();
                case "/metrics": return method == "GET" ? Handlers.Metrics() : MethodNotAllowed();
                case "/road-types": return method == "GET" ? Handlers.RoadTypes() : MethodNotAllowed();
                case "/zone-types": return method == "GET" ? Handlers.ZoneTypes() : MethodNotAllowed();
                case "/action/build-road": return method == "POST" ? Handlers.BuildRoad(body) : MethodNotAllowed();
                case "/action/validate-road": return method == "POST" ? Handlers.ValidateRoad(body) : MethodNotAllowed();
                case "/action/bulldoze": return method == "POST" ? Handlers.Bulldoze(body) : MethodNotAllowed();
                case "/action/upgrade-road": return method == "POST" ? Handlers.UpgradeRoad(body) : MethodNotAllowed();
                case "/action/set-zone": return method == "POST" ? Handlers.SetZone(body) : MethodNotAllowed();
                case "/clock": return method == "POST" ? Handlers.Clock(body) : MethodNotAllowed();
                case "/load-save": return method == "POST" ? Handlers.LoadSave(body) : MethodNotAllowed();
                default: return HttpReply.Json(404, "{\"error\":\"unknown_route\",\"path\":\"" + path + "\"}");
            }
        }

        private static HttpReply MethodNotAllowed() { return HttpReply.Json(405, "{\"error\":\"method_not_allowed\"}"); }
    }
}
