//! Integration tests for System III: Hardware Proprioception
//!
//! Tests environment fingerprinting, transplantation shock detection,
//! and lockdown enforcement.

use zaion_proprioception::{
    EnvFingerprint, FingerprintCollector, LockdownState, ShockDetector, ShockSeverity,
};

#[test]
fn test_fingerprint_collector_initialization() {
    let collector = FingerprintCollector::new();
    assert!(std::ptr::addr_of!(collector).is_aligned());
}

#[test]
fn test_fingerprint_collection() {
    let collector = FingerprintCollector::new();
    let fingerprint = collector.collect().unwrap();

    assert!(!fingerprint.hostname.is_empty());
    assert!(!fingerprint.os_type.is_empty());
    assert!(!fingerprint.fingerprint_hash.is_empty());
    assert!(fingerprint.cpu_count > 0);
    assert!(fingerprint.total_memory > 0);
}

#[test]
fn test_fingerprint_hash_computation() {
    let fp = EnvFingerprint {
        hostname: "test-host".to_string(),
        os_type: "linux".to_string(),
        os_version: "5.10".to_string(),
        cpu_count: 4,
        total_memory: 8192,
        env_vars_hash: "abc123".to_string(),
        fingerprint_hash: String::new(),
        collected_at: chrono::Utc::now(),
    };

    let hash = fp.compute_hash();
    assert!(!hash.is_empty());
    assert_eq!(hash.len(), 64); // SHA-256 hex string
}

#[test]
fn test_identical_fingerprints_match() {
    let mut fp1 = EnvFingerprint {
        hostname: "host1".to_string(),
        os_type: "linux".to_string(),
        os_version: "5.10".to_string(),
        cpu_count: 4,
        total_memory: 8192,
        env_vars_hash: "abc123".to_string(),
        fingerprint_hash: String::new(),
        collected_at: chrono::Utc::now(),
    };

    let mut fp2 = fp1.clone();
    fp1.fingerprint_hash = fp1.compute_hash();
    fp2.fingerprint_hash = fp2.compute_hash();

    assert!(fp1.matches(&fp2));
    assert_eq!(fp1.similarity_score(&fp2), 1.0);
}

#[test]
fn test_different_fingerprints_dont_match() {
    let mut fp1 = EnvFingerprint {
        hostname: "host1".to_string(),
        os_type: "linux".to_string(),
        os_version: "5.10".to_string(),
        cpu_count: 4,
        total_memory: 8192,
        env_vars_hash: "abc123".to_string(),
        fingerprint_hash: String::new(),
        collected_at: chrono::Utc::now(),
    };

    let mut fp2 = EnvFingerprint {
        hostname: "host2".to_string(),
        os_type: "windows".to_string(),
        os_version: "10".to_string(),
        cpu_count: 8,
        total_memory: 16384,
        env_vars_hash: "def456".to_string(),
        fingerprint_hash: String::new(),
        collected_at: chrono::Utc::now(),
    };

    fp1.fingerprint_hash = fp1.compute_hash();
    fp2.fingerprint_hash = fp2.compute_hash();

    assert!(!fp1.matches(&fp2));
    assert!(fp1.similarity_score(&fp2) < 1.0);
}

#[test]
fn test_fingerprint_similarity_calculation() {
    let fp1 = EnvFingerprint {
        hostname: "host1".to_string(),
        os_type: "linux".to_string(),
        os_version: "5.10".to_string(),
        cpu_count: 4,
        total_memory: 8192,
        env_vars_hash: "abc123".to_string(),
        fingerprint_hash: "hash1".to_string(),
        collected_at: chrono::Utc::now(),
    };

    // Same except hostname
    let fp2 = EnvFingerprint {
        hostname: "host2".to_string(),
        os_type: "linux".to_string(),
        os_version: "5.10".to_string(),
        cpu_count: 4,
        total_memory: 8192,
        env_vars_hash: "abc123".to_string(),
        fingerprint_hash: "hash2".to_string(),
        collected_at: chrono::Utc::now(),
    };

    let similarity = fp1.similarity_score(&fp2);
    assert!(similarity > 0.5);
    assert!(similarity < 1.0);
}

#[test]
fn test_shock_detector_initialization() {
    let detector = ShockDetector::new();
    assert!(!detector.has_baseline());
}

