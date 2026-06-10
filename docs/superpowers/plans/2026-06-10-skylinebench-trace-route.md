# Trace Route Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Depends on:** `2026-06-10-skylinebench-segment-observability.md` (`travel_direction` and `speed_limit` on `NetSegment`).

**Goal:** A free `trace_route` tool that answers "which way would traffic go from A to B?" — the question the agent could not answer when its $70k eastern link attracted zero traffic.

**Architecture:** Pure broker-side shortest-path in a new `broker/src/route.rs`: build a directed adjacency from the network (one-way segments contribute one arc, two-way contribute both), weight each arc by `length / max(speed_limit, 0.1)` (a travel-time proxy), run Dijkstra with integer milli-costs (deterministic, no float-ordering issues). `service::trace_route` snaps the requested positions to the nearest nodes and reports the node/segment path. It is an *estimate* — the game's pathfinder also weighs lane changes and congestion — and the tool says so in its response.

**Tech Stack:** Rust (std only: BinaryHeap, HashMap).

---

### Task 1: Directed routing core

**Files:**
- Create: `broker/src/route.rs`
- Modify: `broker/src/lib.rs` (add `pub mod route;`)

- [ ] **Step 1: Write the failing tests**

Create `broker/src/route.rs` starting with its test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{NetNode, NetSegment, Network};

    fn node(id: u32, x: f32, z: f32) -> NetNode {
        NetNode { id, x, y: 0.0, z }
    }

    fn seg(id: u32, a: u32, b: u32, dir: &str, speed: f32) -> NetSegment {
        NetSegment {
            id,
            start_node: a,
            end_node: b,
            prefab: "road".into(),
            lanes: 2,
            length: 100.0,
            one_way: dir != "both",
            travel_direction: dir.into(),
            speed_limit: speed,
        }
    }

    /// Triangle 1→2→3 with a one-way 1→3 shortcut:
    ///   seg 10: 1↔2 (both), seg 11: 2↔3 (both), seg 12: 1→3 (one-way, fast)
    fn network() -> Network {
        Network {
            nodes: vec![node(1, 0.0, 0.0), node(2, 100.0, 0.0), node(3, 100.0, 100.0)],
            segments: vec![
                seg(10, 1, 2, "both", 1.0),
                seg(11, 2, 3, "both", 1.0),
                seg(12, 1, 3, "start_to_end", 4.0),
            ],
        }
    }

    #[test]
    fn forward_route_takes_the_one_way_shortcut() {
        let r = shortest_route(&network(), 1, 3).expect("reachable");
        assert_eq!(r.segments, vec![12]);
        assert_eq!(r.nodes, vec![1, 3]);
        assert_eq!(r.length_m, 100.0);
    }

    #[test]
    fn reverse_route_cannot_use_the_one_way() {
        let r = shortest_route(&network(), 3, 1).expect("reachable via two-ways");
        assert_eq!(r.segments, vec![11, 10]);
        assert_eq!(r.nodes, vec![3, 2, 1]);
        assert_eq!(r.length_m, 200.0);
    }

    #[test]
    fn end_to_start_arc_points_backwards() {
        let mut net = network();
        net.segments[2].travel_direction = "end_to_start".into();
        // Now the shortcut runs 3→1: forward must take the long way…
        let fwd = shortest_route(&net, 1, 3).expect("reachable");
        assert_eq!(fwd.segments, vec![10, 11]);
        // …and reverse takes the shortcut.
        let rev = shortest_route(&net, 3, 1).expect("reachable");
        assert_eq!(rev.segments, vec![12]);
    }

    #[test]
    fn unreachable_returns_none() {
        let net = Network {
            nodes: vec![node(1, 0.0, 0.0), node(2, 100.0, 0.0), node(3, 500.0, 0.0)],
            segments: vec![seg(10, 1, 2, "both", 1.0)],
        };
        assert!(shortest_route(&net, 1, 3).is_none());
    }

    #[test]
    fn same_node_is_an_empty_route() {
        let r = shortest_route(&network(), 2, 2).expect("trivially reachable");
        assert!(r.segments.is_empty());
        assert_eq!(r.nodes, vec![2]);
        assert_eq!(r.length_m, 0.0);
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --manifest-path broker/Cargo.toml route::`
Expected: FAIL — `shortest_route` missing. (Add `pub mod route;` to `broker/src/lib.rs` next to the other modules so the file is in the tree.)

- [ ] **Step 3: Implement Dijkstra over directed arcs**

Above the tests in `broker/src/route.rs`:

```rust
//! Broker-side shortest-path estimation over the road network. This is an
//! approximation of the game's pathfinding (which also weighs lane changes
//! and congestion): arcs follow `travel_direction`, weighted by
//! length / speed_limit as a travel-time proxy.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use crate::contract::Network;

#[derive(Debug, Clone, Copy)]
struct Arc {
    to: u32,
    segment: u32,
    /// Travel-time proxy in integer milli-units, so the priority queue has a
    /// total order (f32 has none) and ties break deterministically.
    millicost: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub nodes: Vec<u32>,
    pub segments: Vec<u32>,
    pub length_m: f32,
}

fn arcs(network: &Network) -> HashMap<u32, Vec<Arc>> {
    network
        .segments
        .iter()
        .flat_map(|s| {
            let millicost = ((s.length / s.speed_limit.max(0.1)) * 1000.0) as u64;
            let fwd = (s.start_node, Arc { to: s.end_node, segment: s.id, millicost });
            let rev = (s.end_node, Arc { to: s.start_node, segment: s.id, millicost });
            match s.travel_direction.as_str() {
                "start_to_end" => vec![fwd],
                "end_to_start" => vec![rev],
                _ => vec![fwd, rev],
            }
        })
        .fold(HashMap::new(), |mut acc: HashMap<u32, Vec<Arc>>, (from, arc)| {
            acc.entry(from).or_default().push(arc);
            acc
        })
}

/// Cheapest directed route from `from` to `to`, or None when unreachable.
pub fn shortest_route(network: &Network, from: u32, to: u32) -> Option<Route> {
    let adjacency = arcs(network);
    let mut best: HashMap<u32, u64> = HashMap::from([(from, 0)]);
    let mut prev: HashMap<u32, (u32, u32)> = HashMap::new(); // node -> (prev node, via segment)
    let mut heap = BinaryHeap::from([Reverse((0u64, from))]);

    while let Some(Reverse((cost, node))) = heap.pop() {
        if node == to {
            break;
        }
        if best.get(&node).is_some_and(|b| *b < cost) {
            continue;
        }
        for arc in adjacency.get(&node).map(Vec::as_slice).unwrap_or(&[]) {
            let next = cost + arc.millicost;
            if best.get(&arc.to).is_none_or(|b| next < *b) {
                best.insert(arc.to, next);
                prev.insert(arc.to, (node, arc.segment));
                heap.push(Reverse((next, arc.to)));
            }
        }
    }

    if from != to && !prev.contains_key(&to) {
        return None;
    }
    let (rev_nodes, rev_segments) = std::iter::successors(Some((to, None::<u32>)), |(n, _)| {
        prev.get(n).map(|(p, seg)| (*p, Some(*seg)))
    })
    .fold((Vec::new(), Vec::new()), |(mut ns, mut ss), (n, seg)| {
        ns.push(n);
        if let Some(seg) = seg {
            ss.push(seg);
        }
        (ns, ss)
    });
    let nodes: Vec<u32> = rev_nodes.into_iter().rev().collect();
    let segments: Vec<u32> = rev_segments.into_iter().rev().collect();
    let seg_lengths: HashMap<u32, f32> = network.segments.iter().map(|s| (s.id, s.length)).collect();
    let length_m = segments.iter().filter_map(|id| seg_lengths.get(id)).sum();
    Some(Route { nodes, segments, length_m })
}
```

(If the toolchain predates `Option::is_none_or`/`is_some_and`, use `.map_or(true, …)`/`.map_or(false, …)`.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path broker/Cargo.toml route::`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add broker/src/route.rs broker/src/lib.rs
git commit -m "feat(broker): directed shortest-route core over the road network"
```

---

### Task 2: trace_route service + tool registration

**Files:**
- Modify: `broker/src/service.rs` (args + function + test)
- Modify: `broker/src/tools.rs` (register; tool-count list)
- Modify: `broker/src/benchmark/server.rs` (register; tool-count list)
- Modify: `benchmark/run.sh` (`ALLOWED` list)

- [ ] **Step 1: Write the failing service test**

Add to `broker/src/service.rs` tests:

```rust
    #[tokio::test]
    async fn trace_route_follows_the_network() {
        let c = client().await;
        // Two roads sharing a middle node at x=50 (snap tolerance joins them).
        for (x0, x1) in [(0.0_f32, 50.0_f32), (50.0, 100.0)] {
            build_road(
                &c,
                BuildRoadArgs {
                    from: Position { x: x0, y: 0.0, z: 0.0 },
                    to: Position { x: x1, y: 0.0, z: 0.0 },
                    road_type: "road".into(),
                    snap: true,
                },
            )
            .await
            .unwrap();
        }
        let v = trace_route(
            &c,
            TraceRouteArgs {
                from: Position { x: 2.0, y: 0.0, z: 0.0 },
                to: Position { x: 99.0, y: 0.0, z: 0.0 },
            },
        )
        .await
        .unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["reachable"], true);
        assert_eq!(v["segments"].as_array().unwrap().len(), 2);
        assert_eq!(v["total_length_m"].as_f64().unwrap().round(), 100.0);
        assert!(v["note"].as_str().unwrap().contains("estimate"));
    }

    #[tokio::test]
    async fn trace_route_reports_unreachable() {
        let c = client().await;
        for (x0, x1) in [(0.0_f32, 50.0_f32), (5000.0, 5050.0)] {
            build_road(
                &c,
                BuildRoadArgs {
                    from: Position { x: x0, y: 0.0, z: 0.0 },
                    to: Position { x: x1, y: 0.0, z: 0.0 },
                    road_type: "road".into(),
                    snap: true,
                },
            )
            .await
            .unwrap();
        }
        let v = trace_route(
            &c,
            TraceRouteArgs {
                from: Position { x: 0.0, y: 0.0, z: 0.0 },
                to: Position { x: 5050.0, y: 0.0, z: 0.0 },
            },
        )
        .await
        .unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["reachable"], false);
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --manifest-path broker/Cargo.toml trace_route`
Expected: FAIL — `trace_route` missing.

- [ ] **Step 3: Implement the service function**

Add to `broker/src/service.rs`:

```rust
#[derive(Deserialize, schemars::JsonSchema)]
pub struct TraceRouteArgs {
    pub from: Position,
    pub to: Position,
}

