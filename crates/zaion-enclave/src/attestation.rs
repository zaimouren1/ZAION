//! AttestationReport — 可验证的飞地状态证明
//!
//! 真实 TEE：Intel SGX DCAP / AMD SEV / ARM TrustZone
//! 软件模拟：Ed25519 签名 + SHA-256 度量值

use crate::{EnclaveError, EnclaveIdentity};
use serde::{Deserialize, Serialize};
use zaion_types::identity::{PublicKeyBytes, SignatureBytes};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationReport {
    /// Enclave unique ID (derived from sealing key)
    pub enclave_id: String,
    /// Principal ID (public key fingerprint)
    pub principal_id: String,
    /// Measurement: SHA-256 of software version string (simulates MRENCLAVE)
    pub measurement_hex: String,
    /// User data bound into the report (e.g. nonce from verifier)
    pub user_data: String,
    /// RFC-3339 timestamp
    pub generated_at: String,
    /// Ed25519 signature over the canonical report body
    pub signature_hex: String,
    /// TEE type identifier
    pub tee_type: String,
}

impl AttestationReport {
    /// Generate a signed attestation report bound to `user_data` and `software_version`.
    pub fn generate(identity: &EnclaveIdentity, user_data: &str, software_version: &str) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        let measurement = Self::compute_measurement(software_version);
        let measurement_hex = hex::encode(measurement);
        let body = Self::canonical_body(
            &identity.enclave_id(),
            &identity.principal_id(),
            &measurement_hex,
            user_data,
            &now,
        );
        let sig = identity.keypair.sign(body.as_bytes());
        AttestationReport {
            enclave_id: identity.enclave_id(),
            principal_id: identity.principal_id(),
            measurement_hex,
            user_data: user_data.to_string(),
            generated_at: now,
            signature_hex: hex::encode(&sig.0),
            tee_type: "software-simulation".to_string(),
        }
    }

    /// SHA-256 of the software version string, prefixed to prevent collision.
    fn compute_measurement(software_version: &str) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(b"zaion-enclave-measurement-v1:");
        h.update(software_version.as_bytes());
        let r = h.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&r);
        out
    }

    /// Canonical pipe-delimited body used for signing and verification.
    pub(crate) fn canonical_body(
        enclave_id: &str,
        principal_id: &str,
        measurement_hex: &str,
        user_data: &str,
        generated_at: &str,
    ) -> String {
        format!(
            "{}|{}|{}|{}|{}",
            enclave_id, principal_id, measurement_hex, user_data, generated_at
        )
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

/// Stateless verifier for attestation reports.
pub struct AttestationVerifier;

impl AttestationVerifier {
    /// Verify that the report was signed by `identity` and the enclave_id matches.
    pub fn verify(
        report: &AttestationReport,
        identity: &EnclaveIdentity,
    ) -> Result<(), EnclaveError> {
        if report.enclave_id != identity.enclave_id() {
            return Err(EnclaveError::AttestationInvalid(
                "enclave_id mismatch".into(),
            ));
        }
        let body = AttestationReport::canonical_body(
            &report.enclave_id,
            &report.principal_id,
            &report.measurement_hex,
            &report.user_data,
            &report.generated_at,
        );
        let sig_bytes = hex::decode(&report.signature_hex)
            .map_err(|_| EnclaveError::AttestationInvalid("invalid signature hex".into()))?;

        let pub_key = PublicKeyBytes(identity.keypair.public_key_bytes().0);
        let sig = SignatureBytes(sig_bytes);
        zaion_crypto::verify_signature(&pub_key, body.as_bytes(), &sig)
            .map_err(|e| EnclaveError::AttestationInvalid(format!("signature failed: {e}")))?;
        Ok(())
    }
}
