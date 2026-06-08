# SkylineBench Broker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Rust `skylinebench` MCP server (the "broker") that exposes Cities: Skylines 1 to AI agents, fully implemented and tested end-to-end against a mock mod with zero game dependency.

**Architecture:** A single Rust binary with two subcommands: `serve` (the MCP server over stdio) and `mock` (an in-memory HTTP server implementing the mod contract, for testing/development). The broker talks to the mod over localhost HTTP. All non-trivial logic (contract types, geometry, graph assembly, validation, schematic rendering, service-layer tool logic) lives in pure/testable modules; the rmcp adapter and the HTTP client are thin. The mock mod implements the exact same HTTP contract the real C# mod will, so the broker is verified end-to-end before the game is ever involved.

**Tech Stack:** Rust 2021, `tokio` (async runtime), `rmcp` (MCP SDK), `axum` (mock mod HTTP server), `reqwest` (HTTP client to the mod), `serde`/`serde_json`, `tiny-skia` (pure-Rust 2D rendering for golden-image tests), `thiserror`, `clap` (CLI), `anyhow`.

This plan implements spec `docs/superpowers/specs/2026-06-08-skylinebench-phase1-design.md`. It covers the broker only; the C# mod, `setup`/`doctor`, and live integration are a separate plan.

---

## File structure

All paths are under `broker/`.

| File | Responsibility |
|---|---|
| `Cargo.toml` | Package `skylinebench` (lib + bin), dependencies. |
| `src/lib.rs` | Library root; `pub mod` declarations. |
| `src/contract.rs` | Serde types for the HTTP contract + `ActionError` reason vocabulary. The single source of truth shared by `bridge_client` and `mock`. |
| `src/geometry.rs` | `Position`, `Bounds`, distance, in-bounds, snapping, segment length, constants. Pure. |
| `src/graph.rs` | Assemble a `Network` into adjacency/connectivity; find intersections & dead-ends. Pure. |
| `src/validate.rs` | Pre-flight validation of build args → `ValidationError`. Pure. |
| `src/render.rs` | `Network` (+ overlays) → PNG bytes via `tiny-skia`. Pure. Golden-tested. |
| `src/bridge_client.rs` | Typed async HTTP client for the mod. The only outbound-I/O module. |
| `src/service.rs` | Async tool logic: takes a `&BridgeClient` + typed args, returns `serde_json::Value`. Fully testable against the mock. |
| `src/tools.rs` | Thin rmcp adapter: declares MCP tools, deserializes args, calls `service`. |
| `src/mock.rs` | In-memory `axum` server implementing the contract; used by tests and the `mock` subcommand. |
| `src/main.rs` | CLI (`clap`): `serve` and `mock` subcommands. |
| `tests/broker_e2e.rs` | End-to-end: spin up the mock, drive the service layer through a full observe→build→step→observe loop. |
| `fixtures/golden_map.png` | Committed golden image for the renderer test. |
| `README.md` | How to build, run `mock`, and register `serve` with Claude Code / Codex. |

---

## Task 0: Scaffold the Cargo project

**Files:**
- Create: `broker/Cargo.toml`
- Create: `broker/src/lib.rs`
- Create: `broker/src/main.rs`
- Create: `broker/rust-toolchain.toml`

- [ ] **Step 1: Create the Cargo manifest**

Create `broker/Cargo.toml`:

```toml
[package]
name = "skylinebench"
version = "0.1.0"
edition = "2021"

[lib]
name = "skylinebench"
path = "src/lib.rs"

[[bin]]
name = "skylinebench"
path = "src/main.rs"

[dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros", "io-std", "sync", "net"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
anyhow = "1"
clap = { version = "4", features = ["derive"] }
reqwest = { version = "0.12", features = ["json"] }
axum = "0.7"
tiny-skia = "0.11"
rmcp = { version = "0.1", features = ["server", "transport-io"] }

[dev-dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time"] }
```

> Note for executor: `rmcp` is young and its API moves between releases. Before Task 8, pin the exact latest `0.x` version and read its server example on docs.rs; the macro attribute names in Task 8 may need to match that version. Everything before Task 8 is independent of `rmcp`.

- [ ] **Step 2: Create the toolchain file**

Create `broker/rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

- [ ] **Step 3: Create a minimal lib root**

Create `broker/src/lib.rs`:

```rust
pub mod contract;
pub mod geometry;
pub mod graph;
pub mod validate;
pub mod render;
pub mod bridge_client;
pub mod service;
pub mod mock;
pub mod tools;
```

> The modules don't exist yet — this won't compile until Task 1. That's expected; we add the file in Step 3 but comment out modules not yet created.

Actually create it with everything commented except as we go. Start with all lines commented:

```rust
// Modules are uncommented as each task lands.
// pub mod contract;
// pub mod geometry;
// pub mod graph;
// pub mod validate;
// pub mod render;
// pub mod bridge_client;
// pub mod service;
// pub mod mock;
// pub mod tools;
```

- [ ] **Step 4: Create a placeholder main**

Create `broker/src/main.rs`:

```rust
fn main() {
    println!("skylinebench: not yet wired up");
}
```

- [ ] **Step 5: Verify it builds**

Run: `cd broker && cargo build`
Expected: compiles successfully (a warning-free binary that prints the placeholder).

- [ ] **Step 6: Commit**

```bash
git add broker/
git commit -m "chore: scaffold skylinebench broker crate"
```

---

## Task 1: Contract types

The serde types mirroring the HTTP contract from spec §3. Source of truth for both the client and the mock.

**Files:**
- Create: `broker/src/contract.rs`
- Modify: `broker/src/lib.rs` (uncomment `pub mod contract;`)

- [ ] **Step 1: Write the failing test**

Create `broker/src/contract.rs` with the types and a round-trip test:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub min_x: f32,
    pub min_z: f32,
    pub max_x: f32,
    pub max_z: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Health {
    pub mod_version: String,
    pub game_version: String,
    pub city_loaded: bool,
    pub paused: bool,
    pub tick: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetNode {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetSegment {
    pub id: u32,
    pub start_node: u32,
    pub end_node: u32,
    pub prefab: String,
    pub lanes: u8,
    pub length: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Network {
    pub nodes: Vec<NetNode>,
    pub segments: Vec<NetSegment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Building {
    pub id: u32,
    pub prefab: String,
    pub category: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub footprint_width: f32,
    pub footprint_length: f32,
    pub level: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Buildings {
    pub buildings: Vec<Building>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoneCell {
    pub x: f32,
    pub z: f32,
    pub zone_type: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Zones {
    pub cells: Vec<ZoneCell>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentLoad {
    pub segment_id: u32,
    pub density: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrafficMetrics {
    pub flow_percent: f32,
    pub active_vehicles: u32,
    pub segment_loads: Vec<SegmentLoad>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EconomyMetrics {
    pub balance: i64,
    pub weekly_income: i64,
    pub weekly_expenses: i64,
    pub funds: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PopulationMetrics {
    pub total: u32,
    pub residential_demand: u8,
    pub commercial_demand: u8,
    pub industrial_demand: u8,
    pub office_demand: u8,
    pub employed: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceMetrics {
    pub happiness: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    pub tick: u64,
    pub traffic: TrafficMetrics,
    pub economy: EconomyMetrics,
    pub population: PopulationMetrics,
    pub services: ServiceMetrics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoadTypes {
    pub road_types: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoneTypes {
    pub zone_types: Vec<String>,
}

/// Normalised failure reasons for actions. Extends spec §5's mod-side set
/// (`COLLISION`, `INSUFFICIENT_FUNDS`, `OUT_OF_BOUNDS`, `INVALID_PREFAB`,
/// `SEGMENT_TOO_LONG`, `UNKNOWN`) with broker-side pre-validation reasons
/// (`DEGENERATE_SEGMENT`, `INVALID_ARGS`). NOTE: sync this addition back into
/// the spec during spec review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActionError {
    Collision,
    InsufficientFunds,
    OutOfBounds,
    InvalidPrefab,
    SegmentTooLong,
    DegenerateSegment,
    InvalidArgs,
    Unknown,
}

/// Result of a mutating action. `ok == true` ⇒ the diff fields are meaningful;
/// `ok == false` ⇒ `reason` is set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionResult {
    pub ok: bool,
    #[serde(default)]
    pub created_nodes: Vec<u32>,
    #[serde(default)]
    pub created_segments: Vec<u32>,
    #[serde(default)]
    pub snapped_nodes: Vec<u32>,
    #[serde(default)]
    pub destroyed: Vec<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reason: Option<ActionError>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClockState {
    pub ok: bool,
    pub paused: bool,
    pub tick: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_error_serializes_screaming_snake() {
        let json = serde_json::to_string(&ActionError::SegmentTooLong).unwrap();
        assert_eq!(json, "\"SEGMENT_TOO_LONG\"");
    }

    #[test]
    fn action_result_round_trips() {
        let original = ActionResult {
            ok: true,
            created_nodes: vec![1, 2],
            created_segments: vec![10],
            snapped_nodes: vec![1],
            destroyed: vec![],
            reason: None,
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: ActionResult = serde_json::from_str(&json).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn metrics_round_trips() {
        let m = Metrics {
            tick: 42,
            traffic: TrafficMetrics { flow_percent: 73.5, active_vehicles: 120, segment_loads: vec![SegmentLoad { segment_id: 5, density: 0.8 }] },
            economy: EconomyMetrics { balance: 1000, weekly_income: 500, weekly_expenses: 400, funds: 50000 },
            population: PopulationMetrics { total: 2000, residential_demand: 50, commercial_demand: 40, industrial_demand: 30, office_demand: 20, employed: 1500 },
            services: ServiceMetrics { happiness: 80 },
        };
        let json = serde_json::to_string(&m).unwrap();
        let parsed: Metrics = serde_json::from_str(&json).unwrap();
        assert_eq!(m, parsed);
    }
}
```

