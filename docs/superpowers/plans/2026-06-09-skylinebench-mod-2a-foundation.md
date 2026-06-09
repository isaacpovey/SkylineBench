# SkylineBench Mod 2a — Foundation & Discovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **GAME-GATED PLAN — NOT FULLY AUTONOMOUS.** Tasks 1–9 are ordinary code/TDD (mostly Mono-buildable without the game). **Tasks 10 and 11 require the human to run Cities: Skylines on their Mac** (build/install the mod, enable it in the Content Manager, load a city, hit the probe). A subagent cannot complete those; it prepares the artifacts and the human runs them, then pastes results back.

**Goal:** Build the SkylineBench mod's foundation — a tested pure JSON/parsing library plus a minimal CS1 mod that loads, hosts a localhost HTTP server, answers `GET /health` and `GET /probe`, and dumps the unknown game-API surface — then run it in-game to produce `mod/DISCOVERY.md`, which unblocks Plan 2b (the contract endpoints).

**Architecture:** A single `net35` class-library DLL loaded by CS1 (per spec §2: layered HTTP/dispatch + `GameBridge` + pure helpers). This plan delivers the pure helpers (fully testable off-game), the HTTP/lifecycle/`SimThread` scaffolding using CONFIRMED APIs, and the discovery probe. Contract endpoints come in Plan 2b once `DISCOVERY.md` resolves the OPEN items.

**Tech Stack:** C# targeting **.NET Framework 3.5** (Unity Mono); references `ICities.dll`, `ColossalManaged.dll`, `Assembly-CSharp.dll`, `UnityEngine.dll` from the game's `Managed/` dir; `System.Net.HttpListener` for the server; Mono `msbuild` to compile on macOS; a zero-dependency console test runner for the pure helpers.

Implements spec `docs/superpowers/specs/2026-06-09-skylinebench-mod-design.md`. CONFIRMED API signatures come from `docs/superpowers/research/2026-06-09-cs1-modding-api.md` (the "API reference"). This plan covers the foundation + discovery only; contract endpoints + verification are Plan 2b.

---

## File structure

All paths under `mod/`.

| File | Responsibility |
|---|---|
| `SkylineBenchMod.csproj` | net35 class library producing `SkylineBenchMod.dll`; HintPaths into the game's `Managed/`. |
| `src/json/JsonWriter.cs` | Minimal JSON serializer (pure, no game deps). |
| `src/json/JsonValue.cs` | Parsed-JSON value model + `JsonReader` parser (pure). |
| `src/http/HttpQuery.cs` | Query-string parsing helper (pure). |
| `src/http/HttpServer.cs` | `HttpListener` wrapper: bind localhost, accept loop, hand requests to a dispatch callback. |
| `src/http/Router.cs` | method+path → handler dispatch; 404/405; turns handler output into an HTTP response. |
| `src/http/Handlers.cs` | `GET /health`, `GET /probe` handlers (this plan); contract handlers added in 2b. |
| `src/bridge/SimThread.cs` | Marshal a closure onto the sim thread and block for its result (CONFIRMED hooks). |
| `src/bridge/GameAccess.cs` | The seam to CS1 managers used *in this plan* (health + probe reads). `GameBridge` proper arrives in 2b. |
| `src/probe/Probe.cs` | Builds the discovery dump (resolves the OPEN items). |
| `src/Mod.cs` | `IUserMod` + `LoadingExtensionBase` + `ThreadingExtensionBase`: lifecycle + server start/stop. |
| `test/Tests.csproj` | net35 console exe; links `src/json/*.cs` + `src/http/HttpQuery.cs` (no game deps) + tests. |
| `test/TestRunner.cs` | Zero-dependency `Assert` helpers + `Main` that runs all tests, exits non-zero on failure. |
| `test/JsonWriterTests.cs`, `test/JsonReaderTests.cs`, `test/HttpQueryTests.cs` | Pure unit tests. |
| `build.sh` | macOS: locate game, check Mono, compile via msbuild, install DLL to the mods folder. |
| `README.md` | Build + install (Mac) + Content-Manager enable + how to run the probe. |
| `DISCOVERY.md` | **Output of Task 11** — resolves the OPEN items for Plan 2b. |

---

## Phase A — Pure helper library (TDD, no game required)

These compile and run under Mono with zero game dependencies, so they are ordinary TDD and a subagent can fully complete them.

### Task 1: Test project + runner harness

**Files:**
- Create: `mod/test/Tests.csproj`
- Create: `mod/test/TestRunner.cs`

- [ ] **Step 1: Create the test project**

Create `mod/test/Tests.csproj` — a net35 console exe that compiles the pure source files (by link) plus the test files. No game references.

```xml
<?xml version="1.0" encoding="utf-8"?>
<Project ToolsVersion="4.0" DefaultTargets="Build" xmlns="http://schemas.microsoft.com/developer/msbuild/2003">
  <PropertyGroup>
    <Configuration Condition=" '$(Configuration)' == '' ">Debug</Configuration>
    <Platform Condition=" '$(Platform)' == '' ">AnyCPU</Platform>
    <OutputType>Exe</OutputType>
    <RootNamespace>SkylineBench.Tests</RootNamespace>
    <AssemblyName>Tests</AssemblyName>
    <TargetFrameworkVersion>v3.5</TargetFrameworkVersion>
    <OutputPath>bin\$(Configuration)\</OutputPath>
  </PropertyGroup>
  <ItemGroup>
    <Reference Include="System" />
    <Reference Include="System.Core" />
  </ItemGroup>
  <ItemGroup>
    <!-- Pure source under test (no game deps) -->
    <Compile Include="..\src\json\JsonWriter.cs">
      <Link>src\JsonWriter.cs</Link>
    </Compile>
    <Compile Include="..\src\json\JsonValue.cs">
      <Link>src\JsonValue.cs</Link>
    </Compile>
    <Compile Include="..\src\http\HttpQuery.cs">
      <Link>src\HttpQuery.cs</Link>
    </Compile>
    <!-- Tests -->
    <Compile Include="TestRunner.cs" />
    <Compile Include="JsonWriterTests.cs" />
    <Compile Include="JsonReaderTests.cs" />
    <Compile Include="HttpQueryTests.cs" />
  </ItemGroup>
  <Import Project="$(MSBuildToolsPath)\Microsoft.CSharp.targets" />
</Project>
```

