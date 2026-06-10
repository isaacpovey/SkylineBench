# Render Overhaul & Timelapse Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Depends on:** `2026-06-10-skylinebench-segment-observability.md` (the `one_way`/`travel_direction` contract fields and per-segment density; the road-only filter removes the pipe/power clutter at the source).

**Goal:** Make `render_map` worth using — congestion-coloured roads, lane-count line widths, one-way arrows, a coordinate grid, and a machine-readable legend — and persist every frame to the run directory so a run can be replayed as a timelapse.

**Architecture:** `render.rs` stays a pure function but now takes the per-segment density map and draws layers (grid → roads coloured by density → one-way chevrons). `service::render_map` joins `/network` + `/metrics` and returns `(png, legend)`. The benchmark server writes each rendered PNG (plus an automatic full-map frame after every successful `step`) to a `--renders-dir`, with an `index.jsonl` sidecar describing each frame. `run.sh` points that at the session dir and moves it into the run's output dir after the session (the sandbox denies repo reads, so the broker can't reliably write into `benchmark/runs/` directly).

**Tech Stack:** Rust (tiny-skia, serde_json), bash.

**Evidence this matters:** the agent in run `20260609-210135` abandoned `render_map` after two calls ("includes pipes/power lines, which clutters it") and spent ~50 Bash calls drawing its own matplotlib maps — which still failed to show the one-way topology that killed the run.

---

### Task 1: render.rs — density colours, widths, arrows, grid

**Files:**
- Modify: `broker/src/render.rs` (full rework of `RenderOptions`, `render_network`, tests)
- Modify: `broker/examples/gen_golden.rs` (mirror)

- [ ] **Step 1: Write the failing tests**

Replace the `tests` module in `broker/src/render.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{NetNode, NetSegment};
    use std::collections::HashMap;

    fn sample_network() -> Network {
        Network {
            nodes: vec![
                NetNode { id: 1, x: -50.0, y: 0.0, z: -50.0 },
                NetNode { id: 2, x: 50.0, y: 0.0, z: -50.0 },
                NetNode { id: 3, x: 50.0, y: 0.0, z: 50.0 },
            ],
            segments: vec![
                NetSegment {
                    id: 10,
                    start_node: 1,
                    end_node: 2,
                    prefab: "road".into(),
                    lanes: 2,
                    length: 100.0,
                    one_way: false,
                    travel_direction: "both".into(),
                    speed_limit: 1.0,
                },
                NetSegment {
                    id: 11,
                    start_node: 2,
                    end_node: 3,
                    prefab: "oneway".into(),
                    lanes: 4,
                    length: 100.0,
                    one_way: true,
                    travel_direction: "start_to_end".into(),
                    speed_limit: 2.0,
                },
            ],
        }
    }

    fn sample_loads() -> HashMap<u32, f32> {
        HashMap::from([(10, 0.15), (11, 0.9)])
    }

    fn opts() -> RenderOptions {
        RenderOptions {
            bounds: Bounds { min_x: -100.0, min_z: -100.0, max_x: 100.0, max_z: 100.0 },
            width_px: 128,
            height_px: 128,
            grid_spacing_m: 50.0,
        }
    }

    #[test]
    fn render_is_deterministic() {
        let a = render_network(&sample_network(), &sample_loads(), &opts());
        let b = render_network(&sample_network(), &sample_loads(), &opts());
        assert_eq!(a, b, "rendering must be a pure function of its inputs");
    }

    #[test]
    fn render_matches_golden() {
        let produced = render_network(&sample_network(), &sample_loads(), &opts());
        let golden = include_bytes!("../fixtures/golden_map.png");
        assert_eq!(
            produced, golden,
            "render output changed; if intentional, regenerate via `cargo run --example gen_golden`"
        );
    }

    #[test]
    fn density_changes_the_image() {
        let hot: HashMap<u32, f32> = HashMap::from([(10, 1.0), (11, 1.0)]);
        assert_ne!(
            render_network(&sample_network(), &sample_loads(), &opts()),
            render_network(&sample_network(), &hot, &opts()),
            "congestion colouring must show up in the pixels"
        );
    }

    #[test]
    fn one_way_arrow_changes_the_image() {
        let mut both = sample_network();
        both.segments[1].one_way = false;
        both.segments[1].travel_direction = "both".into();
        assert_ne!(
            render_network(&sample_network(), &sample_loads(), &opts()),
            render_network(&both, &sample_loads(), &opts()),
            "one-way chevrons must show up in the pixels"
        );
    }

    #[test]
    fn grid_can_be_disabled() {
        let no_grid = RenderOptions { grid_spacing_m: 0.0, ..opts() };
        assert_ne!(
            render_network(&sample_network(), &sample_loads(), &opts()),
            render_network(&sample_network(), &sample_loads(), &no_grid),
        );
    }

    #[test]
    fn density_color_ramps_green_yellow_red() {
        assert_eq!(density_color(0.0), Color::from_rgba8(80, 200, 80, 255));
        assert_eq!(density_color(0.5), Color::from_rgba8(230, 230, 80, 255));
        assert_eq!(density_color(1.0), Color::from_rgba8(230, 60, 50, 255));
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --manifest-path broker/Cargo.toml render`
Expected: FAIL — `render_network` takes 2 arguments, no `grid_spacing_m`, no `density_color`.

