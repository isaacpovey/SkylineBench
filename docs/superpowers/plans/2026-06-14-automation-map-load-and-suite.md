# Benchmark Automation: Map Load + Suite Runs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make map loading reliable and observable (report what was resolved; confirm completion by polling), bind `--map id` to a real save name, and add a sequential suite runner that drives a list of harness/model pairs from a manifest.

**Architecture:** Mod gains observable load results (`/load-save` reports resolved asset identity; on miss returns available save names) and a read-only `/saves` endpoint. The broker surfaces both through `reset_scenario` and a new `list_saves` accessor. `run.sh` resolves `--map <id>` via a `benchmark/maps/maps.tsv` binding, loads the save, and waits for the reload to complete by polling `/health`. A new `benchmark/run-suite.sh` wraps `run.sh`, looping over a `benchmark/suites/<name>.txt` manifest, resetting between runs and recording pass/fail.

**Tech Stack:** C# (CS1 mod, custom JsonWriter, no-game test runner), Rust (broker — axum mock, reqwest client, serde/schemars), Bash (run.sh / run-suite.sh, bash 3.2 compatible).

---

## File Structure

**Mod (C#):**
- `mod/src/dto/Dtos.cs` — extend `LoadResultDto` with resolved identity + available names.
- `mod/src/json/Serialize.cs` — serialize the extended load result + a saves list.
- `mod/src/bridge/SaveLoader.cs` — populate resolved identity; return available names on miss; add `ListSaves`.
- `mod/src/http/Handlers.cs` — add `Saves()` handler.
- `mod/src/http/Router.cs` — route `GET /saves`.
- `mod/test/SerializeTests.cs` — tests for the new shapes.

**Broker (Rust):**
- `broker/src/contract.rs` — extend `LoadResult`; add `SaveInfo` / `Saves`.
- `broker/src/bridge_client.rs` — add `list_saves`; `load_save` already returns `LoadResult`.
- `broker/src/service.rs` — `reset_scenario` surfaces the richer result; tests.
- `broker/src/mock.rs` — model saves + miss path on `/load-save`; add `/saves`.

**Shell:**
- `benchmark/maps/maps.tsv` — id → save_name → source → game_version binding (source of truth).
- `benchmark/maps/README.md` — point at maps.tsv.
- `benchmark/run.sh` — resolve `--map` via maps.tsv; load + wait-for-reload helper.
- `benchmark/suites/default.txt` — example manifest.
- `benchmark/run-suite.sh` — orchestrator.

---

## Part 1 — Mod: observable load

### Task 1: Extend `LoadResultDto` with resolved identity + available names

**Files:**
- Modify: `mod/src/dto/Dtos.cs:41`

- [ ] **Step 1: Replace the `LoadResultDto` definition**

In `mod/src/dto/Dtos.cs`, replace line 41:

```csharp
    public sealed class LoadResultDto { public bool Ok; public bool CityLoaded; }
```

with:

```csharp
    public sealed class SaveInfoDto { public string Name; public string CityName; public string FullName; }

    public sealed class LoadResultDto
    {
        public bool Ok;
        public bool CityLoaded;
        // Identity of the asset the loader resolved (null when Ok==false).
        public SaveInfoDto Resolved;
        // Save names the game exposes; populated only on a no-match miss, so a
        // failed load tells the operator what to pin instead of guessing.
        public List<SaveInfoDto> Available = new List<SaveInfoDto>();
    }
```

(`System.Collections.Generic` is already imported at the top of the file.)

- [ ] **Step 2: Build the test project to confirm it compiles**

The mod targets .NET 3.5 and builds with Mono (`msbuild`/`xbuild`), not `dotnet`. `Dtos.cs` is compiled into the no-game test project (`mod/test/Tests.csproj`).

Run: `cd mod/test && msbuild Tests.csproj` (or `xbuild Tests.csproj` if msbuild is absent)
Expected: build succeeds (no references to the old single-line struct elsewhere yet).

- [ ] **Step 3: Commit**

```bash
git add mod/src/dto/Dtos.cs
git commit -m "feat(mod): LoadResultDto carries resolved identity + available saves"
```

### Task 2: Serialize the extended load result + a saves list

**Files:**
- Modify: `mod/src/json/Serialize.cs:96-101`
- Test: `mod/test/SerializeTests.cs:101-105`

- [ ] **Step 1: Write the failing tests**

In `mod/test/SerializeTests.cs`, replace the `Load` method (lines 101-105) with:

```csharp
        static void Load()
        {
            Assert.Equal("{\"ok\":true,\"city_loaded\":true,\"resolved\":{\"name\":\"gridlock\",\"city_name\":\"Gridlock City\",\"full_name\":\"pkg.gridlock\"}}",
                Serialize.Load(new LoadResultDto
                {
                    Ok = true,
                    CityLoaded = true,
                    Resolved = new SaveInfoDto { Name = "gridlock", CityName = "Gridlock City", FullName = "pkg.gridlock" },
                }));
        }

        static void LoadMissListsAvailable()
        {
            var r = new LoadResultDto { Ok = false, CityLoaded = false };
            r.Available.Add(new SaveInfoDto { Name = "a", CityName = "A City", FullName = "pkg.a" });
            Assert.Equal("{\"ok\":false,\"city_loaded\":false,\"available\":[{\"name\":\"a\",\"city_name\":\"A City\",\"full_name\":\"pkg.a\"}]}",
                Serialize.Load(r));
        }

        static void SavesList()
        {
            var saves = new List<SaveInfoDto> { new SaveInfoDto { Name = "a", CityName = "A City", FullName = "pkg.a" } };
            Assert.Equal("{\"saves\":[{\"name\":\"a\",\"city_name\":\"A City\",\"full_name\":\"pkg.a\"}]}",
                Serialize.Saves(saves));
        }
```

Then register the two new tests next to the existing load registration (line 21):

```csharp
            tests.Add(new KeyValuePair<string, Action>("serialize: load result", Load));
            tests.Add(new KeyValuePair<string, Action>("serialize: load miss lists available", LoadMissListsAvailable));
            tests.Add(new KeyValuePair<string, Action>("serialize: saves list", SavesList));
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd mod/test && msbuild Tests.csproj`
Expected: FAIL to compile — `Serialize.Saves` does not exist and `SaveInfoDto` is used; the `Load` shape differs.

- [ ] **Step 3: Implement the serializers**

In `mod/src/json/Serialize.cs`, replace the `Load` method (lines 96-101) with:

```csharp
        public static string Load(LoadResultDto l)
        {
            var w = new JsonWriter();
            w.BeginObject().Name("ok").Value(l.Ok).Name("city_loaded").Value(l.CityLoaded);
            if (l.Resolved != null) WriteSaveInfo(w.Name("resolved"), l.Resolved);
            if (l.Available.Count > 0)
            {
                w.Name("available").BeginArray();
                foreach (var s in l.Available) WriteSaveInfo(w, s);
                w.EndArray();
            }
            w.EndObject();
            return w.ToString();
        }

        public static string Saves(System.Collections.Generic.List<SaveInfoDto> saves)
        {
            var w = new JsonWriter();
            w.BeginObject().Name("saves").BeginArray();
            foreach (var s in saves) WriteSaveInfo(w, s);
            w.EndArray().EndObject();
            return w.ToString();
        }

        private static void WriteSaveInfo(JsonWriter w, SaveInfoDto s)
        {
            w.BeginObject().Name("name").Value(s.Name).Name("city_name").Value(s.CityName).Name("full_name").Value(s.FullName).EndObject();
        }
```

Note: `w.Name("resolved")` returns the same `JsonWriter`, so `WriteSaveInfo(w.Name("resolved"), l.Resolved)` writes the keyed object inline (matches the existing fluent style at line 53).

- [ ] **Step 4: Run tests to verify they pass**

The test project is an Exe (`TestRunner.Main`) run under Mono — same command as the existing mod tests (`mod/README.md:53`).

Run: `cd mod/test && msbuild Tests.csproj && mono bin/Debug/Tests.exe`
Expected: PASS — all three `serialize:` load/saves tests green; the runner exits 0.

- [ ] **Step 5: Commit**

```bash
git add mod/src/json/Serialize.cs mod/test/SerializeTests.cs
git commit -m "feat(mod): serialize resolved identity, available saves, and saves list"
```

### Task 3: Populate resolved identity / available names in `SaveLoader` + add `ListSaves`

**Files:**
- Modify: `mod/src/bridge/SaveLoader.cs`

- [ ] **Step 1: Add a SaveInfo projection + ListSaves, and enrich Load**

In `mod/src/bridge/SaveLoader.cs`, replace the body from `Load` through `FindSave` (lines 21-69) with the version below. It keeps `FindSave`'s precedence (name → cityName → fullName fallback) but now records the matched asset's identity, and on a miss enumerates every save for the `Available` list.

```csharp
        public static LoadResultDto Load(string saveName)
        {
            if (string.IsNullOrEmpty(saveName)) return Miss();

            Package.Asset target = FindSave(saveName);
            if (target == null) return Miss();

            SaveInfoDto resolved = Describe(target);

            SimThread.Run(delegate
            {
                SaveGameMetaData metaData = target.Instantiate<SaveGameMetaData>();
                SimulationMetaData meta = new SimulationMetaData();
                meta.m_CityName = metaData != null ? metaData.cityName : null;
                meta.m_updateMode = SimulationManager.UpdateMode.LoadGame;
                Singleton<LoadingManager>.instance.LoadLevel(target, "Game", "InGame", meta, false);
            }, 8000);

            // Load runs asynchronously after kick-off; callers confirm completion
            // by polling /health (the bridge restarts on level reload).
            return new LoadResultDto { Ok = true, CityLoaded = true, Resolved = resolved };
        }

        public static System.Collections.Generic.List<SaveInfoDto> ListSaves()
        {
            var list = new System.Collections.Generic.List<SaveInfoDto>();
            foreach (Package.Asset asset in PackageManager.FilterAssets(UserAssetType.SaveGameMetaData))
            {
                if (asset == null) continue;
                list.Add(Describe(asset));
            }
            return list;
        }

        private static LoadResultDto Miss()
        {
            return new LoadResultDto { Ok = false, CityLoaded = false, Available = ListSaves() };
        }

        private static SaveInfoDto Describe(Package.Asset asset)
        {
            string cityName = null;
            try
            {
                SaveGameMetaData metaData = asset.Instantiate<SaveGameMetaData>();
                if (metaData != null) cityName = metaData.cityName;
            }
            catch
            {
                // Corrupt save: report name/fullName without cityName.
            }
            return new SaveInfoDto { Name = asset.name, CityName = cityName, FullName = asset.fullName };
        }

        private static Package.Asset FindSave(string saveName)
        {
            if (string.IsNullOrEmpty(saveName)) return null;

            Package.Asset fullNameMatch = null;
            foreach (Package.Asset asset in PackageManager.FilterAssets(UserAssetType.SaveGameMetaData))
            {
                if (asset == null) continue;
                if (asset.name == saveName) return asset;

                try
                {
                    SaveGameMetaData metaData = asset.Instantiate<SaveGameMetaData>();
                    if (metaData != null && metaData.cityName == saveName) return asset;
                }
                catch
                {
                    // Corrupt save: skip cityName matching for this asset, keep searching.
                }

                if (asset.fullName == saveName) fullNameMatch = asset;
            }

            return fullNameMatch;
        }
```

- [ ] **Step 2: Build to confirm it compiles**

`SaveLoader.cs` has game dependencies, so it builds via the full mod build against the game's `Managed/` assemblies (not the no-game test project).

Run: `mod/build.sh` (requires Mono + the game DLLs; set `MANAGED_DLL_PATH` if not at the default Steam location)
Expected: build succeeds against the 1.21.1-f9 game assemblies and installs the dll.

- [ ] **Step 3: Commit**

```bash
git add mod/src/bridge/SaveLoader.cs
git commit -m "feat(mod): SaveLoader reports resolved identity, available saves on miss, ListSaves"
```

### Task 4: Add `GET /saves` handler + route

**Files:**
- Modify: `mod/src/http/Handlers.cs:57`
- Modify: `mod/src/http/Router.cs:31`

- [ ] **Step 1: Add the handler**

In `mod/src/http/Handlers.cs`, immediately after the `LoadSave` handler (line 57), add:

```csharp
        public static HttpReply Saves() { return HttpReply.Json(200, Serialize.Saves(SaveLoader.ListSaves())); }
```

- [ ] **Step 2: Add the route**

In `mod/src/http/Router.cs`, after the `/load-save` case (line 31), add:

```csharp
                case "/saves": return method == "GET" ? Handlers.Saves() : MethodNotAllowed();
```

- [ ] **Step 3: Build to confirm it compiles**

Run: `mod/build.sh`
Expected: build succeeds and installs the dll.

- [ ] **Step 4: Commit**

```bash
git add mod/src/http/Handlers.cs mod/src/http/Router.cs
git commit -m "feat(mod): GET /saves enumerates available savegames"
```

---

## Part 2 — Broker: surface load identity + saves

### Task 5: Extend `LoadResult`; add `SaveInfo` / `Saves` to the contract

**Files:**
- Modify: `broker/src/contract.rs:224-228`

- [ ] **Step 1: Write the failing test**

In `broker/src/contract.rs`, inside the existing `mod tests` block (after line 232 `use super::*;`), add:

```rust
    #[test]
    fn load_result_defaults_resolved_and_available() {
        let r: LoadResult = serde_json::from_str(r#"{"ok":true,"city_loaded":true}"#).unwrap();
        assert!(r.resolved.is_none());
        assert!(r.available.is_empty());
    }

    #[test]
    fn load_result_parses_resolved_identity() {
        let r: LoadResult = serde_json::from_str(
            r#"{"ok":true,"city_loaded":true,"resolved":{"name":"g","city_name":"G","full_name":"pkg.g"}}"#,
        )
        .unwrap();
        assert_eq!(r.resolved.unwrap().name, "g");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --manifest-path broker/Cargo.toml load_result`
Expected: FAIL to compile — `LoadResult` has no `resolved`/`available` fields and `SaveInfo` is undefined.

- [ ] **Step 3: Replace the `LoadResult` definition**

In `broker/src/contract.rs`, replace lines 224-228:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadResult {
    pub ok: bool,
    pub city_loaded: bool,
}
```

with:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveInfo {
    pub name: String,
    #[serde(default)]
    pub city_name: Option<String>,
    pub full_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadResult {
    pub ok: bool,
    pub city_loaded: bool,
    #[serde(default)]
    pub resolved: Option<SaveInfo>,
    #[serde(default)]
    pub available: Vec<SaveInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Saves {
    pub saves: Vec<SaveInfo>,
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --manifest-path broker/Cargo.toml load_result`
Expected: PASS — both new tests green.

- [ ] **Step 5: Commit**

```bash
git add broker/src/contract.rs
git commit -m "feat(broker): LoadResult carries resolved/available; add SaveInfo, Saves"
```

### Task 6: Add `list_saves` to the bridge client

**Files:**
- Modify: `broker/src/bridge_client.rs` (after `load_save`, ~line 190)

- [ ] **Step 1: Add the accessor**

In `broker/src/bridge_client.rs`, immediately after the `load_save` method (ends line 190), add (match the existing GET style used by `network`/`road_types`):

```rust
    pub async fn list_saves(&self) -> Result<Saves, BridgeError> {
        Ok(self
            .http
            .get(format!("{}/saves", self.base))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
```

Ensure `Saves` is in scope: the file already imports contract types (the `use crate::contract::...` line near the top). If `Saves` is not covered by a glob, add it to that import list.

- [ ] **Step 2: Build to confirm it compiles**

Run: `cargo build --manifest-path broker/Cargo.toml`
Expected: builds (will be exercised by the mock test in Task 7).

- [ ] **Step 3: Commit**

```bash
git add broker/src/bridge_client.rs
git commit -m "feat(broker): bridge client list_saves accessor"
```

### Task 7: Mock — model saves, miss path on `/load-save`, and `/saves`

**Files:**
- Modify: `broker/src/mock.rs:390-407`, `:462-479`

- [ ] **Step 1: Write the failing tests**

In `broker/src/mock.rs`, inside the `mod tests` block (after line 495 `use super::*;`), add:

```rust
    #[tokio::test]
    async fn saves_endpoint_lists_known_save() {
        let (addr, server) = bind("127.0.0.1:0".parse().unwrap()).await;
        tokio::spawn(server);
        let body: crate::contract::Saves = reqwest::Client::new()
            .get(format!("http://{addr}/saves"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(body.saves.iter().any(|s| s.name == "gridlock-v1"));
    }

    #[tokio::test]
    async fn load_save_unknown_name_misses_with_available() {
        let (addr, server) = bind("127.0.0.1:0".parse().unwrap()).await;
        tokio::spawn(server);
        let resp: crate::contract::LoadResult = reqwest::Client::new()
            .post(format!("http://{addr}/load-save"))
            .json(&serde_json::json!({ "save_name": "nope" }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(!resp.ok);
        assert!(resp.available.iter().any(|s| s.name == "gridlock-v1"));
    }

    #[tokio::test]
    async fn load_save_known_name_resolves_identity() {
        let (addr, server) = bind("127.0.0.1:0".parse().unwrap()).await;
        tokio::spawn(server);
        let resp: crate::contract::LoadResult = reqwest::Client::new()
            .post(format!("http://{addr}/load-save"))
            .json(&serde_json::json!({ "save_name": "gridlock-v1" }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(resp.ok);
        assert_eq!(resp.resolved.unwrap().name, "gridlock-v1");
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --manifest-path broker/Cargo.toml mock::tests::load_save`
Expected: FAIL — `/saves` route missing (404 → JSON decode error) and the current `load_save` always returns `ok:true` with no `resolved`.

- [ ] **Step 3: Add a saves helper + replace the `load_save` handler**

In `broker/src/mock.rs`, replace the `load_save` handler (lines 393-407) with the version below, and add a module-level `mock_saves()` helper just above it. `gridlock-v1` is the known name the tests and the default map binding use.

```rust
fn mock_saves() -> Vec<crate::contract::SaveInfo> {
    vec![crate::contract::SaveInfo {
        name: "gridlock-v1".to_string(),
        city_name: Some("Gridlock City".to_string()),
        full_name: "skylinebench.gridlock-v1".to_string(),
    }]
}

async fn load_save(
    State(s): State<MockState>,
    Json(body): Json<LoadSaveBody>,
) -> Json<LoadResult> {
    let saves = mock_saves();
    let resolved = saves.iter().find(|s| s.name == body.save_name).cloned();
    match resolved {
        None => Json(LoadResult {
            ok: false,
            city_loaded: false,
            resolved: None,
            available: saves,
        }),
        Some(resolved) => {
            let mut c = s.city.lock().unwrap();
            c.nodes.clear();
            c.segments.clear();
            c.zones.clear();
            c.tick = 0;
            c.next_id = 1;
            Json(LoadResult {
                ok: true,
                city_loaded: true,
                resolved: Some(resolved),
                available: Vec::new(),
            })
        }
    }
}

async fn saves() -> Json<crate::contract::Saves> {
    Json(crate::contract::Saves { saves: mock_saves() })
}
```

- [ ] **Step 4: Register the `/saves` route**

In `broker/src/mock.rs`, in `router()` (lines 462-479), add after the `/load-save` line (476):

```rust
        .route("/saves", get(saves))
```

- [ ] **Step 5: Update the existing `reset_scenario_clears_the_city` test save name**

The mock now rejects unknown names, so the service test in `broker/src/service.rs:803-808` must use a known save. In `broker/src/service.rs`, change line 806:

```rust
                save: "anything".into(),
```

to:

```rust
                save: "gridlock-v1".into(),
```

- [ ] **Step 6: Run to verify all pass**

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: PASS — new mock tests green; `reset_scenario_clears_the_city` still green with the known name.

- [ ] **Step 7: Commit**

```bash
git add broker/src/mock.rs broker/src/service.rs
git commit -m "test(broker): mock models saves, miss path, and /saves endpoint"
```

### Task 8: `reset_scenario` surfaces resolved identity + available on miss

**Files:**
- Modify: `broker/src/service.rs:338-345`

- [ ] **Step 1: Write the failing tests**

In `broker/src/service.rs`, inside the test module (near the existing `reset_scenario_clears_the_city`, after line 813), add:

```rust
    #[tokio::test]
    async fn reset_scenario_reports_resolved_identity() {
        let c = client().await;
        let res = reset_scenario(
            &c,
            ResetScenarioArgs {
                save: "gridlock-v1".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(res["ok"], true);
        assert_eq!(res["resolved"]["name"], "gridlock-v1");
    }

    #[tokio::test]
    async fn reset_scenario_unknown_save_lists_available() {
        let c = client().await;
        let res = reset_scenario(
            &c,
            ResetScenarioArgs {
                save: "nope".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(res["ok"], false);
        assert_eq!(res["available"][0]["name"], "gridlock-v1");
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --manifest-path broker/Cargo.toml reset_scenario`
Expected: FAIL — `reset_scenario` currently serializes a `LoadResult` lacking `resolved`/`available`, so `res["resolved"]["name"]` is `Null`.

- [ ] **Step 3: Confirm the implementation already passes through**

`reset_scenario` (lines 338-345) does `client.load_save(&args.save)` then `serde_json::to_value(res)`. With `LoadResult` now carrying `resolved`/`available` (Task 5) and the client deserializing them (Task 6, 7), the values flow through unchanged. No code change is expected here — the test simply asserts the now-richer passthrough. If `resolved`/`available` serialize as `null`/absent, verify `LoadResult`'s serde derive (Task 5) and the client's `.json()` decode (Task 7) round-trip the fields.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test --manifest-path broker/Cargo.toml reset_scenario`
Expected: PASS — both new tests green.

- [ ] **Step 5: Commit**

```bash
git add broker/src/service.rs
git commit -m "test(broker): reset_scenario surfaces resolved identity and available saves"
```

---

## Part 3 — Shell: map binding + load-and-wait

### Task 9: Add the map binding file + point the README at it

**Files:**
- Create: `benchmark/maps/maps.tsv`
- Modify: `benchmark/maps/README.md`

- [ ] **Step 1: Create the binding file**

Create `benchmark/maps/maps.tsv` (tab-separated; `#` comments and blank lines ignored by the parser added in Task 10). The `save_name` column is the exact in-game save identity (from `GET /saves`) the loader must send:

```
# id<TAB>save_name<TAB>source<TAB>game_version
gridlock-v1	gridlock-v1	(fill in source)	1.21.1-f9
```

- [ ] **Step 2: Update the README to reference the binding**

In `benchmark/maps/README.md`, replace the "## Pinned saves" table with a pointer:

```markdown
## Pinned saves

The machine-readable id → save-name binding lives in `maps.tsv` (one row per map:
`id`, `save_name`, `source`, `game_version`). `run.sh --map <id>` resolves the id
to its `save_name` and loads it. List the game's actual save identities with
`GET /saves` (or `curl http://127.0.0.1:8787/saves`) to fill in `save_name`.
```

- [ ] **Step 3: Commit**

```bash
git add benchmark/maps/maps.tsv benchmark/maps/README.md
git commit -m "feat(benchmark): maps.tsv binds map id to in-game save name"
```

### Task 10: `run.sh` resolves `--map` via maps.tsv and loads the save with a reload wait

**Files:**
- Modify: `benchmark/run.sh` (the preflight block, lines ~37-46, and add helpers)

The current preflight only *checks* `city_loaded:true`. Replace it with: resolve the map id → save name, POST `/load-save`, then wait for the bridge to cycle (down then back up with `city_loaded:true`). The down→up cycle is the robust completion signal — the mod stops its HTTP bridge in `OnLevelUnloading` and restarts it in `OnLevelLoaded`.

- [ ] **Step 1: Add the resolve + load helpers**

In `benchmark/run.sh`, after the arg-parse/validation block (after line 35, the `case "$MAP"` validation) and before the existing preflight, add:

```bash
# Resolve a map id to its in-game save name via benchmark/maps/maps.tsv.
# Tab-separated: id<TAB>save_name<TAB>source<TAB>game_version; '#'/blank skipped.
resolve_save_name() {
  local want="$1" maps="$ROOT/benchmark/maps/maps.tsv" id save_name rest
  [ -f "$maps" ] || { echo "missing map binding file: $maps" >&2; return 1; }
  while IFS="$(printf '\t')" read -r id save_name rest; do
    case "$id" in ''|'#'*) continue ;; esac
    if [ "$id" = "$want" ]; then printf '%s\n' "$save_name"; return 0; fi
  done < "$maps"
  echo "unknown map id '$want'. Known ids:" >&2
  while IFS="$(printf '\t')" read -r id _; do
    case "$id" in ''|'#'*) continue ;; esac
    echo "  $id" >&2
  done < "$maps"
  return 1
}

