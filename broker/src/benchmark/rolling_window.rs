use std::collections::VecDeque;

/// Rolling buffer of recent samples, providing windowed means for both flow
/// (diagnostic) and congested meters. The congestion end condition (spec §7)
/// fires only once the buffer is full, so a single transient post-edit dip
/// can't end the run.
#[derive(Debug)]
pub struct RollingWindow {
    capacity: usize,
    samples: VecDeque<f64>,
}

impl RollingWindow {
    pub fn new(capacity: usize) -> Self {
        Self { capacity: capacity.max(1), samples: VecDeque::new() }
    }

    pub fn push(&mut self, sample: f64) {
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    pub fn mean(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        self.samples.iter().sum::<f64>() / self.samples.len() as f64
    }

    pub fn is_full(&self) -> bool {
        self.samples.len() == self.capacity
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mean_of_recent_window() {
        let mut w = RollingWindow::new(3);
        w.push(10.0);
        w.push(20.0);
        w.push(30.0);
        w.push(60.0); // evicts 10.0 -> window is [20,30,60]
        assert!((w.mean() - 36.666_666).abs() < 1e-3);
    }

    #[test]
    fn empty_window_mean_is_zero() {
        assert_eq!(RollingWindow::new(4).mean(), 0.0);
    }

    #[test]
    fn is_empty_until_first_sample() {
        let mut w = RollingWindow::new(4);
        assert!(w.is_empty());
        w.push(1.0);
        assert!(!w.is_empty());
    }

    #[test]
    fn zero_capacity_is_promoted_to_one() {
        let mut w = RollingWindow::new(0);
        assert!(!w.is_full());
        w.push(50.0);
        assert!(w.is_full());
        assert_eq!(w.mean(), 50.0);
    }
}
