//! System III: Hardware Proprioception
//!
//! Environment fingerprinting and transplantation shock detection
mod fingerprint;
pub mod lockdown;
mod shock;

pub use fingerprint::{EnvFingerprint, FingerprintCollector};
pub use lockdown::{global_lockdown, LockdownState};
pub use shock::{ShockDetector, ShockSeverity, TransplantationShock};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProprioceptionError {
    #[error("fingerprint mismatch: {0}")]
    FingerprintMismatch(String),
    #[error("shock detected: {0}")]
    ShockDetected(String),
    #[error("environment read failed: {0}")]
    EnvironmentReadFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proprioception_system_loads() {
        let collector = FingerprintCollector::new();
        assert!(std::ptr::addr_of!(collector).is_aligned());
    }
}