- [ ] **Step 2: Uncomment the module**

In `broker/src/lib.rs`, uncomment `pub mod contract;`.

- [ ] **Step 3: Run tests to verify they pass**

Run: `cd broker && cargo test contract::`
Expected: 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add broker/src/contract.rs broker/src/lib.rs
git commit -m "feat: add HTTP contract types"
```

---

## Task 2: Geometry module

**Files:**
- Create: `broker/src/geometry.rs`
- Modify: `broker/src/lib.rs` (uncomment `pub mod geometry;`)

- [ ] **Step 1: Write the failing tests + implementation skeleton**

Create `broker/src/geometry.rs`:

```rust
use crate::contract::{Bounds, NetNode, Position};

/// Half the side length of the playable area, in metres (≈ 9×9 grid of 1920 m
/// tiles). Working assumption from spec §5; confirm against the live API in the
/// mod plan.
pub const PLAYABLE_HALF_EXTENT_M: f32 = 8640.0;

/// Snap radius for reusing an existing node instead of creating a new one
/// (one CS1 zone cell). Spec §5.
pub const SNAP_TOLERANCE_M: f32 = 8.0;

/// Maximum length of a single straight road segment, in metres. Working value;
/// the real game limit is pinned during mod integration.
pub const MAX_SEGMENT_LENGTH_M: f32 = 200.0;

pub fn playable_bounds() -> Bounds {
    Bounds {
        min_x: -PLAYABLE_HALF_EXTENT_M,
        min_z: -PLAYABLE_HALF_EXTENT_M,
        max_x: PLAYABLE_HALF_EXTENT_M,
        max_z: PLAYABLE_HALF_EXTENT_M,
    }
}

/// Horizontal (X/Z) distance between two positions; Y (elevation) is ignored
/// because roads connect in the horizontal plane.
pub fn horizontal_distance(a: Position, b: Position) -> f32 {
    let dx = a.x - b.x;
    let dz = a.z - b.z;
    (dx * dx + dz * dz).sqrt()
}

pub fn in_bounds(p: Position, bounds: Bounds) -> bool {
    p.x >= bounds.min_x && p.x <= bounds.max_x && p.z >= bounds.min_z && p.z <= bounds.max_z
}