- [ ] **Step 2: Write the runner harness**

Create `mod/test/TestRunner.cs` — a zero-dependency assert + runner. Each test class exposes static `void`-returning methods; `Main` calls them, catches failures, prints a summary, exits 1 on any failure.

```csharp
using System;
using System.Collections.Generic;

namespace SkylineBench.Tests
{
    public static class Assert
    {
        public static void Equal(string expected, string actual)
        {
            if (!string.Equals(expected, actual, StringComparison.Ordinal))
                throw new Exception("Expected:\n  " + expected + "\nActual:\n  " + actual);
        }

        public static void Equal(double expected, double actual)
        {
            if (Math.Abs(expected - actual) > 1e-9)
                throw new Exception("Expected " + expected + " but got " + actual);
        }

        public static void True(bool cond, string msg)
        {
            if (!cond) throw new Exception("Expected true: " + msg);
        }
    }

    public static class TestRunner
    {
        public static int Main()
        {
            var tests = new List<KeyValuePair<string, Action>>();
            JsonWriterTests.Register(tests);
            JsonReaderTests.Register(tests);
            HttpQueryTests.Register(tests);

            int passed = 0, failed = 0;
            foreach (var t in tests)
            {
                try { t.Value(); Console.WriteLine("ok   - " + t.Key); passed++; }
                catch (Exception e) { Console.WriteLine("FAIL - " + t.Key + "\n      " + e.Message.Replace("\n", "\n      ")); failed++; }
            }
            Console.WriteLine(string.Format("\n{0} passed, {1} failed", passed, failed));
            return failed == 0 ? 0 : 1;
        }
    }
}
```

- [ ] **Step 3: Add temporary empty test registrars so it compiles**

The test classes don't exist yet. Create minimal stubs so Task 1 compiles in isolation; Tasks 2–4 replace them with real tests.

Create `mod/test/JsonWriterTests.cs`, `mod/test/JsonReaderTests.cs`, `mod/test/HttpQueryTests.cs`, each:

```csharp
using System;
using System.Collections.Generic;

namespace SkylineBench.Tests
{
    public static class JsonWriterTests // (and JsonReaderTests / HttpQueryTests)
    {
        public static void Register(List<KeyValuePair<string, Action>> tests) { }
    }
}
```

> The `Compile Include` for `JsonWriter.cs`/`JsonValue.cs`/`HttpQuery.cs` references files created in Tasks 2–4. To compile Task 1 alone, temporarily comment those three `<Compile>` links out, build, then uncomment as each file lands. Note this in the commit message.

- [ ] **Step 4: Build the test project**

Run: `cd mod/test && msbuild /p:Configuration=Debug Tests.csproj` (or `cd mod/test && mono $(which msbuild) Tests.csproj` — `build.sh` from Task 10 wraps this; for now invoke msbuild directly).
Expected: builds `bin/Debug/Tests.exe`. Run `mono bin/Debug/Tests.exe` → prints `0 passed, 0 failed`, exits 0.

> If `msbuild`/`mono` aren't installed yet: `brew install mono`. Task 10 automates the check.

- [ ] **Step 5: Commit**

```bash
git add mod/test/
git commit -m "test: add zero-dependency test runner harness for the mod's pure helpers"
```

### Task 2: JsonWriter (TDD)

**Files:**
- Create: `mod/src/json/JsonWriter.cs`
- Modify: `mod/test/JsonWriterTests.cs`

- [ ] **Step 1: Write failing tests**

Replace `mod/test/JsonWriterTests.cs`:

```csharp
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd mod/test && msbuild Tests.csproj` — expect a compile error (`SkylineBench.Json.JsonWriter` missing). That's the red state.

- [ ] **Step 3: Implement JsonWriter**

Create `mod/src/json/JsonWriter.cs`:

