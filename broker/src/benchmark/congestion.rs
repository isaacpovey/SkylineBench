use std::collections::HashMap;

use crate::contract::{Network, SegmentLoad};

/// Densities arrive as f32; comparing them against an f64 threshold directly
/// would exclude segments sitting exactly at the threshold (0.7f32 widens to
/// ~0.69999999). Narrow the threshold to f32 first so "at threshold" counts.
/// The same narrowed compare applies to the window's mean path: means of
/// f32-derived samples at the threshold are exact, because the 24-bit-mantissa
/// summands fit easily in f64 with room to spare.
fn meets(density_f64: f64, threshold: f64) -> bool {
    density_f64 >= f64::from(threshold as f32)
}

/// Congested road-meters in one metrics sample: total length of segments at or
/// above the density threshold (spec 2026-06-10 §2.1). Used for the rolling
/// progress value; measurement windows use [`WindowAccum`] instead.
pub fn instant_congested_meters(loads: &[SegmentLoad], threshold: f64) -> f64 {
    loads
        .iter()
        .filter(|l| meets(f64::from(l.density), threshold))
        .map(|l| f64::from(l.length))
        .sum()
}

#[derive(Debug, Default)]
struct SegmentAccum {
    density_sum: f64,
    samples: u32,
    length: f64,
}

/// Accumulates per-segment densities across a measurement window so congested
/// meters are computed from each segment's MEAN density over the window, not
/// per-sample flickers. A segment absent from a sample (e.g. bulldozed) only
/// contributes the samples where it exists. A segment id released and reused
/// mid-window is conflated (last length wins) — acceptable because scoring
/// windows run after the agent phase, when the network is no longer changing.
#[derive(Debug, Default)]
pub struct WindowAccum {
    per_segment: HashMap<u32, SegmentAccum>,
}

impl WindowAccum {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, loads: &[SegmentLoad]) {
        for l in loads {
            let e = self.per_segment.entry(l.segment_id).or_default();
            e.density_sum += f64::from(l.density);
            e.samples += 1;
            e.length = f64::from(l.length);
        }
    }

    pub fn congested_meters(&self, threshold: f64) -> f64 {
        self.per_segment
            .values()
            .filter(|s| s.samples > 0 && meets(s.density_sum / f64::from(s.samples), threshold))
            .map(|s| s.length)
            .sum()
    }

    /// Mean density of one segment over the window, or None if it never
    /// appeared. Shared with the junction counter.
    pub fn mean_density(&self, segment_id: u32) -> Option<f64> {
        self.per_segment
            .get(&segment_id)
            .filter(|s| s.samples > 0)
            .map(|s| s.density_sum / f64::from(s.samples))
    }
}

/// Road-graph adjacency: node id -> incident segment ids. A node's degree is
/// the number of incident segments; the 200 m auto-split makes many degree-2
/// nodes that are not real intersections, so the counter filters on min degree.
#[derive(Debug, Default)]
pub struct Topology {
    incidence: HashMap<u32, Vec<u32>>,
}

impl Topology {
    pub fn from_network(net: &Network) -> Self {
        let incidence = net.segments.iter().fold(
            HashMap::<u32, Vec<u32>>::new(),
            |mut acc, s| {
                acc.entry(s.start_node).or_default().push(s.id);
                acc.entry(s.end_node).or_default().push(s.id);
                acc
            },
        );
        Self { incidence }
    }
}