- [ ] **Step 3: Implement the new renderer**

Replace the non-test body of `broker/src/render.rs` with:

```rust
use std::collections::HashMap;

use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Stroke, Transform};

use crate::contract::{Bounds, Network, Position};

pub struct RenderOptions {
    pub bounds: Bounds,
    pub width_px: u32,
    pub height_px: u32,
    /// World metres between gridlines; 0.0 disables the grid.
    pub grid_spacing_m: f32,
}

/// Map a world position to pixel coordinates within `bounds`. World +Z is drawn
/// upward, so the Z axis is flipped (screen Y grows downward).
fn to_pixel(p: Position, bounds: Bounds, w: u32, h: u32) -> (f32, f32) {
    let span_x = (bounds.max_x - bounds.min_x).max(f32::EPSILON);
    let span_z = (bounds.max_z - bounds.min_z).max(f32::EPSILON);
    let px = (p.x - bounds.min_x) / span_x * w as f32;
    let py = (1.0 - (p.z - bounds.min_z) / span_z) * h as f32;
    (px, py)
}

/// Congestion colour ramp: green (free) → yellow (busy, 0.5) → red (saturated).
fn density_color(d: f32) -> Color {
    let t = d.clamp(0.0, 1.0);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let (r, g, b) = if t < 0.5 {
        let t2 = t / 0.5;
        (lerp(80.0, 230.0, t2), lerp(200.0, 230.0, t2), 80.0)
    } else {
        let t2 = (t - 0.5) / 0.5;
        (230.0, lerp(230.0, 60.0, t2), lerp(80.0, 50.0, t2))
    };
    Color::from_rgba8(r as u8, g as u8, b as u8, 255)
}

fn stroke_line(pixmap: &mut Pixmap, a: (f32, f32), b: (f32, f32), color: Color, width: f32) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    let stroke = Stroke { width, ..Stroke::default() };
    let mut pb = PathBuilder::new();
    pb.move_to(a.0, a.1);
    pb.line_to(b.0, b.1);
    if let Some(path) = pb.finish() {
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

fn draw_grid(pixmap: &mut Pixmap, opts: &RenderOptions, w: u32, h: u32) {
    if opts.grid_spacing_m <= 0.0 {
        return;
    }
    let b = opts.bounds;
    let spacing = opts.grid_spacing_m;
    let line = |v: f32| (v / spacing).ceil() * spacing;
    let grid = Color::from_rgba8(45, 45, 58, 255);
    let axis = Color::from_rgba8(75, 75, 95, 255);
    let mut x = line(b.min_x);
    while x <= b.max_x {
        let (px, _) = to_pixel(Position { x, y: 0.0, z: b.min_z }, b, w, h);
        let color = if x == 0.0 { axis } else { grid };
        stroke_line(pixmap, (px, 0.0), (px, h as f32), color, 1.0);
        x += spacing;
    }
    let mut z = line(b.min_z);
    while z <= b.max_z {
        let (_, py) = to_pixel(Position { x: b.min_x, y: 0.0, z }, b, w, h);
        let color = if z == 0.0 { axis } else { grid };
        stroke_line(pixmap, (0.0, py), (w as f32, py), color, 1.0);
        z += spacing;
    }
}

/// Chevron at the segment midpoint pointing from `a` to `b` (pixel space).
fn draw_arrow(pixmap: &mut Pixmap, a: (f32, f32), b: (f32, f32)) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 6.0 {
        return;
    }
    let (ux, uy) = (dx / len, dy / len);
    let (mx, my) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    let tip = (mx + ux * 4.0, my + uy * 4.0);
    let left = (mx - uy * 3.0 - ux * 1.0, my + ux * 3.0 - uy * 1.0);
    let right = (mx + uy * 3.0 - ux * 1.0, my - ux * 3.0 - uy * 1.0);
    let white = Color::from_rgba8(245, 245, 245, 255);
    stroke_line(pixmap, left, tip, white, 1.2);
    stroke_line(pixmap, right, tip, white, 1.2);
}

/// Render the road network to PNG bytes. `loads` maps segment id → density
/// (0..1); segments missing from it draw neutral gray.
pub fn render_network(network: &Network, loads: &HashMap<u32, f32>, opts: &RenderOptions) -> Vec<u8> {
    // Clamp to at least 1px: dimensions arrive from MCP tool args, and
    // Pixmap::new returns None (→ panic) on a zero dimension.
    let w = opts.width_px.max(1);
    let h = opts.height_px.max(1);
    let mut pixmap = Pixmap::new(w, h).expect("dimensions are clamped to at least 1");
    pixmap.fill(Color::from_rgba8(20, 20, 28, 255));

    draw_grid(&mut pixmap, opts, w, h);

    let node_pos: HashMap<u32, Position> = network
        .nodes
        .iter()
        .map(|n| (n.id, Position { x: n.x, y: n.y, z: n.z }))
        .collect();

    for seg in &network.segments {
        if let (Some(a), Some(b)) = (node_pos.get(&seg.start_node), node_pos.get(&seg.end_node)) {
            let pa = to_pixel(*a, opts.bounds, w, h);
            let pb = to_pixel(*b, opts.bounds, w, h);
            let color = loads
                .get(&seg.id)
                .map(|d| density_color(*d))
                .unwrap_or(Color::from_rgba8(120, 120, 130, 255));
            let width = (1.0 + seg.lanes as f32 * 0.5).min(5.0);
            stroke_line(&mut pixmap, pa, pb, color, width);
            if seg.one_way {
                let (from, to) = if seg.travel_direction == "end_to_start" { (pb, pa) } else { (pa, pb) };
                draw_arrow(&mut pixmap, from, to);
            }
        }
    }

    pixmap
        .encode_png()
        .expect("PNG encoding never fails for a valid pixmap")
}
```