# Issue the load and wait for the level-reload bridge cycle to finish.
# Returns non-zero on timeout (surfaces the "invalid file"/unstable-load case).
load_and_wait() {
  local save_name="$1" deadline
  local resp
  resp="$(curl -fsS -X POST "$MOD_URL/load-save" \
    -H 'content-type: application/json' \
    -d "$(printf '{"save_name":%s}' "$(json_str "$save_name")")" 2>/dev/null || true)"
  case "$(printf '%s' "$resp" | tr -d '[:space:]')" in
    *'"ok":false'*)
      echo "load rejected for save '$save_name'. Mod reported available saves:" >&2
      printf '%s\n' "$resp" >&2
      return 1 ;;
    "") echo "mod not reachable at $MOD_URL/load-save" >&2; return 1 ;;
  esac
  # Phase 1: bridge goes down (reload started). Tolerate up to 30s.
  deadline=$(( $(date +%s) + 30 ))
  until [ "$(date +%s)" -ge "$deadline" ]; do
    curl -fsS "$MOD_URL/health" >/dev/null 2>&1 || break
    sleep 1
  done
  # Phase 2: bridge back up with a city loaded (reload finished). Up to 180s.
  deadline=$(( $(date +%s) + 180 ))
  until [ "$(date +%s)" -ge "$deadline" ]; do
    local h
    h="$(curl -fsS "$MOD_URL/health" 2>/dev/null || true)"
    case "$(printf '%s' "$h" | tr -d '[:space:]')" in
      *'"city_loaded":true'*) return 0 ;;
    esac
    sleep 2
  done
  echo "timed out waiting for save '$save_name' to finish loading" >&2
  return 1
}

