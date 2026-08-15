//! Signed trajectories with Ed25519 cryptographic signatures
//!
//! Every trajectory is signed by the principal identity, providing:
//! - Authenticity: Verify trajectory came from specific principal
//! - Integrity: Detect any tampering with trajectory data
//! - Non-repudiation: Principal cannot deny creating trajectory

use anyhow::{Context, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::trajectory::Trajectory;

/// Ed25519 signature for a trajectory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectorySignature {
    /// Public key (verifying key) of the signer
    pub public_key: Vec<u8>,

    /// Ed25519 signature bytes
    pub signature: Vec<u8>,

    /// SHA-256 hash of the trajectory content
    pub content_hash: Vec<u8>,

    /// Timestamp when signature was created
    pub timestamp: i64,
}

/// A trajectory with cryptographic signature
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedTrajectory {
    /// The trajectory data
    pub trajectory: Trajectory,

    /// Cryptographic signature
    pub signature: TrajectorySignature,
}

impl SignedTrajectory {
    /// Sign a trajectory with a signing key
    pub fn sign(trajectory: Trajectory, signing_key: &SigningKey) -> Result<Self> {
        // Compute content hash
        let content = serde_json::to_vec(&trajectory)?;
        let mut hasher = Sha256::new();
        hasher.update(&content);
        let content_hash = hasher.finalize().to_vec();

        // Sign the content hash
        let signature_bytes = signing_key.sign(&content_hash);

        // Get public key
        let verifying_key = signing_key.verifying_key();
        let public_key = verifying_key.to_bytes().to_vec();

        let signature = TrajectorySignature {
            public_key,
            signature: signature_bytes.to_bytes().to_vec(),
            content_hash,
            timestamp: chrono::Utc::now().timestamp(),
        };

        Ok(Self {
            trajectory,
            signature,
        })
    }

    /// Verify the signature on this trajectory
    pub fn verify(&self) -> Result<bool> {
        // Reconstruct verifying key from public key bytes
        let public_key_bytes: [u8; 32] = self
            .signature
            .public_key
            .as_slice()
            .try_into()
            .context("Invalid public key length")?;
        let verifying_key =
            VerifyingKey::from_bytes(&public_key_bytes).context("Invalid public key")?;

        // Reconstruct signature
        let signature_bytes: [u8; 64] = self
            .signature
            .signature
            .as_slice()
            .try_into()
            .context("Invalid signature length")?;
        let signature = Signature::from_bytes(&signature_bytes);

        // Recompute content hash
        let content = serde_json::to_vec(&self.trajectory)?;
        let mut hasher = Sha256::new();
        hasher.update(&content);
        let computed_hash = hasher.finalize().to_vec();

        // Verify hash matches
        if computed_hash != self.signature.content_hash {
            return Ok(false);
        }

        // Verify signature
        match verifying_key.verify(&self.signature.content_hash, &signature) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// Get the principal ID (hex-encoded public key)
    pub fn principal_id(&self) -> String {
        hex::encode(&self.signature.public_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

    #[test]
    fn test_sign_and_verify() {
        let _csprng = OsRng;
        let signing_key = SigningKey::from_bytes(&rand::random::<[u8; 32]>());

        let trajectory = Trajectory::new("test-1".to_string(), "Test task".to_string());
        let signed = SignedTrajectory::sign(trajectory, &signing_key).unwrap();

        assert!(signed.verify().unwrap());
    }

    #[test]
    fn test_verify_fails_on_tampering() {
        let _csprng = OsRng;
        let signing_key = SigningKey::from_bytes(&rand::random::<[u8; 32]>());

        let trajectory = Trajectory::new("test-1".to_string(), "Test task".to_string());
        let mut signed = SignedTrajectory::sign(trajectory, &signing_key).unwrap();

        // Tamper with trajectory
        signed.trajectory.success = !signed.trajectory.success;

        // Verification should fail
        assert!(!signed.verify().unwrap());
    }

    #[test]
    fn test_principal_id() {
        let _csprng = OsRng;
        let signing_key = SigningKey::from_bytes(&rand::random::<[u8; 32]>());

        let trajectory = Trajectory::new("test-1".to_string(), "Test task".to_string());
        let signed = SignedTrajectory::sign(trajectory, &signing_key).unwrap();

        let principal_id = signed.principal_id();
        assert_eq!(principal_id.len(), 64); // 32 bytes = 64 hex chars
    }
}
