using System;
using System.Collections.Generic;
using SkylineBench.Http;

namespace SkylineBench.Tests
{
    public static class HttpQueryTests
    {
        public static void Register(List<KeyValuePair<string, Action>> tests)
        {
            tests.Add(new KeyValuePair<string, Action>("query: parses pairs", Pairs));
            tests.Add(new KeyValuePair<string, Action>("query: missing key returns null", Missing));
            tests.Add(new KeyValuePair<string, Action>("query: empty string", Empty));
            tests.Add(new KeyValuePair<string, Action>("query: float helper", Floats));
        }

        static void Pairs()
        {
            var q = HttpQuery.Parse("min_x=-50.5&types=road");
            Assert.Equal("-50.5", q.Get("min_x"));
            Assert.Equal("road", q.Get("types"));
        }

        static void Missing()
        {
            var q = HttpQuery.Parse("a=1");
            Assert.True(q.Get("nope") == null, "missing key is null");
        }

        static void Empty()
        {
            var q = HttpQuery.Parse("");
            Assert.True(q.Get("a") == null, "empty query has no keys");
        }

        static void Floats()
        {
            var q = HttpQuery.Parse("min_x=-50.5&bad=xyz");
            Assert.Equal(-50.5, q.GetFloat("min_x", 0f));
            Assert.Equal(7.0, q.GetFloat("bad", 7f));     // unparseable -> default
            Assert.Equal(9.0, q.GetFloat("absent", 9f));  // missing -> default
        }
    }
}