```csharp
using System;
using System.Globalization;
using System.Text;

namespace SkylineBench.Json
{
    /// <summary>Minimal fluent JSON serializer producing the contract's exact
    /// wire shapes. Commas/structure are managed automatically.</summary>
    public sealed class JsonWriter
    {
        private readonly StringBuilder _sb = new StringBuilder();
        private bool _needComma;

        private void Pre()
        {
            if (_needComma) _sb.Append(',');
            _needComma = false;
        }

        public JsonWriter BeginObject() { Pre(); _sb.Append('{'); return this; }
        public JsonWriter EndObject() { _sb.Append('}'); _needComma = true; return this; }
        public JsonWriter BeginArray() { Pre(); _sb.Append('['); return this; }
        public JsonWriter EndArray() { _sb.Append(']'); _needComma = true; return this; }

        public JsonWriter Name(string name)
        {
            Pre();
            WriteString(name);
            _sb.Append(':');
            return this;
        }

        public JsonWriter Value(string s)
        {
            Pre();
            if (s == null) _sb.Append("null"); else WriteString(s);
            _needComma = true;
            return this;
        }

        public JsonWriter Value(long n) { Pre(); _sb.Append(n.ToString(CultureInfo.InvariantCulture)); _needComma = true; return this; }

        public JsonWriter Value(double d)
        {
            Pre();
            // "R" round-trips; InvariantCulture avoids locale commas. Integers print without ".0".
            _sb.Append(d.ToString("R", CultureInfo.InvariantCulture));
            _needComma = true;
            return this;
        }

        public JsonWriter Value(bool b) { Pre(); _sb.Append(b ? "true" : "false"); _needComma = true; return this; }
        public JsonWriter Null() { Pre(); _sb.Append("null"); _needComma = true; return this; }

        private void WriteString(string s)
        {
            _sb.Append('"');
            foreach (char c in s)
            {
                switch (c)
                {
                    case '"': _sb.Append("\\\""); break;
                    case '\\': _sb.Append("\\\\"); break;
                    case '\n': _sb.Append("\\n"); break;
                    case '\r': _sb.Append("\\r"); break;
                    case '\t': _sb.Append("\\t"); break;
                    default:
                        if (c < ' ') _sb.Append("\\u").Append(((int)c).ToString("x4", CultureInfo.InvariantCulture));
                        else _sb.Append(c);
                        break;
                }
            }
            _sb.Append('"');
        }

        public override string ToString() { return _sb.ToString(); }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd mod/test && msbuild Tests.csproj && mono bin/Debug/Tests.exe`
Expected: the 5 writer tests print `ok`, summary `5 passed, 0 failed`, exit 0.

> Note: `double.ToString("R")` prints `1.5` and `-50` (no `.0`) which matches the assertions. If a future value needs fixed formatting, adjust here — but the broker parses with serde `f32`, which accepts both `-50` and `-50.0`.

- [ ] **Step 5: Commit**

```bash
git add mod/src/json/JsonWriter.cs mod/test/JsonWriterTests.cs
git commit -m "feat: add minimal JSON writer with tests"
```

### Task 3: JsonValue + JsonReader (TDD)

**Files:**
- Create: `mod/src/json/JsonValue.cs`
- Modify: `mod/test/JsonReaderTests.cs`

- [ ] **Step 1: Write failing tests**

Replace `mod/test/JsonReaderTests.cs`:

```csharp
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
```

- [ ] **Step 2: Run to verify failure**

Run: `cd mod/test && msbuild Tests.csproj` — expect compile error (`JsonReader`/`JsonValue` missing).

- [ ] **Step 3: Implement JsonValue + JsonReader**

Create `mod/src/json/JsonValue.cs`:

```csharp
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd mod/test && msbuild Tests.csproj && mono bin/Debug/Tests.exe`
Expected: writer + reader tests pass, summary `9 passed, 0 failed`, exit 0.

- [ ] **Step 5: Commit**

```bash
git add mod/src/json/JsonValue.cs mod/test/JsonReaderTests.cs
git commit -m "feat: add minimal JSON reader/value model with tests"
```

### Task 4: HttpQuery parsing (TDD)

**Files:**
- Create: `mod/src/http/HttpQuery.cs`
- Modify: `mod/test/HttpQueryTests.cs`

- [ ] **Step 1: Write failing tests**

Replace `mod/test/HttpQueryTests.cs`:

```csharp
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
            Assert.Equal(7.0, q.GetFloat("bad", 7f));     // unparseable → default
            Assert.Equal(9.0, q.GetFloat("absent", 9f));  // missing → default
        }
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd mod/test && msbuild Tests.csproj` — expect compile error (`SkylineBench.Http.HttpQuery` missing).

- [ ] **Step 3: Implement HttpQuery**

Create `mod/src/http/HttpQuery.cs`:

```csharp
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd mod/test && msbuild Tests.csproj && mono bin/Debug/Tests.exe`
Expected: `13 passed, 0 failed`, exit 0.

- [ ] **Step 5: Commit**

```bash
git add mod/src/http/HttpQuery.cs mod/test/HttpQueryTests.cs
git commit -m "feat: add query-string parsing helper with tests"
```

---

## Phase B — Mod foundation (CONFIRMED game APIs; builds against the game, runs in-game)

From here the code references game assemblies, so it builds only against the game's `Managed/` dir (Task 5 sets that up) and is exercised in-game (Tasks 10–11). The APIs used are all CONFIRMED in the API reference.

### Task 5: Mod project file + entry point + lifecycle

**Files:**
- Create: `mod/SkylineBenchMod.csproj`
- Create: `mod/src/Mod.cs`

- [ ] **Step 1: Create the csproj**

Create `mod/SkylineBenchMod.csproj`. `$(ManagedDLLPath)` defaults to the macOS bundle path and is overridable via an env/property (the spike confirms the literal path; this default matches the Unity convention).

