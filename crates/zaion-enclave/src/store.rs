//! EnclaveStore — 持久化密封密钥存储

use crate::{EnclaveError, SealedSecret};
use std::path::{Path, PathBuf};

pub struct EnclaveStore {
    dir: PathBuf,
}

impl EnclaveStore {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
        }
    }

    fn secrets_path(&self) -> PathBuf {
        self.dir.join("enclave_secrets.json")
    }

    /// Persist a sealed secret. Overwrites any existing entry with the same label.
    pub fn save_secret(&self, secret: &SealedSecret) -> Result<(), EnclaveError> {
        std::fs::create_dir_all(&self.dir)?;
        let mut secrets = self.load_all_secrets().unwrap_or_default();
        // Replace-or-append: remove any existing entry with the same label first.
        secrets.retain(|s: &SealedSecret| s.label != secret.label);
        secrets.push(secret.clone());
        let json = serde_json::to_string_pretty(&secrets)?;
        std::fs::write(self.secrets_path(), json)?;
        Ok(())
    }

    /// Load a single secret by label, or `None` if not found.
    pub fn load_secret(&self, label: &str) -> Option<SealedSecret> {
        self.load_all_secrets()
            .ok()?
            .into_iter()
            .find(|s| s.label == label)
    }

    /// Load all sealed secrets from disk.
    pub fn load_all_secrets(&self) -> Result<Vec<SealedSecret>, EnclaveError> {
        let path = self.secrets_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let json = std::fs::read_to_string(&path)?;
        serde_json::from_str(&json).map_err(EnclaveError::Serialization)
    }

    /// Remove a sealed secret by label.
    pub fn delete_secret(&self, label: &str) -> Result<(), EnclaveError> {
        let mut secrets = self.load_all_secrets().unwrap_or_default();
        secrets.retain(|s| s.label != label);
        let json = serde_json::to_string_pretty(&secrets)?;
        std::fs::write(self.secrets_path(), json)?;
        Ok(())
    }
}
