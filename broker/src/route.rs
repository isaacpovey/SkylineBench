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