/// Returns the id of the nearest node within `SNAP_TOLERANCE_M` of `p`, if any.
pub fn nearest_node_within_tolerance(p: Position, nodes: &[NetNode]) -> Option<u32> {
    nodes
        .iter()
        .map(|n| (n.id, horizontal_distance(p, Position { x: n.x, y: n.y, z: n.z })))
        .filter(|(_, d)| *d <= SNAP_TOLERANCE_M)
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .map(|(id, _)| id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(x: f32, z: f32) -> Position {
        Position { x, y: 0.0, z }
    }

    #[test]
    fn distance_is_horizontal() {
        assert_eq!(horizontal_distance(pos(0.0, 0.0), pos(3.0, 4.0)), 5.0);
    }

    #[test]
    fn distance_ignores_elevation() {
        let a = Position { x: 0.0, y: 0.0, z: 0.0 };
        let b = Position { x: 3.0, y: 100.0, z: 4.0 };
        assert_eq!(horizontal_distance(a, b), 5.0);
    }

    #[test]
    fn in_bounds_accepts_inside_and_rejects_outside() {
        let b = playable_bounds();
        assert!(in_bounds(pos(0.0, 0.0), b));
        assert!(!in_bounds(pos(PLAYABLE_HALF_EXTENT_M + 1.0, 0.0), b));
    }

    #[test]
    fn snap_finds_nearest_within_tolerance() {
        let nodes = vec![
            NetNode { id: 1, x: 5.0, y: 0.0, z: 0.0 },
            NetNode { id: 2, x: 2.0, y: 0.0, z: 0.0 },
        ];
        assert_eq!(nearest_node_within_tolerance(pos(0.0, 0.0), &nodes), Some(2));
    }

    #[test]
    fn snap_returns_none_when_all_too_far() {
        let nodes = vec![NetNode { id: 1, x: 50.0, y: 0.0, z: 0.0 }];
        assert_eq!(nearest_node_within_tolerance(pos(0.0, 0.0), &nodes), None);
    }
}
```

- [ ] **Step 2: Uncomment the module**

In `broker/src/lib.rs`, uncomment `pub mod geometry;`.

- [ ] **Step 3: Run tests**

Run: `cd broker && cargo test geometry::`
Expected: 5 tests pass.

- [ ] **Step 4: Commit**

```bash
git add broker/src/geometry.rs broker/src/lib.rs
git commit -m "feat: add geometry helpers (distance, bounds, snapping)"
```

---

## Task 3: Graph module

Turn a raw `Network` into connectivity info: node degree, intersections (degree ≥ 3), dead-ends (degree 1).

**Files:**
- Create: `broker/src/graph.rs`
- Modify: `broker/src/lib.rs` (uncomment `pub mod graph;`)

- [ ] **Step 1: Write the failing tests + implementation**

Create `broker/src/graph.rs`:

```rust
use std::collections::HashMap;

use crate::contract::Network;

#[derive(Debug, Clone, PartialEq)]
pub struct Connectivity {
    /// node id -> ids of directly connected nodes (via a segment)
    pub adjacency: HashMap<u32, Vec<u32>>,
}

impl Connectivity {
    pub fn degree(&self, node_id: u32) -> usize {
        self.adjacency.get(&node_id).map(|v| v.len()).unwrap_or(0)
    }

    /// Nodes with three or more connections — junctions.
    pub fn intersections(&self) -> Vec<u32> {
        let mut out: Vec<u32> = self
            .adjacency
            .iter()
            .filter(|(_, neighbours)| neighbours.len() >= 3)
            .map(|(id, _)| *id)
            .collect();
        out.sort_unstable();
        out
    }

    /// Nodes with exactly one connection — dead-ends.
    pub fn dead_ends(&self) -> Vec<u32> {
        let mut out: Vec<u32> = self
            .adjacency
            .iter()
            .filter(|(_, neighbours)| neighbours.len() == 1)
            .map(|(id, _)| *id)
            .collect();
        out.sort_unstable();
        out
    }
}

pub fn build_connectivity(network: &Network) -> Connectivity {
    let adjacency = network.segments.iter().fold(
        HashMap::<u32, Vec<u32>>::new(),
        |mut acc, seg| {
            acc.entry(seg.start_node).or_default().push(seg.end_node);
            acc.entry(seg.end_node).or_default().push(seg.start_node);
            acc
        },
    );
    Connectivity { adjacency }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{NetNode, NetSegment};

    fn node(id: u32) -> NetNode {
        NetNode { id, x: id as f32, y: 0.0, z: 0.0 }
    }

    fn seg(id: u32, a: u32, b: u32) -> NetSegment {
        NetSegment { id, start_node: a, end_node: b, prefab: "road".into(), lanes: 2, length: 10.0 }
    }

    // Network shaped like:  1 - 2 - 3,  2 - 4  (node 2 is a junction; 1,3,4 are dead-ends)
    fn sample() -> Network {
        Network {
            nodes: vec![node(1), node(2), node(3), node(4)],
            segments: vec![seg(10, 1, 2), seg(11, 2, 3), seg(12, 2, 4)],
        }
    }

    #[test]
    fn degree_counts_connections() {
        let c = build_connectivity(&sample());
        assert_eq!(c.degree(2), 3);
        assert_eq!(c.degree(1), 1);
    }

    #[test]
    fn intersections_are_degree_three_plus() {
        let c = build_connectivity(&sample());
        assert_eq!(c.intersections(), vec![2]);
    }

    #[test]
    fn dead_ends_are_degree_one() {
        let c = build_connectivity(&sample());
        assert_eq!(c.dead_ends(), vec![1, 3, 4]);
    }

    #[test]
    fn empty_network_has_no_features() {
        let c = build_connectivity(&Network { nodes: vec![], segments: vec![] });
        assert!(c.intersections().is_empty());
        assert!(c.dead_ends().is_empty());
    }
}
```

- [ ] **Step 2: Uncomment the module**

In `broker/src/lib.rs`, uncomment `pub mod graph;`.

- [ ] **Step 3: Run tests**

Run: `cd broker && cargo test graph::`
Expected: 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add broker/src/graph.rs broker/src/lib.rs
git commit -m "feat: add network connectivity graph"
```

---

## Task 4: Validation module

Pre-flight checks for `build_road` so failures are legible before they reach the game.

**Files:**
- Create: `broker/src/validate.rs`
- Modify: `broker/src/lib.rs` (uncomment `pub mod validate;`)

- [ ] **Step 1: Write the failing tests + implementation**

Create `broker/src/validate.rs`:

```rust
use crate::contract::{ActionError, Position};
use crate::geometry::{horizontal_distance, in_bounds, playable_bounds, MAX_SEGMENT_LENGTH_M};

/// Validate a proposed straight road segment. `known_road_types` is the set the
/// mod reported via `GET /road-types`. Returns the first failing reason, or
/// `Ok(())` if the build is structurally acceptable (the game may still reject
/// it for COLLISION / INSUFFICIENT_FUNDS, which only the mod can know).
pub fn validate_build_road(
    start: Position,
    end: Position,
    road_type: &str,
    known_road_types: &[String],
) -> Result<(), ActionError> {
    if !known_road_types.iter().any(|t| t == road_type) {
        return Err(ActionError::InvalidPrefab);
    }
    let bounds = playable_bounds();
    if !in_bounds(start, bounds) || !in_bounds(end, bounds) {
        return Err(ActionError::OutOfBounds);
    }
    let length = horizontal_distance(start, end);
    if length < f32::EPSILON {
        return Err(ActionError::DegenerateSegment);
    }
    if length > MAX_SEGMENT_LENGTH_M {
        return Err(ActionError::SegmentTooLong);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(x: f32, z: f32) -> Position {
        Position { x, y: 0.0, z }
    }

    fn road_types() -> Vec<String> {
        vec!["road".into(), "highway".into()]
    }

    #[test]
    fn accepts_a_valid_segment() {
        assert_eq!(validate_build_road(pos(0.0, 0.0), pos(50.0, 0.0), "road", &road_types()), Ok(()));
    }

    #[test]
    fn rejects_unknown_prefab() {
        assert_eq!(
            validate_build_road(pos(0.0, 0.0), pos(50.0, 0.0), "monorail", &road_types()),
            Err(ActionError::InvalidPrefab)
        );
    }

    #[test]
    fn rejects_out_of_bounds() {
        assert_eq!(
            validate_build_road(pos(0.0, 0.0), pos(99999.0, 0.0), "road", &road_types()),
            Err(ActionError::OutOfBounds)
        );
    }

    #[test]
    fn rejects_degenerate_segment() {
        assert_eq!(
            validate_build_road(pos(10.0, 10.0), pos(10.0, 10.0), "road", &road_types()),
            Err(ActionError::DegenerateSegment)
        );
    }

    #[test]
    fn rejects_too_long_segment() {
        assert_eq!(
            validate_build_road(pos(0.0, 0.0), pos(MAX_SEGMENT_LENGTH_M + 1.0, 0.0), "road", &road_types()),
            Err(ActionError::SegmentTooLong)
        );
    }
}
```

- [ ] **Step 2: Uncomment the module**

In `broker/src/lib.rs`, uncomment `pub mod validate;`.

- [ ] **Step 3: Run tests**

Run: `cd broker && cargo test validate::`
Expected: 5 tests pass.

- [ ] **Step 4: Commit**

```bash
git add broker/src/validate.rs broker/src/lib.rs
git commit -m "feat: add build-road pre-flight validation"
```

---

## Task 5: Schematic renderer

`Network` → top-down PNG via `tiny-skia`. Roads drawn as lines, nodes as dots. Golden-image tested.

**Files:**
- Create: `broker/src/render.rs`
- Create: `broker/fixtures/golden_map.png` (generated in Step 4)
- Modify: `broker/src/lib.rs` (uncomment `pub mod render;`)

- [ ] **Step 1: Write the implementation**

Create `broker/src/render.rs`:

```rust
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Stroke, Transform};

use crate::contract::{Bounds, Network, Position};

pub struct RenderOptions {
    pub bounds: Bounds,
    pub width_px: u32,
    pub height_px: u32,
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

/// Render the network to PNG bytes.
pub fn render_network(network: &Network, opts: &RenderOptions) -> Vec<u8> {
    let mut pixmap = Pixmap::new(opts.width_px, opts.height_px).expect("non-zero dimensions");
    pixmap.fill(Color::from_rgba8(20, 20, 28, 255));

    let node_pos = network
        .nodes
        .iter()
        .map(|n| (n.id, Position { x: n.x, y: n.y, z: n.z }))
        .collect::<std::collections::HashMap<_, _>>();

    let mut road_paint = Paint::default();
    road_paint.set_color(Color::from_rgba8(230, 230, 120, 255));
    road_paint.anti_alias = true;
    let stroke = Stroke { width: 2.0, ..Stroke::default() };

    for seg in &network.segments {
        if let (Some(a), Some(b)) = (node_pos.get(&seg.start_node), node_pos.get(&seg.end_node)) {
            let (ax, ay) = to_pixel(*a, opts.bounds, opts.width_px, opts.height_px);
            let (bx, by) = to_pixel(*b, opts.bounds, opts.width_px, opts.height_px);
            let mut pb = PathBuilder::new();
            pb.move_to(ax, ay);
            pb.line_to(bx, by);
            if let Some(path) = pb.finish() {
                pixmap.stroke_path(&path, &road_paint, &stroke, Transform::identity(), None);
            }
        }
    }

    pixmap.encode_png().expect("PNG encoding never fails for a valid pixmap")
}
```

- [ ] **Step 2: Write the golden test (initially failing — no golden yet)**

Append to `broker/src/render.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{NetNode, NetSegment};

    fn sample_network() -> Network {
        Network {
            nodes: vec![
                NetNode { id: 1, x: -50.0, y: 0.0, z: -50.0 },
                NetNode { id: 2, x: 50.0, y: 0.0, z: -50.0 },
                NetNode { id: 3, x: 50.0, y: 0.0, z: 50.0 },
            ],
            segments: vec![
                NetSegment { id: 10, start_node: 1, end_node: 2, prefab: "road".into(), lanes: 2, length: 100.0 },
                NetSegment { id: 11, start_node: 2, end_node: 3, prefab: "road".into(), lanes: 2, length: 100.0 },
            ],
        }
    }

    fn opts() -> RenderOptions {
        RenderOptions {
            bounds: Bounds { min_x: -100.0, min_z: -100.0, max_x: 100.0, max_z: 100.0 },
            width_px: 128,
            height_px: 128,
        }
    }

    #[test]
    fn render_is_deterministic() {
        let a = render_network(&sample_network(), &opts());
        let b = render_network(&sample_network(), &opts());
        assert_eq!(a, b, "rendering must be a pure function of its inputs");
    }

    #[test]
    fn render_matches_golden() {
        let produced = render_network(&sample_network(), &opts());
        let golden = include_bytes!("../fixtures/golden_map.png");
        assert_eq!(
            produced, golden,
            "render output changed; if intentional, regenerate fixtures/golden_map.png (see Task 5 Step 4)"
        );
    }
}
```

- [ ] **Step 3: Uncomment the module and run the deterministic test**

In `broker/src/lib.rs`, uncomment `pub mod render;`.

Run: `cd broker && cargo test render::tests::render_is_deterministic`
Expected: PASS. (`render_matches_golden` will fail to compile/run until the golden exists — that's Step 4.)

- [ ] **Step 4: Generate the golden image**

The `render_matches_golden` test references a file that doesn't exist yet. Generate it with a throwaway binary, then confirm by eye it's a sensible top-down map.

Create `broker/examples/gen_golden.rs`:

```rust
use skylinebench::contract::{NetNode, NetSegment, Network, Bounds};
use skylinebench::render::{render_network, RenderOptions};

fn main() {
    let network = Network {
        nodes: vec![
            NetNode { id: 1, x: -50.0, y: 0.0, z: -50.0 },
            NetNode { id: 2, x: 50.0, y: 0.0, z: -50.0 },
            NetNode { id: 3, x: 50.0, y: 0.0, z: 50.0 },
        ],
        segments: vec![
            NetSegment { id: 10, start_node: 1, end_node: 2, prefab: "road".into(), lanes: 2, length: 100.0 },
            NetSegment { id: 11, start_node: 2, end_node: 3, prefab: "road".into(), lanes: 2, length: 100.0 },
        ],
    };
    let opts = RenderOptions {
        bounds: Bounds { min_x: -100.0, min_z: -100.0, max_x: 100.0, max_z: 100.0 },
        width_px: 128,
        height_px: 128,
    };
    let png = render_network(&network, &opts);
    std::fs::create_dir_all("fixtures").unwrap();
    std::fs::write("fixtures/golden_map.png", png).unwrap();
    println!("wrote fixtures/golden_map.png");
}
```

Run: `cd broker && cargo run --example gen_golden`
Expected: writes `fixtures/golden_map.png`. Open it; confirm it shows an L-shaped yellow road on a dark background (two connected segments).

> The example binary must be kept in `fixtures/golden_map.png`'s test (it's the documented regeneration path referenced in the failure message). Leave `examples/gen_golden.rs` in the repo.

- [ ] **Step 5: Run the golden test**

Run: `cd broker && cargo test render::`
Expected: both render tests pass.

- [ ] **Step 6: Commit**

```bash
git add broker/src/render.rs broker/src/lib.rs broker/examples/gen_golden.rs broker/fixtures/golden_map.png
git commit -m "feat: add schematic network renderer with golden test"
```

---

## Task 6: Mock mod (in-memory HTTP server)

An `axum` server implementing the full contract over an in-memory city. Used by every later test and by `skylinebench mock` for manual development. Builds roads with auto-incrementing ids and honest snapping so the broker's behaviour is exercised realistically.

**Files:**
- Create: `broker/src/mock.rs`
- Modify: `broker/src/lib.rs` (uncomment `pub mod mock;`)

- [ ] **Step 1: Write the mock state and router**

Create `broker/src/mock.rs`:

```rust
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tokio::net::TcpListener;

use crate::contract::*;
use crate::geometry::{horizontal_distance, SNAP_TOLERANCE_M};

#[derive(Default)]
struct City {
    nodes: Vec<NetNode>,
    segments: Vec<NetSegment>,
    next_id: u32,
    tick: u64,
    paused: bool,
    funds: i64,
}

#[derive(Clone)]
pub struct MockState {
    city: Arc<Mutex<City>>,
}

impl MockState {
    fn new() -> Self {
        MockState {
            city: Arc::new(Mutex::new(City { next_id: 1, funds: 100_000, paused: true, ..City::default() })),
        }
    }
}

fn road_types() -> Vec<String> {
    vec!["road".into(), "highway".into(), "oneway".into()]
}

fn zone_types() -> Vec<String> {
    vec!["residential".into(), "commercial".into(), "industrial".into(), "office".into()]
}

async fn health(State(s): State<MockState>) -> Json<Health> {
    let c = s.city.lock().unwrap();
    Json(Health {
        mod_version: "mock-0.1.0".into(),
        game_version: "mock".into(),
        city_loaded: true,
        paused: c.paused,
        tick: c.tick,
    })
}

async fn network(State(s): State<MockState>) -> Json<Network> {
    let c = s.city.lock().unwrap();
    Json(Network { nodes: c.nodes.clone(), segments: c.segments.clone() })
}

async fn buildings(State(_s): State<MockState>) -> Json<Buildings> {
    Json(Buildings { buildings: vec![] })
}

async fn zones(State(_s): State<MockState>) -> Json<Zones> {
    Json(Zones { cells: vec![] })
}

async fn metrics(State(s): State<MockState>) -> Json<Metrics> {
    let c = s.city.lock().unwrap();
    // Mock heuristic: flow degrades as the network grows, so the broker's
    // observe→build→step→observe loop sees a real, deterministic metric change.
    let flow = (100.0 - c.segments.len() as f32 * 5.0).max(0.0);
    Json(Metrics {
        tick: c.tick,
        traffic: TrafficMetrics {
            flow_percent: flow,
            active_vehicles: c.segments.len() as u32 * 10,
            segment_loads: c.segments.iter().map(|sg| SegmentLoad { segment_id: sg.id, density: 0.5 }).collect(),
        },
        economy: EconomyMetrics { balance: 0, weekly_income: 1000, weekly_expenses: 800, funds: c.funds },
        population: PopulationMetrics { total: 1000, residential_demand: 50, commercial_demand: 40, industrial_demand: 30, office_demand: 20, employed: 700 },
        services: ServiceMetrics { happiness: 75 },
    })
}

async fn road_types_ep() -> Json<RoadTypes> {
    Json(RoadTypes { road_types: road_types() })
}

async fn zone_types_ep() -> Json<ZoneTypes> {
    Json(ZoneTypes { zone_types: zone_types() })
}

#[derive(Deserialize)]
struct BuildRoadBody {
    start: Position,
    end: Position,
    prefab: String,
    snap_to_existing_nodes: bool,
}

async fn build_road(State(s): State<MockState>, Json(body): Json<BuildRoadBody>) -> Json<ActionResult> {
    let mut c = s.city.lock().unwrap();
    if !road_types().contains(&body.prefab) {
        return Json(ActionResult { ok: false, created_nodes: vec![], created_segments: vec![], snapped_nodes: vec![], destroyed: vec![], reason: Some(ActionError::InvalidPrefab) });
    }

    let mut created_nodes = vec![];
    let mut snapped_nodes = vec![];

    let mut resolve = |p: Position, c: &mut City| -> u32 {
        if body.snap_to_existing_nodes {
            if let Some(existing) = c
                .nodes
                .iter()
                .filter(|n| horizontal_distance(p, Position { x: n.x, y: n.y, z: n.z }) <= SNAP_TOLERANCE_M)
                .min_by(|a, b| {
                    horizontal_distance(p, Position { x: a.x, y: a.y, z: a.z })
                        .partial_cmp(&horizontal_distance(p, Position { x: b.x, y: b.y, z: b.z }))
                        .unwrap()
                })
            {
                let id = existing.id;
                snapped_nodes.push(id);
                return id;
            }
        }
        let id = c.next_id;
        c.next_id += 1;
        c.nodes.push(NetNode { id, x: p.x, y: p.y, z: p.z });
        created_nodes.push(id);
        id
    };

    let start_id = resolve(body.start, &mut c);
    let end_id = resolve(body.end, &mut c);

    let seg_id = c.next_id;
    c.next_id += 1;
    let length = horizontal_distance(body.start, body.end);
    c.segments.push(NetSegment { id: seg_id, start_node: start_id, end_node: end_id, prefab: body.prefab, lanes: 2, length });

    Json(ActionResult { ok: true, created_nodes, created_segments: vec![seg_id], snapped_nodes, destroyed: vec![], reason: None })
}

#[derive(Deserialize)]
struct ClockBody {
    op: String,
    ticks: Option<u32>,
    #[allow(dead_code)]
    speed: Option<u8>,
}

async fn clock(State(s): State<MockState>, Json(body): Json<ClockBody>) -> Json<ClockState> {
    let mut c = s.city.lock().unwrap();
    match body.op.as_str() {
        "pause" => c.paused = true,
        "resume" => c.paused = false,
        "step" => c.tick += body.ticks.unwrap_or(0) as u64,
        _ => {}
    }
    Json(ClockState { ok: true, paused: c.paused, tick: c.tick })
}

pub fn router() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/network", get(network))
        .route("/buildings", get(buildings))
        .route("/zones", get(zones))
        .route("/metrics", get(metrics))
        .route("/road-types", get(road_types_ep))
        .route("/zone-types", get(zone_types_ep))
        .route("/action/build-road", post(build_road))
        .route("/clock", post(clock))
        .with_state(MockState::new())
}

/// Bind to `addr` (use port 0 for an ephemeral port) and return the actual
/// bound address plus a future that serves until the process ends.
pub async fn bind(addr: SocketAddr) -> (SocketAddr, impl std::future::Future<Output = ()>) {
    let listener = TcpListener::bind(addr).await.expect("bind mock");
    let local = listener.local_addr().unwrap();
    let fut = async move {
        axum::serve(listener, router()).await.unwrap();
    };
    (local, fut)
}
```

- [ ] **Step 2: Write a test that the mock serves health**

Append to `broker/src/mock.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_serves_health() {
        let (addr, server) = bind("127.0.0.1:0".parse().unwrap()).await;
        tokio::spawn(server);
        let url = format!("http://{addr}/health");
        let resp: Health = reqwest::get(&url).await.unwrap().json().await.unwrap();
        assert!(resp.city_loaded);
        assert!(resp.paused);
    }

    #[tokio::test]
    async fn build_then_metrics_changes_flow() {
        let (addr, server) = bind("127.0.0.1:0".parse().unwrap()).await;
        tokio::spawn(server);
        let client = reqwest::Client::new();

        let before: Metrics = client.get(format!("http://{addr}/metrics")).send().await.unwrap().json().await.unwrap();

        let body = serde_json::json!({
            "start": {"x": 0.0, "y": 0.0, "z": 0.0},
            "end": {"x": 50.0, "y": 0.0, "z": 0.0},
            "prefab": "road",
            "snap_to_existing_nodes": true
        });
        let res: ActionResult = client.post(format!("http://{addr}/action/build-road")).json(&body).send().await.unwrap().json().await.unwrap();
        assert!(res.ok);
        assert_eq!(res.created_segments.len(), 1);

        let after: Metrics = client.get(format!("http://{addr}/metrics")).send().await.unwrap().json().await.unwrap();
        assert!(after.traffic.flow_percent < before.traffic.flow_percent);
    }
}
```

- [ ] **Step 3: Uncomment the module**

In `broker/src/lib.rs`, uncomment `pub mod mock;`. Add `reqwest` to `[dev-dependencies]` if cargo complains it's not available in tests — it's already a normal dependency, so it is.

- [ ] **Step 4: Run tests**

Run: `cd broker && cargo test mock::`
Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add broker/src/mock.rs broker/src/lib.rs
git commit -m "feat: add in-memory mock mod implementing the contract"
```

---

## Task 7: Bridge client

Typed async HTTP client wrapping the contract endpoints. The only outbound-I/O module.

**Files:**
- Create: `broker/src/bridge_client.rs`
- Modify: `broker/src/lib.rs` (uncomment `pub mod bridge_client;`)

- [ ] **Step 1: Write the client**

Create `broker/src/bridge_client.rs`:

```rust
use serde::Serialize;

use crate::contract::*;

#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
}

