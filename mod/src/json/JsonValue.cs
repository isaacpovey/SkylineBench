using System;
using System.Collections.Generic;
using System.Globalization;
using System.Text;

namespace SkylineBench.Json
{
    /// <summary>A parsed JSON value. Request bodies are small and known, so this
    /// is a deliberately minimal model: object, array, string, number, bool, null.</summary>
    public sealed class JsonValue
    {
        public enum Kind { Object, Array, String, Number, Bool, Null }

        public readonly Kind Type;
        private readonly Dictionary<string, JsonValue> _obj;
        private readonly List<JsonValue> _arr;
        private readonly string _str;
        private readonly double _num;
        private readonly bool _bool;

        private JsonValue(Kind t, Dictionary<string, JsonValue> o, List<JsonValue> a, string s, double n, bool b)
        { Type = t; _obj = o; _arr = a; _str = s; _num = n; _bool = b; }

        public static JsonValue Obj(Dictionary<string, JsonValue> o) { return new JsonValue(Kind.Object, o, null, null, 0, false); }
        public static JsonValue Ar(List<JsonValue> a) { return new JsonValue(Kind.Array, null, a, null, 0, false); }
        public static JsonValue Str(string s) { return new JsonValue(Kind.String, null, null, s, 0, false); }
        public static JsonValue Num(double n) { return new JsonValue(Kind.Number, null, null, null, n, false); }
        public static JsonValue Bln(bool b) { return new JsonValue(Kind.Bool, null, null, null, 0, b); }
        public static readonly JsonValue NullValue = new JsonValue(Kind.Null, null, null, null, 0, false);

        public JsonValue this[string key]
        {
            get { JsonValue v; if (_obj != null && _obj.TryGetValue(key, out v)) return v; return NullValue; }
        }
        public JsonValue this[int i] { get { return _arr[i]; } }
        public int Count { get { return _arr != null ? _arr.Count : 0; } }
        public bool Has(string key) { return _obj != null && _obj.ContainsKey(key); }

        public string AsString() { return _str; }
        public double AsDouble() { return _num; }
        public bool AsBool() { return _bool; }
        public bool IsNull { get { return Type == Kind.Null; } }
    }

    /// <summary>Minimal recursive-descent JSON parser for request bodies.</summary>
    public static class JsonReader
    {
        public static JsonValue Parse(string text)
        {
            int i = 0;
            JsonValue v = ParseValue(text, ref i);
            SkipWs(text, ref i);
            if (i != text.Length) throw new FormatException("Trailing JSON at " + i);
            return v;
        }

        private static JsonValue ParseValue(string s, ref int i)
        {
            SkipWs(s, ref i);
            char c = s[i];
            if (c == '{') return ParseObject(s, ref i);
            if (c == '[') return ParseArray(s, ref i);
            if (c == '"') return JsonValue.Str(ParseString(s, ref i));
            if (c == 't' || c == 'f') return ParseBool(s, ref i);
            if (c == 'n') { Expect(s, ref i, "null"); return JsonValue.NullValue; }
            return JsonValue.Num(ParseNumber(s, ref i));
        }

        private static JsonValue ParseObject(string s, ref int i)
        {
            var d = new Dictionary<string, JsonValue>();
            i++; SkipWs(s, ref i);
            if (s[i] == '}') { i++; return JsonValue.Obj(d); }
            while (true)
            {
                SkipWs(s, ref i);
                string key = ParseString(s, ref i);
                SkipWs(s, ref i);
                if (s[i] != ':') throw new FormatException("Expected ':' at " + i);
                i++;
                d[key] = ParseValue(s, ref i);
                SkipWs(s, ref i);
                char c = s[i++];
                if (c == '}') break;
                if (c != ',') throw new FormatException("Expected ',' or '}' at " + (i - 1));
            }
            return JsonValue.Obj(d);
        }

        private static JsonValue ParseArray(string s, ref int i)
        {
            var list = new List<JsonValue>();
            i++; SkipWs(s, ref i);
            if (s[i] == ']') { i++; return JsonValue.Ar(list); }
            while (true)
            {
                list.Add(ParseValue(s, ref i));
                SkipWs(s, ref i);
                char c = s[i++];
                if (c == ']') break;
                if (c != ',') throw new FormatException("Expected ',' or ']' at " + (i - 1));
            }
            return JsonValue.Ar(list);
        }

        private static string ParseString(string s, ref int i)
        {
            if (s[i] != '"') throw new FormatException("Expected string at " + i);
            i++;
            var sb = new StringBuilder();
            while (true)
            {
                char c = s[i++];
                if (c == '"') break;
                if (c == '\\')
                {
                    char e = s[i++];
                    switch (e)
                    {
                        case '"': sb.Append('"'); break;
                        case '\\': sb.Append('\\'); break;
                        case '/': sb.Append('/'); break;
                        case 'n': sb.Append('\n'); break;
                        case 'r': sb.Append('\r'); break;
                        case 't': sb.Append('\t'); break;
                        case 'b': sb.Append('\b'); break;
                        case 'f': sb.Append('\f'); break;
                        case 'u':
                            int code = int.Parse(s.Substring(i, 4), NumberStyles.HexNumber, CultureInfo.InvariantCulture);
                            sb.Append((char)code); i += 4; break;
                        default: throw new FormatException("Bad escape at " + (i - 1));
                    }
                }
                else sb.Append(c);
            }
            return sb.ToString();
        }

        private static double ParseNumber(string s, ref int i)
        {
            int start = i;
            while (i < s.Length && ("-+.eE0123456789".IndexOf(s[i]) >= 0)) i++;
            return double.Parse(s.Substring(start, i - start), CultureInfo.InvariantCulture);
        }

        private static JsonValue ParseBool(string s, ref int i)
        {
            if (s[i] == 't') { Expect(s, ref i, "true"); return JsonValue.Bln(true); }
            Expect(s, ref i, "false"); return JsonValue.Bln(false);
        }

        private static void Expect(string s, ref int i, string lit)
        {
            if (i + lit.Length > s.Length || s.Substring(i, lit.Length) != lit)
                throw new FormatException("Expected '" + lit + "' at " + i);
            i += lit.Length;
        }

        private static void SkipWs(string s, ref int i)
        {
            while (i < s.Length && (s[i] == ' ' || s[i] == '\t' || s[i] == '\n' || s[i] == '\r')) i++;
        }
    }
}
