//! Integration tests for System Integration: Singularity Runtime
//!
//! Tests the orchestration of all 5 v5.0 systems into a unified runtime.

use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

use zaion_crypto::keypair::ZaionKeypair;
use zaion_curiosity::IdleState;
use zaion_ledger::EventLedger;
use zaion_metabolic::{DegradationLevel, MetabolicAction};
use zaion_proprioception::ShockSeverity;
use zaion_singularity::SingularityRuntime;
use zaion_types::NamespaceKey;

fn setup_runtime() -> (SingularityRuntime, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let keypair = ZaionKeypair::generate();
    let namespace_key = NamespaceKey("test-namespace".to_string());
    let ledger_path = temp_dir.path().join("ledger.db");
    let ledger = Arc::new(EventLedger::new(&ledger_path));
    ledger.ensure().unwrap();

    let runtime =
        SingularityRuntime::new(temp_dir.path(), ledger, Arc::new(keypair), namespace_key).unwrap();

    (runtime, temp_dir)
}

#[test]
fn test_runtime_initialization() {
    let (runtime, _temp) = setup_runtime();

    // Verify initial state
    assert_eq!(runtime.remaining_budget(), 100000);
    assert_eq!(runtime.idle_state(), IdleState::Active);
    assert!(matches!(
        runtime.hunger_degradation(),
        &DegradationLevel::None
    ));
    assert_eq!(runtime.check_pain().len(), 0);
}

#[test]
fn test_system_i_ego_system_prompt() {
    let (runtime, _temp) = setup_runtime();

    let prompt = runtime.system_prompt();
    assert!(prompt.contains("<Zaion_Protocol>"));
    assert!(prompt.contains("</Zaion_Protocol>"));
    assert!(prompt.contains("<Identity>"));
}

#[test]
fn test_system_i_ego_baffle_filtering() {
    let (runtime, _temp) = setup_runtime();

    // Default manifest has no banned tokens
    assert!(runtime.is_token_allowed("test"));
    assert!(runtime.is_token_allowed("hello"));

    let filtered = runtime.filter_response("test response");
    assert_eq!(filtered, "test response");
}

#[test]
fn test_system_i_ego_soul_hash() {
    let (runtime, _temp) = setup_runtime();

    let soul_hash = runtime.soul_hash();
    assert!(!soul_hash.manifest_hash.is_empty());
    assert!(!soul_hash.signature_hex.is_empty());
}

#[test]
fn test_system_iii_proprioception_shock_detection() {
    let (mut runtime, _temp) = setup_runtime();

    // First check should be None (same environment)
    let severity = runtime.check_shock().unwrap();
    assert_eq!(severity, ShockSeverity::None);
}

#[test]
fn test_system_iv_metabolic_token_consumption() {
    let (mut runtime, _temp) = setup_runtime();

    // Consume tokens
    runtime.consume_tokens(1000).unwrap();
    assert_eq!(runtime.remaining_budget(), 99000);

    // Consume more
    runtime.consume_tokens(5000).unwrap();
    assert_eq!(runtime.remaining_budget(), 94000);
}

#[test]
fn test_system_iv_metabolic_hunger_increases_with_consumption() {
    let (mut runtime, _temp) = setup_runtime();

    // Initial state
    assert!(matches!(
        runtime.hunger_degradation(),
        &DegradationLevel::None
    ));

    // Consume significant tokens
    runtime.consume_tokens(30000).unwrap();

    // Hunger should increase (30000 / 100000 = 0.3 hunger ratio -> Mild)
    assert!(matches!(
        runtime.hunger_degradation(),
        &DegradationLevel::Mild
    ));
}

