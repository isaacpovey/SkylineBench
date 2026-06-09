using System;
using System.Collections.Generic;
using SkylineBench.Json;

namespace SkylineBench.Tests
{
    public static class JsonReaderTests
    {
        public static void Register(List<KeyValuePair<string, Action>> tests)
        {
            tests.Add(new KeyValuePair<string, Action>("reader: object fields", ObjectFields));
            tests.Add(new KeyValuePair<string, Action>("reader: nested object + number", Nested));
            tests.Add(new KeyValuePair<string, Action>("reader: array", Arr));
            tests.Add(new KeyValuePair<string, Action>("reader: escapes + bool", Escapes));
        }

        static void ObjectFields()
        {
            var v = JsonReader.Parse("{\"op\":\"step\",\"ticks\":256}");
            Assert.Equal("step", v["op"].AsString());
            Assert.Equal(256.0, v["ticks"].AsDouble());
        }

        static void Nested()
        {
            var v = JsonReader.Parse("{\"start\":{\"x\":-50.5,\"y\":0,\"z\":12}}");
            Assert.Equal(-50.5, v["start"]["x"].AsDouble());
            Assert.Equal(12.0, v["start"]["z"].AsDouble());
        }

        static void Arr()
        {
            var v = JsonReader.Parse("{\"ids\":[1,2,3]}");
            Assert.True(v["ids"].Count == 3, "array length");
            Assert.Equal(2.0, v["ids"][1].AsDouble());
        }

        static void Escapes()
        {
            var v = JsonReader.Parse("{\"name\":\"a\\\"b\",\"snap\":true}");
            Assert.Equal("a\"b", v["name"].AsString());
            Assert.True(v["snap"].AsBool(), "snap true");
        }
    }
}
