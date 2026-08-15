//! Transplantation Shock Detection
//!
//! Detects when a process has been moved to a different environment
use crate::fingerprint::EnvFingerprint;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ShockSeverity {
    None,
    Mild,     // Minor differences (e.g., different hostname)
    Moderate, // Significant differences (e.g., different OS version)
    Severe,   // Major differences (e.g., different OS type, CPU count)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransplantationShock {
    pub severity: ShockSeverity,
    pub similarity_score: f64,
    pub differences: Vec<String>,
    pub detected_at: chrono::DateTime<chrono::Utc>,
}

pub struct ShockDetector {
    baseline: Option<EnvFingerprint>,
}

impl ShockDetector {
    pub fn new() -> Self {
        Self { baseline: None }
    }

    pub fn with_baseline(baseline: EnvFingerprint) -> Self {
        Self {
            baseline: Some(baseline),
        }
    }

    pub fn set_baseline(&mut self, baseline: EnvFingerprint) {
        self.baseline = Some(baseline);
    }

    pub fn detect(
        &self,
        current: &EnvFingerprint,
    ) -> Result<TransplantationShock, crate::ProprioceptionError> {
        let baseline = self.baseline.as_ref().ok_or_else(|| {
            crate::ProprioceptionError::ShockDetected("No baseline fingerprint set".to_string())
        })?;

        let similarity = baseline.similarity_score(current);
        let mut differences = Vec::new();

        // Check for differences
        if baseline.hostname != current.hostname {
            differences.push(format!(
                "hostname: {} -> {}",
                baseline.hostname, current.hostname
            ));
        }

        if baseline.os_type != current.os_type {
            differences.push(format!(
                "os_type: {} -> {}",
                baseline.os_type, current.os_type
            ));
        }

        if baseline.os_version != current.os_version {
            differences.push(format!(
                "os_version: {} -> {}",
                baseline.os_version, current.os_version
            ));
        }

        if baseline.cpu_count != current.cpu_count {
            differences.push(format!(
                "cpu_count: {} -> {}",
                baseline.cpu_count, current.cpu_count
            ));
        }

        let mem_diff_pct = ((baseline.total_memory as f64 - current.total_memory as f64).abs()
            / baseline.total_memory as f64)
            * 100.0;
        if mem_diff_pct > 10.0 {
            differences.push(format!(
                "memory: {} -> {} ({:.1}% change)",
                baseline.total_memory, current.total_memory, mem_diff_pct
            ));
        }

        if baseline.env_vars_hash != current.env_vars_hash {
            differences.push("env_vars: changed".to_string());
        }

        // Determine severity
        let severity = if differences.is_empty() {
            ShockSeverity::None
        } else if similarity >= 0.75 {
            ShockSeverity::Mild
        } else if similarity >= 0.5 {
            ShockSeverity::Moderate
        } else {
            ShockSeverity::Severe
        };

        Ok(TransplantationShock {
            severity,
            similarity_score: similarity,
            differences,
            detected_at: chrono::Utc::now(),
        })
    }

    pub fn has_baseline(&self) -> bool {
        self.baseline.is_some()
    }
}

impl Default for ShockDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_fingerprint(hostname: &str, os_type: &str, cpu_count: usize) -> EnvFingerprint {
        EnvFingerprint {
            hostname: hostname.to_string(),
            os_type: os_type.to_string(),
            os_version: "1.0".to_string(),
            cpu_count,
            total_memory: 8192,
            env_vars_hash: "test".to_string(),
            fingerprint_hash: "test".to_string(),
            collected_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn detector_starts_without_baseline() {
        let detector = ShockDetector::new();
        assert!(!detector.has_baseline());
    }

    #[test]
    fn can_set_baseline() {
        let mut detector = ShockDetector::new();
        let fp = create_test_fingerprint("host1", "linux", 4);
        detector.set_baseline(fp);
        assert!(detector.has_baseline());
    }

    #[test]
    fn identical_environment_no_shock() {
        let baseline = create_test_fingerprint("host1", "linux", 4);
        let current = baseline.clone();

        let detector = ShockDetector::with_baseline(baseline);
        let shock = detector.detect(&current).unwrap();

        assert_eq!(shock.severity, ShockSeverity::None);
        assert_eq!(shock.differences.len(), 0);
        assert_eq!(shock.similarity_score, 1.0);
    }

    #[test]
    fn hostname_change_mild_shock() {
        let baseline = create_test_fingerprint("host1", "linux", 4);
        let current = create_test_fingerprint("host2", "linux", 4);

        let detector = ShockDetector::with_baseline(baseline);
        let shock = detector.detect(&current).unwrap();

        assert_eq!(shock.severity, ShockSeverity::Mild);
        assert!(shock.differences.iter().any(|d| d.contains("hostname")));
    }

    #[test]
    fn os_change_severe_shock() {
        let baseline = create_test_fingerprint("host1", "linux", 4);
        let current = create_test_fingerprint("host2", "windows", 8);

        let detector = ShockDetector::with_baseline(baseline);
        let shock = detector.detect(&current).unwrap();

        assert_eq!(shock.severity, ShockSeverity::Severe);
        assert!(shock.differences.len() > 1);
        assert!(shock.similarity_score < 0.5);
    }
}
