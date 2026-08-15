//! Token Budget Management
//!
//! Tracks token consumption and enforces budget limits
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBudget {
    pub total: u64,
    pub used: u64,
    pub reserved: u64,
}

impl TokenBudget {
    pub fn new(total: u64) -> Self {
        Self {
            total,
            used: 0,
            reserved: 0,
        }
    }

    pub fn remaining(&self) -> u64 {
        self.total.saturating_sub(self.used + self.reserved)
    }

    pub fn available(&self) -> u64 {
        self.total.saturating_sub(self.used)
    }

    pub fn utilization(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        (self.used as f64 / self.total as f64) * 100.0
    }

    pub fn can_afford(&self, amount: u64) -> bool {
        self.remaining() >= amount
    }
}

pub struct BudgetTracker {
    budget: Arc<Mutex<TokenBudget>>,
    warning_threshold: f64,
    critical_threshold: f64,
}

impl BudgetTracker {
    pub fn new(total: u64) -> Self {
        Self::with_thresholds(total, 0.80, 0.95)
    }

    pub fn with_thresholds(total: u64, warning: f64, critical: f64) -> Self {
        Self {
            budget: Arc::new(Mutex::new(TokenBudget::new(total))),
            warning_threshold: warning,
            critical_threshold: critical,
        }
    }

    pub fn consume(&self, amount: u64) -> Result<(), BudgetExceededError> {
        let mut budget = self.budget.lock().unwrap();

        if !budget.can_afford(amount) {
            return Err(BudgetExceededError {
                requested: amount,
                available: budget.remaining(),
                total: budget.total,
            });
        }

        budget.used += amount;
        Ok(())
    }

    pub fn reserve(&self, amount: u64) -> Result<(), BudgetExceededError> {
        let mut budget = self.budget.lock().unwrap();

        if !budget.can_afford(amount) {
            return Err(BudgetExceededError {
                requested: amount,
                available: budget.remaining(),
                total: budget.total,
            });
        }

        budget.reserved += amount;
        Ok(())
    }

    pub fn release_reservation(&self, amount: u64) {
        let mut budget = self.budget.lock().unwrap();
        budget.reserved = budget.reserved.saturating_sub(amount);
    }

    pub fn remaining(&self) -> u64 {
        self.budget.lock().unwrap().remaining()
    }

    pub fn utilization(&self) -> f64 {
        self.budget.lock().unwrap().utilization()
    }

    pub fn snapshot(&self) -> TokenBudget {
        self.budget.lock().unwrap().clone()
    }

    pub fn reset(&self) {
        let mut budget = self.budget.lock().unwrap();
        budget.used = 0;
        budget.reserved = 0;
    }

    /// Returns true when utilization reaches the warning threshold (default 80%).
    pub fn threshold_warning(&self) -> bool {
        self.utilization() >= self.warning_threshold * 100.0
    }

    /// Returns true when utilization reaches the critical threshold (default 95%).
    pub fn threshold_critical(&self) -> bool {
        self.utilization() >= self.critical_threshold * 100.0
    }

    /// Configure warning and critical thresholds.
    /// `warning` must be less than `critical`.
    pub fn set_thresholds(&mut self, warning: f64, critical: f64) {
        self.warning_threshold = warning;
        self.critical_threshold = critical;
    }

    /// Returns the current warning threshold value.
    pub fn warning_threshold(&self) -> f64 {
        self.warning_threshold
    }

    /// Returns the current critical threshold value.
    pub fn critical_threshold(&self) -> f64 {
        self.critical_threshold
    }
}

#[derive(Debug, Clone)]
pub struct BudgetExceededError {
    pub requested: u64,
    pub available: u64,
    pub total: u64,
}

impl std::fmt::Display for BudgetExceededError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Budget exceeded: requested {} tokens, only {} available (total: {})",
            self.requested, self.available, self.total
        )
    }
}

