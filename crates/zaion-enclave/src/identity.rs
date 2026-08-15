//! EnclaveIdentity — 飞地唯一身份
//!
//! 基于 Ed25519 keypair 衍生出两个用途：
//!   1. signing_key  — 用于签名 AttestationReport
//!   2. sealing_key  — 用于 AES-256-GCM 加密密封存储（SHA-256 衍生）

use serde::{Deserialize, Serialize};
use zaion_crypto::keypair::ZaionKeypair;

#[derive(Clone)]
pub struct EnclaveIdentity {
    pub keypair: ZaionKeypair,
    /// 32-byte sealing key derived from the keypair public key via SHA-256
    pub sealing_key: [u8; 32],
}

impl EnclaveIdentity {
    /// Generate a fresh enclave identity.
    #[cfg(test)]
    pub fn generate() -> Self {
        let keypair = ZaionKeypair::generate();
        let sealing_key = derive_sealing_key(&keypair);
        Self {
            keypair,
            sealing_key,
        }
    }

    /// Load from an existing keypair (for persistence / deterministic re-creation).
    pub fn from_keypair(keypair: ZaionKeypair) -> Self {
        let sealing_key = derive_sealing_key(&keypair);
        Self {
            keypair,
            sealing_key,
        }
    }

    pub fn principal_id(&self) -> String {
        self.keypair.principal_id().as_str().to_string()
    }

    /// Enclave ID = first 8 bytes of sealing key, hex-encoded (16 chars).
    pub fn enclave_id(&self) -> String {
        hex::encode(&self.sealing_key[..8])
    }
}

fn derive_sealing_key(keypair: &ZaionKeypair) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let pub_bytes = keypair.public_key_bytes();
    let mut hasher = Sha256::new();
    hasher.update(b"zaion-enclave-sealing-key-v1:");
    hasher.update(&pub_bytes.0);
    let result = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&result[..32]);
    key
}

/// Lightweight serialisable record — used for audit/logs, not the full identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnclaveIdRecord {
    pub enclave_id: String,
    pub principal_id: String,
    pub created_at: String,
}