pub struct BridgeClient {
    base: String,
    http: reqwest::Client,
}

#[derive(Serialize)]
struct BuildRoadBody<'a> {
    start: Position,
    end: Position,
    prefab: &'a str,
    snap_to_existing_nodes: bool,
}

#[derive(Serialize)]
struct ClockBody<'a> {
    op: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    ticks: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    speed: Option<u8>,
}

impl BridgeClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        BridgeClient { base: base_url.into(), http: reqwest::Client::new() }
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, BridgeError> {
        Ok(self.http.get(format!("{}{path}", self.base)).send().await?.error_for_status()?.json().await?)
    }

    pub async fn health(&self) -> Result<Health, BridgeError> {
        self.get_json("/health").await
    }

    pub async fn network(&self) -> Result<Network, BridgeError> {
        self.get_json("/network").await
    }

    pub async fn buildings(&self) -> Result<Buildings, BridgeError> {
        self.get_json("/buildings").await
    }

    pub async fn zones(&self) -> Result<Zones, BridgeError> {
        self.get_json("/zones").await
    }

    pub async fn metrics(&self) -> Result<Metrics, BridgeError> {
        self.get_json("/metrics").await
    }

    pub async fn road_types(&self) -> Result<RoadTypes, BridgeError> {
        self.get_json("/road-types").await
    }

    pub async fn zone_types(&self) -> Result<ZoneTypes, BridgeError> {
        self.get_json("/zone-types").await
    }

    pub async fn build_road(&self, start: Position, end: Position, prefab: &str, snap: bool) -> Result<ActionResult, BridgeError> {
        let body = BuildRoadBody { start, end, prefab, snap_to_existing_nodes: snap };
        Ok(self.http.post(format!("{}/action/build-road", self.base)).json(&body).send().await?.error_for_status()?.json().await?)
    }

    pub async fn clock(&self, op: &str, ticks: Option<u32>, speed: Option<u8>) -> Result<ClockState, BridgeError> {
        let body = ClockBody { op, ticks, speed };
        Ok(self.http.post(format!("{}/clock", self.base)).json(&body).send().await?.error_for_status()?.json().await?)
    }
}
```

- [ ] **Step 2: Write tests against the mock**

Append to `broker/src/bridge_client.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock;

    async fn start_mock() -> String {
        let (addr, server) = mock::bind("127.0.0.1:0".parse().unwrap()).await;
        tokio::spawn(server);
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn fetches_health() {
        let client = BridgeClient::new(start_mock().await);
        let h = client.health().await.unwrap();
        assert!(h.city_loaded);
    }

    #[tokio::test]
    async fn builds_a_road_and_sees_it_in_network() {
        let client = BridgeClient::new(start_mock().await);
        let res = client
            .build_road(Position { x: 0.0, y: 0.0, z: 0.0 }, Position { x: 50.0, y: 0.0, z: 0.0 }, "road", true)
            .await
            .unwrap();
        assert!(res.ok);
        let net = client.network().await.unwrap();
        assert_eq!(net.segments.len(), 1);
        assert_eq!(net.nodes.len(), 2);
    }

    #[tokio::test]
    async fn rejects_invalid_prefab_with_reason() {
        let client = BridgeClient::new(start_mock().await);
        let res = client
            .build_road(Position { x: 0.0, y: 0.0, z: 0.0 }, Position { x: 50.0, y: 0.0, z: 0.0 }, "monorail", true)
            .await
            .unwrap();
        assert!(!res.ok);
        assert_eq!(res.reason, Some(ActionError::InvalidPrefab));
    }
}
```

- [ ] **Step 3: Uncomment the module**

In `broker/src/lib.rs`, uncomment `pub mod bridge_client;`.

- [ ] **Step 4: Run tests**

Run: `cd broker && cargo test bridge_client::`
Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add broker/src/bridge_client.rs broker/src/lib.rs
git commit -m "feat: add typed HTTP bridge client"
```

