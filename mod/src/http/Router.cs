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
                default: return HttpReply.Json(404, "{\"error\":\"unknown_route\",\"path\":\"" + path + "\"}");
            }
        }

        private static HttpReply MethodNotAllowed() { return HttpReply.Json(405, "{\"error\":\"method_not_allowed\"}"); }
    }
}
