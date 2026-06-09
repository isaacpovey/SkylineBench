use std::collections::VecDeque;

/// Rolling buffer of recent `flow_percent` samples. The flow-target out (spec
/// §7) fires only once the buffer is full and its mean clears the target, so a
/// single transient post-edit spike can't end the run.
#[derive(Debug)]
pub struct FlowWindow {
    capacity: usize,
    samples: VecDeque<f64>,
}

impl FlowWindow {
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

    pub fn target_reached(&self, target: f64) -> bool {
        self.is_full() && self.mean() >= target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mean_of_recent_window() {
        let mut w = FlowWindow::new(3);
        w.push(10.0);
        w.push(20.0);
        w.push(30.0);
        w.push(60.0); // evicts 10.0 -> window is [20,30,60]
        assert!((w.mean() - 36.666_666).abs() < 1e-3);
    }

    #[test]
    fn empty_window_mean_is_zero() {
        assert_eq!(FlowWindow::new(4).mean(), 0.0);
    }

    #[test]
    fn target_reached_only_on_windowed_mean() {
        let mut w = FlowWindow::new(2);
        w.push(100.0);
        assert!(!w.target_reached(95.0), "single sample must not trip");
        w.push(96.0);
        assert!(w.target_reached(95.0));
    }
}