#[test]
fn test_shock_detector_set_baseline() {
    let mut detector = ShockDetector::new();

    let fingerprint = EnvFingerprint {
        hostname: "host1".to_string(),
        os_type: "linux".to_string(),
        os_version: "5.10".to_string(),
        cpu_count: 4,
        total_memory: 8192,
        env_vars_hash: "abc123".to_string(),
        fingerprint_hash: "hash1".to_string(),
        collected_at: chrono::Utc::now(),
    };

    detector.set_baseline(fingerprint);
    assert!(detector.has_baseline());
}

#[test]
fn test_shock_detector_no_shock_on_identical() {
    let baseline = EnvFingerprint {
        hostname: "host1".to_string(),
        os_type: "linux".to_string(),
        os_version: "5.10".to_string(),
        cpu_count: 4,
        total_memory: 8192,
        env_vars_hash: "abc123".to_string(),
        fingerprint_hash: "hash1".to_string(),
        collected_at: chrono::Utc::now(),
    };

    let current = baseline.clone();
    let detector = ShockDetector::with_baseline(baseline);
    let shock = detector.detect(&current).unwrap();

    assert_eq!(shock.severity, ShockSeverity::None);
    assert_eq!(shock.differences.len(), 0);
    assert_eq!(shock.similarity_score, 1.0);
}

#[test]
fn test_shock_detector_mild_shock_hostname_change() {
    let baseline = EnvFingerprint {
        hostname: "host1".to_string(),
        os_type: "linux".to_string(),
        os_version: "5.10".to_string(),
        cpu_count: 4,
        total_memory: 8192,
        env_vars_hash: "abc123".to_string(),
        fingerprint_hash: "hash1".to_string(),
        collected_at: chrono::Utc::now(),
    };

    let current = EnvFingerprint {
        hostname: "host2".to_string(),
        ..baseline.clone()
    };

    let detector = ShockDetector::with_baseline(baseline);
    let shock = detector.detect(&current).unwrap();

    assert_eq!(shock.severity, ShockSeverity::Mild);
    assert!(!shock.differences.is_empty());
    assert!(shock.differences.iter().any(|d| d.contains("hostname")));
}

#[test]
fn test_shock_detector_severe_shock_major_changes() {
    let baseline = EnvFingerprint {
        hostname: "host1".to_string(),
        os_type: "linux".to_string(),
        os_version: "5.10".to_string(),
        cpu_count: 4,
        total_memory: 8192,
        env_vars_hash: "abc123".to_string(),
        fingerprint_hash: "hash1".to_string(),
        collected_at: chrono::Utc::now(),
    };

    let current = EnvFingerprint {
        hostname: "host2".to_string(),
        os_type: "windows".to_string(),
        os_version: "10".to_string(),
        cpu_count: 8,
        total_memory: 16384,
        env_vars_hash: "def456".to_string(),
        fingerprint_hash: "hash2".to_string(),
        collected_at: chrono::Utc::now(),
    };

    let detector = ShockDetector::with_baseline(baseline);
    let shock = detector.detect(&current).unwrap();

    assert_eq!(shock.severity, ShockSeverity::Severe);
    assert!(shock.differences.len() > 2);
    assert!(shock.similarity_score < 0.5);
}

#[test]
fn test_shock_detector_differences_tracking() {
    let baseline = EnvFingerprint {
        hostname: "host1".to_string(),
        os_type: "linux".to_string(),
        os_version: "5.10".to_string(),
        cpu_count: 4,
        total_memory: 8192,
        env_vars_hash: "abc123".to_string(),
        fingerprint_hash: "hash1".to_string(),
        collected_at: chrono::Utc::now(),
    };

    let current = EnvFingerprint {
        hostname: "host1".to_string(),
        os_type: "linux".to_string(),
        os_version: "5.15".to_string(), // Changed
        cpu_count: 4,
        total_memory: 8192,
        env_vars_hash: "abc123".to_string(),
        fingerprint_hash: "hash2".to_string(),
        collected_at: chrono::Utc::now(),
    };

    let detector = ShockDetector::with_baseline(baseline);
    let shock = detector.detect(&current).unwrap();

    assert!(shock.differences.iter().any(|d| d.contains("os_version")));
}

#[test]
fn test_lockdown_state_initialization() {
    let state = LockdownState::new();
    assert!(!state.is_locked());
    assert_eq!(state.severity, ShockSeverity::None);
}

#[test]
fn test_lockdown_engage() {
    let mut state = LockdownState::new();
    state.engage(ShockSeverity::Moderate, "Test lockdown".to_string());

    assert!(state.is_locked());
    assert_eq!(state.severity, ShockSeverity::Moderate);
    assert_eq!(state.reason, "Test lockdown");
    assert!(state.locked_at.is_some());
}

