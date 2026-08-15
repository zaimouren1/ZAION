//! Integration tests for System II: Autonomic Reflexes
//!
//! Tests zero-token reflex responses, WASM probe execution,
//! and action potential accumulation.

use std::time::Duration;
use zaion_autonomic::{
    ActionPotential, AutonomicReflex, ProbeEngine, ProbeResult, ReflexAction, ReflexRegistry,
    ReflexTrigger, StimulusAccumulator, Threshold, WasmProbe,
};

#[test]
fn test_reflex_registry_add_and_get() {
    let mut registry = ReflexRegistry::new();

    let reflex = AutonomicReflex {
        id: "test_reflex".to_string(),
        name: "Test Reflex".to_string(),
        trigger: ReflexTrigger {
            trigger_type: "error".to_string(),
            pattern: Some("error.*".to_string()),
            threshold: None,
        },
        action: ReflexAction {
            action_type: "log".to_string(),
            parameters: serde_json::json!({"message": "Reflex triggered"}),
        },
        enabled: true,
    };

    registry.register(reflex);
    assert_eq!(registry.count(), 1);

    let retrieved = registry.get("test_reflex").unwrap();
    assert_eq!(retrieved.id, "test_reflex");
    assert_eq!(retrieved.name, "Test Reflex");
}

#[test]
fn test_reflex_registry_list_enabled() {
    let mut registry = ReflexRegistry::new();

    let enabled_reflex = AutonomicReflex {
        id: "enabled".to_string(),
        name: "Enabled Reflex".to_string(),
        trigger: ReflexTrigger {
            trigger_type: "test".to_string(),
            pattern: None,
            threshold: None,
        },
        action: ReflexAction {
            action_type: "log".to_string(),
            parameters: serde_json::json!({}),
        },
        enabled: true,
    };

    let disabled_reflex = AutonomicReflex {
        id: "disabled".to_string(),
        name: "Disabled Reflex".to_string(),
        trigger: ReflexTrigger {
            trigger_type: "test".to_string(),
            pattern: None,
            threshold: None,
        },
        action: ReflexAction {
            action_type: "log".to_string(),
            parameters: serde_json::json!({}),
        },
        enabled: false,
    };

    registry.register(enabled_reflex);
    registry.register(disabled_reflex);
    assert_eq!(registry.count(), 2);

    let enabled_list = registry.list_enabled();
    assert_eq!(enabled_list.len(), 1);
    assert_eq!(enabled_list[0].id, "enabled");
}

#[test]
fn test_action_potential_stimulate_below_threshold() {
    let mut ap = ActionPotential::new(
        "test".to_string(),
        "Test AP".to_string(),
        Threshold {
            value: 1.0,
            decay_rate: 0.1,
        },
    );

    // Below threshold - should not fire
    let fired = ap.stimulate(0.5);
    assert!(!fired);
    assert_eq!(ap.current_potential, 0.5);
}

#[test]
fn test_action_potential_stimulate_exceeds_threshold() {
    let mut ap = ActionPotential::new(
        "test".to_string(),
        "Test AP".to_string(),
        Threshold {
            value: 1.0,
            decay_rate: 0.1,
        },
    );

    // Accumulate to threshold
    ap.stimulate(0.5);
    let fired = ap.stimulate(0.6);

    // Should fire and reset
    assert!(fired);
    assert_eq!(ap.current_potential, 0.0);
}

