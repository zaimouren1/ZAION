//! SealedSecret — AES-256-GCM 密封存储
//!
//! 密封 = AES-256-GCM 加密，密钥来自 EnclaveIdentity::sealing_key
//! 任何其他飞地身份无法解封（硬件 TEE 语义的软件模拟）

use crate::{EnclaveError, EnclaveIdentity};
use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealPayload {
    pub label: String,
    pub data: serde_json::Value,
    pub principal_id: String,
    pub sealed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedSecret {
    /// AES-256-GCM nonce (12 bytes, hex-encoded)
    pub nonce_hex: String,
    /// Ciphertext + GCM auth tag (hex-encoded)
    pub ciphertext_hex: String,
    /// Human-readable label for lookup
    pub label: String,
    /// Enclave ID that sealed this secret
    pub enclave_id: String,
}

impl SealedSecret {
    /// Seal a payload using the enclave's sealing key.
    pub fn seal(
        identity: &EnclaveIdentity,
        label: &str,
        data: serde_json::Value,
    ) -> Result<Self, EnclaveError> {
        let payload = SealPayload {
            label: label.to_string(),
            data,
            principal_id: identity.principal_id(),
            sealed_at: chrono::Utc::now().to_rfc3339(),
        };
        let plaintext =
            serde_json::to_vec(&payload).map_err(|e| EnclaveError::SealFailed(e.to_string()))?;

        let key = Key::<Aes256Gcm>::from_slice(&identity.sealing_key);
        let cipher = Aes256Gcm::new(key);
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(&nonce, plaintext.as_ref())
            .map_err(|e| EnclaveError::SealFailed(e.to_string()))?;

        Ok(SealedSecret {
            nonce_hex: hex::encode(nonce),
            ciphertext_hex: hex::encode(&ciphertext),
            label: label.to_string(),
            enclave_id: identity.enclave_id(),
        })
    }

    /// Unseal using the same enclave identity. Fails if identity doesn't match
    /// or if the ciphertext has been tampered with.
    pub fn unseal(&self, identity: &EnclaveIdentity) -> Result<SealPayload, EnclaveError> {
        if self.enclave_id != identity.enclave_id() {
            return Err(EnclaveError::UnsealFailed);
        }
        let nonce_bytes = hex::decode(&self.nonce_hex).map_err(|_| EnclaveError::UnsealFailed)?;
        let ciphertext =
            hex::decode(&self.ciphertext_hex).map_err(|_| EnclaveError::UnsealFailed)?;

        let key = Key::<Aes256Gcm>::from_slice(&identity.sealing_key);
        let cipher = Aes256Gcm::new(key);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let plaintext = cipher
            .decrypt(nonce, ciphertext.as_ref())
            .map_err(|_| EnclaveError::UnsealFailed)?;

        serde_json::from_slice(&plaintext).map_err(|_| EnclaveError::UnsealFailed)
    }
}