```xml
<?xml version="1.0" encoding="utf-8"?>
<Project ToolsVersion="4.0" DefaultTargets="Build" xmlns="http://schemas.microsoft.com/developer/msbuild/2003">
  <PropertyGroup>
    <Configuration Condition=" '$(Configuration)' == '' ">Release</Configuration>
    <Platform Condition=" '$(Platform)' == '' ">AnyCPU</Platform>
    <OutputType>Library</OutputType>
    <RootNamespace>SkylineBench</RootNamespace>
    <AssemblyName>SkylineBenchMod</AssemblyName>
    <TargetFrameworkVersion>v3.5</TargetFrameworkVersion>
    <OutputPath>bin\$(Configuration)\</OutputPath>
  </PropertyGroup>
  <PropertyGroup Condition=" '$(ManagedDLLPath)' == '' ">
    <ManagedDLLPath>$(HOME)/Library/Application Support/Steam/steamapps/common/Cities_Skylines/Cities.app/Contents/Resources/Data/Managed</ManagedDLLPath>
  </PropertyGroup>
  <ItemGroup>
    <Reference Include="System" />
    <Reference Include="System.Core" />
    <Reference Include="ICities"><HintPath>$(ManagedDLLPath)/ICities.dll</HintPath><Private>false</Private></Reference>
    <Reference Include="ColossalManaged"><HintPath>$(ManagedDLLPath)/ColossalManaged.dll</HintPath><Private>false</Private></Reference>
    <Reference Include="Assembly-CSharp"><HintPath>$(ManagedDLLPath)/Assembly-CSharp.dll</HintPath><Private>false</Private></Reference>
    <Reference Include="UnityEngine"><HintPath>$(ManagedDLLPath)/UnityEngine.dll</HintPath><Private>false</Private></Reference>
  </ItemGroup>
  <ItemGroup>
    <Compile Include="src\json\JsonWriter.cs" />
    <Compile Include="src\json\JsonValue.cs" />
    <Compile Include="src\http\HttpQuery.cs" />
    <Compile Include="src\http\HttpServer.cs" />
    <Compile Include="src\http\Router.cs" />
    <Compile Include="src\http\Handlers.cs" />
    <Compile Include="src\bridge\SimThread.cs" />
    <Compile Include="src\bridge\GameAccess.cs" />
    <Compile Include="src\probe\Probe.cs" />
    <Compile Include="src\Mod.cs" />
  </ItemGroup>
  <Import Project="$(MSBuildToolsPath)\Microsoft.CSharp.targets" />
</Project>
```

> The `<Compile>` list references files from Tasks 5–9. As in Task 1, comment out not-yet-created entries to build incrementally; each task uncomments its file. `<Private>false</Private>` keeps the game DLLs out of the output (they exist at runtime).

- [ ] **Step 2: Implement the entry point + lifecycle**

Create `mod/src/Mod.cs`. Uses CONFIRMED APIs: `IUserMod`, `LoadingExtensionBase.OnLevelLoaded(LoadMode)`/`OnLevelUnloading`, and `ThreadingExtensionBase`/`IThreading` for the sim thread + clock.

```csharp
using ICities;
using UnityEngine;

namespace SkylineBench
{
    public sealed class Mod : IUserMod
    {
        public string Name { get { return "SkylineBench Bridge"; } }
        public string Description { get { return "Localhost HTTP bridge for the SkylineBench AI harness."; } }
    }

    /// <summary>Starts the HTTP server when a city/game loads; stops it on unload.
    /// LoadMode.LoadGame/NewGame are the only modes where the simulation managers exist.</summary>
    public sealed class SkylineLoading : LoadingExtensionBase
    {
        public override void OnLevelLoaded(LoadMode mode)
        {
            if (mode == LoadMode.LoadGame || mode == LoadMode.NewGame || mode == LoadMode.NewGameFromScenario)
            {
                Bridge.ModRuntime.Start();
            }
        }

        public override void OnLevelUnloading()
        {
            Bridge.ModRuntime.Stop();
        }
    }

    /// <summary>Runs on the simulation thread. Drives SimThread's queue drain and
    /// exposes IThreading (clock control + tick) to the rest of the mod.</summary>
    public sealed class SkylineThreading : ThreadingExtensionBase
    {
        public override void OnBeforeSimulationTick()
        {
            Bridge.ModRuntime.SetThreading(threadingManager);
            Bridge.SimThread.DrainOnSimThread();
        }
    }
}
```

> `threadingManager` is the `IThreading` provided by `ThreadingExtensionBase`. `ModRuntime` (Task 8) holds the server + the `IThreading` reference; `SimThread` (Task 8) defines `DrainOnSimThread()`. This file compiles once those land — build it last in Phase B, or stub `ModRuntime`/`Bridge.SimThread` temporarily.

- [ ] **Step 3: Commit (after Phase B compiles — see Task 8 Step 4)**

```bash
git add mod/SkylineBenchMod.csproj mod/src/Mod.cs
git commit -m "feat: add mod project file, IUserMod entry point, and lifecycle hooks"
```

### Task 6: HTTP server (HttpListener)

**Files:**
- Create: `mod/src/http/HttpServer.cs`

- [ ] **Step 1: Implement the server**

Create `mod/src/http/HttpServer.cs`. Binds `HttpListener` to `127.0.0.1:<port>`, runs an accept loop on a background thread, and hands each request to a dispatch delegate that returns `(int status, string contentType, string body)`.

