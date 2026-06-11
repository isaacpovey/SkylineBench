use std::collections::HashMap;

use crate::contract::SegmentLoad;

/// Densities arrive as f32; comparing them against an f64 threshold directly
/// would exclude segments sitting exactly at the threshold (0.7f32 widens to
/// ~0.69999999). Narrow the threshold to f32 first so "at threshold" counts.
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
/// contributes the samples where it exists.
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
