//! zaion-enclave — Software TEE (Trusted Execution Environment)
//!
//! 架构层次：
//!   EnclaveIdentity   — 唯一飞地身份（Ed25519 keypair + 衍生密封密钥）
//!   SealedSecret      — AES-256-GCM 加密，绑定到 EnclaveIdentity
//!   AttestationReport — 可验证的飞地状态证明
//!   SecureContext     — 内存隔离计算上下文（软件模拟）
//!   EnclaveStore      — 持久化密封密钥

pub mod attestation;
pub mod context;
pub mod error;
pub mod identity;
pub mod sealed;
pub mod store;

pub use attestation::{AttestationReport, AttestationVerifier};
pub use context::SecureContext;
pub use error::EnclaveError;
pub use identity::EnclaveIdentity;
pub use sealed::{SealPayload, SealedSecret};
pub use store::EnclaveStore;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn seal_and_unseal_roundtrip() {
        let identity = EnclaveIdentity::generate();
        let data = serde_json::json!({"secret": "api_key_xyz", "value": 42});
        let sealed = SealedSecret::seal(&identity, "test-secret", data.clone()).unwrap();
        let payload = sealed.unseal(&identity).unwrap();
        assert_eq!(payload.data, data);
        assert_eq!(payload.label, "test-secret");
    }

    #[test]
    fn unseal_fails_with_wrong_identity() {
        let id1 = EnclaveIdentity::generate();
        let id2 = EnclaveIdentity::generate();
        let sealed = SealedSecret::seal(&id1, "secret", serde_json::json!("value")).unwrap();
        assert!(sealed.unseal(&id2).is_err());
    }

    #[test]
    fn attestation_report_verifies_correctly() {
        let identity = EnclaveIdentity::generate();
        let report = AttestationReport::generate(&identity, "nonce-12345", "0.1.0");
        AttestationVerifier::verify(&report, &identity).unwrap();
    }

    #[test]
    fn attestation_fails_with_wrong_identity() {
        let id1 = EnclaveIdentity::generate();
        let id2 = EnclaveIdentity::generate();
        let report = AttestationReport::generate(&id1, "nonce", "0.1.0");
        assert!(AttestationVerifier::verify(&report, &id2).is_err());
    }

    #[test]
    fn secure_context_executes_and_logs() {
        let identity = EnclaveIdentity::generate();
        let mut ctx = SecureContext::new(identity);
        let result = ctx.execute("task-001", serde_json::json!({"x": 1}), |input| {
            let x = input["x"].as_i64().unwrap_or(0);
            serde_json::json!({"result": x * 2})
        });
        assert_eq!(result.output["result"], 2);
        assert!(result.executed_in_enclave);
        assert_eq!(ctx.audit_log().len(), 2);
    }

    #[test]
    fn enclave_store_save_and_load() {
        let dir = tempdir().unwrap();
        let identity = EnclaveIdentity::generate();
        let store = EnclaveStore::new(dir.path());
        let secret =
            SealedSecret::seal(&identity, "api-key", serde_json::json!("sk-test-12345")).unwrap();
        store.save_secret(&secret).unwrap();
        let loaded = store.load_secret("api-key").unwrap();
        let payload = loaded.unseal(&identity).unwrap();
        assert_eq!(payload.data, serde_json::json!("sk-test-12345"));
    }

    #[test]
    fn tampered_ciphertext_fails_unseal() {
        let identity = EnclaveIdentity::generate();
        let mut sealed = SealedSecret::seal(&identity, "label", serde_json::json!("data")).unwrap();
        // Flip the first byte of the ciphertext to simulate tampering.
        let mut ct = hex::decode(&sealed.ciphertext_hex).unwrap();
        ct[0] ^= 0xFF;
        sealed.ciphertext_hex = hex::encode(&ct);
        assert!(sealed.unseal(&identity).is_err());
    }

    #[test]
    fn enclave_id_is_deterministic() {
        use zaion_crypto::keypair::ZaionKeypair;
        let kp = ZaionKeypair::generate();
        let id1 = EnclaveIdentity::from_keypair(kp.clone());
        let id2 = EnclaveIdentity::from_keypair(kp.clone());
        assert_eq!(id1.enclave_id(), id2.enclave_id());
    }

    #[test]
    fn store_overwrite_same_label() {
        let dir = tempdir().unwrap();
        let identity = EnclaveIdentity::generate();
        let store = EnclaveStore::new(dir.path());
        let s1 = SealedSecret::seal(&identity, "key", serde_json::json!("v1")).unwrap();
        let s2 = SealedSecret::seal(&identity, "key", serde_json::json!("v2")).unwrap();
        store.save_secret(&s1).unwrap();
        store.save_secret(&s2).unwrap();
        let secrets = store.load_all_secrets().unwrap();
        assert_eq!(
            secrets.iter().filter(|s| s.label == "key").count(),
            1,
            "duplicate labels must be collapsed"
        );
        let loaded = store.load_secret("key").unwrap().unseal(&identity).unwrap();
        assert_eq!(loaded.data, serde_json::json!("v2"));
    }
}
