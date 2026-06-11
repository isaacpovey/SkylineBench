# Real In-Game Screenshot Timelapse Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Capture real in-game screenshots during benchmark runs (overview after every sim step, close-up after every agent action) and assemble them into an annotated timelapse mp4 with one command.

**Architecture:** The C# mod gains a `POST /screenshot` endpoint that moves the game camera on Unity's main thread, reads the framebuffer, and returns PNG bytes. The Rust broker orchestrates capture (after steps and mutating actions in the benchmark server), persists frames + `index.jsonl` sidecars under `screenshots/{overview,actions}/`, and a new `broker timelapse` subcommand composites a HUD strip onto each frame and shells out to ffmpeg. Spec: `docs/superpowers/specs/2026-06-11-real-screenshot-timelapse-design.md`.

**Tech Stack:** C# (Mono, Unity/CS1 modding, pre-C#6 syntax — no string interpolation), Rust (tokio, reqwest, axum mock, tiny-skia, rmcp), new dep `ab_glyph` for text, ffmpeg (external).

**Conventions:** Mod C# follows the existing terse style (see `mod/src/http/Handlers.cs`). Rust follows existing module style. Run mod pure tests with `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe`. Run broker tests with `cargo test --manifest-path broker/Cargo.toml`.

---

### Task 1: Mod — screenshot request parsing (pure, TDD)

**Files:**
- Modify: `mod/src/json/RequestParse.cs`
- Test: `mod/test/RequestParseTests.cs`

- [ ] **Step 1: Write the failing test**

In `mod/test/RequestParseTests.cs`, register and add:

```csharp
// in Register(...):
tests.Add(new KeyValuePair<string, Action>("parse: screenshot", Screenshot));

// new test method:
static void Screenshot()
{
    var r = RequestParse.Screenshot(JsonReader.Parse(
        "{\"x\":-120.5,\"z\":340,\"size\":500,\"top_down\":true}"));
    Assert.Equal(-120.5, r.X); Assert.Equal(340.0, r.Z); Assert.Equal(500.0, r.Size);
    Assert.True(r.TopDown, "top_down");

    var d = RequestParse.Screenshot(JsonReader.Parse("{\"x\":0,\"z\":0}"));
    Assert.Equal(1000.0, d.Size);
    Assert.True(!d.TopDown, "top_down defaults false");
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe`
Expected: build FAILS — `RequestParse` has no `Screenshot` / `ScreenshotReq` (a compile error is this harness's "failing test").

- [ ] **Step 3: Implement the parser**

In `mod/src/json/RequestParse.cs`, add the struct next to the other request structs and the parser next to the other parse methods:

```csharp
public struct ScreenshotReq { public float X, Z, Size; public bool TopDown; }
```

```csharp
public static ScreenshotReq Screenshot(JsonValue v)
{
    return new ScreenshotReq
    {
        X = (float)v["x"].AsDouble(),
        Z = (float)v["z"].AsDouble(),
        Size = v["size"].IsNull ? 1000f : (float)v["size"].AsDouble(),
        TopDown = !v["top_down"].IsNull && v["top_down"].AsBool()
    };
}
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe`
Expected: `ok   - parse: screenshot`, 0 failed.

- [ ] **Step 5: Commit**

```bash
git add mod/src/json/RequestParse.cs mod/test/RequestParseTests.cs
git commit -m "feat(mod): parse screenshot capture requests"
```

---

### Task 2: Mod — binary HTTP replies

The mod's `HttpReply` only carries string bodies; PNG responses need bytes.

**Files:**
- Modify: `mod/src/http/HttpServer.cs:11-18` (HttpReply struct) and `:78` (Handle)

- [ ] **Step 1: Extend HttpReply**

```csharp
public struct HttpReply
{
    public int Status;
    public string ContentType;
    public string Body;
    public byte[] Bytes;
    public static HttpReply Json(int status, string body) { return new HttpReply { Status = status, ContentType = "application/json", Body = body }; }
    public static HttpReply Text(int status, string body) { return new HttpReply { Status = status, ContentType = "text/plain", Body = body }; }
    public static HttpReply Png(byte[] bytes) { return new HttpReply { Status = 200, ContentType = "image/png", Bytes = bytes }; }
}
```

- [ ] **Step 2: Use bytes when present in `Handle`**

Replace `byte[] buf = Encoding.UTF8.GetBytes(reply.Body ?? "");` with:

```csharp
byte[] buf = reply.Bytes != null ? reply.Bytes : Encoding.UTF8.GetBytes(reply.Body ?? "");
```

- [ ] **Step 3: Verify the test project still builds and passes**

Run: `cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe`
Expected: all tests pass (no behavior change for string replies).

- [ ] **Step 4: Commit**

```bash
git add mod/src/http/HttpServer.cs
git commit -m "feat(mod): binary (PNG) HTTP reply support"
```

---

### Task 3: Mod — CaptureBehaviour, camera control, /screenshot route

Capture must run on Unity's main/render thread (the existing `SimThread` runs on the sim thread). A `MonoBehaviour` drains a request queue in `Update()` and runs a coroutine per request. Note pre-C#6 Mono: `yield return` cannot sit inside a `try` block that has a `catch` — structure the coroutine accordingly.

**Files:**
- Create: `mod/src/bridge/Capture.cs`
- Modify: `mod/src/bridge/GameAccess.cs` (ModRuntime Start/Stop), `mod/src/http/Router.cs`, `mod/src/http/Handlers.cs`

There is no game-free test harness for Unity-dependent code (same as `GameActions`); this task is verified in-game in Task 10.

- [ ] **Step 1: Create `mod/src/bridge/Capture.cs`**

```csharp
using System;
using System.Collections;
using System.Collections.Generic;
using System.Threading;
using UnityEngine;

namespace SkylineBench.Bridge
{
    public sealed class CaptureRequest
    {
        public float X, Z, Size;
        public bool TopDown;
        public byte[] Png;
        public Exception Error;
        public readonly ManualResetEvent Done = new ManualResetEvent(false);
    }

    /// <summary>Runs screenshot captures on Unity's main thread. The HTTP
    /// thread enqueues a request and blocks on Done; Update() drains the queue
    /// and runs one coroutine per request (the sim is paused between agent
    /// steps, so requests never race game mutations).</summary>
    public sealed class CaptureBehaviour : MonoBehaviour
    {
        private static readonly Queue<CaptureRequest> _queue = new Queue<CaptureRequest>();
        private static readonly object _lock = new object();

        public static byte[] Capture(float x, float z, float size, bool topDown, int timeoutMs)
        {
            var req = new CaptureRequest { X = x, Z = z, Size = size, TopDown = topDown };
            lock (_lock) { _queue.Enqueue(req); }
            if (!req.Done.WaitOne(timeoutMs))
                throw new TimeoutException("screenshot capture timed out after " + timeoutMs + "ms");
            if (req.Error != null) throw req.Error;
            return req.Png;
        }

        private void Update()
        {
            CaptureRequest req = null;
            lock (_lock) { if (_queue.Count > 0) req = _queue.Dequeue(); }
            if (req != null) StartCoroutine(Run(req));
        }

        private IEnumerator Run(CaptureRequest req)
        {
            CameraController cc = null;
            bool prevFree = false;
            try
            {
                cc = ToolsModifierControl.cameraController;
                prevFree = cc.m_freeCamera;
                // Free camera hides the game UI chrome so frames are clean.
                cc.m_freeCamera = true;
                var pos = new Vector3(req.X, 0f, req.Z);
                var angle = req.TopDown ? new Vector2(0f, 90f) : new Vector2(0f, 45f);
                // Setting target AND current skips the easing animation.
                cc.m_targetPosition = pos; cc.m_currentPosition = pos;
                cc.m_targetSize = req.Size; cc.m_currentSize = req.Size;
                cc.m_targetAngle = angle; cc.m_currentAngle = angle;
            }
            catch (Exception e) { req.Error = e; req.Done.Set(); yield break; }

            // Two end-of-frame waits so the moved camera actually renders.
            yield return new WaitForEndOfFrame();
            yield return new WaitForEndOfFrame();

            try
            {
                var tex = new Texture2D(Screen.width, Screen.height, TextureFormat.RGB24, false);
                tex.ReadPixels(new Rect(0f, 0f, Screen.width, Screen.height), 0, 0);
                tex.Apply();
                req.Png = tex.EncodeToPNG();
                UnityEngine.Object.Destroy(tex);
            }
            catch (Exception e) { req.Error = e; }
            finally
            {
                if (cc != null) cc.m_freeCamera = prevFree;
                req.Done.Set();
            }
        }
    }
}
```

- [ ] **Step 2: Create/destroy the behaviour in `ModRuntime` (`mod/src/bridge/GameAccess.cs`)**

```csharp
public static class ModRuntime
{
    private static HttpServer _server;
    private static GameObject _capture;
    public static IThreading Threading { get; private set; }

    public static void SetThreading(IThreading t) { Threading = t; }

    public static void Start()
    {
        if (_server != null) return;
        _server = new HttpServer(8787, Router.Route);
        _server.Start();
        _capture = new GameObject("SkylineBenchCapture");
        _capture.AddComponent<CaptureBehaviour>();
        UnityEngine.Object.DontDestroyOnLoad(_capture);
    }

    public static void Stop()
    {
        if (_server != null) { _server.Stop(); _server = null; }
        if (_capture != null) { UnityEngine.Object.Destroy(_capture); _capture = null; }
        Threading = null;
    }
}
```

Add `using UnityEngine;` to the file's usings.

- [ ] **Step 3: Route and handler**

`mod/src/http/Router.cs` — add to the switch:

```csharp
case "/screenshot": return method == "POST" ? Handlers.Screenshot(body) : MethodNotAllowed();
```

`mod/src/http/Handlers.cs` — add `using System;` and:

```csharp
public static HttpReply Screenshot(string body)
{
    var req = RequestParse.Screenshot(JsonReader.Parse(body));
    try
    {
        byte[] png = Bridge.CaptureBehaviour.Capture(req.X, req.Z, req.Size, req.TopDown, 5000);
        return HttpReply.Png(png);
    }
    catch (Exception e)
    {
        return HttpReply.Json(500, "{\"error\":\"capture_failed\",\"message\":\"" + e.Message.Replace("\"", "'") + "\"}");
    }
}
```

- [ ] **Step 4: Build the mod against game assemblies**

Run: `mod/build.sh`
Expected: compiles and installs `SkylineBenchMod.dll`. If `CameraController` field names fail to compile, check `mod/DISCOVERY.md` and the game's decompiled `CameraController` (fields `m_targetPosition`, `m_currentPosition`, `m_targetSize`, `m_currentSize`, `m_targetAngle`, `m_currentAngle`, `m_freeCamera` exist in CS1 1.21.x).

- [ ] **Step 5: Commit**

```bash
git add mod/src/bridge/Capture.cs mod/src/bridge/GameAccess.cs mod/src/http/Router.cs mod/src/http/Handlers.cs
git commit -m "feat(mod): POST /screenshot — main-thread camera capture to PNG"
```

---

### Task 4: Broker — mock /screenshot endpoint + bridge client method (TDD)

**Files:**
- Modify: `broker/src/mock.rs` (new route), `broker/src/bridge_client.rs` (new method + test)

- [ ] **Step 1: Write the failing test**

In `broker/src/bridge_client.rs` tests:

```rust
#[tokio::test]
async fn fetches_screenshot_png_bytes() {
    let client = BridgeClient::new(start_mock().await);
    let png = client.screenshot(0.0, 0.0, 500.0, true).await.unwrap();
    assert_eq!(&png[1..4], b"PNG");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path broker/Cargo.toml fetches_screenshot_png_bytes`
Expected: compile error — no `screenshot` method.

- [ ] **Step 3: Implement client method and mock route**

`broker/src/bridge_client.rs`:

```rust
pub async fn screenshot(
    &self,
    x: f32,
    z: f32,
    size: f32,
    top_down: bool,
) -> Result<Vec<u8>, BridgeError> {
    let body = serde_json::json!({ "x": x, "z": z, "size": size, "top_down": top_down });
    Ok(self
        .http
        .post(format!("{}/screenshot", self.base))
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?
        .to_vec())
}
```

`broker/src/mock.rs` — a deterministic stand-in: render the mock city's network through the existing synthetic renderer (a real PNG, varies with city state):

```rust
async fn screenshot(
    State(s): State<MockState>,
    Json(_body): Json<serde_json::Value>,
) -> impl axum::response::IntoResponse {
    let net = {
        let c = s.city.lock().unwrap();
        Network { nodes: c.nodes.clone(), segments: c.segments.clone() }
    };
    let opts = crate::render::RenderOptions {
        bounds: crate::geometry::playable_bounds(),
        width_px: 64,
        height_px: 64,
        grid_spacing_m: 0.0,
    };
    let png = crate::render::render_network(&net, &std::collections::HashMap::new(), &opts);
    ([(axum::http::header::CONTENT_TYPE, "image/png")], png)
}
```

Register it where the other routes are built: `.route("/screenshot", post(screenshot))`. Match the `RenderOptions` field names against `broker/src/render.rs` when wiring (they are `bounds`, `width_px`, `height_px`, `grid_spacing_m`).

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --manifest-path broker/Cargo.toml fetches_screenshot_png_bytes`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/bridge_client.rs broker/src/mock.rs
git commit -m "feat(broker): screenshot bridge call + mock endpoint"
```

---

### Task 5: Broker — camera framing in the service layer (TDD)

`CameraShot` lives in `service.rs` so a future agent-facing MCP tool is a thin wrapper. `size` maps to the game's `CameraController.m_targetSize` (≈ vertical view extent in metres); constants get calibrated in-game in Task 10.

**Files:**
- Modify: `broker/src/service.rs`

- [ ] **Step 1: Write failing tests**

In `broker/src/service.rs` tests:

```rust
#[test]
fn overview_shot_frames_the_network_with_margin() {
    let net = crate::contract::Network {
        nodes: vec![
            crate::contract::NetNode { id: 1, x: -1000.0, y: 0.0, z: -500.0 },
            crate::contract::NetNode { id: 2, x: 1000.0, y: 0.0, z: 500.0 },
        ],
        segments: vec![],
    };
    let shot = overview_shot(&net);
    assert_eq!(shot.x, 0.0);
    assert_eq!(shot.z, 0.0);
    assert!(shot.top_down);
    // span 2000m * 1.15 margin / 2 = 1150, below the 1200 floor → clamped.
    assert_eq!(shot.size, 1200.0);
}

#[test]
fn overview_shot_of_empty_network_uses_default_frame() {
    let net = crate::contract::Network { nodes: vec![], segments: vec![] };
    let shot = overview_shot(&net);
    assert_eq!((shot.x, shot.z), (0.0, 0.0));
    assert_eq!(shot.size, 2000.0);
}

#[test]
fn closeup_shot_targets_the_location() {
    let shot = closeup_shot(150.0, -75.0);
    assert_eq!((shot.x, shot.z), (150.0, -75.0));
    assert!(!shot.top_down);
    assert_eq!(shot.size, 350.0);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path broker/Cargo.toml overview_shot`
Expected: compile error — no `overview_shot`/`CameraShot`.

- [ ] **Step 3: Implement**

In `broker/src/service.rs`:

```rust
#[derive(Debug, Clone, Copy)]
pub struct CameraShot {
    pub x: f32,
    pub z: f32,
    pub size: f32,
    pub top_down: bool,
}

/// Floor for the overview zoom so tiny networks aren't framed from 10 m up.
const OVERVIEW_MIN_SIZE_M: f32 = 1200.0;
const OVERVIEW_MARGIN: f32 = 1.15;
/// Close-up zoom: wide enough to show an intersection plus surroundings.
const CLOSEUP_SIZE_M: f32 = 350.0;

pub fn overview_shot(net: &crate::contract::Network) -> CameraShot {
    let bounds = net.nodes.iter().fold(None, |acc, n| {
        let (min_x, max_x, min_z, max_z) = acc.unwrap_or((n.x, n.x, n.z, n.z));
        Some((min_x.min(n.x), max_x.max(n.x), min_z.min(n.z), max_z.max(n.z)))
    });
    match bounds {
        None => CameraShot { x: 0.0, z: 0.0, size: 2000.0, top_down: true },
        Some((min_x, max_x, min_z, max_z)) => CameraShot {
            x: (min_x + max_x) / 2.0,
            z: (min_z + max_z) / 2.0,
            size: ((max_x - min_x).max(max_z - min_z) * OVERVIEW_MARGIN / 2.0)
                .max(OVERVIEW_MIN_SIZE_M),
            top_down: true,
        },
    }
}

pub fn closeup_shot(x: f32, z: f32) -> CameraShot {
    CameraShot { x, z, size: CLOSEUP_SIZE_M, top_down: false }
}

pub async fn capture_screenshot(
    client: &BridgeClient,
    shot: CameraShot,
) -> Result<Vec<u8>, ServiceError> {
    Ok(client.screenshot(shot.x, shot.z, shot.size, shot.top_down).await?)
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --manifest-path broker/Cargo.toml --lib service`
Expected: new tests PASS, existing service tests still PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/service.rs
git commit -m "feat(broker): camera framing + capture_screenshot service call"
```

---

### Task 6: Broker — ScreenshotSink persistence module (TDD)

A self-contained sink that captures via the bridge and persists frames + `index.jsonl`, with the spec's failure policy: any capture/persist failure logs and disables the sink for the rest of the run (telemetry must never fail a benchmark, and a missing endpoint on an older mod must not add latency to every step).

**Files:**
- Create: `broker/src/benchmark/screenshots.rs`
- Modify: `broker/src/benchmark/mod.rs` (add `pub mod screenshots;` next to the other module declarations)

- [ ] **Step 1: Write failing tests**

In `broker/src/benchmark/screenshots.rs` (tests live in the same file, mirroring sibling modules):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark::config::BenchConfig;
    use crate::benchmark::state::RunState;
    use crate::bridge_client::BridgeClient;
    use crate::mock;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    async fn sink_with_mock(dir: &std::path::Path) -> (ScreenshotSink, Arc<BridgeClient>, Arc<Mutex<RunState>>) {
        let (addr, server) = mock::bind("127.0.0.1:0".parse().unwrap()).await;
        tokio::spawn(server);
        let client = Arc::new(BridgeClient::new(format!("http://{addr}")));
        let state = Arc::new(Mutex::new(RunState::new(BenchConfig::default(), HashMap::new())));
        (ScreenshotSink::new(dir.to_path_buf()), client, state)
    }

    #[tokio::test]
    async fn persists_overview_and_action_frames_with_indexes() {
        let dir = std::env::temp_dir().join(format!("sb-shots-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let (sink, client, state) = sink_with_mock(&dir).await;

        let shot = crate::service::overview_shot(&crate::contract::Network { nodes: vec![], segments: vec![] });
        sink.capture(&client, &state, shot, Stream::Overview, "step", None).await;
        sink.capture(&client, &state, crate::service::closeup_shot(10.0, 20.0), Stream::Action, "build_road",
            Some("build_road: road".into())).await;

        let overview = std::fs::read_to_string(dir.join("overview/index.jsonl")).unwrap();
        let entry: serde_json::Value = serde_json::from_str(overview.lines().next().unwrap()).unwrap();
        assert_eq!(entry["seq"], 1);
        assert_eq!(entry["trigger"], "step");
        assert!(entry["tick"].is_u64());
        assert!(dir.join("overview").join(entry["file"].as_str().unwrap()).exists());

        let actions = std::fs::read_to_string(dir.join("actions/index.jsonl")).unwrap();
        let entry: serde_json::Value = serde_json::from_str(actions.lines().next().unwrap()).unwrap();
        assert_eq!(entry["action"], "build_road");
        assert_eq!(entry["caption"], "build_road: road");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn disables_itself_after_a_capture_failure() {
        let dir = std::env::temp_dir().join(format!("sb-shots-fail-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        // Point at a dead address: every capture errors.
        let client = Arc::new(BridgeClient::new("http://127.0.0.1:1"));
        let state = Arc::new(Mutex::new(RunState::new(BenchConfig::default(), HashMap::new())));
        let sink = ScreenshotSink::new(dir.clone());

        sink.capture(&client, &state, crate::service::closeup_shot(0.0, 0.0), Stream::Action, "bulldoze", None).await;
        assert!(sink.disabled(), "first failure disables the sink");
        sink.capture(&client, &state, crate::service::closeup_shot(0.0, 0.0), Stream::Action, "bulldoze", None).await;
        assert!(!dir.join("actions/index.jsonl").exists(), "no frames after disable");
        std::fs::remove_dir_all(&dir).ok();
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path broker/Cargo.toml screenshots`
Expected: compile error — module doesn't exist yet (add the `pub mod screenshots;` declaration first so the error is about the missing types).

- [ ] **Step 3: Implement the sink**

`broker/src/benchmark/screenshots.rs`:

```rust
//! Real in-game screenshot persistence (spec: 2026-06-11 timelapse design).
//!
//! Best-effort telemetry: a failed capture logs once and disables the sink for
//! the rest of the run — never fails the tool call, never retries per-frame
//! against a mod that lacks the endpoint.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use tokio::sync::Mutex;

use crate::benchmark::state::RunState;
use crate::bridge_client::BridgeClient;
use crate::service::CameraShot;

#[derive(Clone, Copy)]
pub enum Stream {
    Overview,
    Action,
}

impl Stream {
    fn subdir(self) -> &'static str {
        match self {
            Stream::Overview => "overview",
            Stream::Action => "actions",
        }
    }
}

pub struct ScreenshotSink {
    dir: PathBuf,
    overview_seq: AtomicU64,
    action_seq: AtomicU64,
    disabled: AtomicBool,
}

impl ScreenshotSink {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            overview_seq: AtomicU64::new(0),
            action_seq: AtomicU64::new(0),
            disabled: AtomicBool::new(false),
        }
    }

    pub fn disabled(&self) -> bool {
        self.disabled.load(Ordering::Relaxed)
    }

    pub async fn capture(
        &self,
        client: &BridgeClient,
        state: &Mutex<RunState>,
        shot: CameraShot,
        stream: Stream,
        trigger: &str,
        caption: Option<String>,
    ) {
        if self.disabled() {
            return;
        }
        let png = match client.screenshot(shot.x, shot.z, shot.size, shot.top_down).await {
            Ok(png) => png,
            Err(e) => {
                eprintln!("benchmark: screenshot capture failed ({e}); disabling screenshots for this run");
                self.disabled.store(true, Ordering::Relaxed);
                return;
            }
        };
        let tick = client.health().await.map(|h| h.tick).unwrap_or(0);
        let seq = match stream {
            Stream::Overview => self.overview_seq.fetch_add(1, Ordering::Relaxed) + 1,
            Stream::Action => self.action_seq.fetch_add(1, Ordering::Relaxed) + 1,
        };
        let (changes, flow, congested) = {
            let s = state.lock().await;
            (s.num_changes, s.flow.mean(), (!s.congestion.is_empty()).then(|| s.congestion.mean()))
        };
        let dir = self.dir.join(stream.subdir());
        let name = format!("{seq:05}-tick{tick}.png");
        let written = std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(dir.join(&name), &png))
            .and_then(|()| {
                let action = matches!(stream, Stream::Action).then_some(trigger);
                let line = serde_json::json!({
                    "seq": seq, "file": name, "tick": tick, "trigger": trigger,
                    "changes": changes, "flow": flow, "congested": congested,
                    "action": action, "caption": caption,
                });
                let mut f = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join("index.jsonl"))?;
                use std::io::Write;
                writeln!(f, "{line}")
            });
        if let Err(e) = written {
            eprintln!("benchmark: screenshot persist failed ({e}); disabling screenshots for this run");
            self.disabled.store(true, Ordering::Relaxed);
        }
    }
}
```

Check `RunState`'s field names against `broker/src/benchmark/state.rs` while implementing — `num_changes`, `flow.mean()`, `congestion.is_empty()`/`.mean()` are the names used by `persist_render` in `broker/src/benchmark/server.rs:102-134`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --manifest-path broker/Cargo.toml screenshots`
Expected: both tests PASS.

- [ ] **Step 5: Commit**

```bash
git add broker/src/benchmark/screenshots.rs broker/src/benchmark/mod.rs
git commit -m "feat(broker): ScreenshotSink — capture + index persistence with fail-safe disable"
```

---### Task 7: Broker — benchmark server hooks (TDD)

Wire the sink into `BenchmarkServer`: overview after every successful step, close-up after every successful mutating action, one framing close-up per `apply_plan`.

**Files:**
- Modify: `broker/src/benchmark/server.rs`

- [ ] **Step 1: Write failing tests**

In `broker/src/benchmark/server.rs` tests:

```rust
#[tokio::test]
async fn step_persists_an_overview_screenshot() {
    let dir = std::env::temp_dir().join(format!("sb-srv-shots-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    let bench = bench_with_mock().await.with_screenshots_dir(dir.clone());
    bench
        .control_time(Parameters(crate::service::ControlTimeArgs {
            op: "step".into(),
            ticks: Some(10),
            speed: None,
        }))
        .await
        .unwrap();
    let index = std::fs::read_to_string(dir.join("overview/index.jsonl")).unwrap();
    let entry: serde_json::Value = serde_json::from_str(index.lines().next().unwrap()).unwrap();
    assert_eq!(entry["trigger"], "step");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn successful_build_persists_an_action_closeup() {
    let dir = std::env::temp_dir().join(format!("sb-srv-shots2-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    let bench = bench_with_mock().await.with_screenshots_dir(dir.clone());
    bench
        .build_road(Parameters(crate::service::BuildRoadArgs {
            from: crate::contract::Position { x: 0.0, y: 0.0, z: 0.0 },
            to: crate::contract::Position { x: 50.0, y: 0.0, z: 0.0 },
            road_type: "road".into(),
            snap: true,
        }))
        .await
        .unwrap();
    let index = std::fs::read_to_string(dir.join("actions/index.jsonl")).unwrap();
    let entry: serde_json::Value = serde_json::from_str(index.lines().next().unwrap()).unwrap();
    assert_eq!(entry["action"], "build_road");
    assert_eq!(entry["caption"], "build_road: road");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn failed_action_persists_no_screenshot() {
    let dir = std::env::temp_dir().join(format!("sb-srv-shots3-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    let bench = bench_with_mock().await.with_screenshots_dir(dir.clone());
    bench
        .bulldoze(Parameters(crate::service::BulldozeArgs { target_type: "segment".into(), id: 9999 }))
        .await
        .unwrap();
    assert!(!dir.join("actions/index.jsonl").exists());
    std::fs::remove_dir_all(&dir).ok();
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path broker/Cargo.toml step_persists_an_overview`
Expected: compile error — no `with_screenshots_dir`.

- [ ] **Step 3: Implement the wiring**

In `BenchmarkServer`:

1. Add field `screenshots: Option<Arc<crate::benchmark::screenshots::ScreenshotSink>>` (initialize `None` in `new`, thread through the struct-update constructors like `with_renders_dir` does) and:

```rust
pub fn with_screenshots_dir(self, dir: std::path::PathBuf) -> Self {
    Self {
        screenshots: Some(Arc::new(crate::benchmark::screenshots::ScreenshotSink::new(dir))),
        ..self
    }
}
```

2. Private helpers:

```rust
async fn shoot_overview(&self) {
    let Some(sink) = &self.screenshots else { return };
    let Ok(net) = self.client.network().await else { return };
    sink.capture(
        &self.client,
        &self.state,
        crate::service::overview_shot(&net),
        crate::benchmark::screenshots::Stream::Overview,
        "step",
        None,
    )
    .await;
}

async fn shoot_action(&self, x: f32, z: f32, action: &str, caption: String) {
    let Some(sink) = &self.screenshots else { return };
    sink.capture(
        &self.client,
        &self.state,
        crate::service::closeup_shot(x, z),
        crate::benchmark::screenshots::Stream::Action,
        action,
        Some(caption),
    )
    .await;
}
```

3. Hook points (all gated on the action's `ok == true`, matching where `record_mutation` is called):
   - `control_time` step branch: immediately after the existing auto-render block (`server.rs:406-421`), add `self.shoot_overview().await;`
   - `build_road`: location is the midpoint — capture `let (mx, mz) = ((args.from.x + args.to.x) / 2.0, (args.from.z + args.to.z) / 2.0);` before `args` is moved, then after a successful mutation: `self.shoot_action(mx, mz, "build_road", format!("build_road: {road_type}")).await;`
   - `upgrade_road`: the handler already fetches the network for the segment length; extend that lookup to also compute the segment midpoint from its endpoint nodes (`nodes` lookup by `start_node`/`end_node`, average x/z; fall back to `(0.0, 0.0)` and skip the screenshot if the segment isn't found). After success: `self.shoot_action(mx, mz, "upgrade_road", format!("upgrade_road: segment {segment_id} → {road_type}")).await;`
   - `bulldoze`: the target is gone after the action, so resolve its location *before* calling the service: fetch `self.client.network().await` and find the segment midpoint / node position by `args.id` (for `"building"`, look it up in `self.client.buildings()`). Only fetch when `self.screenshots.is_some()`. After success, if a location was found: `self.shoot_action(x, z, "bulldoze", format!("bulldoze: {target_type} {id}")).await;`
   - `set_zoning`: rect center `((area.min_x + area.max_x) / 2.0, (area.min_z + area.max_z) / 2.0)`; caption `format!("set_zoning: {zone_type}")`.
   - `apply_plan`: after the execution loop (only when not `validate_only` and at least one op executed `ok`), compute the bounding-box center of executed ops' positions (Build: from/to; Upgrade/Bulldoze segment: midpoint via the `net` already fetched at the top; Zone: rect center) and `self.shoot_action(cx, cz, "apply_plan", format!("apply_plan: {n} ops")).await;` where `n` is the count of ops with `ok == true`.

- [ ] **Step 4: Run the full broker suite**

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: new tests PASS; all existing tests PASS (servers built without `with_screenshots_dir` are unaffected).

- [ ] **Step 5: Commit**

```bash
git add broker/src/benchmark/server.rs
git commit -m "feat(broker): screenshot hooks — overview per step, close-up per action"
```

---

### Task 8: Broker — CLI flag + run.sh wiring

**Files:**
- Modify: `broker/src/main.rs` (Benchmark subcommand), `benchmark/run.sh`

- [ ] **Step 1: Add `--screenshots-dir` to the Benchmark subcommand**

In `broker/src/main.rs`, next to `renders_dir`:

```rust
/// Directory for real in-game screenshot frames (timelapse). Omit to disable.
#[arg(long)]
screenshots_dir: Option<std::path::PathBuf>,
```

Destructure it in the match arm and extend the server construction:

```rust
let server = {
    let s = BenchmarkServer::new(client, state.clone()).with_persist(persister.clone());
    let s = match renders_dir {
        Some(dir) => s.with_renders_dir(dir),
        None => s,
    };
    match screenshots_dir {
        Some(dir) => s.with_screenshots_dir(dir),
        None => s,
    }
}
```

- [ ] **Step 2: Verify it builds**

Run: `cargo build --manifest-path broker/Cargo.toml`
Expected: clean build.

- [ ] **Step 3: Wire run.sh**

In `benchmark/run.sh`:
- In the `mcp.json` heredoc (line 107), append `--screenshots-dir $SESSION_DIR/screenshots` to the broker args string.
- After the renders move block (lines 149-151), add:

```bash
if [ -d "$SESSION_DIR/screenshots" ]; then
  mv "$SESSION_DIR/screenshots" "$OUT_DIR/screenshots"
fi
```

- [ ] **Step 4: Dry-run check**

Run: `DRY_RUN=1 benchmark/run.sh --map test-map`
Expected: printed mcp.json contains `--screenshots-dir`.

- [ ] **Step 5: Commit**

```bash
git add broker/src/main.rs benchmark/run.sh
git commit -m "feat(benchmark): --screenshots-dir flag wired through run.sh"
```

---

### Task 9: Broker — timelapse module + subcommand (TDD)

Frame merge, HUD annotation, and video assembly. New dep: `ab_glyph` (tiny-skia has no text). Font embedded from `broker/assets/`.

**Files:**
- Create: `broker/src/timelapse.rs`, `broker/assets/DejaVuSans.ttf`
- Modify: `broker/Cargo.toml`, `broker/src/lib.rs` (add `pub mod timelapse;`), `broker/src/main.rs` (subcommand)

- [ ] **Step 1: Add the dependency and font asset**

In `broker/Cargo.toml` dependencies: `ab_glyph = "0.2"`.

```bash
curl -L -o /tmp/dejavu.zip https://github.com/dejavu-fonts/dejavu-fonts/releases/download/version_2_37/dejavu-fonts-ttf-2.37.zip
mkdir -p broker/assets
unzip -j -o /tmp/dejavu.zip dejavu-fonts-ttf-2.37/ttf/DejaVuSans.ttf -d broker/assets/
```

(DejaVu's license is free/redistributable. If offline, copy any redistributable .ttf to `broker/assets/DejaVuSans.ttf`.)

- [ ] **Step 2: Write failing tests**

In `broker/src/timelapse.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn frame(tick: u64, hold: u32) -> Frame {
        Frame {
            path: std::path::PathBuf::from(format!("f{tick}.png")),
            tick,
            changes: 0,
            flow: Some(50.0),
            congested: Some(1000.0),
            caption: None,
            hold,
        }
    }

    #[test]
    fn merge_orders_by_tick_with_overview_before_actions() {
        let overview = vec![frame(100, 1), frame(200, 1)];
        let actions = vec![frame(100, 3)];
        let merged = merge_frames(overview, actions);
        let key: Vec<(u64, u32)> = merged.iter().map(|f| (f.tick, f.hold)).collect();
        assert_eq!(key, vec![(100, 1), (100, 3), (200, 1)]);
    }

    #[test]
    fn parse_index_skips_malformed_lines() {
        let dir = std::env::temp_dir().join(format!("sb-tl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("index.jsonl"),
            "{\"seq\":1,\"file\":\"a.png\",\"tick\":5,\"trigger\":\"step\",\"changes\":0,\"flow\":50.0,\"congested\":null}\nnot json\n",
        )
        .unwrap();
        let frames = parse_index(&dir, 1);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].tick, 5);
        assert_eq!(frames[0].path, dir.join("a.png"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn annotate_adds_a_hud_strip() {
        let net = crate::contract::Network { nodes: vec![], segments: vec![] };
        let opts = crate::render::RenderOptions {
            bounds: crate::geometry::playable_bounds(),
            width_px: 320,
            height_px: 200,
            grid_spacing_m: 0.0,
        };
        let png = crate::render::render_network(&net, &std::collections::HashMap::new(), &opts);
        let out = annotate(&png, &frame(123, 1)).unwrap();
        let decoded = tiny_skia::Pixmap::decode_png(&out).unwrap();
        assert_eq!(decoded.width(), 320);
        assert_eq!(decoded.height(), 200 + HUD_HEIGHT_PX);
    }
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test --manifest-path broker/Cargo.toml timelapse`
Expected: compile error — module missing (add `pub mod timelapse;` to `broker/src/lib.rs` first).

- [ ] **Step 4: Implement the module**

`broker/src/timelapse.rs`:

```rust
//! Post-run timelapse assembly: merge frame indexes, burn a HUD strip into
//! each frame, and drive ffmpeg to produce an mp4.

use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const HUD_HEIGHT_PX: u32 = 40;
/// Action close-ups are duplicated this many times so they hold on screen.
const ACTION_HOLD: u32 = 3;

static FONT_BYTES: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");

#[derive(Debug, Clone)]
pub struct Frame {
    pub path: PathBuf,
    pub tick: u64,
    pub changes: u64,
    pub flow: Option<f64>,
    pub congested: Option<f64>,
    pub caption: Option<String>,
    pub hold: u32,
}

#[derive(Deserialize)]
struct IndexEntry {
    file: String,
    tick: u64,
    changes: u64,
    flow: Option<f64>,
    congested: Option<f64>,
    #[serde(default)]
    caption: Option<String>,
}

pub fn parse_index(dir: &Path, hold: u32) -> Vec<Frame> {
    let Ok(raw) = std::fs::read_to_string(dir.join("index.jsonl")) else {
        return vec![];
    };
    raw.lines()
        .filter_map(|line| match serde_json::from_str::<IndexEntry>(line) {
            Ok(e) => Some(Frame {
                path: dir.join(&e.file),
                tick: e.tick,
                changes: e.changes,
                flow: e.flow,
                congested: e.congested,
                caption: e.caption,
                hold,
            }),
            Err(err) => {
                eprintln!("timelapse: skipping malformed index line ({err})");
                None
            }
        })
        .collect()
}

/// Chronological merge; at equal ticks the overview (hold 1) precedes the
/// action close-ups taken while paused at that tick.
pub fn merge_frames(overview: Vec<Frame>, actions: Vec<Frame>) -> Vec<Frame> {
    let mut all: Vec<Frame> = overview.into_iter().chain(actions).collect();
    all.sort_by_key(|f| (f.tick, f.hold));
    all
}

fn hud_line(f: &Frame) -> String {
    let flow = f.flow.map(|v| format!("{v:.1}%")).unwrap_or_else(|| "—".into());
    let congested = f.congested.map(|v| format!("{v:.0}m")).unwrap_or_else(|| "—".into());
    let caption = f.caption.as_deref().unwrap_or("");
    format!("tick {}  flow {}  congested {}  changes {}  {}", f.tick, flow, congested, f.changes, caption)
}

fn draw_text(pixmap: &mut tiny_skia::Pixmap, x: f32, baseline_y: f32, px: f32, text: &str) {
    use ab_glyph::{Font, FontRef, ScaleFont};
    let font = FontRef::try_from_slice(FONT_BYTES).expect("embedded font parses");
    let scaled = font.as_scaled(px);
    let width = pixmap.width();
    let height = pixmap.height();
    text.chars().fold(x, |caret, ch| {
        let mut glyph = scaled.scaled_glyph(ch);
        glyph.position = ab_glyph::point(caret, baseline_y);
        let advance = scaled.h_advance(glyph.id);
        if let Some(outline) = scaled.outline_glyph(glyph) {
            let bb = outline.px_bounds();
            outline.draw(|gx, gy, c| {
                let (px_x, px_y) = (bb.min.x as i64 + gx as i64, bb.min.y as i64 + gy as i64);
                if px_x >= 0 && px_y >= 0 && (px_x as u32) < width && (px_y as u32) < height {
                    let idx = (px_y as usize * width as usize + px_x as usize) * 4;
                    let a = (c * 255.0) as u8;
                    let data = pixmap.data_mut();
                    data[idx] = data[idx].max(a);
                    data[idx + 1] = data[idx + 1].max(a);
                    data[idx + 2] = data[idx + 2].max(a);
                    data[idx + 3] = 255;
                }
            });
        }
        caret + advance
    });
}

/// Decode a frame PNG, extend the canvas with a black HUD strip at the bottom,
/// draw the metadata line, re-encode.
pub fn annotate(png: &[u8], frame: &Frame) -> Result<Vec<u8>, anyhow::Error> {
    let src = tiny_skia::Pixmap::decode_png(png)
        .map_err(|e| anyhow::anyhow!("frame decode failed: {e}"))?;
    let mut out = tiny_skia::Pixmap::new(src.width(), src.height() + HUD_HEIGHT_PX)
        .ok_or_else(|| anyhow::anyhow!("zero-sized frame"))?;
    out.fill(tiny_skia::Color::BLACK);
    out.draw_pixmap(
        0,
        0,
        src.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        tiny_skia::Transform::identity(),
        None,
    );
    let baseline = src.height() as f32 + HUD_HEIGHT_PX as f32 * 0.7;
    draw_text(&mut out, 8.0, baseline, HUD_HEIGHT_PX as f32 * 0.55, &hud_line(frame));
    out.encode_png().map_err(|e| anyhow::anyhow!("frame encode failed: {e}"))
}

/// Assemble `<run_dir>` frames into an mp4. Prefers real screenshots; falls
/// back to synthetic renders for runs captured before screenshots existed.
pub fn assemble(run_dir: &Path, fps: u32, out: &Path) -> Result<(), anyhow::Error> {
    let shots = run_dir.join("screenshots");
    let frames = if shots.is_dir() {
        merge_frames(
            parse_index(&shots.join("overview"), 1),
            parse_index(&shots.join("actions"), ACTION_HOLD),
        )
    } else {
        parse_index(&run_dir.join("renders"), 1)
    };
    anyhow::ensure!(!frames.is_empty(), "no frames found under {}", run_dir.display());

    let staging = run_dir.join("timelapse-frames");
    std::fs::create_dir_all(&staging)?;
    let total: u32 = frames.iter().try_fold(0u32, |n, f| {
        let png = std::fs::read(&f.path)?;
        let annotated = annotate(&png, f)?;
        (0..f.hold).try_fold(n, |n, _| {
            std::fs::write(staging.join(format!("{:06}.png", n + 1)), &annotated)?;
            Ok::<u32, anyhow::Error>(n + 1)
        })
    })?;
    eprintln!("timelapse: {total} frames staged; running ffmpeg…");

    let status = std::process::Command::new("ffmpeg")
        .args(["-y", "-framerate", &fps.to_string(), "-i"])
        .arg(staging.join("%06d.png"))
        .args(["-pix_fmt", "yuv420p"])
        .arg(out)
        .status()
        .map_err(|e| anyhow::anyhow!("could not run ffmpeg ({e}) — install it with `brew install ffmpeg`"))?;
    anyhow::ensure!(status.success(), "ffmpeg exited with {status}");
    std::fs::remove_dir_all(&staging).ok();
    eprintln!("timelapse: wrote {}", out.display());
    Ok(())
}
```

Note: frames from different streams may differ in pixel size (game window vs renders) — ffmpeg rejects mixed sizes. Real screenshots are all window-sized so the two streams match; the renders fallback is uniformly 1024×1024 plus agent renders which may vary. If mixed sizes turn up in practice, add `-vf scale` later; out of scope now.

- [ ] **Step 5: Add the subcommand**

In `broker/src/main.rs`:

```rust
/// Assemble a run's frames (screenshots, or renders as fallback) into an
/// annotated timelapse mp4. Requires ffmpeg.
Timelapse {
    run_dir: std::path::PathBuf,
    #[arg(long, default_value_t = 4)]
    fps: u32,
    /// Output path (default: <run_dir>/timelapse.mp4).
    #[arg(long)]
    out: Option<std::path::PathBuf>,
},
```

Match arm:

```rust
Command::Timelapse { run_dir, fps, out } => {
    let out = out.unwrap_or_else(|| run_dir.join("timelapse.mp4"));
    skylinebench::timelapse::assemble(&run_dir, fps, &out)?;
}
```

- [ ] **Step 6: Run to verify pass**

Run: `cargo test --manifest-path broker/Cargo.toml timelapse && cargo build --manifest-path broker/Cargo.toml`
Expected: 3 tests PASS, clean build.

- [ ] **Step 7: Smoke-test against an existing run (renders fallback)**

```bash
cargo run --manifest-path broker/Cargo.toml -- timelapse benchmark/runs/20260610-150754
open benchmark/runs/20260610-150754/timelapse.mp4
```

Expected: an mp4 plays showing the 18 synthetic frames with a readable HUD strip (tick/flow/congested/changes).

- [ ] **Step 8: Commit**

```bash
git add broker/Cargo.toml broker/Cargo.lock broker/assets/DejaVuSans.ttf broker/src/timelapse.rs broker/src/lib.rs broker/src/main.rs
git commit -m "feat(broker): timelapse subcommand — HUD-annotated mp4 from run frames"
```

---

### Task 10: In-game verification + docs

The mod's capture path can only be verified against the real game. **Requires the operator**: Cities: Skylines running on this Mac with a city loaded and the rebuilt mod enabled.

**Files:**
- Modify: `benchmark/README.md`, `mod/README.md` (and `mod/DISCOVERY.md` if camera findings differ)

- [ ] **Step 1: Rebuild + reload the mod, then curl a capture**

```bash
mod/build.sh   # then restart the game / reload the save
curl -s -X POST localhost:8787/screenshot \
  -d '{"x":0,"z":0,"size":2000,"top_down":true}' -o /tmp/shot.png
file /tmp/shot.png && open /tmp/shot.png
```

Expected: `PNG image data` at the game window's resolution; a real top-down city view with no UI chrome. Verify: framing matches the requested area; a second curl with `"size":350,"top_down":false` gives an angled close-up. If the zoom looks wrong, calibrate `OVERVIEW_MIN_SIZE_M` / `CLOSEUP_SIZE_M` / the margin in `broker/src/service.rs` (`m_targetSize` semantics are only pinned by this experiment) and record the finding in `mod/DISCOVERY.md`.

- [ ] **Step 2: Short end-to-end run**

```bash
benchmark/run.sh --map <test-map-id>
```

(Interrupt-friendly: even a few agent steps suffice.) Expected: `benchmark/runs/<ts>/screenshots/overview/*.png` after steps, `actions/*.png` after the agent's first build/zoning, both with `index.jsonl`.

- [ ] **Step 3: Build the real timelapse**

```bash
broker/target/release/skylinebench timelapse benchmark/runs/<ts>
open benchmark/runs/<ts>/timelapse.mp4
```

Expected: real game frames, action close-ups held longer, HUD readable.

- [ ] **Step 4: Update docs**

- `benchmark/README.md`: replace the manual ffmpeg one-liner (line 28) with the `skylinebench timelapse <run-dir>` command; document `screenshots/{overview,actions}/` in the run-artifacts list and the fail-safe (screenshots disable themselves if the mod lacks the endpoint).
- `mod/README.md`: document `POST /screenshot` (body fields, PNG response, 500 on failure, ~5s timeout).

- [ ] **Step 5: Commit**

```bash
git add benchmark/README.md mod/README.md mod/DISCOVERY.md
git commit -m "docs: screenshot endpoint + timelapse workflow"
```

---

## Self-Review Notes

- Spec coverage: mod endpoint (T1-3), bridge/service (T4-5), persistence + failure isolation (T6), step/action hooks incl. apply_plan (T7), CLI flag + run.sh + auto-off-without-flag (T8), timelapse with HUD/holds/fallback/ffmpeg error (T9), in-game calibration + docs (T10). Future agent-tool option satisfied by `capture_screenshot` living in `service.rs` (T5).
- The spec's `--screenshots off` materialized as an opt-in `--screenshots-dir` (mirroring `--renders-dir`); run.sh always passes it. Off in mock/tests because nothing passes the flag.
- Type consistency: `CameraShot`/`overview_shot`/`closeup_shot` (T5) used in T6/T7; `ScreenshotSink::capture(client, state, shot, stream, trigger, caption)` signature consistent between T6 impl and T6/T7 tests; `Frame.hold` drives both merge ordering and duplication in T9.
