//! Integration tests for System IV: Metabolic Engine
//!
//! Tests token budget tracking, hunger-driven degradation,
//! pain receptor system, and metabolic policy evaluation.

use zaion_metabolic::{
    BudgetTracker, DegradationLevel, HungerState, MetabolicAction, MetabolicPolicy, PainReceptor,
    PainSignal, PainThreshold, TokenBudget,
};

#[test]
fn test_token_budget_initialization() {
    let budget = TokenBudget::new(1000);
    assert_eq!(budget.total, 1000);
    assert_eq!(budget.used, 0);
    assert_eq!(budget.available(), 1000);
    assert_eq!(budget.reserved, 0);
}

#[test]
fn test_token_budget_remaining() {
    let mut budget = TokenBudget::new(1000);
    assert_eq!(budget.remaining(), 1000);

    budget.used = 300;
    assert_eq!(budget.remaining(), 700);

    budget.reserved = 200;
    assert_eq!(budget.remaining(), 500); // 1000 - 300 - 200
}

#[test]
fn test_token_budget_utilization() {
    let mut budget = TokenBudget::new(1000);
    assert_eq!(budget.utilization(), 0.0);

    budget.used = 500;
    assert_eq!(budget.utilization(), 50.0);

    budget.used = 800;
    assert_eq!(budget.utilization(), 80.0);
}

#[test]
fn test_token_budget_can_afford() {
    let mut budget = TokenBudget::new(1000);
    assert!(budget.can_afford(500));

    budget.used = 700;
    budget.reserved = 200;
    assert!(budget.can_afford(100)); // 1000 - 700 - 200 = 100
    assert!(!budget.can_afford(200));
}

#[test]
fn test_budget_tracker_initialization() {
    let tracker = BudgetTracker::new(1000);
    assert_eq!(tracker.utilization(), 0.0);
    assert!(!tracker.threshold_warning());
    assert!(!tracker.threshold_critical());
}

#[test]
fn test_budget_tracker_consume() {
    let tracker = BudgetTracker::new(1000);

    let result = tracker.consume(300);
    assert!(result.is_ok());
    assert_eq!(tracker.utilization(), 30.0); // Utilization returns percentage
}

#[test]
fn test_budget_tracker_warning_threshold() {
    let tracker = BudgetTracker::new(1000);

    // Below warning (80%)
    tracker.consume(700).unwrap();
    assert!(!tracker.threshold_warning());
    assert!(!tracker.threshold_critical());

    // At warning threshold
    tracker.consume(100).unwrap(); // Total 800, 80%
    assert!(tracker.threshold_warning());
    assert!(!tracker.threshold_critical());
}

#[test]
fn test_budget_tracker_critical_threshold() {
    let tracker = BudgetTracker::new(1000);

    // At critical threshold (95%)
    tracker.consume(950).unwrap();
    assert!(tracker.threshold_warning());
    assert!(tracker.threshold_critical());
}

#[test]
fn test_budget_tracker_with_custom_thresholds() {
    let tracker = BudgetTracker::with_thresholds(1000, 0.6, 0.9);

    // Below custom warning (60%)
    tracker.consume(500).unwrap();
    assert!(!tracker.threshold_warning());

    // At custom warning threshold
    tracker.consume(100).unwrap(); // Total 600, 60%
    assert!(tracker.threshold_warning());
    assert!(!tracker.threshold_critical());

    // At custom critical threshold (90%)
    tracker.consume(300).unwrap(); // Total 900, 90%
    assert!(tracker.threshold_critical());
}

#[test]
fn test_budget_tracker_reserve_and_release() {
    let tracker = BudgetTracker::new(1000);

    let result = tracker.reserve(200);
    assert!(result.is_ok());
    assert_eq!(tracker.remaining(), 800); // 1000 - 200

    tracker.release_reservation(200);
    assert_eq!(tracker.remaining(), 1000);
}

#[test]
fn test_budget_tracker_reset() {
    let tracker = BudgetTracker::new(1000);
    tracker.consume(500).unwrap();
    tracker.reserve(200).unwrap();

    tracker.reset();
    assert_eq!(tracker.utilization(), 0.0);
    assert_eq!(tracker.remaining(), 1000);
}

#[test]
fn test_hunger_state_initialization() {
    let hunger = HungerState::new();
    assert_eq!(hunger.degradation, DegradationLevel::None);
    assert_eq!(hunger.hunger_level, 0.0);
}