---

## Task 8: Service layer (tool logic)

The actual logic behind each MCP tool, as plain async functions that take `&BridgeClient` + typed args and return `serde_json::Value`. Fully testable against the mock; the rmcp layer (Task 9) is a thin wrapper over this.

**Files:**
- Create: `broker/src/service.rs`
- Modify: `broker/src/lib.rs` (uncomment `pub mod service;`)

- [ ] **Step 1: Write the service functions**

Create `broker/src/service.rs`:

```rust
use serde::Deserialize;
use serde_json::{json, Value};

use crate::bridge_client::{BridgeClient, BridgeError};
use crate::contract::{ActionError, Bounds, Position};
use crate::geometry::playable_bounds;
use crate::graph::build_connectivity;
use crate::render::{render_network, RenderOptions};
use crate::validate::validate_build_road;

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error(transparent)]
    Bridge(#[from] BridgeError),
}

pub async fn get_city_overview(client: &BridgeClient) -> Result<Value, ServiceError> {
    let health = client.health().await?;
    let metrics = client.metrics().await?;
    let net = client.network().await?;
    Ok(json!({
        "tick": health.tick,
        "paused": health.paused,
        "population": metrics.population.total,
        "funds": metrics.economy.funds,
        "traffic_flow_percent": metrics.traffic.flow_percent,
        "node_count": net.nodes.len(),
        "segment_count": net.segments.len(),
    }))
}

pub async fn observe_area(client: &BridgeClient) -> Result<Value, ServiceError> {
    let net = client.network().await?;
    let buildings = client.buildings().await?;
    let zones = client.zones().await?;
    let connectivity = build_connectivity(&net);
    Ok(json!({
        "network": net,
        "buildings": buildings.buildings,
        "zones": zones.cells,
        "intersections": connectivity.intersections(),
        "dead_ends": connectivity.dead_ends(),
    }))
}

#[derive(Deserialize)]
pub struct GetMetricsArgs {
    /// Optional subset of groups: "traffic","economy","population","services".
    #[serde(default)]
    pub groups: Vec<String>,
}

pub async fn get_metrics(client: &BridgeClient, args: GetMetricsArgs) -> Result<Value, ServiceError> {
    let m = client.metrics().await?;
    let want = |g: &str| args.groups.is_empty() || args.groups.iter().any(|x| x == g);
    let mut out = json!({ "tick": m.tick });
    if want("traffic") {
        out["traffic"] = serde_json::to_value(&m.traffic).unwrap();
    }
    if want("economy") {
        out["economy"] = serde_json::to_value(&m.economy).unwrap();
    }
    if want("population") {
        out["population"] = serde_json::to_value(&m.population).unwrap();
    }
    if want("services") {
        out["services"] = serde_json::to_value(&m.services).unwrap();
    }
    Ok(out)
}

#[derive(Deserialize)]
pub struct RenderMapArgs {
    #[serde(default)]
    pub bounds: Option<Bounds>,
    #[serde(default = "default_size")]
    pub width_px: u32,
    #[serde(default = "default_size")]
    pub height_px: u32,
}

fn default_size() -> u32 {
    512
}

/// Returns the rendered PNG bytes (the rmcp layer wraps these as an image
/// content block).
pub async fn render_map(client: &BridgeClient, args: RenderMapArgs) -> Result<Vec<u8>, ServiceError> {
    let net = client.network().await?;
    let opts = RenderOptions {
        bounds: args.bounds.unwrap_or_else(playable_bounds),
        width_px: args.width_px,
        height_px: args.height_px,
    };
    Ok(render_network(&net, &opts))
}

#[derive(Deserialize)]
pub struct BuildRoadArgs {
    pub from: Position,
    pub to: Position,
    pub road_type: String,
    #[serde(default = "default_true")]
    pub snap: bool,
}

fn default_true() -> bool {
    true
}

pub async fn build_road(client: &BridgeClient, args: BuildRoadArgs) -> Result<Value, ServiceError> {
    let road_types = client.road_types().await?.road_types;
    if let Err(reason) = validate_build_road(args.from, args.to, &args.road_type, &road_types) {
        return Ok(action_error_value(reason));
    }
    let res = client.build_road(args.from, args.to, &args.road_type, args.snap).await?;
    Ok(serde_json::to_value(res).unwrap())
}

pub async fn list_road_types(client: &BridgeClient) -> Result<Value, ServiceError> {
    Ok(json!({ "road_types": client.road_types().await?.road_types }))
}

pub async fn list_zone_types(client: &BridgeClient) -> Result<Value, ServiceError> {
    Ok(json!({ "zone_types": client.zone_types().await?.zone_types }))
}

#[derive(Deserialize)]
pub struct ControlTimeArgs {
    pub op: String,
    #[serde(default)]
    pub ticks: Option<u32>,
    #[serde(default)]
    pub speed: Option<u8>,
}

pub async fn control_time(client: &BridgeClient, args: ControlTimeArgs) -> Result<Value, ServiceError> {
    let state = client.clock(&args.op, args.ticks, args.speed).await?;
    Ok(serde_json::to_value(state).unwrap())
}

fn action_error_value(reason: ActionError) -> Value {
    json!({ "ok": false, "reason": reason })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock;

    async fn client() -> BridgeClient {
        let (addr, server) = mock::bind("127.0.0.1:0".parse().unwrap()).await;
        tokio::spawn(server);
        BridgeClient::new(format!("http://{addr}"))
    }

    #[tokio::test]
    async fn overview_reports_empty_city() {
        let c = client().await;
        let v = get_city_overview(&c).await.unwrap();
        assert_eq!(v["segment_count"], 0);
        assert_eq!(v["traffic_flow_percent"], 100.0);
    }

    #[tokio::test]
    async fn get_metrics_filters_groups() {
        let c = client().await;
        let v = get_metrics(&c, GetMetricsArgs { groups: vec!["traffic".into()] }).await.unwrap();
        assert!(v.get("traffic").is_some());
        assert!(v.get("economy").is_none());
    }

    #[tokio::test]
    async fn build_road_rejects_unknown_type_before_hitting_mod() {
        let c = client().await;
        let v = build_road(&c, BuildRoadArgs {
            from: Position { x: 0.0, y: 0.0, z: 0.0 },
            to: Position { x: 50.0, y: 0.0, z: 0.0 },
            road_type: "teleporter".into(),
            snap: true,
        }).await.unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["reason"], "INVALID_PREFAB");
    }

    #[tokio::test]
    async fn build_road_succeeds_and_observe_sees_it() {
        let c = client().await;
        let built = build_road(&c, BuildRoadArgs {
            from: Position { x: 0.0, y: 0.0, z: 0.0 },
            to: Position { x: 50.0, y: 0.0, z: 0.0 },
            road_type: "road".into(),
            snap: true,
        }).await.unwrap();
        assert_eq!(built["ok"], true);
        let obs = observe_area(&c).await.unwrap();
        assert_eq!(obs["network"]["segments"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn render_map_returns_png_bytes() {
        let c = client().await;
        let png = render_map(&c, RenderMapArgs { bounds: None, width_px: 64, height_px: 64 }).await.unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }
}
```