#[test]
fn test_system_iv_metabolic_feed_tokens() {
    let (mut runtime, _temp) = setup_runtime();

    // Consume to increase hunger
    runtime.consume_tokens(50000).unwrap();
    assert!(matches!(
        runtime.hunger_degradation(),
        &DegradationLevel::Moderate
    ));

    // Feed to reduce hunger (80000 / 100000 = 0.8 feed ratio)
    // Current hunger: 0.5, after feed: 0.5 - 0.8 = -0.3 → clamped to 0.0 (None)
    runtime.feed_tokens(30000);

    // After feeding 30000 (0.3 ratio), hunger goes from 0.5 to 0.2 (Mild)
    assert!(matches!(
        runtime.hunger_degradation(),
        &DegradationLevel::None | &DegradationLevel::Mild
    ));
}

#[test]
fn test_system_iv_metabolic_policy_evaluation() {
    let (mut runtime, _temp) = setup_runtime();

    // Normal state
    assert_eq!(runtime.evaluate_metabolic_policy(), MetabolicAction::Normal);

    // Warning state (80%)
    runtime.consume_tokens(80000).unwrap();
    assert_eq!(
        runtime.evaluate_metabolic_policy(),
        MetabolicAction::ReduceConcurrency { max_parallel: 2 }
    );

    // Critical state (95%)
    runtime.consume_tokens(15000).unwrap(); // Total 95000
    assert_eq!(
        runtime.evaluate_metabolic_policy(),
        MetabolicAction::EmergencyThrottle
    );
}

#[test]
fn test_system_v_curiosity_activity_tracking() {
    let (mut runtime, _temp) = setup_runtime();

    // Initial state
    assert_eq!(runtime.idle_state(), IdleState::Active);

    // Mark activity
    runtime.mark_activity();
    assert_eq!(runtime.idle_state(), IdleState::Active);
}

#[test]
fn test_system_v_curiosity_idle_state_transitions() {
    let (runtime, _temp) = setup_runtime();

    // Active initially
    assert_eq!(runtime.idle_state(), IdleState::Active);

    // Wait for idle (5 minutes configured in runtime)
    // Note: This test doesn't actually wait, just verifies immediate state
    thread::sleep(Duration::from_millis(100));

    // Still active (not enough time passed)
    assert_eq!(runtime.idle_state(), IdleState::Active);
}

#[test]
fn test_system_v_curiosity_ideation_not_triggered_when_active() {
    let (mut runtime, _temp) = setup_runtime();

    // Should not ideate when active
    let prompt = runtime.should_ideate();
    assert!(prompt.is_none());
}

#[test]
fn test_end_to_end_full_system_integration() {
    let (mut runtime, _temp) = setup_runtime();

    // System I: Check ego
    let system_prompt = runtime.system_prompt();
    assert!(system_prompt.contains("<Zaion_Protocol>"));
    assert!(runtime.is_token_allowed("test"));

    // System III: Check shock
    let shock = runtime.check_shock().unwrap();
    assert_eq!(shock, ShockSeverity::None);

    // System IV: Consume tokens
    runtime.consume_tokens(50000).unwrap();
    assert_eq!(runtime.remaining_budget(), 50000);

    // Check hunger increased
    assert!(matches!(
        runtime.hunger_degradation(),
        &DegradationLevel::Moderate
    ));

    // Check policy evaluation
    assert_eq!(runtime.evaluate_metabolic_policy(), MetabolicAction::Normal);

    // Consume more to trigger warning
    runtime.consume_tokens(30000).unwrap();
    assert_eq!(
        runtime.evaluate_metabolic_policy(),
        MetabolicAction::ReduceConcurrency { max_parallel: 2 }
    );

    // System V: Mark activity
    runtime.mark_activity();
    assert_eq!(runtime.idle_state(), IdleState::Active);

    // Feed tokens to recover (40000 / 100000 = 0.4 feed ratio)
    // Current hunger: 0.8, after feed: 0.8 - 0.4 = 0.4 (Moderate)
    runtime.feed_tokens(40000);
    assert!(matches!(
        runtime.hunger_degradation(),
        &DegradationLevel::Moderate | &DegradationLevel::Mild
    ));
}