#[test]
fn test_hunger_state_degradation_levels() {
    let mut hunger = HungerState::new();

    // None -> Mild (hunger 0.2-0.4)
    hunger.starve(0.3);
    assert_eq!(hunger.degradation, DegradationLevel::Mild);
    assert_eq!(hunger.degradation.performance_multiplier(), 0.9);

    // Mild -> Moderate (hunger 0.4-0.6)
    hunger.starve(0.2);
    assert_eq!(hunger.degradation, DegradationLevel::Moderate);
    assert_eq!(hunger.degradation.performance_multiplier(), 0.7);

    // Moderate -> Severe (hunger 0.6-0.8)
    hunger.starve(0.2);
    assert_eq!(hunger.degradation, DegradationLevel::Severe);
    assert_eq!(hunger.degradation.performance_multiplier(), 0.5);

    // Severe -> Critical (hunger >= 0.8)
    hunger.starve(0.3);
    assert_eq!(hunger.degradation, DegradationLevel::Critical);
    assert_eq!(hunger.degradation.performance_multiplier(), 0.3);
}

#[test]
fn test_hunger_state_feed_recovery() {
    let mut hunger = HungerState::new();

    // Degrade to Severe
    hunger.starve(0.7); // hunger_level = 0.7
    assert_eq!(hunger.degradation, DegradationLevel::Severe);

    // Feed back to Moderate
    hunger.feed(0.2); // hunger_level = 0.5
    assert_eq!(hunger.degradation, DegradationLevel::Moderate);

    // Feed back to Mild
    hunger.feed(0.2); // hunger_level = 0.3
    assert_eq!(hunger.degradation, DegradationLevel::Mild);

    // Feed back to None
    hunger.feed(0.3); // hunger_level = 0.0
    assert_eq!(hunger.degradation, DegradationLevel::None);
}

#[test]
fn test_hunger_state_bounds() {
    let mut hunger = HungerState::new();

    // Cannot go below 0
    hunger.feed(1.0);
    assert_eq!(hunger.hunger_level, 0.0);

    // Cannot exceed 1
    hunger.starve(2.0);
    assert_eq!(hunger.hunger_level, 1.0);
}

#[test]
fn test_degradation_level_performance_multipliers() {
    assert_eq!(DegradationLevel::None.performance_multiplier(), 1.0);
    assert_eq!(DegradationLevel::Mild.performance_multiplier(), 0.9);
    assert_eq!(DegradationLevel::Moderate.performance_multiplier(), 0.7);
    assert_eq!(DegradationLevel::Severe.performance_multiplier(), 0.5);
    assert_eq!(DegradationLevel::Critical.performance_multiplier(), 0.3);
}

#[test]
fn test_pain_receptor_initialization() {
    let receptor = PainReceptor::new();
    assert_eq!(receptor.active_signals().len(), 0);
    assert_eq!(receptor.count(), 0);
}

#[test]
fn test_pain_receptor_register() {
    let mut receptor = PainReceptor::new();

    receptor.register(
        "token_starvation".to_string(),
        PainThreshold::new(PainSignal::TokenStarvation, 0.8),
    );

    assert_eq!(receptor.count(), 1);
    assert!(receptor.get("token_starvation").is_some());
}

#[test]
fn test_pain_receptor_signal_below_threshold() {
    let mut receptor = PainReceptor::new();
    receptor.register(
        "token_starvation".to_string(),
        PainThreshold::new(PainSignal::TokenStarvation, 0.8),
    );

    // Below threshold - should not trigger
    let result = receptor.signal("token_starvation", 0.5);
    assert!(result.is_ok());
    assert_eq!(receptor.active_signals().len(), 0);
}

#[test]
fn test_pain_receptor_signal_exceeds_threshold() {
    let mut receptor = PainReceptor::new();
    receptor.register(
        "token_starvation".to_string(),
        PainThreshold::new(PainSignal::TokenStarvation, 0.8),
    );

    // Exceeds threshold - should trigger error on first crossing
    let result = receptor.signal("token_starvation", 0.9);
    assert!(result.is_err());

    // Signal should be active
    let active = receptor.active_signals();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].signal_type, PainSignal::TokenStarvation);
}

#[test]
fn test_pain_receptor_multiple_signals() {
    let mut receptor = PainReceptor::new();
    receptor.register(
        "token_starvation".to_string(),
        PainThreshold::new(PainSignal::TokenStarvation, 0.8),
    );
    receptor.register(
        "memory_pressure".to_string(),
        PainThreshold::new(PainSignal::MemoryPressure, 0.7),
    );

    // Trigger both
    receptor.signal("token_starvation", 0.85).ok();
    receptor.signal("memory_pressure", 0.75).ok();

    let active = receptor.active_signals();
    assert_eq!(active.len(), 2);
}

#[test]
fn test_pain_receptor_reset_signal() {
    let mut receptor = PainReceptor::new();
    receptor.register(
        "token_starvation".to_string(),
        PainThreshold::new(PainSignal::TokenStarvation, 0.8),
    );

    // Trigger signal
    receptor.signal("token_starvation", 0.9).ok();
    assert_eq!(receptor.active_signals().len(), 1);

    // Reset signal
    receptor.reset("token_starvation");
    assert_eq!(receptor.active_signals().len(), 0);
}