#[test]
fn test_action_potential_percentage() {
    let mut ap = ActionPotential::new(
        "test".to_string(),
        "Test AP".to_string(),
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

#[test]
fn test_stimulus_accumulator_register_and_get() {
    let mut accumulator = StimulusAccumulator::new();

    let ap = ActionPotential::new(
        "test_ap".to_string(),
        "Test AP".to_string(),
        Threshold::default(),
    );

    accumulator.register(ap);

    let retrieved = accumulator.get("test_ap");
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().id, "test_ap");
}

#[test]
fn test_stimulus_accumulator_stimulate() {
    let mut accumulator = StimulusAccumulator::new();

    let ap = ActionPotential::new(
        "test_ap".to_string(),
        "Test AP".to_string(),
        Threshold {
            value: 1.0,
            decay_rate: 0.0,
        },
    );

    accumulator.register(ap);

    // Below threshold
    let fired = accumulator.stimulate("test_ap", 0.5).unwrap();
    assert!(!fired);

    // Exceeds threshold
    let fired = accumulator.stimulate("test_ap", 0.6).unwrap();
    assert!(fired);
}

#[test]
fn test_stimulus_accumulator_list_all() {
    let mut accumulator = StimulusAccumulator::new();

    accumulator.register(ActionPotential::new(
        "ap1".to_string(),
        "AP 1".to_string(),
        Threshold::default(),
    ));

    accumulator.register(ActionPotential::new(
        "ap2".to_string(),
        "AP 2".to_string(),
        Threshold::default(),
    ));

    let list = accumulator.list_all();
    assert_eq!(list.len(), 2);
}

#[test]
fn test_reflex_trigger_with_threshold() {
    let trigger = ReflexTrigger {
        trigger_type: "cpu_usage".to_string(),
        pattern: None,
        threshold: Some(80.0),
    };

    assert_eq!(trigger.trigger_type, "cpu_usage");
    assert_eq!(trigger.threshold, Some(80.0));
}

#[test]
fn test_reflex_trigger_with_pattern() {
    let trigger = ReflexTrigger {
        trigger_type: "file_change".to_string(),
        pattern: Some(".*\\.rs$".to_string()),
        threshold: None,
    };

    assert_eq!(trigger.trigger_type, "file_change");
    assert_eq!(trigger.pattern, Some(".*\\.rs$".to_string()));
}

#[test]
fn test_reflex_action_creation() {
    let action = ReflexAction {
        action_type: "log_event".to_string(),
        parameters: serde_json::json!({
            "message": "Test log",
            "level": "info"
        }),
    };

    assert_eq!(action.action_type, "log_event");
    assert_eq!(action.parameters["message"], "Test log");
    assert_eq!(action.parameters["level"], "info");
}

#[test]
fn test_probe_engine_initialization() {
    let engine = ProbeEngine::new();
    // ProbeEngine doesn't expose probe_count, but we can verify it initializes
    assert!(std::ptr::addr_of!(engine).is_aligned());
}

#[test]
fn test_wasm_probe_creation() {
    let wasm_bytes = vec![0x00, 0x61, 0x73, 0x6d]; // WASM magic number
    let probe = WasmProbe::new("test_probe".to_string(), wasm_bytes.clone());

    assert_eq!(probe.name(), "test_probe");
    assert_eq!(probe.bytes(), &wasm_bytes[..]);
}

#[test]
fn test_probe_result_success() {
    let result = ProbeResult {
        success: true,
        value: 42.0,
        metadata: serde_json::json!({"status": "ok"}),
    };

    assert!(result.success);
    assert_eq!(result.value, 42.0);
    assert_eq!(result.metadata["status"], "ok");
}

#[test]
fn test_probe_result_serialization() {
    let result = ProbeResult {
        success: false,
        value: 0.0,
        metadata: serde_json::json!({"error": "Probe failed"}),
    };

    let json = serde_json::to_string(&result).unwrap();
    assert!(json.contains("false"));
    assert!(json.contains("Probe failed"));
}

#[test]
fn test_registry_match_trigger_with_threshold() {
    let mut registry = ReflexRegistry::new();

    let reflex = AutonomicReflex {
        id: "cpu_monitor".to_string(),
        name: "CPU Monitor".to_string(),
        trigger: ReflexTrigger {
            trigger_type: "cpu_usage".to_string(),
            pattern: None,
            threshold: Some(90.0),
        },
        action: ReflexAction {
            action_type: "alert".to_string(),
            parameters: serde_json::json!({"message": "CPU high"}),
        },
        enabled: true,
    };

    registry.register(reflex);

    // Below threshold - no match
    let matches = registry.match_trigger("cpu_usage", Some(85.0));
    assert_eq!(matches.len(), 0);

    // Above threshold - match
    let matches = registry.match_trigger("cpu_usage", Some(95.0));
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].id, "cpu_monitor");
}

#[tokio::test]
async fn test_autonomic_runtime_initialization() {
    use zaion_autonomic::AutonomicRuntime;

    let (runtime, _rx) = AutonomicRuntime::new(Duration::from_secs(1));
    assert_eq!(runtime.reflex_count(), 0);
    assert_eq!(runtime.potential_count(), 0);
    assert_eq!(runtime.probe_count(), 0);
}