pub async fn trace_route(
    client: &BridgeClient,
    args: TraceRouteArgs,
) -> Result<Value, ServiceError> {
    let net = client.network().await?;
    let nearest = |p: Position| {
        net.nodes
            .iter()
            .map(|n| {
                (
                    n.id,
                    crate::geometry::horizontal_distance(p, Position { x: n.x, y: 0.0, z: n.z }),
                )
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    };
    let (Some((from_node, from_dist)), Some((to_node, to_dist))) =
        (nearest(args.from), nearest(args.to))
    else {
        return Ok(json!({ "ok": false, "reason": "EMPTY_NETWORK" }));
    };
    let route = crate::route::shortest_route(&net, from_node, to_node);
    let note = "broker-side estimate from segment lengths, speed limits and one-way directions; \
                the game's own pathfinding also weighs congestion and lane changes";
    Ok(match route {
        Some(r) => json!({
            "ok": true,
            "reachable": true,
            "from_node": from_node,
            "from_snap_distance_m": from_dist,
            "to_node": to_node,
            "to_snap_distance_m": to_dist,
            "nodes": r.nodes,
            "segments": r.segments,
            "total_length_m": r.length_m,
            "note": note,
        }),
        None => json!({
            "ok": true,
            "reachable": false,
            "from_node": from_node,
            "to_node": to_node,
            "note": "no directed path exists — check one-way directions and disconnected components",
        }),
    })
}
```

- [ ] **Step 4: Register in both servers**

`broker/src/tools.rs` (add `TraceRouteArgs` to the service imports):

```rust
    #[tool(description = "Estimate the route traffic would take between two positions \
        (snapped to nearest road nodes), honoring one-way directions and speed limits. \
        Free read — use it to check whether a new link will actually attract traffic.")]
    async fn trace_route(
        &self,
        Parameters(args): Parameters<TraceRouteArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match service::trace_route(&self.client, args).await {
            Ok(v) => json_result(v),
            Err(e) => Ok(tool_error(e)),
        }
    }
```

`broker/src/benchmark/server.rs` — same description, benchmark wrapper pattern:

```rust
    async fn trace_route(&self, Parameters(args): Parameters<TraceRouteArgs>) -> Result<CallToolResult, ErrorData> {
        self.ensure_baseline().await;
        match service::trace_route(&self.client, args).await {
            Ok(v) => self.finish(v).await,
            Err(e) => Ok(tool_err(e)),
        }
    }
```

Insert `"trace_route"` into both sorted tool-count test lists (between `"submit_solution"` and `"upgrade_road"` in the benchmark list; between `"set_zoning"` and `"upgrade_road"` in the standard list).

- [ ] **Step 5: Allow the tool in run.sh**

`benchmark/run.sh` — add `,mcp__skylinebench__trace_route` to `ALLOWED`.

- [ ] **Step 6: Run the suite, then commit**

Run: `cargo test --manifest-path broker/Cargo.toml`
Expected: ALL PASS.

```bash
git add broker/src/service.rs broker/src/tools.rs broker/src/benchmark/server.rs benchmark/run.sh
git commit -m "feat(broker): trace_route directed path estimation tool"
```