- [ ] **Step 2: Uncomment the module**

In `broker/src/lib.rs`, uncomment `pub mod service;`.

- [ ] **Step 3: Run tests**

Run: `cd broker && cargo test service::`
Expected: 5 tests pass.

- [ ] **Step 4: Commit**

```bash
git add broker/src/service.rs broker/src/lib.rs
git commit -m "feat: add service layer for all MCP tools"
```

---

## Task 9: rmcp tool adapter

> **Read first:** Pin the exact `rmcp` version now (`cargo add rmcp@<latest 0.x> --features server,transport-io`) and open its server example on docs.rs. The macro attribute syntax below matches the `0.1`-era `#[tool]`/`#[tool(tool_box)]` API; if the pinned version differs, adapt the attributes/trait-impl to its example while keeping the structure (one thin method per tool, each delegating to `service`). Do not change the `service` layer.

**Files:**
- Create: `broker/src/tools.rs`
- Modify: `broker/src/lib.rs` (uncomment `pub mod tools;`)

- [ ] **Step 1: Write the adapter**

Create `broker/src/tools.rs`:

```rust
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, ServerHandler};

use crate::bridge_client::BridgeClient;
use crate::service;

#[derive(Clone)]
pub struct Skyline {
    client: std::sync::Arc<BridgeClient>,
}

impl Skyline {
    pub fn new(base_url: impl Into<String>) -> Self {
        Skyline { client: std::sync::Arc::new(BridgeClient::new(base_url)) }
    }

    fn json_result(value: serde_json::Value) -> CallToolResult {
        CallToolResult::success(vec![Content::text(value.to_string())])
    }
}

#[tool(tool_box)]
impl Skyline {
    #[tool(description = "High-level city summary: tick, population, funds, traffic flow, network size.")]
    async fn get_city_overview(&self) -> CallToolResult {
        match service::get_city_overview(&self.client).await {
            Ok(v) => Self::json_result(v),
            Err(e) => CallToolResult::error(vec![Content::text(e.to_string())]),
        }
    }

    #[tool(description = "Structured network graph (nodes, segments, connectivity), buildings, and zones for the current city.")]
    async fn observe_area(&self) -> CallToolResult {
        match service::observe_area(&self.client).await {
            Ok(v) => Self::json_result(v),
            Err(e) => CallToolResult::error(vec![Content::text(e.to_string())]),
        }
    }

    #[tool(description = "Top-down schematic PNG of the road network. Returns a PNG image.")]
    async fn render_map(&self, #[tool(aggr)] args: service::RenderMapArgs) -> CallToolResult {
        match service::render_map(&self.client, args).await {
            Ok(png) => CallToolResult::success(vec![Content::image(
                rmcp::base64::engine::general_purpose::STANDARD.encode(&png),
                "image/png".to_string(),
            )]),
            Err(e) => CallToolResult::error(vec![Content::text(e.to_string())]),
        }
    }

    #[tool(description = "City metrics. Optionally filter to groups: traffic, economy, population, services.")]
    async fn get_metrics(&self, #[tool(aggr)] args: service::GetMetricsArgs) -> CallToolResult {
        match service::get_metrics(&self.client, args).await {
            Ok(v) => Self::json_result(v),
            Err(e) => CallToolResult::error(vec![Content::text(e.to_string())]),
        }
    }

    #[tool(description = "Build a straight road segment between two world positions. Validates prefab/bounds/length first.")]
    async fn build_road(&self, #[tool(aggr)] args: service::BuildRoadArgs) -> CallToolResult {
        match service::build_road(&self.client, args).await {
            Ok(v) => Self::json_result(v),
            Err(e) => CallToolResult::error(vec![Content::text(e.to_string())]),
        }
    }

    #[tool(description = "List valid road type identifiers accepted by build_road.")]
    async fn list_road_types(&self) -> CallToolResult {
        match service::list_road_types(&self.client).await {
            Ok(v) => Self::json_result(v),
            Err(e) => CallToolResult::error(vec![Content::text(e.to_string())]),
        }
    }

    #[tool(description = "List valid zone type identifiers.")]
    async fn list_zone_types(&self) -> CallToolResult {
        match service::list_zone_types(&self.client).await {
            Ok(v) => Self::json_result(v),
            Err(e) => CallToolResult::error(vec![Content::text(e.to_string())]),
        }
    }

    #[tool(description = "Control simulation time. op = pause | resume | set-speed | step. step uses ticks; set-speed uses speed.")]
    async fn control_time(&self, #[tool(aggr)] args: service::ControlTimeArgs) -> CallToolResult {
        match service::control_time(&self.client, args).await {
            Ok(v) => Self::json_result(v),
            Err(e) => CallToolResult::error(vec![Content::text(e.to_string())]),
        }
    }
}

#[tool(tool_box)]
impl ServerHandler for Skyline {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo {
            instructions: Some("Play Cities: Skylines 1: observe the city, control time, and build/modify the road network.".into()),
            ..Default::default()
        }
    }
}
```

> Remove any imports the macro doesn't need, and add any it does, per the pinned rmcp version's example.

- [ ] **Step 2: Uncomment the module and build**

In `broker/src/lib.rs`, uncomment `pub mod tools;`.

Run: `cd broker && cargo build`
Expected: compiles. If rmcp macro/import names differ in your pinned version, fix imports/attributes per its example until it compiles. The `service` layer must remain untouched.

- [ ] **Step 3: Commit**

```bash
git add broker/src/tools.rs broker/src/lib.rs broker/Cargo.toml broker/Cargo.lock
git commit -m "feat: add rmcp tool adapter over the service layer"
```

---

## Task 10: CLI wiring (`serve` and `mock`)

**Files:**
- Modify: `broker/src/main.rs`

- [ ] **Step 1: Write the CLI**

Replace `broker/src/main.rs` with:

```rust
use clap::{Parser, Subcommand};

use skylinebench::mock;
use skylinebench::tools::Skyline;

#[derive(Parser)]
#[command(name = "skylinebench", about = "Cities: Skylines 1 MCP harness (broker)")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the MCP server over stdio, talking to the mod at --mod-url.
    Serve {
        #[arg(long, default_value = "http://127.0.0.1:8787")]
        mod_url: String,
    },
    /// Run the in-memory mock mod (for development/testing) on --addr.
    Mock {
        #[arg(long, default_value = "127.0.0.1:8787")]
        addr: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Mock { addr } => {
            let (bound, server) = mock::bind(addr.parse()?).await;
            eprintln!("mock mod listening on http://{bound}");
            server.await;
        }
        Command::Serve { mod_url } => {
            // rmcp stdio server bootstrap. Match this to the pinned rmcp version's
            // server example; the shape is: build the handler, serve over stdio.
            use rmcp::transport::io::stdio;
            use rmcp::ServiceExt;
            let handler = Skyline::new(mod_url);
            let service = handler.serve(stdio()).await?;
            service.waiting().await?;
        }
    }
    Ok(())
}
```

> The `Serve` arm's exact rmcp bootstrap (`stdio()`, `serve`, `waiting`) matches the `0.1`-era API. Adapt to the pinned version's example if names differ.

- [ ] **Step 2: Build**

Run: `cd broker && cargo build`
Expected: compiles.

- [ ] **Step 3: Manually verify the mock subcommand runs**

Run: `cd broker && (cargo run -- mock &) && sleep 2 && curl -s http://127.0.0.1:8787/health && pkill -f "skylinebench"`
Expected: prints a JSON health object with `"city_loaded":true`.

- [ ] **Step 4: Commit**

```bash
git add broker/src/main.rs
git commit -m "feat: wire up serve and mock CLI subcommands"
```

---

## Task 11: Remaining action & lifecycle verbs

Carry `bulldoze`, `upgrade_road`, `set_zoning`, and `reset_scenario` through every layer (mock → client → service → tools), matching the `build_road` pattern. Required by spec §10.