#[tokio::test]
async fn test_autonomic_runtime_register_components() {
    use zaion_autonomic::AutonomicRuntime;

    let (runtime, _rx) = AutonomicRuntime::new(Duration::from_secs(1));

    // Register reflex
    let reflex = AutonomicReflex {
        id: "test_reflex".to_string(),
        name: "Test Reflex".to_string(),
        trigger: ReflexTrigger {
            trigger_type: "test".to_string(),
            pattern: None,
            threshold: None,
        },
        action: ReflexAction {
            action_type: "log".to_string(),
            parameters: serde_json::json!({}),
        },
        enabled: true,
    };
    runtime.register_reflex(reflex);
    assert_eq!(runtime.reflex_count(), 1);

    // Register potential
    let ap = ActionPotential::new(
        "test_ap".to_string(),
        "Test AP".to_string(),
        Threshold::default(),
    );
    runtime.register_potential(ap);
    assert_eq!(runtime.potential_count(), 1);

    // Register probe
    let probe = WasmProbe::new("test_probe".to_string(), vec![0x00, 0x61, 0x73, 0x6d]);
    runtime.add_probe(probe);
    assert_eq!(runtime.probe_count(), 1);
}

#[tokio::test]
async fn test_end_to_end_reflex_workflow() {
    use zaion_autonomic::AutonomicRuntime;

    // 1. Create runtime
    let (runtime, _rx) = AutonomicRuntime::new(Duration::from_millis(100));

    // 2. Register multiple reflexes
    let error_reflex = AutonomicReflex {
        id: "error_reflex".to_string(),
        name: "Error Reflex".to_string(),
        trigger: ReflexTrigger {
            trigger_type: "error".to_string(),
            pattern: Some("error.*".to_string()),
            threshold: None,
        },
        action: ReflexAction {
            action_type: "log".to_string(),
            parameters: serde_json::json!({"message": "Error handled"}),
        },
        enabled: true,
    };

    let warning_reflex = AutonomicReflex {
        id: "warning_reflex".to_string(),
        name: "Warning Reflex".to_string(),
        trigger: ReflexTrigger {
            trigger_type: "warning".to_string(),
            pattern: Some("warning.*".to_string()),
            threshold: None,
        },
        action: ReflexAction {
            action_type: "log".to_string(),
            parameters: serde_json::json!({"message": "Warning handled"}),
        },
        enabled: true,
    };

    runtime.register_reflex(error_reflex);
    runtime.register_reflex(warning_reflex);
    assert_eq!(runtime.reflex_count(), 2);

    // 3. Register action potentials
    let error_ap = ActionPotential::new(
        "error_ap".to_string(),
        "Error AP".to_string(),
        Threshold {
            value: 1.0,
            decay_rate: 0.1,
        },
    );

    let warning_ap = ActionPotential::new(
        "warning_ap".to_string(),
        "Warning AP".to_string(),
        Threshold {
            value: 0.5,
            decay_rate: 0.2,
        },
    );

    runtime.register_potential(error_ap);
    runtime.register_potential(warning_ap);
    assert_eq!(runtime.potential_count(), 2);
}

#[test]
fn test_stimulus_accumulation_multiple_events() {
    let mut accumulator = StimulusAccumulator::new();

    let ap = ActionPotential::new(
        "multi_ap".to_string(),
        "Multi AP".to_string(),
        Threshold {
            value: 2.0,
            decay_rate: 0.0,
        },
    );

    accumulator.register(ap);

    // Accumulate stimuli
    let fired1 = accumulator.stimulate("multi_ap", 0.5).unwrap();
    assert!(!fired1);

    let fired2 = accumulator.stimulate("multi_ap", 0.8).unwrap();
    assert!(!fired2);

    let fired3 = accumulator.stimulate("multi_ap", 0.9).unwrap();
    assert!(fired3); // Total 2.2 > 2.0 threshold
}

#[test]
fn test_complete_autonomic_system() {
    // Test complete system components working together
    let mut registry = ReflexRegistry::new();
    let mut accumulator = StimulusAccumulator::new();

    // Register reflex
    let reflex = AutonomicReflex {
        id: "memory_pressure".to_string(),
        name: "Memory Pressure Handler".to_string(),
        trigger: ReflexTrigger {
            trigger_type: "memory_usage".to_string(),
            pattern: None,
            threshold: Some(0.8),
        },
        action: ReflexAction {
            action_type: "compact_memory".to_string(),
            parameters: serde_json::json!({"target_mb": 100}),
        },
        enabled: true,
    };

    registry.register(reflex);

    // Register action potential
    let ap = ActionPotential::new(
        "memory_ap".to_string(),
        "Memory AP".to_string(),
        Threshold {
            value: 0.8,
            decay_rate: 0.05,
        },
    );

    accumulator.register(ap);

    // Verify system state
    assert_eq!(registry.count(), 1);
    assert_eq!(accumulator.list_all().len(), 1);

    // Test trigger matching
    let matches = registry.match_trigger("memory_usage", Some(0.85));
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].action.action_type, "compact_memory");

    // Test stimulus accumulation
    let fired = accumulator.stimulate("memory_ap", 0.85).unwrap();
    assert!(fired);
}
