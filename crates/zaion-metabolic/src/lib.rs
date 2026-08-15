//! System IV: Metabolic Engine
//!
//! Token budgeting, pain receptors, and hunger-driven degradation
mod budget;
mod hunger;
mod pain;
pub mod policy;

pub use budget::{BudgetExceededError, BudgetTracker, TokenBudget};
pub use hunger::{DegradationLevel, HungerDegradation, HungerState};
pub use pain::{PainReceptor, PainSignal, PainThreshold};
pub use policy::{MetabolicAction, MetabolicPolicy};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum MetabolicError {
    #[error("budget exceeded: used={0}, limit={1}")]
    BudgetExceeded(u64, u64),
    #[error("pain threshold exceeded: {0}")]
    PainThresholdExceeded(String),
    #[error("severe hunger degradation: {0}")]
    SevereHungerDegradation(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metabolic_system_loads() {
        let tracker = BudgetTracker::new(1000);
        assert_eq!(tracker.remaining(), 1000);
    }
}