**Files:**
- Modify: `broker/src/contract.rs` (add `LoadResult`)
- Modify: `broker/src/mock.rs` (zone storage + 4 endpoints)
- Modify: `broker/src/bridge_client.rs` (4 methods)
- Modify: `broker/src/service.rs` (4 functions + tests)
- Modify: `broker/src/tools.rs` (4 tool methods)

- [ ] **Step 1: Add the `LoadResult` contract type**

In `broker/src/contract.rs`, add after `ClockState`:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadResult {
    pub ok: bool,
    pub city_loaded: bool,
}
```

- [ ] **Step 2: Extend the mock with zone storage and the four endpoints**

In `broker/src/mock.rs`, add a `zones` field to `City`:

```rust
#[derive(Default)]
struct City {
    nodes: Vec<NetNode>,
    segments: Vec<NetSegment>,
    zones: Vec<ZoneCell>,
    next_id: u32,
    tick: u64,
    paused: bool,
    funds: i64,
}
```

Replace the `zones` handler so it returns stored cells:

```rust
async fn zones(State(s): State<MockState>) -> Json<Zones> {
    let c = s.city.lock().unwrap();
    Json(Zones { cells: c.zones.clone() })
}
```

Add these handlers (after `build_road`):

```rust
#[derive(Deserialize)]
struct BulldozeBody {
    target_type: String,
    id: u32,
}

async fn bulldoze(State(s): State<MockState>, Json(body): Json<BulldozeBody>) -> Json<ActionResult> {
    let mut c = s.city.lock().unwrap();
    let removed = match body.target_type.as_str() {
        "segment" => {
            let before = c.segments.len();
            c.segments.retain(|sg| sg.id != body.id);
            before != c.segments.len()
        }
        "node" => {
            let before = c.nodes.len();
            c.nodes.retain(|n| n.id != body.id);
            before != c.nodes.len()
        }
        _ => false,
    };
    if removed {
        Json(ActionResult { ok: true, created_nodes: vec![], created_segments: vec![], snapped_nodes: vec![], destroyed: vec![body.id], reason: None })
    } else {
        Json(ActionResult { ok: false, created_nodes: vec![], created_segments: vec![], snapped_nodes: vec![], destroyed: vec![], reason: Some(ActionError::InvalidArgs) })
    }
}

#[derive(Deserialize)]
struct UpgradeBody {
    segment_id: u32,
    prefab: String,
}

async fn upgrade_road(State(s): State<MockState>, Json(body): Json<UpgradeBody>) -> Json<ActionResult> {
    let mut c = s.city.lock().unwrap();
    if !road_types().contains(&body.prefab) {
        return Json(ActionResult { ok: false, created_nodes: vec![], created_segments: vec![], snapped_nodes: vec![], destroyed: vec![], reason: Some(ActionError::InvalidPrefab) });
    }
    match c.segments.iter_mut().find(|sg| sg.id == body.segment_id) {
        Some(sg) => {
            sg.prefab = body.prefab;
            Json(ActionResult { ok: true, created_nodes: vec![], created_segments: vec![body.segment_id], snapped_nodes: vec![], destroyed: vec![], reason: None })
        }
        None => Json(ActionResult { ok: false, created_nodes: vec![], created_segments: vec![], snapped_nodes: vec![], destroyed: vec![], reason: Some(ActionError::InvalidArgs) }),
    }
}

#[derive(Deserialize)]
struct SetZoneBody {
    rect: Bounds,
    zone_type: String,
}

async fn set_zone(State(s): State<MockState>, Json(body): Json<SetZoneBody>) -> Json<ActionResult> {
    let mut c = s.city.lock().unwrap();
    if !zone_types().contains(&body.zone_type) {
        return Json(ActionResult { ok: false, created_nodes: vec![], created_segments: vec![], snapped_nodes: vec![], destroyed: vec![], reason: Some(ActionError::InvalidArgs) });
    }
    // One representative cell at the rect centre keeps the mock simple but observable.
    c.zones.push(ZoneCell {
        x: (body.rect.min_x + body.rect.max_x) / 2.0,
        z: (body.rect.min_z + body.rect.max_z) / 2.0,
        zone_type: body.zone_type,
    });
    Json(ActionResult { ok: true, created_nodes: vec![], created_segments: vec![], snapped_nodes: vec![], destroyed: vec![], reason: None })
}

#[derive(Deserialize)]
struct LoadSaveBody {
    #[allow(dead_code)]
    save_name: String,
}

async fn load_save(State(s): State<MockState>, Json(_body): Json<LoadSaveBody>) -> Json<LoadResult> {
    let mut c = s.city.lock().unwrap();
    c.nodes.clear();
    c.segments.clear();
    c.zones.clear();
    c.tick = 0;
    c.next_id = 1;
    Json(LoadResult { ok: true, city_loaded: true })
}
```

Register them in `router()`:

```rust
        .route("/action/bulldoze", post(bulldoze))
        .route("/action/upgrade-road", post(upgrade_road))
        .route("/action/set-zone", post(set_zone))
        .route("/load-save", post(load_save))
```

- [ ] **Step 3: Add client methods**

In `broker/src/bridge_client.rs`, add to `impl BridgeClient`:

```rust
    pub async fn bulldoze(&self, target_type: &str, id: u32) -> Result<ActionResult, BridgeError> {
        let body = serde_json::json!({ "target_type": target_type, "id": id });
        Ok(self.http.post(format!("{}/action/bulldoze", self.base)).json(&body).send().await?.error_for_status()?.json().await?)
    }

    pub async fn upgrade_road(&self, segment_id: u32, prefab: &str) -> Result<ActionResult, BridgeError> {
        let body = serde_json::json!({ "segment_id": segment_id, "prefab": prefab });
        Ok(self.http.post(format!("{}/action/upgrade-road", self.base)).json(&body).send().await?.error_for_status()?.json().await?)
    }

    pub async fn set_zone(&self, rect: Bounds, zone_type: &str) -> Result<ActionResult, BridgeError> {
        let body = serde_json::json!({ "rect": rect, "zone_type": zone_type });
        Ok(self.http.post(format!("{}/action/set-zone", self.base)).json(&body).send().await?.error_for_status()?.json().await?)
    }

    pub async fn load_save(&self, save_name: &str) -> Result<LoadResult, BridgeError> {
        let body = serde_json::json!({ "save_name": save_name });
        Ok(self.http.post(format!("{}/load-save", self.base)).json(&body).send().await?.error_for_status()?.json().await?)
    }
```

Add `Bounds` and `LoadResult` to the `use crate::contract::*;` glob — already covered by the glob import; no change needed.

- [ ] **Step 4: Add service functions**

In `broker/src/service.rs`, add (and import `crate::contract::Bounds` is already imported):

```rust
#[derive(Deserialize)]
pub struct BulldozeArgs {
    pub target_type: String,
    pub id: u32,
}

pub async fn bulldoze(client: &BridgeClient, args: BulldozeArgs) -> Result<Value, ServiceError> {
    if !matches!(args.target_type.as_str(), "segment" | "node" | "building") {
        return Ok(action_error_value(ActionError::InvalidArgs));
    }
    let res = client.bulldoze(&args.target_type, args.id).await?;
    Ok(serde_json::to_value(res).unwrap())
}

#[derive(Deserialize)]
pub struct UpgradeRoadArgs {
    pub segment: u32,
    pub road_type: String,
}

pub async fn upgrade_road(client: &BridgeClient, args: UpgradeRoadArgs) -> Result<Value, ServiceError> {
    let road_types = client.road_types().await?.road_types;
    if !road_types.iter().any(|t| *t == args.road_type) {
        return Ok(action_error_value(ActionError::InvalidPrefab));
    }
    let res = client.upgrade_road(args.segment, &args.road_type).await?;
    Ok(serde_json::to_value(res).unwrap())
}

#[derive(Deserialize)]
pub struct SetZoningArgs {
    pub area: Bounds,
    pub zone_type: String,
}

pub async fn set_zoning(client: &BridgeClient, args: SetZoningArgs) -> Result<Value, ServiceError> {
    let zone_types = client.zone_types().await?.zone_types;
    if !zone_types.iter().any(|t| *t == args.zone_type) {
        return Ok(action_error_value(ActionError::InvalidArgs));
    }
    let res = client.set_zone(args.area, &args.zone_type).await?;
    Ok(serde_json::to_value(res).unwrap())
}

#[derive(Deserialize)]
pub struct ResetScenarioArgs {
    pub save: String,
}

pub async fn reset_scenario(client: &BridgeClient, args: ResetScenarioArgs) -> Result<Value, ServiceError> {
    let res = client.load_save(&args.save).await?;
    Ok(serde_json::to_value(res).unwrap())
}
```

- [ ] **Step 5: Add service tests**

Append inside the existing `#[cfg(test)] mod tests` in `service.rs`:

```rust
    #[tokio::test]
    async fn bulldoze_removes_a_segment() {
        let c = client().await;
        let built = build_road(&c, BuildRoadArgs {
            from: Position { x: 0.0, y: 0.0, z: 0.0 },
            to: Position { x: 50.0, y: 0.0, z: 0.0 },
            road_type: "road".into(),
            snap: true,
        }).await.unwrap();
        let seg_id = built["created_segments"][0].as_u64().unwrap() as u32;
        let res = bulldoze(&c, BulldozeArgs { target_type: "segment".into(), id: seg_id }).await.unwrap();
        assert_eq!(res["ok"], true);
        let obs = observe_area(&c).await.unwrap();
        assert_eq!(obs["network"]["segments"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn set_zoning_rejects_unknown_zone() {
        let c = client().await;
        let res = set_zoning(&c, SetZoningArgs {
            area: crate::contract::Bounds { min_x: 0.0, min_z: 0.0, max_x: 10.0, max_z: 10.0 },
            zone_type: "spaceport".into(),
        }).await.unwrap();
        assert_eq!(res["ok"], false);
        assert_eq!(res["reason"], "INVALID_ARGS");
    }

    #[tokio::test]
    async fn reset_scenario_clears_the_city() {
        let c = client().await;
        build_road(&c, BuildRoadArgs {
            from: Position { x: 0.0, y: 0.0, z: 0.0 },
            to: Position { x: 50.0, y: 0.0, z: 0.0 },
            road_type: "road".into(),
            snap: true,
        }).await.unwrap();
        reset_scenario(&c, ResetScenarioArgs { save: "anything".into() }).await.unwrap();
        let obs = observe_area(&c).await.unwrap();
        assert_eq!(obs["network"]["segments"].as_array().unwrap().len(), 0);
    }
```

- [ ] **Step 6: Run the service tests**

Run: `cd broker && cargo test service::`
Expected: the new tests pass alongside the existing ones.

- [ ] **Step 7: Add the four tool methods**

In `broker/src/tools.rs`, add inside the `#[tool(tool_box)] impl Skyline` block:

```rust
    #[tool(description = "Remove a network segment, node, or building. target_type = segment | node | building.")]
    async fn bulldoze(&self, #[tool(aggr)] args: service::BulldozeArgs) -> CallToolResult {
        match service::bulldoze(&self.client, args).await {
            Ok(v) => Self::json_result(v),
            Err(e) => CallToolResult::error(vec![Content::text(e.to_string())]),
        }
    }

    #[tool(description = "Change an existing road segment's type. Validates the new road_type first.")]
    async fn upgrade_road(&self, #[tool(aggr)] args: service::UpgradeRoadArgs) -> CallToolResult {
        match service::upgrade_road(&self.client, args).await {
            Ok(v) => Self::json_result(v),
            Err(e) => CallToolResult::error(vec![Content::text(e.to_string())]),
        }
    }

    #[tool(description = "Set zoning over a rectangular area. zone_type from list_zone_types.")]
    async fn set_zoning(&self, #[tool(aggr)] args: service::SetZoningArgs) -> CallToolResult {
        match service::set_zoning(&self.client, args).await {
            Ok(v) => Self::json_result(v),
            Err(e) => CallToolResult::error(vec![Content::text(e.to_string())]),
        }
    }

    #[tool(description = "Reload a named savegame — the benchmark reset primitive.")]
    async fn reset_scenario(&self, #[tool(aggr)] args: service::ResetScenarioArgs) -> CallToolResult {
        match service::reset_scenario(&self.client, args).await {
            Ok(v) => Self::json_result(v),
            Err(e) => CallToolResult::error(vec![Content::text(e.to_string())]),
        }
    }
```

- [ ] **Step 8: Build and run the full suite**

Run: `cd broker && cargo build && cargo test`
Expected: compiles; all tests pass.

- [ ] **Step 9: Commit**

```bash
git add broker/src/contract.rs broker/src/mock.rs broker/src/bridge_client.rs broker/src/service.rs broker/src/tools.rs
git commit -m "feat: add bulldoze, upgrade_road, set_zoning, reset_scenario verbs"
```

---

## Task 12: End-to-end loop test + README

Prove the full observe → build → step → observe loop through the service layer against the mock, and document MCP registration.

**Files:**
- Create: `broker/tests/broker_e2e.rs`
- Create: `broker/README.md`

- [ ] **Step 1: Write the end-to-end test**

Create `broker/tests/broker_e2e.rs`:

```rust
use skylinebench::bridge_client::BridgeClient;
use skylinebench::contract::Position;
use skylinebench::mock;
use skylinebench::service::{self, BuildRoadArgs, ControlTimeArgs, GetMetricsArgs};

#[tokio::test]
async fn full_observe_build_step_observe_loop() {
    let (addr, server) = mock::bind("127.0.0.1:0".parse().unwrap()).await;
    tokio::spawn(server);
    let client = BridgeClient::new(format!("http://{addr}"));

    // Observe: empty city, full flow.
    let before = service::get_metrics(&client, GetMetricsArgs { groups: vec!["traffic".into()] }).await.unwrap();
    let flow_before = before["traffic"]["flow_percent"].as_f64().unwrap();

    // Act: build a road.
    let built = service::build_road(&client, BuildRoadArgs {
        from: Position { x: 0.0, y: 0.0, z: 0.0 },
        to: Position { x: 50.0, y: 0.0, z: 0.0 },
        road_type: "road".into(),
        snap: true,
    }).await.unwrap();
    assert_eq!(built["ok"], true);

    // Step the clock.
    let clock = service::control_time(&client, ControlTimeArgs { op: "step".into(), ticks: Some(256), speed: None }).await.unwrap();
    assert_eq!(clock["tick"], 256);

    // Observe again: the metric changed (proof the loop is real).
    let after = service::get_metrics(&client, GetMetricsArgs { groups: vec!["traffic".into()] }).await.unwrap();
    let flow_after = after["traffic"]["flow_percent"].as_f64().unwrap();
    assert!(flow_after < flow_before, "building a road should change traffic flow in the mock");

    // The built segment is observable.
    let obs = service::observe_area(&client).await.unwrap();
    assert_eq!(obs["network"]["segments"].as_array().unwrap().len(), 1);
}
```

- [ ] **Step 2: Run the e2e test**

Run: `cd broker && cargo test --test broker_e2e`
Expected: 1 test passes.

- [ ] **Step 3: Run the entire suite + clippy**

Run: `cd broker && cargo test && cargo clippy -- -D warnings`
Expected: all tests pass; clippy clean.

- [ ] **Step 4: Write the README**

Create `broker/README.md`:

```markdown
# skylinebench (broker)

The Rust MCP server for SkylineBench. Exposes Cities: Skylines 1 to AI agents.
Until the C# mod (separate plan) is installed, run against the built-in mock mod.

## Build

    cd broker
    cargo build --release

## Try it against the mock mod

Terminal 1 — start the mock city:

    cargo run -- mock            # listens on http://127.0.0.1:8787

Terminal 2 — point an MCP client at the server (see below). The server talks to
the mod URL given by `--mod-url` (default `http://127.0.0.1:8787`, i.e. the mock).

## Register with Claude Code

    claude mcp add skylinebench -- /absolute/path/to/broker/target/release/skylinebench serve

Or in `.mcp.json` / settings:

    {
      "mcpServers": {
        "skylinebench": {
          "command": "/absolute/path/to/broker/target/release/skylinebench",
          "args": ["serve", "--mod-url", "http://127.0.0.1:8787"]
        }
      }
    }

## Register with Codex

Add to your Codex MCP config:

    [mcp_servers.skylinebench]
    command = "/absolute/path/to/broker/target/release/skylinebench"
    args = ["serve", "--mod-url", "http://127.0.0.1:8787"]

## Tools

- Observe: `get_city_overview`, `observe_area`, `render_map`, `get_metrics`
- Act: `build_road`, `bulldoze`, `upgrade_road`, `set_zoning`
- Reference: `list_road_types`, `list_zone_types`
- Control: `control_time`, `reset_scenario`

## Tests

    cargo test                       # unit + integration, no game needed
    cargo run --example gen_golden   # regenerate the renderer golden image
```

- [ ] **Step 5: Commit**

```bash
git add broker/tests/broker_e2e.rs broker/README.md
git commit -m "test: add end-to-end loop test; docs: add broker README"
```

---

## Done criteria for this plan

- `cargo test` passes (contract, geometry, graph, validate, render+golden, mock, bridge_client, service, e2e).
- `cargo clippy -- -D warnings` is clean.
- `skylinebench mock` serves the contract; `skylinebench serve` starts an MCP server exposing all eight tools.
- An MCP client can connect to `serve` (pointed at `mock`) and run the full observe → build → step → observe loop.

This delivers the entire broker against the contract. The **next plan** implements the C# mod that satisfies the same contract, plus `setup`/`doctor` and the live smoke test — at which point `serve --mod-url` points at the real game instead of the mock, with no broker changes.

## Known simplifications (carried, not gaps)

- **Area-scoped observation is pass-through, not yet exercised at scale.** Spec §4 has `observe_area` take `center/radius|bounds` and the contract's `GET /network` supports a bounds filter for token discipline. Against the mock the city is tiny, so the broker requests the full network and `observe_area` takes no bounds arg yet. Wiring bounds query params through `bridge_client.network()` and the mock is a small, mechanical follow-up; do it when the real mod can return large networks (mod plan) so token-bounding is testable against realistic data.

## Notes for spec sync

- `ActionError` adds two broker-side pre-validation reasons (`DEGENERATE_SEGMENT`, `INVALID_ARGS`) beyond spec §5's six mod-side codes. Fold this into the spec on next review.
- The contract adds `GET /road-types` and `GET /zone-types` (implied by spec §4's `list_road_types`/`list_zone_types` tools but not enumerated in §3). The mod plan must implement these.
