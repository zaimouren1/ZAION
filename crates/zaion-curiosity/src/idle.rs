//! Idle Timer
//!
//! Tracks activity and detects idle periods
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum IdleState {
    Active,
    Idle,
    DeepIdle,
}

pub struct IdleTimer {
    last_activity: Instant,
    idle_threshold: Duration,
    deep_idle_threshold: Duration,
}

impl IdleTimer {
    pub fn new(idle_threshold: Duration) -> Self {
        Self {
            last_activity: Instant::now(),
            idle_threshold,
            deep_idle_threshold: idle_threshold * 3,
        }
    }

    pub fn with_thresholds(idle_threshold: Duration, deep_idle_threshold: Duration) -> Self {
        Self {
            last_activity: Instant::now(),
            idle_threshold,
            deep_idle_threshold,
        }
    }

    pub fn reset(&mut self) {
        self.last_activity = Instant::now();
    }

    pub fn time_since_activity(&self) -> Duration {
        Instant::now().duration_since(self.last_activity)
    }

    pub fn state(&self) -> IdleState {
        let elapsed = self.time_since_activity();

        if elapsed >= self.deep_idle_threshold {
            IdleState::DeepIdle
        } else if elapsed >= self.idle_threshold {
            IdleState::Idle
        } else {
            IdleState::Active
        }
    }

    pub fn is_idle(&self) -> bool {
        matches!(self.state(), IdleState::Idle | IdleState::DeepIdle)
    }

    pub fn is_deep_idle(&self) -> bool {
        matches!(self.state(), IdleState::DeepIdle)
    }

    pub fn idle_percentage(&self) -> f64 {
        let elapsed = self.time_since_activity().as_secs_f64();
        let threshold = self.idle_threshold.as_secs_f64();

        if elapsed < threshold {
            0.0
        } else {
            ((elapsed - threshold) / threshold * 100.0).min(100.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn new_timer_is_active() {
        let timer = IdleTimer::new(Duration::from_secs(1));
        assert_eq!(timer.state(), IdleState::Active);
        assert!(!timer.is_idle());
    }

    #[test]
    fn timer_becomes_idle() {
        let timer = IdleTimer::new(Duration::from_millis(50));
        // generous sleep: macOS thread::sleep can return early, so 3x the
        // timer window keeps this deterministic across runners.
        // sleep in (50ms, 150ms): past idle_threshold, before deep_idle (x3).
        thread::sleep(Duration::from_millis(100));
        assert_eq!(timer.state(), IdleState::Idle);
        assert!(timer.is_idle());
    }

    #[test]
    fn timer_becomes_deep_idle() {
        let timer = IdleTimer::new(Duration::from_millis(20));
        // past deep_idle (20ms x3 = 60ms), generous for early-return sleep.
        thread::sleep(Duration::from_millis(120));
        assert_eq!(timer.state(), IdleState::DeepIdle);
        assert!(timer.is_deep_idle());
    }

    #[test]
    fn reset_clears_idle_state() {
        let mut timer = IdleTimer::new(Duration::from_millis(50));
        thread::sleep(Duration::from_millis(60));
        assert!(timer.is_idle());

        timer.reset();
        assert!(!timer.is_idle());
        assert_eq!(timer.state(), IdleState::Active);
    }

    #[test]
    fn idle_percentage_calculation() {
        let timer = IdleTimer::new(Duration::from_secs(1));
        assert_eq!(timer.idle_percentage(), 0.0);
    }
}