- [ ] **Step 4: Regenerate the golden fixture**

Replace `broker/examples/gen_golden.rs` body so it mirrors the new test fixtures exactly:

```rust
// Regenerates fixtures/golden_map.png for the render::tests::render_matches_golden
// test. The network, loads, and RenderOptions below MUST stay identical to
// render::tests::{sample_network, sample_loads, opts} — if they diverge,
// regenerating from this example produces a golden the test no longer matches.
use std::collections::HashMap;

use skylinebench::contract::{Bounds, NetNode, NetSegment, Network};
use skylinebench::render::{render_network, RenderOptions};

fn main() {
    let network = Network {
        nodes: vec![
            NetNode { id: 1, x: -50.0, y: 0.0, z: -50.0 },
            NetNode { id: 2, x: 50.0, y: 0.0, z: -50.0 },
            NetNode { id: 3, x: 50.0, y: 0.0, z: 50.0 },
        ],
        segments: vec![
            NetSegment {
                id: 10,
                start_node: 1,
                end_node: 2,
                prefab: "road".into(),
                lanes: 2,
                length: 100.0,
                one_way: false,
                travel_direction: "both".into(),
                speed_limit: 1.0,
            },
            NetSegment {
                id: 11,
                start_node: 2,
                end_node: 3,
                prefab: "oneway".into(),
                lanes: 4,
                length: 100.0,
                one_way: true,
                travel_direction: "start_to_end".into(),
                speed_limit: 2.0,
            },
        ],
    };
    let loads = HashMap::from([(10u32, 0.15f32), (11, 0.9)]);
    let opts = RenderOptions {
        bounds: Bounds { min_x: -100.0, min_z: -100.0, max_x: 100.0, max_z: 100.0 },
        width_px: 128,
        height_px: 128,
        grid_spacing_m: 50.0,
    };
    let png = render_network(&network, &loads, &opts);
    std::fs::create_dir_all("fixtures").unwrap();
    std::fs::write("fixtures/golden_map.png", png).unwrap();
    println!("wrote fixtures/golden_map.png");
}
```