# Minimal JSON string encoder (escape backslash and double-quote).
json_str() {
  local s="$1"
  s="${s//\\/\\\\}"
  s="${s//\"/\\\"}"
  printf '"%s"' "$s"
}
```

- [ ] **Step 2: Replace the check-only preflight with resolve + load**

In `benchmark/run.sh`, replace the existing preflight block (lines 37-46, the `if [ "${DRY_RUN:-0}" != "1" ]; then ... fi` that only checks `/health`) with:

```bash
SAVE_NAME="$(resolve_save_name "$MAP")" || exit 1

# Preflight + load (skipped under DRY_RUN, which only inspects the resolved
# command). Reachability is implied by load_and_wait's first curl.
if [ "${DRY_RUN:-0}" != "1" ]; then
  echo "loading map '$MAP' (save '$SAVE_NAME')…" >&2
  load_and_wait "$SAVE_NAME" || exit 1
fi
```

- [ ] **Step 3: Verify DRY_RUN still resolves and prints a plan without touching the game**

Run: `DRY_RUN=1 benchmark/run.sh --map gridlock-v1 --harness claude`
Expected: prints the resolved harness command (existing DRY_RUN output) and exits 0; resolves `gridlock-v1` from maps.tsv with no curl to the game. An unknown id fails:
Run: `DRY_RUN=1 benchmark/run.sh --map bogus --harness claude`
Expected: exits non-zero with "unknown map id 'bogus'. Known ids:" listing `gridlock-v1`.

- [ ] **Step 4: Commit**

```bash
git add benchmark/run.sh
git commit -m "feat(benchmark): run.sh resolves map id to save name and loads with reload wait"
```

---

## Part 4 — Shell: sequential suite runner

### Task 11: Add an example suite manifest

**Files:**
- Create: `benchmark/suites/default.txt`

- [ ] **Step 1: Create the manifest**

Create `benchmark/suites/default.txt`:

```
# Suite: one run per line, harness[:model]. '#' comments and blank lines ignored.
# harness with no :model uses the harness default.
claude:claude-opus-4-8
claude:claude-sonnet-4-6
codex
gemini:gemini-2.5-flash
opencode
```

- [ ] **Step 2: Commit**

```bash
git add benchmark/suites/default.txt
git commit -m "feat(benchmark): example suite manifest"
```

### Task 12: Add `run-suite.sh` orchestrator

**Files:**
- Create: `benchmark/run-suite.sh`

- [ ] **Step 1: Write the orchestrator**

Create `benchmark/run-suite.sh` (mode 0755). It parses the manifest, runs each entry through `run.sh` in order, resets between entries (handled inside `run.sh`'s load step — each `run.sh` invocation loads the map first), and records pass/fail. Record-and-continue by default; `--fail-fast` stops on first failure.

```bash
#!/usr/bin/env bash
set -euo pipefail