```csharp
using System;
using System.IO;
using System.Net;
using System.Text;
using System.Threading;

namespace SkylineBench.Http
{
    public delegate HttpReply Dispatch(string method, string path, HttpQuery query, string body);

    public struct HttpReply
    {
        public int Status;
        public string ContentType;
        public string Body;
        public static HttpReply Json(int status, string body) { return new HttpReply { Status = status, ContentType = "application/json", Body = body }; }
        public static HttpReply Text(int status, string body) { return new HttpReply { Status = status, ContentType = "text/plain", Body = body }; }
    }

    public sealed class HttpServer
    {
        private readonly HttpListener _listener = new HttpListener();
        private readonly Dispatch _dispatch;
        private Thread _thread;
        private volatile bool _running;

        public HttpServer(int port, Dispatch dispatch)
        {
            _dispatch = dispatch;
            _listener.Prefixes.Add("http://127.0.0.1:" + port + "/");
        }

        public void Start()
        {
            _listener.Start();
            _running = true;
            _thread = new Thread(Loop) { IsBackground = true, Name = "SkylineBenchHttp" };
            _thread.Start();
            Log.Info("HTTP server listening on " + string.Join(",", new string[] { GetPrefix() }));
        }

        private string GetPrefix() { foreach (var p in _listener.Prefixes) return p; return ""; }

        public void Stop()
        {
            _running = false;
            try { _listener.Stop(); } catch { }
            try { _listener.Close(); } catch { }
        }

        private void Loop()
        {
            while (_running)
            {
                HttpListenerContext ctx;
                try { ctx = _listener.GetContext(); }
                catch { if (!_running) return; continue; }
                try { Handle(ctx); }
                catch (Exception e) { Log.Error("request handling failed: " + e); }
            }
        }

        private void Handle(HttpListenerContext ctx)
        {
            var req = ctx.Request;
            string body = "";
            if (req.HasEntityBody)
                using (var sr = new StreamReader(req.InputStream, req.ContentEncoding ?? Encoding.UTF8))
                    body = sr.ReadToEnd();

            string path = req.Url.AbsolutePath;
            var query = HttpQuery.Parse(req.Url.Query);

            HttpReply reply;
            try { reply = _dispatch(req.HttpMethod, path, query, body); }
            catch (Exception e) { reply = HttpReply.Text(500, "internal: " + e.Message); }

            byte[] buf = Encoding.UTF8.GetBytes(reply.Body ?? "");
            ctx.Response.StatusCode = reply.Status;
            ctx.Response.ContentType = reply.ContentType ?? "text/plain";
            ctx.Response.ContentLength64 = buf.Length;
            ctx.Response.OutputStream.Write(buf, 0, buf.Length);
            ctx.Response.OutputStream.Close();
        }
    }
}
```

> Uses a tiny `Log` helper (Task 7) wrapping the game's debug log. **`HttpListener` is OPEN** (may not work in CS1's Mono); the probe (Task 9) is the first thing that exercises it. If Task 10 shows it fails, Plan 2b adds a `TcpListener` implementation behind the same `Dispatch` delegate — no other code changes. That contingency is why dispatch is abstracted now.

- [ ] **Step 2: Build check deferred to Task 8** (the csproj needs all Phase-B files; build once they exist).

### Task 7: Router + Log + health handler

**Files:**
- Create: `mod/src/http/Router.cs`
- Create: `mod/src/http/Handlers.cs`

- [ ] **Step 1: Implement Log + Router**

Create `mod/src/http/Router.cs`:

```csharp
using UnityEngine;

namespace SkylineBench.Http
{
    public static class Log
    {
        public static void Info(string m) { Debug.Log("[SkylineBench] " + m); }
        public static void Error(string m) { Debug.LogError("[SkylineBench] " + m); }
    }

    /// <summary>Exact method+path dispatch. Unknown route → 404, wrong method → 405.</summary>
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
```

> `Log` uses `UnityEngine.Debug.Log` (always available). The game also has `ColossalFramework`-based logging, but `UnityEngine.Debug` is sufficient and lands in the Player.log — keep it minimal.

- [ ] **Step 2: Implement the health handler**

Create `mod/src/http/Handlers.cs`:

```csharp
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
            return HttpReply.Json(200, Probe.BuildDump());
        }
    }
}
```

> `GameAccess.ReadHealth()` (Task 8) and `Probe.BuildDump()` (Task 9) land next. The `/health` JSON shape matches the broker's `contract.rs` `Health` exactly (`mod_version`, `game_version`, `city_loaded`, `paused`, `tick`).

### Task 8: SimThread + GameAccess + ModRuntime

**Files:**
- Create: `mod/src/bridge/SimThread.cs`
- Create: `mod/src/bridge/GameAccess.cs`
- Modify: `mod/src/http/Router.cs` (add `ModRuntime` — or put it in its own file)

- [ ] **Step 1: Implement SimThread (sim-thread marshalling)**

Create `mod/src/bridge/SimThread.cs`. A thread-safe queue drained by `SkylineThreading.OnBeforeSimulationTick`; `Run` enqueues a closure and blocks on a `ManualResetEvent` with a timeout.

```csharp
using System;
using System.Collections.Generic;
using System.Threading;

namespace SkylineBench.Bridge
{
    public static class SimThread
    {
        private sealed class Job
        {
            public Action Work;
            public Exception Error;
            public readonly ManualResetEvent Done = new ManualResetEvent(false);
        }

        private static readonly Queue<Job> _queue = new Queue<Job>();
        private static readonly object _lock = new object();

        /// <summary>Enqueue work to run on the simulation thread; block until done (or timeout).
        /// CONFIRMED: ThreadingExtensionBase hooks run on the sim thread. PAUSED-drain behavior
        /// is verified by the probe; if queued work does not run while paused, ModRuntime briefly
        /// unpauses to flush (see Plan 2b / DISCOVERY.md).</summary>
        public static void Run(Action work, int timeoutMs)
        {
            var job = new Job { Work = work };
            lock (_lock) { _queue.Enqueue(job); }
            if (!job.Done.WaitOne(timeoutMs))
                throw new TimeoutException("sim-thread job timed out after " + timeoutMs + "ms");
            if (job.Error != null) throw job.Error;
        }

        public static T Run<T>(Func<T> work, int timeoutMs)
        {
            T result = default(T);
            Run(delegate { result = work(); }, timeoutMs);
            return result;
        }

        /// <summary>Called from the simulation thread (ThreadingExtensionBase) to execute queued jobs.</summary>
        public static void DrainOnSimThread()
        {
            while (true)
            {
                Job job;
                lock (_lock) { if (_queue.Count == 0) return; job = _queue.Dequeue(); }
                try { job.Work(); }
                catch (Exception e) { job.Error = e; }
                finally { job.Done.Set(); }
            }
        }
    }
}
```

