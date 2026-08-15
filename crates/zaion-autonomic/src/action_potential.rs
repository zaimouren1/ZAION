//! Action Potential System
//!
//! Implements stimulus accumulation and threshold-based firing,
//! inspired by biological neurons.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Threshold {
    pub value: f64,
    pub decay_rate: f64, // How fast accumulated potential decays over time
}

impl Default for Threshold {
    fn default() -> Self {
        Self {
            value: 1.0,
            decay_rate: 0.1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionPotential {
    pub id: String,
    pub name: String,
    pub threshold: Threshold,
    pub current_potential: f64,
    pub last_update: chrono::DateTime<chrono::Utc>,
}

impl ActionPotential {
    pub fn new(id: String, name: String, threshold: Threshold) -> Self {
        Self {
            id,
            name,
            threshold,
            current_potential: 0.0,
            last_update: chrono::Utc::now(),
        }
    }

    /// Add stimulus and check if threshold is reached
    pub fn stimulate(&mut self, amount: f64) -> bool {
        self.apply_decay();
        self.current_potential += amount;
        self.last_update = chrono::Utc::now();

        if self.current_potential >= self.threshold.value {
            self.fire();
            true
        } else {
            false
        }
    }

    /// Apply time-based decay to current potential
    fn apply_decay(&mut self) {
        let now = chrono::Utc::now();
        let elapsed = (now - self.last_update).num_seconds() as f64;
        let decay = self.threshold.decay_rate * elapsed;
        self.current_potential = (self.current_potential - decay).max(0.0);
    }

    /// Fire the action potential (reset to zero)
    fn fire(&mut self) {
        self.current_potential = 0.0;
    }

    /// Get current potential as percentage of threshold
    pub fn potential_percentage(&self) -> f64 {
        (self.current_potential / self.threshold.value * 100.0).min(100.0)
    }
}

pub struct StimulusAccumulator {
    potentials: HashMap<String, ActionPotential>,
}

impl StimulusAccumulator {
    pub fn new() -> Self {
        Self {
            potentials: HashMap::new(),
        }
    }

    pub fn register(&mut self, potential: ActionPotential) {
        self.potentials.insert(potential.id.clone(), potential);
    }

    pub fn stimulate(&mut self, id: &str, amount: f64) -> Result<bool, crate::AutonomicError> {
        let potential = self
            .potentials
            .get_mut(id)
            .ok_or_else(|| crate::AutonomicError::ReflexNotFound(id.to_string()))?;

        Ok(potential.stimulate(amount))
    }

    pub fn get(&self, id: &str) -> Option<&ActionPotential> {
        self.potentials.get(id)
    }

    pub fn list_all(&self) -> Vec<&ActionPotential> {
        self.potentials.values().collect()
    }
}

impl Default for StimulusAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_potential_fires_at_threshold() {
        let mut ap = ActionPotential::new(
            "test".to_string(),
            "Test AP".to_string(),
            Threshold::default(),
        );

        // Below threshold
        assert!(!ap.stimulate(0.5));
        assert_eq!(ap.current_potential, 0.5);

        // Reach threshold
        assert!(ap.stimulate(0.5));
        assert_eq!(ap.current_potential, 0.0); // Reset after firing
    }

    #[test]
    fn action_potential_accumulates() {
        let mut ap = ActionPotential::new(
            "test".to_string(),
            "Test AP".to_string(),
            Threshold {
                value: 2.0,
                decay_rate: 0.0,
            },
        );

        assert!(!ap.stimulate(0.5));
        assert!(!ap.stimulate(0.5));
        assert!(!ap.stimulate(0.5));
        assert!(ap.stimulate(0.5)); // 4th stimulus crosses threshold
    }

    #[test]
    fn stimulus_accumulator_manages_multiple() {
        let mut acc = StimulusAccumulator::new();

        acc.register(ActionPotential::new(
            "ap1".to_string(),
            "AP 1".to_string(),
            Threshold::default(),
        ));

        acc.register(ActionPotential::new(
            "ap2".to_string(),
            "AP 2".to_string(),
            Threshold::default(),
        ));

        assert_eq!(acc.list_all().len(), 2);
        assert!(acc.get("ap1").is_some());
        assert!(acc.get("ap2").is_some());
    }

    #[test]
    fn potential_percentage_calculation() {
        let mut ap = ActionPotential::new(
            "test".to_string(),
            "Test".to_string(),
            Threshold {
                value: 2.0,
                decay_rate: 0.0,
            },
        );

        ap.stimulate(1.0);
        assert_eq!(ap.potential_percentage(), 50.0);

        ap.stimulate(0.5);
        assert_eq!(ap.potential_percentage(), 75.0);
    }
}