MAP=""
SUITE=""
MOD_URL="http://127.0.0.1:8787"
MAP_SOURCE="test"
FAIL_FAST=0
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SUITE_ID="suite-$(date +%Y%m%d-%H%M%S)"
OUT_DIR="$ROOT/benchmark/runs/$SUITE_ID"

while [ $# -gt 0 ]; do
  case "$1" in
    --map) MAP="$2"; shift 2 ;;
    --suite) SUITE="$2"; shift 2 ;;
    --mod-url) MOD_URL="$2"; shift 2 ;;
    --map-source) MAP_SOURCE="$2"; shift 2 ;;
    --out) OUT_DIR="$2"; shift 2 ;;
    --fail-fast) FAIL_FAST=1; shift ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

[ -n "$MAP" ] && [ -n "$SUITE" ] || {
  echo "usage: run-suite.sh --map <id> --suite <file> [--mod-url URL] [--map-source SRC] [--out DIR] [--fail-fast]" >&2
  exit 2
}
[ -f "$SUITE" ] || { echo "suite file not found: $SUITE" >&2; exit 2; }

mkdir -p "$OUT_DIR"
cp "$SUITE" "$OUT_DIR/suite.txt"
SUMMARY="$OUT_DIR/summary.tsv"
printf 'harness\tmodel\trunid\tstatus\texit_code\n' > "$SUMMARY"

