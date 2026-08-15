//! Ego integration layer — connects EgoManifest to streaming response pipeline
//!
//! Responsibilities:
//! - Load ego.toml and verify Soul_Hash signature
//! - Compile ego.toml to XML system prompt
//! - Filter streaming tokens through DynamicLexicalBaffle
//! - Log ego mutations to ledger
use crate::RuntimeError;
use std::path::Path;
use zaion_crypto::keypair::ZaionKeypair;
use zaion_ego::{DynamicLexicalBaffle, EgoCompiler, EgoManifest, EgoStore, SoulHash};
use zaion_ledger::EventLedger;
use zaion_types::session::NamespaceKey;

pub struct EgoIntegration {
    manifest: EgoManifest,
    baffle: DynamicLexicalBaffle,
    ledger: EventLedger,
    keypair: ZaionKeypair,
    namespace_key: NamespaceKey,
}

impl EgoIntegration {
    /// Load ego.toml, verify signature, initialize baffle
    pub fn new(
        zaion_dir: impl AsRef<Path>,
        ledger: EventLedger,
        keypair: ZaionKeypair,
        namespace_key: NamespaceKey,
    ) -> Result<Self, RuntimeError> {
        let store = EgoStore::new(&zaion_dir);

        // Load manifest (or use default if not found)
        let manifest = if store.exists() {
            store
                .load()
                .map_err(|e| RuntimeError::Internal(format!("ego load failed: {}", e)))?
        } else {
            EgoManifest::default()
        };

        // Initialize baffle
        let baffle = DynamicLexicalBaffle::new(&manifest)
            .map_err(|e| RuntimeError::Internal(format!("baffle init failed: {}", e)))?;

        Ok(Self {
            manifest,
            baffle,
            ledger,
            keypair,
            namespace_key,
        })
    }

    /// Get compiled XML system prompt for LLM
    pub fn system_prompt(&self) -> String {
        EgoCompiler::compile(&self.manifest)
    }

    /// Filter a token through lexical baffle. Returns true if token is allowed.
    pub fn is_token_allowed(&self, token: &str) -> bool {
        self.baffle.is_allowed(token)
    }

    /// Filter complete response text
    pub fn filter_response(&self, response: &str) -> String {
        self.baffle.filter_response(response)
    }

    /// Get ego manifest (for inspection)
    pub fn manifest(&self) -> &EgoManifest {
        &self.manifest
    }

    /// Update ego.toml and log mutation to ledger
    pub fn update_manifest(
        &mut self,
        new_manifest: EgoManifest,
        zaion_dir: impl AsRef<Path>,
    ) -> Result<(), RuntimeError> {
        let store = EgoStore::new(&zaion_dir);
        store
            .save(&new_manifest)
            .map_err(|e| RuntimeError::Internal(format!("ego save failed: {}", e)))?;

        // Compute and sign new Soul_Hash
        let soul_hash = SoulHash::compute(&new_manifest, &self.keypair)
            .map_err(|e| RuntimeError::Internal(format!("soul hash failed: {}", e)))?;

        // Log mutation to ledger
        let payload = serde_json::json!({
            "soul_name": new_manifest.soul.name,
            "core_tone": new_manifest.soul.core_tone,
            "manifest_hash": soul_hash.manifest_hash,
            "signature": soul_hash.signature_hex,
        });

        self.ledger
            .append_signed_event(
                &self.keypair,
                &self.namespace_key,
                "ego.manifest_mutated",
                payload,
                None,
            )
            .map_err(RuntimeError::Ledger)?;

        // Update internal state
        self.manifest = new_manifest;
        self.baffle = DynamicLexicalBaffle::new(&self.manifest)
            .map_err(|e| RuntimeError::Internal(format!("baffle reinit failed: {}", e)))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn ego_integration_loads_default() {
        let dir = tempdir().unwrap();
        let ledger = EventLedger::new(dir.path().join("ledger.db"));
        let kp = zaion_crypto::keypair::ZaionKeypair::generate();
        let ns = zaion_types::session::NamespaceKey("test".to_string());

        let ego = EgoIntegration::new(dir.path(), ledger, kp, ns).unwrap();
        assert_eq!(ego.manifest().soul.name, "Zaion");
    }

    #[test]
    fn ego_system_prompt_contains_xml() {
        let dir = tempdir().unwrap();
        let ledger = EventLedger::new(dir.path().join("ledger.db"));
        let kp = zaion_crypto::keypair::ZaionKeypair::generate();
        let ns = zaion_types::session::NamespaceKey("test".to_string());

        let ego = EgoIntegration::new(dir.path(), ledger, kp, ns).unwrap();
        let prompt = ego.system_prompt();
        assert!(prompt.contains("<Zaion_Protocol>"));
        assert!(prompt.contains("</Zaion_Protocol>"));
    }

    #[test]
    fn ego_filters_banned_tokens() {
        let dir = tempdir().unwrap();
        let ledger = EventLedger::new(dir.path().join("ledger.db"));
        let kp = zaion_crypto::keypair::ZaionKeypair::generate();
        let ns = zaion_types::session::NamespaceKey("test".to_string());

        let ego = EgoIntegration::new(dir.path(), ledger, kp, ns).unwrap();
        assert!(!ego.is_token_allowed("作为一名AI"));
        assert!(ego.is_token_allowed("你好"));
    }
}
