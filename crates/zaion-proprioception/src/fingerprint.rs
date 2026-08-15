//! Environment Fingerprinting
//!
//! Collects hardware and environment characteristics for identity verification
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvFingerprint {
    pub hostname: String,
    pub os_type: String,
    pub os_version: String,
    pub cpu_count: usize,
    pub total_memory: u64,
    pub env_vars_hash: String,
    pub fingerprint_hash: String,
    pub collected_at: chrono::DateTime<chrono::Utc>,
}

impl EnvFingerprint {
    pub fn compute_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.hostname.as_bytes());
        hasher.update(self.os_type.as_bytes());
        hasher.update(self.os_version.as_bytes());
        hasher.update(self.cpu_count.to_le_bytes());
        hasher.update(self.total_memory.to_le_bytes());
        hasher.update(self.env_vars_hash.as_bytes());
        hex::encode(hasher.finalize())
    }

    pub fn matches(&self, other: &EnvFingerprint) -> bool {
        self.fingerprint_hash == other.fingerprint_hash
    }

    pub fn similarity_score(&self, other: &EnvFingerprint) -> f64 {
        let mut matches = 0;
        let mut total = 0;

        // Hostname match
        total += 1;
        if self.hostname == other.hostname {
            matches += 1;
        }

        // OS type match
        total += 1;
        if self.os_type == other.os_type {
            matches += 1;
        }

        // CPU count match
        total += 1;
        if self.cpu_count == other.cpu_count {
            matches += 1;
        }

        // Memory match (within 10%)
        total += 1;
        let mem_diff = (self.total_memory as f64 - other.total_memory as f64).abs();
        let mem_avg = (self.total_memory + other.total_memory) as f64 / 2.0;
        if mem_diff / mem_avg < 0.1 {
            matches += 1;
        }

        matches as f64 / total as f64
    }
}

pub struct FingerprintCollector {
    env_vars_to_check: Vec<String>,
}

impl FingerprintCollector {
    pub fn new() -> Self {
        Self {
            env_vars_to_check: vec![
                "PATH".to_string(),
                "HOME".to_string(),
                "USER".to_string(),
                "SHELL".to_string(),
            ],
        }
    }

    pub fn collect(&self) -> Result<EnvFingerprint, crate::ProprioceptionError> {
        let hostname = hostname::get()
            .map_err(|e| {
                crate::ProprioceptionError::EnvironmentReadFailed(format!("hostname: {}", e))
            })?
            .to_string_lossy()
            .to_string();

        let os_type = std::env::consts::OS.to_string();
        let os_version = sys_info::os_release().unwrap_or_else(|_| "unknown".to_string());

        let cpu_count = num_cpus::get();
        let total_memory = sys_info::mem_info().map(|m| m.total * 1024).unwrap_or(0);

        let env_vars_hash = self.hash_env_vars();

        let mut fp = EnvFingerprint {
            hostname,
            os_type,
            os_version,
            cpu_count,
            total_memory,
            env_vars_hash,
            fingerprint_hash: String::new(),
            collected_at: chrono::Utc::now(),
        };

        fp.fingerprint_hash = fp.compute_hash();
        Ok(fp)
    }

    fn hash_env_vars(&self) -> String {
        let mut hasher = Sha256::new();
        let mut env_map: HashMap<String, String> = HashMap::new();

        for var in &self.env_vars_to_check {
            if let Ok(value) = std::env::var(var) {
                env_map.insert(var.clone(), value);
            }
        }

        let mut keys: Vec<_> = env_map.keys().collect();
        keys.sort();

        for key in keys {
            hasher.update(key.as_bytes());
            hasher.update(env_map[key].as_bytes());
        }

        hex::encode(hasher.finalize())
    }
}

impl Default for FingerprintCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_collector_initializes() {
        let collector = FingerprintCollector::new();
        assert!(!collector.env_vars_to_check.is_empty());
    }

    #[test]
    fn can_collect_fingerprint() {
        let collector = FingerprintCollector::new();
        let fp = collector.collect().unwrap();

        assert!(!fp.hostname.is_empty());
        assert!(!fp.os_type.is_empty());
        assert!(!fp.fingerprint_hash.is_empty());
        assert!(fp.cpu_count > 0);
    }

    #[test]
    fn identical_fingerprints_match() {
        let mut fp1 = EnvFingerprint {
            hostname: "test".to_string(),
            os_type: "linux".to_string(),
            os_version: "5.0".to_string(),
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
    fn different_fingerprints_dont_match() {
        let mut fp1 = EnvFingerprint {
            hostname: "test1".to_string(),
            os_type: "linux".to_string(),
            os_version: "5.0".to_string(),
            cpu_count: 4,
            total_memory: 8192,
            env_vars_hash: "abc123".to_string(),
            fingerprint_hash: String::new(),
            collected_at: chrono::Utc::now(),
        };

        let mut fp2 = EnvFingerprint {
            hostname: "test2".to_string(),
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
}