impl std::error::Error for BudgetExceededError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_budget_starts_empty() {
        let budget = TokenBudget::new(1000);
        assert_eq!(budget.total, 1000);
        assert_eq!(budget.used, 0);
        assert_eq!(budget.remaining(), 1000);
    }

    #[test]
    fn can_consume_within_budget() {
        let tracker = BudgetTracker::new(1000);
        assert!(tracker.consume(500).is_ok());
        assert_eq!(tracker.remaining(), 500);
    }

    #[test]
    fn cannot_exceed_budget() {
        let tracker = BudgetTracker::new(1000);
        tracker.consume(800).unwrap();
        assert!(tracker.consume(300).is_err());
    }

    #[test]
    fn reservation_blocks_tokens() {
        let tracker = BudgetTracker::new(1000);
        tracker.reserve(300).unwrap();
        assert_eq!(tracker.remaining(), 700);

        tracker.consume(600).unwrap();
        assert!(tracker.consume(200).is_err()); // Would exceed with reservation
    }

    #[test]
    fn release_reservation_frees_tokens() {
        let tracker = BudgetTracker::new(1000);
        tracker.reserve(300).unwrap();
        assert_eq!(tracker.remaining(), 700);

        tracker.release_reservation(300);
        assert_eq!(tracker.remaining(), 1000);
    }

    #[test]
    fn utilization_calculation() {
        let tracker = BudgetTracker::new(1000);
        tracker.consume(250).unwrap();
        assert_eq!(tracker.utilization(), 25.0);

        tracker.consume(250).unwrap();
        assert_eq!(tracker.utilization(), 50.0);
    }

    #[test]
    fn reset_clears_usage() {
        let tracker = BudgetTracker::new(1000);
        tracker.consume(500).unwrap();
        tracker.reserve(200).unwrap();

        tracker.reset();
        assert_eq!(tracker.remaining(), 1000);
    }

    #[test]
    fn threshold_warning_triggers_at_eighty_percent() {
        let tracker = BudgetTracker::new(1000);
        assert!(!tracker.threshold_warning());

        tracker.consume(799).unwrap(); // 79.9%
        assert!(!tracker.threshold_warning());

        tracker.consume(2).unwrap(); // 80.1%
        assert!(tracker.threshold_warning());
    }

    #[test]
    fn threshold_critical_triggers_at_ninety_five_percent() {
        let tracker = BudgetTracker::new(1000);
        assert!(!tracker.threshold_critical());

        tracker.consume(949).unwrap(); // 94.9%
        assert!(!tracker.threshold_critical());

        tracker.consume(2).unwrap(); // 95.1%
        assert!(tracker.threshold_critical());
    }

    #[test]
    fn warning_and_critical_are_both_true_at_high_utilization() {
        let tracker = BudgetTracker::new(1000);
        tracker.consume(960).unwrap(); // 96%
        assert!(tracker.threshold_warning());
        assert!(tracker.threshold_critical());
    }

    #[test]
    fn set_thresholds_changes_behavior() {
        let mut tracker = BudgetTracker::new(1000);
        tracker.consume(250).unwrap(); // 25%

        // Default thresholds: warning=80, critical=95
        assert!(!tracker.threshold_warning());
        assert!(!tracker.threshold_critical());

        // Reconfigure to 20% warning, 30% critical
        tracker.set_thresholds(0.20, 0.30);
        assert!(tracker.threshold_warning());
        assert!(!tracker.threshold_critical());

        tracker.consume(100).unwrap(); // 35%
        assert!(tracker.threshold_critical());
    }

    #[test]
    fn with_thresholds_constructs_with_custom_values() {
        let tracker = BudgetTracker::with_thresholds(1000, 0.50, 0.75);
        assert_eq!(tracker.warning_threshold(), 0.50);
        assert_eq!(tracker.critical_threshold(), 0.75);

        tracker.consume(500).unwrap(); // 50%
        assert!(tracker.threshold_warning());
        assert!(!tracker.threshold_critical());

        tracker.consume(250).unwrap(); // 75%
        assert!(tracker.threshold_warning());
        assert!(tracker.threshold_critical());
    }
}