#[test]
fn test_pain_receptor_signal_types() {
    let mut receptor = PainReceptor::new();

    receptor.register(
        "token_starvation".to_string(),
        PainThreshold::new(PainSignal::TokenStarvation, 0.8),
    );
    receptor.register(
        "memory_pressure".to_string(),
        PainThreshold::new(PainSignal::MemoryPressure, 0.8),
    );
    receptor.register(
        "context_overflow".to_string(),
        PainThreshold::new(PainSignal::ContextOverflow, 0.8),
    );
    receptor.register(
        "repeated_failure".to_string(),
        PainThreshold::new(PainSignal::RepeatedFailure, 0.8),
    );
    receptor.register(
        "timeout_exceeded".to_string(),
        PainThreshold::new(PainSignal::TimeoutExceeded, 0.8),
    );

    // Trigger all types
    receptor.signal("token_starvation", 0.9).ok();
    receptor.signal("memory_pressure", 0.9).ok();
    receptor.signal("context_overflow", 0.9).ok();
    receptor.signal("repeated_failure", 0.9).ok();
    receptor.signal("timeout_exceeded", 0.9).ok();

    assert_eq!(receptor.active_signals().len(), 5);
}

#[test]
fn test_pain_threshold_severity() {
    let mut threshold = PainThreshold::new(PainSignal::TokenStarvation, 1.0);

    threshold.update(0.5);
    assert_eq!(threshold.severity(), 0.5);

    threshold.update(1.5);
    assert_eq!(threshold.severity(), 1.5);

    threshold.update(3.0);
    assert_eq!(threshold.severity(), 2.0); // Capped at 2x
}

#[test]
fn test_metabolic_policy_evaluate_normal() {
    let tracker = BudgetTracker::new(1000);

    // Below warning threshold (80%)
    tracker.consume(500).unwrap();
    let action = MetabolicPolicy::evaluate(&tracker);
    assert_eq!(action, MetabolicAction::Normal);
}

#[test]
fn test_metabolic_policy_evaluate_reduce_concurrency() {
    let tracker = BudgetTracker::new(1000);

    // At warning threshold (80%)
    tracker.consume(800).unwrap();
    let action = MetabolicPolicy::evaluate(&tracker);
    assert_eq!(
        action,
        MetabolicAction::ReduceConcurrency { max_parallel: 2 }
    );

    // Between warning and critical
    let tracker2 = BudgetTracker::new(1000);
    tracker2.consume(900).unwrap();
    let action2 = MetabolicPolicy::evaluate(&tracker2);
    assert_eq!(
        action2,
        MetabolicAction::ReduceConcurrency { max_parallel: 2 }
    );
}

#[test]
fn test_metabolic_policy_evaluate_emergency_throttle() {
    let tracker = BudgetTracker::new(1000);

    // At critical threshold (95%)
    tracker.consume(950).unwrap();
    let action = MetabolicPolicy::evaluate(&tracker);
    assert_eq!(action, MetabolicAction::EmergencyThrottle);

    // Above critical
    let tracker2 = BudgetTracker::new(1000);
    tracker2.consume(990).unwrap();
    let action2 = MetabolicPolicy::evaluate(&tracker2);
    assert_eq!(action2, MetabolicAction::EmergencyThrottle);
}

#[test]
fn test_metabolic_policy_decision_table() {
    // Test full decision table
    let t1 = BudgetTracker::new(1000);
    t1.consume(0).unwrap();
    assert_eq!(MetabolicPolicy::evaluate(&t1), MetabolicAction::Normal);

    let t2 = BudgetTracker::new(1000);
    t2.consume(500).unwrap();
    assert_eq!(MetabolicPolicy::evaluate(&t2), MetabolicAction::Normal);

    let t3 = BudgetTracker::new(1000);
    t3.consume(790).unwrap();
    assert_eq!(MetabolicPolicy::evaluate(&t3), MetabolicAction::Normal);

    let t4 = BudgetTracker::new(1000);
    t4.consume(800).unwrap();
    assert_eq!(
        MetabolicPolicy::evaluate(&t4),
        MetabolicAction::ReduceConcurrency { max_parallel: 2 }
    );

    let t5 = BudgetTracker::new(1000);
    t5.consume(850).unwrap();
    assert_eq!(
        MetabolicPolicy::evaluate(&t5),
        MetabolicAction::ReduceConcurrency { max_parallel: 2 }
    );

    let t6 = BudgetTracker::new(1000);
    t6.consume(940).unwrap();
    assert_eq!(
        MetabolicPolicy::evaluate(&t6),
        MetabolicAction::ReduceConcurrency { max_parallel: 2 }
    );

    let t7 = BudgetTracker::new(1000);
    t7.consume(950).unwrap();
    assert_eq!(
        MetabolicPolicy::evaluate(&t7),
        MetabolicAction::EmergencyThrottle
    );

    let t8 = BudgetTracker::new(1000);
    t8.consume(1000).unwrap();
    assert_eq!(
        MetabolicPolicy::evaluate(&t8),
        MetabolicAction::EmergencyThrottle
    );
}