- [ ] **Step 2: Implement GameAccess (health read) + ModRuntime**

Create `mod/src/bridge/GameAccess.cs`. For this plan it only reads health; the full `GameBridge` (contract reads/writes) is Plan 2b.

```csharp
using ICities;
using SkylineBench.Http;

namespace SkylineBench.Bridge
{
    public struct HealthInfo
    {
        public string GameVersion;
        public bool CityLoaded;
        public bool Paused;
        public uint Tick;
    }

    public static class GameAccess
    {
        public static HealthInfo ReadHealth()
        {
            var t = ModRuntime.Threading; // IThreading captured on the sim thread
            return new HealthInfo
            {
                GameVersion = BuildConfig.applicationVersion, // CONFIRM exact accessor in Task 10; fallback "unknown"
                CityLoaded = t != null,
                Paused = t != null && t.simulationPaused,
                Tick = t != null ? t.simulationTick : 0u
            };
        }
    }

    /// <summary>Holds the running HTTP server and the IThreading reference.</summary>
    public static class ModRuntime
    {
        private static HttpServer _server;
        public static IThreading Threading { get; private set; }

        public static void SetThreading(IThreading t) { Threading = t; }

        public static void Start()
        {
            if (_server != null) return;
            _server = new HttpServer(8787, Router.Route);
            _server.Start();
        }

        public static void Stop()
        {
            if (_server != null) { _server.Stop(); _server = null; }
            Threading = null;
        }
    }
}
```

> `BuildConfig.applicationVersion` is the conventional game-version accessor; **Task 10 confirms it** (if absent, return `"unknown"` — non-blocking for the contract). `IThreading.simulationPaused`/`simulationTick` are CONFIRMED. `ModRuntime` is referenced by `Mod.cs` (Task 5).

- [ ] **Step 3: Uncomment all Phase-B `<Compile>` entries** in `SkylineBenchMod.csproj` now that every referenced file exists.

- [ ] **Step 4: Build the mod DLL against the game assemblies**

Run (set the real Managed path if the default is wrong):
`cd mod && msbuild /p:Configuration=Release /p:ManagedDLLPath="$HOME/Library/Application Support/Steam/steamapps/common/Cities_Skylines/Cities.app/Contents/Resources/Data/Managed" SkylineBenchMod.csproj`
Expected: produces `bin/Release/SkylineBenchMod.dll` with no errors. (If the game isn't installed at that path, Task 10's `build.sh` handles detection; for a pure compile check you can point `ManagedDLLPath` at a copy of the four DLLs.)

> If a referenced game type/namespace is wrong (e.g. the logging namespace, `BuildConfig`), this is where it surfaces — fix the `using`/accessor against the real assemblies and note it. These are the only places this plan touches game types beyond the CONFIRMED set.

- [ ] **Step 5: Commit Phase B**

```bash
git add mod/SkylineBenchMod.csproj mod/src/
git commit -m "feat: add HTTP server, router, sim-thread marshalling, health endpoint, lifecycle"
```

---

## Phase C — Discovery probe

### Task 9: Probe.cs (dump the OPEN items)

**Files:**
- Create: `mod/src/probe/Probe.cs`

- [ ] **Step 1: Implement the probe dump**

Create `mod/src/probe/Probe.cs`. It builds a JSON report (and logs it) resolving the OPEN items from the API reference. It uses reflection to report whether candidate fields exist, so it can't crash on a wrong guess — it *reports* the truth rather than assuming it.

```csharp
using System;
using System.Reflection;
using System.Text;
using ICities;
using UnityEngine;
using SkylineBench.Bridge;
using SkylineBench.Json;

namespace SkylineBench
{
    public static class Probe
    {
        public static string BuildDump()
        {
            var w = new JsonWriter();
            w.BeginObject();

            // 1. Confirm HttpListener worked: reaching this code via /probe over HTTP proves it.
            w.Name("http_listener_works").Value(true);

            // 2. Enumerate road prefab names (CONFIRMED API).
            w.Name("road_prefabs").BeginArray();
            try
            {
                uint count = PrefabCollection<NetInfo>.PrefabCount();
                for (uint i = 0; i < count; i++)
                {
                    var p = PrefabCollection<NetInfo>.GetPrefab(i);
                    if (p != null && p.name != null) w.Value(p.name);
                }
            }
            catch (Exception e) { w.Value("ERROR: " + e.Message); }
            w.EndArray();

            // 3. Report whether candidate manager fields exist (resolves OPEN field names).
            w.Name("fields").BeginObject();
            ReportField(w, "VehicleManager.m_vehicleCount", typeof(VehicleManager), "m_vehicleCount");
            ReportField(w, "NetSegment.m_trafficDensity", typeof(NetSegment), "m_trafficDensity");
            ReportField(w, "BuildingManager.m_buildings", typeof(BuildingManager), "m_buildings");
            ReportField(w, "EconomyManager.instance", typeof(EconomyManager), null);
            ReportField(w, "ZoneManager.m_actualResidentialDemand", typeof(ZoneManager), "m_actualResidentialDemand");
            ReportField(w, "ZoneManager.m_actualCommercialDemand", typeof(ZoneManager), "m_actualCommercialDemand");
            ReportField(w, "ZoneManager.m_actualWorkplaceDemand", typeof(ZoneManager), "m_actualWorkplaceDemand");
            w.EndObject();

            // 4. List public instance members of the managers we still need (so we can see real names).
            w.Name("member_dump").BeginObject();
            DumpMembers(w, "EconomyManager", typeof(EconomyManager));
            DumpMembers(w, "ZoneManager", typeof(ZoneManager));
            DumpMembers(w, "BuildingManager", typeof(BuildingManager));
            DumpMembers(w, "VehicleManager", typeof(VehicleManager));
            DumpMembers(w, "ZoneBlock", typeof(ZoneBlock));
            DumpMembers(w, "LoadingManager", typeof(LoadingManager));
            w.EndObject();

            // 5. Paused-action probe: report current paused state + tick so the human can compare
            //    a /probe hit while paused vs running to see if queued work drains.
            var t = ModRuntime.Threading;
            w.Name("clock").BeginObject()
                .Name("paused").Value(t != null && t.simulationPaused)
                .Name("tick").Value((long)(t != null ? t.simulationTick : 0u))
                .Name("speed").Value((long)(t != null ? t.simulationSpeed : 0))
             .EndObject();

            w.EndObject();
            string json = w.ToString();
            Http.Log.Info("PROBE DUMP: " + json);
            return json;
        }

        private static void ReportField(JsonWriter w, string label, Type type, string field)
        {
            try
            {
                if (field == null) { w.Name(label).Value(type != null ? "type_exists" : "missing"); return; }
                var f = type.GetField(field, BindingFlags.Public | BindingFlags.Instance | BindingFlags.Static);
                w.Name(label).Value(f != null ? ("exists: " + f.FieldType.Name) : "MISSING");
            }
            catch (Exception e) { w.Name(label).Value("ERROR: " + e.Message); }
        }

        private static void DumpMembers(JsonWriter w, string label, Type type)
        {
            w.Name(label).BeginArray();
            try
            {
                foreach (var f in type.GetFields(BindingFlags.Public | BindingFlags.Instance))
                    w.Value(f.Name + " : " + f.FieldType.Name);
                foreach (var p in type.GetProperties(BindingFlags.Public | BindingFlags.Instance))
                    w.Value("prop " + p.Name + " : " + p.PropertyType.Name);
            }
            catch (Exception e) { w.Value("ERROR: " + e.Message); }
            w.EndArray();
        }
    }
}
```

