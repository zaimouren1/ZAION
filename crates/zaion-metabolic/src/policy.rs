//! Metabolic Policy Engine
//!
//! Evaluates the current budget state and decides what degradation action to enforce.
use crate::budget::BudgetTracker;

/// Actions the metabolic engine can take based on budget utilization.
#[derive(Debug, Clone, PartialEq)]
pub enum MetabolicAction {
    /// All systems operating normally — no restrictions.
    Normal,
    /// Warning threshold crossed: cap concurrent shadow tasks.
    ReduceConcurrency { max_parallel: usize },
    /// Critical threshold crossed: switch to a cheaper model.
    SwitchModel { preferred_model: String },
    /// Both concurrency reduction and model switch are in effect.
    EmergencyThrottle,
}

/// Stateless policy evaluator.
pub struct MetabolicPolicy;

impl MetabolicPolicy {
    /// Decide what action to take based on the current tracker state.
    ///
    /// Decision table:
    /// - ≥ 95% used → `EmergencyThrottle`
    /// - ≥ 80% used → `ReduceConcurrency { max_parallel: 2 }`
    /// - < 80% used → `Normal`
    pub fn evaluate(tracker: &BudgetTracker) -> MetabolicAction {
        if tracker.threshold_critical() {
            MetabolicAction::EmergencyThrottle
        } else if tracker.threshold_warning() {
            MetabolicAction::ReduceConcurrency { max_parallel: 2 }
        } else {
            MetabolicAction::Normal
        }
    }

    /// Human-readable one-liner for the given action.
    pub fn describe(action: &MetabolicAction) -> &'static str {
        match action {
            MetabolicAction::Normal => "Normal — all systems operating at full capacity",
            MetabolicAction::ReduceConcurrency { .. } => {
                "Warning — concurrency reduced to 2 parallel tasks"
            }
            MetabolicAction::SwitchModel { .. } => {
                "Warning — switched to cheaper model to conserve budget"
            }
            MetabolicAction::EmergencyThrottle => {
                "Critical — emergency throttle: concurrency capped and model downgraded"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::BudgetTracker;

    #[test]
    fn normal_below_warning() {
        // 0% used — should be Normal
        let tracker = BudgetTracker::new(100_000);
        let action = MetabolicPolicy::evaluate(&tracker);
        assert_eq!(action, MetabolicAction::Normal);
    }

    #[test]
    fn reduce_at_warning() {
        // Consume exactly 80% — should trigger ReduceConcurrency
        let tracker = BudgetTracker::new(100_000);
        tracker.consume(80_000).expect("consume 80k");
        assert!(tracker.threshold_warning(), "should be at warning level");
        assert!(
            !tracker.threshold_critical(),
            "should not be at critical level"
        );

        let action = MetabolicPolicy::evaluate(&tracker);
        assert_eq!(
            action,
            MetabolicAction::ReduceConcurrency { max_parallel: 2 }
        );
    }

    #[test]
    fn emergency_at_critical() {
        // Consume exactly 95% — should trigger EmergencyThrottle
        let tracker = BudgetTracker::new(100_000);
        tracker.consume(95_000).expect("consume 95k");
        assert!(tracker.threshold_critical(), "should be at critical level");

        let action = MetabolicPolicy::evaluate(&tracker);
        assert_eq!(action, MetabolicAction::EmergencyThrottle);
    }

    #[test]
    fn describe_returns_static_str() {
        assert!(!MetabolicPolicy::describe(&MetabolicAction::Normal).is_empty());
        assert!(
            !MetabolicPolicy::describe(&MetabolicAction::ReduceConcurrency { max_parallel: 2 })
                .is_empty()
        );
        assert!(!MetabolicPolicy::describe(&MetabolicAction::SwitchModel {
            preferred_model: "gpt-4o-mini".to_string()
        })
        .is_empty());
        assert!(!MetabolicPolicy::describe(&MetabolicAction::EmergencyThrottle).is_empty());
    }
}
