//! Pain Receptor System
//!
//! Detects and signals adverse conditions in the system
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PainSignal {
    TokenStarvation,
    MemoryPressure,
    ContextOverflow,
    RepeatedFailure,
    TimeoutExceeded,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PainThreshold {
    pub signal_type: PainSignal,
    pub threshold: f64,
    pub current_level: f64,
    pub triggered: bool,
}

impl PainThreshold {
    pub fn new(signal_type: PainSignal, threshold: f64) -> Self {
        Self {
            signal_type,
            threshold,
            current_level: 0.0,
            triggered: false,
        }
    }

    pub fn update(&mut self, level: f64) -> bool {
        self.current_level = level;
        let was_triggered = self.triggered;
        self.triggered = level >= self.threshold;

        // Return true if threshold was just crossed
        !was_triggered && self.triggered
    }

    pub fn reset(&mut self) {
        self.current_level = 0.0;
        self.triggered = false;
    }

    pub fn severity(&self) -> f64 {
        if self.threshold == 0.0 {
            return 0.0;
        }
        (self.current_level / self.threshold).min(2.0) // Cap at 2x threshold
    }
}

pub struct PainReceptor {
    thresholds: HashMap<String, PainThreshold>,
}

impl PainReceptor {
    pub fn new() -> Self {
        Self {
            thresholds: HashMap::new(),
        }
    }

    pub fn register(&mut self, id: String, threshold: PainThreshold) {
        self.thresholds.insert(id, threshold);
    }

    pub fn signal(&mut self, id: &str, level: f64) -> Result<bool, crate::MetabolicError> {
        let threshold = self.thresholds.get_mut(id).ok_or_else(|| {
            crate::MetabolicError::PainThresholdExceeded(format!("Unknown pain receptor: {}", id))
        })?;

        let just_triggered = threshold.update(level);

        if just_triggered {
            return Err(crate::MetabolicError::PainThresholdExceeded(format!(
                "{:?} threshold exceeded: {:.2} >= {:.2}",
                threshold.signal_type, level, threshold.threshold
            )));
        }

        Ok(threshold.triggered)
    }

    pub fn get(&self, id: &str) -> Option<&PainThreshold> {
        self.thresholds.get(id)
    }

    pub fn reset(&mut self, id: &str) {
        if let Some(threshold) = self.thresholds.get_mut(id) {
            threshold.reset();
        }
    }

    pub fn reset_all(&mut self) {
        for threshold in self.thresholds.values_mut() {
            threshold.reset();
        }
    }

    pub fn active_signals(&self) -> Vec<&PainThreshold> {
        self.thresholds.values().filter(|t| t.triggered).collect()
    }

    pub fn count(&self) -> usize {
        self.thresholds.len()
    }
}

impl Default for PainReceptor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pain_threshold_starts_untriggered() {
        let threshold = PainThreshold::new(PainSignal::TokenStarvation, 0.8);
        assert!(!threshold.triggered);
        assert_eq!(threshold.current_level, 0.0);
    }

    #[test]
    fn threshold_triggers_when_exceeded() {
        let mut threshold = PainThreshold::new(PainSignal::MemoryPressure, 0.8);

        assert!(!threshold.update(0.5));
        assert!(!threshold.triggered);

        assert!(threshold.update(0.9));
        assert!(threshold.triggered);
    }

    #[test]
    fn receptor_registers_thresholds() {
        let mut receptor = PainReceptor::new();
        receptor.register(
            "token_starvation".to_string(),
            PainThreshold::new(PainSignal::TokenStarvation, 0.9),
        );

        assert_eq!(receptor.count(), 1);
        assert!(receptor.get("token_starvation").is_some());
    }

    #[test]
    fn receptor_signals_pain() {
        let mut receptor = PainReceptor::new();
        receptor.register(
            "memory".to_string(),
            PainThreshold::new(PainSignal::MemoryPressure, 0.8),
        );

        // Below threshold - no error
        assert!(receptor.signal("memory", 0.5).is_ok());

        // Above threshold - error on first crossing
        assert!(receptor.signal("memory", 0.9).is_err());

        // Still above - no new error
        assert!(receptor.signal("memory", 0.95).is_ok());
    }

    #[test]
    fn reset_clears_threshold() {
        let mut receptor = PainReceptor::new();
        receptor.register(
            "test".to_string(),
            PainThreshold::new(PainSignal::Custom("test".to_string()), 0.5),
        );

        receptor.signal("test", 0.8).ok();
        assert!(receptor.get("test").unwrap().triggered);

        receptor.reset("test");
        assert!(!receptor.get("test").unwrap().triggered);
    }

    #[test]
    fn active_signals_filters_triggered() {
        let mut receptor = PainReceptor::new();
        receptor.register(
            "signal1".to_string(),
            PainThreshold::new(PainSignal::TokenStarvation, 0.5),
        );
        receptor.register(
            "signal2".to_string(),
            PainThreshold::new(PainSignal::MemoryPressure, 0.5),
        );

        receptor.signal("signal1", 0.8).ok();

        let active = receptor.active_signals();
        assert_eq!(active.len(), 1);
    }

    #[test]
    fn severity_calculation() {
        let mut threshold = PainThreshold::new(PainSignal::TokenStarvation, 1.0);

        threshold.update(0.5);
        assert_eq!(threshold.severity(), 0.5);

        threshold.update(1.5);
        assert_eq!(threshold.severity(), 1.5);

        threshold.update(3.0);
        assert_eq!(threshold.severity(), 2.0); // Capped at 2x
    }
}