> The probe never assumes a field exists — it *reflects* and reports. If a type name above (e.g. `ZoneBlock`, `LoadingManager`) is itself wrong, the build error in Task 8 Step 4 / a probe `ERROR:` line tells us, and we adjust. This is the safe way to resolve OPEN field names without fabricating them.

- [ ] **Step 2: Build with the probe included**

Run: `cd mod && msbuild /p:Configuration=Release SkylineBenchMod.csproj` (with the right `ManagedDLLPath`).
Expected: builds clean. Fix any wrong manager type names surfaced here.

- [ ] **Step 3: Commit**

```bash
git add mod/src/probe/Probe.cs mod/src/http/Router.cs mod/src/http/Handlers.cs
git commit -m "feat: add discovery probe dumping prefab names + manager field surface"
```

---

## Phase D — Build/install tooling + run the spike

### Task 10: build.sh + README (macOS install path)

**Files:**
- Create: `mod/build.sh`
- Create: `mod/README.md`

- [ ] **Step 1: Write build.sh**

Create `mod/build.sh` (macOS-first): check Mono, detect the game's `Managed/` dir, compile, install the DLL.

```bash
#!/usr/bin/env bash
set -euo pipefail

# 1. Mono check
if ! command -v msbuild >/dev/null 2>&1; then
  echo "msbuild not found. Install Mono:  brew install mono" >&2
  exit 1
fi

# 2. Locate the game's Managed dir (override with MANAGED_DLL_PATH=...)
DEFAULT_MANAGED="$HOME/Library/Application Support/Steam/steamapps/common/Cities_Skylines/Cities.app/Contents/Resources/Data/Managed"
MANAGED="${MANAGED_DLL_PATH:-$DEFAULT_MANAGED}"
if [ ! -f "$MANAGED/ICities.dll" ]; then
  echo "Game assemblies not found at: $MANAGED" >&2
  echo "Set MANAGED_DLL_PATH to your Cities.app .../Data/Managed directory." >&2
  exit 1
fi

# 3. Compile
echo "Building against: $MANAGED"
msbuild /p:Configuration=Release /p:ManagedDLLPath="$MANAGED" "$(dirname "$0")/SkylineBenchMod.csproj"

# 4. Install
MODS="$HOME/Library/Application Support/Colossal Order/Cities_Skylines/Addons/Mods/SkylineBench"
mkdir -p "$MODS"
cp "$(dirname "$0")/bin/Release/SkylineBenchMod.dll" "$MODS/"
echo "Installed SkylineBenchMod.dll -> $MODS"
echo "Now enable 'SkylineBench Bridge' in the game's Content Manager > Mods, then load a city."
```

- [ ] **Step 2: Write README.md**

Create `mod/README.md`:

```markdown
# SkylineBench Mod (Cities: Skylines 1 bridge)

In-game C# mod exposing a localhost HTTP API for the SkylineBench broker.

## Prerequisites
- Cities: Skylines 1 installed (Steam, macOS).
- Mono: `brew install mono` (provides `msbuild` for net35).

## Build & install (macOS)

    cd mod
    ./build.sh
    # If your game is elsewhere:
    # MANAGED_DLL_PATH="/path/to/Cities.app/Contents/Resources/Data/Managed" ./build.sh

This compiles `SkylineBenchMod.dll` and copies it to
`~/Library/Application Support/Colossal Order/Cities_Skylines/Addons/Mods/SkylineBench/`.

## Enable in-game (one-time, manual)
1. Launch Cities: Skylines.
2. Content Manager > Mods > enable **SkylineBench Bridge**.
3. Load (or start) a city. The HTTP server starts on `http://127.0.0.1:8787` when the city finishes loading.