Run: `(cd broker && cargo run --example gen_golden)`
Expected: `wrote fixtures/golden_map.png`.

- [ ] **Step 5: Fix the service call site and run the suite**

`broker/src/service.rs::render_map` no longer compiles (new signature) — patch it minimally for now (Task 2 reworks it properly):

```rust
pub async fn render_map(
    client: &BridgeClient,
    args: RenderMapArgs,
) -> Result<Vec<u8>, ServiceError> {
    let net = client.network().await?;
    let loads = client
        .metrics()
        .await?
        .traffic
        .segment_loads
        .iter()
        .map(|l| (l.segment_id, l.density))
        .collect();
    let opts = RenderOptions {
        bounds: args.bounds.unwrap_or_else(playable_bounds),
        width_px: args.width_px,
        height_px: args.height_px,
        grid_spacing_m: 1000.0,
    };
    Ok(render_network(&net, &loads, &opts))
}
```

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: ALL PASS (including the regenerated golden).

- [ ] **Step 6: Visually inspect one render**

Add a temporary test to `broker/src/render.rs`:

```rust
    #[test]
    fn write_preview_for_manual_inspection() {
        let big = RenderOptions { width_px: 512, height_px: 512, ..opts() };
        std::fs::write("/tmp/render_preview.png", render_network(&sample_network(), &sample_loads(), &big)).unwrap();
    }
```

Run: `cargo test --manifest-path broker/Cargo.toml write_preview`, then open `/tmp/render_preview.png` with the Read tool. Confirm: gridlines with brighter axes, a green-ish horizontal road, a red-ish diagonal road that is visibly wider (4 lanes), and a white chevron on the diagonal pointing from node 2 toward node 3 (up-right). Then DELETE the temporary test.

- [ ] **Step 7: Commit**

```bash
git add broker/src/render.rs broker/src/service.rs broker/examples/gen_golden.rs broker/fixtures/golden_map.png
git commit -m "feat(broker): congestion-coloured render with lane widths, one-way chevrons, grid"
```

---

### Task 2: render_map returns a legend; grid is configurable

**Files:**
- Modify: `broker/src/service.rs` (`RenderMapArgs`, `render_map` → returns `(Vec<u8>, Value)`, + test)
- Modify: `broker/src/tools.rs:72-87`
- Modify: `broker/src/benchmark/server.rs:158-181`

- [ ] **Step 1: Write the failing test**

In `broker/src/service.rs` tests, replace `render_map_returns_png_bytes` with:

```rust
    #[tokio::test]
    async fn render_map_returns_png_and_legend() {
        let c = client().await;
        let (png, legend) = render_map(
            &c,
            RenderMapArgs { bounds: None, width_px: 64, height_px: 64, grid_spacing_m: None },
        )
        .await
        .unwrap();
        assert_eq!(&png[1..4], b"PNG");
        assert_eq!(legend["grid_spacing_m"], 1000.0);
        assert!(legend["bounds"]["min_x"].is_number());
        assert!(legend["encoding"]["color"].is_string());
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --manifest-path broker/Cargo.toml render_map_returns`
Expected: FAIL — `render_map` returns `Vec<u8>`, `RenderMapArgs` has no `grid_spacing_m`.

- [ ] **Step 3: Implement**

In `broker/src/service.rs`:

```rust
#[derive(Deserialize, schemars::JsonSchema)]
pub struct RenderMapArgs {
    #[serde(default)]
    pub bounds: Option<Bounds>,
    #[serde(default = "default_size")]
    pub width_px: u32,
    #[serde(default = "default_size")]
    pub height_px: u32,
    /// World metres between gridlines (default 1000; 0 disables the grid).
    #[serde(default)]
    pub grid_spacing_m: Option<f32>,
}

/// Returns the rendered PNG bytes plus a JSON legend describing the encoding
/// (the rmcp layer returns both as image + text content blocks).
pub async fn render_map(
    client: &BridgeClient,
    args: RenderMapArgs,
) -> Result<(Vec<u8>, Value), ServiceError> {
    let net = client.network().await?;
    let loads: std::collections::HashMap<u32, f32> = client
        .metrics()
        .await?
        .traffic
        .segment_loads
        .iter()
        .map(|l| (l.segment_id, l.density))
        .collect();
    let opts = RenderOptions {
        bounds: args.bounds.unwrap_or_else(playable_bounds),
        width_px: args.width_px,
        height_px: args.height_px,
        grid_spacing_m: args.grid_spacing_m.unwrap_or(1000.0),
    };
    let legend = json!({
        "bounds": opts.bounds,
        "width_px": opts.width_px,
        "height_px": opts.height_px,
        "grid_spacing_m": opts.grid_spacing_m,
        "encoding": {
            "color": "segment congestion: green = free, yellow = busy, red = saturated, gray = no data",
            "line_width": "scales with lane count",
            "arrows": "white chevron = one-way travel direction",
            "orientation": "+x right, +z up; gridlines every grid_spacing_m world metres, brighter lines are the x=0 / z=0 axes",
        },
    });
    Ok((render_network(&net, &loads, &opts), legend))
}
```

`broker/src/tools.rs::render_map`:

```rust
    #[tool(description = "Render the road network to a PNG image: congestion colours, lane widths, \
        one-way arrows, coordinate grid. Returns the image plus a JSON legend.")]
    async fn render_map(
        &self,
        Parameters(args): Parameters<RenderMapArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match service::render_map(&self.client, args).await {
            Ok((png, legend)) => {
                let data = base64::engine::general_purpose::STANDARD.encode(png);
                Ok(CallToolResult::success(vec![
                    Content::image(data, "image/png".to_string()),
                    Content::text(legend.to_string()),
                ]))
            }
            Err(e) => Ok(tool_error(e)),
        }
    }
```

