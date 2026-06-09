using System;
using System.Collections.Generic;
using System.Globalization;

namespace SkylineBench.Http
{
    /// <summary>Parses a URL query string into key/value pairs with typed getters.
    /// Pure; no game or System.Web dependency (System.Web is absent in the game's Mono profile).</summary>
    public sealed class HttpQuery
    {
        private readonly Dictionary<string, string> _pairs;
        private HttpQuery(Dictionary<string, string> p) { _pairs = p; }

        public static HttpQuery Parse(string query)
        {
            var d = new Dictionary<string, string>(StringComparer.Ordinal);
            if (!string.IsNullOrEmpty(query))
            {
                if (query[0] == '?') query = query.Substring(1);
                foreach (var part in query.Split('&'))
                {
                    if (part.Length == 0) continue;
                    int eq = part.IndexOf('=');
                    if (eq < 0) d[Decode(part)] = "";
                    else d[Decode(part.Substring(0, eq))] = Decode(part.Substring(eq + 1));
                }
            }
            return new HttpQuery(d);
        }

        public string Get(string key) { string v; return _pairs.TryGetValue(key, out v) ? v : null; }

        public float GetFloat(string key, float fallback)
        {
            string v = Get(key);
            float r;
            if (v != null && float.TryParse(v, NumberStyles.Float, CultureInfo.InvariantCulture, out r)) return r;
            return fallback;
        }

        private static string Decode(string s) { return Uri.UnescapeDataString(s); }
    }
}
