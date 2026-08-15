//! Hunger-Driven Degradation
//!
//! Models performance degradation under resource starvation
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum DegradationLevel {
    None,
    Mild,
    Moderate,
    Severe,
    Critical,
}

impl DegradationLevel {
    pub fn performance_multiplier(&self) -> f64 {
        match self {
            DegradationLevel::None => 1.0,
            DegradationLevel::Mild => 0.9,
            DegradationLevel::Moderate => 0.7,
            DegradationLevel::Severe => 0.5,
            DegradationLevel::Critical => 0.3,
        }
    }

    pub fn from_hunger(hunger: f64) -> Self {
        if hunger < 0.2 {
            DegradationLevel::None
        } else if hunger < 0.4 {
            DegradationLevel::Mild
        } else if hunger < 0.6 {
            DegradationLevel::Moderate
        } else if hunger < 0.8 {
            DegradationLevel::Severe
        } else {
            DegradationLevel::Critical
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HungerState {
    pub hunger_level: f64, // 0.0 = satiated, 1.0 = starving
    pub last_fed: chrono::DateTime<chrono::Utc>,
    pub degradation: DegradationLevel,
}

impl HungerState {
    pub fn new() -> Self {
        Self {
            hunger_level: 0.0,
            last_fed: chrono::Utc::now(),
            degradation: DegradationLevel::None,
        }
    }

    pub fn feed(&mut self, amount: f64) {
        self.hunger_level = (self.hunger_level - amount).max(0.0);
        self.last_fed = chrono::Utc::now();
        self.degradation = DegradationLevel::from_hunger(self.hunger_level);
    }

    pub fn starve(&mut self, amount: f64) {
        self.hunger_level = (self.hunger_level + amount).min(1.0);
        self.degradation = DegradationLevel::from_hunger(self.hunger_level);
    }

    pub fn time_since_fed(&self) -> chrono::Duration {
        chrono::Utc::now() - self.last_fed
    }

    pub fn is_critical(&self) -> bool {
        self.degradation == DegradationLevel::Critical
    }
}

impl Default for HungerState {
    fn default() -> Self {
        Self::new()
    }
}

pub struct HungerDegradation {
    state: HungerState,
    decay_rate: f64, // Hunger increase per second
}

impl HungerDegradation {
    pub fn new(decay_rate: f64) -> Self {
        Self {
            state: HungerState::new(),
            decay_rate,
        }
    }

    pub fn update(&mut self) {
        let elapsed = self.state.time_since_fed().num_seconds() as f64;
        let hunger_increase = self.decay_rate * elapsed;
        self.state.starve(hunger_increase);
    }

    pub fn feed(&mut self, amount: f64) {
        self.state.feed(amount);
    }

    pub fn state(&self) -> &HungerState {
        &self.state
    }

    pub fn performance_multiplier(&self) -> f64 {
        self.state.degradation.performance_multiplier()
    }

    pub fn should_warn(&self) -> bool {
        self.state.degradation >= DegradationLevel::Moderate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_hunger_state_is_satiated() {
        let state = HungerState::new();
        assert_eq!(state.hunger_level, 0.0);
        assert_eq!(state.degradation, DegradationLevel::None);
    }

    #[test]
    fn feeding_reduces_hunger() {
        let mut state = HungerState::new();
        state.starve(0.5);
        assert_eq!(state.hunger_level, 0.5);

        state.feed(0.3);
        assert_eq!(state.hunger_level, 0.2);
    }

    #[test]
    fn hunger_cannot_go_negative() {
        let mut state = HungerState::new();
        state.feed(0.5);
        assert_eq!(state.hunger_level, 0.0);
    }

    #[test]
    fn hunger_cannot_exceed_one() {
        let mut state = HungerState::new();
        state.starve(1.5);
        assert_eq!(state.hunger_level, 1.0);
    }

    #[test]
    fn degradation_levels_map_correctly() {
        assert_eq!(DegradationLevel::from_hunger(0.1), DegradationLevel::None);
        assert_eq!(DegradationLevel::from_hunger(0.3), DegradationLevel::Mild);
        assert_eq!(
            DegradationLevel::from_hunger(0.5),
            DegradationLevel::Moderate
        );
        assert_eq!(DegradationLevel::from_hunger(0.7), DegradationLevel::Severe);
        assert_eq!(
            DegradationLevel::from_hunger(0.9),
            DegradationLevel::Critical
        );
    }

    #[test]
    fn performance_multiplier_decreases_with_hunger() {
        assert_eq!(DegradationLevel::None.performance_multiplier(), 1.0);
        assert_eq!(DegradationLevel::Mild.performance_multiplier(), 0.9);
        assert_eq!(DegradationLevel::Moderate.performance_multiplier(), 0.7);
        assert_eq!(DegradationLevel::Severe.performance_multiplier(), 0.5);
        assert_eq!(DegradationLevel::Critical.performance_multiplier(), 0.3);
    }

    #[test]
    fn critical_state_detection() {
        let mut state = HungerState::new();
        assert!(!state.is_critical());

        state.starve(0.9);
        assert!(state.is_critical());
    }

    #[test]
    fn hunger_degradation_warns_at_moderate() {
        let mut degradation = HungerDegradation::new(0.01);
        assert!(!degradation.should_warn());

        degradation.state.starve(0.6);
        assert!(degradation.should_warn());
    }
}
