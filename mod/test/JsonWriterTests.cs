using System;
using System.Collections.Generic;
using SkylineBench.Json;

namespace SkylineBench.Tests
{
    public static class JsonWriterTests
    {
        public static void Register(List<KeyValuePair<string, Action>> tests)
        {
            tests.Add(new KeyValuePair<string, Action>("writer: flat object", FlatObject));
            tests.Add(new KeyValuePair<string, Action>("writer: escaping", Escaping));
            tests.Add(new KeyValuePair<string, Action>("writer: numbers are invariant", Numbers));
            tests.Add(new KeyValuePair<string, Action>("writer: nested array of objects", Nested));
            tests.Add(new KeyValuePair<string, Action>("writer: bool and null", BoolNull));
        }

        static void FlatObject()
        {
            var w = new JsonWriter();
            w.BeginObject().Name("ok").Value(true).Name("tick").Value(42L).EndObject();
            Assert.Equal("{\"ok\":true,\"tick\":42}", w.ToString());
        }

        static void Escaping()
        {
            var w = new JsonWriter();
            w.BeginObject().Name("msg").Value("a\"b\\c\n").EndObject();
            Assert.Equal("{\"msg\":\"a\\\"b\\\\c\\n\"}", w.ToString());
        }

        static void Numbers()
        {
            var w = new JsonWriter();
            w.BeginObject().Name("x").Value(1.5).Name("z").Value(-50.0).EndObject();
            Assert.Equal("{\"x\":1.5,\"z\":-50}", w.ToString());
        }

        static void Nested()
        {
            var w = new JsonWriter();
            w.BeginObject().Name("nodes").BeginArray()
                .BeginObject().Name("id").Value(1L).EndObject()
                .BeginObject().Name("id").Value(2L).EndObject()
             .EndArray().EndObject();
            Assert.Equal("{\"nodes\":[{\"id\":1},{\"id\":2}]}", w.ToString());
        }

        static void BoolNull()
        {
            var w = new JsonWriter();
            w.BeginObject().Name("a").Value(false).Name("b").Null().EndObject();
            Assert.Equal("{\"a\":false,\"b\":null}", w.ToString());
        }
    }
}