## Verify
- `curl -s http://127.0.0.1:8787/health` → JSON with `"city_loaded":true`.
- `curl -s http://127.0.0.1:8787/probe`  → the discovery dump (also written to the game log).

## Logs
The mod logs via the game's debug log. On macOS the player log is at
`~/Library/Logs/Unity/Player.log` (and the game's `output_log`); search for `[SkylineBench]`.

## Run the pure tests (no game needed)

    cd mod/test && msbuild Tests.csproj && mono bin/Debug/Tests.exe
```

- [ ] **Step 3: Make build.sh executable & commit**

```bash
chmod +x mod/build.sh
git add mod/build.sh mod/README.md
git commit -m "feat: add macOS build/install script and mod README"
```

### Task 11: Run the discovery spike (HUMAN-IN-THE-LOOP) → write DISCOVERY.md

**Files:**
- Create: `mod/DISCOVERY.md`

> **This task requires the human to run Cities: Skylines.** A subagent prepares the commands and the `DISCOVERY.md` template; the human runs the game and pastes results.

- [ ] **Step 1: Human builds & installs the probe mod**

Run: `cd mod && ./build.sh`
Expected: builds and installs the DLL. If `build.sh` reports the game path is wrong, re-run with `MANAGED_DLL_PATH=...`. If `msbuild` is missing, `brew install mono` first.

- [ ] **Step 2: Human enables the mod and loads a city**

Enable **SkylineBench Bridge** in Content Manager > Mods; load any existing save (or start a new game). Watch for `[SkylineBench] HTTP server listening` in `~/Library/Logs/Unity/Player.log`.

- [ ] **Step 3: Human captures health + probe output**

Run (with the city loaded):
```bash
curl -s http://127.0.0.1:8787/health ; echo
curl -s http://127.0.0.1:8787/probe > /tmp/skylinebench-probe.json ; echo "saved"
```
Expected: `/health` returns `city_loaded:true`. `/probe` returns the dump. **If `/health`/`/probe` fail to connect**, `HttpListener` does not work in this Mono — record that (it triggers the TcpListener fallback task in Plan 2b) and grab the dump from the Player.log `PROBE DUMP:` line instead.

- [ ] **Step 4: Human runs the paused-drain check**

With the city **paused** (spacebar), in another terminal:
```bash
# build_road won't exist yet; use /probe twice — once paused, once running — and compare the
# clock.tick / verify the server still responds while paused (proves the accept loop runs).
curl -s http://127.0.0.1:8787/probe | grep -o '"paused":[a-z]*'
```
Record whether the server responds while paused (it should — the HTTP thread is independent) and note this; the *action*-drain-while-paused question is fully answered in Plan 2b when build_road exists, but capture any observation now.

- [ ] **Step 5: Write DISCOVERY.md from the captured output**

Create `mod/DISCOVERY.md` filling each section from `/tmp/skylinebench-probe.json` and observations:

```markdown
# SkylineBench Mod — Discovery Findings

Captured: <date>, game version <from /health>.

## HTTP transport
- HttpListener works in CS1 Mono: <YES/NO>. <if NO: Plan 2b must implement the TcpListener fallback>
- macOS firewall prompt on first bind: <observed?>

## Road prefab identifiers (for list_road_types / build_road)
<paste road_prefabs array; note the names for Basic Road, Highway, etc.>

## Manager field resolution (fills the OPEN items)
- Active vehicle count: <field, from VehicleManager member dump>
- Per-segment traffic: NetSegment.m_trafficDensity exists? <yes/no + type>. If no, compute from lane chain.
- Buildings buffer + BuildingInfo name/category: <fields>
- Economy money / income / expenses: <fields from EconomyManager member dump>
- Population + RCI demand: <fields>
- Zoning: ZoneBlock representation, cell size, how to read/set a cell: <findings>
- load-save mid-session: <LoadingManager members; is programmatic load possible? constraints>

## Constants
- Playable extent (metres): <value>  (vs broker's ±8640 assumption)
- Max single-segment length: <value>  (vs broker's MAX_SEGMENT_LENGTH_M = 200)
- Zone cell size: <value> (expected 8m)
- macOS Managed path confirmed: <path>; assembly names: <list>

## game-version accessor
- BuildConfig.applicationVersion works? <yes/no; else the accessor used>

## Open after probe (if any)
<anything still unknown that Plan 2b must probe further>
```

- [ ] **Step 6: Commit**

```bash
git add mod/DISCOVERY.md
git commit -m "docs: record in-game discovery findings (DISCOVERY.md)"
```

---

## Done criteria for this plan

- Pure helper tests pass: `cd mod/test && msbuild Tests.csproj && mono bin/Debug/Tests.exe` → `13 passed, 0 failed`.
- The mod DLL builds against the game assemblies via `./build.sh`.
- In-game: the mod loads, `GET /health` returns `city_loaded:true`, and `GET /probe` returns the dump (or, if `HttpListener` is unavailable, the dump is captured from the Player.log and that limitation is recorded).
- `mod/DISCOVERY.md` resolves the OPEN items (or explicitly flags any that need further probing).

**This unblocks Plan 2b** (contract endpoints + error table + build/verify), which is authored *from* `DISCOVERY.md` so its game-coupled code uses real field names — no fabricated signatures.

## Notes
- `HttpListener` viability and the paused-action-drain behavior are the two genuine risks; both are surfaced by the probe before any contract endpoint is written.
- If the spike reveals `HttpListener` doesn't work, Plan 2b's first task is a `TcpListener` implementation behind the existing `Dispatch` delegate — no rewrite of routing/handlers/helpers.