/// Count congested junctions: nodes of degree >= `min_degree` with at least
/// `min_congested` incident segments at/above `threshold`. `density_of` returns
/// a segment's density (window-mean or latest sample), or None when unknown —
/// which counts as not congested.
pub fn congested_junctions(
    topo: &Topology,
    density_of: impl Fn(u32) -> Option<f64>,
    threshold: f64,
    min_degree: usize,
    min_congested: usize,
) -> u32 {
    topo.incidence
        .values()
        .filter(|segs| segs.len() >= min_degree)
        .filter(|segs| {
            segs.iter()
                .filter(|id| density_of(**id).is_some_and(|d| meets(d, threshold)))
                .count()
                >= min_congested
        })
        .count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{NetNode, NetSegment, Network};

    fn load(id: u32, density: f32, length: f32) -> SegmentLoad {
        SegmentLoad { segment_id: id, density, length }
    }

    #[test]
    fn instant_sums_lengths_at_or_above_threshold() {
        let loads = vec![load(1, 0.9, 100.0), load(2, 0.7, 50.0), load(3, 0.69, 999.0)];
        assert_eq!(instant_congested_meters(&loads, 0.7), 150.0);
    }

    #[test]
    fn instant_on_empty_is_zero() {
        assert_eq!(instant_congested_meters(&[], 0.7), 0.0);
    }

    #[test]
    fn window_uses_mean_density_per_segment() {
        let mut w = WindowAccum::new();
        // Segment 1 flickers above the threshold once but its mean is 0.6.
        w.push(&[load(1, 0.9, 100.0), load(2, 0.8, 40.0)]);
        w.push(&[load(1, 0.3, 100.0), load(2, 0.8, 40.0)]);
        assert_eq!(w.congested_meters(0.7), 40.0);
    }

    #[test]
    fn window_handles_segment_absent_from_some_samples() {
        let mut w = WindowAccum::new();
        w.push(&[load(7, 0.8, 60.0)]);
        w.push(&[]); // segment bulldozed mid-window
        assert_eq!(w.congested_meters(0.7), 60.0, "mean over existing samples only");
    }

    #[test]
    fn window_counts_segment_exactly_at_threshold() {
        let mut w = WindowAccum::new();
        w.push(&[load(1, 0.7, 25.0)]);
        w.push(&[load(1, 0.7, 25.0)]);
        assert_eq!(w.congested_meters(0.7), 25.0);
    }

    fn node(id: u32) -> NetNode { NetNode { id, x: 0.0, y: 0.0, z: 0.0 } }
    fn seg(id: u32, a: u32, b: u32) -> NetSegment {
        NetSegment {
            id, start_node: a, end_node: b, prefab: "road".into(), lanes: 2,
            length: 100.0, one_way: false, travel_direction: "both".into(), speed_limit: 1.0,
        }
    }

    #[test]
    fn junction_needs_degree_at_least_min_and_two_congested_approaches() {
        let net = Network {
            nodes: vec![node(1), node(2), node(3), node(4), node(5)],
            segments: vec![seg(10, 1, 3), seg(11, 1, 4), seg(12, 1, 5), seg(20, 2, 3)],
        };
        let topo = Topology::from_network(&net);
        let dense = |id: u32| match id { 10 | 11 => Some(0.9), _ => Some(0.2) };
        assert_eq!(congested_junctions(&topo, dense, 0.7, 3, 2), 1);
        let one = |id: u32| match id { 10 => Some(0.9), _ => Some(0.2) };
        assert_eq!(congested_junctions(&topo, one, 0.7, 3, 2), 0);
    }

    #[test]
    fn degree_two_node_is_never_a_junction_even_if_both_congested() {
        let net = Network { nodes: vec![node(2), node(3), node(4)], segments: vec![seg(20, 2, 3), seg(21, 2, 4)] };
        let topo = Topology::from_network(&net);
        assert_eq!(congested_junctions(&topo, |_| Some(0.9), 0.7, 3, 2), 0);
    }

    #[test]
    fn missing_density_counts_as_not_congested() {
        // Node 1 has degree 3. Only segment 10 is congested; 11 is below
        // threshold and 12 has no density. With min_congested=2 the count is 1
        // (not a junction) — but only if a missing density is treated as
        // not-congested. If None counted as congested it would be 2 → 1 junction.
        let net = Network { nodes: vec![node(1), node(3), node(4), node(5)], segments: vec![seg(10, 1, 3), seg(11, 1, 4), seg(12, 1, 5)] };
        let topo = Topology::from_network(&net);
        let dense = |id: u32| match id { 10 => Some(0.9), 11 => Some(0.2), _ => None };
        assert_eq!(congested_junctions(&topo, dense, 0.7, 3, 2), 0);
    }

    #[test]
    fn window_mean_density_reads_back_per_segment() {
        let mut w = WindowAccum::new();
        w.push(&[load(1, 0.8, 100.0)]);
        w.push(&[load(1, 0.6, 100.0)]);
        assert!((w.mean_density(1).unwrap() - 0.7).abs() < 1e-6);
        assert_eq!(w.mean_density(999), None);
    }
}