`broker/src/benchmark/server.rs::render_map` — replace the whole `Ok(png) => { ... }` arm with (the timeout-check/persist/progress block is today's code, just kept inside the new destructure):

```rust
            Ok((png, legend)) => {
                let data = base64::engine::general_purpose::STANDARD.encode(&png);
                let progress = {
                    let mut s = self.state.lock().await;
                    s.check_timeout();
                    if let Some(p) = &self.persist {
                        if let Err(e) = p.write(&s) {
                            eprintln!("benchmark: end-state persist error: {e}");
                        }
                    }
                    s.progress()
                };
                let mut text = legend;
                if let Value::Object(ref mut map) = text {
                    map.insert("benchmark_progress".into(), progress);
                }
                Ok(CallToolResult::success(vec![
                    Content::image(data, "image/png".to_string()),
                    Content::text(text.to_string()),
                ]))
            }
```

(keep `png` un-moved — it is persisted in Task 3, hence `encode(&png)`).

- [ ] **Step 4: Run the suite, then commit**

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: ALL PASS.

```bash
git add broker/src/service.rs broker/src/tools.rs broker/src/benchmark/server.rs
git commit -m "feat(broker): render_map legend + configurable grid"
```

---

### Task 3: Persist render frames per run (timelapse)

**Files:**
- Modify: `broker/src/benchmark/state.rs` (add `render_seq`)
- Modify: `broker/src/benchmark/server.rs` (renders_dir plumbing; write on render_map and after step)
- Modify: `broker/src/main.rs` (`--renders-dir` arg)
- Modify: `benchmark/run.sh` (pass the flag; move frames into OUT_DIR)
- Modify: `benchmark/README.md` (timelapse how-to)

- [ ] **Step 1: Write the failing server test**

Add to `broker/src/benchmark/server.rs` tests:

```rust
    #[tokio::test]
    async fn renders_are_persisted_with_index() {
        let dir = std::env::temp_dir().join(format!("sb-renders-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let bench = bench_with_mock().await.with_renders_dir(dir.clone());

        bench
            .render_map(Parameters(crate::service::RenderMapArgs {
                bounds: None,
                width_px: 32,
                height_px: 32,
                grid_spacing_m: None,
            }))
            .await
            .unwrap();
        bench
            .control_time(Parameters(crate::service::ControlTimeArgs {
                op: "step".into(),
                ticks: Some(10),
                speed: None,
            }))
            .await
            .unwrap();

        let mut frames: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
            .filter(|n| n.ends_with(".png"))
            .collect();
        frames.sort();
        assert_eq!(frames.len(), 2, "one agent render + one auto step frame: {frames:?}");
        assert!(frames[0].starts_with("00001"), "{frames:?}");

        let index = std::fs::read_to_string(dir.join("index.jsonl")).unwrap();
        let lines: Vec<serde_json::Value> =
            index.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["trigger"], "render_map");
        assert_eq!(lines[1]["trigger"], "step");
        assert!(lines[1]["tick"].is_u64());
        std::fs::remove_dir_all(&dir).ok();
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --manifest-path broker/Cargo.toml renders_are_persisted`
Expected: FAIL — no `with_renders_dir`.

- [ ] **Step 3: Implement persistence**

`broker/src/benchmark/state.rs` — add to `RunState`:

```rust
    pub render_seq: u32,
```
initialised `render_seq: 0,` in `RunState::new`, plus:
```rust
    pub fn next_render_seq(&mut self) -> u32 {
        self.render_seq += 1;
        self.render_seq
    }
```

`broker/src/benchmark/server.rs`:

```rust
#[derive(Clone)]
pub struct BenchmarkServer {
    client: Arc<BridgeClient>,
    state: Arc<Mutex<RunState>>,
    persist: Option<Arc<EndStatePersister>>,
    renders_dir: Option<std::path::PathBuf>,
    tool_router: ToolRouter<Self>,
}
```
(update `new` to set `renders_dir: None` and `with_persist`'s struct-update accordingly), plus:

```rust
    pub fn with_renders_dir(self, dir: std::path::PathBuf) -> Self {
        Self { renders_dir: Some(dir), ..self }
    }

    /// Best-effort frame write: a failed render persist must never fail the
    /// tool call (same policy as end-state persistence).
    async fn persist_render(&self, png: &[u8], tick: u64, trigger: &str) {
        let Some(dir) = &self.renders_dir else { return };
        let (seq, changes, flow) = {
            let mut s = self.state.lock().await;
            (s.next_render_seq(), s.num_changes, s.flow.mean())
        };
        let _ = std::fs::create_dir_all(dir);
        let name = format!("{seq:05}-tick{tick}.png");
        if let Err(e) = std::fs::write(dir.join(&name), png) {
            eprintln!("benchmark: render persist error: {e}");
            return;
        }
        let line = serde_json::json!({
            "seq": seq, "file": name, "tick": tick, "trigger": trigger,
            "changes": changes, "flow": flow,
        });
        let appended = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("index.jsonl"))
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "{line}")
            });
        if let Err(e) = appended {
            eprintln!("benchmark: render index error: {e}");
        }
    }
```

In the `render_map` handler, after the `Ok((png, legend))` destructure and before building the response:

```rust
                let tick = self.client.health().await.map(|h| h.tick).unwrap_or(0);
                self.persist_render(&png, tick, "render_map").await;
```

In the `control_time` handler, in the step branch after the chunk loop succeeds (i.e. once `out` and the flow sample exist) — render an automatic full-map frame:

```rust
        if self.renders_dir.is_some() {
            let frame = service::render_map(
                &self.client,
                crate::service::RenderMapArgs {
                    bounds: None,
                    width_px: 1024,
                    height_px: 1024,
                    grid_spacing_m: None,
                },
            )
            .await;
            if let Ok((png, _)) = frame {
                let tick = out.get("tick").and_then(|t| t.as_u64()).unwrap_or(0);
                self.persist_render(&png, tick, "step").await;
            }
        }
```

(If the harness-reliability plan has not landed yet, the step branch is the existing single-call path — insert the same block after its metrics fetch, reading `tick` from the response value.)

- [ ] **Step 4: Run the suite**

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: ALL PASS.

- [ ] **Step 5: Plumb the CLI flag**

`broker/src/main.rs` — add to the `Benchmark` variant:

```rust
        /// Directory for per-run render frames (timelapse). Omit to disable.
        #[arg(long)]
        renders_dir: Option<std::path::PathBuf>,
```
destructure it in the match arm and build the server with:

```rust
            let server = {
                let s = BenchmarkServer::new(client, state.clone()).with_persist(persister.clone());
                match renders_dir {
                    Some(dir) => s.with_renders_dir(dir),
                    None => s,
                }
            }
            .serve((tokio::io::stdin(), tokio::io::stdout()))
            .await?;
```

- [ ] **Step 6: Wire run.sh**

In `benchmark/run.sh`, in the mcp.json heredoc, extend the broker command with `--renders-dir $SESSION_DIR/renders`:

```bash
      "args": ["-c", "$BROKER_BIN benchmark --map $MAP --map-source $MAP_SOURCE --mod-url $MOD_URL --out $OUT_DIR --renders-dir $SESSION_DIR/renders"]
```

After the agent session (both watch and headless branches), before `benchmark-finalize`, move the frames into the run dir:

```bash
if [ -d "$SESSION_DIR/renders" ]; then
  mv "$SESSION_DIR/renders" "$OUT_DIR/renders"
fi
```

Verify: `bash -n benchmark/run.sh` then `DRY_RUN=1 ./benchmark/run.sh --map gridlock-v1` shows `--renders-dir` in the printed command.

- [ ] **Step 7: Document the timelapse**

Add to `benchmark/README.md` under the artifacts list:

```markdown
   - `renders/` — one PNG per agent `render_map` call plus an automatic
     full-map frame after every sim step, with `index.jsonl` (tick, changes,
     flow per frame). Timelapse:
     `ffmpeg -framerate 4 -pattern_type glob -i 'benchmark/runs/<ts>/renders/*.png' -pix_fmt yuv420p timelapse.mp4`
```

- [ ] **Step 8: Commit**

```bash
git add broker/src/benchmark/state.rs broker/src/benchmark/server.rs broker/src/main.rs benchmark/run.sh benchmark/README.md
git commit -m "feat(benchmark): persist render frames per run for timelapse"
```