#[test]
fn test_multi_system_stress_scenario() {
    let (mut runtime, _temp) = setup_runtime();

    // Simulate high load
    for _ in 0..8 {
        runtime.consume_tokens(10000).unwrap();
    }

    // Should be at 80% (warning threshold)
    assert_eq!(runtime.remaining_budget(), 20000);
    assert_eq!(
        runtime.evaluate_metabolic_policy(),
        MetabolicAction::ReduceConcurrency { max_parallel: 2 }
    );

    // Check hunger is severe
    assert!(matches!(
        runtime.hunger_degradation(),
        &DegradationLevel::Severe
    ));

    // Continue to critical
    runtime.consume_tokens(15000).unwrap();
    assert_eq!(
        runtime.evaluate_metabolic_policy(),
        MetabolicAction::EmergencyThrottle
    );

    // Check hunger is critical
    assert!(matches!(
        runtime.hunger_degradation(),
        &DegradationLevel::Critical
    ));

    // Mark activity (System V still functioning)
    runtime.mark_activity();
    assert_eq!(runtime.idle_state(), IdleState::Active);

    // Check shock (System III still monitoring)
    let shock = runtime.check_shock().unwrap();
    assert_eq!(shock, ShockSeverity::None);
}

#[test]
fn test_system_state_independence() {
    let (mut runtime, _temp) = setup_runtime();

    // System I operates independently
    let prompt1 = runtime.system_prompt();
    runtime.consume_tokens(50000).unwrap();
    let prompt2 = runtime.system_prompt();
    assert_eq!(prompt1, prompt2); // Ego not affected by metabolic state

    // System III operates independently
    let shock1 = runtime.check_shock().unwrap();
    runtime.mark_activity();
    let shock2 = runtime.check_shock().unwrap();
    assert_eq!(shock1, shock2); // Proprioception not affected by curiosity
}

#[test]
fn test_runtime_recovery_workflow() {
    let (mut runtime, _temp) = setup_runtime();

    // Degrade system
    runtime.consume_tokens(90000).unwrap();
    assert_eq!(
        runtime.evaluate_metabolic_policy(),
        MetabolicAction::ReduceConcurrency { max_parallel: 2 }
    );
    assert!(matches!(
        runtime.hunger_degradation(),
        &DegradationLevel::Critical
    ));

    // Recovery: feed tokens (80000 / 100000 = 0.8 feed ratio)
    // Current hunger: 0.9, after feed: 0.9 - 0.8 = 0.1 (None)
    runtime.feed_tokens(80000);
    assert!(matches!(
        runtime.hunger_degradation(),
        &DegradationLevel::None
    ));

    // System still functional
    assert!(runtime.is_token_allowed("test"));
    runtime.mark_activity();
    assert_eq!(runtime.idle_state(), IdleState::Active);
}

#[tokio::test]
async fn test_system_ii_autonomic_reflex_matching() {
    let (mut runtime, _temp) = setup_runtime();

    // Check reflexes (no reflexes registered in default runtime)
    let actions = runtime.check_reflexes("test_trigger", 0.9).await.unwrap();
    assert_eq!(actions.len(), 0);
}

#[test]
fn test_cross_system_coordination() {
    let (mut runtime, _temp) = setup_runtime();

    // System IV affects System V indirectly through activity
    runtime.consume_tokens(30000).unwrap();
    runtime.mark_activity(); // Activity resets idle timer
    assert_eq!(runtime.idle_state(), IdleState::Active);

    // System III can trigger lockdown affecting all systems
    let shock = runtime.check_shock().unwrap();
    assert_eq!(shock, ShockSeverity::None); // No shock in same env

    // All systems continue to operate
    assert!(runtime.is_token_allowed("test")); // System I
    assert_eq!(runtime.remaining_budget(), 70000); // System IV
    assert_eq!(runtime.idle_state(), IdleState::Active); // System V
}
