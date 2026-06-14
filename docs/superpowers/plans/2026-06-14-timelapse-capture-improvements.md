# Timelapse Capture Improvements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Improve the run timelapse so it shows the AI's individual road edits, the city-wide congestion change (red/green traffic layer on overviews), a tighter rotated overview, and a recorded begin/end highway flyby with moving cars.

**Architecture:** Generalize the screenshot camera control once (explicit yaw/pitch + a traffic info-view flag), then layer four features on it: a frame-filling rotated overview with the traffic layer, per-op before/after capture inside `apply_plan`, and a recorded flyby driven by a new in-mod coroutine that interpolates the camera along the main-highway keyframes while the sim runs (cars move), assembled by ffmpeg.

**Tech Stack:** Rust (broker: `service.rs`, `bridge_client.rs`, `benchmark/*.rs`, `timelapse.rs`, `main.rs`), C# / Unity + ColossalFramework (CS1 mod: `bridge/Capture.cs`, `http/*.cs`, `json/RequestParse.cs`), ffmpeg.

**Spec:** `docs/superpowers/specs/2026-06-14-timelapse-capture-improvements-design.md`

---

## File Structure

**Broker (Rust):**
- `broker/src/service.rs` — `InfoView` enum, `CameraShot` fields (yaw/pitch/info_view), `overview_shot`/`closeup_shot`/`region_shot`, new `CameraKeyframe` + `highway_flyby_path`.
- `broker/src/bridge_client.rs` — `screenshot(...)` new params; new `flyby(...)`.
- `broker/src/benchmark/screenshots.rs` — `grab` passes new camera fields; expose sink dir.
- `broker/src/benchmark/server.rs` — `apply_plan` per-op pairs; begin-flyby in `ensure_baseline`.
- `broker/src/benchmark/measure.rs` / `broker/src/main.rs` — end-flyby in the `benchmark-finalize` command.
- `broker/src/timelapse.rs` — flyby assembly (`flyby_start.mp4`/`flyby_end.mp4`) + concat into `timelapse.mp4`.
- `broker/src/mock.rs` — `/flyby` mock that writes stub frames.