#[test]
fn test_lockdown_disengage() {
    let mut state = LockdownState::new();
    state.engage(ShockSeverity::Severe, "Severe shock".to_string());
    assert!(state.is_locked());

    state.disengage();
    assert!(!state.is_locked());
    assert_eq!(state.severity, ShockSeverity::None);
    assert!(state.locked_at.is_none());
}

#[test]
fn test_lockdown_token_unlock_requires_token() {
    let mut state = LockdownState::new();
    state.engage(ShockSeverity::Severe, "Severe shock".to_string());

    // No token set - should reject
    let result = state.disengage_with_token("any-token");
    assert!(result.is_err());
    assert!(state.is_locked());
}

#[test]
fn test_lockdown_token_unlock_rejects_wrong_token() {
    let mut state = LockdownState::new();
    state.engage(ShockSeverity::Severe, "Severe shock".to_string());
    state.unlock_token = Some("correct-token".to_string());

    let result = state.disengage_with_token("wrong-token");
    assert!(result.is_err());
    assert!(state.is_locked());
}

#[test]
fn test_lockdown_token_unlock_accepts_correct_token() {
    let mut state = LockdownState::new();
    state.engage(ShockSeverity::Severe, "Severe shock".to_string());
    state.unlock_token = Some("correct-token".to_string());

    let result = state.disengage_with_token("correct-token");
    assert!(result.is_ok());
    assert!(!state.is_locked());
}

#[test]
fn test_lockdown_severity_escalation() {
    let mut state = LockdownState::new();

    state.engage(ShockSeverity::Moderate, "Moderate shock".to_string());
    assert_eq!(state.severity, ShockSeverity::Moderate);

    state.engage(ShockSeverity::Severe, "Severe shock".to_string());
    assert_eq!(state.severity, ShockSeverity::Severe);
    assert_eq!(state.reason, "Severe shock");
}

#[test]
fn test_lockdown_no_downgrade() {
    let mut state = LockdownState::new();

    state.engage(ShockSeverity::Severe, "Severe shock".to_string());
    assert_eq!(state.severity, ShockSeverity::Severe);

    // Try to downgrade - should be ignored
    state.engage(ShockSeverity::Moderate, "Moderate shock".to_string());
    assert_eq!(state.severity, ShockSeverity::Severe);
    assert_eq!(state.reason, "Severe shock");
}

#[test]
fn test_lockdown_summary() {
    let mut state = LockdownState::new();

    let unlocked_summary = state.summary();
    assert!(unlocked_summary.contains("UNLOCKED"));

    state.engage(ShockSeverity::Moderate, "Test reason".to_string());
    let locked_summary = state.summary();
    assert!(locked_summary.contains("LOCKED"));
    assert!(locked_summary.contains("Moderate"));
    assert!(locked_summary.contains("Test reason"));
}

#[test]
fn test_global_lockdown_singleton() {
    use zaion_proprioception::global_lockdown;

    let lockdown1 = global_lockdown();
    let lockdown2 = global_lockdown();

    // Both should point to same Arc
    assert!(std::sync::Arc::ptr_eq(&lockdown1, &lockdown2));
}

#[test]
fn test_end_to_end_proprioception_workflow() {
    // 1. Collect baseline fingerprint
    let collector = FingerprintCollector::new();
    let baseline = collector.collect().unwrap();
    assert!(!baseline.hostname.is_empty());

    // 2. Create shock detector with baseline
    let detector = ShockDetector::with_baseline(baseline.clone());
    assert!(detector.has_baseline());

    // 3. Simulate current environment (same as baseline)
    let current = baseline.clone();
    let shock = detector.detect(&current).unwrap();
    assert_eq!(shock.severity, ShockSeverity::None);

    // 4. Create lockdown state
    let mut lockdown = LockdownState::new();
    assert!(!lockdown.is_locked());

    // 5. Simulate moderate shock
    let modified_current = EnvFingerprint {
        hostname: "different-host".to_string(),
        os_version: "different-version".to_string(),
        ..baseline.clone()
    };

    let shock = detector.detect(&modified_current).unwrap();
    if matches!(
        shock.severity,
        ShockSeverity::Moderate | ShockSeverity::Severe
    ) {
        lockdown.engage(shock.severity, "Transplantation detected".to_string());
        assert!(lockdown.is_locked());
    }

    // 6. Unlock with token
    lockdown.unlock_token = Some("unlock-code-123".to_string());
    let result = lockdown.disengage_with_token("unlock-code-123");
    assert!(result.is_ok());
    assert!(!lockdown.is_locked());
}