#[test]
fn test_end_to_end_metabolic_workflow() {
    // 1. Create budget tracker
    let tracker = BudgetTracker::new(1000);
    assert_eq!(tracker.utilization(), 0.0);

    // 2. Create hunger state
    let mut hunger = HungerState::new();
    assert_eq!(hunger.degradation, DegradationLevel::None);

    // 3. Create pain receptor
    let mut pain = PainReceptor::new();
    pain.register(
        "token_starvation".to_string(),
        PainThreshold::new(PainSignal::TokenStarvation, 80.0),
    );

    // 4. Simulate token consumption
    tracker.consume(700).unwrap();
    assert_eq!(tracker.utilization(), 70.0);
    assert_eq!(MetabolicPolicy::evaluate(&tracker), MetabolicAction::Normal);

    // 5. Consume more - hit warning threshold
    tracker.consume(100).unwrap(); // Total 800, 80% utilization
    assert!(tracker.threshold_warning());
    assert_eq!(
        MetabolicPolicy::evaluate(&tracker),
        MetabolicAction::ReduceConcurrency { max_parallel: 2 }
    );

    // 6. Trigger pain signal
    let result = pain.signal("token_starvation", tracker.utilization());
    assert!(result.is_err()); // Threshold exceeded
    assert_eq!(pain.active_signals().len(), 1);

    // 7. Increase hunger
    hunger.starve(0.3);
    assert_eq!(hunger.degradation, DegradationLevel::Mild);
    assert_eq!(hunger.degradation.performance_multiplier(), 0.9);

    // 8. Continue consumption - hit critical
    tracker.consume(150).unwrap(); // Total 950, 95% utilization
    assert!(tracker.threshold_critical());
    assert_eq!(
        MetabolicPolicy::evaluate(&tracker),
        MetabolicAction::EmergencyThrottle
    );

    // 9. Severe hunger
    hunger.starve(0.4); // Total 0.7
    assert_eq!(hunger.degradation, DegradationLevel::Severe);
    assert_eq!(hunger.degradation.performance_multiplier(), 0.5);

    // 10. Reset and recover
    tracker.reset();
    assert_eq!(tracker.utilization(), 0.0);
    assert!(!tracker.threshold_warning());

    hunger.feed(0.7);
    assert_eq!(hunger.degradation, DegradationLevel::None);

    pain.reset("token_starvation");
    assert_eq!(pain.active_signals().len(), 0);
}

#[test]
fn test_metabolic_system_stress_scenario() {
    // Simulate high-stress metabolic scenario
    let tracker = BudgetTracker::new(1000);
    let mut hunger = HungerState::new();
    let mut pain = PainReceptor::new();

    pain.register(
        "token_starvation".to_string(),
        PainThreshold::new(PainSignal::TokenStarvation, 80.0),
    );
    pain.register(
        "memory_pressure".to_string(),
        PainThreshold::new(PainSignal::MemoryPressure, 70.0),
    );

    // Rapid token consumption
    tracker.consume(600).unwrap();
    assert_eq!(MetabolicPolicy::evaluate(&tracker), MetabolicAction::Normal);

    tracker.consume(200).unwrap(); // 80%
    assert_eq!(
        MetabolicPolicy::evaluate(&tracker),
        MetabolicAction::ReduceConcurrency { max_parallel: 2 }
    );
    pain.signal("token_starvation", tracker.utilization()).ok();

    // Hunger increases
    hunger.starve(0.5); // Moderate
    assert_eq!(hunger.degradation.performance_multiplier(), 0.7);

    tracker.consume(100).unwrap(); // 90%
    pain.signal("memory_pressure", 75.0).ok();

    // Critical state
    tracker.consume(50).unwrap(); // 95%
    assert_eq!(
        MetabolicPolicy::evaluate(&tracker),
        MetabolicAction::EmergencyThrottle
    );

    hunger.starve(0.4); // 0.9 total - Critical
    assert_eq!(hunger.degradation, DegradationLevel::Critical);
    assert_eq!(hunger.degradation.performance_multiplier(), 0.3);

    // Verify system is in distress
    assert!(tracker.threshold_critical());
    assert_eq!(pain.active_signals().len(), 2);
    assert_eq!(
        MetabolicPolicy::evaluate(&tracker),
        MetabolicAction::EmergencyThrottle
    );
}