**Mod (C#):**
- `mod/src/json/RequestParse.cs` — `ScreenshotReq` new fields; new `FlybyReq`.
- `mod/src/bridge/Capture.cs` — `Capture(...)` new params + info-view toggle; new `Flyby(...)` + recording coroutine.
- `mod/src/http/Handlers.cs` + `mod/src/http/Router.cs` — `/flyby` route.
- `mod/test/RequestParseTests.cs` + `mod/test/FlybyMathTests.cs` — parse + Catmull-Rom tests.

**Build/test commands:**
- Rust: `cargo test -p skylinebench <name>` (run from `broker/`), `cargo build` from `broker/`.
- C#: `cd mod && ./build.sh` to compile the mod; `cd mod/test && <run TestRunner>` for the pure-logic tests.

---

## Group 1 — Shared camera plumbing

### Task 1: `InfoView` enum + `CameraShot` fields (Rust)

**Files:**
- Modify: `broker/src/service.rs:403-451`
- Test: `broker/src/service.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)] mod tests` in `service.rs` (create the module if absent):

```rust
#[test]
fn camera_shots_carry_yaw_pitch_and_info_view() {
    let cu = closeup_shot(10.0, 20.0);
    assert_eq!(cu.pitch, 45.0, "close-ups use the angled game tilt");
    assert_eq!(cu.yaw, 0.0);
    assert!(matches!(cu.info_view, InfoView::None), "close-ups stay a clean render");
    assert_eq!(InfoView::Traffic.as_str(), "traffic");
    assert_eq!(InfoView::None.as_str(), "none");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd broker && cargo test -p skylinebench camera_shots_carry`
Expected: FAIL — `InfoView` not found / no field `yaw`.

- [ ] **Step 3: Implement**

Replace the `CameraShot` struct and the three shot constructors in `service.rs`. Add `InfoView` above `CameraShot`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoView {
    None,
    Traffic,
}

impl InfoView {
    pub fn as_str(self) -> &'static str {
        match self {
            InfoView::None => "none",
            InfoView::Traffic => "traffic",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CameraShot {
    pub x: f32,
    pub z: f32,
    pub size: f32,
    /// Camera heading in degrees (0 = north-up).
    pub yaw: f32,
    /// Camera tilt in degrees (90 = straight down, 45 = default game tilt).
    pub pitch: f32,
    pub info_view: InfoView,
}
```

Update `overview_shot` (keep top-down behaviour for now — Task 6 reworks framing):

```rust
pub fn overview_shot(net: &crate::contract::Network) -> CameraShot {
    let bounds = trimmed_bounds(net.nodes.iter().map(|n| n.x))
        .zip(trimmed_bounds(net.nodes.iter().map(|n| n.z)));
    match bounds {
        None => CameraShot { x: 0.0, z: 0.0, size: 2000.0, yaw: 0.0, pitch: 90.0, info_view: InfoView::None },
        Some(((min_x, max_x), (min_z, max_z))) => CameraShot {
            x: (min_x + max_x) / 2.0,
            z: (min_z + max_z) / 2.0,
            size: ((max_x - min_x).max(max_z - min_z) * OVERVIEW_MARGIN / 2.0)
                .max(OVERVIEW_MIN_SIZE_M),
            yaw: 0.0,
            pitch: 90.0,
            info_view: InfoView::None,
        },
    }
}

pub fn closeup_shot(x: f32, z: f32) -> CameraShot {
    CameraShot { x, z, size: CLOSEUP_SIZE_M, yaw: 0.0, pitch: 45.0, info_view: InfoView::None }
}
```

Update `region_shot`'s returned struct literal to the new fields (close-up style):

```rust
    Some(CameraShot {
        x: (min_x + max_x) / 2.0,
        z: (min_z + max_z) / 2.0,
        size: ((max_x - min_x).max(max_z - min_z) * CLOSEUP_MARGIN / 2.0).max(CLOSEUP_SIZE_M),
        yaw: 0.0,
        pitch: 45.0,
        info_view: InfoView::None,
    })
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd broker && cargo test -p skylinebench camera_shots_carry`
Expected: PASS. (The crate won't fully compile yet — `bridge_client`/`screenshots` still reference `top_down`; that's Task 2. Run the single test with `--lib` may still fail to compile. If so, do Steps 3 of Task 2 in the same commit before running tests.)

- [ ] **Step 5: Commit**

```bash
git add broker/src/service.rs
git commit -m "feat(broker): add yaw/pitch/info_view to CameraShot"
```

### Task 2: Thread new camera fields through `bridge_client` + `screenshots` (Rust)

**Files:**
- Modify: `broker/src/bridge_client.rs:192-210`
- Modify: `broker/src/benchmark/screenshots.rs:58`
- Modify: `broker/src/service.rs:470-475` (`capture_screenshot`)

- [ ] **Step 1: Update `bridge_client.screenshot`**

```rust
    pub async fn screenshot(
        &self,
        x: f32,
        z: f32,
        size: f32,
        yaw: f32,
        pitch: f32,
        info_view: &str,
    ) -> Result<Vec<u8>, BridgeError> {
        let body = serde_json::json!({
            "x": x, "z": z, "size": size,
            "yaw": yaw, "pitch": pitch, "info_view": info_view,
        });
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

- [ ] **Step 2: Update `screenshots.rs::grab` call site**

In `broker/src/benchmark/screenshots.rs`, the line inside `grab`:

```rust
        match client.screenshot(shot.x, shot.z, shot.size, shot.yaw, shot.pitch, shot.info_view.as_str()).await {
```

- [ ] **Step 3: Update `service.rs::capture_screenshot`**

```rust
pub async fn capture_screenshot(
    client: &BridgeClient,
    shot: CameraShot,
) -> Result<Vec<u8>, ServiceError> {
    Ok(client.screenshot(shot.x, shot.z, shot.size, shot.yaw, shot.pitch, shot.info_view.as_str()).await?)
}
```

- [ ] **Step 4: Build + run existing screenshot tests**

Run: `cd broker && cargo test -p skylinebench screenshots`
Expected: PASS (the `screenshots.rs` tests construct shots via `overview_shot`/`closeup_shot`, which now compile).

- [ ] **Step 5: Commit**

```bash
git add broker/src/bridge_client.rs broker/src/benchmark/screenshots.rs broker/src/service.rs
git commit -m "feat(broker): pass yaw/pitch/info_view to the screenshot bridge call"
```

### Task 3: Parse new screenshot fields in the mod (C#, TDD)

**Files:**
- Modify: `mod/src/json/RequestParse.cs:9,60-69`
- Test: `mod/test/RequestParseTests.cs:40-50`

- [ ] **Step 1: Update the `Screenshot` test**

Replace the `Screenshot()` test body in `mod/test/RequestParseTests.cs`:

```csharp
        static void Screenshot()
        {
            var r = RequestParse.Screenshot(JsonReader.Parse(
                "{\"x\":-120.5,\"z\":340,\"size\":500,\"yaw\":90,\"pitch\":32,\"info_view\":\"traffic\"}"));
            Assert.Equal(-120.5, r.X); Assert.Equal(340.0, r.Z); Assert.Equal(500.0, r.Size);
            Assert.Equal(90.0, r.Yaw); Assert.Equal(32.0, r.Pitch);
            Assert.Equal("traffic", r.InfoView);

            var d = RequestParse.Screenshot(JsonReader.Parse("{\"x\":0,\"z\":0}"));
            Assert.Equal(1000.0, d.Size);
            Assert.Equal(90.0, d.Pitch, "pitch defaults to straight-down");
            Assert.Equal(0.0, d.Yaw);
            Assert.Equal("none", d.InfoView);
        }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd mod/test && dotnet run` (or the project's TestRunner per `mod/test/Tests.csproj`).
Expected: FAIL — `ScreenshotReq` has no `Yaw`/`Pitch`/`InfoView`.

- [ ] **Step 3: Implement**

In `mod/src/json/RequestParse.cs`, replace the `ScreenshotReq` struct (line 9):

```csharp
    public struct ScreenshotReq { public float X, Z, Size, Yaw, Pitch; public string InfoView; }
```

Replace the `Screenshot` parser (lines 60-69):

```csharp
        public static ScreenshotReq Screenshot(JsonValue v)
        {
            return new ScreenshotReq
            {
                X = (float)v["x"].AsDouble(),
                Z = (float)v["z"].AsDouble(),
                Size = v["size"].IsNull ? 1000f : (float)v["size"].AsDouble(),
                Yaw = v["yaw"].IsNull ? 0f : (float)v["yaw"].AsDouble(),
                Pitch = v["pitch"].IsNull ? 90f : (float)v["pitch"].AsDouble(),
                InfoView = v["info_view"].IsNull ? "none" : v["info_view"].AsString(),
            };
        }
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd mod/test && dotnet run`
Expected: PASS for `parse: screenshot`.

- [ ] **Step 5: Commit**

```bash
git add mod/src/json/RequestParse.cs mod/test/RequestParseTests.cs
git commit -m "feat(mod): parse yaw/pitch/info_view on the screenshot request"
```

### Task 4: Apply yaw/pitch + traffic info-view in the capture coroutine (C#)

No pure-logic test (needs Unity). Verified by building the mod and a real run.

**Files:**
- Modify: `mod/src/bridge/Capture.cs:9-16,40-48,97-152`
- Modify: `mod/src/http/Handlers.cs:59-73`

- [ ] **Step 1: Update `CaptureRequest` fields**

In `mod/src/bridge/Capture.cs`, replace the `CaptureRequest` class fields (lines 11-12):

```csharp
        public float X, Z, Size, Yaw, Pitch;
        public string InfoView;
```

- [ ] **Step 2: Update the `Capture` entry point (lines 40-48)**

```csharp
        public static byte[] Capture(float x, float z, float size, float yaw, float pitch, string infoView, int timeoutMs)
        {
            var req = new CaptureRequest { X = x, Z = z, Size = size, Yaw = yaw, Pitch = pitch, InfoView = infoView };
            lock (_lock) { _queue.Enqueue(req); }
            if (!req.Done.WaitOne(timeoutMs))
                throw new TimeoutException("screenshot capture timed out after " + timeoutMs + "ms");
            if (req.Error != null) throw req.Error;
            return req.Png;
        }
```

- [ ] **Step 3: Use yaw/pitch + toggle the traffic info-view in `Run` (lines 111-151)**

Replace the camera-setup block and the capture block so the angle uses the request's yaw/pitch and the traffic info-view is toggled around the read. Full replacement of the `try { ... }` camera block through the capture `try/finally`:

```csharp
            CameraController cc = null;
            bool prevFree = false;
            ColossalFramework.UI.InfoManager im = null;
            InfoManager.InfoMode prevMode = InfoManager.InfoMode.None;
            InfoManager.SubInfoMode prevSub = InfoManager.SubInfoMode.Default;
            bool trafficOn = string.Equals(req.InfoView, "traffic", StringComparison.OrdinalIgnoreCase);
            try
            {
                cc = ToolsModifierControl.cameraController;
                prevFree = cc.m_freeCamera;
                cc.m_freeCamera = true;
                var pos = new Vector3(req.X, 0f, req.Z);
                var angle = new Vector2(req.Yaw, req.Pitch);
                cc.m_targetPosition = pos; cc.m_currentPosition = pos;
                cc.m_targetSize = req.Size; cc.m_currentSize = req.Size;
                cc.m_targetAngle = angle; cc.m_currentAngle = angle;
                if (trafficOn)
                {
                    im = ColossalFramework.Singleton<InfoManager>.instance;
                    prevMode = im.CurrentMode; prevSub = im.CurrentSubMode;
                    im.SetCurrentMode(InfoManager.InfoMode.Traffic, InfoManager.SubInfoMode.Default);
                }
            }
            catch (Exception e) { req.Error = e; req.Done.Set(); yield break; }

            // End-of-frame waits so the moved camera renders; the longer wait when
            // the info view is on lets its colour fade settle.
            yield return new WaitForEndOfFrame();
            yield return new WaitForEndOfFrame();
            if (trafficOn) yield return new WaitForSecondsRealtime(0.5f);

            try
            {
                var tex = new Texture2D(Screen.width, Screen.height, TextureFormat.RGB24, false);
                try
                {
                    tex.ReadPixels(new Rect(0f, 0f, Screen.width, Screen.height), 0, 0);
                    tex.Apply();
                    req.Png = tex.EncodeToPNG();
                }
                finally
                {
                    UnityEngine.Object.Destroy(tex);
                }
            }
            catch (Exception e) { req.Error = e; }
            finally
            {
                if (im != null) try { im.SetCurrentMode(prevMode, prevSub); } catch { }
                if (cc != null) cc.m_freeCamera = prevFree;
                req.Done.Set();
            }
```

(Note: `InfoManager` lives in the game assembly's global namespace; the existing file already references `ColossalFramework.UI.UIView` and `ColossalFramework.Singleton`. If `InfoManager` resolves without the `ColossalFramework.UI.` prefix, drop the prefix on the field type — adjust at build time.)

- [ ] **Step 4: Update `Handlers.Screenshot` (lines 59-73)**

```csharp
        public static HttpReply Screenshot(string body)
        {
            var req = RequestParse.Screenshot(JsonReader.Parse(body));
            try
            {
                byte[] png = CaptureBehaviour.Capture(req.X, req.Z, req.Size, req.Yaw, req.Pitch, req.InfoView, 5000);
                return HttpReply.Png(png);
            }
            catch (Exception e)
            {
                var w = new JsonWriter();
                w.BeginObject().Name("error").Value("capture_failed").Name("message").Value(e.Message).EndObject();
                return HttpReply.Json(500, w.ToString());
            }
        }
```

- [ ] **Step 5: Build the mod**

Run: `cd mod && ./build.sh`
Expected: Release build succeeds. (Resolve the `InfoManager` namespace prefix here if the compiler complains.)

- [ ] **Step 6: Commit**

```bash
git add mod/src/bridge/Capture.cs mod/src/http/Handlers.cs
git commit -m "feat(mod): drive screenshot camera by yaw/pitch and toggle traffic info-view"
```

---

## Group 2 — Feature A+B: rotated, tighter overview with traffic layer

### Task 5: Frame-filling rotated overview + traffic info-view (Rust, TDD)

**Files:**
- Modify: `broker/src/service.rs:411-413,434-447`
- Test: `broker/src/service.rs` inline tests

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn overview_rotates_long_axis_into_the_wide_frame_with_traffic() {
    use crate::contract::{NetNode, Network};
    // City much wider in x (2000m) than tall in z (200m): the long axis should
    // map across the wide frame, which means yaw 90.
    let net = Network {
        nodes: vec![
            NetNode { id: 0, x: -1000.0, y: 0.0, z: -100.0 },
            NetNode { id: 1, x: 1000.0, y: 0.0, z: 100.0 },
        ],
        segments: vec![],
    };
    let ov = overview_shot(&net);
    assert_eq!(ov.yaw, 90.0, "wider-than-tall city rotates so x runs across the frame");
    assert_eq!(ov.pitch, 90.0, "overview stays top-down");
    assert!(matches!(ov.info_view, InfoView::Traffic), "overview carries the traffic layer");
}

#[test]
fn overview_keeps_north_up_when_taller_than_wide() {
    use crate::contract::{NetNode, Network};
    let net = Network {
        nodes: vec![
            NetNode { id: 0, x: -100.0, y: 0.0, z: -1000.0 },
            NetNode { id: 1, x: 100.0, y: 0.0, z: 1000.0 },
        ],
        segments: vec![],
    };
    assert_eq!(overview_shot(&net).yaw, 0.0);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd broker && cargo test -p skylinebench overview_`
Expected: FAIL (yaw is currently always 0.0, info_view None).

- [ ] **Step 3: Implement**

Replace the overview constants (lines 411-413):

```rust
/// Floor for the overview zoom so tiny networks aren't framed from 10 m up.
const OVERVIEW_MIN_SIZE_M: f32 = 600.0;
const OVERVIEW_MARGIN: f32 = 1.08;
/// Screen aspect (≈16:9 at the 720p the game runs). The camera `size` is the
/// vertical half-extent in metres; the horizontal half-extent is `size * aspect`.
const OVERVIEW_ASPECT: f32 = 16.0 / 9.0;
```

Replace `overview_shot` (lines 434-447):

```rust
pub fn overview_shot(net: &crate::contract::Network) -> CameraShot {
    let bounds = trimmed_bounds(net.nodes.iter().map(|n| n.x))
        .zip(trimmed_bounds(net.nodes.iter().map(|n| n.z)));
    match bounds {
        None => CameraShot { x: 0.0, z: 0.0, size: 2000.0, yaw: 0.0, pitch: 90.0, info_view: InfoView::Traffic },
        Some(((min_x, max_x), (min_z, max_z))) => {
            let dx = max_x - min_x;
            let dz = max_z - min_z;
            // size needed for a (vertical, horizontal) world extent in the frame.
            let size_for = |vertical: f32, horizontal: f32| {
                (vertical.max(horizontal / OVERVIEW_ASPECT) * OVERVIEW_MARGIN / 2.0).max(OVERVIEW_MIN_SIZE_M)
            };
            // yaw 0 (north-up): vertical = z, horizontal = x.
            let north = size_for(dz, dx);
            // yaw 90: the axes swap, so vertical = x, horizontal = z.
            let rotated = size_for(dx, dz);
            let (yaw, size) = if rotated < north { (90.0, rotated) } else { (0.0, north) };
            CameraShot {
                x: (min_x + max_x) / 2.0,
                z: (min_z + max_z) / 2.0,
                size,
                yaw,
                pitch: 90.0,
                info_view: InfoView::Traffic,
            }
        }
    }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cd broker && cargo test -p skylinebench overview_`
Expected: PASS both tests. Also run `cargo test -p skylinebench` to confirm nothing else regressed.

- [ ] **Step 5: Commit**

```bash
git add broker/src/service.rs
git commit -m "feat(broker): rotate+tighten overview and enable the traffic layer"
```

---

## Group 3 — Feature C: per-op before/after inside plans

### Task 6: Capture a before/after pair per logical op in `apply_plan` (Rust, TDD)

**Files:**
- Modify: `broker/src/benchmark/server.rs:749-846`
- Test: `broker/src/benchmark/server.rs:1280-1312` (replace `apply_plan_persists_one_before_after_pair_framing_all_ops`)

- [ ] **Step 1: Rewrite the existing capture test to expect per-op pairs**

Replace the test `apply_plan_persists_one_before_after_pair_framing_all_ops` (around line 1280) with:

```rust
    #[tokio::test]
    async fn apply_plan_persists_a_before_after_pair_per_op() {
        let (server, dir) = bench_server_with_screenshots().await;
        server
            .apply_plan(Parameters(ApplyPlanArgs {
                ops: vec![plan_build(0.0, 50.0), plan_build(1000.0, 1050.0)],
                validate_only: false,
                stop_on_error: true,
            }))
            .await
            .unwrap();

        let actions = std::fs::read_to_string(dir.join("actions/index.jsonl")).unwrap();
        let entries: Vec<serde_json::Value> =
            actions.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        // 2 ops × (before + after) = 4 frames.
        assert_eq!(entries.len(), 4, "two ops produce two before/after pairs");
        assert_eq!(entries[0]["caption"], "apply_plan op 1/2: build_road (before)");
        assert_eq!(entries[1]["caption"], "apply_plan op 1/2: build_road (after)");
        assert_eq!(entries[2]["caption"], "apply_plan op 2/2: build_road (before)");
        assert_eq!(entries[3]["caption"], "apply_plan op 2/2: build_road (after)");
        std::fs::remove_dir_all(&dir).ok();
    }
```

(If a `bench_server_with_screenshots()` helper does not already exist in the test module, reuse whatever helper the existing test used to obtain `server` + screenshots `dir`; keep its setup identical and only change the assertions.)

- [ ] **Step 2: Run to verify failure**

Run: `cd broker && cargo test -p skylinebench apply_plan_persists_a_before_after_pair_per_op`
Expected: FAIL — currently only one combined pair (2 frames) is written.

- [ ] **Step 3: Implement per-op grouping + capture**

In `apply_plan`, after `ctx`/`exec` are built and `seg_midpoint` is defined, replace the combined-shot block (lines 756-778) with per-source-op shots. Add this helper right after `seg_midpoint`:

```rust
        // Map an exec op to its edit location, reusing seg_midpoint.
        let op_position = |op: &ExecOp| -> Option<(f32, f32)> {
            match op {
                ExecOp::Build { from, to, .. } => Some(((from.x + to.x) / 2.0, (from.z + to.z) / 2.0)),
                ExecOp::Upgrade { segment, .. } => seg_midpoint(*segment),
                ExecOp::Bulldoze { target_type, id } => match target_type.as_str() {
                    "segment" => seg_midpoint(*id),
                    "node" => net.nodes.iter().find(|nd| nd.id == *id).map(|nd| (nd.x, nd.z)),
                    "building" => buildings.iter().find(|bd| bd.id == *id).map(|bd| (bd.x, bd.z)),
                    _ => None,
                },
                ExecOp::Zone { area, .. } => {
                    Some(((area.min_x + area.max_x) / 2.0, (area.min_z + area.max_z) / 2.0))
                }
                ExecOp::Invalid => None,
            }
        };

        // Group expanded ops back to their source op; one shot framing each
        // source op's full span (chunks of one logical road stay together).
        let source_indices: Vec<usize> = {
            let mut seen: Vec<usize> = Vec::new();
            for (source, _) in &exec {
                if !seen.contains(source) {
                    seen.push(*source);
                }
            }
            seen
        };
        let n_ops = source_indices.len();
        // shot + before-frame for each source op, captured before any op runs.
        let mut op_shots: std::collections::HashMap<usize, crate::service::CameraShot> =
            std::collections::HashMap::new();
        let mut op_before: std::collections::HashMap<usize, Option<Vec<u8>>> =
            std::collections::HashMap::new();
        for &src in &source_indices {
            let positions: Vec<(f32, f32)> =
                exec.iter().filter(|(s, _)| *s == src).filter_map(|(_, op)| op_position(op)).collect();
            let shot = crate::service::region_shot(&positions);
            op_before.insert(src, self.grab_before(shot).await);
            if let Some(shot) = shot {
                op_shots.insert(src, shot);
            }
        }
```

Then track which source ops succeeded. In the execution loop, where `n_all_ok += 1;` is incremented on success (around line 810), also record the source index. Add before the loop:

```rust
        let mut ok_sources: std::collections::HashSet<usize> = std::collections::HashSet::new();
```

and inside the `if ok {` arm (right after `n_all_ok += 1;`):

```rust
                        ok_sources.insert(*source);
```

Finally replace the post-loop capture block (lines 843-846) with per-op pairs:

```rust
        if n_all_ok > 0 {
            self.refresh_topology().await;
            for (k, &src) in source_indices.iter().enumerate() {
                if !ok_sources.contains(&src) {
                    continue;
                }
                let tool = validations.iter().find(|v| v.0 == src).map(|v| tool_name(v.1)).unwrap_or("apply_plan");
                let caption = format!("apply_plan op {}/{}: {tool}", k + 1, n_ops);
                self.shoot_action_pair(
                    op_shots.get(&src).copied(),
                    op_before.get(&src).and_then(|b| b.clone()),
                    "apply_plan",
                    caption,
                )
                .await;
            }
        }
```

Remove the now-unused `planned_positions`/`shot`/`before` bindings (old lines 756-778) and the old `shoot_action_pair(shot, before, ...)` call.

- [ ] **Step 4: Run to verify pass**

Run: `cd broker && cargo test -p skylinebench apply_plan`
Expected: PASS, including the rewritten test. Fix any other `apply_plan` test that asserted the old single-pair caption.

- [ ] **Step 5: Commit**

```bash
git add broker/src/benchmark/server.rs
git commit -m "feat(broker): capture before/after per logical op in apply_plan"
```

---

## Group 4 — Feature D: recorded begin/end highway flyby

### Task 7: `CameraKeyframe` + `highway_flyby_path` (Rust, TDD)

**Files:**
- Modify: `broker/src/service.rs` (add near `CameraShot`, after `region_shot`)
- Test: `broker/src/service.rs` inline tests

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn highway_flyby_path_orders_ns_by_z_and_we_by_x() {
    use crate::contract::{NetNode, NetSegment, Network};
    let node = |id: u32, x: f32, z: f32| NetNode { id, x, y: 0.0, z };
    let seg = |id: u32, a: u32, b: u32| NetSegment {
        id, start_node: a, end_node: b, prefab: "Highway".into(),
        lanes: 4, length: 100.0, one_way: false, travel_direction: "both".into(), speed_limit: 2.0,
    };
    let net = Network {
        nodes: vec![node(0, 0.0, -500.0), node(1, 10.0, 0.0), node(2, -10.0, 500.0),
                    node(3, -500.0, 5.0), node(4, 500.0, -5.0)],
        segments: vec![seg(0, 0, 1), seg(1, 1, 2), seg(2, 3, 4)],
    };
    let path = highway_flyby_path(&net);
    assert!(!path.ns.is_empty() && !path.we.is_empty());
    // N/S keyframes ascend in z; W/E ascend in x.
    assert!(path.ns.windows(2).all(|w| w[0].z <= w[1].z), "ns ascends south->north");
    assert!(path.we.windows(2).all(|w| w[0].x <= w[1].x), "we ascends west->east");
    assert_eq!(path.ns[0].yaw, 0.0);
    assert_eq!(path.we[0].yaw, 90.0);
    assert_eq!(path.ns[0].pitch, FLYBY_PITCH_DEG);
    assert_eq!(path.ns[0].size, FLYBY_SIZE_M);
}

#[test]
fn highway_flyby_path_falls_back_to_all_segments_without_highways() {
    use crate::contract::{NetNode, NetSegment, Network};
    let net = Network {
        nodes: vec![NetNode { id: 0, x: 0.0, y: 0.0, z: 0.0 }, NetNode { id: 1, x: 100.0, y: 0.0, z: 100.0 }],
        segments: vec![NetSegment {
            id: 0, start_node: 0, end_node: 1, prefab: "Basic Road".into(),
            lanes: 2, length: 100.0, one_way: false, travel_direction: "both".into(), speed_limit: 1.0,
        }],
    };
    assert!(!highway_flyby_path(&net).ns.is_empty(), "falls back to all segments");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd broker && cargo test -p skylinebench highway_flyby_path`
Expected: FAIL — `CameraKeyframe`/`highway_flyby_path`/`FLYBY_*` not found.

- [ ] **Step 3: Implement**

Add to `service.rs` (after `region_shot`):

```rust
/// Flyby tuning (single source of truth — adjust after the first real run).
pub const FLYBY_KEYFRAMES_PER_PASS: usize = 8;
pub const FLYBY_SIZE_M: f32 = 500.0;
pub const FLYBY_PITCH_DEG: f32 = 32.0;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct CameraKeyframe {
    pub x: f32,
    pub z: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub size: f32,
}

#[derive(Debug, Clone, Default)]
pub struct FlybyPath {
    pub ns: Vec<CameraKeyframe>,
    pub we: Vec<CameraKeyframe>,
}

fn is_highway(prefab: &str) -> bool {
    prefab.to_lowercase().contains("highway")
}

/// Reduce a point cloud to a smoothed centerline of keyframes along one axis.
/// `along_z` true: bucket by z (south→north), median x per bucket, yaw 0.
/// false: bucket by x (west→east), median z per bucket, yaw 90.
fn flyby_pass(mut pts: Vec<(f32, f32)>, along_z: bool) -> Vec<CameraKeyframe> {
    if pts.is_empty() {
        return vec![];
    }
    let along = |p: &(f32, f32)| if along_z { p.1 } else { p.0 };
    let cross = |p: &(f32, f32)| if along_z { p.0 } else { p.1 };
    pts.sort_by(|a, b| along(a).total_cmp(&along(b)));
    let n = pts.len();
    let bins = FLYBY_KEYFRAMES_PER_PASS.min(n).max(1);
    (0..bins)
        .map(|i| {
            let lo = i * n / bins;
            let hi = (((i + 1) * n / bins).max(lo + 1)).min(n);
            let slice = &pts[lo..hi];
            let along_mean = slice.iter().map(along).sum::<f32>() / slice.len() as f32;
            let mut cs: Vec<f32> = slice.iter().map(cross).collect();
            cs.sort_by(f32::total_cmp);
            let cross_med = cs[cs.len() / 2];
            let (x, z) = if along_z { (cross_med, along_mean) } else { (along_mean, cross_med) };
            CameraKeyframe {
                x,
                z,
                yaw: if along_z { 0.0 } else { 90.0 },
                pitch: FLYBY_PITCH_DEG,
                size: FLYBY_SIZE_M,
            }
        })
        .collect()
}

/// Build N/S and W/E flyby keyframe passes along the main highways. Falls back
/// to the whole network when the city has no highway segments.
pub fn highway_flyby_path(net: &crate::contract::Network) -> FlybyPath {
    let node_xz: std::collections::HashMap<u32, (f32, f32)> =
        net.nodes.iter().map(|n| (n.id, (n.x, n.z))).collect();
    let collect = |highway_only: bool| -> Vec<(f32, f32)> {
        net.segments
            .iter()
            .filter(|s| !highway_only || is_highway(&s.prefab))
            .flat_map(|s| [s.start_node, s.end_node])
            .filter_map(|id| node_xz.get(&id).copied())
            .collect()
    };
    let pts = {
        let hw = collect(true);
        if hw.is_empty() {
            collect(false)
        } else {
            hw
        }
    };
    FlybyPath {
        ns: flyby_pass(pts.clone(), true),
        we: flyby_pass(pts, false),
    }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cd broker && cargo test -p skylinebench highway_flyby_path`
Expected: PASS both tests.

- [ ] **Step 5: Commit**

```bash
git add broker/src/service.rs
git commit -m "feat(broker): build N/S and W/E highway flyby keyframe paths"
```

### Task 8: `bridge_client.flyby` + mock `/flyby` (Rust)

**Files:**
- Modify: `broker/src/bridge_client.rs` (add method after `screenshot`)
- Modify: `broker/src/mock.rs:444-480` (add `flyby` handler + route)

- [ ] **Step 1: Add `flyby` to `bridge_client.rs`**

```rust
    /// Drive a recorded flyby: the mod interpolates the camera along `keyframes`
    /// over `duration_s` while the sim runs, writing numbered PNG frames into
    /// `out_dir`. Blocks for the whole pass, so the timeout is generous.
    pub async fn flyby(
        &self,
        keyframes: &[crate::service::CameraKeyframe],
        duration_s: f32,
        capture_fps: u32,
        out_dir: &str,
    ) -> Result<(), BridgeError> {
        let body = serde_json::json!({
            "keyframes": keyframes,
            "duration_s": duration_s,
            "capture_fps": capture_fps,
            "out_dir": out_dir,
        });
        self.http
            .post(format!("{}/flyby", self.base))
            .json(&body)
            .timeout(std::time::Duration::from_secs(duration_s as u64 + 30))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
```

- [ ] **Step 2: Add a mock `/flyby` that writes stub frames**

In `broker/src/mock.rs`, add a handler before `pub fn router()`:

```rust
async fn flyby(Json(body): Json<serde_json::Value>) -> impl axum::response::IntoResponse {
    // Write a couple of 1x1 PNG stub frames so broker-side assembly is testable.
    if let Some(dir) = body.get("out_dir").and_then(|v| v.as_str()) {
        let _ = std::fs::create_dir_all(dir);
        let opts = crate::render::RenderOptions {
            bounds: crate::geometry::playable_bounds(),
            width_px: 64,
            height_px: 64,
            grid_spacing_m: 0.0,
        };
        let png = crate::render::render_network(
            &Network { nodes: vec![], segments: vec![] },
            &std::collections::HashMap::new(),
            &opts,
        );
        for i in 1..=2 {
            let _ = std::fs::write(std::path::Path::new(dir).join(format!("{i:05}.png")), &png);
        }
    }
    axum::http::StatusCode::OK
}
```

Add the route inside `router()`:

```rust
        .route("/flyby", post(flyby))
```

- [ ] **Step 3: Build**

Run: `cd broker && cargo build`
Expected: compiles.

- [ ] **Step 4: Commit**

```bash
git add broker/src/bridge_client.rs broker/src/mock.rs
git commit -m "feat(broker): add flyby bridge call + mock endpoint"
```

### Task 9: Parse `/flyby` request in the mod (C#, TDD)

**Files:**
- Modify: `mod/src/json/RequestParse.cs` (add `FlybyReq` + `Keyframe` + parser)
- Test: `mod/test/RequestParseTests.cs` (add `Flyby` test + register it)

- [ ] **Step 1: Add the test + registration**

In `mod/test/RequestParseTests.cs`, add to `Register`:

```csharp
            tests.Add(new KeyValuePair<string, Action>("parse: flyby", Flyby));
```

and the test method:

```csharp
        static void Flyby()
        {
            var r = RequestParse.Flyby(JsonReader.Parse(
                "{\"keyframes\":[{\"x\":1,\"z\":2,\"yaw\":0,\"pitch\":32,\"size\":500},{\"x\":3,\"z\":4,\"yaw\":0,\"pitch\":32,\"size\":500}],\"duration_s\":6,\"capture_fps\":12,\"out_dir\":\"/tmp/fly\"}"));
            Assert.True(r.Keyframes.Length == 2, "two keyframes");
            Assert.Equal(1.0, r.Keyframes[0].X); Assert.Equal(4.0, r.Keyframes[1].Z);
            Assert.Equal(6.0, r.DurationS); Assert.True(r.CaptureFps == 12, "fps");
            Assert.Equal("/tmp/fly", r.OutDir);
        }
```

- [ ] **Step 2: Run to verify failure**

Run: `cd mod/test && dotnet run`
Expected: FAIL — `RequestParse.Flyby`/`FlybyReq` missing.

- [ ] **Step 3: Implement**

In `mod/src/json/RequestParse.cs`, add the structs (next to the other `*Req` structs):

```csharp
    public struct KeyframeReq { public float X, Z, Yaw, Pitch, Size; }
    public struct FlybyReq { public KeyframeReq[] Keyframes; public float DurationS; public int CaptureFps; public string OutDir; }
```

and the parser (inside `RequestParse`):

```csharp
        public static FlybyReq Flyby(JsonValue v)
        {
            var arr = v["keyframes"];
            int n = arr.Count;
            var kfs = new KeyframeReq[n];
            for (int i = 0; i < n; i++)
            {
                var k = arr[i];
                kfs[i] = new KeyframeReq
                {
                    X = (float)k["x"].AsDouble(),
                    Z = (float)k["z"].AsDouble(),
                    Yaw = (float)k["yaw"].AsDouble(),
                    Pitch = (float)k["pitch"].AsDouble(),
                    Size = (float)k["size"].AsDouble(),
                };
            }
            return new FlybyReq
            {
                Keyframes = kfs,
                DurationS = (float)v["duration_s"].AsDouble(),
                CaptureFps = (int)v["capture_fps"].AsDouble(),
                OutDir = v["out_dir"].AsString(),
            };
        }
```

(If `JsonValue` exposes array length differently than `.Count` / indexer `[i]`, match the accessor used elsewhere in `JsonReader`/`Serialize` — check `mod/src/json/JsonReader.cs` and adapt.)

- [ ] **Step 4: Run to verify pass**

Run: `cd mod/test && dotnet run`
Expected: PASS `parse: flyby`.

- [ ] **Step 5: Commit**

```bash
git add mod/src/json/RequestParse.cs mod/test/RequestParseTests.cs
git commit -m "feat(mod): parse the flyby keyframe request"
```

### Task 10: Catmull-Rom helper (C#, TDD)

**Files:**
- Create: `mod/src/bridge/FlybyMath.cs`
- Create: `mod/test/FlybyMathTests.cs`
- Modify: `mod/test/TestRunner.cs` (register the new test class)

- [ ] **Step 1: Write the test**

Create `mod/test/FlybyMathTests.cs`:

```csharp
using System;
using System.Collections.Generic;
using SkylineBench.Bridge;
using UnityEngine;

namespace SkylineBench.Tests
{
    public static class FlybyMathTests
    {
        public static void Register(List<KeyValuePair<string, Action>> tests)
        {
            tests.Add(new KeyValuePair<string, Action>("flyby: catmull endpoints", Endpoints));
            tests.Add(new KeyValuePair<string, Action>("flyby: catmull midpoint", Midpoint));
        }

        static void Endpoints()
        {
            var pts = new Vector2[] { new Vector2(0, 0), new Vector2(10, 0), new Vector2(20, 0) };
            var a = FlybyMath.Sample(pts, 0f);
            var b = FlybyMath.Sample(pts, 1f);
            Assert.True(Mathf.Abs(a.x - 0f) < 0.001f, "u=0 is the first point");
            Assert.True(Mathf.Abs(b.x - 20f) < 0.001f, "u=1 is the last point");
        }

        static void Midpoint()
        {
            var pts = new Vector2[] { new Vector2(0, 0), new Vector2(10, 0), new Vector2(20, 0) };
            var m = FlybyMath.Sample(pts, 0.5f);
            Assert.True(Mathf.Abs(m.x - 10f) < 0.001f, "u=0.5 is the middle control point");
        }
    }
}
```

- [ ] **Step 2: Register in the runner**

In `mod/test/TestRunner.cs`, alongside the other `*.Register(tests)` calls, add:

```csharp
            FlybyMathTests.Register(tests);
```

- [ ] **Step 3: Run to verify failure**

Run: `cd mod/test && dotnet run`
Expected: FAIL — `FlybyMath` does not exist.

- [ ] **Step 4: Implement**

Create `mod/src/bridge/FlybyMath.cs`:

```csharp
using UnityEngine;

namespace SkylineBench.Bridge
{
    /// <summary>Catmull-Rom sampling across a list of control points. `u` runs
    /// 0..1 over the whole path; endpoints are clamped (duplicated) so the curve
    /// passes through the first and last control points.</summary>
    public static class FlybyMath
    {
        public static Vector2 Sample(Vector2[] pts, float u)
        {
            if (pts.Length == 0) return Vector2.zero;
            if (pts.Length == 1) return pts[0];
            u = Mathf.Clamp01(u);
            int segments = pts.Length - 1;
            float scaled = u * segments;
            int i = Mathf.Min((int)scaled, segments - 1);
            float t = scaled - i;
            Vector2 p0 = pts[Mathf.Max(i - 1, 0)];
            Vector2 p1 = pts[i];
            Vector2 p2 = pts[i + 1];
            Vector2 p3 = pts[Mathf.Min(i + 2, pts.Length - 1)];
            return CatmullRom(p0, p1, p2, p3, t);
        }

        static Vector2 CatmullRom(Vector2 p0, Vector2 p1, Vector2 p2, Vector2 p3, float t)
        {
            float t2 = t * t;
            float t3 = t2 * t;
            return 0.5f * (
                (2f * p1) +
                (-p0 + p2) * t +
                (2f * p0 - 5f * p1 + 4f * p2 - p3) * t2 +
                (-p0 + 3f * p1 - 3f * p2 + p3) * t3);
        }
    }
}
```

- [ ] **Step 5: Run to verify pass**

Run: `cd mod/test && dotnet run`
Expected: PASS both flyby math tests. (If `UnityEngine.Vector2` is unavailable to the test project, the test project already references the managed Unity DLLs via `Tests.csproj`; confirm `mod/test/Tests.csproj` includes the same `ManagedDLLPath` references as the mod build. If not, add the reference.)

- [ ] **Step 6: Commit**

```bash
git add mod/src/bridge/FlybyMath.cs mod/test/FlybyMathTests.cs mod/test/TestRunner.cs
git commit -m "feat(mod): Catmull-Rom flyby path sampling"
```

### Task 11: Flyby recording coroutine + route (C#)

No pure-logic test (needs Unity + a running city). Verified by build + real run.

**Files:**
- Modify: `mod/src/bridge/Capture.cs` (add flyby queue + `Flyby` + `RunFlyby`)
- Modify: `mod/src/http/Handlers.cs` (add `Flyby` handler)
- Modify: `mod/src/http/Router.cs:32` (add `/flyby` route)

- [ ] **Step 1: Add the flyby request type + queue to `CaptureBehaviour`**

In `mod/src/bridge/Capture.cs`, add a request class near `CaptureRequest`:

```csharp
    public sealed class FlybyRequest
    {
        public RequestParse.KeyframeReq[] Keyframes;
        public float DurationS;
        public int CaptureFps;
        public string OutDir;
        public Exception Error;
        public readonly ManualResetEvent Done = new ManualResetEvent(false);
    }
```

Add a queue field next to `_queue`:

```csharp
        private static readonly Queue<FlybyRequest> _flybys = new Queue<FlybyRequest>();
```

Add the blocking entry point next to `Capture`:

```csharp
        public static void Flyby(FlybyRequest req, int timeoutMs)
        {
            lock (_lock) { _flybys.Enqueue(req); }
            if (!req.Done.WaitOne(timeoutMs))
                throw new TimeoutException("flyby timed out after " + timeoutMs + "ms");
            if (req.Error != null) throw req.Error;
        }
```

In `CancelAll`, also drain `_flybys`:

```csharp
                while (_flybys.Count > 0)
                {
                    var fb = _flybys.Dequeue();
                    fb.Error = reason;
                    fb.Done.Set();
                }
```

In `Update`, after the existing capture dequeue, start a flyby coroutine when one is queued:

```csharp
            FlybyRequest fly = null;
            lock (_lock) { if (_flybys.Count > 0) fly = _flybys.Dequeue(); }
            if (fly != null) StartCoroutine(RunFlyby(fly));
```

- [ ] **Step 2: Implement the recording coroutine**

Add to `CaptureBehaviour`:

```csharp
        private IEnumerator RunFlyby(FlybyRequest req)
        {
            var xs = new Vector2[req.Keyframes.Length];
            for (int i = 0; i < req.Keyframes.Length; i++)
                xs[i] = new Vector2(req.Keyframes[i].X, req.Keyframes[i].Z);
            int total = Mathf.Max(2, Mathf.RoundToInt(req.DurationS * req.CaptureFps));
            float interval = 1f / Mathf.Max(1, req.CaptureFps);

            var t = ModRuntime.Threading;
            CameraController cc = null;
            bool prevFree = false;
            bool prevPaused = t != null && t.simulationPaused;
            int prevSpeed = t != null ? t.simulationSpeed : 1;
            try
            {
                cc = ToolsModifierControl.cameraController;
                prevFree = cc.m_freeCamera;
                cc.m_freeCamera = true;
                if (t != null) { t.simulationPaused = false; t.simulationSpeed = 1; }
                try { System.IO.Directory.CreateDirectory(req.OutDir); } catch { }
            }
            catch (Exception e) { req.Error = e; req.Done.Set(); yield break; }

            int frame = 0;
            for (int i = 0; i < total; i++)
            {
                float u = (float)i / (total - 1);
                Vector2 pos2 = FlybyMath.Sample(xs, u);
                // yaw/pitch/size lerp linearly across the keyframes.
                float fk = u * (req.Keyframes.Length - 1);
                int k = Mathf.Min((int)fk, req.Keyframes.Length - 2);
                float kt = fk - k;
                var a = req.Keyframes[k];
                var b = req.Keyframes[Mathf.Min(k + 1, req.Keyframes.Length - 1)];
                float yaw = Mathf.Lerp(a.Yaw, b.Yaw, kt);
                float pitch = Mathf.Lerp(a.Pitch, b.Pitch, kt);
                float size = Mathf.Lerp(a.Size, b.Size, kt);

                Exception err = null;
                try
                {
                    var p = new Vector3(pos2.x, 0f, pos2.y);
                    cc.m_targetPosition = p; cc.m_currentPosition = p;
                    cc.m_targetSize = size; cc.m_currentSize = size;
                    cc.m_targetAngle = new Vector2(yaw, pitch); cc.m_currentAngle = new Vector2(yaw, pitch);
                }
                catch (Exception e) { err = e; }
                if (err != null) { req.Error = err; break; }

                yield return new WaitForEndOfFrame();

                try
                {
                    var tex = new Texture2D(Screen.width, Screen.height, TextureFormat.RGB24, false);
                    try
                    {
                        tex.ReadPixels(new Rect(0f, 0f, Screen.width, Screen.height), 0, 0);
                        tex.Apply();
                        byte[] png = tex.EncodeToPNG();
                        frame++;
                        System.IO.File.WriteAllBytes(System.IO.Path.Combine(req.OutDir, frame.ToString("D5") + ".png"), png);
                    }
                    finally { UnityEngine.Object.Destroy(tex); }
                }
                catch (Exception e) { req.Error = e; break; }

                // Let real time (and the sim) advance so cars move between frames.
                yield return new WaitForSecondsRealtime(interval);
            }

            if (cc != null) cc.m_freeCamera = prevFree;
            if (t != null) { t.simulationPaused = prevPaused; t.simulationSpeed = prevSpeed; }
            req.Done.Set();
        }
```

- [ ] **Step 3: Add the HTTP handler + route**

In `mod/src/http/Handlers.cs`, add:

```csharp
        public static HttpReply Flyby(string body)
        {
            var req = RequestParse.Flyby(JsonReader.Parse(body));
            try
            {
                var fb = new FlybyRequest
                {
                    Keyframes = req.Keyframes,
                    DurationS = req.DurationS,
                    CaptureFps = req.CaptureFps,
                    OutDir = req.OutDir,
                };
                CaptureBehaviour.Flyby(fb, (int)(req.DurationS * 1000) + 30000);
                var w = new JsonWriter();
                w.BeginObject().Name("ok").Value(true).EndObject();
                return HttpReply.Json(200, w.ToString());
            }
            catch (Exception e)
            {
                var w = new JsonWriter();
                w.BeginObject().Name("error").Value("flyby_failed").Name("message").Value(e.Message).EndObject();
                return HttpReply.Json(500, w.ToString());
            }
        }
```

(`FlybyRequest` is the top-level class declared in Step 1, in `SkylineBench.Bridge`; `Handlers.cs` already has `using SkylineBench.Bridge;`, so reference it unqualified. `Flyby(...)` is a static method on `CaptureBehaviour`.)

In `mod/src/http/Router.cs`, add after the `/screenshot` case (line 32):

```csharp
                case "/flyby": return method == "POST" ? Handlers.Flyby(body) : MethodNotAllowed();
```

- [ ] **Step 4: Build the mod**

Run: `cd mod && ./build.sh`
Expected: Release build succeeds.

- [ ] **Step 5: Commit**

```bash
git add mod/src/bridge/Capture.cs mod/src/http/Handlers.cs mod/src/http/Router.cs
git commit -m "feat(mod): record flyby frames along interpolated keyframes"
```

### Task 12: Trigger the begin flyby in `ensure_baseline` (Rust)

**Files:**
- Modify: `broker/src/benchmark/screenshots.rs` (expose the sink dir)
- Modify: `broker/src/benchmark/server.rs:214-246` (`ensure_baseline`) + add a `run_flyby` helper

- [ ] **Step 1: Expose the sink directory**

In `broker/src/benchmark/screenshots.rs`, add to `impl ScreenshotSink`:

```rust
    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }
```

- [ ] **Step 2: Add a `run_flyby` helper on the server**

In `broker/src/benchmark/server.rs`, add a method near `shoot_overview`:

```rust
    /// Record a begin/end highway flyby into `<screenshots>/flyby/<label>_{ns,we}`.
    /// Best-effort: a failure logs and never affects the run.
    async fn run_flyby(&self, label: &str) {
        let Some(sink) = &self.screenshots else { return };
        if sink.disabled() {
            return;
        }
        let Ok(net) = self.client.network().await else { return };
        let path = crate::service::highway_flyby_path(&net);
        let base = sink.dir().join("flyby");
        for (suffix, kfs) in [("ns", &path.ns), ("we", &path.we)] {
            if kfs.is_empty() {
                continue;
            }
            let dir = base.join(format!("{label}_{suffix}"));
            let dir_str = dir.to_string_lossy().to_string();
            // 6s per pass at 12fps (tunable; matches the spec's open knobs).
            if let Err(e) = self.client.flyby(kfs, 6.0, 12, &dir_str).await {
                eprintln!("benchmark: flyby '{label}_{suffix}' failed ({e}); skipping");
                return;
            }
        }
    }
```

- [ ] **Step 3: Call it once at baseline**

In `ensure_baseline`, after `self.client.network()` observe block at the end (line 243-245), add:

```rust
        self.run_flyby("start").await;
```

(Place it after the baseline is set so it only runs on the first tool call — `ensure_baseline` already returns early once `baseline.is_some()`.)

- [ ] **Step 4: Build + test**

Run: `cd broker && cargo test -p skylinebench`
Expected: PASS (existing tests; the mock `/flyby` accepts the call and writes stub frames so no test hangs).

- [ ] **Step 5: Commit**

```bash
git add broker/src/benchmark/screenshots.rs broker/src/benchmark/server.rs
git commit -m "feat(broker): record the begin flyby at baseline"
```

### Task 13: Trigger the end flyby in `benchmark-finalize` (Rust)

**Files:**
- Modify: `broker/src/main.rs:221-238` (`BenchmarkFinalize` command)

- [ ] **Step 1: Record the end flyby before scoring**

In the `Command::BenchmarkFinalize` arm in `broker/src/main.rs`, after the `health` check and before `finalize(&client, end, &out).await?`, add:

```rust
            // Screenshots were moved to <out>/screenshots after the session;
            // record the end flyby there so the timelapse can append it.
            {
                use skylinebench::service::highway_flyby_path;
                let base = out.join("screenshots").join("flyby");
                if let Ok(net) = client.network().await {
                    let path = highway_flyby_path(&net);
                    for (suffix, kfs) in [("ns", &path.ns), ("we", &path.we)] {
                        if kfs.is_empty() {
                            continue;
                        }
                        let dir = base.join(format!("end_{suffix}"));
                        let dir_str = dir.to_string_lossy().to_string();
                        if let Err(e) = client.flyby(kfs, 6.0, 12, &dir_str).await {
                            eprintln!("benchmark-finalize: end flyby '{suffix}' failed ({e}); skipping");
                            break;
                        }
                    }
                }
            }
```

- [ ] **Step 2: Build**

Run: `cd broker && cargo build`
Expected: compiles.

- [ ] **Step 3: Commit**

```bash
git add broker/src/main.rs
git commit -m "feat(broker): record the end flyby during benchmark-finalize"
```

### Task 14: Assemble flyby clips + concat into the timelapse (Rust, TDD)

**Files:**
- Modify: `broker/src/timelapse.rs` (add flyby assembly + concat; wire into `assemble`)
- Test: `broker/src/timelapse.rs` inline tests

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` in `timelapse.rs`:

```rust
    #[test]
    fn flyby_pass_dirs_collects_passes_in_order() {
        let run = std::env::temp_dir().join(format!("sb-fly-{}", std::process::id()));
        let base = run.join("screenshots/flyby");
        for sub in ["start_ns", "start_we", "end_ns", "end_we"] {
            std::fs::create_dir_all(base.join(sub)).unwrap();
            std::fs::write(base.join(sub).join("00001.png"), b"x").unwrap();
        }
        let start = flyby_pass_dirs(&run, "start");
        let end = flyby_pass_dirs(&run, "end");
        assert_eq!(start, vec![base.join("start_ns"), base.join("start_we")]);
        assert_eq!(end, vec![base.join("end_ns"), base.join("end_we")]);
        // Missing passes are skipped, not errors:
        std::fs::remove_dir_all(base.join("start_we")).unwrap();
        assert_eq!(flyby_pass_dirs(&run, "start"), vec![base.join("start_ns")]);
        std::fs::remove_dir_all(&run).ok();
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cd broker && cargo test -p skylinebench flyby_pass_dirs`
Expected: FAIL — `flyby_pass_dirs` not found.

- [ ] **Step 3: Implement assembly + concat**

Add to `timelapse.rs`:

```rust
/// The flyby pass subdirs for `label` ("start"/"end") that exist and contain a
/// frame, in playback order (N/S then W/E).
pub fn flyby_pass_dirs(run_dir: &Path, label: &str) -> Vec<PathBuf> {
    let base = run_dir.join("screenshots").join("flyby");
    ["ns", "we"]
        .iter()
        .map(|s| base.join(format!("{label}_{s}")))
        .filter(|d| d.join("00001.png").exists())
        .collect()
}

/// Encode a directory of NNNNN.png frames into `out` at `fps`.
fn encode_png_dir(dir: &Path, fps: u32, out: &Path) -> Result<(), anyhow::Error> {
    let status = std::process::Command::new("ffmpeg")
        .args(["-y", "-framerate", &fps.to_string(), "-i"])
        .arg(dir.join("%05d.png"))
        .args(["-pix_fmt", "yuv420p"])
        .arg(out)
        .status()
        .map_err(|e| anyhow::anyhow!("could not run ffmpeg ({e}) — install it with `brew install ffmpeg`"))?;
    anyhow::ensure!(status.success(), "ffmpeg exited with {status}");
    Ok(())
}

/// Concatenate mp4 `parts` into `out` via the ffmpeg concat demuxer.
fn concat_mp4(parts: &[PathBuf], out: &Path) -> Result<(), anyhow::Error> {
    let staging = out.with_extension("concat.txt");
    let list = parts
        .iter()
        .map(|p| format!("file '{}'", p.display()))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&staging, list)?;
    let status = std::process::Command::new("ffmpeg")
        .args(["-y", "-f", "concat", "-safe", "0", "-i"])
        .arg(&staging)
        .args(["-c", "copy"])
        .arg(out)
        .status()
        .map_err(|e| anyhow::anyhow!("could not run ffmpeg ({e})"))?;
    std::fs::remove_file(&staging).ok();
    anyhow::ensure!(status.success(), "ffmpeg concat exited with {status}");
    Ok(())
}

/// Build flyby_<label>.mp4 from its passes (24fps playback). Returns the path if
/// any pass existed, else None.
fn assemble_flyby(run_dir: &Path, label: &str) -> Result<Option<PathBuf>, anyhow::Error> {
    let dirs = flyby_pass_dirs(run_dir, label);
    if dirs.is_empty() {
        return Ok(None);
    }
    let pass_mp4s: Vec<PathBuf> = dirs
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let p = run_dir.join(format!("flyby-{label}-{i}.mp4"));
            encode_png_dir(d, 24, &p).map(|()| p)
        })
        .collect::<Result<_, _>>()?;
    let out = run_dir.join(format!("flyby_{label}.mp4"));
    if pass_mp4s.len() == 1 {
        std::fs::copy(&pass_mp4s[0], &out)?;
    } else {
        concat_mp4(&pass_mp4s, &out)?;
    }
    for p in &pass_mp4s {
        std::fs::remove_file(p).ok();
    }
    Ok(Some(out))
}
```

Then wire it into `assemble`. After the existing ffmpeg call that writes the core `out` mp4 (line 165), but before `eprintln!("timelapse: wrote ...")`, rename the core output and stitch:

Replace the tail of `assemble` (from the core ffmpeg invocation through the final `Ok(())`) with:

```rust
    let core = run_dir.join("timelapse-core.mp4");
    let status = std::process::Command::new("ffmpeg")
        .args(["-y", "-framerate", &fps.to_string(), "-i"])
        .arg(staging.join("%06d.png"))
        .args(["-pix_fmt", "yuv420p"])
        .arg(&core)
        .status()
        .map_err(|e| anyhow::anyhow!("could not run ffmpeg ({e}) — install it with `brew install ffmpeg`"))?;
    anyhow::ensure!(status.success(), "ffmpeg exited with {status}");
    std::fs::remove_dir_all(&staging).ok();

    let start = assemble_flyby(run_dir, "start")?;
    let end = assemble_flyby(run_dir, "end")?;
    let parts: Vec<PathBuf> = start.into_iter().chain([core.clone()]).chain(end).collect();
    if parts.len() == 1 {
        std::fs::rename(&core, out)?;
    } else {
        concat_mp4(&parts, out)?;
        std::fs::remove_file(&core).ok();
    }
    eprintln!("timelapse: wrote {}", out.display());
    Ok(())
```

(Note: concat with `-c copy` requires the parts share codec/params. Since every part is produced by this module with the same `-pix_fmt yuv420p` default H.264 settings, that holds. If a real run shows concat rejecting mismatched params, switch `concat_mp4` to re-encode: drop `-c copy` and add `-pix_fmt yuv420p`. Flagged as the spec's known assembly risk.)

- [ ] **Step 4: Run to verify pass**

Run: `cd broker && cargo test -p skylinebench flyby_pass_dirs`
Expected: PASS. Then `cargo test -p skylinebench timelapse` and `cargo build`.

- [ ] **Step 5: Commit**

```bash
git add broker/src/timelapse.rs
git commit -m "feat(broker): assemble flyby clips and concat them into the timelapse"
```

---

## Final verification (real run)

After all tasks, the Rust/C# unit suites pass but the Unity-dependent pieces (info-view render, flyby recording, ffmpeg concat on real frames) need a live run:

- [ ] `cd broker && cargo test -p skylinebench` — all green.
- [ ] `cd mod/test && dotnet run` — all green.
- [ ] `cd mod && ./build.sh` — mod builds.
- [ ] Run a real benchmark (`benchmark/run.sh --map <id>`), then `skylinebench timelapse <out_dir>`.
- [ ] Verify on the produced artifacts:
  - Overview frames show the red/green traffic layer, rotated to fill the frame, zoomed in.
  - `apply_plan` actions show one before/after pair per logical op.
  - `flyby_start.mp4` / `flyby_end.mp4` exist, show moving cars following the highways (N/S then W/E), and `timelapse.mp4` runs intro → core → outro.
- [ ] Tune the open knobs from the spec against what you see: overview `OVERVIEW_MIN_SIZE_M`/`OVERVIEW_MARGIN`, flyby `FLYBY_*` + duration/fps, traffic fade wait (0.5s in `Capture.cs`).