# Parse manifest into harness/model pairs (skip '#'/blank).
ENTRIES=()
while IFS= read -r line || [ -n "$line" ]; do
  case "$line" in ''|'#'*) continue ;; esac
  ENTRIES+=("$line")
done < "$SUITE"

[ "${#ENTRIES[@]}" -gt 0 ] || { echo "suite '$SUITE' has no runnable entries" >&2; exit 2; }

# Pre-suite validation: every distinct harness's binary + secrets resolve.
# DRY_RUN=1 run.sh exits 0 only if harness-prepare succeeds; we additionally
# probe the harness binary + required env the same way run.sh does at launch.
echo "validating ${#ENTRIES[@]} suite entries…" >&2
for entry in "${ENTRIES[@]}"; do
  harness="${entry%%:*}"
  model=""
  case "$entry" in *:*) model="${entry#*:}" ;; esac
  if ! DRY_RUN=1 "$ROOT/benchmark/run.sh" --map "$MAP" --map-source "$MAP_SOURCE" \
      --mod-url "$MOD_URL" --harness "$harness" ${model:+--model "$model"} >/dev/null; then
    echo "suite validation failed for entry '$entry'" >&2
    exit 1
  fi
done

run_one() {
  local entry="$1" harness model runid child status=ok code=0
  harness="${entry%%:*}"
  model=""
  case "$entry" in *:*) model="${entry#*:}" ;; esac
  runid="$(date +%Y%m%d-%H%M%S)-$harness${model:+-$model}"
  child="$OUT_DIR/$runid"
  echo "=== running $entry → $child ===" >&2
  if "$ROOT/benchmark/run.sh" --map "$MAP" --map-source "$MAP_SOURCE" \
      --mod-url "$MOD_URL" --harness "$harness" ${model:+--model "$model"} \
      --out "$child"; then
    status=ok
  else
    code=$?
    status=failed
  fi
  printf '%s\t%s\t%s\t%s\t%s\n' "$harness" "$model" "$runid" "$status" "$code" >> "$SUMMARY"
  [ "$status" = ok ]
}

