use std::collections::BTreeMap;

use crate::contract::Network;

#[derive(Debug, Clone, PartialEq)]
pub struct Connectivity {
    /// node id -> ids of directly connected nodes (via a segment). A `BTreeMap`
    /// so iteration is key-sorted, giving `intersections`/`dead_ends`
    /// deterministic ascending output without a separate sort step.
    pub adjacency: BTreeMap<u32, Vec<u32>>,
}

impl Connectivity {
    pub fn degree(&self, node_id: u32) -> usize {
        self.adjacency.get(&node_id).map(|v| v.len()).unwrap_or(0)
    }

    /// Nodes with three or more connections — junctions.
    pub fn intersections(&self) -> Vec<u32> {
        self.adjacency
            .iter()
            .filter(|(_, neighbours)| neighbours.len() >= 3)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Nodes with exactly one connection — dead-ends.
    pub fn dead_ends(&self) -> Vec<u32> {
        self.adjacency
            .iter()
            .filter(|(_, neighbours)| neighbours.len() == 1)
            .map(|(id, _)| *id)
            .collect()
    }
}

pub fn build_connectivity(network: &Network) -> Connectivity {
    let adjacency =
        network
            .segments
            .iter()
            .fold(BTreeMap::<u32, Vec<u32>>::new(), |mut acc, seg| {
                acc.entry(seg.start_node).or_default().push(seg.end_node);
                acc.entry(seg.end_node).or_default().push(seg.start_node);
                acc
            });
    Connectivity { adjacency }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{NetNode, NetSegment};

    fn node(id: u32) -> NetNode {
        NetNode {
            id,
            x: id as f32,
            y: 0.0,
            z: 0.0,
        }
    }

    fn seg(id: u32, a: u32, b: u32) -> NetSegment {
        NetSegment {
            id,
            start_node: a,
            end_node: b,
            prefab: "road".into(),
            lanes: 2,
            length: 10.0,
        }
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
        let c = build_connectivity(&Network {
            nodes: vec![],
            segments: vec![],
        });
        assert!(c.intersections().is_empty());
        assert!(c.dead_ends().is_empty());
    }
}