FAILED=0
for entry in "${ENTRIES[@]}"; do
  if ! run_one "$entry"; then
    FAILED=$((FAILED + 1))
    if [ "$FAIL_FAST" = 1 ]; then
      echo "fail-fast: stopping suite after '$entry'" >&2
      break
    fi
  fi
done

echo "suite complete: $OUT_DIR (failed: $FAILED)" >&2
column -t -s "$(printf '\t')" "$SUMMARY" >&2 || cat "$SUMMARY" >&2
[ "$FAILED" -eq 0 ]
```

- [ ] **Step 2: Make it executable**

Run: `chmod +x benchmark/run-suite.sh`

- [ ] **Step 3: Verify usage + manifest parsing without launching anything**

Run: `benchmark/run-suite.sh` (no args)
Expected: prints usage, exits 2.

Run: `DRY_RUN=1 benchmark/run-suite.sh --map gridlock-v1 --suite benchmark/suites/default.txt --out /tmp/suite-dryrun`
Expected: validation loop runs `DRY_RUN=1 run.sh` per entry (each prints its resolved command to stderr and exits 0), then the run loop also executes under `DRY_RUN=1` — each `run.sh` exits 0 at the DRY_RUN plan print without loading the game or launching a harness, so every entry records `ok` in `/tmp/suite-dryrun/summary.tsv`. Confirm `suite.txt` and `summary.tsv` exist with one row per non-comment manifest line.

Note: under `DRY_RUN=1` the child `run.sh` exits before its lock/session/build logic (the DRY_RUN branch prints the plan and exits 0), so suite dry-runs need no game, no broker build, and no secrets.

- [ ] **Step 4: Commit**

```bash
git add benchmark/run-suite.sh
git commit -m "feat(benchmark): run-suite.sh drives a manifest of harness/model runs in order"
```

### Task 13: Document the suite runner

**Files:**
- Modify: `benchmark/README.md`

- [ ] **Step 1: Add a suite section**

Read `benchmark/README.md` first, then append a "## Running a suite" section documenting:
- `maps.tsv` binding and how to fill `save_name` from `GET /saves`.
- `run-suite.sh --map <id> --suite <file> [--fail-fast]`.
- Output layout (`runs/suite-<ts>/` with per-run dirs + `summary.tsv`).
- Record-and-continue default; `--fail-fast` to stop on first failure.
- The game must be running with the mod enabled; each entry loads the map itself (no manual menu load).

- [ ] **Step 2: Commit**

```bash
git add benchmark/README.md
git commit -m "docs(benchmark): document maps.tsv binding and run-suite.sh"
```

---

## Self-Review

**Spec coverage:**
- §1.1 observable load result → Tasks 1, 2, 3, 8. ✓
- §1.2 read-only `/saves` → Tasks 3 (ListSaves), 4 (route), 6 (client), 7 (mock). ✓
- §1.3 confirm completion by polling → Task 10 (`load_and_wait` down→up cycle). ✓
- §1.4 bind `--map id` → save name → Tasks 9, 10. ✓
- §2.1 manifest format → Task 11. ✓
- §2.2 `run-suite.sh` over `run.sh`, record-and-continue + `--fail-fast` → Task 12. ✓
- §2.3 locking via run.sh's existing LOCK_DIR → Task 12 (no extra lock; documented). ✓
- §2.4 output layout (`suite-<ts>/`, per-run dirs, `summary.tsv`) → Task 12. ✓
- §2.5 pre-suite validation → Task 12 (validation loop). ✓
- Testing: mod serialize tests (Task 2), broker contract/mock/service tests (Tasks 5,7,8), DRY_RUN plans (Tasks 10,12). ✓

**Type consistency:**
- `SaveInfoDto`/`SaveInfo` fields `name`/`city_name`/`full_name` consistent across mod DTO (Task 1), mod serializer (Task 2), broker contract (Task 5), mock (Task 7).
- `LoadResultDto`/`LoadResult` fields `ok`/`city_loaded`/`resolved`/`available` consistent (Tasks 1,2,5,7,8).
- `Serialize.Saves(List<SaveInfoDto>)` defined Task 2, called Task 4. `SaveLoader.ListSaves()` defined Task 3, called Tasks 4. ✓
- Mock save name `gridlock-v1` consistent across mock (Task 7), service test (Task 8), maps.tsv + default suite (Tasks 9,11).

**Residual risk:** mid-session `LoadLevel` may still be unstable (DISCOVERY.md). `load_and_wait` Phase-2 timeout converts that into a clean per-run failure recorded in `summary.tsv` rather than a corrupt run — consistent with the spec's risk note.
